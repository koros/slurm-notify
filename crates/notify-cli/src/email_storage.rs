//! API-email durable frames and provider-scoped recovery, under the existing store owner.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PauseReason {
    Authentication,
    Policy,
    Credentials,
}
impl PauseReason {
    fn code(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Policy => "policy",
            Self::Credentials => "credentials",
        }
    }
    fn parse(s: &str) -> Result<Self, StorageError> {
        match s {
            "authentication" => Ok(Self::Authentication),
            "policy" => Ok(Self::Policy),
            "credentials" => Ok(Self::Credentials),
            _ => Err(StorageError::Conflict),
        }
    }
}
pub(super) fn pause(
    tx: &rusqlite::Transaction<'_>,
    cluster: &str,
    destination: &str,
    revision: &str,
    reason: PauseReason,
    now: i64,
) -> Result<(), StorageError> {
    if !public_alias(cluster)
        || !public_alias(destination)
        || !public_revision(revision)
        || !(0..=4_102_444_799).contains(&now)
    {
        return Err(StorageError::InvalidInput);
    }
    let bound:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM email_event_frames f JOIN public_event_routes r USING(internal_event_id,destination) JOIN events e ON e.event_id=r.internal_event_id JOIN outbox o ON o.event_id=e.event_id AND o.destination=r.destination WHERE e.cluster=?1 AND r.destination=?2 AND r.revision=?3 AND o.status='pending')",params![cluster,destination,revision],|r|r.get(0))?;
    if !bound {
        return Err(StorageError::Conflict);
    }
    let changed=tx.execute("INSERT INTO email_provider_pauses VALUES (?1,?2,?3,?4,?5) ON CONFLICT(cluster,destination) DO UPDATE SET reason=excluded.reason,paused_at=excluded.paused_at WHERE revision=excluded.revision",params![cluster,destination,revision,reason.code(),now])?;
    if changed != 1 {
        return Err(StorageError::Conflict);
    }
    Ok(())
}
impl Store {
    pub(crate) fn email_bound_revision(&self, item: &DueDelivery) -> Result<String, StorageError> {
        let revision=self.db.query_row("SELECT r.revision FROM public_event_routes r JOIN email_event_frames f USING(internal_event_id,destination) WHERE r.internal_event_id=?1 AND r.destination=?2",params![item.event_id,item.destination],|r|r.get::<_,String>(0)).optional()?.ok_or(StorageError::Conflict)?;
        if !public_revision(&revision) {
            return Err(StorageError::Conflict);
        }
        Ok(revision)
    }
    pub(crate) fn email_paused(&self, item: &DueDelivery, now: i64) -> Result<bool, StorageError> {
        let version: i64 = self
            .db
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 7 {
            return Ok(false);
        }
        let paused:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM email_provider_pauses WHERE cluster=?1 AND destination=?2)",params![item.event.identity.cluster,item.destination],|r|r.get(0))?;
        if paused || version < 8 {
            return Ok(paused);
        }
        self.db.query_row("SELECT EXISTS(SELECT 1 FROM email_provider_cooldowns WHERE cluster=?1 AND destination=?2 AND retry_at>?3)",params![item.event.identity.cluster,item.destination,now],|r|r.get(0)).map_err(StorageError::from)
    }

    pub(crate) fn defer_email_preflight(
        &mut self,
        item: &DueDelivery,
        retry_at: i64,
        now: i64,
    ) -> Result<(), StorageError> {
        if now < 0 || retry_at <= now || !(0..=4_102_444_799).contains(&retry_at) {
            return Err(StorageError::InvalidInput);
        }
        let revision = self.email_bound_revision(item)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 7 {
            tx.execute_batch(include_str!("../../notify-storage/migrations/0008.sql"))?;
        }
        let changed=tx.execute("INSERT INTO email_provider_cooldowns VALUES(?1,?2,?3,?4) ON CONFLICT(cluster,destination) DO UPDATE SET revision=excluded.revision,retry_at=MAX(retry_at,excluded.retry_at) WHERE revision=excluded.revision OR (retry_at<=?5 AND NOT EXISTS(SELECT 1 FROM public_event_routes r JOIN events e ON e.event_id=r.internal_event_id JOIN outbox o ON o.event_id=e.event_id AND o.destination=r.destination WHERE e.cluster=?1 AND r.destination=?2 AND o.status='pending' AND r.revision<>?3))",params![item.event.identity.cluster,item.destination,revision,retry_at,now])?;
        if changed != 1 {
            return Err(StorageError::Conflict);
        }
        tx.commit()?;
        Ok(())
    }
    // Revision guard and canonical-frame integrity must pass before materializing
    // the bounded credential-free provider request.
    pub(crate) fn email_delivery(
        &self,
        internal: &str,
        destination: &str,
        revision: &str,
        profile: &crate::email_profile::Profile,
    ) -> Result<crate::email_wire::Request, StorageError> {
        self.public_delivery(internal, destination, revision)?
            .ok_or(StorageError::Conflict)?;
        let version: i64 = self
            .db
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 7 {
            return Err(StorageError::Conflict);
        }
        let row=self.db.query_row("SELECT profile_reference,CASE WHEN length(CAST(content_type AS BLOB)) BETWEEN 1 AND 200 THEN content_type END,CASE WHEN typeof(payload)='blob' AND length(payload) BETWEEN 1 AND 16384 THEN payload END,CASE WHEN length(sha256)=32 THEN sha256 END FROM email_event_frames WHERE internal_event_id=?1 AND destination=?2",params![internal,destination],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<Vec<u8>>>(2)?,r.get::<_,Option<Vec<u8>>>(3)?))).optional()?.ok_or(StorageError::Conflict)?;
        let (reference, content_type, body, hash) = row;
        if reference != profile.revision() {
            return Err(StorageError::Conflict);
        }
        let request = crate::email_wire::Request {
            content_type: content_type.ok_or(StorageError::Conflict)?,
            body: zeroize::Zeroizing::new(body.ok_or(StorageError::Conflict)?),
        };
        if request.body.len() > profile.payload_limit
            || Some(request.fingerprint().as_slice()) != hash.as_deref()
        {
            return Err(StorageError::Conflict);
        }
        Ok(request)
    }
    pub(crate) fn pause_email_before_attempt(
        &mut self,
        cluster: &str,
        destination: &str,
        revision: &str,
        reason: PauseReason,
        now: i64,
    ) -> Result<(), StorageError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        pause(&tx, cluster, destination, revision, reason, now)?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn pause_email_after_attempt(
        &mut self,
        item: &DueDelivery,
        revision: &str,
        reason: PauseReason,
        now: i64,
        retry_at: i64,
    ) -> Result<(), StorageError> {
        self.record_delivery_using(
            item,
            DeliveryOutcome::TransientFailure,
            now,
            Some(retry_at),
            |tx| {
                pause(
                    tx,
                    &item.event.identity.cluster,
                    &item.destination,
                    revision,
                    reason,
                    now,
                )
            },
        )
    }
    pub(crate) fn email_pause(
        &self,
        cluster: &str,
        destination: &str,
        revision: &str,
    ) -> Result<Option<(PauseReason, i64)>, StorageError> {
        let row=self.db.query_row("SELECT revision,reason,paused_at FROM email_provider_pauses WHERE cluster=?1 AND destination=?2",params![cluster,destination],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?))).optional()?;
        let Some((bound, reason, at)) = row else {
            return Ok(None);
        };
        if bound != revision || !(0..=4_102_444_799).contains(&at) {
            return Err(StorageError::Conflict);
        }
        Ok(Some((PauseReason::parse(&reason)?, at)))
    }
    pub(crate) fn resume_email(
        &mut self,
        cluster: &str,
        destination: &str,
        revision: &str,
    ) -> Result<bool, StorageError> {
        if !public_alias(cluster) || !public_alias(destination) || !public_revision(revision) {
            return Err(StorageError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let bound: Option<String> = tx
            .query_row(
                "SELECT revision FROM email_provider_pauses WHERE cluster=?1 AND destination=?2",
                params![cluster, destination],
                |r| r.get(0),
            )
            .optional()?;
        let Some(bound) = bound else {
            return Ok(false);
        };
        if bound != revision {
            return Err(StorageError::Conflict);
        }
        let conflict:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM public_event_routes r JOIN events e ON e.event_id=r.internal_event_id JOIN outbox o ON o.event_id=e.event_id AND o.destination=r.destination WHERE e.cluster=?1 AND r.destination=?2 AND o.status='pending' AND r.revision<>?3)",params![cluster,destination,revision],|r|r.get(0))?;
        if conflict {
            return Err(StorageError::Conflict);
        }
        tx.execute(
            "DELETE FROM email_provider_pauses WHERE cluster=?1 AND destination=?2 AND revision=?3",
            params![cluster, destination, revision],
        )?;
        tx.commit()?;
        Ok(true)
    }
}

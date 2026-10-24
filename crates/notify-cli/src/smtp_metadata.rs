//! Private per-event metadata selection, frozen by atomic prepared polling.
use super::lifecycle::Event;
use ring::digest::{Context, SHA256};

pub(super) const REVISION_VERSION: &str = "sha256-event-route-revision-v2";
pub(super) const VERSION: &str = "sha256-event-route-v1";

pub(super) struct Selected {
    pub(super) identity: String,
    pub(super) timestamp: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Identity,
    Date,
}

pub(super) fn timestamp(observed_at: i64) -> Result<u64, Error> {
    if !(0..=4_102_444_799).contains(&observed_at) {
        return Err(Error::Date);
    }
    Ok(observed_at as u64)
}

#[cfg(test)]
pub(super) fn select(
    event: &Event,
    route: &str,
    namespace: &str,
    observed_at: i64,
) -> Result<Selected, Error> {
    select_fields(event, route, namespace, observed_at, None)
}

pub(super) fn select_revision(
    event: &Event,
    route: &str,
    namespace: &str,
    observed_at: i64,
    revision: &str,
) -> Result<Selected, Error> {
    if revision.len() != 64
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Identity);
    }
    select_fields(event, route, namespace, observed_at, Some(revision))
}

fn select_fields(
    event: &Event,
    route: &str,
    namespace: &str,
    observed_at: i64,
    revision: Option<&str>,
) -> Result<Selected, Error> {
    let timestamp = timestamp(observed_at)?;
    let fields = [
        event.identity.cluster.as_str(),
        event.identity.job.as_str(),
        event.identity.submit.as_str(),
        event.identity.start.as_str(),
        event.state.code(),
        route,
        namespace,
    ];
    if fields
        .iter()
        .enumerate()
        .any(|(index, field)| (field.is_empty() && index != 3) || field.len() > 4096)
    {
        return Err(Error::Identity);
    }
    let mut digest = Context::new(&SHA256);
    digest.update(if revision.is_some() {
        b"slurm-notify-private-smtp-id-v2\0"
    } else {
        b"slurm-notify-private-smtp-id-v1\0"
    });
    for field in fields {
        digest.update(&(field.len() as u64).to_be_bytes());
        digest.update(field.as_bytes());
    }
    if let Some(revision) = revision {
        digest.update(&(revision.len() as u64).to_be_bytes());
        digest.update(revision.as_bytes());
    }
    let digest = digest.finish();
    let mut identity = String::with_capacity(64);
    const HEX: &[u8] = b"0123456789abcdef";
    for byte in digest.as_ref() {
        identity.push(HEX[(byte >> 4) as usize] as char);
        identity.push(HEX[(byte & 15) as usize] as char);
    }
    Ok(Selected {
        identity,
        timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::{Identity, State};
    fn event() -> Event {
        Event {
            identity: Identity {
                cluster: "cluster".into(),
                job: "42_7".into(),
                submit: "submit-fixture".into(),
                start: "start-fixture".into(),
            },
            state: State::Completed,
        }
    }
    #[test]
    fn revision_identity_is_versioned_stable_and_validates_token() {
        let event = event();
        let token = "a".repeat(64);
        let selected = select_revision(&event, "smtp", "fixture", 10, &token).unwrap();
        assert_eq!(
            selected.identity,
            "0c8067a7b6681928a360bd5fb8507692bf4035e881b8f20a8a253333c85d2122"
        );
        assert_eq!(
            selected.identity,
            select_revision(&event, "smtp", "fixture", 20, &token)
                .unwrap()
                .identity
        );
        assert_ne!(
            selected.identity,
            select(&event, "smtp", "fixture", 10).unwrap().identity
        );
        assert_ne!(
            selected.identity,
            select_revision(&event, "smtp", "fixture", 10, &"b".repeat(64))
                .unwrap()
                .identity
        );
        for invalid in [
            "".to_owned(),
            "A".repeat(64),
            "a".repeat(63),
            "a".repeat(65),
            "g".repeat(64),
        ] {
            assert!(matches!(
                select_revision(&event, "smtp", "fixture", 10, &invalid),
                Err(Error::Identity)
            ));
        }
        assert!(matches!(
            select_revision(&event, "smtp", "fixture", -1, &token),
            Err(Error::Date)
        ));
    }
    #[test]
    fn identities_are_stable_and_distinguish_event_route_namespace_and_field_boundaries() {
        let event = event();
        let first = select(&event, "a", "namespace", 10).unwrap();
        assert_eq!(first.identity.len(), 64);
        assert_eq!(
            first.identity,
            "9cab5ab07f3339035ebeab42348622f78e17723a43f6db2f246f8f22566de1c0"
        );
        assert!(first.identity.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(
            first.identity,
            select(&event, "a", "namespace", 20).unwrap().identity
        );
        assert_eq!(first.timestamp, 10);
        assert_ne!(
            first.identity,
            select(&event, "b", "namespace", 10).unwrap().identity
        );
        assert_ne!(
            first.identity,
            select(&event, "a", "changed", 10).unwrap().identity
        );
        for field in 0..5 {
            let mut changed = event.clone();
            match field {
                0 => changed.identity.cluster.push('x'),
                1 => changed.identity.job.push('1'),
                2 => changed.identity.submit.push('x'),
                3 => changed.identity.start.push('x'),
                _ => changed.state = State::Failed,
            }
            assert_ne!(
                first.identity,
                select(&changed, "a", "namespace", 10).unwrap().identity
            );
        }
        let mut left = event.clone();
        left.identity.submit = "ab".into();
        left.identity.start = "c".into();
        let mut right = event;
        right.identity.submit = "a".into();
        right.identity.start = "bc".into();
        assert_ne!(
            select(&left, "a", "namespace", 10).unwrap().identity,
            select(&right, "a", "namespace", 10).unwrap().identity
        );
    }
    #[test]
    fn cancellation_without_start_context_is_distinct_and_supported() {
        let mut cancelled = event();
        cancelled.state = State::Cancelled;
        cancelled.identity.start.clear();
        let no_start = select(&cancelled, "a", "namespace", 10).unwrap();
        cancelled.identity.start = "actual-fixture".into();
        assert_ne!(
            no_start.identity,
            select(&cancelled, "a", "namespace", 10).unwrap().identity
        );
        cancelled.identity.submit.clear();
        assert!(matches!(
            select(&cancelled, "a", "namespace", 10),
            Err(Error::Identity)
        ));
    }

    #[test]
    fn observed_time_and_identity_work_are_bounded_with_static_errors() {
        for time in [-1, 4_102_444_800, i64::MAX] {
            assert!(matches!(
                select(&event(), "a", "namespace", time),
                Err(Error::Date)
            ));
        }
        assert_eq!(timestamp(0), Ok(0));
        assert_eq!(timestamp(4_102_444_799), Ok(4_102_444_799));
        assert!(matches!(
            select(&event(), "", "namespace", 10),
            Err(Error::Identity)
        ));
        assert!(matches!(
            select(&event(), "a", "", 10),
            Err(Error::Identity)
        ));
        let mut oversized = event();
        oversized.identity.submit = "private-fixture".repeat(4097);
        assert!(matches!(
            select(&oversized, "a", "namespace", 10),
            Err(Error::Identity)
        ));
        oversized.identity.submit = "x".repeat(4096);
        assert!(select(&oversized, "a", "namespace", 10).is_ok());
    }
}

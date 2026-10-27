use super::*;
#[allow(unused_imports)] // Custom process harness omits unit-test bodies.
use crate::storage::email::PauseReason;
fn profile(kind: &str) -> crate::email_profile::Profile {
    crate::email_profile::Profile::parse(
        &serde_json::to_vec(&crate::email_profile::tests::fixture(kind)).unwrap(),
    )
    .unwrap()
}
fn seed(
    store: &mut Store,
    profile: &crate::email_profile::Profile,
) -> Result<Vec<DueDelivery>, StorageError> {
    let mut candidate = crate::lifecycle::Reconciler::with_capacity(10);
    let events = candidate
        .observe_snapshot(
            "",
            "cluster|123_2|alice|s|a|COMPLETED|0:0",
            "cluster",
            "alice",
            true,
        )
        .unwrap();
    let revision = profile.revision();
    store.persist_public_webhooks(&candidate, &events, ("cluster", "window", 0), |_| {
        vec![
            PublicRoute {
                destination: "email",
                revision: &revision,
                slack: None,
                email: Some(profile),
            },
            PublicRoute {
                destination: "other",
                revision: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                slack: None,
                email: None,
            },
        ]
    })?;
    store.due_deliveries("cluster", 0, 10)
}
#[test]
fn email_frames_are_atomic_immutable_private_and_replayed_exactly() {
    for kind in ["sendgrid", "mailgun", "gmail_api"] {
        let d = tests::Directory::new();
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let p = profile(kind);
        let items = seed(&mut store, &p).unwrap();
        let item = items.iter().find(|i| i.destination == "email").unwrap();
        let frozen = store
            .email_delivery(&item.event_id, "email", &p.revision(), &p)
            .unwrap();
        assert!(store
            .db
            .execute("UPDATE email_event_frames SET content_type='changed'", [])
            .is_err());
        assert!(store
            .db
            .execute("DELETE FROM email_event_frames", [])
            .is_err());
        let body = frozen.body.to_vec();
        assert!(!String::from_utf8_lossy(&body).contains("API_KEY"));
        assert_eq!(
            store
                .db
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        drop(store);
        let store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        assert_eq!(
            store
                .email_delivery(&item.event_id, "email", &p.revision(), &p)
                .unwrap()
                .body
                .as_slice(),
            body
        );
        let mut changed = profile(kind);
        changed.recipient = zeroize::Zeroizing::new("changed@fixture.invalid".into());
        assert!(store
            .email_delivery(&item.event_id, "email", &p.revision(), &changed)
            .is_err());
        assert!(store
            .email_delivery(&item.event_id, "other", &p.revision(), &p)
            .is_err());
        store.db.execute_batch("DROP TRIGGER email_frame_no_update; UPDATE email_event_frames SET content_type='changed';").unwrap();
        assert!(store
            .email_delivery(&item.event_id, "email", &p.revision(), &p)
            .is_err());
    }
}
#[test]
fn preparation_and_late_schema_failure_roll_back_the_whole_poll() {
    for late in [false, true] {
        let d = tests::Directory::new();
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let mut p = profile("mailgun");
        if late {
            store
                .db
                .execute_batch("CREATE TABLE email_event_frames(blocker INTEGER);")
                .unwrap();
        } else {
            p.payload_limit = 1;
        }
        assert!(seed(&mut store, &p).is_err());
        assert_eq!(
            store
                .db
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            4
        );
        for table in ["jobs", "events", "outbox", "checkpoints"] {
            assert_eq!(
                store
                    .db
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }
}
#[test]
fn provider_pauses_persist_isolate_and_resume_without_resetting_attempts() {
    let d = tests::Directory::new();
    let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
    let p = profile("sendgrid");
    let items = seed(&mut store, &p).unwrap();
    let item = items.iter().find(|i| i.destination == "email").unwrap();
    for reason in [PauseReason::Credentials, PauseReason::Policy] {
        store
            .pause_email_before_attempt("cluster", "email", &p.revision(), reason, 0)
            .unwrap();
        assert_eq!(
            store
                .due_deliveries("cluster", 0, 10)
                .unwrap()
                .iter()
                .map(|i| i.destination.as_str())
                .collect::<Vec<_>>(),
            ["other"]
        );
        assert_eq!(
            store
                .email_pause("cluster", "email", &p.revision())
                .unwrap(),
            Some((reason, 0))
        );
        assert!(store
            .resume_email(
                "cluster",
                "email",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            )
            .is_err());
        assert!(store
            .resume_email("cluster", "email", &p.revision())
            .unwrap());
        assert!(!store
            .resume_email("cluster", "email", &p.revision())
            .unwrap());
        assert_eq!(
            store
                .due_deliveries("cluster", 0, 10)
                .unwrap()
                .iter()
                .find(|i| i.destination == "email")
                .unwrap()
                .attempt_number,
            1
        );
    }
    let wrong = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    assert!(store
        .pause_email_after_attempt(item, wrong, PauseReason::Authentication, 0, 10)
        .is_err());
    assert_eq!(
        store
            .db
            .query_row("SELECT COUNT(*) FROM delivery_attempts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    store
        .pause_email_after_attempt(item, &p.revision(), PauseReason::Authentication, 0, 10)
        .unwrap();
    assert_eq!(
        store
            .db
            .query_row("SELECT COUNT(*) FROM delivery_attempts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(store);
    let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
    assert_eq!(
        store
            .email_pause("cluster", "email", &p.revision())
            .unwrap(),
        Some((PauseReason::Authentication, 0))
    );
    assert!(store
        .due_deliveries("cluster", 10, 10)
        .unwrap()
        .iter()
        .all(|i| i.destination != "email"));
    assert!(store
        .resume_email("cluster", "email", &p.revision())
        .unwrap());
    assert_eq!(
        store
            .due_deliveries("cluster", 10, 10)
            .unwrap()
            .iter()
            .find(|i| i.destination == "email")
            .unwrap()
            .attempt_number,
        2
    );
    assert!(store
        .pause_email_before_attempt(
            "cluster",
            "other",
            &p.revision(),
            PauseReason::Credentials,
            10
        )
        .is_err());
    assert!(store
        .pause_email_before_attempt(
            "cluster",
            "email",
            &p.revision(),
            PauseReason::Credentials,
            -1
        )
        .is_err());
}

#[test]
fn worker_pauses_api_destinations_and_keeps_unrelated_delivery_live() {
    use crate::delivery::{deliver_due_checked, CheckedHooks, ProviderOutcome, RetryPolicy};
    for preflight in [true, false] {
        let d = tests::Directory::new();
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let p = profile("sendgrid");
        let items = seed(&mut store, &p).unwrap();
        let email = items.iter().find(|i| i.destination == "email").unwrap();
        let policy = RetryPolicy {
            max_attempts: 3,
            initial_delay_seconds: 5,
            max_delay_seconds: 30,
            jitter_seconds: 0,
            jitter_seed: 1,
        };
        let mut calls = Vec::new();
        let report = deliver_due_checked(
            &mut store,
            "cluster",
            0,
            10,
            policy,
            CheckedHooks {
                authorize: |_: &Store, item: &DueDelivery| {
                    if preflight && item.destination == "email" {
                        Err(crate::supervised_provider::Diagnostic::EmailCredentials)
                    } else {
                        Ok(())
                    }
                },
                send: |item: &DueDelivery, ()| {
                    calls.push(item.destination.clone());
                    Ok(if item.destination == "email" {
                        ProviderOutcome::PauseAuthentication
                    } else {
                        ProviderOutcome::Delivered
                    })
                },
                clock: || Some(1),
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(report.delivered, 1);
        assert_eq!(report.retrying, usize::from(!preflight));
        assert_eq!(calls.contains(&"email".into()), !preflight);
        assert!(store.due_deliveries("cluster", 100, 10).unwrap().is_empty());
        let attempts: i64 = store
            .db
            .query_row(
                "SELECT count(*) FROM delivery_attempts WHERE event_id=?1 AND destination='email'",
                [&email.event_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(attempts, i64::from(!preflight));
        drop(store);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        assert!(store
            .email_pause("cluster", "email", &p.revision())
            .unwrap()
            .is_some());
        assert!(store
            .resume_email("cluster", "email", &p.revision())
            .unwrap());
        let pending = store.due_deliveries("cluster", 100, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].attempt_number, if preflight { 1 } else { 2 });
    }
}

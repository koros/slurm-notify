//! Private synchronous fixture preparation; no production renderer or poll defaults.
use super::{
    lifecycle::{Event, Reconciler},
    scheduler::{self, PollRequest, QueryError},
    smtp_message::Prepared,
    storage::{PollPreparation, SmtpPreparation, Store},
};
use std::{process::Command, time::Duration};

pub(super) struct Settings<'a> {
    pub(super) destinations: &'a [&'a str],
    pub(super) smtp_destinations: &'a [&'a str],
    pub(super) max_messages: usize,
}
#[derive(Debug)]
pub(super) enum Error {
    InvalidInput,
    Poll(scheduler::PollError),
    Preparation,
    Limit,
    Storage(super::storage::StorageError),
}

// Caller restores durable state first. Preparation is synchronous, count-bounded
// and local; an arbitrary blocking callback cannot be interrupted by this API.
pub(super) fn valid_settings(settings: &Settings<'_>) -> bool {
    let mut routes = std::collections::HashSet::new();
    let mut smtp = std::collections::HashSet::new();
    !(settings.max_messages == 0
        || settings.max_messages.checked_mul(16384).is_none()
        || settings.smtp_destinations.is_empty()
        || settings
            .destinations
            .iter()
            .any(|name| name.is_empty() || !routes.insert(*name))
        || settings
            .smtp_destinations
            .iter()
            .any(|name| !routes.contains(name) || !smtp.insert(*name)))
}

pub(super) fn poll_with(
    request: &PollRequest<'_>,
    reconciler: &mut Reconciler,
    store: &mut Store,
    settings: &Settings<'_>,
    observed_at: i64,
    query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
    prepare: impl FnMut(&Event, &str) -> Result<Prepared, ()>,
) -> Result<Vec<Event>, Error> {
    poll_prepared(
        request,
        reconciler,
        store,
        (settings, None),
        observed_at,
        query,
        prepare,
    )
}

pub(super) struct RevisionSettings<'a> {
    pub(super) settings: Settings<'a>,
    pub(super) revisions: &'a [super::smtp_revision::Descriptor],
}

pub(super) fn poll_with_revisions(
    request: &PollRequest<'_>,
    reconciler: &mut Reconciler,
    store: &mut Store,
    settings: &RevisionSettings<'_>,
    observed_at: i64,
    query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
    prepare: impl FnMut(&Event, &str) -> Result<Prepared, ()>,
) -> Result<Vec<Event>, Error> {
    poll_prepared(
        request,
        reconciler,
        store,
        (&settings.settings, Some(settings.revisions)),
        observed_at,
        query,
        prepare,
    )
}

fn poll_prepared(
    request: &PollRequest<'_>,
    reconciler: &mut Reconciler,
    store: &mut Store,
    settings: (&Settings<'_>, Option<&[super::smtp_revision::Descriptor]>),
    observed_at: i64,
    query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
    mut prepare: impl FnMut(&Event, &str) -> Result<Prepared, ()>,
) -> Result<Vec<Event>, Error> {
    let (settings, revisions) = settings;
    if observed_at < 0 || !valid_settings(settings) {
        return Err(Error::InvalidInput);
    }
    if let Some(revisions) = revisions {
        let mut names = std::collections::HashSet::new();
        if revisions.len() != settings.smtp_destinations.len()
            || revisions.iter().any(|revision| {
                !revision.revision_identity()
                    || !settings.smtp_destinations.contains(&revision.destination())
                    || !names.insert(revision.destination())
            })
        {
            return Err(Error::InvalidInput);
        }
    }
    let mut candidate = reconciler.clone();
    let events = scheduler::poll_with(request, &mut candidate, query).map_err(Error::Poll)?;
    let count = events
        .len()
        .checked_mul(settings.smtp_destinations.len())
        .filter(|count| *count <= settings.max_messages)
        .ok_or(Error::Limit)?;
    let mut owned = Vec::with_capacity(count);
    for (index, event) in events.iter().enumerate() {
        for destination in settings.smtp_destinations {
            let message = prepare(event, destination).map_err(|()| Error::Preparation)?;
            owned.push((index, *destination, message));
        }
    }
    let messages: Vec<_> = owned
        .iter()
        .map(|(index, destination, message)| SmtpPreparation {
            event: &events[*index],
            destination,
            message,
        })
        .collect();
    let preparation = PollPreparation {
        candidate: &candidate,
        deliverable: &events,
        destinations: settings.destinations,
        smtp_destinations: settings.smtp_destinations,
        messages: &messages,
    };
    match revisions {
        Some(revisions) => store.persist_prepared_revisions(
            preparation,
            revisions,
            request.cluster,
            request.history_end,
            observed_at,
        ),
        None => store.persist_prepared_reconciler(
            preparation,
            request.cluster,
            request.history_end,
            observed_at,
        ),
    }
    .map_err(Error::Storage)?;
    *reconciler = candidate;
    Ok(events)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    fn request() -> PollRequest<'static> {
        PollRequest {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(5),
            output_limit: 4096,
            emit_discovered: true,
        }
    }
    fn settings() -> Settings<'static> {
        Settings {
            destinations: &["plain", "a", "b"],
            smtp_destinations: &["a", "b"],
            max_messages: 4,
        }
    }
    fn query(command: &mut Command, _: Duration, _: usize) -> Result<String, QueryError> {
        Ok(if command.get_program() == "squeue" {
            ""
        } else {
            "cluster|1|alice|s|actual|COMPLETED|0:0"
        }
        .into())
    }
    fn message(_: &Event, destination: &str) -> Result<Prepared, ()> {
        super::super::smtp_message::prepare(
            "s@fixture.invalid",
            &format!("{destination}@fixture.invalid"),
            "stored",
            "done",
            "original\n.\n.dot",
            123,
        )
        .map_err(|_| ())
    }
    fn store(directory: &super::super::storage::tests::Directory) -> Store {
        Store::open(&directory.path(), Duration::from_millis(50)).unwrap()
    }
    #[test]
    fn revision_preflight_rejects_incomplete_duplicate_legacy_and_wrong_routes_before_query() {
        use super::super::{
            smtp_render::JobVisibility,
            smtp_revision::Descriptor,
            smtp_routes::{Metadata, Plan, Route},
        };
        let routes: Vec<_> = ["a", "b", "outside"]
            .iter()
            .map(|name| Route {
                name,
                visibility: JobVisibility::Hidden,
                metadata: Metadata {
                    sender: "s@fixture.invalid",
                    recipient: "r@fixture.invalid",
                    namespace: "fixture",
                },
            })
            .collect();
        let plan = Plan::new(&["a", "b", "outside"], &routes, 4).unwrap();
        for case in 0..5 {
            let revisions = match case {
                0 => vec![],
                1 => vec![Descriptor::from_revision_plan(&plan, "a").unwrap()],
                2 => vec![
                    Descriptor::from_revision_plan(&plan, "a").unwrap(),
                    Descriptor::from_revision_plan(&plan, "a").unwrap(),
                ],
                3 => vec![
                    Descriptor::from_plan(&plan, "a").unwrap(),
                    Descriptor::from_revision_plan(&plan, "b").unwrap(),
                ],
                _ => vec![
                    Descriptor::from_revision_plan(&plan, "a").unwrap(),
                    Descriptor::from_revision_plan(&plan, "outside").unwrap(),
                ],
            };
            let directory = super::super::storage::tests::Directory::new();
            let mut store = store(&directory);
            let mut reconciler = Reconciler::with_capacity(10);
            assert!(matches!(
                poll_with_revisions(
                    &request(),
                    &mut reconciler,
                    &mut store,
                    &RevisionSettings {
                        settings: settings(),
                        revisions: &revisions
                    },
                    10,
                    |_, _, _| panic!("invalid revisions queried"),
                    |_, _| panic!("invalid revisions rendered")
                ),
                Err(Error::InvalidInput)
            ));
            assert!(reconciler.latest.is_empty());
            assert!(store.restore("cluster", 10).unwrap().1.is_none());
        }
    }
    #[test]
    fn prepared_poll_freezes_routes_and_replay_never_calls_renderer() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = store(&directory);
        let mut reconciler = Reconciler::with_capacity(10);
        let mut prepared = 0;
        let events = poll_with(
            &request(),
            &mut reconciler,
            &mut store,
            &settings(),
            10,
            query,
            |event, destination| {
                prepared += 1;
                message(event, destination)
            },
        )
        .unwrap();
        assert_eq!(prepared, 2);
        assert_eq!(events.len(), 1);
        assert!(reconciler.emitted.contains(&events[0]));
        let due = store.due_deliveries("cluster", 10, 10).unwrap();
        assert_eq!(due.len(), 3);
        for item in &due {
            let frozen = store
                .frozen_smtp(&item.event_id, &item.destination)
                .unwrap();
            if item.destination == "plain" {
                assert!(frozen.is_none());
            } else {
                let frozen = frozen.unwrap();
                let expected = message(&item.event, &item.destination).unwrap();
                assert!(frozen.data() == expected.data());
                assert!(frozen.envelope().1 == format!("{}@fixture.invalid", item.destination));
            }
        }
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, checkpoint) = store.restore("cluster", 10).unwrap();
        assert_eq!(checkpoint.as_deref(), Some(request().history_end));
        let changed = Settings {
            destinations: &["changed"],
            smtp_destinations: &["changed"],
            max_messages: 4,
        };
        assert!(poll_with(
            &request(),
            &mut restored,
            &mut store,
            &changed,
            20,
            query,
            |_, _| panic!("historical message must not be rebuilt")
        )
        .unwrap()
        .is_empty());
        let replay = store.due_deliveries("cluster", 20, 10).unwrap();
        assert_eq!(
            replay
                .iter()
                .map(|item| (&item.event_id, &item.destination))
                .collect::<Vec<_>>(),
            due.iter()
                .map(|item| (&item.event_id, &item.destination))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn preparation_failure_and_count_limit_preserve_memory_and_database() {
        for limit in [1, 4] {
            let directory = super::super::storage::tests::Directory::new();
            let mut store = store(&directory);
            let mut reconciler = Reconciler::with_capacity(10);
            let mut calls = 0;
            let mut config = settings();
            config.max_messages = limit;
            let result = poll_with(
                &request(),
                &mut reconciler,
                &mut store,
                &config,
                10,
                query,
                |event, destination| {
                    calls += 1;
                    if calls == 2 {
                        Err(())
                    } else {
                        message(event, destination)
                    }
                },
            );
            if limit == 1 {
                assert!(matches!(result, Err(Error::Limit)));
                assert_eq!(calls, 0);
            } else {
                assert!(matches!(result, Err(Error::Preparation)));
                assert_eq!(calls, 2);
            }
            assert!(reconciler.latest.is_empty() && reconciler.emitted.is_empty());
            drop(store);
            let mut reopened = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
            let (restored, checkpoint) = reopened.restore("cluster", 10).unwrap();
            assert!(restored.latest.is_empty());
            assert!(checkpoint.is_none());
            assert!(reopened
                .due_deliveries("cluster", 100, 10)
                .unwrap()
                .is_empty());
            assert_eq!(
                poll_with(
                    &request(),
                    &mut reconciler,
                    &mut reopened,
                    &settings(),
                    10,
                    query,
                    message
                )
                .unwrap()
                .len(),
                1
            );
        }
    }
    #[test]
    fn storage_failure_preserves_candidate_then_retry_commits() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = store(&directory);
        let mut reconciler = Reconciler::with_capacity(10);
        let mut initial = request();
        initial.emit_discovered = false;
        initial.history_end = "2026-10-10T01:00:00";
        poll_with(
            &initial,
            &mut reconciler,
            &mut store,
            &settings(),
            9,
            |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    "1|alice|s|estimate|PENDING"
                } else {
                    ""
                }
                .into())
            },
            |_, _| panic!("pending baseline prepared"),
        )
        .unwrap();
        let before = reconciler.clone();
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("CREATE TRIGGER stop_poll BEFORE INSERT ON checkpoints BEGIN SELECT RAISE(ABORT,'private-fixture'); END;").unwrap();
        assert!(matches!(
            poll_with(
                &request(),
                &mut reconciler,
                &mut store,
                &settings(),
                10,
                query,
                message
            ),
            Err(Error::Storage(
                super::super::storage::StorageError::Database(_)
            ))
        ));
        assert_eq!(reconciler.latest, before.latest);
        assert_eq!(reconciler.emitted, before.emitted);
        let (restored, checkpoint) = store.restore("cluster", 10).unwrap();
        assert_eq!(restored.latest, before.latest);
        assert_eq!(checkpoint.as_deref(), Some(initial.history_end));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM smtp_messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        db.execute_batch("DROP TRIGGER stop_poll").unwrap();
        assert_eq!(
            poll_with(
                &request(),
                &mut reconciler,
                &mut store,
                &settings(),
                10,
                query,
                message
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(store.due_deliveries("cluster", 10, 10).unwrap().len(), 3);
    }
    fn rendered(event: &Event, route: &str, invalid: bool) -> Result<Prepared, ()> {
        use super::super::smtp_render::{self, JobVisibility, Metadata};
        smtp_render::prepare(
            event,
            &Metadata {
                sender: "s@fixture.invalid",
                recipient: &format!("{route}@fixture.invalid"),
                identity: if invalid {
                    "invalid\r\nfixture"
                } else {
                    "render_fixture"
                },
                timestamp: 123,
            },
            if route == "a" {
                JobVisibility::Hidden
            } else {
                JobVisibility::Identifier
            },
        )
        .map_err(|_| ())
    }

    #[test]
    fn later_mixed_route_render_failure_preserves_existing_state_and_reopen_recovers() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = store(&directory);
        let mut reconciler = Reconciler::with_capacity(10);
        poll_with(
            &request(),
            &mut reconciler,
            &mut store,
            &settings(),
            10,
            query,
            |event, route| rendered(event, route, false),
        )
        .unwrap();
        let before = reconciler.clone();
        let existing = store.due_deliveries("cluster", 10, 10).unwrap();
        let saved = store
            .frozen_smtp(&existing[0].event_id, "a")
            .unwrap()
            .unwrap();
        let mut next_request = request();
        next_request.history_end = "2026-10-11T01:00:00";
        let next_query = |command: &mut Command, _: Duration, _: usize| {
            Ok(if command.get_program() == "squeue" {
                ""
            } else {
                "cluster|2_3|alice|s|actual|FAILED|1:0\ncluster|3|alice|s|actual|COMPLETED|0:0"
            }
            .into())
        };
        let mut calls = 0;
        let result = poll_with(
            &next_request,
            &mut reconciler,
            &mut store,
            &settings(),
            20,
            next_query,
            |event, route| {
                calls += 1;
                rendered(event, route, calls == 4)
            },
        );
        assert!(matches!(result, Err(Error::Preparation)));
        assert_eq!(calls, 4);
        assert_eq!(reconciler.latest, before.latest);
        assert_eq!(reconciler.emitted, before.emitted);
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, checkpoint) = store.restore("cluster", 10).unwrap();
        assert_eq!(restored.latest, before.latest);
        assert_eq!(restored.emitted, before.emitted);
        assert_eq!(checkpoint.as_deref(), Some(request().history_end));
        assert_eq!(store.due_deliveries("cluster", 20, 10).unwrap().len(), 3);
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM smtp_messages", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(db);
        calls = 0;
        assert_eq!(
            poll_with(
                &next_request,
                &mut restored,
                &mut store,
                &settings(),
                20,
                next_query,
                |event, route| {
                    calls += 1;
                    rendered(event, route, false)
                }
            )
            .unwrap()
            .len(),
            2
        );
        assert_eq!(calls, 4);
        let due = store.due_deliveries("cluster", 20, 10).unwrap();
        assert_eq!(due.len(), 9);
        for item in &due {
            let snapshot = store
                .frozen_smtp(&item.event_id, &item.destination)
                .unwrap();
            if item.destination == "plain" {
                assert!(snapshot.is_none());
                continue;
            }
            let snapshot = snapshot.unwrap();
            let text = std::str::from_utf8(snapshot.data()).unwrap();
            assert!(snapshot.envelope().1 == format!("{}@fixture.invalid", item.destination));
            assert!(!text.contains("cluster") && !text.contains("actual"));
            assert_eq!(text.contains("Job: [redacted]"), item.destination == "a");
            assert_eq!(
                text.contains(&format!("Job: {}\r\n", item.event.identity.job)),
                item.destination == "b"
            );
            let expected = rendered(&item.event, &item.destination, false).unwrap();
            assert!(snapshot.data() == expected.data());
        }
        let kept = store
            .frozen_smtp(&existing[0].event_id, "a")
            .unwrap()
            .unwrap();
        assert!(kept.data() == saved.data());
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, checkpoint) = store.restore("cluster", 10).unwrap();
        assert_eq!(checkpoint.as_deref(), Some(next_request.history_end));
        assert!(poll_with(
            &next_request,
            &mut restored,
            &mut store,
            &settings(),
            30,
            next_query,
            |_, _| panic!("replay must preserve frozen disclosure and metadata")
        )
        .unwrap()
        .is_empty());
        assert_eq!(store.due_deliveries("cluster", 30, 10).unwrap().len(), 9);
        let kept = store
            .frozen_smtp(&existing[0].event_id, "a")
            .unwrap()
            .unwrap();
        assert!(kept.data() == saved.data());
    }

    #[test]
    fn invalid_settings_query_parse_and_suppression_do_not_prepare() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = store(&directory);
        let mut reconciler = Reconciler::with_capacity(10);
        for case in 0..7 {
            let mut settings = settings();
            match case {
                0 => settings.max_messages = 0,
                1 => settings.max_messages = usize::MAX,
                2 => settings.smtp_destinations = &[],
                3 => settings.destinations = &[""],
                4 => settings.destinations = &["a", "a"],
                5 => settings.smtp_destinations = &["absent"],
                _ => settings.smtp_destinations = &["a", "a"],
            }
            assert!(matches!(
                poll_with(
                    &request(),
                    &mut reconciler,
                    &mut store,
                    &settings,
                    10,
                    |_, _, _| panic!("invalid route queried scheduler"),
                    |_, _| panic!("invalid route prepared")
                ),
                Err(Error::InvalidInput)
            ));
        }
        assert!(matches!(
            poll_with(
                &request(),
                &mut reconciler,
                &mut store,
                &settings(),
                -1,
                query,
                message
            ),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            poll_with(
                &request(),
                &mut reconciler,
                &mut store,
                &settings(),
                10,
                |_, _, _| Err(QueryError::Io),
                |_, _| panic!("failed query prepared")
            ),
            Err(Error::Poll(scheduler::PollError::Active(QueryError::Io)))
        ));
        assert!(matches!(
            poll_with(
                &request(),
                &mut reconciler,
                &mut store,
                &settings(),
                10,
                |_, _, _| Ok("malformed".into()),
                |_, _| panic!("malformed snapshot prepared")
            ),
            Err(Error::Poll(scheduler::PollError::Parse(_)))
        ));
        assert!(reconciler.latest.is_empty());
        let mut request = request();
        request.emit_discovered = false;
        assert!(poll_with(
            &request,
            &mut reconciler,
            &mut store,
            &settings(),
            10,
            query,
            |_, _| panic!("suppressed event prepared")
        )
        .unwrap()
        .is_empty());
        assert_eq!(reconciler.emitted.len(), 1);
        assert!(store.due_deliveries("cluster", 10, 10).unwrap().is_empty());
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some(request.history_end)
        );
    }
}

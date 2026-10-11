//! Private validated rendering plan; no public routing/configuration contract.
use super::{
    lifecycle::{Event, Reconciler},
    scheduler::{PollRequest, QueryError},
    smtp_message::Prepared,
    smtp_metadata, smtp_poll,
    smtp_render::{self, JobVisibility},
    storage::Store,
};
use std::{process::Command, time::Duration};

// Explicit route namespace; event observation supplies the frozen Date.
pub(super) struct Metadata<'a> {
    pub(super) sender: &'a str,
    pub(super) recipient: &'a str,
    pub(super) namespace: &'a str,
}

// Explicit bounded inputs; no credentials, transport policy or runtime defaults.
pub(super) struct Route<'a> {
    pub(super) name: &'a str,
    pub(super) metadata: Metadata<'a>,
    pub(super) visibility: JobVisibility,
}

// Immutable borrows prevent route metadata/disclosure changes after validation.
// No Debug/Clone: route envelopes and metadata are private user data.
pub(super) struct Plan<'a> {
    destinations: &'a [&'a str],
    routes: &'a [Route<'a>],
    smtp_destinations: Vec<&'a str>,
    max_messages: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Policy,
    Metadata(smtp_render::Error),
    Route,
    Render(smtp_render::Error),
    Selection(smtp_metadata::Error),
}

impl<'a> Plan<'a> {
    pub(super) fn new(
        destinations: &'a [&'a str],
        routes: &'a [Route<'a>],
        max_messages: usize,
    ) -> Result<Self, Error> {
        // Check collection/name bounds before sets, allocation or library parsing.
        if destinations.is_empty()
            || destinations.len() > 64
            || routes.is_empty()
            || routes.len() > 64
            || destinations.iter().any(|name| !valid_name(name))
            || routes.iter().any(|route| !valid_name(route.name))
        {
            return Err(Error::Policy);
        }
        let smtp_destinations: Vec<_> = routes.iter().map(|route| route.name).collect();
        let settings = smtp_poll::Settings {
            destinations,
            smtp_destinations: &smtp_destinations,
            max_messages,
        };
        if !smtp_poll::valid_settings(&settings) {
            return Err(Error::Policy);
        }
        for route in routes {
            smtp_render::validate_metadata(&smtp_render::Metadata {
                sender: route.metadata.sender,
                recipient: route.metadata.recipient,
                identity: route.metadata.namespace,
                timestamp: 0,
            })
            .map_err(Error::Metadata)?;
        }
        Ok(Self {
            destinations,
            routes,
            smtp_destinations,
            max_messages,
        })
    }

    pub(super) fn prepare(
        &self,
        event: &Event,
        name: &str,
        observed_at: i64,
    ) -> Result<Prepared, Error> {
        let route = self
            .routes
            .iter()
            .find(|route| route.name == name)
            .ok_or(Error::Route)?;
        let selected = smtp_metadata::select(event, name, route.metadata.namespace, observed_at)
            .map_err(Error::Selection)?;
        smtp_render::prepare(
            event,
            &smtp_render::Metadata {
                sender: route.metadata.sender,
                recipient: route.metadata.recipient,
                identity: &selected.identity,
                timestamp: selected.timestamp,
            },
            route.visibility,
        )
        .map_err(Error::Render)
    }

    pub(super) fn poll_with(
        &self,
        request: &PollRequest<'_>,
        reconciler: &mut Reconciler,
        store: &mut Store,
        observed_at: i64,
        query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
    ) -> Result<Vec<Event>, smtp_poll::Error> {
        if smtp_metadata::timestamp(observed_at).is_err() {
            return Err(smtp_poll::Error::InvalidInput);
        }
        smtp_poll::poll_with(
            request,
            reconciler,
            store,
            &smtp_poll::Settings {
                destinations: self.destinations,
                smtp_destinations: &self.smtp_destinations,
                max_messages: self.max_messages,
            },
            observed_at,
            query,
            |event, route| self.prepare(event, route, observed_at).map_err(|_| ()),
        )
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::{Identity, State};
    fn route(name: &'static str, visibility: JobVisibility) -> Route<'static> {
        Route {
            name,
            visibility,
            metadata: Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: name,
            },
        }
    }
    fn event(job: &str) -> Event {
        Event {
            identity: Identity {
                cluster: "cluster".into(),
                job: job.into(),
                submit: "s".into(),
                start: "actual".into(),
            },
            state: State::Completed,
        }
    }
    fn failure(result: Result<Plan<'_>, Error>) -> Error {
        match result {
            Err(error) => error,
            Ok(_) => panic!("invalid plan accepted"),
        }
    }

    #[test]
    fn bounded_bindings_and_metadata_are_rejected_at_plan_construction() {
        for case in 0..12 {
            let mut routes = vec![
                route("a", JobVisibility::Hidden),
                route("b", JobVisibility::Identifier),
            ];
            let mut destinations: &[&str] = &["a", "b", "plain"];
            let mut max_messages = 4;
            match case {
                0 => routes.clear(),
                1 => destinations = &[],
                2 => destinations = &["a", "a"],
                3 => routes[1].name = "missing",
                4 => routes[1].name = "a",
                5 => routes[1].name = "injection\r\nfixture",
                6 => max_messages = 0,
                7 => max_messages = usize::MAX,
                8 => routes[1].metadata.sender = "private-invalid",
                9 => routes[1].metadata.namespace = "private\r\ninvalid",
                10 => routes[1].metadata.namespace = "",
                _ => routes[1].metadata.recipient = "private-invalid",
            }
            let error = failure(Plan::new(destinations, &routes, max_messages));
            assert!(
                !format!("{error:?}").contains("private")
                    && !format!("{error:?}").contains("fixture")
            );
        }
        let too_many: Vec<_> = (0..65).map(|_| route("a", JobVisibility::Hidden)).collect();
        assert_eq!(failure(Plan::new(&["a"], &too_many, 4)), Error::Policy);
        let destinations = vec!["a"; 65];
        assert_eq!(
            failure(Plan::new(
                &destinations,
                &[route("a", JobVisibility::Hidden)],
                4
            )),
            Error::Policy
        );
        let name = "a".repeat(65);
        assert_eq!(
            failure(Plan::new(&[&name], &[route("a", JobVisibility::Hidden)], 4)),
            Error::Policy
        );
    }

    #[test]
    fn maximum_route_count_and_name_length_are_validated_without_defaults() {
        let names: Vec<_> = (0..64).map(|index| format!("r{index:063}")).collect();
        let destinations: Vec<_> = names.iter().map(String::as_str).collect();
        let routes: Vec<_> = names
            .iter()
            .map(|name| Route {
                name,
                visibility: JobVisibility::Hidden,
                metadata: Metadata {
                    sender: "s@fixture.invalid",
                    recipient: "r@fixture.invalid",
                    namespace: name,
                },
            })
            .collect();
        let plan = Plan::new(&destinations, &routes, 64).unwrap();
        assert!(plan.prepare(&event("1"), &names[63], 10).is_ok());
    }

    #[test]
    fn valid_plan_binds_disclosure_and_rejects_unknown_routes_and_invalid_events() {
        let routes = [
            route("a", JobVisibility::Hidden),
            route("b", JobVisibility::Identifier),
        ];
        let plan = Plan::new(&["a", "b", "plain"], &routes, 4).unwrap();
        for (name, expected) in [("a", "Job: [redacted]"), ("b", "Job: 42_7")] {
            let prepared = plan.prepare(&event("42_7"), name, 10).unwrap();
            assert!(std::str::from_utf8(prepared.data())
                .unwrap()
                .contains(expected));
        }
        assert!(matches!(
            plan.prepare(&event("1"), "plain", 10),
            Err(Error::Route)
        ));
        assert!(matches!(
            plan.prepare(&event("unsafe-fixture"), "b", 10),
            Err(Error::Render(smtp_render::Error::Identifier))
        ));
        assert!(plan.prepare(&event("unsafe-fixture"), "a", 10).is_ok());
        let mut unsupported = event("1");
        unsupported.state = State::Unknown("private-fixture".into());
        assert!(matches!(
            plan.prepare(&unsupported, "a", 10),
            Err(Error::Render(smtp_render::Error::State))
        ));
    }

    #[test]
    fn event_metadata_is_unique_frozen_and_replayed_without_new_dates() {
        let directory = crate::storage::tests::Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let mut routes = [
            route("a", JobVisibility::Hidden),
            route("b", JobVisibility::Hidden),
        ];
        // One namespace may be shared: the destination remains part of the digest.
        routes[1].metadata.namespace = "a";
        let plan = Plan::new(&["a", "b", "plain"], &routes, 4).unwrap();
        let request = PollRequest {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        let query = |command: &mut Command, _: Duration, _: usize| {
            Ok(if command.get_program() == "squeue" {
                ""
            } else {
                "cluster|1|alice|s|actual|COMPLETED|0:0\ncluster|2|alice|s|actual|FAILED|1:0"
            }
            .into())
        };
        for observed in [-1, 4_102_444_800] {
            assert!(matches!(
                plan.poll_with(
                    &request,
                    &mut reconciler,
                    &mut store,
                    observed,
                    |_, _, _| panic!("invalid Date queried scheduler")
                ),
                Err(smtp_poll::Error::InvalidInput)
            ));
        }
        assert_eq!(
            plan.poll_with(&request, &mut reconciler, &mut store, 10, query)
                .unwrap()
                .len(),
            2
        );
        let due = store.due_deliveries("cluster", 10, 10).unwrap();
        let mut ids = std::collections::HashSet::new();
        let mut saved = Vec::new();
        for item in &due {
            if item.destination == "plain" {
                continue;
            }
            let frozen = store
                .frozen_smtp(&item.event_id, &item.destination)
                .unwrap()
                .unwrap();
            let selected = smtp_metadata::select(&item.event, &item.destination, "a", 10).unwrap();
            assert!(ids.insert(selected.identity.clone()));
            let text = std::str::from_utf8(frozen.data()).unwrap();
            assert!(text.contains(&format!(
                "Message-ID: <{}@fixture.invalid>\r\n",
                selected.identity
            )));
            assert!(text.contains("Date: Thu, 01 Jan 1970 00:00:10 +0000\r\n"));
            assert!(!text.contains("cluster") && !text.contains("actual"));
            saved.push((item.event_id.clone(), item.destination.clone(), frozen));
        }
        assert_eq!(ids.len(), 4);
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, _) = store.restore("cluster", 10).unwrap();
        assert!(plan
            .poll_with(&request, &mut restored, &mut store, 20, query)
            .unwrap()
            .is_empty());
        for (id, route, saved) in saved {
            let frozen = store.frozen_smtp(&id, &route).unwrap().unwrap();
            assert!(frozen.data() == saved.data());
        }
        assert_eq!(store.due_deliveries("cluster", 20, 10).unwrap().len(), 6);
    }

    #[test]
    fn validated_plan_polls_atomically_and_reopen_keeps_original_disclosure() {
        let directory = crate::storage::tests::Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let routes = [
            route("a", JobVisibility::Hidden),
            route("b", JobVisibility::Identifier),
        ];
        let plan = Plan::new(&["a", "b", "plain"], &routes, 4).unwrap();
        let request = PollRequest {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        // Numeric scheduler IDs can exceed this renderer's disclosure bound.
        let oversized = "1".repeat(33);
        let query = |command: &mut Command, _: Duration, _: usize| {
            Ok(if command.get_program() == "squeue" {
                String::new()
            } else {
                format!("cluster|{oversized}|alice|s|actual|COMPLETED|0:0")
            })
        };
        assert!(matches!(
            plan.poll_with(&request, &mut reconciler, &mut store, 10, query),
            Err(smtp_poll::Error::Preparation)
        ));
        assert!(reconciler.latest.is_empty());
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, checkpoint) = store.restore("cluster", 10).unwrap();
        assert!(checkpoint.is_none() && restored.emitted.is_empty());
        assert!(store.due_deliveries("cluster", 10, 10).unwrap().is_empty());
        let hidden = [
            route("a", JobVisibility::Hidden),
            route("b", JobVisibility::Hidden),
        ];
        let corrected = Plan::new(&["a", "b", "plain"], &hidden, 4).unwrap();
        assert_eq!(
            corrected
                .poll_with(&request, &mut restored, &mut store, 10, query)
                .unwrap()
                .len(),
            1
        );
        let due = store.due_deliveries("cluster", 10, 10).unwrap();
        assert_eq!(due.len(), 3);
        let saved = store.frozen_smtp(&due[0].event_id, "b").unwrap().unwrap();
        assert!(std::str::from_utf8(saved.data())
            .unwrap()
            .contains("Job: [redacted]"));
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, _) = store.restore("cluster", 10).unwrap();
        // Changed disclosure would fail rendering this historical ID if invoked.
        assert!(plan
            .poll_with(&request, &mut restored, &mut store, 20, query)
            .unwrap()
            .is_empty());
        let kept = store.frozen_smtp(&due[0].event_id, "b").unwrap().unwrap();
        assert!(kept.data() == saved.data());
        assert_eq!(store.due_deliveries("cluster", 20, 10).unwrap().len(), 3);
    }
}

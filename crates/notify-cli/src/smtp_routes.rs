//! Private validated rendering plan; no public routing/configuration contract.
use super::{
    lifecycle::{Event, Reconciler},
    scheduler::{PollRequest, QueryError},
    smtp_message::Prepared,
    smtp_poll,
    smtp_render::{self, JobVisibility, Metadata},
    storage::Store,
};
use std::{collections::HashSet, process::Command, time::Duration};

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
        let mut identities = HashSet::new();
        for route in routes {
            smtp_render::validate_metadata(&route.metadata).map_err(Error::Metadata)?;
            // Explicit per-route identity must not accidentally share Message-ID
            // across destinations. Per-event identity selection is still a caller
            // design concern; this experiment does not activate production IDs.
            if !identities.insert(route.metadata.identity) {
                return Err(Error::Policy);
            }
        }
        Ok(Self {
            destinations,
            routes,
            smtp_destinations,
            max_messages,
        })
    }

    pub(super) fn prepare(&self, event: &Event, name: &str) -> Result<Prepared, Error> {
        let route = self
            .routes
            .iter()
            .find(|route| route.name == name)
            .ok_or(Error::Route)?;
        smtp_render::prepare(event, &route.metadata, route.visibility).map_err(Error::Render)
    }

    pub(super) fn poll_with(
        &self,
        request: &PollRequest<'_>,
        reconciler: &mut Reconciler,
        store: &mut Store,
        observed_at: i64,
        query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
    ) -> Result<Vec<Event>, smtp_poll::Error> {
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
            |event, route| self.prepare(event, route).map_err(|_| ()),
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
                identity: name,
                timestamp: 123,
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
        for case in 0..13 {
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
                9 => routes[1].metadata.identity = "private\r\ninvalid",
                10 => routes[1].metadata.timestamp = u64::MAX,
                11 => routes[1].metadata.identity = "a",
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
                    identity: name,
                    timestamp: 4_102_444_799,
                },
            })
            .collect();
        let plan = Plan::new(&destinations, &routes, 64).unwrap();
        assert!(plan.prepare(&event("1"), &names[63]).is_ok());
    }

    #[test]
    fn valid_plan_binds_disclosure_and_rejects_unknown_routes_and_invalid_events() {
        let routes = [
            route("a", JobVisibility::Hidden),
            route("b", JobVisibility::Identifier),
        ];
        let plan = Plan::new(&["a", "b", "plain"], &routes, 4).unwrap();
        for (name, expected) in [("a", "Job: [redacted]"), ("b", "Job: 42_7")] {
            let prepared = plan.prepare(&event("42_7"), name).unwrap();
            assert!(std::str::from_utf8(prepared.data())
                .unwrap()
                .contains(expected));
        }
        assert!(matches!(
            plan.prepare(&event("1"), "plain"),
            Err(Error::Route)
        ));
        assert!(matches!(
            plan.prepare(&event("unsafe-fixture"), "b"),
            Err(Error::Render(smtp_render::Error::Identifier))
        ));
        assert!(plan.prepare(&event("unsafe-fixture"), "a").is_ok());
        let mut unsupported = event("1");
        unsupported.state = State::Unknown("private-fixture".into());
        assert!(matches!(
            plan.prepare(&unsupported, "a"),
            Err(Error::Render(smtp_render::Error::State))
        ));
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

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
    Provider(super::smtp_provider_revision::Error),
    Metadata(smtp_render::Error),
    #[cfg(test)]
    Route,
    #[cfg(test)]
    Render(smtp_render::Error),
    #[cfg(test)]
    Selection(smtp_metadata::Error),
}

// Captures immutable validated rendering/provider descriptors before any query.
// No Debug/Clone: profile identities and addresses are private metadata.
pub(super) struct ProviderPlan<'a, 'b> {
    rendering: &'a Plan<'b>,
    revisions: Vec<super::smtp_revision::Descriptor>,
    providers: Vec<super::smtp_provider_revision::Descriptor>,
}
impl<'a, 'b> ProviderPlan<'a, 'b> {
    pub(super) fn poll_selected_with<'s>(
        &self,
        request: &PollRequest<'_>,
        reconciler: &mut Reconciler,
        store: &mut Store,
        observed_at: i64,
        query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        select: impl FnMut(&Event) -> Vec<&'s str>,
    ) -> Result<Vec<Event>, smtp_poll::Error> {
        self.poll_selected_using(
            request,
            reconciler,
            store,
            (observed_at, None),
            query,
            select,
        )
    }
    pub(super) fn poll_public_selected_with<'s>(
        &self,
        request: &PollRequest<'_>,
        reconciler: &mut Reconciler,
        store: &mut Store,
        context: (i64, &[crate::config_composition::Webhook]),
        query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        select: impl FnMut(&Event) -> Vec<&'s str>,
    ) -> Result<Vec<Event>, smtp_poll::Error> {
        self.poll_selected_using(
            request,
            reconciler,
            store,
            (context.0, Some(context.1)),
            query,
            select,
        )
    }
    fn poll_selected_using<'s>(
        &self,
        request: &PollRequest<'_>,
        reconciler: &mut Reconciler,
        store: &mut Store,
        context: (i64, Option<&[crate::config_composition::Webhook]>),
        query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        mut select: impl FnMut(&Event) -> Vec<&'s str>,
    ) -> Result<Vec<Event>, smtp_poll::Error> {
        let (observed_at, webhooks) = context;
        let mut bindings = Vec::new();
        if let Some(webhooks) = webhooks {
            let mut seen = std::collections::HashSet::new();
            for webhook in webhooks {
                let destination = webhook.alias.as_str();
                if !self.rendering.destinations.contains(&destination)
                    || self.rendering.smtp_destinations.contains(&destination)
                    || !seen.insert(destination)
                {
                    return Err(smtp_poll::Error::InvalidInput);
                }
                bindings.push(crate::storage::PublicRoute {
                    email: None,
                    slack: webhook.resources.slack_options(),
                    destination,
                    revision: webhook
                        .resources
                        .public_revision()
                        .ok_or(smtp_poll::Error::InvalidInput)?,
                });
            }
            if self.rendering.destinations.iter().any(|name| {
                !self.rendering.smtp_destinations.contains(name) && !seen.contains(name)
            }) {
                return Err(smtp_poll::Error::InvalidInput);
            }
        }
        if smtp_metadata::timestamp(observed_at).is_err() {
            return Err(smtp_poll::Error::InvalidInput);
        }
        let mut candidate = reconciler.clone();
        let events = super::scheduler::poll_with(request, &mut candidate, query)
            .map_err(smtp_poll::Error::Poll)?;
        let mut selected = std::collections::HashMap::new();
        let mut count = 0usize;
        for event in &events {
            let names = select(event);
            let mut seen = std::collections::HashSet::new();
            if names.len() > 64
                || names
                    .iter()
                    .any(|name| !self.rendering.destinations.contains(name) || !seen.insert(*name))
            {
                return Err(smtp_poll::Error::InvalidInput);
            }
            count = count
                .checked_add(
                    names
                        .iter()
                        .filter(|name| self.rendering.smtp_destinations.contains(name))
                        .count(),
                )
                .filter(|count| *count <= self.rendering.max_messages)
                .ok_or(smtp_poll::Error::Limit)?;
            selected.insert(event.clone(), names);
        }
        if let Some(webhooks) = webhooks {
            for event in &events {
                let routes: Vec<_> = webhooks
                    .iter()
                    .filter(|webhook| selected[event].contains(&webhook.alias.as_str()))
                    .collect();
                if routes.is_empty() {
                    continue;
                }
                let payload = crate::public_event::payload(
                    "evt_00000000000040008000000000000000",
                    &event.identity.cluster,
                    &event.identity.job,
                    event.state.code(),
                    observed_at,
                )
                .map_err(|_| smtp_poll::Error::Preparation)?;
                if routes
                    .iter()
                    .any(|webhook| !webhook.resources.accepts_public_payload(&payload))
                {
                    return Err(smtp_poll::Error::Limit);
                }
            }
        }
        let mut owned = Vec::with_capacity(count);
        for event in &events {
            for name in &selected[event] {
                if self.rendering.smtp_destinations.contains(name) {
                    let message = self
                        .rendering
                        .prepare_revision(event, name, observed_at, &self.revisions)
                        .map_err(|_| smtp_poll::Error::Preparation)?;
                    owned.push((event, *name, message));
                }
            }
        }
        let messages: Vec<_> = owned
            .iter()
            .map(
                |(event, destination, message)| super::storage::SmtpPreparation {
                    event,
                    destination,
                    message,
                },
            )
            .collect();
        let preparation = super::storage::PollPreparation {
            candidate: &candidate,
            deliverable: &events,
            destinations: self.rendering.destinations,
            smtp_destinations: &self.rendering.smtp_destinations,
            messages: &messages,
        };
        let revisions = super::storage::SmtpRevisions {
            rendering: &self.revisions,
            providers: &self.providers,
        };
        if webhooks.is_some() {
            store.persist_public_profiles(
                preparation,
                revisions,
                (request.cluster, request.history_end, observed_at),
                &selected,
                &bindings,
            )
        } else {
            store.persist_selected_profiles(
                preparation,
                revisions,
                request.cluster,
                request.history_end,
                observed_at,
                &selected,
            )
        }
        .map_err(smtp_poll::Error::Storage)?;
        *reconciler = candidate;
        Ok(events)
    }
    pub(super) fn new(
        rendering: &'a Plan<'b>,
        profiles: &[super::smtp_provider_revision::Profile<'_>],
    ) -> Result<Self, Error> {
        if profiles.len() != rendering.routes.len() {
            return Err(Error::Policy);
        }
        let mut names = std::collections::HashSet::new();
        if profiles.iter().any(|profile| {
            rendering.route(profile.destination).is_none() || !names.insert(profile.destination)
        }) {
            return Err(Error::Policy);
        }
        let providers = profiles
            .iter()
            .map(super::smtp_provider_revision::Descriptor::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::Provider)?;
        let revisions = rendering
            .routes
            .iter()
            .map(|route| {
                super::smtp_revision::Descriptor::from_revision_plan(rendering, route.name)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Error::Policy)?;
        Ok(Self {
            rendering,
            revisions,
            providers,
        })
    }
    #[cfg(test)]
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
        smtp_poll::poll_with_profiles(
            request,
            reconciler,
            store,
            &smtp_poll::ProfileSettings {
                settings: smtp_poll::Settings {
                    destinations: self.rendering.destinations,
                    smtp_destinations: &self.rendering.smtp_destinations,
                    max_messages: self.rendering.max_messages,
                },
                revisions: &self.revisions,
                providers: &self.providers,
            },
            observed_at,
            query,
            |event, name| {
                self.rendering
                    .prepare_revision(event, name, observed_at, &self.revisions)
            },
        )
    }
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

    pub(super) fn route(&self, name: &str) -> Option<&Route<'a>> {
        self.routes.iter().find(|route| route.name == name)
    }

    #[cfg(test)]
    pub(super) fn prepare(
        &self,
        event: &Event,
        name: &str,
        observed_at: i64,
    ) -> Result<Prepared, Error> {
        let route = self.route(name).ok_or(Error::Route)?;
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

    #[cfg(test)]
    pub(super) fn poll_revision_with(
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
        let revisions: Vec<_> = self
            .routes
            .iter()
            .map(|route| super::smtp_revision::Descriptor::from_revision_plan(self, route.name))
            .collect::<Result<_, _>>()
            .map_err(|_| smtp_poll::Error::Preparation)?;
        smtp_poll::poll_with_revisions(
            request,
            reconciler,
            store,
            &smtp_poll::RevisionSettings {
                settings: smtp_poll::Settings {
                    destinations: self.destinations,
                    smtp_destinations: &self.smtp_destinations,
                    max_messages: self.max_messages,
                },
                revisions: &revisions,
            },
            observed_at,
            query,
            |event, name| self.prepare_revision(event, name, observed_at, &revisions),
        )
    }

    fn prepare_revision(
        &self,
        event: &Event,
        name: &str,
        observed_at: i64,
        revisions: &[super::smtp_revision::Descriptor],
    ) -> Result<Prepared, ()> {
        let route = self.route(name).ok_or(())?;
        let revision = revisions
            .iter()
            .find(|revision| revision.destination() == name)
            .ok_or(())?;
        let selected = smtp_metadata::select_revision(
            event,
            name,
            route.metadata.namespace,
            observed_at,
            revision.token(),
        )
        .map_err(|_| ())?;
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
        .map_err(|_| ())
    }

    #[cfg(test)]
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

    fn revision_request() -> PollRequest<'static> {
        PollRequest {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        }
    }
    fn revision_query(
        records: &str,
    ) -> impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError> + '_ {
        move |command, _, _| {
            Ok(if command.get_program() == "squeue" {
                ""
            } else {
                records
            }
            .into())
        }
    }
    fn table_count(directory: &crate::storage::tests::Directory, table: &str) -> i64 {
        rusqlite::Connection::open(directory.path())
            .unwrap()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }
    fn provider_profile<'a>(
        name: &'a str,
        relay: &'static str,
    ) -> super::super::smtp_provider_revision::Profile<'a> {
        super::super::smtp_provider_revision::Profile {
            destination: name,
            relay,
            port: 587,
            tls: super::super::smtp_provider_revision::TlsMode::RequiredStartTls,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        }
    }
    #[test]
    fn provider_plan_rejects_incomplete_duplicate_unknown_and_invalid_profiles() {
        let routes = [
            route("a", JobVisibility::Hidden),
            route("b", JobVisibility::Hidden),
        ];
        let rendering = Plan::new(&["a", "b", "plain"], &routes, 4).unwrap();
        for profiles in [
            vec![],
            vec![provider_profile("a", "relay.invalid")],
            vec![
                provider_profile("a", "relay.invalid"),
                provider_profile("a", "relay.invalid"),
            ],
            vec![
                provider_profile("a", "relay.invalid"),
                provider_profile("plain", "relay.invalid"),
            ],
            vec![
                provider_profile("a", "relay.invalid"),
                provider_profile("b", "unsafe\r\n"),
            ],
        ] {
            assert!(ProviderPlan::new(&rendering, &profiles).is_err());
        }
        assert!(ProviderPlan::new(
            &rendering,
            &[
                provider_profile("b", "relay.invalid"),
                provider_profile("a", "relay.invalid")
            ]
        )
        .is_ok());
    }
    #[test]
    fn provider_poll_commits_both_bindings_and_changed_profiles_preserve_reopened_history() {
        let directory = crate::storage::tests::Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let routes = [route("a", JobVisibility::Hidden)];
        let rendering = Plan::new(&["a", "plain"], &routes, 4).unwrap();
        let plan =
            ProviderPlan::new(&rendering, &[provider_profile("a", "relay.invalid")]).unwrap();
        let request = revision_request();
        let records = "cluster|1|alice|s|actual|COMPLETED|0:0";
        plan.poll_with(
            &request,
            &mut reconciler,
            &mut store,
            10,
            revision_query(records),
        )
        .unwrap();
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let original = store
            .bound_smtp_provider(&item.event_id, "a")
            .unwrap()
            .unwrap();
        let bytes = store.frozen_smtp(&item.event_id, "a").unwrap().unwrap();
        assert!(store
            .bound_smtp_provider(&item.event_id, "plain")
            .unwrap()
            .is_none());
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut reconciler, _) = store.restore("cluster", 10).unwrap();
        let changed =
            ProviderPlan::new(&rendering, &[provider_profile("a", "changed.invalid")]).unwrap();
        let records = format!("{records}\ncluster|2|alice|s|actual|FAILED|1:0");
        assert_eq!(
            changed
                .poll_with(
                    &request,
                    &mut reconciler,
                    &mut store,
                    20,
                    revision_query(&records)
                )
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .bound_smtp_provider(&item.event_id, "a")
                .unwrap()
                .unwrap()
                .token(),
            original.token()
        );
        assert_eq!(
            store
                .frozen_smtp(&item.event_id, "a")
                .unwrap()
                .unwrap()
                .data(),
            bytes.data()
        );
        let new = store
            .due_deliveries("cluster", 20, 10)
            .unwrap()
            .into_iter()
            .find(|item| item.event.identity.job == "2" && item.destination == "a")
            .unwrap();
        assert_eq!(
            store
                .bound_smtp_provider(&new.event_id, "a")
                .unwrap()
                .unwrap()
                .profile()
                .relay,
            "changed.invalid"
        );
        assert!(changed
            .poll_with(
                &request,
                &mut reconciler,
                &mut store,
                30,
                revision_query(&records)
            )
            .unwrap()
            .is_empty());
        assert_eq!(table_count(&directory, "smtp_provider_revisions"), 2);
        assert_eq!(table_count(&directory, "smtp_provider_bindings"), 2);
    }
    #[test]
    fn provider_poll_late_write_failure_preserves_memory_and_recovers_after_reopen() {
        let directory = crate::storage::tests::Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let routes = [route("a", JobVisibility::Hidden)];
        let rendering = Plan::new(&["a"], &routes, 4).unwrap();
        let plan =
            ProviderPlan::new(&rendering, &[provider_profile("a", "relay.invalid")]).unwrap();
        let request = revision_request();
        let records = "cluster|1|alice|s|actual|COMPLETED|0:0";
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("CREATE TRIGGER reject_provider BEFORE INSERT ON smtp_provider_bindings BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(matches!(
            plan.poll_with(
                &request,
                &mut reconciler,
                &mut store,
                10,
                revision_query(records)
            ),
            Err(smtp_poll::Error::Storage(_))
        ));
        assert!(reconciler.latest.is_empty() && reconciler.emitted.is_empty());
        for table in [
            "events",
            "outbox",
            "smtp_messages",
            "smtp_revision_bindings",
            "smtp_provider_revisions",
            "smtp_provider_bindings",
            "checkpoints",
        ] {
            assert_eq!(table_count(&directory, table), 0);
        }
        drop(store);
        db.execute_batch("DROP TRIGGER reject_provider;").unwrap();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, checkpoint) = store.restore("cluster", 10).unwrap();
        assert!(checkpoint.is_none());
        assert_eq!(
            plan.poll_with(
                &request,
                &mut restored,
                &mut store,
                20,
                revision_query(records)
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(table_count(&directory, "smtp_provider_bindings"), 1);
    }
    #[test]
    fn provider_poll_suppression_legacy_replay_and_render_failure_do_not_backfill() {
        let directory = crate::storage::tests::Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let routes = [route("a", JobVisibility::Identifier)];
        let rendering = Plan::new(&["a"], &routes, 4).unwrap();
        let plan =
            ProviderPlan::new(&rendering, &[provider_profile("a", "relay.invalid")]).unwrap();
        let mut request = revision_request();
        for time in [-1, 4_102_444_800] {
            assert!(matches!(
                plan.poll_with(
                    &request,
                    &mut reconciler,
                    &mut store,
                    time,
                    |_, _, _| panic!("invalid time queried")
                ),
                Err(smtp_poll::Error::InvalidInput)
            ));
        }
        let oversized = format!("cluster|{}|alice|s|actual|COMPLETED|0:0", "1".repeat(33));
        assert!(matches!(
            plan.poll_with(
                &request,
                &mut reconciler,
                &mut store,
                10,
                revision_query(&oversized)
            ),
            Err(smtp_poll::Error::Preparation)
        ));
        assert!(reconciler.latest.is_empty());
        let records = "cluster|1|alice|s|actual|COMPLETED|0:0";
        request.emit_discovered = false;
        assert!(plan
            .poll_with(
                &request,
                &mut reconciler,
                &mut store,
                10,
                revision_query(records)
            )
            .unwrap()
            .is_empty());
        request.emit_discovered = true;
        let records = format!("{records}\ncluster|2|alice|s|actual|FAILED|1:0");
        rendering
            .poll_revision_with(
                &request,
                &mut reconciler,
                &mut store,
                20,
                revision_query(&records),
            )
            .unwrap();
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, _) = store.restore("cluster", 10).unwrap();
        assert!(plan
            .poll_with(
                &request,
                &mut restored,
                &mut store,
                30,
                revision_query(&records)
            )
            .unwrap()
            .is_empty());
        assert_eq!(table_count(&directory, "smtp_provider_revisions"), 0);
        assert_eq!(table_count(&directory, "smtp_provider_bindings"), 0);
        assert_eq!(table_count(&directory, "smtp_messages"), 1);
    }

    #[test]
    fn revision_poll_changes_new_events_and_preserves_bound_and_legacy_history() {
        let directory = crate::storage::tests::Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let routes = [route("a", JobVisibility::Hidden)];
        let plan = Plan::new(&["a", "plain"], &routes, 4).unwrap();
        let request = revision_request();
        let first = "cluster|1|alice|s|actual|COMPLETED|0:0";
        plan.poll_revision_with(
            &request,
            &mut reconciler,
            &mut store,
            10,
            revision_query(first),
        )
        .unwrap();
        let item = store
            .due_deliveries("cluster", 10, 10)
            .unwrap()
            .into_iter()
            .find(|item| item.destination == "a")
            .unwrap();
        let saved = store.frozen_smtp(&item.event_id, "a").unwrap().unwrap();
        let bound = store
            .bound_smtp_revision(&item.event_id, "a")
            .unwrap()
            .unwrap();
        let selected =
            smtp_metadata::select_revision(&item.event, "a", "a", 10, bound.token()).unwrap();
        let text = std::str::from_utf8(saved.data()).unwrap();
        assert!(text.contains(&format!(
            "Message-ID: <{}@fixture.invalid>",
            selected.identity
        )));
        assert!(text.contains("Date: Thu, 01 Jan 1970 00:00:10 +0000"));
        assert!(store
            .bound_smtp_revision(&item.event_id, "plain")
            .unwrap()
            .is_none());
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut reconciler, _) = store.restore("cluster", 10).unwrap();
        let mut changed = [
            route("a", JobVisibility::Identifier),
            route("b", JobVisibility::Hidden),
        ];
        changed[0].metadata.recipient = "changed@fixture.invalid";
        let changed = Plan::new(&["a", "b", "plain"], &changed, 4).unwrap();
        let records = format!("{first}\ncluster|2|alice|s|actual|FAILED|1:0");
        assert_eq!(
            changed
                .poll_revision_with(
                    &request,
                    &mut reconciler,
                    &mut store,
                    20,
                    revision_query(&records)
                )
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .bound_smtp_revision(&item.event_id, "a")
                .unwrap()
                .unwrap()
                .token(),
            bound.token()
        );
        assert_eq!(
            store
                .frozen_smtp(&item.event_id, "a")
                .unwrap()
                .unwrap()
                .data(),
            saved.data()
        );
        assert!(store.frozen_smtp(&item.event_id, "b").unwrap().is_none());
        let new = store
            .due_deliveries("cluster", 20, 10)
            .unwrap()
            .into_iter()
            .find(|row| row.event.identity.job == "2" && row.destination == "a")
            .unwrap();
        let revision = store
            .bound_smtp_revision(&new.event_id, "a")
            .unwrap()
            .unwrap();
        assert_ne!(revision.token(), bound.token());
        assert_eq!(
            store
                .frozen_smtp(&new.event_id, "a")
                .unwrap()
                .unwrap()
                .envelope()
                .1,
            "changed@fixture.invalid"
        );
        let legacy_records = format!("{records}\ncluster|3|alice|s|actual|COMPLETED|0:0");
        plan.poll_with(
            &request,
            &mut reconciler,
            &mut store,
            30,
            revision_query(&legacy_records),
        )
        .unwrap();
        assert!(changed
            .poll_revision_with(
                &request,
                &mut reconciler,
                &mut store,
                40,
                revision_query(&legacy_records)
            )
            .unwrap()
            .is_empty());
        let legacy = store
            .due_deliveries("cluster", 40, 10)
            .unwrap()
            .into_iter()
            .find(|row| row.event.identity.job == "3" && row.destination == "a")
            .unwrap();
        assert!(store
            .bound_smtp_revision(&legacy.event_id, "a")
            .unwrap()
            .is_none());
        assert_eq!(table_count(&directory, "smtp_revision_bindings"), 3);
        assert_eq!(table_count(&directory, "smtp_render_revisions"), 3);
    }
    #[test]
    fn revision_poll_late_binding_and_checkpoint_failures_preserve_memory_and_recover() {
        for failure in ["CREATE TRIGGER reject_revision BEFORE INSERT ON smtp_revision_bindings BEGIN SELECT RAISE(ABORT,'fixture'); END;", "CREATE TRIGGER reject_revision BEFORE INSERT ON checkpoints BEGIN SELECT RAISE(ABORT,'fixture'); END;"] {
            let directory = crate::storage::tests::Directory::new();
            let mut store = Store::open(&directory.path(),Duration::from_millis(50)).unwrap();
            let mut reconciler = Reconciler::with_capacity(10);
            let routes = [route("a",JobVisibility::Hidden),route("b",JobVisibility::Hidden)];
            let plan = Plan::new(&["a","b"],&routes,4).unwrap();
            let db = rusqlite::Connection::open(directory.path()).unwrap();
            db.execute_batch(failure).unwrap();
            let request = revision_request();
            let records = "cluster|1|alice|s|actual|COMPLETED|0:0";
            assert!(matches!(plan.poll_revision_with(&request,&mut reconciler,&mut store,10,revision_query(records)),Err(smtp_poll::Error::Storage(_))));
            assert!(reconciler.latest.is_empty() && reconciler.emitted.is_empty());
            for table in ["jobs","events","outbox","smtp_messages","smtp_render_revisions","smtp_revision_bindings","checkpoints"] { assert_eq!(table_count(&directory,table),0); }
            drop(store);
            db.execute_batch("DROP TRIGGER reject_revision;").unwrap();
            let mut store = Store::open(&directory.path(),Duration::from_millis(50)).unwrap();
            let (mut restored,checkpoint) = store.restore("cluster",10).unwrap();
            assert!(checkpoint.is_none());
            assert_eq!(plan.poll_revision_with(&request,&mut restored,&mut store,20,revision_query(records)).unwrap().len(),1);
            assert_eq!(table_count(&directory,"smtp_revision_bindings"),2);
            assert_eq!(store.restore("cluster",10).unwrap().1.as_deref(),Some(request.history_end));
        }
    }
    #[test]
    fn atomic_revision_conflict_keeps_prior_snapshot_and_checkpoint() {
        let directory = crate::storage::tests::Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let routes = [route("a", JobVisibility::Hidden)];
        let plan = Plan::new(&["a"], &routes, 4).unwrap();
        let mut request = revision_request();
        let first = "cluster|1|alice|s|actual|COMPLETED|0:0";
        plan.poll_revision_with(
            &request,
            &mut reconciler,
            &mut store,
            10,
            revision_query(first),
        )
        .unwrap();
        let original = reconciler.clone();
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let saved = store.frozen_smtp(&item.event_id, "a").unwrap().unwrap();
        let descriptor =
            super::super::smtp_revision::Descriptor::from_revision_plan(&plan, "a").unwrap();
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute(
            "UPDATE smtp_render_revisions SET descriptor=?1",
            [b"corrupt".as_slice()],
        )
        .unwrap();
        request.history_end = "2026-10-12T00:00:00";
        let records = format!("{first}\ncluster|2|alice|s|actual|FAILED|1:0");
        assert!(matches!(
            plan.poll_revision_with(
                &request,
                &mut reconciler,
                &mut store,
                20,
                revision_query(&records)
            ),
            Err(smtp_poll::Error::Storage(
                crate::storage::StorageError::Conflict
            ))
        ));
        assert_eq!(reconciler.latest, original.latest);
        assert_eq!(reconciler.emitted, original.emitted);
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some("2026-10-11T00:00:00")
        );
        assert_eq!(table_count(&directory, "smtp_revision_bindings"), 1);
        assert_eq!(table_count(&directory, "events"), 1);
        assert_eq!(
            store
                .frozen_smtp(&item.event_id, "a")
                .unwrap()
                .unwrap()
                .data(),
            saved.data()
        );
        db.execute(
            "UPDATE smtp_render_revisions SET descriptor=?1",
            [descriptor.canonical()],
        )
        .unwrap();
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, _) = store.restore("cluster", 10).unwrap();
        assert_eq!(
            plan.poll_revision_with(
                &request,
                &mut restored,
                &mut store,
                30,
                revision_query(&records)
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(table_count(&directory, "smtp_render_revisions"), 1);
        assert_eq!(table_count(&directory, "smtp_revision_bindings"), 2);
    }

    #[test]
    fn revision_poll_suppression_limits_and_render_failure_do_not_create_provenance() {
        let directory = crate::storage::tests::Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let routes = [
            route("a", JobVisibility::Hidden),
            route("b", JobVisibility::Identifier),
        ];
        let plan = Plan::new(&["a", "b"], &routes, 4).unwrap();
        let mut request = revision_request();
        let oversized = format!("cluster|{}|alice|s|actual|COMPLETED|0:0", "1".repeat(33));
        assert!(matches!(
            plan.poll_revision_with(
                &request,
                &mut reconciler,
                &mut store,
                10,
                revision_query(&oversized)
            ),
            Err(smtp_poll::Error::Preparation)
        ));
        let limited = Plan::new(&["a", "b"], &routes, 1).unwrap();
        assert!(matches!(
            limited.poll_revision_with(
                &request,
                &mut reconciler,
                &mut store,
                10,
                revision_query("cluster|1|alice|s|actual|COMPLETED|0:0")
            ),
            Err(smtp_poll::Error::Limit)
        ));
        assert!(reconciler.latest.is_empty());
        assert_eq!(table_count(&directory, "smtp_render_revisions"), 0);
        for observed in [-1, 4_102_444_800] {
            assert!(matches!(
                plan.poll_revision_with(
                    &request,
                    &mut reconciler,
                    &mut store,
                    observed,
                    |_, _, _| panic!("invalid Date queried")
                ),
                Err(smtp_poll::Error::InvalidInput)
            ));
        }
        request.emit_discovered = false;
        assert!(plan
            .poll_revision_with(
                &request,
                &mut reconciler,
                &mut store,
                10,
                revision_query("cluster|1|alice|s|actual|COMPLETED|0:0")
            )
            .unwrap()
            .is_empty());
        assert_eq!(table_count(&directory, "events"), 1);
        assert_eq!(table_count(&directory, "smtp_render_revisions"), 0);
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, _) = store.restore("cluster", 10).unwrap();
        request.emit_discovered = true;
        assert!(plan
            .poll_revision_with(
                &request,
                &mut restored,
                &mut store,
                20,
                revision_query("cluster|1|alice|s|actual|COMPLETED|0:0")
            )
            .unwrap()
            .is_empty());
        assert_eq!(table_count(&directory, "smtp_revision_bindings"), 0);
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

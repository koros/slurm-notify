//! Private mixed-draft composition; no installed behavior or permission enrollment.
use crate::{
    config_composition,
    config_validate::{self, get, list, object, text},
    smtp_provider_delivery::{self, Permission, Resolved},
    smtp_provider_revision::{Descriptor, Profile, TlsMode},
    smtp_render::JobVisibility,
    smtp_routes, trust_snapshot,
};
use std::{path::PathBuf, time::Duration};
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Snapshot(crate::config_snapshot::Error),
    Validation(config_validate::Error),
    Shape,
    NoSmtp,
    Rendering,
    Profile,
    Trust(trust_snapshot::Error),
    Common(config_composition::Error),
    Credentials(crate::credential_resolution::Error),
}
struct Smtp {
    name: Zeroizing<String>,
    sender: Zeroizing<String>,
    recipient: Zeroizing<String>,
    namespace: Zeroizing<String>,
    visibility: JobVisibility,
    descriptor: Descriptor,
    rendering: crate::smtp_revision::Descriptor,
    trust: trust_snapshot::Trust,
    username: Zeroizing<String>,
    password: Zeroizing<String>,
    timeout: Duration,
}
// No Debug/Clone; secrets are looked up only after delivery's exact-pair guard.
pub(super) struct Plan {
    common: config_composition::Plan,
    smtp: Vec<Smtp>,
    permissions: Vec<[Zeroizing<String>; 4]>,
    max_messages: usize,
}
pub(super) struct PublicDeliveryHooks<L, S, W, C, X> {
    pub(super) lookup: L,
    pub(super) smtp_helper: S,
    pub(super) webhook_helper: W,
    pub(super) clock: C,
    pub(super) cancelled: X,
}
use crate::config_reference::reference;
impl Smtp {
    fn route(&self) -> smtp_routes::Route<'_> {
        smtp_routes::Route {
            name: &self.name,
            metadata: smtp_routes::Metadata {
                sender: &self.sender,
                recipient: &self.recipient,
                namespace: &self.namespace,
            },
            visibility: self.visibility,
        }
    }
}
impl Plan {
    pub(super) fn poll_interval(&self) -> Duration {
        self.common.controls.poll
    }
    // Existing owned namespace fixes the database basename; never reinterpret a path.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) fn open_owned(
        &self,
        lock_timeout: Duration,
    ) -> Result<crate::storage::Store, crate::storage::StorageError> {
        let parent = crate::state_path::database_parent(self.common.storage_path.as_str())
            .map_err(|_| crate::storage::StorageError::InvalidInput)?;
        crate::storage::Store::open_private(parent, lock_timeout)
    }
    pub(super) fn status(
        &self,
        cycles: &crate::polling_cycle::Cycles,
        store: &mut crate::storage::Store,
        now: i64,
    ) -> Result<crate::polling_cycle::HealthReport, crate::polling_cycle::ReportError> {
        cycles.status(
            crate::polling_cycle::StatusPolicy {
                cluster: self.common.observation.cluster(),
                capacities: (
                    self.common.controls.identity_limit,
                    self.common.controls.event_limit,
                ),
                stale_after: self.common.controls.maintenance.stale_after,
                retain_seconds: self.common.controls.maintenance.retain_seconds,
                sample_limit: self.common.controls.maintenance.health_sample_limit,
            },
            store,
            now,
        )
    }
    pub(super) fn permissions(&self) -> Vec<Permission<'_>> {
        self.permissions
            .iter()
            .map(|p| Permission {
                cluster: &p[0],
                destination: &p[1],
                rendering: &p[2],
                provider: &p[3],
            })
            .collect()
    }
    pub(super) fn revisions(&self) -> Vec<(&str, &str, &str)> {
        self.smtp
            .iter()
            .map(|s| (s.name.as_str(), s.rendering.token(), s.descriptor.token()))
            .collect()
    }
    pub(super) fn resolve(
        &self,
        bound: &Descriptor,
        lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    ) -> Result<Resolved, Error> {
        let smtp = self
            .smtp
            .iter()
            .find(|s| s.descriptor.token() == bound.token())
            .ok_or(Error::Profile)?;
        let profile = smtp.descriptor.profile();
        if !bound.matches(&profile).map_err(|_| Error::Profile)? {
            return Err(Error::Profile);
        }
        let credentials = crate::credential_resolution::resolve(
            profile.authentication_identity,
            &smtp.username,
            &smtp.password,
            lookup,
        )
        .map_err(Error::Credentials)?;
        Resolved::from_owned(bound, &profile, &smtp.trust.roots(), credentials)
            .map_err(|_| Error::Profile)
    }
    pub(super) fn timeout(&self, name: &str) -> Result<Duration, Error> {
        self.smtp
            .iter()
            .find(|s| s.name.as_str() == name)
            .map(|s| s.timeout)
            .ok_or(Error::Profile)
    }
    pub(super) fn deliver_public_with<L, S, W, C, X>(
        &self,
        store: &mut crate::storage::Store,
        now: i64,
        hooks: PublicDeliveryHooks<L, S, W, C, X>,
    ) -> Result<Result<crate::delivery::BatchReport, crate::delivery::BatchFailure>, Error>
    where
        L: FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
        S: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
        W: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        let entries = self.permissions();
        let permissions =
            smtp_provider_delivery::Permissions::new(&entries).map_err(|_| Error::Shape)?;
        Ok(self.deliver_public_bound(store, now, &permissions, hooks))
    }
    fn deliver_public_bound<L, S, W, C, X>(
        &self,
        store: &mut crate::storage::Store,
        now: i64,
        permissions: &smtp_provider_delivery::Permissions<'_>,
        hooks: PublicDeliveryHooks<L, S, W, C, X>,
    ) -> Result<crate::delivery::BatchReport, crate::delivery::BatchFailure>
    where
        L: FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
        S: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
        W: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        let PublicDeliveryHooks {
            mut lookup,
            mut smtp_helper,
            webhook_helper,
            clock,
            cancelled,
        } = hooks;
        smtp_provider_delivery::deliver_due_public_mixed(
            store,
            self.common.observation.cluster(),
            now,
            self.common.controls.maintenance.delivery_limit,
            self.common.controls.maintenance.retry,
            smtp_provider_delivery::PublicMixedHooks {
                webhooks: &self.common.webhooks,
                helper: webhook_helper,
                smtp: smtp_provider_delivery::Hooks {
                    permissions,
                    resolve: |bound: &Descriptor| {
                        self.resolve(bound, &mut lookup)
                            .map_err(|error| match error {
                                Error::Credentials(_) => smtp_provider_delivery::Error::Credentials,
                                _ => smtp_provider_delivery::Error::Profile,
                            })
                    },
                    send: |item: &crate::storage::DueDelivery,
                           _: &crate::smtp_revision::Descriptor,
                           frozen: &crate::storage::FrozenMessage,
                           resolved| {
                        let timeout = self.timeout(&item.destination).map_err(|_| {
                            crate::supervised_provider::Diagnostic::SmtpConfiguration
                        })?;
                        crate::smtp_bound_transport::dispatch(
                            &mut smtp_helper(item),
                            resolved,
                            frozen,
                            timeout,
                        )
                    },
                    clock,
                    cancelled,
                },
            },
        )
    }
    pub(super) fn tick_public_with<Q, L, S, W, C, X>(
        &self,
        programs: (&std::path::Path, &std::path::Path),
        cycles: &mut crate::polling_cycle::Cycles,
        store: &mut crate::storage::Store,
        hooks: crate::polling_cycle::Hooks<Q, (L, S, W), C, X>,
    ) -> Result<crate::polling_cycle::TickReport, Error>
    where
        Q: FnMut(
            &mut std::process::Command,
            Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
        L: FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
        S: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
        W: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        let mut names: Vec<_> = self
            .common
            .webhooks
            .iter()
            .map(|w| w.alias.as_str())
            .collect();
        names.extend(self.smtp.iter().map(|s| s.name.as_str()));
        let entries = self.permissions();
        let permissions =
            smtp_provider_delivery::Permissions::new(&entries).map_err(|_| Error::Shape)?;
        let settings = crate::polling_cycle::Settings {
            active_program: programs.0,
            accounting_program: programs.1,
            cluster: self.common.observation.cluster(),
            user: self.common.observation.user(),
            timeout: self.common.controls.query_timeout,
            output_limit: self.common.controls.output_limit,
            emit_discovered: self.common.observation.emit_discovered(),
            capacity: self.common.controls.identity_limit,
            event_capacity: self.common.controls.event_limit,
            destinations: &names,
            window: self.common.controls.window,
        };
        let crate::polling_cycle::Hooks {
            mut query,
            send,
            clock,
            cancelled,
        } = hooks;
        Ok(cycles.tick_using_delivery(
            &settings,
            &self.common.controls.maintenance,
            store,
            crate::polling_cycle::Hooks {
                query: (),
                send,
                clock,
                cancelled,
            },
            || {
                let expected = self.common.routes.providers();
                (expected.len() == names.len()
                    && expected.iter().all(|name| names.contains(name))
                    && self
                        .common
                        .webhooks
                        .iter()
                        .all(|w| w.resources.public_revision().is_some()))
                .then_some(())
            },
            |cycles, store, now, _| {
                cycles.step_using(
                    &settings,
                    store,
                    false,
                    now,
                    |request, reconciler, store| {
                        self.poll_public_with(
                            config_composition::PollInput {
                                active_program: request.active_program,
                                accounting_program: request.accounting_program,
                                history_start: request.history_start,
                                history_end: request.history_end,
                                observed_at: now,
                            },
                            reconciler,
                            store,
                            &mut query,
                        )
                        .map_err(crate::polling_cycle::prepared_error)
                    },
                )
            },
            |store, now, (lookup, smtp_helper, webhook_helper), clock, cancelled| {
                self.deliver_public_bound(
                    store,
                    now,
                    &permissions,
                    PublicDeliveryHooks {
                        lookup,
                        smtp_helper,
                        webhook_helper,
                        clock,
                        cancelled,
                    },
                )
            },
        ))
    }
    // Each SMTP destination retains its configured transport deadline.
    pub(super) fn tick_with<Q, L, H, O, C, X>(
        &self,
        programs: (&std::path::Path, &std::path::Path),
        cycles: &mut crate::polling_cycle::Cycles,
        store: &mut crate::storage::Store,
        hooks: crate::polling_cycle::Hooks<Q, (L, H, O), C, X>,
    ) -> Result<crate::polling_cycle::TickReport, Error>
    where
        Q: FnMut(
            &mut std::process::Command,
            Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
        L: FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
        H: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
        O: FnMut(
            &crate::storage::DueDelivery,
        ) -> Result<
            crate::delivery::ProviderOutcome,
            crate::supervised_provider::Diagnostic,
        >,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        use crate::polling_cycle::{BoundSmtp, Hooks, MixedBoundSmtp, Rendering, Settings};
        let timeout = self.smtp.first().ok_or(Error::NoSmtp)?.timeout;
        let deadlines: Vec<_> = self
            .smtp
            .iter()
            .map(|s| (s.name.as_str(), s.timeout))
            .collect();
        let ordinary: Vec<_> = self
            .common
            .webhooks
            .iter()
            .map(|w| w.alias.as_str())
            .collect();
        let mut names = ordinary.clone();
        names.extend(self.smtp.iter().map(|s| s.name.as_str()));
        let routes: Vec<_> = self.smtp.iter().map(Smtp::route).collect();
        let profiles: Vec<_> = self.smtp.iter().map(|s| s.descriptor.profile()).collect();
        let entries = self.permissions();
        let permissions =
            smtp_provider_delivery::Permissions::new(&entries).map_err(|_| Error::Shape)?;
        let settings = Settings {
            active_program: programs.0,
            accounting_program: programs.1,
            cluster: self.common.observation.cluster(),
            user: self.common.observation.user(),
            timeout: self.common.controls.query_timeout,
            output_limit: self.common.controls.output_limit,
            emit_discovered: self.common.observation.emit_discovered(),
            capacity: self.common.controls.identity_limit,
            event_capacity: self.common.controls.event_limit,
            destinations: &names,
            window: self.common.controls.window,
        };
        let Hooks {
            query,
            send: (mut lookup, helper, ordinary_send),
            clock,
            cancelled,
        } = hooks;
        Ok(cycles.tick_selected_mixed_bound_smtp_deadlines(
            &settings,
            &self.common.controls.maintenance,
            store,
            (
                MixedBoundSmtp {
                    ordinary_destinations: &ordinary,
                    smtp: BoundSmtp {
                        rendering: Rendering {
                            routes: &routes,
                            max_messages: self.max_messages,
                        },
                        profiles: &profiles,
                        permissions: &permissions,
                        timeout,
                    },
                },
                Some(&deadlines),
            ),
            Hooks {
                query,
                send: (
                    |bound: &Descriptor| {
                        self.resolve(bound, &mut lookup)
                            .map_err(|error| match error {
                                Error::Credentials(_) => smtp_provider_delivery::Error::Credentials,
                                _ => smtp_provider_delivery::Error::Profile,
                            })
                    },
                    helper,
                    ordinary_send,
                ),
                clock,
                cancelled,
            },
            |event| self.common.routes.destinations(&event.state),
        ))
    }
    pub(super) fn poll_with(
        &self,
        input: config_composition::PollInput<'_>,
        reconciler: &mut crate::lifecycle::Reconciler,
        store: &mut crate::storage::Store,
        query: impl FnMut(
            &mut std::process::Command,
            Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
    ) -> Result<Vec<crate::lifecycle::Event>, crate::smtp_poll::Error> {
        self.poll_using(false, input, reconciler, store, query)
    }
    pub(super) fn poll_public_with(
        &self,
        input: config_composition::PollInput<'_>,
        reconciler: &mut crate::lifecycle::Reconciler,
        store: &mut crate::storage::Store,
        query: impl FnMut(
            &mut std::process::Command,
            Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
    ) -> Result<Vec<crate::lifecycle::Event>, crate::smtp_poll::Error> {
        self.poll_using(true, input, reconciler, store, query)
    }
    fn poll_using(
        &self,
        public: bool,
        input: config_composition::PollInput<'_>,
        reconciler: &mut crate::lifecycle::Reconciler,
        store: &mut crate::storage::Store,
        query: impl FnMut(
            &mut std::process::Command,
            Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
    ) -> Result<Vec<crate::lifecycle::Event>, crate::smtp_poll::Error> {
        let mut names: Vec<_> = self
            .common
            .webhooks
            .iter()
            .map(|w| w.alias.as_str())
            .collect();
        names.extend(self.smtp.iter().map(|s| s.name.as_str()));
        if public {
            let expected = self.common.routes.providers();
            if expected.len() != names.len() || expected.iter().any(|name| !names.contains(name)) {
                return Err(crate::smtp_poll::Error::InvalidInput);
            }
        }
        let routes: Vec<_> = self.smtp.iter().map(Smtp::route).collect();
        let profiles: Vec<_> = self.smtp.iter().map(|s| s.descriptor.profile()).collect();
        let rendering = smtp_routes::Plan::new(&names, &routes, self.max_messages)
            .map_err(|_| crate::smtp_poll::Error::Preparation)?;
        let provider = smtp_routes::ProviderPlan::new(&rendering, &profiles)
            .map_err(|_| crate::smtp_poll::Error::Preparation)?;
        let request = crate::scheduler::PollRequest {
            active_program: input.active_program,
            accounting_program: input.accounting_program,
            cluster: self.common.observation.cluster(),
            user: self.common.observation.user(),
            history_start: input.history_start,
            history_end: input.history_end,
            timeout: self.common.controls.query_timeout,
            output_limit: self.common.controls.output_limit,
            emit_discovered: self.common.observation.emit_discovered(),
        };
        let select =
            |event: &crate::lifecycle::Event| self.common.routes.destinations(&event.state);
        if public {
            provider.poll_public_selected_with(
                &request,
                reconciler,
                store,
                (input.observed_at, &self.common.webhooks),
                query,
                select,
            )
        } else {
            provider.poll_selected_with(
                &request,
                reconciler,
                store,
                input.observed_at,
                query,
                select,
            )
        }
    }
}
// Read once; owned composition survives subsequent path replacement. No ambient lookups.
pub(super) fn prepare_path(
    path: &std::path::Path,
    identity: impl FnOnce(usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
) -> Result<Plan, Error> {
    let snapshot = crate::config_snapshot::read(path).map_err(Error::Snapshot)?;
    prepare(snapshot.bytes(), identity, lookup)
}

pub(super) fn prepare(
    bytes: &[u8],
    identity: impl FnOnce(usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
) -> Result<Plan, Error> {
    let draft = config_validate::validate(bytes).map_err(Error::Validation)?;
    let shape = |_| Error::Shape;
    let root = object(&draft.value).map_err(shape)?;
    let providers = object(get(root, "providers").map_err(shape)?).map_err(shape)?;
    let names: Vec<_> = providers.iter().map(|(name, _)| name.as_str()).collect();
    let mut entries = Vec::new();
    let mut routes = Vec::new();
    for (name, value) in providers {
        let p = object(value).map_err(shape)?;
        if text(get(p, "type").map_err(shape)?).map_err(shape)? != "smtp" {
            continue;
        }
        let visibility = if text(get(p, "visibility").map_err(shape)?).map_err(shape)? == "hidden" {
            JobVisibility::Hidden
        } else {
            JobVisibility::Identifier
        };
        routes.push(smtp_routes::Route {
            name,
            visibility,
            metadata: smtp_routes::Metadata {
                sender: text(get(p, "sender").map_err(shape)?).map_err(shape)?,
                recipient: text(get(p, "recipient").map_err(shape)?).map_err(shape)?,
                namespace: text(get(p, "identity_namespace").map_err(shape)?).map_err(shape)?,
            },
        });
        entries.push(p);
    }
    if routes.is_empty() {
        return Err(Error::NoSmtp);
    }
    let delivery = object(get(root, "delivery").map_err(shape)?).map_err(shape)?;
    let integer = |p, key| match get(p, key).map_err(shape)? {
        crate::config_decode::Value::Integer(n) => Ok(*n),
        _ => Err(Error::Shape),
    };
    let max_messages = usize::try_from(integer(delivery, "prepared_smtp_message_limit")?)
        .map_err(|_| Error::Shape)?;
    // Validate all rendering metadata before protected trust reads or injected lookups.
    let rendering =
        smtp_routes::Plan::new(&names, &routes, max_messages).map_err(|_| Error::Rendering)?;
    let mut smtp = Vec::new();
    for (route, p) in routes.iter().zip(entries) {
        let roots = list(get(p, "trust_der_files").map_err(shape)?, 1, 32)
            .map_err(shape)?
            .iter()
            .map(text)
            .collect::<Result<Vec<_>, _>>()
            .map_err(shape)?;
        let auth = object(get(p, "authentication").map_err(shape)?).map_err(shape)?;
        let username = text(get(auth, "username_env").map_err(shape)?).map_err(shape)?;
        let password = text(get(auth, "password_env").map_err(shape)?).map_err(shape)?;
        let trust_reference = reference(b"trust\0", &roots);
        let credential_reference = reference(b"credentials\0", &[username, password]);
        let profile = |fingerprint| -> Result<Descriptor, Error> {
            Descriptor::new(&Profile {
                destination: route.name,
                relay: text(get(p, "relay").map_err(shape)?).map_err(shape)?,
                port: u16::try_from(integer(p, "port")?).map_err(|_| Error::Shape)?,
                tls: TlsMode::RequiredStartTls,
                server_name: text(get(p, "server_name").map_err(shape)?).map_err(shape)?,
                trust_reference: &trust_reference,
                trust_fingerprint: fingerprint,
                authentication_identity: text(get(auth, "identity").map_err(shape)?)
                    .map_err(shape)?,
                credential_reference: &credential_reference,
            })
            .map_err(|_| Error::Profile)
        };
        let placeholder_fingerprint = "0".repeat(64);
        profile(&placeholder_fingerprint)?;
        let trust = trust_snapshot::load(&roots.iter().map(PathBuf::from).collect::<Vec<_>>())
            .map_err(Error::Trust)?;
        let descriptor = profile(trust.fingerprint())?;
        let revision = crate::smtp_revision::Descriptor::from_revision_plan(&rendering, route.name)
            .map_err(|_| Error::Rendering)?;
        smtp.push(Smtp {
            name: Zeroizing::new(route.name.into()),
            sender: Zeroizing::new(route.metadata.sender.into()),
            recipient: Zeroizing::new(route.metadata.recipient.into()),
            namespace: Zeroizing::new(route.metadata.namespace.into()),
            visibility: route.visibility,
            descriptor,
            rendering: revision,
            trust,
            username: Zeroizing::new(username.into()),
            password: Zeroizing::new(password.into()),
            timeout: Duration::from_millis(integer(p, "request_timeout_milliseconds")?),
        });
    }
    let mut permissions = Vec::new();
    for value in list(get(delivery, "smtp_permissions").map_err(shape)?, 0, 64).map_err(shape)? {
        let p = object(value).map_err(shape)?;
        let values = [
            "cluster_id",
            "destination",
            "rendering_revision",
            "provider_revision",
        ]
        .map(|key| {
            get(p, key)
                .and_then(text)
                .map(|s| Zeroizing::new(s.to_owned()))
        });
        let [a, b, c, d] = values;
        permissions.push([
            a.map_err(shape)?,
            b.map_err(shape)?,
            c.map_err(shape)?,
            d.map_err(shape)?,
        ]);
    }
    let common =
        config_composition::prepare_draft(draft, true, identity, lookup).map_err(Error::Common)?;
    let plan = Plan {
        common,
        smtp,
        permissions,
        max_messages,
    };
    smtp_provider_delivery::Permissions::new(&plan.permissions()).map_err(|_| Error::Shape)?;
    Ok(plan)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod tests {
    use super::*;
    use crate::{lifecycle::Reconciler, storage::Store};
    use serde_json::{json, Value};
    use std::{io::Write, os::unix::fs::OpenOptionsExt, path::Path};
    fn fixture(directory: &crate::storage::tests::Directory) -> Value {
        let root = directory
            .path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("root.der");
        let cert = rcgen::generate_simple_self_signed(vec!["smtp.example.invalid".into()]).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&root)
            .unwrap()
            .write_all(cert.cert.der())
            .unwrap();
        let mut v: Value = serde_json::from_slice(include_bytes!(
            "../../../docs/proposals/examples/local-mixed-config.json"
        ))
        .unwrap();
        v["storage"]["path"] = json!(directory.path());
        v["providers"]["research_email"]["trust_der_files"] = json!([root]);
        v["monitor"]["initial_observation"] = json!("emit");
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["research_email"]}]);
        v
    }
    fn prepare_fixture(v: &Value) -> Plan {
        prepare(
            &serde_json::to_vec(v).unwrap(),
            |_| Ok(Zeroizing::new(b"alice".to_vec())),
            |name, _| {
                assert!(!name.contains("SMTP"));
                Ok(Zeroizing::new(if name.ends_with("_URL") {
                    b"https://fixture.invalid/hook".to_vec()
                } else {
                    b"Bearer fixture".to_vec()
                }))
            },
        )
        .unwrap()
    }

    fn replace_config(path: &Path, bytes: &[u8]) {
        let next = path.with_extension("next");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&next)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        std::fs::rename(next, path).unwrap();
    }
    fn config_path(d: &crate::storage::tests::Directory) -> PathBuf {
        d.path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("config.json")
    }
    fn prepare_file_fixture(v: &Value, path: &Path) -> Plan {
        replace_config(path, &serde_json::to_vec(v).unwrap());
        prepare_path(
            path,
            |_| Ok(Zeroizing::new(b"alice".to_vec())),
            |name, _| {
                assert!(!name.contains("SMTP"));
                Ok(Zeroizing::new(if name.ends_with("_URL") {
                    b"https://fixture.invalid/hook".to_vec()
                } else {
                    b"Bearer fixture".to_vec()
                }))
            },
        )
        .unwrap()
    }

    fn owned_path(d: &crate::storage::tests::Directory) -> PathBuf {
        config_path(d)
            .parent()
            .unwrap()
            .join("owned-state")
            .join("state.sqlite3")
    }
    fn open_owned_fixture(plan: &Plan) -> Store {
        let until = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            match plan.open_owned(Duration::from_millis(50)) {
                Ok(store) => return store,
                Err(crate::storage::StorageError::StatePath(
                    crate::state_path::Error::AlreadyOwned,
                )) if std::time::Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error) => panic!("owned fixture acquisition failed: {error:?}"),
            }
        }
    }
    fn start_session(path: &Path) -> crate::foreground_session::Session {
        let until = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            let result = crate::foreground_session::Session::start(
                path,
                2,
                Duration::from_millis(50),
                |_| Ok(Zeroizing::new(b"alice".to_vec())),
                |name, _| {
                    Ok(Zeroizing::new(if name.ends_with("_URL") {
                        b"https://fixture.invalid/hook".to_vec()
                    } else {
                        b"Bearer fixture".to_vec()
                    }))
                },
                || false,
            );
            match result {
                Ok(session) => return session,
                Err(crate::foreground_session::Error::Ownership)
                    if std::time::Instant::now() < until =>
                {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error) => panic!("session fixture startup failed: {error:?}"),
            }
        }
    }
    #[test]
    fn local_os_adapter_uses_effective_identity_environment_and_synthetic_scheduler_children() {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["primary","research_email"]}]);
        let path = config_path(&d);
        replace_config(&path, &serde_json::to_vec(&v).unwrap());
        let user = crate::effective_user::lookup(256).unwrap();
        let user = std::str::from_utf8(&user).unwrap();
        let active = path.with_extension("squeue");
        let accounting = path.with_extension("sacct");
        for (program,body) in [(&active,"#!/bin/sh\nexit 0\n".to_owned()),
            (&accounting,"#!/bin/sh\nprintf 'research-cluster|1|%s|s|a|COMPLETED|0:0\\n' \"$SLURM_NOTIFY_LOCAL_EXPECTED_USER\"\n".to_owned())] {
            std::fs::OpenOptions::new().write(true).create_new(true).mode(0o700)
                .open(program).unwrap().write_all(body.as_bytes()).unwrap();
        }
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "mixed_config_composition::tests::local_os_adapter_child",
                "--ignored",
                "--nocapture",
            ])
            .env("SLURM_NOTIFY_LOCAL_EXPECTED_USER", user)
            .env("SLURM_NOTIFY_LOCAL_CHILD_CONFIG", &path)
            .env("SLURM_NOTIFY_LOCAL_CHILD_ACTIVE", &active)
            .env("SLURM_NOTIFY_LOCAL_CHILD_ACCOUNTING", &accounting)
            .env("SLURM_NOTIFY_WEBHOOK_URL", "https://fixture.invalid/hook")
            .env("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION", "Bearer fixture");
        let output = crate::scheduler::run(&mut command, Duration::from_secs(10), 8192).unwrap();
        assert!(output.contains("local OS adapter accepted"));
    }
    #[test]
    #[ignore = "isolated OS environment/process helper invoked by parent acceptance"]
    fn local_os_adapter_child() {
        let path = PathBuf::from(std::env::var_os("SLURM_NOTIFY_LOCAL_CHILD_CONFIG").unwrap());
        let active = PathBuf::from(std::env::var_os("SLURM_NOTIFY_LOCAL_CHILD_ACTIVE").unwrap());
        let accounting =
            PathBuf::from(std::env::var_os("SLURM_NOTIFY_LOCAL_CHILD_ACCOUNTING").unwrap());
        let signal = crate::foreground_wait::Signal::new();
        let summary = crate::foreground_runtime::run_local_continuous(
            crate::foreground_runtime::ContinuousRequest {
                path: &path,
                poll_batch: 1,
                lock_timeout: Duration::from_millis(50),
            },
            &signal,
            (&active, &accounting),
            (
                |_: &crate::storage::DueDelivery| -> std::process::Command {
                    panic!("cancelled SMTP helper")
                },
                |_: &crate::storage::DueDelivery| {
                    signal.cancel().unwrap();
                    let mut command = std::process::Command::new("python3");
                    command.args(["-c","import sys,struct,json; b=sys.stdin.buffer.read(); f=[]\nwhile b:\n n=struct.unpack('>I',b[:4])[0]; f.append(b[4:4+n]); b=b[4+n:]\nassert len(f)==5; assert f[0]==b'https://fixture.invalid/hook'; assert f[1]==b'Bearer fixture'; assert json.loads(f[3])['event_id']==f[2].decode(); sys.stdout.write('OK\\n')"]);
                    command
                },
            ),
        );
        assert_eq!(summary.render(), "foreground outcome=cancelled ticks=1\n");
        let plan = prepare_path(
            &path,
            crate::effective_user::lookup,
            crate::environment_resolution::lookup,
        )
        .unwrap();
        let store = open_owned_fixture(&plan);
        let now = crate::delivery::unix_time().unwrap();
        let due = store.due_deliveries("research-cluster", now, 10).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].destination, "research_email");
        assert_eq!(due[0].attempt_number, 1);
        assert!(store
            .public_delivery(
                &due[0].event_id,
                "primary",
                plan.common.webhooks[0].resources.public_revision().unwrap()
            )
            .unwrap()
            .is_some());
        assert!(store
            .frozen_smtp(&due[0].event_id, "research_email")
            .unwrap()
            .is_some());
        println!("local OS adapter accepted");
    }

    #[test]
    fn public_continuous_orchestration_cancels_after_durable_poll_and_releases_owner() {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["primary","research_email"]}]);
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        let signal = crate::foreground_wait::Signal::new();
        let summary = crate::foreground_runtime::run_continuous(
            crate::foreground_runtime::ContinuousRequest {
                path: &path,
                poll_batch: 1,
                lock_timeout: Duration::from_millis(50),
            },
            &signal,
            crate::foreground_runtime::Hooks {
                identity: |_| Ok(Zeroizing::new(b"alice".to_vec())),
                lookup: |name: &str, _| {
                    Ok(Zeroizing::new(if name.ends_with("_URL") {
                        b"https://fixture.invalid/hook".to_vec()
                    } else {
                        b"Bearer fixture".to_vec()
                    }))
                },
                tick: |session: &mut crate::foreground_session::Session,
                       signal: &crate::foreground_wait::Signal| {
                    std::fs::remove_file(&path).unwrap();
                    session.tick_public_with(
                        (Path::new("squeue"), Path::new("sacct")),
                        crate::polling_cycle::Hooks {
                            query: |command: &mut std::process::Command, _, _| {
                                Ok(if command.get_program() == "squeue" {
                                    ""
                                } else {
                                    signal.cancel().unwrap();
                                    "research-cluster|1|alice|s|a|COMPLETED|0:0"
                                }
                                .into())
                            },
                            send: (
                                |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                                    panic!("cancelled credentials")
                                },
                                |_: &crate::storage::DueDelivery| -> std::process::Command {
                                    panic!("cancelled SMTP")
                                },
                                |_: &crate::storage::DueDelivery| -> std::process::Command {
                                    panic!("cancelled webhook")
                                },
                            ),
                            clock: || Some(7200),
                            cancelled: || signal.cancelled().unwrap_or(true),
                        },
                    )
                },
            },
        );
        assert_eq!(summary.render(), "foreground outcome=cancelled ticks=1\n");
        let store = open_owned_fixture(&plan);
        let due = store.due_deliveries("research-cluster", 7200, 10).unwrap();
        assert_eq!(due.len(), 2);
        assert!(due.iter().all(|item| item.attempt_number == 1));
        assert!(store
            .public_delivery(
                &due[0].event_id,
                "primary",
                plan.common.webhooks[0].resources.public_revision().unwrap()
            )
            .unwrap()
            .is_some());
    }

    #[test]
    fn public_session_cancel_after_commit_closes_owner_and_leaves_both_deliveries_pending() {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["primary","research_email"]}]);
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        let mut session = start_session(&path);
        let cancel = std::cell::Cell::new(false);
        let run = |session: &mut crate::foreground_session::Session| {
            session.tick_public_with(
                (Path::new("squeue"), Path::new("sacct")),
                crate::polling_cycle::Hooks {
                    query: |command: &mut std::process::Command, _, _| {
                        Ok(if command.get_program() == "squeue" {
                            ""
                        } else {
                            cancel.set(true);
                            "research-cluster|1|alice|s|a|COMPLETED|0:0"
                        }
                        .into())
                    },
                    send: (
                        |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                            panic!("cancelled credentials")
                        },
                        |_: &crate::storage::DueDelivery| -> std::process::Command {
                            panic!("cancelled SMTP")
                        },
                        |_: &crate::storage::DueDelivery| -> std::process::Command {
                            panic!("cancelled webhook")
                        },
                    ),
                    clock: || Some(7200),
                    cancelled: || cancel.get(),
                },
            )
        };
        assert!(run(&mut session).unwrap().cancelled);
        assert!(matches!(
            run(&mut session),
            Err(crate::foreground_session::Error::Closed)
        ));
        let store = open_owned_fixture(&plan);
        let due = store.due_deliveries("research-cluster", 7200, 10).unwrap();
        assert_eq!(due.len(), 2);
        assert!(due.iter().all(|item| item.attempt_number == 1));
        assert!(store
            .public_delivery(
                &due[0].event_id,
                "primary",
                plan.common.webhooks[0].resources.public_revision().unwrap()
            )
            .unwrap()
            .is_some());
        assert!(store
            .frozen_smtp(&due[0].event_id, "research_email")
            .unwrap()
            .is_some());
    }

    #[test]
    fn public_continuous_session_retains_owner_and_prepared_resources_across_poll_renewal() {
        use crate::{
            foreground_schedule::{self, Wake},
            polling_cycle::Hooks,
        };
        use std::cell::{Cell, RefCell};
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["primary","research_email"]}]);
        v["delivery"]["jitter_seconds"] = json!(0);
        let initial = prepare_fixture(&v);
        let (_, rendering, provider) = initial.revisions()[0];
        v["delivery"]["smtp_permissions"][0]["rendering_revision"] = json!(rendering);
        v["delivery"]["smtp_permissions"][0]["provider_revision"] = json!(provider);
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        let mut session = start_session(&path);
        let ticks = Cell::new(0);
        let mono = Cell::new(Duration::ZERO);
        let cancel = Cell::new(false);
        let event_id = RefCell::new(String::new());
        let helper = |ack: &str| {
            let mut command = std::process::Command::new("python3");
            command.args([
                "-c",
                &format!(
                    "import sys; assert sys.stdin.buffer.read(); sys.stdout.write('{ack}\\n')"
                ),
            ]);
            command
        };
        let exit = session.run_continuous_with(1,foreground_schedule::Hooks {
            tick: |session: &mut crate::foreground_session::Session| {
                let tick = ticks.get()+1;
                ticks.set(tick);
                assert!(matches!(plan.open_owned(Duration::from_millis(10)),
                    Err(crate::storage::StorageError::StatePath(crate::state_path::Error::AlreadyOwned))));
                if tick==1 { std::fs::remove_file(&path).unwrap(); }
                let report = session.tick_public_with((Path::new("squeue"),Path::new("sacct")),Hooks {
                    query: |command: &mut std::process::Command,_,_| Ok(if command.get_program()=="squeue" { "" }
                        else { "research-cluster|1|alice|s|a|COMPLETED|0:0" }.into()),
                    send: (
                        |name: &str,_| Ok(Zeroizing::new(if name.ends_with("USERNAME") { b"researcher".to_vec() } else { b"fixture-password".to_vec() })),
                        |item: &crate::storage::DueDelivery| { assert_eq!(tick,1); *event_id.borrow_mut()=item.event_id.clone(); helper("SMTP_SUBMIT FINAL ACCEPTED RA") },
                        |item: &crate::storage::DueDelivery| { assert!(tick<=2); *event_id.borrow_mut()=item.event_id.clone(); helper(if tick==1 { "RETRY" } else { "OK" }) },
                    ),clock: || Some(7200+(tick-1)*60),cancelled: || false,
                })?;
                assert!(matches!(report.poll,Some(Ok(crate::polling_cycle::Outcome::Committed { events,.. })) if events==usize::from(tick==1)));
                let delivery = report.delivery.as_ref().unwrap().as_ref().unwrap();
                assert_eq!(delivery.delivered,usize::from(tick<=2));
                assert_eq!(delivery.retrying,usize::from(tick==1));
                assert!(session.status(7200+(tick-1)*60).unwrap().poll.last_failure.is_none());
                Ok(report)
            },monotonic: || Some(mono.get()),
            wait: |remaining| { mono.set(mono.get()+remaining); if ticks.get()==3 { cancel.set(true); Ok(Wake::Cancelled) } else { Ok(Wake::Ready) } },
            cancelled: || cancel.get(),
        }).unwrap();
        assert_eq!(exit, foreground_schedule::Exit::Cancelled { ticks: 3 });
        assert!(matches!(
            session.status(7400),
            Err(crate::foreground_session::Error::Closed)
        ));
        let mut store = open_owned_fixture(&plan);
        assert!(store
            .due_deliveries("research-cluster", 7400, 10)
            .unwrap()
            .is_empty());
        let frame = store
            .public_delivery(
                &event_id.borrow(),
                "primary",
                plan.common.webhooks[0].resources.public_revision().unwrap(),
            )
            .unwrap()
            .unwrap();
        assert!(crate::public_event::valid_id(&frame.id));
        assert!(store
            .frozen_smtp(&event_id.borrow(), "research_email")
            .unwrap()
            .is_some());
        assert_eq!(
            store
                .restore("research-cluster", 10)
                .unwrap()
                .0
                .emitted
                .len(),
            1
        );
    }

    #[test]
    fn foreground_startup_rejects_before_resources_and_releases_cancelled_owner() {
        use crate::foreground_session::{Error as E, Session};
        let d = crate::storage::tests::Directory::new();
        let path = config_path(&d);
        for (limit, cancel, expected) in [
            (0, false, E::InvalidLimit),
            (1, true, E::Cancelled),
            (1, false, E::Preparation),
        ] {
            assert!(
                matches!(Session::start(&path, limit, Duration::from_millis(50),
                |_| panic!("identity"), |_, _| panic!("lookup"), || cancel), Err(error) if error == expected)
            );
        }
        let mut v = fixture(&d);
        let database = owned_path(&d);
        v["storage"]["path"] = json!(database);
        drop(prepare_file_fixture(&v, &path));
        for sample in [2, 3] {
            let mut calls = 0;
            assert!(matches!(
                Session::start(
                    &path,
                    2,
                    Duration::from_millis(50),
                    |_| Ok(Zeroizing::new(b"alice".to_vec())),
                    |name, _| Ok(Zeroizing::new(if name.ends_with("_URL") {
                        b"https://fixture.invalid/hook".to_vec()
                    } else {
                        b"Bearer fixture".to_vec()
                    })),
                    || {
                        calls += 1;
                        calls == sample
                    }
                ),
                Err(E::Cancelled)
            ));
            assert_eq!(database.exists(), sample == 3);
        }
        let mut session = start_session(&path);
        assert!(session.status(7200).unwrap().poll.stale);
        session.stop();
        session.stop();
        assert!(matches!(session.status(7200), Err(E::Closed)));
        drop(start_session(&path));
    }
    #[test]
    fn foreground_session_owns_plan_and_preserves_pending_work_through_cancel_restart() {
        use crate::{
            foreground_session::Error as E,
            polling_cycle::{Hooks, StageError},
            storage::DueDelivery,
        };
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        let path = config_path(&d);
        drop(prepare_file_fixture(&v, &path));
        let mut session = start_session(&path);
        assert!(matches!(session.status(-1), Err(E::Status)));
        assert!(matches!(
            crate::foreground_session::Session::start(
                &path,
                1,
                Duration::from_millis(50),
                |_| Ok(Zeroizing::new(b"alice".to_vec())),
                |name, _| Ok(Zeroizing::new(if name.ends_with("_URL") {
                    b"https://fixture.invalid/hook".to_vec()
                } else {
                    b"Bearer fixture".to_vec()
                })),
                || false,
            ),
            Err(E::Ownership)
        ));
        let status = crate::status_composition::prepare_path(&path).unwrap();
        assert!(matches!(
            status.inspect(
                &crate::polling_cycle::Cycles::new(1).unwrap(),
                7200,
                Duration::from_millis(50)
            ),
            Err(crate::status_composition::InspectionError::State(
                crate::state_path::Error::AlreadyOwned
            ))
        ));
        // The running session owns its prepared plan even after configuration disappears.
        std::fs::remove_file(&path).unwrap();
        let run = |session: &mut crate::foreground_session::Session,
                   now: i64,
                   cancelled: bool,
                   fail: bool| {
            session.tick_with(
                (Path::new("squeue"), Path::new("sacct")),
                Hooks {
                    query: |command: &mut std::process::Command, _, _| {
                        assert!(!cancelled);
                        if fail {
                            return Err(crate::scheduler::QueryError::Timeout);
                        }
                        Ok(if command.get_program() == "squeue" {
                            ""
                        } else {
                            "research-cluster|1|alice|s|actual|COMPLETED|0:0"
                        }
                        .into())
                    },
                    send: (
                        |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> { panic!("denied secret") },
                        |_: &DueDelivery| -> std::process::Command { panic!("denied helper") },
                        |_: &DueDelivery| -> Result<
                            crate::delivery::ProviderOutcome,
                            crate::supervised_provider::Diagnostic,
                        > { panic!("ordinary") },
                    ),
                    clock: || {
                        assert!(!cancelled);
                        Some(now)
                    },
                    cancelled: || cancelled,
                },
            )
        };
        let first = run(&mut session, 7200, false, false).unwrap();
        assert!(matches!(
            first.delivery,
            Some(Err(StageError::DispatchBlocked))
        ));
        assert!(!session.status(7200).unwrap().poll.stale);
        let failed = run(&mut session, 7201, false, true).unwrap();
        assert!(matches!(failed.poll, Some(Err(StageError::Poll))));
        assert_eq!(session.status(7201).unwrap().poll.consecutive_failures, 1);
        assert!(run(&mut session, 7202, true, false).unwrap().cancelled);
        assert!(matches!(session.status(7202), Err(E::Closed)));
        assert!(matches!(
            run(&mut session, 7202, true, false),
            Err(E::Closed)
        ));
        let plan = prepare_file_fixture(&v, &path);
        let store = open_owned_fixture(&plan);
        let pending = store.due_deliveries("research-cluster", 7202, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].attempt_number, 1);
        let id = pending[0].event_id.clone();
        let frozen = store
            .frozen_smtp(&id, "research_email")
            .unwrap()
            .unwrap()
            .data()
            .to_vec();
        drop(store);
        let mut restarted = start_session(&path);
        assert_eq!(restarted.status(7202).unwrap().poll.consecutive_failures, 0);
        let monotonic = std::cell::Cell::new(Duration::ZERO);
        let mut ticks = 0;
        let mut waits = 0;
        let scheduled = restarted
            .run_with(
                2,
                crate::foreground_schedule::Hooks {
                    tick: |session: &mut crate::foreground_session::Session| {
                        let replay = run(session, 7202, false, false)?;
                        assert!(matches!(
                            replay.poll,
                            Some(Ok(crate::polling_cycle::Outcome::Committed {
                                events: 0,
                                ..
                            }))
                        ));
                        assert!(matches!(
                            replay.delivery,
                            Some(Err(StageError::DispatchBlocked))
                        ));
                        ticks += 1;
                        Ok(replay)
                    },
                    monotonic: || Some(monotonic.get()),
                    wait: |remaining| {
                        waits += 1;
                        monotonic.set(monotonic.get() + remaining);
                        Ok(crate::foreground_schedule::Wake::Ready)
                    },
                    cancelled: || false,
                },
            )
            .unwrap();
        assert_eq!(
            scheduled,
            crate::foreground_schedule::Exit::Exhausted { ticks: 2 }
        );
        assert_eq!((ticks, waits), (2, 1));
        assert!(matches!(restarted.status(7202), Err(E::Closed)));
        drop(restarted);
        let store = open_owned_fixture(&plan);
        assert_eq!(
            store
                .frozen_smtp(&id, "research_email")
                .unwrap()
                .unwrap()
                .data(),
            frozen
        );
        assert_eq!(
            store.due_deliveries("research-cluster", 7202, 10).unwrap()[0].attempt_number,
            1
        );
    }
    #[test]
    fn scheduled_session_wait_cancellation_and_errors_release_ownership() {
        use crate::foreground_schedule::{Error as E, Exit, Hooks, Wake};
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        for case in 0..4 {
            let mut session = start_session(&path);
            let mut ticks = 0;
            let result = session.run_with(
                if case == 0 { 0 } else { 2 },
                Hooks {
                    tick: |_: &mut crate::foreground_session::Session| {
                        ticks += 1;
                        Ok(crate::polling_cycle::TickReport::default())
                    },
                    monotonic: || {
                        if case == 1 {
                            None
                        } else {
                            Some(Duration::ZERO)
                        }
                    },
                    wait: |remaining| {
                        assert_eq!(remaining, plan.poll_interval());
                        if case == 3 {
                            Err(())
                        } else {
                            Ok(Wake::Cancelled)
                        }
                    },
                    cancelled: || false,
                },
            );
            assert_eq!(
                result,
                match case {
                    0 => Err(E::Policy),
                    1 => Err(E::Clock),
                    2 => Ok(Exit::Cancelled { ticks: 1 }),
                    _ => Err(E::Wait),
                }
            );
            assert_eq!(ticks, usize::from(case >= 2));
            assert!(matches!(
                session.status(7200),
                Err(crate::foreground_session::Error::Closed)
            ));
            assert_eq!(
                session.run_with(
                    1,
                    Hooks {
                        tick: |_: &mut crate::foreground_session::Session| panic!("closed tick"),
                        monotonic: || panic!("closed clock"),
                        wait: |_| panic!("closed wait"),
                        cancelled: || panic!("closed cancellation"),
                    }
                ),
                Err(E::Tick)
            );
            drop(open_owned_fixture(&plan));
        }
    }
    #[test]
    fn interruptible_session_cancels_between_ticks_and_reopens_owned_state() {
        use crate::{foreground_schedule::Exit, foreground_wait::Signal};
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        let signal = Signal::new();
        let mut session = start_session(&path);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker_signal = signal.clone();
        let worker = std::thread::spawn(move || {
            let result = session.run_interruptible(2, &worker_signal, |session| {
                assert!(session.status(7200).unwrap().poll.stale);
                ready_tx.send(()).unwrap();
                Ok(crate::polling_cycle::TickReport::default())
            });
            let closed = matches!(
                session.status(7200),
                Err(crate::foreground_session::Error::Closed)
            );
            done_tx.send((result, closed)).unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        signal.cancel().unwrap();
        let (result, closed) = done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(result, Ok(Exit::Cancelled { ticks: 1 }));
        assert!(closed);
        worker.join().unwrap();
        drop(open_owned_fixture(&plan));
        // A fresh session sharing the sticky signal runs no tick and releases ownership.
        let mut restarted = start_session(&path);
        assert_eq!(
            restarted.run_interruptible(2, &signal, |_| panic!("cancelled tick")),
            Ok(Exit::Cancelled { ticks: 0 })
        );
        drop(open_owned_fixture(&plan));
        assert_eq!(
            restarted.run_interruptible(1, &Signal::new(), |_| panic!("closed tick")),
            Err(crate::foreground_schedule::Error::Tick)
        );
    }
    #[test]
    fn shared_tick_cancellation_after_query_preserves_frozen_work_for_restart() {
        use crate::{
            foreground_schedule::Exit, foreground_wait::Signal, polling_cycle::Hooks,
            storage::DueDelivery,
        };
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        let signal = Signal::new();
        let mut session = start_session(&path);
        let mut queries = 0;
        let outcome = session
            .run_interruptible(2, &signal, |session| {
                let report = session.tick_interruptible(
                    (Path::new("squeue"), Path::new("sacct")),
                    Hooks {
                        query: |command: &mut std::process::Command, _, _| {
                            queries += 1;
                            if command.get_program() == "sacct" {
                                signal.cancel().unwrap();
                                Ok("research-cluster|1|alice|s|actual|COMPLETED|0:0".into())
                            } else {
                                Ok(String::new())
                            }
                        },
                        send: (
                            |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                                panic!("cancelled resolution")
                            },
                            |_: &DueDelivery| -> std::process::Command {
                                panic!("cancelled helper")
                            },
                            |_: &DueDelivery| -> Result<
                                crate::delivery::ProviderOutcome,
                                crate::supervised_provider::Diagnostic,
                            > { panic!("cancelled send") },
                        ),
                        clock: || Some(7200),
                        cancelled: &signal,
                    },
                )?;
                assert!(report.cancelled);
                assert!(matches!(
                    report.poll,
                    Some(Ok(crate::polling_cycle::Outcome::Committed {
                        events: 1,
                        ..
                    }))
                ));
                assert!(report.delivery.is_none());
                assert!(report.cleanup.is_none());
                Ok(report)
            })
            .unwrap();
        assert_eq!(outcome, Exit::Cancelled { ticks: 1 });
        assert_eq!(queries, 2);
        let store = open_owned_fixture(&plan);
        let due = store.due_deliveries("research-cluster", 7200, 10).unwrap();
        assert_eq!(due.len(), 1);
        let frozen = store
            .frozen_smtp(&due[0].event_id, "research_email")
            .unwrap()
            .unwrap()
            .data()
            .to_vec();
        assert_eq!(due[0].attempt_number, 1);
        drop(store);
        let mut restarted = start_session(&path);
        // Sticky cancellation is also authoritative for direct ticks before any hook.
        let report = restarted
            .tick_interruptible(
                (Path::new("squeue"), Path::new("sacct")),
                Hooks {
                    query: |_: &mut std::process::Command, _, _| panic!("cancelled query"),
                    send: (
                        |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> { panic!("resolution") },
                        |_: &DueDelivery| -> std::process::Command { panic!("helper") },
                        |_: &DueDelivery| -> Result<
                            crate::delivery::ProviderOutcome,
                            crate::supervised_provider::Diagnostic,
                        > { panic!("send") },
                    ),
                    clock: || panic!("cancelled clock"),
                    cancelled: &signal,
                },
            )
            .unwrap();
        assert!(report.cancelled && report.poll.is_none());
        let store = open_owned_fixture(&plan);
        assert_eq!(
            store
                .frozen_smtp(&due[0].event_id, "research_email")
                .unwrap()
                .unwrap()
                .data(),
            frozen
        );
    }
    #[test]
    fn shared_tick_cancellation_after_delivery_records_progress_and_leaves_remaining_work() {
        use crate::{foreground_wait::Signal, polling_cycle::Hooks, storage::DueDelivery};
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        v["routes"] = json!([{"events":["JOB_FAILED"],"send_to":["primary"]}]);
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        let signal = Signal::new();
        let mut session = start_session(&path);
        let mut sends = 0;
        let run = |session: &mut crate::foreground_session::Session,
                   signal: &Signal,
                   cancel: bool,
                   sends: &mut usize| {
            session.tick_interruptible((Path::new("squeue"), Path::new("sacct")), Hooks {
                query: |command: &mut std::process::Command, _, _| Ok(if command.get_program() == "squeue" { "" } else {
                    "research-cluster|1|alice|s|actual|FAILED|1:0\nresearch-cluster|2|alice|s|actual|FAILED|1:0"
                }.into()),
                send: (|_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> { panic!("SMTP resolution") },
                    |_: &DueDelivery| -> std::process::Command { panic!("SMTP helper") },
                    |due: &DueDelivery| { assert_eq!(due.destination, "primary"); *sends += 1;
                        if cancel { signal.cancel().unwrap(); }
                        Ok(crate::delivery::ProviderOutcome::Delivered) }),
                clock: || Some(7200), cancelled: signal,
            }).unwrap()
        };
        let first = run(&mut session, &signal, true, &mut sends);
        assert!(first.cancelled);
        assert_eq!(sends, 1);
        assert!(matches!(
            first.delivery,
            Some(Err(crate::polling_cycle::StageError::Cancelled))
        ));
        assert_eq!(first.delivery_failure.unwrap().committed.delivered, 1);
        assert!(first.cleanup.is_none());
        let store = open_owned_fixture(&plan);
        let pending = store.due_deliveries("research-cluster", 7200, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].attempt_number, 1);
        drop(store);
        let mut restarted = start_session(&path);
        let second = run(&mut restarted, &Signal::new(), false, &mut sends);
        assert!(!second.cancelled);
        assert!(matches!(
            second.poll,
            Some(Ok(crate::polling_cycle::Outcome::Committed {
                events: 0,
                ..
            }))
        ));
        assert_eq!(second.delivery.unwrap().unwrap().delivered, 1);
        assert_eq!(sends, 2);
        drop(restarted);
        let store = open_owned_fixture(&plan);
        assert!(store
            .due_deliveries("research-cluster", 7200, 10)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn reported_shutdown_is_bounded_and_preserves_owner_release_for_all_exit_classes() {
        use crate::foreground_schedule::Hooks;
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        for case in 0..3 {
            let mut session = start_session(&path);
            let report = session.run_reported(
                1,
                Hooks {
                    tick: |_: &mut crate::foreground_session::Session| {
                        Ok(crate::polling_cycle::TickReport::default())
                    },
                    monotonic: || {
                        if case == 2 {
                            None
                        } else {
                            Some(Duration::ZERO)
                        }
                    },
                    wait: |_| panic!("single tick must not wait"),
                    cancelled: || case == 0,
                },
            );
            let output = report.render();
            assert_eq!(
                output,
                match case {
                    0 => "foreground outcome=cancelled ticks=0\n",
                    1 => "foreground outcome=exhausted ticks=1\n",
                    _ => "foreground outcome=failed_clock ticks=unknown\n",
                }
            );
            assert!(output.len() <= crate::foreground_report::MAX_BYTES);
            assert!(
                !output.contains("research-cluster")
                    && !output.contains("alice")
                    && !output.contains("state.sqlite3")
            );
            assert!(matches!(
                session.status(7200),
                Err(crate::foreground_session::Error::Closed)
            ));
            drop(open_owned_fixture(&plan));
        }
    }
    #[test]
    fn reported_startup_preserves_failure_ordering_and_owned_state() {
        use crate::foreground_session::Session;
        let d = crate::storage::tests::Directory::new();
        let path = config_path(&d);
        for (limit, cancel, expected) in [
            (0, false, "failed_limit"),
            (1, true, "cancelled"),
            (1, false, "failed_preparation"),
        ] {
            let report = match Session::start_reported(
                &path,
                limit,
                Duration::from_millis(50),
                |_| panic!("unexpected identity"),
                |_, _| panic!("unexpected lookup"),
                || cancel,
            ) {
                Err(report) => report,
                Ok(_) => panic!("unexpected startup"),
            };
            assert_eq!(report.render(), format!("foreground startup={expected}\n"));
        }
        assert!(!owned_path(&d).exists());
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        let plan = prepare_file_fixture(&v, &path);
        let mut owner = start_session(&path);
        let lookup = |name: &str, _| {
            Ok(Zeroizing::new(if name.ends_with("_URL") {
                b"https://fixture.invalid/hook".to_vec()
            } else {
                b"Bearer fixture".to_vec()
            }))
        };
        let report = match Session::start_reported(
            &path,
            1,
            Duration::from_millis(50),
            |_| Ok(Zeroizing::new(b"alice".to_vec())),
            lookup,
            || false,
        ) {
            Err(report) => report,
            Ok(_) => panic!("second owner"),
        };
        assert_eq!(report.render(), "foreground startup=failed_ownership\n");
        assert!(owner.status(7200).unwrap().poll.stale);
        owner.stop();
        let mut session = Session::start_reported(
            &path,
            1,
            Duration::from_millis(50),
            |_| Ok(Zeroizing::new(b"alice".to_vec())),
            lookup,
            || false,
        )
        .unwrap();
        assert!(session.status(7200).unwrap().poll.stale);
        session.stop();
        drop(open_owned_fixture(&plan));
        // Malformed protected input maps statically without reaching resource callbacks.
        replace_config(&path, b"{private malformed fixture");
        assert_eq!(
            match Session::start_reported(
                &path,
                1,
                Duration::from_millis(50),
                |_| panic!("identity"),
                |_, _| panic!("lookup"),
                || false
            ) {
                Err(report) => report.render(),
                Ok(_) => panic!("invalid config"),
            },
            "foreground startup=failed_preparation\n"
        );
    }
    #[test]
    fn runtime_startup_failures_precede_hooks_and_state_access() {
        use crate::{
            foreground_runtime::{self, Hooks, Request},
            foreground_wait::Signal,
        };
        let d = crate::storage::tests::Directory::new();
        let path = config_path(&d);
        for (ticks, cancel, expected) in [
            (0, false, "failed_limit"),
            (1025, false, "failed_limit"),
            (1, true, "cancelled"),
            (1, false, "failed_preparation"),
        ] {
            let signal = Signal::new();
            if cancel {
                signal.cancel().unwrap();
            }
            let summary = foreground_runtime::run(
                Request {
                    path: &path,
                    poll_limit: 1,
                    tick_limit: ticks,
                    lock_timeout: Duration::from_millis(50),
                },
                &signal,
                Hooks {
                    identity: |_| panic!("identity"),
                    lookup: |_: &str, _| panic!("lookup"),
                    tick: |_: &mut crate::foreground_session::Session, _: &Signal| panic!("tick"),
                },
            );
            assert_eq!(summary.render(), format!("foreground startup={expected}\n"));
        }
        assert!(!owned_path(&d).exists());
    }
    #[test]
    fn runtime_shared_query_cancellation_and_fresh_restart_preserve_durable_work() {
        use crate::{
            foreground_runtime::{self, Request},
            foreground_wait::Signal,
            polling_cycle::Hooks,
            storage::DueDelivery,
        };
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        v["routes"] = json!([{"events":["JOB_FAILED"],"send_to":["primary"]}]);
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        let mut sends = 0;
        for cancel in [true, false] {
            let signal = Signal::new();
            let summary = foreground_runtime::run(
                Request {
                    path: &path,
                    poll_limit: 1,
                    tick_limit: 1,
                    lock_timeout: Duration::from_millis(50),
                },
                &signal,
                foreground_runtime::Hooks {
                    identity: |_| Ok(Zeroizing::new(b"alice".to_vec())),
                    lookup: |name: &str, _| {
                        Ok(Zeroizing::new(if name.ends_with("_URL") {
                            b"https://fixture.invalid/hook".to_vec()
                        } else {
                            b"Bearer fixture".to_vec()
                        }))
                    },
                    tick: |session: &mut crate::foreground_session::Session, shared: &Signal| {
                        let report = session.tick_interruptible(
                            (Path::new("squeue"), Path::new("sacct")),
                            Hooks {
                                query: |command: &mut std::process::Command, _, _| {
                                    if command.get_program() == "squeue" {
                                        return Ok(String::new());
                                    }
                                    if cancel {
                                        shared.cancel().unwrap();
                                    }
                                    Ok("research-cluster|1|alice|s|actual|FAILED|1:0".into())
                                },
                                send: (
                                    |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                                        panic!("SMTP resolution")
                                    },
                                    |_: &DueDelivery| -> std::process::Command {
                                        panic!("SMTP helper")
                                    },
                                    |_: &DueDelivery| {
                                        assert!(!cancel);
                                        sends += 1;
                                        Ok(crate::delivery::ProviderOutcome::Delivered)
                                    },
                                ),
                                clock: || Some(7200),
                                cancelled: shared,
                            },
                        )?;
                        assert!(
                            matches!(report.poll, Some(Ok(crate::polling_cycle::Outcome::Committed { events, .. })) if events == usize::from(cancel))
                        );
                        Ok(report)
                    },
                },
            );
            assert_eq!(
                summary.render(),
                if cancel {
                    "foreground outcome=cancelled ticks=1\n"
                } else {
                    "foreground outcome=exhausted ticks=1\n"
                }
            );
            let store = open_owned_fixture(&plan);
            assert_eq!(
                store
                    .due_deliveries("research-cluster", 7200, 10)
                    .unwrap()
                    .len(),
                usize::from(cancel)
            );
        }
        assert_eq!(sends, 1);
    }
    #[test]
    fn composed_owned_state_creates_private_namespace_and_excludes_second_owner() {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        let path = owned_path(&d);
        v["storage"]["path"] = json!(path);
        let plan = prepare_file_fixture(&v, &config_path(&d));
        assert!(!path.exists());
        let store = plan.open_owned(Duration::from_millis(50)).unwrap();
        for (path, mode) in [
            (path.parent().unwrap().to_path_buf(), 0o700),
            (path.clone(), 0o600),
            (path.parent().unwrap().join(".owner.lock"), 0o600),
        ] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o7777,
                mode
            );
        }
        assert!(matches!(
            plan.open_owned(Duration::from_millis(50)),
            Err(crate::storage::StorageError::StatePath(
                crate::state_path::Error::AlreadyOwned
            ))
        ));
        drop(store);
        let reopened = open_owned_fixture(&plan);
        assert!(reopened
            .due_deliveries("research-cluster", 10, 10)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn composed_owned_state_rejects_ambiguous_paths_and_unsafe_namespace() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        let path = owned_path(&d);
        for bad in [
            path.with_file_name("custom.sqlite3"),
            path.parent().unwrap().join(".").join("state.sqlite3"),
            path.parent().unwrap().join("..").join("state.sqlite3"),
            PathBuf::from("/state.sqlite3"),
        ] {
            v["storage"]["path"] = json!(bad);
            assert!(matches!(
                prepare_fixture(&v).open_owned(Duration::from_millis(50)),
                Err(crate::storage::StorageError::InvalidInput)
            ));
            assert!(!path.parent().unwrap().exists());
        }
        v["storage"]["path"] = json!(path);
        let plan = prepare_fixture(&v);
        let parent = path.parent().unwrap();
        std::fs::create_dir(parent).unwrap();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            plan.open_owned(Duration::from_millis(50)),
            Err(crate::storage::StorageError::StatePath(
                crate::state_path::Error::UnsafePermissions
            ))
        ));
        assert!(!path.exists());
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = parent.parent().unwrap().join("untouched");
        std::fs::write(&target, b"synthetic sentinel").unwrap();
        symlink(&target, &path).unwrap();
        assert!(plan.open_owned(Duration::from_millis(50)).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"synthetic sentinel");
        std::fs::remove_file(&path).unwrap();
        // A failed database check must release the acquired ownership lock.
        drop(open_owned_fixture(&plan));
    }
    #[test]
    fn protected_config_failures_stop_before_composition_lookups() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let d = crate::storage::tests::Directory::new();
        let v = fixture(&d);
        let path = config_path(&d);
        let rejected = || {
            prepare_path(
                &path,
                |_| panic!("rejected identity"),
                |_, _| panic!("rejected lookup"),
            )
        };
        assert!(matches!(
            rejected(),
            Err(Error::Snapshot(crate::config_snapshot::Error::Io))
        ));
        replace_config(&path, b"{");
        assert!(matches!(rejected(), Err(Error::Validation(_))));
        replace_config(&path, &vec![b' '; 65537]);
        assert!(matches!(
            rejected(),
            Err(Error::Snapshot(crate::config_snapshot::Error::Limit))
        ));
        replace_config(&path, &serde_json::to_vec(&v).unwrap());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            rejected(),
            Err(Error::Snapshot(
                crate::config_snapshot::Error::UnsafePermissions
            ))
        ));
        std::fs::remove_file(&path).unwrap();
        symlink("root.der", &path).unwrap();
        assert!(matches!(
            rejected(),
            Err(Error::Snapshot(crate::config_snapshot::Error::Io))
        ));
    }
    #[test]
    fn config_replacement_during_lookup_keeps_one_owned_validated_snapshot() {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        let path = config_path(&d);
        replace_config(&path, &serde_json::to_vec(&v).unwrap());
        let old = prepare_path(
            &path,
            |_| {
                // The protected read and validation have finished before this hook.
                replace_config(&path, b"invalid replacement");
                Ok(Zeroizing::new(b"alice".to_vec()))
            },
            |name, _| {
                Ok(Zeroizing::new(if name.ends_with("_URL") {
                    b"https://fixture.invalid/hook".to_vec()
                } else {
                    b"Bearer fixture".to_vec()
                }))
            },
        )
        .unwrap();
        assert_eq!(old.common.observation.user(), "alice");
        assert!(matches!(
            prepare_path(
                &path,
                |_| panic!("invalid replacement identity"),
                |_, _| panic!("invalid replacement lookup")
            ),
            Err(Error::Validation(_))
        ));
        v["providers"]["research_email"]["request_timeout_milliseconds"] = json!(1234);
        let new = prepare_file_fixture(&v, &path);
        assert_eq!(
            old.timeout("research_email").unwrap(),
            Duration::from_secs(10)
        );
        assert_eq!(
            new.timeout("research_email").unwrap(),
            Duration::from_millis(1234)
        );
        assert_eq!(old.revisions(), new.revisions());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(old.permissions()[0].provider, "b".repeat(64));
    }
    #[test]
    fn validation_and_trust_failure_precede_all_injected_lookups() {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["schema_version"] = json!(2);
        assert!(matches!(
            prepare(
                &serde_json::to_vec(&v).unwrap(),
                |_| panic!("identity"),
                |_, _| panic!("lookup")
            ),
            Err(Error::Validation(_))
        ));
        v["schema_version"] = json!(1);
        v["providers"]["research_email"]["trust_der_files"] = json!(["/missing/private/root.der"]);
        assert!(matches!(
            prepare(
                &serde_json::to_vec(&v).unwrap(),
                |_| panic!("identity"),
                |_, _| panic!("lookup")
            ),
            Err(Error::Trust(_))
        ));
        v["providers"]["research_email"]["sender"] = json!("bad address");
        assert!(prepare(
            &serde_json::to_vec(&v).unwrap(),
            |_| panic!("identity"),
            |_, _| panic!("lookup")
        )
        .is_err());
    }
    #[test]
    fn owned_plan_keeps_explicit_permissions_and_stable_reference_bindings() {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        let plan = prepare_fixture(&v);
        assert_eq!(
            plan.timeout("research_email").unwrap(),
            Duration::from_secs(10)
        );
        assert_eq!(plan.timeout("missing"), Err(Error::Profile));
        assert_eq!(plan.permissions()[0].provider, "b".repeat(64));
        let old = plan.revisions()[0].2.to_owned();
        assert_eq!(prepare_fixture(&v).revisions()[0].2, old);
        v["providers"]["research_email"]["authentication"]["password_env"] =
            json!("ROTATED_PASSWORD");
        let changed = prepare_fixture(&v);
        assert_ne!(changed.revisions()[0].2, old);
        assert_eq!(changed.revisions()[0].1, plan.revisions()[0].1);
        assert!(matches!(
            changed.resolve(&plan.smtp[0].descriptor, |_, _| panic!(
                "mismatched reference lookup"
            )),
            Err(Error::Profile)
        ));
        drop(v);
        for password in [b"first".as_slice(), b"rotated".as_slice()] {
            let resolved = plan
                .resolve(&plan.smtp[0].descriptor, |name, _| {
                    Ok(Zeroizing::new(if name.ends_with("USERNAME") {
                        b"researcher".to_vec()
                    } else {
                        password.to_vec()
                    }))
                })
                .unwrap();
            assert_eq!(resolved.provider().token(), old);
        }
        assert!(matches!(
            plan.resolve(&plan.smtp[0].descriptor, |_, _| Err(())),
            Err(Error::Credentials(_))
        ));
    }
    #[test]
    fn public_cycles_commit_deliver_report_query_failure_and_preserve_exhaustion_on_restart() {
        use crate::polling_cycle::{Cycles, Failure, Hooks, Outcome, StageError};
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["primary","research_email"]}]);
        let initial = prepare_fixture(&v);
        let (_, rendering, provider) = initial.revisions()[0];
        v["delivery"]["smtp_permissions"][0]["rendering_revision"] = json!(rendering);
        v["delivery"]["smtp_permissions"][0]["provider_revision"] = json!(provider);
        let plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let mut cycles = Cycles::new(2).unwrap();
        let helper = |ack: &str| {
            let mut command = std::process::Command::new("python3");
            command.args([
                "-c",
                &format!(
                    "import sys; assert sys.stdin.buffer.read(); sys.stdout.write('{ack}\\n')"
                ),
            ]);
            command
        };
        for (now, fail) in [(7200, false), (7201, true), (7202, false)] {
            let report = plan
                .tick_public_with(
                    (Path::new("squeue"), Path::new("sacct")),
                    &mut cycles,
                    &mut store,
                    Hooks {
                        query: |command: &mut std::process::Command, _, _| {
                            assert_ne!(now, 7202, "exhausted query");
                            if fail {
                                return Err(crate::scheduler::QueryError::Timeout);
                            }
                            Ok(if command.get_program() == "squeue" {
                                ""
                            } else {
                                "research-cluster|1|alice|s|a|COMPLETED|0:0"
                            }
                            .into())
                        },
                        send: (
                            |name: &str, _| {
                                Ok(Zeroizing::new(if name.ends_with("USERNAME") {
                                    b"researcher".to_vec()
                                } else {
                                    b"fixture-password".to_vec()
                                }))
                            },
                            |_: &crate::storage::DueDelivery| {
                                assert_eq!(now, 7200);
                                helper("SMTP_SUBMIT FINAL ACCEPTED RA")
                            },
                            |_: &crate::storage::DueDelivery| {
                                assert_eq!(now, 7200);
                                helper("OK")
                            },
                        ),
                        clock: || Some(now),
                        cancelled: || false,
                    },
                )
                .unwrap();
            assert!(report.preflight.unwrap().is_ok());
            if now == 7200 {
                assert!(matches!(
                    report.poll,
                    Some(Ok(Outcome::Committed { events: 1, .. }))
                ));
                assert_eq!(report.delivery.unwrap().unwrap().delivered, 2);
            } else if fail {
                assert!(matches!(report.poll, Some(Err(StageError::Poll))));
                assert_eq!(
                    report.health.unwrap().unwrap().poll.last_failure,
                    Some(Failure::ActiveQuery)
                );
            } else {
                assert!(matches!(report.poll, Some(Ok(Outcome::Exhausted))));
            }
        }
        drop(store);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        assert!(store
            .due_deliveries("research-cluster", 7300, 10)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .restore("research-cluster", 10)
                .unwrap()
                .0
                .emitted
                .len(),
            1
        );
    }
    #[test]
    fn public_cycle_cancellation_after_query_preserves_frames_without_dispatch() {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["primary","research_email"]}]);
        let plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let mut cycles = crate::polling_cycle::Cycles::new(2).unwrap();
        let cancel = std::cell::Cell::new(false);
        let report = plan
            .tick_public_with(
                (Path::new("squeue"), Path::new("sacct")),
                &mut cycles,
                &mut store,
                crate::polling_cycle::Hooks {
                    query: |command: &mut std::process::Command, _, _| {
                        Ok(if command.get_program() == "squeue" {
                            ""
                        } else {
                            cancel.set(true);
                            "research-cluster|1|alice|s|a|COMPLETED|0:0"
                        }
                        .into())
                    },
                    send: (
                        |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                            panic!("cancelled credentials")
                        },
                        |_: &crate::storage::DueDelivery| -> std::process::Command {
                            panic!("cancelled SMTP")
                        },
                        |_: &crate::storage::DueDelivery| -> std::process::Command {
                            panic!("cancelled webhook")
                        },
                    ),
                    clock: || Some(7200),
                    cancelled: || cancel.get(),
                },
            )
            .unwrap();
        assert!(report.cancelled);
        assert!(report.delivery.is_none());
        assert!(matches!(
            report.poll,
            Some(Ok(crate::polling_cycle::Outcome::Committed {
                events: 1,
                ..
            }))
        ));
        drop(store);
        let store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let due = store.due_deliveries("research-cluster", 7200, 10).unwrap();
        assert_eq!(due.len(), 2);
        assert!(due.iter().all(|item| item.attempt_number == 1));
        let id = &due[0].event_id;
        assert!(store
            .public_delivery(
                id,
                "primary",
                plan.common.webhooks[0].resources.public_revision().unwrap()
            )
            .unwrap()
            .is_some());
        assert!(store.frozen_smtp(id, "research_email").unwrap().is_some());
    }

    #[test]
    fn public_mixed_delivery_cancellation_and_provider_confusion_do_not_resolve_or_spawn() {
        let d = crate::storage::tests::Directory::new();
        let v = fixture(&d);
        let mut plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        plan.poll_public_with(
            config_composition::PollInput {
                active_program: Path::new("squeue"),
                accounting_program: Path::new("sacct"),
                history_start: "2026-10-10T00:00:00",
                history_end: "2026-10-11T00:00:00",
                observed_at: 10,
            },
            &mut reconciler,
            &mut store,
            |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    ""
                } else {
                    "research-cluster|1|alice|s|a|COMPLETED|0:0"
                }
                .into())
            },
        )
        .unwrap();
        let blocked = |plan: &Plan, store: &mut Store, cancel| {
            plan.deliver_public_with(
                store,
                10,
                PublicDeliveryHooks {
                    lookup: |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                        panic!("blocked lookup")
                    },
                    smtp_helper: |_: &crate::storage::DueDelivery| -> std::process::Command {
                        panic!("blocked SMTP child")
                    },
                    webhook_helper: |_: &crate::storage::DueDelivery| -> std::process::Command {
                        panic!("blocked webhook child")
                    },
                    clock: || panic!("blocked completion"),
                    cancelled: || cancel,
                },
            )
            .unwrap()
            .unwrap_err()
        };
        assert_eq!(
            blocked(&plan, &mut store, true).kind,
            crate::delivery::FailureKind::Cancelled
        );
        plan.common.webhooks[0].alias = Zeroizing::new("research_email".into());
        assert_eq!(
            blocked(&plan, &mut store, false).diagnostic,
            Some(crate::supervised_provider::Diagnostic::SmtpProviderDenied)
        );
        assert_eq!(
            store.due_deliveries("research-cluster", 10, 10).unwrap()[0].attempt_number,
            1
        );
    }

    #[test]
    fn public_mixed_delivery_preserves_webhook_completion_when_smtp_denied_then_resumes_after_restart(
    ) {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["primary","research_email"]}]);
        let plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        plan.poll_public_with(
            config_composition::PollInput {
                active_program: Path::new("squeue"),
                accounting_program: Path::new("sacct"),
                history_start: "2026-10-10T00:00:00",
                history_end: "2026-10-11T00:00:00",
                observed_at: 10,
            },
            &mut reconciler,
            &mut store,
            |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    ""
                } else {
                    "research-cluster|1|alice|s|a|COMPLETED|0:0"
                }
                .into())
            },
        )
        .unwrap();
        let helper = |ack: &str| {
            let mut command = std::process::Command::new("python3");
            command.args([
                "-c",
                &format!(
                    "import sys; assert sys.stdin.buffer.read(); sys.stdout.write('{ack}\\n')"
                ),
            ]);
            command
        };
        let denied = plan
            .deliver_public_with(
                &mut store,
                10,
                PublicDeliveryHooks {
                    lookup: |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                        panic!("denied secret lookup")
                    },
                    smtp_helper: |_: &crate::storage::DueDelivery| -> std::process::Command {
                        panic!("denied SMTP child")
                    },
                    webhook_helper: |_: &crate::storage::DueDelivery| helper("OK"),
                    clock: || Some(10),
                    cancelled: || false,
                },
            )
            .unwrap()
            .unwrap_err();
        assert_eq!(denied.committed.delivered, 1);
        assert_eq!(
            denied.diagnostic,
            Some(crate::supervised_provider::Diagnostic::SmtpProviderDenied)
        );
        let pending = store.due_deliveries("research-cluster", 10, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].destination, "research_email");
        assert_eq!(pending[0].attempt_number, 1);
        let (_, rendering, provider) = plan.revisions()[0];
        v["delivery"]["smtp_permissions"][0]["rendering_revision"] = json!(rendering);
        v["delivery"]["smtp_permissions"][0]["provider_revision"] = json!(provider);
        drop(store);
        let plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let report = plan
            .deliver_public_with(
                &mut store,
                10,
                PublicDeliveryHooks {
                    lookup: |name: &str, _| {
                        Ok(Zeroizing::new(if name.ends_with("USERNAME") {
                            b"researcher".to_vec()
                        } else {
                            b"fixture-password".to_vec()
                        }))
                    },
                    smtp_helper: |_: &crate::storage::DueDelivery| {
                        helper("SMTP_SUBMIT FINAL ACCEPTED RA")
                    },
                    webhook_helper: |_: &crate::storage::DueDelivery| -> std::process::Command {
                        panic!("completed webhook replayed")
                    },
                    clock: || Some(10),
                    cancelled: || false,
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(report.delivered, 1);
        assert!(store
            .due_deliveries("research-cluster", 10, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn public_mixed_poll_is_atomic_and_restores_both_provider_preparations() {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["primary","research_email"]}]);
        let plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let input = || config_composition::PollInput {
            active_program: Path::new("squeue"),
            accounting_program: Path::new("sacct"),
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            observed_at: 10,
        };
        let query = |command: &mut std::process::Command, _, _| {
            Ok(if command.get_program() == "squeue" {
                ""
            } else {
                "research-cluster|1|alice|s|actual|COMPLETED|0:0"
            }
            .into())
        };
        let db = rusqlite::Connection::open(d.path()).unwrap();
        db.execute_batch("CREATE TRIGGER fail_checkpoint BEFORE INSERT ON checkpoints BEGIN SELECT RAISE(ABORT,'fixture'); END").unwrap();
        assert!(plan
            .poll_public_with(input(), &mut reconciler, &mut store, query)
            .is_err());
        assert!(reconciler.latest.is_empty() && reconciler.emitted.is_empty());
        assert!(store
            .due_deliveries("research-cluster", 10, 10)
            .unwrap()
            .is_empty());
        db.execute_batch("DROP TRIGGER fail_checkpoint").unwrap();
        plan.poll_public_with(input(), &mut reconciler, &mut store, query)
            .unwrap();
        let due = store.due_deliveries("research-cluster", 10, 10).unwrap();
        assert_eq!(due.len(), 2);
        let id = due[0].event_id.clone();
        let revision = plan.common.webhooks[0].resources.public_revision().unwrap();
        let frame = store
            .public_delivery(&id, "primary", revision)
            .unwrap()
            .unwrap();
        let smtp = store
            .frozen_smtp(&id, "research_email")
            .unwrap()
            .unwrap()
            .data()
            .to_vec();
        drop(store);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let (mut restored, _) = store.restore("research-cluster", 10).unwrap();
        assert!(plan
            .poll_public_with(input(), &mut restored, &mut store, query)
            .unwrap()
            .is_empty());
        let reopened = store
            .public_delivery(&id, "primary", revision)
            .unwrap()
            .unwrap();
        assert_eq!(reopened.id, frame.id);
        assert_eq!(reopened.payload, frame.payload);
        assert_eq!(
            store
                .frozen_smtp(&id, "research_email")
                .unwrap()
                .unwrap()
                .data(),
            smtp
        );
        assert!(store
            .bound_smtp_provider(&id, "research_email")
            .unwrap()
            .is_some());
    }

    #[test]
    fn public_mixed_poll_rejects_incomplete_resources_before_query_and_payload_overflow_before_commit(
    ) {
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["routes"] = json!([{"events":["JOB_COMPLETED"],"send_to":["primary","research_email"]}]);
        v["providers"]["primary"]["payload_limit_bytes"] = json!(1024);
        let mut plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let input = || config_composition::PollInput {
            active_program: Path::new("squeue"),
            accounting_program: Path::new("sacct"),
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            observed_at: 10,
        };
        let webhook = plan.common.webhooks.pop().unwrap();
        assert!(plan
            .poll_public_with(input(), &mut reconciler, &mut store, |_, _, _| panic!(
                "missing resource queried"
            ))
            .is_err());
        plan.common.webhooks.push(webhook);
        let huge = "1".repeat(2000);
        let result =
            plan.poll_public_with(input(), &mut reconciler, &mut store, |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    String::new()
                } else {
                    format!("research-cluster|{huge}|alice|s|actual|COMPLETED|0:0")
                })
            });
        assert!(matches!(result, Err(crate::smtp_poll::Error::Limit)));
        assert!(reconciler.latest.is_empty() && reconciler.emitted.is_empty());
        assert!(store
            .due_deliveries("research-cluster", 10, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn composed_poll_freezes_once_and_denies_unenrolled_delivery_before_resolution() {
        use crate::{
            delivery, smtp_provider_delivery as guard,
            storage::{DueDelivery, FrozenMessage},
        };
        let d = crate::storage::tests::Directory::new();
        let v = fixture(&d);
        let plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(plan.common.controls.identity_limit);
        let poll = || config_composition::PollInput {
            active_program: Path::new("squeue"),
            accounting_program: Path::new("sacct"),
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            observed_at: 10,
        };
        let query = |command: &mut std::process::Command, _, _| {
            Ok(if command.get_program()=="squeue" { "" } else { "research-cluster|1|alice|s|actual|COMPLETED|0:0\nresearch-cluster|2|alice|s|actual|TIMEOUT|1:0" }.into())
        };
        assert_eq!(
            plan.poll_with(poll(), &mut reconciler, &mut store, query)
                .unwrap()
                .len(),
            2
        );
        let pending = store.due_deliveries("research-cluster", 10, 10).unwrap();
        assert_eq!(pending.len(), 1);
        let id = &pending[0].event_id;
        let frozen = store
            .frozen_smtp(id, "research_email")
            .unwrap()
            .unwrap()
            .data()
            .to_vec();
        let bound = store
            .bound_smtp_provider(id, "research_email")
            .unwrap()
            .unwrap();
        assert_eq!(bound.token(), plan.revisions()[0].2);
        let permissions = plan.permissions();
        let permissions = guard::Permissions::new(&permissions).unwrap();
        let failure = guard::deliver_due_mixed(
            &mut store,
            "research-cluster",
            10,
            10,
            delivery::RetryPolicy {
                max_attempts: 3,
                initial_delay_seconds: 5,
                max_delay_seconds: 20,
                jitter_seconds: 0,
                jitter_seed: 0,
            },
            guard::MixedHooks {
                ordinary_destinations: &["primary"],
                ordinary_send: |_: &DueDelivery| panic!("ordinary"),
                smtp: guard::Hooks {
                    permissions: &permissions,
                    resolve: |_: &Descriptor| -> Result<Resolved, guard::Error> {
                        panic!("denied resolution")
                    },
                    send: |_: &DueDelivery,
                           _: &crate::smtp_revision::Descriptor,
                           _: &FrozenMessage,
                           _: Resolved| panic!("denied send"),
                    clock: || panic!("denied clock"),
                    cancelled: || false,
                },
            },
        )
        .unwrap_err();
        assert_eq!(failure.kind, delivery::FailureKind::DispatchBlocked);
        drop(store);
        let restarted = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let (mut reconciler, _) = store
            .restore("research-cluster", restarted.common.controls.identity_limit)
            .unwrap();
        assert!(restarted
            .poll_with(poll(), &mut reconciler, &mut store, query)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .frozen_smtp(id, "research_email")
                .unwrap()
                .unwrap()
                .data(),
            frozen
        );
        assert_eq!(
            store.due_deliveries("research-cluster", 10, 10).unwrap()[0].attempt_number,
            1
        );
        assert!(restarted
            .resolve(&bound, |name, _| Ok(Zeroizing::new(
                if name.ends_with("USERNAME") {
                    b"researcher".to_vec()
                } else {
                    b"rotated".to_vec()
                }
            )))
            .is_ok());
    }

    #[test]
    fn composed_cycles_supervise_retry_and_recover_with_rotated_credentials_and_edited_routes() {
        use crate::{
            polling_cycle::{Cycles, Hooks, StageError},
            storage::DueDelivery,
        };
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        v["routes"] = json!([
            {"events":["JOB_COMPLETED"],"send_to":["research_email"]},
            {"events":["JOB_FAILED"],"send_to":["primary"]}
        ]);
        v["delivery"]["jitter_seconds"] = json!(0);
        let provisional = prepare_fixture(&v);
        let (_, rendering, provider) = provisional.revisions()[0];
        // Explicit test-only enrollment of exactly the inspected pair.
        v["delivery"]["smtp_permissions"] = json!([{
            "cluster_id":"research-cluster","destination":"research_email",
            "rendering_revision":rendering,"provider_revision":provider
        }]);
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        let mut store = open_owned_fixture(&plan);
        let mut cycles = Cycles::new(1).unwrap();
        let query = |command: &mut std::process::Command, timeout, limit| {
            assert!(timeout <= Duration::from_secs(10));
            assert_eq!(limit, 1048576);
            Ok(if command.get_program()=="squeue" {""} else {"research-cluster|1|alice|s|actual|COMPLETED|0:0\nresearch-cluster|2|alice|s|actual|FAILED|1:0\nresearch-cluster|3|alice|s|actual|TIMEOUT|1:0"}.into())
        };
        let run = |plan: &Plan,
                   cycles: &mut Cycles,
                   store: &mut Store,
                   now,
                   reply: &str,
                   password: &[u8],
                   cancelled| {
            let mut ordinary = 0;
            let mut secrets = 0;
            let result = plan
                .tick_with(
                    (Path::new("squeue"), Path::new("sacct")),
                    cycles,
                    store,
                    Hooks {
                        query,
                        send: (
                            |name: &str, _| {
                                secrets += 1;
                                Ok(Zeroizing::new(if name.ends_with("USERNAME") {
                                    b"researcher".to_vec()
                                } else {
                                    password.to_vec()
                                }))
                            },
                            |due: &DueDelivery| {
                                assert_eq!(due.destination, "research_email");
                                assert!(!reply.is_empty());
                                let mut cmd = std::process::Command::new("/bin/sh");
                                cmd.args([
                                    "-c",
                                    "cat >/dev/null; printf '%s\\n' \"$1\"",
                                    "fixture",
                                    reply,
                                ]);
                                cmd
                            },
                            |due: &DueDelivery| {
                                assert_eq!(due.destination, "primary");
                                ordinary += 1;
                                Ok(crate::delivery::ProviderOutcome::Delivered)
                            },
                        ),
                        clock: || Some(now),
                        cancelled: || cancelled,
                    },
                )
                .unwrap();
            (result, ordinary, secrets)
        };
        let (cancelled, ordinary, secrets) =
            run(&plan, &mut cycles, &mut store, 7200, "", b"first", true);
        assert!(cancelled.cancelled);
        assert_eq!((ordinary, secrets), (0, 0));
        assert!(store
            .due_deliveries("research-cluster", 7200, 10)
            .unwrap()
            .is_empty());
        let (first, ordinary, secrets) = run(
            &plan,
            &mut cycles,
            &mut store,
            7200,
            "SMTP_SUBMIT FINAL TEMP NA",
            b"first",
            false,
        );
        assert_eq!((ordinary, secrets), (1, 2));
        let batch = first.delivery.unwrap().unwrap();
        assert_eq!((batch.delivered, batch.retrying), (1, 1));
        let pending = store
            .due_deliveries("research-cluster", 7205, 10)
            .unwrap()
            .remove(0);
        let frozen = store
            .frozen_smtp(&pending.event_id, "research_email")
            .unwrap()
            .unwrap()
            .data()
            .to_vec();
        assert_eq!(pending.attempt_number, 2);
        drop(store);
        v["routes"] = json!([{"events":["JOB_COMPLETED","JOB_TIMEOUT"],"send_to":["primary"]}]);
        let allowed_source = v.clone();
        v["delivery"]["smtp_permissions"] = json!([]);
        let denied = prepare_file_fixture(&v, &path);
        let mut store = open_owned_fixture(&denied);
        let (blocked, ordinary, secrets) = run(
            &denied,
            &mut Cycles::new(1).unwrap(),
            &mut store,
            7205,
            "",
            b"rotated",
            false,
        );
        assert_eq!(blocked.delivery, Some(Err(StageError::DispatchBlocked)));
        assert_eq!((ordinary, secrets), (0, 0));
        assert!(blocked.cleanup.is_none());
        drop(store);
        let recovered = prepare_file_fixture(&allowed_source, &path);
        let mut store = open_owned_fixture(&recovered);
        assert_eq!(
            store
                .frozen_smtp(&pending.event_id, "research_email")
                .unwrap()
                .unwrap()
                .data(),
            frozen
        );
        let (last, ordinary, secrets) = run(
            &recovered,
            &mut Cycles::new(1).unwrap(),
            &mut store,
            7205,
            "SMTP_SUBMIT FINAL ACCEPTED RA",
            b"rotated",
            false,
        );
        assert_eq!((ordinary, secrets), (0, 2));
        assert_eq!(last.delivery.unwrap().unwrap().delivered, 1);
        assert!(store
            .due_deliveries("research-cluster", 7206, 10)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .restore("research-cluster", 10000)
                .unwrap()
                .0
                .emitted
                .len(),
            3
        );
        assert_eq!(
            store
                .frozen_smtp(&pending.event_id, "research_email")
                .unwrap()
                .unwrap()
                .data(),
            frozen
        );
    }

    #[test]
    fn composed_cycle_rejects_invalid_deadlines_before_query_or_storage() {
        use crate::{
            polling_cycle::{Cycles, Hooks},
            storage::DueDelivery,
        };
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["providers"]["second_email"] = v["providers"]["research_email"].clone();
        v["providers"]["second_email"]["request_timeout_milliseconds"] = json!(2000);
        let mut plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        // Corrupt only private fixture state to exercise the runtime preflight.
        plan.smtp[0].timeout = Duration::from_nanos(1);
        for empty in [false, true] {
            if empty {
                plan.smtp.clear();
            }
            let result = plan.tick_with(
                (Path::new("squeue"), Path::new("sacct")),
                &mut Cycles::new(1).unwrap(),
                &mut store,
                Hooks {
                    query: |_: &mut std::process::Command, _, _| panic!("unsupported query"),
                    send: (
                        |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                            panic!("unsupported lookup")
                        },
                        |_: &DueDelivery| -> std::process::Command { panic!("unsupported helper") },
                        |_: &DueDelivery| panic!("unsupported ordinary"),
                    ),
                    clock: || Some(7200),
                    cancelled: || false,
                },
            );
            if empty {
                assert!(matches!(result, Err(Error::NoSmtp)));
            } else {
                assert_eq!(
                    result.unwrap().preflight,
                    Some(Err(crate::polling_cycle::StageError::Policy))
                );
            }
        }
        assert!(store
            .due_deliveries("research-cluster", 7200, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn composed_distinct_deadlines_retry_short_destination_and_preserve_binding_on_restart() {
        use crate::{
            polling_cycle::{Cycles, Hooks},
            storage::DueDelivery,
        };
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["providers"]["second_email"] = v["providers"]["research_email"].clone();
        v["providers"]["research_email"]["request_timeout_milliseconds"] = json!(40);
        v["providers"]["second_email"]["request_timeout_milliseconds"] = json!(2000);
        v["routes"][0]["send_to"] = json!(["research_email", "second_email"]);
        v["delivery"]["jitter_seconds"] = json!(0);
        let provisional = prepare_fixture(&v);
        v["delivery"]["smtp_permissions"] = json!(provisional.revisions().iter().map(|(name,rendering,provider)| json!({
            "cluster_id":"research-cluster","destination":name,"rendering_revision":rendering,"provider_revision":provider
        })).collect::<Vec<_>>());
        let plan = prepare_fixture(&v);
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let run = |plan: &Plan, store: &mut Store, now| {
            plan.tick_with((Path::new("squeue"),Path::new("sacct")),&mut Cycles::new(1).unwrap(),store,Hooks {
                query: |cmd: &mut std::process::Command,_,_| Ok(if cmd.get_program()=="squeue" {""} else {"research-cluster|1|alice|s|actual|COMPLETED|0:0"}.into()),
                send: (|name: &str,_| Ok(Zeroizing::new(if name.ends_with("USERNAME") {b"researcher".to_vec()} else {b"fixture".to_vec()})),
                    |_: &DueDelivery| {let mut cmd = std::process::Command::new("/bin/sh");cmd.args(["-c","cat >/dev/null; sleep 0.2; printf 'SMTP_SUBMIT FINAL ACCEPTED RA\\n'"]);cmd},
                    |_: &DueDelivery| panic!("unselected ordinary")),
                clock: || Some(now),cancelled: || false,
            }).unwrap()
        };
        let batch = run(&plan, &mut store, 7200).delivery.unwrap().unwrap();
        assert_eq!((batch.delivered, batch.retrying), (1, 1));
        let pending = store
            .due_deliveries("research-cluster", 7205, 10)
            .unwrap()
            .remove(0);
        assert_eq!(pending.destination, "research_email");
        assert_eq!(pending.attempt_number, 2);
        let frozen = store
            .frozen_smtp(&pending.event_id, &pending.destination)
            .unwrap()
            .unwrap()
            .data()
            .to_vec();
        let bound = store
            .bound_smtp_provider(&pending.event_id, &pending.destination)
            .unwrap()
            .unwrap();
        drop(store);
        // Operational timeout edits do not change identity, trust or frozen bytes.
        v["providers"]["research_email"]["request_timeout_milliseconds"] = json!(2000);
        let recovered = prepare_fixture(&v);
        assert_eq!(
            recovered
                .revisions()
                .iter()
                .find(|(name, _, _)| *name == "research_email")
                .unwrap()
                .2,
            bound.token()
        );
        let mut store = Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        assert_eq!(
            run(&recovered, &mut store, 7205)
                .delivery
                .unwrap()
                .unwrap()
                .delivered,
            1
        );
        assert!(store
            .due_deliveries("research-cluster", 7206, 10)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .frozen_smtp(&pending.event_id, &pending.destination)
                .unwrap()
                .unwrap()
                .data(),
            frozen
        );
    }

    #[test]
    fn composed_identity_and_event_budgets_preserve_pending_work_and_recover_after_reopen() {
        use crate::{
            polling_cycle::{Cycles, Hooks, Outcome, StageError},
            storage::DueDelivery,
        };
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        v["monitor"]["retained_identity_limit"] = json!(2);
        v["monitor"]["retained_event_limit"] = json!(2);
        v["routes"] = json!([{"events":["JOB_COMPLETED","JOB_STARTED"],"send_to":["primary"]}]);
        let path = config_path(&d);
        let plan = prepare_file_fixture(&v, &path);
        let mut store = open_owned_fixture(&plan);
        let run = |plan: &Plan,
                   store: &mut Store,
                   records: &str,
                   now,
                   stop_after_poll: bool,
                   forbid_query: bool| {
            let mut cancellations = 0;
            let mut sends = 0;
            let report = plan
                .tick_with(
                    (Path::new("squeue"), Path::new("sacct")),
                    &mut Cycles::new(1).unwrap(),
                    store,
                    Hooks {
                        query: |cmd: &mut std::process::Command, _, _| {
                            assert!(!forbid_query);
                            Ok(if cmd.get_program() == "squeue" {
                                ""
                            } else {
                                records
                            }
                            .into())
                        },
                        send: (
                            |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                                panic!("ordinary credentials")
                            },
                            |_: &DueDelivery| -> std::process::Command {
                                panic!("ordinary SMTP helper")
                            },
                            |due: &DueDelivery| {
                                assert_eq!(due.destination, "primary");
                                sends += 1;
                                Ok(crate::delivery::ProviderOutcome::Delivered)
                            },
                        ),
                        clock: || Some(now),
                        cancelled: || {
                            cancellations += 1;
                            stop_after_poll && cancellations > 1
                        },
                    },
                )
                .unwrap();
            (report, sends)
        };
        let (first, _) = run(
            &plan,
            &mut store,
            "research-cluster|1|alice|s|actual|RUNNING|0:0",
            7200,
            false,
            false,
        );
        let health = first.health.unwrap().unwrap().storage;
        assert!(!health.jobs_over_capacity && !health.events_over_capacity);
        let completed = "research-cluster|1|alice|s|actual|COMPLETED|0:0";
        let (second, sends) = run(&plan, &mut store, completed, 7260, true, false);
        assert!(second.cancelled);
        assert_eq!(sends, 0);
        let pending = store
            .due_deliveries("research-cluster", 7260, 10)
            .unwrap()
            .remove(0);
        assert_eq!(pending.attempt_number, 1);
        drop(store);
        v["monitor"]["retained_identity_limit"] = json!(1);
        v["monitor"]["retained_event_limit"] = json!(1);
        let reduced = prepare_file_fixture(&v, &path);
        let mut store = open_owned_fixture(&reduced);
        let (blocked, sends) = run(&reduced, &mut store, completed, 7320, false, true);
        assert_eq!(blocked.preflight, Some(Err(StageError::Capacity)));
        let pressure = blocked.capacity_pressure.unwrap();
        assert!(!pressure.jobs_over_capacity && pressure.events_over_capacity);
        assert_eq!(sends, 0);
        assert!(blocked.poll.is_none() && blocked.delivery.is_none() && blocked.cleanup.is_none());
        assert_eq!(
            store.due_deliveries("research-cluster", 7320, 10).unwrap()[0].event_id,
            pending.event_id
        );
        assert!(store
            .restore_with_capacities("research-cluster", (1, 1))
            .is_err());
        assert_eq!(
            store
                .restore_with_capacities("research-cluster", (1, 2))
                .unwrap()
                .0
                .emitted
                .len(),
            2
        );
        drop(store);
        v["monitor"]["retained_identity_limit"] = json!(2);
        v["monitor"]["retained_event_limit"] = json!(2);
        let exact = prepare_file_fixture(&v, &path);
        let mut store = open_owned_fixture(&exact);
        let (recovered, sends) = run(&exact, &mut store, completed, 7320, false, false);
        assert_eq!(sends, 1);
        assert!(matches!(
            recovered.poll,
            Some(Ok(Outcome::Committed { events: 0, .. }))
        ));
        let health = recovered.health.unwrap().unwrap().storage;
        assert!(!health.jobs_over_capacity && !health.events_over_capacity);
        let new_running = "research-cluster|2|alice|s|actual|RUNNING|0:0";
        let checkpoint = store
            .restore_with_capacities("research-cluster", (1, 2))
            .unwrap()
            .1;
        let (overflow, sends) = run(&exact, &mut store, new_running, 7380, false, false);
        assert_eq!(overflow.poll, Some(Err(StageError::Poll)));
        assert_eq!(sends, 0);
        assert_eq!(overflow.delivery.unwrap().unwrap().delivered, 0);
        let (restored, unchanged) = store
            .restore_with_capacities("research-cluster", (1, 2))
            .unwrap();
        assert_eq!(unchanged, checkpoint);
        assert_eq!(restored.emitted.len(), 2);
        assert_eq!(
            restored.latest.values().next(),
            Some(&crate::lifecycle::State::Completed)
        );
        drop(store);
        v["monitor"]["retained_event_limit"] = json!(3);
        let raised = prepare_file_fixture(&v, &path);
        let mut store = open_owned_fixture(&raised);
        let (last, sends) = run(&raised, &mut store, new_running, 7380, false, false);
        assert_eq!(sends, 1);
        assert!(matches!(
            last.poll,
            Some(Ok(Outcome::Committed { events: 1, .. }))
        ));
        let (identity_overflow, sends) = run(
            &raised,
            &mut store,
            "research-cluster|3|alice|s|actual|PENDING|0:0",
            7440,
            false,
            false,
        );
        assert_eq!(identity_overflow.poll, Some(Err(StageError::Poll)));
        assert_eq!(sends, 0);
        let (restored, _) = store
            .restore_with_capacities("research-cluster", (2, 3))
            .unwrap();
        assert_eq!((restored.latest.len(), restored.emitted.len()), (2, 3));
    }

    #[test]
    fn composed_status_reads_fresh_stale_and_pressure_snapshots_without_resources_or_writes() {
        use crate::polling_cycle::{Cycles, Hooks, ReportError};
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        v["monitor"]["retained_identity_limit"] = json!(1);
        v["monitor"]["retained_event_limit"] = json!(2);
        let plan = prepare_file_fixture(&v, &config_path(&d));
        let mut store = open_owned_fixture(&plan);
        let mut cycles = Cycles::new(2).unwrap();
        let cold = plan.status(&cycles, &mut store, 7200).unwrap();
        assert!(cold.poll.stale);
        assert_eq!(cold.poll.checkpoint_age_seconds, None);
        for (state, now) in [("RUNNING", 7200), ("COMPLETED", 7260)] {
            let mut samples = 0;
            let report = plan
                .tick_with(
                    (Path::new("squeue"), Path::new("sacct")),
                    &mut cycles,
                    &mut store,
                    Hooks {
                        query: |cmd: &mut std::process::Command, _, _| {
                            Ok(if cmd.get_program() == "squeue" {
                                String::new()
                            } else {
                                format!("research-cluster|1|alice|s|actual|{state}|0:0")
                            })
                        },
                        send: (
                            |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                                panic!("status credentials")
                            },
                            |_: &crate::storage::DueDelivery| -> std::process::Command {
                                panic!("status helper")
                            },
                            |_: &crate::storage::DueDelivery| panic!("status ordinary"),
                        ),
                        clock: || Some(now),
                        cancelled: || {
                            samples += 1;
                            samples > 1
                        },
                    },
                )
                .unwrap();
            assert!(report.cancelled);
        }
        let pending = store.due_deliveries("research-cluster", 7260, 10).unwrap();
        assert_eq!(pending.len(), 1);
        let frozen = store
            .frozen_smtp(&pending[0].event_id, "research_email")
            .unwrap()
            .unwrap()
            .data()
            .to_vec();
        drop(store);
        let restarted = Cycles::new(1).unwrap();
        v["monitor"]["retained_event_limit"] = json!(1);
        let reduced = prepare_file_fixture(&v, &config_path(&d));
        let mut store = open_owned_fixture(&reduced);
        for (now, age, stale) in [(7260, 0, false), (7439, 179, false), (7440, 180, true)] {
            let report = reduced.status(&restarted, &mut store, now).unwrap();
            assert_eq!(
                (report.poll.checkpoint_age_seconds, report.poll.stale),
                (Some(age), stale)
            );
            assert!(!report.storage.jobs_over_capacity && report.storage.events_over_capacity);
            let printed = format!("{report:?}");
            for private in [
                "alice",
                "researcher",
                "research-cluster",
                "smtp.example.invalid",
                "Bearer fixture",
                "SLURM_NOTIFY",
                config_path(&d).to_str().unwrap(),
            ] {
                assert!(!printed.contains(private));
            }
        }
        assert!(matches!(
            reduced.status(&restarted, &mut store, 7259),
            Err(ReportError::ClockRegressed)
        ));
        assert!(matches!(
            reduced.status(&restarted, &mut store, -1),
            Err(ReportError::InvalidInput)
        ));
        let after = store.due_deliveries("research-cluster", 7440, 10).unwrap();
        assert_eq!(after.len(), pending.len());
        assert_eq!(
            (&after[0].event_id, after[0].attempt_number),
            (&pending[0].event_id, pending[0].attempt_number)
        );
        assert_eq!(
            store
                .frozen_smtp(&pending[0].event_id, "research_email")
                .unwrap()
                .unwrap()
                .data(),
            frozen
        );
    }
    #[test]
    fn composed_status_rejects_corrupt_checkpoint_and_invalid_policy_statically() {
        use crate::polling_cycle::{Cycles, ReportError};
        let d = crate::storage::tests::Directory::new();
        let mut v = fixture(&d);
        v["storage"]["path"] = json!(owned_path(&d));
        let mut plan = prepare_file_fixture(&v, &config_path(&d));
        let mut store = open_owned_fixture(&plan);
        let cycles = Cycles::new(1).unwrap();
        plan.common.controls.event_limit = 0;
        assert!(matches!(
            plan.status(&cycles, &mut store, 7200),
            Err(ReportError::InvalidInput)
        ));
        plan.common.controls.event_limit = 20000;
        let db = rusqlite::Connection::open(owned_path(&d)).unwrap();
        db.execute(
            "INSERT INTO checkpoints(cluster,history_through) VALUES (?1,?2)",
            rusqlite::params!["research-cluster", "synthetic corrupt checkpoint"],
        )
        .unwrap();
        assert!(matches!(
            plan.status(&cycles, &mut store, 7200),
            Err(ReportError::Storage)
        ));
        db.execute("DROP TABLE checkpoints", []).unwrap();
        assert!(matches!(
            plan.status(&cycles, &mut store, 7200),
            Err(ReportError::Storage)
        ));
    }
    #[test]
    fn changed_trust_after_restart_rejects_old_bound_profile_before_credentials() {
        let d = crate::storage::tests::Directory::new();
        let v = fixture(&d);
        let old = prepare_fixture(&v);
        let path = v["providers"]["research_email"]["trust_der_files"][0]
            .as_str()
            .unwrap();
        let cert = rcgen::generate_simple_self_signed(vec!["smtp.example.invalid".into()]).unwrap();
        std::fs::write(path, cert.cert.der()).unwrap();
        let current = prepare_fixture(&v);
        assert_ne!(old.revisions()[0].2, current.revisions()[0].2);
        assert!(matches!(
            current.resolve(&old.smtp[0].descriptor, |_, _| panic!(
                "changed trust credentials"
            )),
            Err(Error::Profile)
        ));
        assert!(old
            .resolve(&old.smtp[0].descriptor, |name, _| Ok(Zeroizing::new(
                if name.ends_with("USERNAME") {
                    b"researcher".to_vec()
                } else {
                    b"fixture".to_vec()
                }
            )))
            .is_ok());
    }
}

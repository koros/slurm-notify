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
fn reference(purpose: &[u8], values: &[&str]) -> String {
    use ring::digest::{Context, SHA256};
    let mut hash = Context::new(&SHA256);
    hash.update(b"slurm-notify-private-config-reference-v1\0");
    hash.update(purpose);
    for value in values {
        hash.update(&(value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    hash.finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
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
        let mut names: Vec<_> = self
            .common
            .webhooks
            .iter()
            .map(|w| w.alias.as_str())
            .collect();
        names.extend(self.smtp.iter().map(|s| s.name.as_str()));
        let routes: Vec<_> = self.smtp.iter().map(Smtp::route).collect();
        let profiles: Vec<_> = self.smtp.iter().map(|s| s.descriptor.profile()).collect();
        let rendering = smtp_routes::Plan::new(&names, &routes, self.max_messages)
            .map_err(|_| crate::smtp_poll::Error::Preparation)?;
        let provider = smtp_routes::ProviderPlan::new(&rendering, &profiles)
            .map_err(|_| crate::smtp_poll::Error::Preparation)?;
        provider.poll_selected_with(
            &crate::scheduler::PollRequest {
                active_program: input.active_program,
                accounting_program: input.accounting_program,
                cluster: self.common.observation.cluster(),
                user: self.common.observation.user(),
                history_start: input.history_start,
                history_end: input.history_end,
                timeout: self.common.controls.query_timeout,
                output_limit: self.common.controls.output_limit,
                emit_discovered: self.common.observation.emit_discovered(),
            },
            reconciler,
            store,
            input.observed_at,
            query,
            |event| self.common.routes.destinations(&event.state),
        )
    }
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

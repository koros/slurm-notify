//! Private pair permission and resolution guard for caller-selected bound transport.
use super::{
    delivery::{self, BatchFailure, BatchReport, ProviderOutcome, RetryPolicy},
    smtp_auth::Credentials,
    smtp_provider_revision::{self, Descriptor, Profile},
    smtp_revision,
    storage::{DueDelivery, FrozenMessage, Store},
    supervised_provider::Diagnostic,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Policy,
    Profile,
    Trust,
    Credentials,
}

pub(super) struct Permission<'a> {
    pub(super) cluster: &'a str,
    pub(super) destination: &'a str,
    pub(super) rendering: &'a str,
    pub(super) provider: &'a str,
}
pub(super) struct Permissions<'a> {
    entries: &'a [Permission<'a>],
}
impl<'a> Permissions<'a> {
    pub(super) fn new(entries: &'a [Permission<'a>]) -> Result<Self, Error> {
        if entries.len() > 64 {
            return Err(Error::Policy);
        }
        let mut seen = std::collections::HashSet::new();
        for entry in entries {
            super::smtp_revision_delivery::Permissions::new(&[
                super::smtp_revision_delivery::Permission {
                    cluster: entry.cluster,
                    destination: entry.destination,
                    revision: entry.rendering,
                },
            ])
            .map_err(|_| Error::Policy)?;
            if entry.provider.len() != 64
                || !entry
                    .provider
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || !seen.insert((
                    entry.cluster,
                    entry.destination,
                    entry.rendering,
                    entry.provider,
                ))
            {
                return Err(Error::Policy);
            }
        }
        Ok(Self { entries })
    }
    fn permits(&self, cluster: &str, destination: &str, rendering: &str, provider: &str) -> bool {
        self.entries.iter().any(|p| {
            p.cluster == cluster
                && p.destination == destination
                && p.rendering == rendering
                && p.provider == provider
        })
    }
}

// No Debug/Clone: credentials and resolved identities must not enter diagnostics.
// Constructor validates actual DER trust content and authentication identity;
// a profile fingerprint alone is insufficient evidence of resolved resources.
pub(super) struct Resolved {
    provider: Descriptor,
    roots: Vec<Vec<u8>>,
    credentials: Credentials,
}
impl Resolved {
    pub(super) fn new(
        bound: &Descriptor,
        profile: &Profile<'_>,
        roots: &[&[u8]],
        username: &str,
        password: &str,
    ) -> Result<Self, Error> {
        if !bound.matches(profile).map_err(|_| Error::Profile)?
            || username != profile.authentication_identity
        {
            return Err(Error::Profile);
        }
        if smtp_provider_revision::trust_fingerprint(roots).map_err(|_| Error::Trust)?
            != profile.trust_fingerprint
        {
            return Err(Error::Trust);
        }
        let credentials =
            Credentials::new(Some(username), Some(password)).map_err(|_| Error::Credentials)?;
        Ok(Self {
            provider: Descriptor::new(profile).map_err(|_| Error::Profile)?,
            roots: roots.iter().map(|root| root.to_vec()).collect(),
            credentials,
        })
    }
    pub(super) fn provider(&self) -> &Descriptor {
        &self.provider
    }
    pub(super) fn into_parts(self) -> (Descriptor, Vec<Vec<u8>>, Credentials) {
        (self.provider, self.roots, self.credentials)
    }
}

pub(super) struct Hooks<'a, R, S, C, X> {
    pub(super) permissions: &'a Permissions<'a>,
    // Local caller-managed resolution only; no implicit environment/file lookup.
    pub(super) resolve: R,
    pub(super) send: S,
    pub(super) clock: C,
    pub(super) cancelled: X,
}
pub(super) fn deliver_due<R, S, C, X>(
    store: &mut Store,
    cluster: &str,
    now: i64,
    limit: usize,
    policy: RetryPolicy,
    mut hooks: Hooks<'_, R, S, C, X>,
) -> Result<BatchReport, BatchFailure>
where
    R: FnMut(&Descriptor) -> Result<Resolved, Error>,
    S: FnMut(
        &DueDelivery,
        &smtp_revision::Descriptor,
        &FrozenMessage,
        Resolved,
    ) -> Result<ProviderOutcome, Diagnostic>,
    C: FnMut() -> Option<i64>,
    X: FnMut() -> bool,
{
    delivery::deliver_due_checked(
        store,
        cluster,
        now,
        limit,
        policy,
        delivery::CheckedHooks {
            authorize: |store: &Store, item: &DueDelivery| {
                authorize(store, item, cluster, hooks.permissions, &mut hooks.resolve)
            },
            send: |item: &DueDelivery,
                   (rendering, frozen, resolved): (
                smtp_revision::Descriptor,
                FrozenMessage,
                Resolved,
            )| (hooks.send)(item, &rendering, &frozen, resolved),
            clock: hooks.clock,
            cancelled: hooks.cancelled,
        },
    )
}

type Context = (smtp_revision::Descriptor, FrozenMessage, Resolved);
fn authorize(
    store: &Store,
    item: &DueDelivery,
    cluster: &str,
    permissions: &Permissions<'_>,
    resolve: &mut impl FnMut(&Descriptor) -> Result<Resolved, Error>,
) -> Result<Context, Diagnostic> {
    let rendering = store
        .bound_smtp_revision(&item.event_id, &item.destination)
        .map_err(|_| Diagnostic::SmtpRevisionInvalid)?
        .ok_or(Diagnostic::SmtpRevisionMissing)?;
    let provider = store
        .bound_smtp_provider(&item.event_id, &item.destination)
        .map_err(|_| Diagnostic::SmtpProviderInvalid)?
        .ok_or(Diagnostic::SmtpProviderMissing)?;
    if !permissions.permits(
        cluster,
        &item.destination,
        rendering.token(),
        provider.token(),
    ) {
        return Err(Diagnostic::SmtpProviderDenied);
    }
    let resolved = (resolve)(&provider).map_err(Diagnostic::SmtpProviderResolution)?;
    if resolved.provider().token() != provider.token()
        || resolved.provider().canonical() != provider.canonical()
    {
        return Err(Diagnostic::SmtpProviderResolution(Error::Profile));
    }
    let frozen = store
        .frozen_smtp(&item.event_id, &item.destination)
        .map_err(|_| Diagnostic::SmtpRevisionInvalid)?
        .ok_or(Diagnostic::SmtpRevisionMissing)?;
    Ok((rendering, frozen, resolved))
}

pub(super) struct MixedHooks<'a, R, S, O, C, X> {
    pub(super) smtp: Hooks<'a, R, S, C, X>,
    pub(super) ordinary_destinations: &'a [&'a str],
    pub(super) ordinary_send: O,
}
pub(super) fn deliver_due_mixed<R, S, O, C, X>(
    store: &mut Store,
    cluster: &str,
    now: i64,
    limit: usize,
    policy: RetryPolicy,
    mut mixed: MixedHooks<'_, R, S, O, C, X>,
) -> Result<BatchReport, BatchFailure>
where
    R: FnMut(&Descriptor) -> Result<Resolved, Error>,
    S: FnMut(
        &DueDelivery,
        &smtp_revision::Descriptor,
        &FrozenMessage,
        Resolved,
    ) -> Result<ProviderOutcome, Diagnostic>,
    O: FnMut(&DueDelivery) -> Result<ProviderOutcome, Diagnostic>,
    C: FnMut() -> Option<i64>,
    X: FnMut() -> bool,
{
    let hooks = &mut mixed.smtp;
    delivery::deliver_due_checked(
        store,
        cluster,
        now,
        limit,
        policy,
        delivery::CheckedHooks {
            authorize: |store: &Store, item: &DueDelivery| {
                if mixed
                    .ordinary_destinations
                    .contains(&item.destination.as_str())
                {
                    if store
                        .has_smtp_artifacts(&item.event_id, &item.destination)
                        .map_err(|_| Diagnostic::SmtpRevisionInvalid)?
                    {
                        return Err(Diagnostic::SmtpProviderDenied);
                    }
                    Ok(None)
                } else {
                    authorize(store, item, cluster, hooks.permissions, &mut hooks.resolve).map(Some)
                }
            },
            send: |item: &DueDelivery, context: Option<Context>| match context {
                Some((rendering, frozen, resolved)) => {
                    (hooks.send)(item, &rendering, &frozen, resolved)
                }
                None => (mixed.ordinary_send)(item),
            },
            clock: &mut hooks.clock,
            cancelled: &mut hooks.cancelled,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)] // Custom acceptance harness compiles helpers without unit tests.
    use crate::{
        lifecycle::Reconciler,
        scheduler::PollRequest,
        smtp_render::JobVisibility,
        smtp_routes::{Metadata, Plan, ProviderPlan, Route},
        storage::{self, DeliveryOutcome},
    };
    use std::{path::Path, time::Duration};
    struct Fixture {
        root: Vec<u8>,
        hash: String,
        provider: Descriptor,
    }
    impl Fixture {
        fn new() -> Self {
            let root = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()])
                .unwrap()
                .serialize_der()
                .unwrap();
            let hash = smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
            let provider = Descriptor::new(&Profile {
                trust_fingerprint: &hash,
                ..profile()
            })
            .unwrap();
            Self {
                root,
                hash,
                provider,
            }
        }
        fn resolve(&self, bound: &Descriptor, password: &str) -> Result<Resolved, Error> {
            Resolved::new(
                bound,
                &self.provider.profile(),
                &[&self.root],
                "fixture-user",
                password,
            )
        }
    }
    fn profile() -> Profile<'static> {
        Profile {
            destination: "smtp",
            relay: "relay.fixture.invalid",
            port: 587,
            tls: smtp_provider_revision::TlsMode::RequiredStartTls,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        }
    }
    fn open(directory: &storage::tests::Directory) -> Store {
        Store::open(&directory.path(), Duration::from_millis(50)).unwrap()
    }
    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            initial_delay_seconds: 5,
            max_delay_seconds: 20,
            jitter_seconds: 0,
            jitter_seed: 0,
        }
    }
    fn seed(store: &mut Store, fixture: &Fixture, mode: u8) {
        let routes = [Route {
            name: "smtp",
            visibility: JobVisibility::Hidden,
            metadata: Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: "fixture",
            },
        }];
        let rendering = Plan::new(&["smtp"], &routes, 4).unwrap();
        let request = PollRequest {
            active_program: Path::new("squeue"),
            accounting_program: Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        let query = |command: &mut std::process::Command, _, _| {
            Ok(if command.get_program() == "squeue" {
                ""
            } else {
                "cluster|1|alice|s|actual|COMPLETED|0:0"
            }
            .into())
        };
        let mut reconciler = Reconciler::with_capacity(10);
        match mode {
            0 => ProviderPlan::new(&rendering, &[fixture.provider.profile()])
                .unwrap()
                .poll_with(&request, &mut reconciler, store, 10, query)
                .unwrap(),
            1 => rendering
                .poll_revision_with(&request, &mut reconciler, store, 10, query)
                .unwrap(),
            _ => rendering
                .poll_with(&request, &mut reconciler, store, 10, query)
                .unwrap(),
        };
    }
    #[test]
    fn ordinary_route_cannot_downgrade_any_durable_smtp_evidence() {
        for mode in 0..3 {
            let directory = storage::tests::Directory::new();
            let mut store = open(&directory);
            let fixture = Fixture::new();
            seed(&mut store, &fixture, mode);
            let denied = Permissions::new(&[]).unwrap();
            let result = deliver_due_mixed(
                &mut store,
                "cluster",
                10,
                10,
                policy(),
                MixedHooks {
                    ordinary_destinations: &["smtp"],
                    ordinary_send: |_: &DueDelivery| panic!("SMTP downgrade sent ordinary payload"),
                    smtp: Hooks {
                        permissions: &denied,
                        resolve: |_: &Descriptor| -> Result<Resolved, Error> {
                            panic!("downgrade resolved")
                        },
                        send: |_: &DueDelivery,
                               _: &smtp_revision::Descriptor,
                               _: &FrozenMessage,
                               _: Resolved| {
                            panic!("downgrade sent SMTP")
                        },
                        clock: || panic!("downgrade clock"),
                        cancelled: || false,
                    },
                },
            )
            .unwrap_err();
            assert_eq!(result.kind, delivery::FailureKind::DispatchBlocked);
            assert_eq!(result.committed.delivered, 0);
            drop(store);
            let store = open(&directory);
            assert_eq!(
                store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
                1
            );
        }
    }

    #[test]
    fn resolution_requires_actual_trust_and_auth_identity_but_allows_secret_rotation() {
        let fixture = Fixture::new();
        for password in ["first-fixture", "rotated-fixture"] {
            let resolved = fixture.resolve(&fixture.provider, password).unwrap();
            assert_eq!(resolved.provider().token(), fixture.provider.token());
            let (provider, roots, _credentials) = resolved.into_parts();
            assert_eq!(provider.profile().authentication_identity, "fixture-user");
            assert_eq!(roots, vec![fixture.root.clone()]);
        }
        assert!(matches!(
            fixture.resolve(&fixture.provider, ""),
            Err(Error::Credentials)
        ));
        assert!(matches!(
            Resolved::new(
                &fixture.provider,
                &fixture.provider.profile(),
                &[&fixture.root],
                "other-user",
                "fixture"
            ),
            Err(Error::Profile)
        ));
        assert!(matches!(
            Resolved::new(
                &fixture.provider,
                &fixture.provider.profile(),
                &[],
                "fixture-user",
                "fixture"
            ),
            Err(Error::Trust)
        ));
        let changed = Fixture::new();
        assert!(matches!(
            Resolved::new(
                &fixture.provider,
                &fixture.provider.profile(),
                &[&changed.root],
                "fixture-user",
                "fixture"
            ),
            Err(Error::Trust)
        ));
        let changed = Profile {
            relay: "other.fixture.invalid",
            trust_fingerprint: &fixture.hash,
            ..profile()
        };
        assert!(matches!(
            Resolved::new(
                &fixture.provider,
                &changed,
                &[&fixture.root],
                "fixture-user",
                "fixture"
            ),
            Err(Error::Profile)
        ));
        assert_eq!(format!("{:?}", Error::Credentials), "Credentials");
    }
    #[test]
    fn pair_permissions_are_bounded_and_require_all_four_fields() {
        let token = "a".repeat(64);
        let other = "b".repeat(64);
        let entries = [
            Permission {
                cluster: "cluster",
                destination: "smtp",
                rendering: &token,
                provider: &token,
            },
            Permission {
                cluster: "cluster",
                destination: "smtp",
                rendering: &token,
                provider: &other,
            },
        ];
        let permissions = Permissions::new(&entries).unwrap();
        assert!(permissions.permits("cluster", "smtp", &token, &other));
        for (cluster, destination, rendering, provider) in [
            ("other", "smtp", token.as_str(), other.as_str()),
            ("cluster", "other", token.as_str(), other.as_str()),
            ("cluster", "smtp", other.as_str(), other.as_str()),
            ("cluster", "smtp", token.as_str(), "unknown"),
        ] {
            assert!(!permissions.permits(cluster, destination, rendering, provider));
        }
        assert!(!Permissions::new(&[])
            .unwrap()
            .permits("cluster", "smtp", &token, &token));
        let entry = || Permission {
            cluster: "cluster",
            destination: "smtp",
            rendering: &token,
            provider: &token,
        };
        assert!(matches!(
            Permissions::new(&[entry(), entry()]),
            Err(Error::Policy)
        ));
        let many: Vec<_> = (0..65).map(|_| entry()).collect();
        assert!(matches!(Permissions::new(&many), Err(Error::Policy)));
        for (cluster, provider) in [("", token.as_str()), ("cluster", "INVALID")] {
            assert!(Permissions::new(&[Permission {
                cluster,
                destination: "smtp",
                rendering: &token,
                provider
            }])
            .is_err());
        }
    }
    #[test]
    fn missing_corrupt_and_denied_pairs_block_before_resolution_send_and_retirement() {
        for mode in 0..4 {
            let fixture = Fixture::new();
            let directory = storage::tests::Directory::new();
            let mut store = open(&directory);
            seed(&mut store, &fixture, if mode == 3 { 0 } else { mode });
            if mode == 3 {
                rusqlite::Connection::open(directory.path())
                    .unwrap()
                    .execute(
                        "UPDATE smtp_provider_revisions SET descriptor=?1",
                        [b"bad".as_slice()],
                    )
                    .unwrap();
            }
            let permissions = Permissions::new(&[]).unwrap();
            let failure = deliver_due(
                &mut store,
                "cluster",
                10,
                10,
                policy(),
                Hooks {
                    permissions: &permissions,
                    resolve: |_: &Descriptor| panic!("blocked pair resolved"),
                    send: |_: &DueDelivery,
                           _: &smtp_revision::Descriptor,
                           _: &FrozenMessage,
                           _: Resolved| panic!("blocked pair sent"),
                    clock: || panic!("blocked pair clock"),
                    cancelled: || false,
                },
            )
            .unwrap_err();
            assert_eq!(
                failure.diagnostic,
                Some(match mode {
                    0 => Diagnostic::SmtpProviderDenied,
                    1 => Diagnostic::SmtpProviderMissing,
                    2 => Diagnostic::SmtpRevisionMissing,
                    _ => Diagnostic::SmtpProviderInvalid,
                })
            );
            assert!(!failure.acceptance_uncertain);
            assert_eq!(
                store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
                1
            );
        }
        let fixture = Fixture::new();
        let directory = storage::tests::Directory::new();
        let mut store = open(&directory);
        seed(&mut store, &fixture, 0);
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        store
            .record_delivery(&item, DeliveryOutcome::TransientFailure, 10, Some(15))
            .unwrap();
        let permissions = Permissions::new(&[]).unwrap();
        let mut lowered = policy();
        lowered.max_attempts = 1;
        let failure = deliver_due(
            &mut store,
            "cluster",
            15,
            10,
            lowered,
            Hooks {
                permissions: &permissions,
                resolve: |_: &Descriptor| panic!("denied exhausted resolved"),
                send: |_: &DueDelivery,
                       _: &smtp_revision::Descriptor,
                       _: &FrozenMessage,
                       _: Resolved| panic!("denied exhausted sent"),
                clock: || None,
                cancelled: || false,
            },
        )
        .unwrap_err();
        assert_eq!(failure.diagnostic, Some(Diagnostic::SmtpProviderDenied));
        assert_eq!(
            store.due_deliveries("cluster", 15, 10).unwrap()[0].attempt_number,
            2
        );
    }
    #[test]
    fn unresolved_and_wrong_returned_profiles_block_without_attempts() {
        let fixture = Fixture::new();
        let directory = storage::tests::Directory::new();
        let mut store = open(&directory);
        seed(&mut store, &fixture, 0);
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let rendering = store
            .bound_smtp_revision(&item.event_id, "smtp")
            .unwrap()
            .unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "smtp",
            rendering: rendering.token(),
            provider: fixture.provider.token(),
        }];
        let permissions = Permissions::new(&entries).unwrap();
        for error in [Error::Profile, Error::Trust, Error::Credentials] {
            let failure = deliver_due(
                &mut store,
                "cluster",
                10,
                10,
                policy(),
                Hooks {
                    permissions: &permissions,
                    resolve: |_: &Descriptor| Err(error),
                    send: |_: &DueDelivery,
                           _: &smtp_revision::Descriptor,
                           _: &FrozenMessage,
                           _: Resolved| panic!("unresolved sent"),
                    clock: || panic!("unresolved clock"),
                    cancelled: || false,
                },
            )
            .unwrap_err();
            assert_eq!(
                failure.diagnostic,
                Some(Diagnostic::SmtpProviderResolution(error))
            );
        }
        let changed = Descriptor::new(&Profile {
            relay: "other.fixture.invalid",
            trust_fingerprint: &fixture.hash,
            ..profile()
        })
        .unwrap();
        let failure = deliver_due(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                resolve: |_: &Descriptor| {
                    Resolved::new(
                        &changed,
                        &changed.profile(),
                        &[&fixture.root],
                        "fixture-user",
                        "fixture",
                    )
                },
                send: |_: &DueDelivery,
                       _: &smtp_revision::Descriptor,
                       _: &FrozenMessage,
                       _: Resolved| panic!("wrong profile sent"),
                clock: || None,
                cancelled: || false,
            },
        )
        .unwrap_err();
        assert_eq!(
            failure.diagnostic,
            Some(Diagnostic::SmtpProviderResolution(Error::Profile))
        );
        assert_eq!(
            store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
            1
        );
    }
    #[test]
    fn resolved_retry_reopen_preserves_pair_and_snapshot_with_rotated_secret() {
        let fixture = Fixture::new();
        let directory = storage::tests::Directory::new();
        let mut store = open(&directory);
        seed(&mut store, &fixture, 0);
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let rendering = store
            .bound_smtp_revision(&item.event_id, "smtp")
            .unwrap()
            .unwrap();
        let saved = store.frozen_smtp(&item.event_id, "smtp").unwrap().unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "smtp",
            rendering: rendering.token(),
            provider: fixture.provider.token(),
        }];
        let permissions = Permissions::new(&entries).unwrap();
        let report = deliver_due(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                resolve: |bound: &Descriptor| fixture.resolve(bound, "first"),
                send: |_: &DueDelivery,
                       render: &smtp_revision::Descriptor,
                       frozen: &FrozenMessage,
                       resolved: Resolved| {
                    assert_eq!(render.token(), rendering.token());
                    assert_eq!(frozen.data(), saved.data());
                    assert_eq!(resolved.provider().token(), fixture.provider.token());
                    let _ = resolved.into_parts();
                    Ok(ProviderOutcome::Retry)
                },
                clock: || Some(12),
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(report.retrying, 1);
        drop(store);
        let mut store = open(&directory);
        let failure = deliver_due(
            &mut store,
            "cluster",
            17,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                resolve: |_: &Descriptor| panic!("cancelled resolution"),
                send: |_: &DueDelivery,
                       _: &smtp_revision::Descriptor,
                       _: &FrozenMessage,
                       _: Resolved| panic!("cancelled send"),
                clock: || None,
                cancelled: || true,
            },
        )
        .unwrap_err();
        assert_eq!(failure.kind, delivery::FailureKind::Cancelled);
        let report = deliver_due(
            &mut store,
            "cluster",
            17,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                resolve: |bound: &Descriptor| fixture.resolve(bound, "rotated"),
                send: |item: &DueDelivery,
                       render: &smtp_revision::Descriptor,
                       frozen: &FrozenMessage,
                       resolved: Resolved| {
                    assert_eq!(item.attempt_number, 2);
                    assert_eq!(render.token(), rendering.token());
                    assert_eq!(frozen.data(), saved.data());
                    assert_eq!(resolved.provider().token(), fixture.provider.token());
                    let _ = resolved.into_parts();
                    Ok(ProviderOutcome::Delivered)
                },
                clock: || Some(18),
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(report.delivered, 1);
        assert!(store.due_deliveries("cluster", 18, 10).unwrap().is_empty());
    }
}

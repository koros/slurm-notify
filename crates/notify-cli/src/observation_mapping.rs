//! Private identity/observation mapping; installed identity binding remains pending.
use zeroize::Zeroizing;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Policy,
    Lookup,
    Identity,
}
// No Debug/Clone: scheduler identity and cluster are private local metadata.
pub(super) struct Plan {
    cluster: Zeroizing<String>,
    user: Zeroizing<Vec<u8>>,
    emit: bool,
}
impl Plan {
    pub(super) fn cluster(&self) -> &str {
        &self.cluster
    }
    pub(super) fn user(&self) -> &str {
        std::str::from_utf8(&self.user).expect("validated ASCII identity")
    }
    pub(super) fn emit_discovered(&self) -> bool {
        self.emit
    }
}
pub(super) fn map(
    cluster: &str,
    user_jobs_only: bool,
    initial: &str,
    lookup: impl FnOnce(usize) -> Result<Zeroizing<Vec<u8>>, ()>,
) -> Result<Plan, Error> {
    if !user_jobs_only
        || cluster.is_empty()
        || cluster.len() > 64
        || !cluster
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(Error::Policy);
    }
    let emit = match initial {
        "emit" => true,
        "suppress" => false,
        _ => return Err(Error::Policy),
    };
    let user = lookup(256).map_err(|_| Error::Lookup)?;
    if !valid_identity(&user) {
        return Err(Error::Identity);
    }
    Ok(Plan {
        cluster: Zeroizing::new(cluster.to_owned()),
        user,
        emit,
    })
}
// Private limitation: ASCII without separators, whitespace or flag prefixes.
pub(super) fn valid_identity(user: &[u8]) -> bool {
    !user.is_empty()
        && user.len() <= 256
        && user[0] != b'-'
        && user
            .iter()
            .all(|byte| (33..=126).contains(byte) && *byte != b'|')
}
#[cfg(test)]
mod tests {
    use super::*;
    fn plan(initial: &str) -> Plan {
        map("cluster", true, initial, |limit| {
            assert_eq!(limit, 256);
            Ok(Zeroizing::new(b"alice".to_vec()))
        })
        .unwrap()
    }
    #[test]
    fn policy_preflight_never_looks_up_identity() {
        for (cluster, only, initial) in [
            ("cluster", false, "emit"),
            ("", true, "emit"),
            ("bad cluster", true, "emit"),
            ("cluster", true, "default"),
            (&"x".repeat(65), true, "emit"),
        ] {
            assert!(matches!(
                map(cluster, only, initial, |_| panic!("invalid policy lookup")),
                Err(Error::Policy)
            ));
        }
    }
    #[test]
    fn identity_is_owned_and_invalid_sources_have_static_categories() {
        let mut source = Zeroizing::new(b"alice".to_vec());
        let owned = map("cluster", true, "emit", |_| {
            Ok(Zeroizing::new(source.to_vec()))
        })
        .unwrap();
        source.clear();
        drop(source);
        assert_eq!(
            (owned.cluster(), owned.user(), owned.emit_discovered()),
            ("cluster", "alice", true)
        );
        assert!(matches!(
            map("cluster", true, "emit", |_| Err(())),
            Err(Error::Lookup)
        ));
        for bad in [
            vec![],
            vec![b'x'; 257],
            vec![0xff],
            b"-user".to_vec(),
            b"alice bob".to_vec(),
            b"alice|bob".to_vec(),
            b"alice\n".to_vec(),
        ] {
            let error = map("cluster", true, "emit", |_| Ok(Zeroizing::new(bad)))
                .err()
                .unwrap();
            assert_eq!(format!("{error:?}"), "Identity");
        }
        assert!(
            map(&"x".repeat(64), true, "suppress", |_| Ok(Zeroizing::new(
                vec![b'x'; 256]
            )))
            .is_ok()
        );
    }
    #[test]
    fn initial_emit_and_suppress_remain_deduplicated_across_reopen_and_policy_change() {
        use crate::{
            scheduler::{self, PollRequest},
            storage::Store,
        };
        use std::{path::Path, time::Duration};
        for initial in ["emit", "suppress"] {
            let directory = crate::storage::tests::Directory::new();
            let open = || Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
            let mut store = open();
            let mut observed = plan(initial);
            for scan in 0..2 {
                let request = PollRequest {
                    active_program: Path::new("squeue"),
                    accounting_program: Path::new("sacct"),
                    cluster: observed.cluster(),
                    user: observed.user(),
                    history_start: "2026-10-10T00:00:00",
                    history_end: "2026-10-11T00:00:00",
                    timeout: Duration::from_secs(1),
                    output_limit: 4096,
                    emit_discovered: observed.emit_discovered(),
                };
                let (mut reconciler, _) = store.restore(observed.cluster(), 10).unwrap();
                let events = scheduler::poll_durable_with(&request,&mut reconciler,&mut store,&["primary"],10,
                    |command: &mut std::process::Command,_,_| {
                        assert!(command.get_args().any(|argument| argument == "--user=alice"));
                        Ok(if command.get_program() == "squeue" { "" }
                            else { "cluster|1|alice|s|actual|COMPLETED|0:0\ncluster|2|other|s|actual|FAILED|1:0" }.into())
                    }).unwrap();
                assert_eq!(events.len(), usize::from(scan == 0 && initial == "emit"));
                let pending = store.due_deliveries("cluster", 10, 10).unwrap();
                assert_eq!(pending.len(), usize::from(initial == "emit"));
                assert!(pending.iter().all(|due| due.attempt_number == 1));
                let (restored, checkpoint) = store.restore("cluster", 10).unwrap();
                assert_eq!(restored.latest.len(), 1);
                assert_eq!(checkpoint.as_deref(), Some("2026-10-11T00:00:00"));
                drop(store);
                store = open();
                observed = plan(if initial == "emit" {
                    "suppress"
                } else {
                    "emit"
                });
            }
        }
    }
}

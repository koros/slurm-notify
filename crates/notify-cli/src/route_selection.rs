//! Private bounded event selection; no installed routing or public event contract.
use crate::lifecycle::State;
use std::collections::HashSet;
use zeroize::Zeroizing;
pub(super) struct Rule<'a> {
    pub(super) events: &'a [&'a str],
    pub(super) destinations: &'a [&'a str],
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Limit,
    Reference,
    Duplicate,
}
// No Debug/Clone; owned aliases preserve the compiled selection independently.
pub(super) struct Plan {
    aliases: Vec<Zeroizing<String>>,
    masks: [u64; 9],
}
fn event(name: &str) -> Result<usize, Error> {
    match name {
        "JOB_STARTED" => Ok(0),
        "JOB_COMPLETED" => Ok(1),
        "JOB_FAILED" => Ok(2),
        "JOB_TIMEOUT" => Ok(3),
        "JOB_CANCELLED" => Ok(4),
        "JOB_OUT_OF_MEMORY" => Ok(5),
        "JOB_NODE_FAILURE" => Ok(6),
        "JOB_BOOT_FAILURE" => Ok(7),
        "JOB_DEADLINE" => Ok(8),
        _ => Err(Error::Reference),
    }
}
impl Plan {
    pub(super) fn poll_with(
        &self,
        request: &crate::scheduler::PollRequest<'_>,
        reconciler: &mut crate::lifecycle::Reconciler,
        store: &mut crate::storage::Store,
        observed_at: i64,
        query: impl FnMut(
            &mut std::process::Command,
            std::time::Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
    ) -> Result<Vec<crate::lifecycle::Event>, crate::scheduler::DurablePollError> {
        let mut candidate = reconciler.clone();
        let events = crate::scheduler::poll_with(request, &mut candidate, query)
            .map_err(crate::scheduler::DurablePollError::Poll)?;
        store
            .persist_routed_reconciler(
                &candidate,
                &events,
                request.cluster,
                request.history_end,
                observed_at,
                |event| self.destinations(&event.state),
            )
            .map_err(crate::scheduler::DurablePollError::Storage)?;
        *reconciler = candidate;
        Ok(events)
    }
    pub(super) fn new(providers: &[&str], rules: &[Rule<'_>]) -> Result<Self, Error> {
        if providers.is_empty()
            || providers.len() > 64
            || rules.len() > 128
            || rules.iter().any(|rule| {
                rule.events.is_empty()
                    || rule.events.len() > 9
                    || rule.destinations.is_empty()
                    || rule.destinations.len() > 64
            })
        {
            return Err(Error::Limit);
        }
        let mut seen = HashSet::new();
        for name in providers {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return Err(Error::Reference);
            }
            if !seen.insert(name) {
                return Err(Error::Duplicate);
            }
        }
        let mut masks = [0u64; 9];
        for rule in rules {
            let mut selected = HashSet::new();
            for name in rule.events {
                let index = event(name)?;
                if !selected.insert(index) {
                    return Err(Error::Duplicate);
                }
            }
            let mut destinations = HashSet::new();
            let mut mask = 0;
            for name in rule.destinations {
                let index = providers
                    .iter()
                    .position(|provider| provider == name)
                    .ok_or(Error::Reference)?;
                if !destinations.insert(index) {
                    return Err(Error::Duplicate);
                }
                mask |= 1u64 << index;
            }
            for index in selected {
                masks[index] |= mask;
            }
        }
        Ok(Self {
            aliases: providers
                .iter()
                .map(|name| Zeroizing::new((*name).to_owned()))
                .collect(),
            masks,
        })
    }
    pub(super) fn destinations(&self, state: &State) -> Vec<&str> {
        let index = match state {
            State::Running => 0,
            State::Completed => 1,
            State::Failed => 2,
            State::Timeout => 3,
            State::Cancelled => 4,
            State::OutOfMemory => 5,
            State::NodeFailure => 6,
            State::BootFailure => 7,
            State::Deadline => 8,
            State::Pending | State::Unknown(_) => return vec![],
        };
        self.aliases
            .iter()
            .enumerate()
            .filter(|(i, _)| self.masks[index] & (1u64 << i) != 0)
            .map(|(_, name)| name.as_str())
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_rules_union_once_in_provider_order_and_own_aliases() {
        let mut source = String::from("first");
        let plan = Plan::new(
            &[&source, "second"],
            &[
                Rule {
                    events: &["JOB_FAILED"],
                    destinations: &["second", "first"],
                },
                Rule {
                    events: &["JOB_FAILED", "JOB_COMPLETED"],
                    destinations: &["second"],
                },
            ],
        )
        .unwrap();
        source.clear();
        drop(source);
        assert_eq!(plan.destinations(&State::Failed), vec!["first", "second"]);
        assert_eq!(plan.destinations(&State::Completed), vec!["second"]);
        assert!(plan.destinations(&State::Running).is_empty());
        assert!(Plan::new(&["first"], &[])
            .unwrap()
            .destinations(&State::Failed)
            .is_empty());
    }
    #[test]
    fn every_lifecycle_kind_selects_and_highest_destination_bit_is_preserved() {
        let names: Vec<_> = (0..64).map(|i| format!("provider{i}")).collect();
        let refs: Vec<_> = names.iter().map(String::as_str).collect();
        let events = [
            "JOB_STARTED",
            "JOB_COMPLETED",
            "JOB_FAILED",
            "JOB_TIMEOUT",
            "JOB_CANCELLED",
            "JOB_OUT_OF_MEMORY",
            "JOB_NODE_FAILURE",
            "JOB_BOOT_FAILURE",
            "JOB_DEADLINE",
        ];
        let plan = Plan::new(
            &refs,
            &[Rule {
                events: &events,
                destinations: &refs,
            }],
        )
        .unwrap();
        for state in [
            State::Running,
            State::Completed,
            State::Failed,
            State::Timeout,
            State::Cancelled,
            State::OutOfMemory,
            State::NodeFailure,
            State::BootFailure,
            State::Deadline,
        ] {
            assert_eq!(plan.destinations(&state), refs);
        }
        assert!(plan.destinations(&State::Pending).is_empty());
        assert!(plan
            .destinations(&State::Unknown("fixture".into()))
            .is_empty());
    }
    #[test]
    fn invalid_rules_fail_before_a_plan_is_published() {
        for providers in [vec![], vec!["x"; 65]] {
            assert!(matches!(Plan::new(&providers, &[]), Err(Error::Limit)));
        }
        for providers in [
            vec!["x", "x"],
            vec!["bad name"],
            vec![""],
            vec![&"x".repeat(65)],
        ] {
            assert!(Plan::new(&providers, &[]).is_err());
        }
        for (events, destinations, error) in [
            (vec![], vec!["x"], Error::Limit),
            (vec!["JOB_FAILED"; 10], vec!["x"], Error::Limit),
            (vec!["JOB_FAILED"], vec![], Error::Limit),
            (vec!["JOB_FAILED"], vec!["x"; 65], Error::Limit),
            (vec!["UNKNOWN"], vec!["x"], Error::Reference),
            (vec!["JOB_FAILED"], vec!["missing"], Error::Reference),
            (vec!["JOB_FAILED"; 2], vec!["x"], Error::Duplicate),
            (vec!["JOB_FAILED"], vec!["x"; 2], Error::Duplicate),
        ] {
            assert_eq!(
                Plan::new(
                    &["x"],
                    &[Rule {
                        events: &events,
                        destinations: &destinations
                    }]
                )
                .err()
                .unwrap(),
                error
            );
        }
        let rules: Vec<_> = (0..128)
            .map(|_| Rule {
                events: &["JOB_FAILED"],
                destinations: &["x"],
            })
            .collect();
        assert_eq!(
            Plan::new(&["x"], &rules)
                .unwrap()
                .destinations(&State::Failed),
            vec!["x"]
        );
        let mut rules = rules;
        rules.push(Rule {
            events: &["JOB_FAILED"],
            destinations: &["x"],
        });
        assert!(matches!(Plan::new(&["x"], &rules), Err(Error::Limit)));
    }
    #[test]
    fn event_routes_commit_atomically_and_do_not_retarget_pending_work_after_reopen() {
        use crate::{lifecycle::Reconciler, scheduler::PollRequest, storage::Store};
        use std::{path::Path, time::Duration};
        let directory = crate::storage::tests::Directory::new();
        let open = || Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut store = open();
        let plan = Plan::new(
            &["success", "failure"],
            &[
                Rule {
                    events: &["JOB_COMPLETED"],
                    destinations: &["success"],
                },
                Rule {
                    events: &["JOB_FAILED"],
                    destinations: &["failure"],
                },
            ],
        )
        .unwrap();
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
            Ok(if command.get_program()=="squeue" {""}
            else {"cluster|1|alice|s|actual|COMPLETED|0:0\ncluster|2|alice|s|actual|FAILED|1:0\ncluster|3|alice|s|actual|TIMEOUT|1:0"}.into())
        };
        let mut reconciler = Reconciler::with_capacity(10);
        let fault = rusqlite::Connection::open(directory.path()).unwrap();
        fault
            .execute_batch(
                "CREATE TRIGGER block_route BEFORE INSERT ON outbox
            WHEN NEW.destination='failure' BEGIN SELECT RAISE(ABORT,'fixture route block'); END;",
            )
            .unwrap();
        assert!(plan
            .poll_with(&request, &mut reconciler, &mut store, 10, query)
            .is_err());
        assert!(reconciler.latest.is_empty() && reconciler.emitted.is_empty());
        assert!(store.restore("cluster", 10).unwrap().1.is_none());
        assert!(store.due_deliveries("cluster", 10, 10).unwrap().is_empty());
        fault.execute_batch("DROP TRIGGER block_route;").unwrap();
        assert_eq!(
            plan.poll_with(&request, &mut reconciler, &mut store, 10, query)
                .unwrap()
                .len(),
            3
        );
        let pending = store.due_deliveries("cluster", 10, 10).unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending
            .iter()
            .any(|due| due.event.identity.job == "1" && due.destination == "success"));
        assert!(pending
            .iter()
            .any(|due| due.event.identity.job == "2" && due.destination == "failure"));
        let ids: Vec<_> = pending
            .iter()
            .map(|due| (due.event_id.clone(), due.destination.clone()))
            .collect();
        drop(store);
        store = open();
        let (mut reconciler, checkpoint) = store.restore("cluster", 10).unwrap();
        assert_eq!(reconciler.emitted.len(), 3);
        assert_eq!(checkpoint.as_deref(), Some(request.history_end));
        let changed = Plan::new(
            &["replacement"],
            &[Rule {
                events: &["JOB_COMPLETED", "JOB_FAILED", "JOB_TIMEOUT"],
                destinations: &["replacement"],
            }],
        )
        .unwrap();
        assert!(changed
            .poll_with(&request, &mut reconciler, &mut store, 11, query)
            .unwrap()
            .is_empty());
        let after = store.due_deliveries("cluster", 11, 10).unwrap();
        assert_eq!(
            after
                .iter()
                .map(|due| (due.event_id.clone(), due.destination.clone()))
                .collect::<Vec<_>>(),
            ids
        );
        assert!(after.iter().all(|due| due.attempt_number == 1));
    }
}

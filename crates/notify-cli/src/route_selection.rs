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
}

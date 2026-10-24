//! Test-only lifecycle prototype. No scheduler transport or public contract.

use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Identity {
    pub(super) cluster: String,
    pub(super) job: String,
    pub(super) submit: String,
    pub(super) start: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum State {
    Pending,
    Running,
    Completed,
    Failed,
    Timeout,
    Cancelled,
    OutOfMemory,
    NodeFailure,
    BootFailure,
    Deadline,
    Unknown(String),
}

impl State {
    pub(super) fn parse(value: &str) -> Self {
        match value {
            "PENDING" | "REQUEUED" => Self::Pending,
            "RUNNING" => Self::Running,
            "COMPLETED" => Self::Completed,
            "FAILED" => Self::Failed,
            "TIMEOUT" => Self::Timeout,
            "CANCELLED" => Self::Cancelled,
            v if v.starts_with("CANCELLED by ") => Self::Cancelled,
            "OUT_OF_MEMORY" => Self::OutOfMemory,
            "NODE_FAIL" => Self::NodeFailure,
            "BOOT_FAIL" => Self::BootFailure,
            "DEADLINE" => Self::Deadline,
            other => Self::Unknown(other.to_owned()),
        }
    }

    pub(super) fn code(&self) -> &str {
        match self {
            Self::Pending => "PENDING",
            Self::Running => "RUNNING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Timeout => "TIMEOUT",
            Self::Cancelled => "CANCELLED",
            Self::OutOfMemory => "OUT_OF_MEMORY",
            Self::NodeFailure => "NODE_FAIL",
            Self::BootFailure => "BOOT_FAIL",
            Self::Deadline => "DEADLINE",
            Self::Unknown(value) => value,
        }
    }

    pub(super) fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Failed
                | Self::Timeout
                | Self::Cancelled
                | Self::OutOfMemory
                | Self::NodeFailure
                | Self::BootFailure
                | Self::Deadline
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Record {
    identity: Identity,
    state: State,
    exit_code: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ParseError {
    line: usize,
    reason: &'static str,
}

// Synthetic accounting format: Cluster|JobID|User|Submit|Start|State|ExitCode.
// Reject an entire malformed snapshot instead of interpreting missing rows as exits.
pub(super) fn parse_accounting(input: &str, user: &str) -> Result<Vec<Record>, ParseError> {
    let mut records = Vec::new();
    for (index, line) in input.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split('|').map(str::trim).collect();
        let error = |reason| ParseError {
            line: index + 1,
            reason,
        };
        if fields.len() != 7 {
            return Err(error("expected seven accounting fields"));
        }
        if fields[2] != user {
            continue;
        }
        if fields[1].contains('.') {
            continue; // Job steps must not produce allocation lifecycle events.
        }
        let id_parts: Vec<_> = fields[1].split('_').collect();
        if !(1..=2).contains(&id_parts.len())
            || id_parts
                .iter()
                .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(error("unsupported or malformed allocation identity"));
        }
        if [fields[0], fields[3], fields[4], fields[5]]
            .iter()
            .any(|v| v.is_empty())
        {
            return Err(error("missing identity or state field"));
        }
        let exit: Vec<_> = fields[6].split(':').collect();
        if exit.len() != 2
            || exit
                .iter()
                .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(error("malformed exit code"));
        }
        let state = State::parse(fields[5]);
        // Pending Start can be an estimate. It is never an attempt identifier.
        let start = if state == State::Pending || (state == State::Cancelled && fields[4] == "N/A")
        {
            ""
        } else {
            fields[4]
        };
        if state != State::Pending && matches!(start, "N/A" | "Unknown" | "Unknown/None") {
            return Err(error("actual start context unavailable"));
        }
        if matches!(fields[3], "N/A" | "Unknown" | "Unknown/None") {
            return Err(error("submission context unavailable"));
        }
        records.push(Record {
            identity: Identity {
                cluster: fields[0].to_owned(),
                job: fields[1].to_owned(),
                submit: fields[3].to_owned(),
                start: start.to_owned(),
            },
            state,
            exit_code: fields[6].to_owned(),
        });
    }
    Ok(records)
}

// Active output lacks ExitCode. Keep that unknown rather than fabricating success.
pub(super) fn parse_active(
    input: &str,
    cluster: &str,
    user: &str,
) -> Result<Vec<Record>, ParseError> {
    if cluster.is_empty() || cluster.contains(['|', '\n', '\r']) {
        return Err(ParseError {
            line: 0,
            reason: "missing cluster identity",
        });
    }
    let mut normalized = String::new();
    for (index, line) in input.lines().enumerate() {
        if line.trim().is_empty() {
            normalized.push('\n');
            continue;
        }
        if line.split('|').count() != 5 {
            return Err(ParseError {
                line: index + 1,
                reason: "expected five active fields",
            });
        }
        normalized.push_str(&format!("{cluster}|{line}|0:0\n"));
    }
    let mut records = parse_accounting(&normalized, user)?;
    for record in &mut records {
        record.exit_code.clear();
    }
    Ok(records)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Event {
    pub(super) identity: Identity,
    pub(super) state: State,
}

#[derive(Clone, Default)]
pub(super) struct Reconciler {
    // Candidate state is persisted by the private storage pipeline; retention is future work.
    pub(super) latest: HashMap<Identity, State>,
    pub(super) emitted: HashSet<Event>,
    capacity: Option<(usize, usize)>,
}

impl Reconciler {
    #[cfg(test)]
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self::with_capacities(capacity, capacity)
    }
    pub(super) fn with_capacities(identities: usize, events: usize) -> Self {
        Self {
            capacity: Some((identities, events)),
            ..Self::default()
        }
    }

    // Parse both sources before changing reconciliation state. Accounting is
    // processed first so late active records cannot reverse a terminal outcome.
    pub(super) fn observe_snapshot(
        &mut self,
        active: &str,
        accounting: &str,
        cluster: &str,
        user: &str,
        emit_discovered: bool,
    ) -> Result<Vec<Event>, ParseError> {
        let mut records = parse_accounting(accounting, user)?;
        let active_records = parse_active(active, cluster, user)?;
        records.extend(active_records);
        // A start-less cancellation cannot be assigned to an already observed
        // running attempt. Fail atomically rather than inventing a second identity.
        for record in &records {
            if record.state == State::Cancelled && record.identity.start.is_empty() {
                let same_allocation = |other: &Identity| {
                    other.cluster == record.identity.cluster
                        && other.job == record.identity.job
                        && other.submit == record.identity.submit
                        && !other.start.is_empty()
                };
                if self.latest.keys().any(same_allocation)
                    || records.iter().any(|other| same_allocation(&other.identity))
                {
                    return Err(ParseError {
                        line: 0,
                        reason: "ambiguous cancellation attempt",
                    });
                }
            }
        }
        let mut candidate = self.clone();
        let events = candidate.observe(&records, emit_discovered);
        if let Some((identities, events)) = self.capacity {
            if candidate.latest.len() > identities || candidate.emitted.len() > events {
                return Err(ParseError {
                    line: 0,
                    reason: "reconciliation capacity exhausted",
                });
            }
        }
        *self = candidate;
        Ok(events)
    }

    // Caller explicitly chooses initial/catch-up emission; no product default here.
    fn observe(&mut self, records: &[Record], emit_discovered: bool) -> Vec<Event> {
        let mut events = Vec::new();
        for record in records {
            // Promote an observed pending allocation to its first actual attempt.
            // This carries initial-discovery policy across a changing Start field.
            let mut pending = record.identity.clone();
            pending.start.clear();
            if !record.identity.start.is_empty()
                && !self.latest.contains_key(&record.identity)
                && self.latest.get(&pending) == Some(&State::Pending)
            {
                if let Some(state) = self.latest.remove(&pending) {
                    self.latest.insert(record.identity.clone(), state);
                }
            }
            let prior = self.latest.get(&record.identity);
            // Once terminal, conflicting/stale observations require a future
            // explicit correction policy; never emit a second lifecycle outcome.
            if prior.map(State::terminal).unwrap_or(false) {
                continue;
            }
            let changed = prior != Some(&record.state);
            let eligible = record.state == State::Running || record.state.terminal();
            let discovered = prior.is_none();
            let event = Event {
                identity: record.identity.clone(),
                state: record.state.clone(),
            };
            if changed
                && eligible
                && (!discovered || emit_discovered)
                && self.emitted.insert(event.clone())
            {
                events.push(event);
            }
            self.latest
                .insert(record.identity.clone(), record.state.clone());
            // Suppressed initial events are still deduplicated on later observations.
            if discovered && !emit_discovered && eligible {
                self.emitted.insert(Event {
                    identity: record.identity.clone(),
                    state: record.state.clone(),
                });
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = include_str!("../../../tests/fixtures/slurm/accounting.psv");

    fn row(job: &str, start: &str, state: &str) -> Record {
        parse_accounting(
            &format!("cluster|{job}|alice|2026-10-10T10:00:00|{start}|{state}|0:0"),
            "alice",
        )
        .unwrap()
        .remove(0)
    }

    #[test]
    fn capacity_failure_is_atomic_and_preserves_deduplication() {
        let mut r = Reconciler::with_capacity(1);
        let first = "1|alice|s|actual|RUNNING";
        assert_eq!(
            r.observe_snapshot(first, "", "cluster", "alice", true)
                .unwrap()
                .len(),
            1
        );
        assert!(r
            .observe_snapshot("2|alice|s|actual|RUNNING", "", "cluster", "alice", true)
            .is_err());
        assert_eq!(r.latest.len(), 1);
        assert_eq!(r.emitted.len(), 1);
        assert!(r
            .observe_snapshot(first, "", "cluster", "alice", true)
            .unwrap()
            .is_empty());
        // Event ledger is independently limited, even for an existing job.
        assert!(r
            .observe_snapshot(
                "",
                "cluster|1|alice|s|actual|COMPLETED|0:0",
                "cluster",
                "alice",
                true
            )
            .is_err());
        assert_eq!(r.latest.values().next(), Some(&State::Running));
    }

    #[test]
    fn additional_terminal_states_emit_once_but_preemption_stays_unresolved() {
        for state in ["NODE_FAIL", "BOOT_FAIL", "DEADLINE"] {
            let mut r = Reconciler::default();
            let record = row("1", "actual", state);
            assert!(record.state.terminal());
            assert_eq!(r.observe(&[record.clone()], true).len(), 1);
            assert!(r.observe(&[record], true).is_empty());
        }
        assert!(Reconciler::default()
            .observe(&[row("1", "actual", "PREEMPTED")], true)
            .is_empty());
    }

    #[test]
    fn pending_cancelled_without_start_emits_once() {
        let mut r = Reconciler::default();
        r.observe_snapshot("1|alice|s|estimate|PENDING", "", "cluster", "alice", false)
            .unwrap();
        let cancelled = "cluster|1|alice|s|N/A|CANCELLED by 123|0:0";
        let events = r
            .observe_snapshot("", cancelled, "cluster", "alice", false)
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state, State::Cancelled);
        assert!(events[0].identity.start.is_empty());
        assert!(r
            .observe_snapshot("", cancelled, "cluster", "alice", true)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn discovered_prestart_cancellation_obeys_explicit_catch_up_policy() {
        let cancelled = "cluster|1|alice|s|N/A|CANCELLED|0:0";
        assert_eq!(
            Reconciler::default()
                .observe_snapshot("", cancelled, "cluster", "alice", true)
                .unwrap()
                .len(),
            1
        );
        let mut r = Reconciler::default();
        assert!(r
            .observe_snapshot("", cancelled, "cluster", "alice", false)
            .unwrap()
            .is_empty());
        assert!(r
            .observe_snapshot("", cancelled, "cluster", "alice", true)
            .unwrap()
            .is_empty());
        assert!(parse_accounting("cluster|1|alice|s|Unknown|CANCELLED|0:0", "alice").is_err());
    }

    #[test]
    fn ambiguous_startless_cancellation_does_not_mutate_running_attempt() {
        let mut r = Reconciler::default();
        let active = "1|alice|s|actual|RUNNING";
        r.observe_snapshot(active, "", "cluster", "alice", true)
            .unwrap();
        let cancelled = "cluster|1|alice|s|N/A|CANCELLED|0:0";
        assert!(r
            .observe_snapshot("", cancelled, "cluster", "alice", true)
            .is_err());
        assert_eq!(r.latest.len(), 1);
        assert_eq!(r.emitted.len(), 1);
        assert!(r
            .observe_snapshot(active, "", "cluster", "alice", true)
            .unwrap()
            .is_empty());
        assert!(Reconciler::default()
            .observe_snapshot(active, cancelled, "cluster", "alice", true)
            .is_err());
    }

    #[test]
    fn snapshot_failure_is_atomic_and_accounting_terminal_wins() {
        let mut r = Reconciler::default();
        let accounting = "cluster|1|alice|s|actual|COMPLETED|0:0";
        assert!(r
            .observe_snapshot("malformed", accounting, "cluster", "alice", true)
            .is_err());
        assert!(r.latest.is_empty());
        assert!(r.emitted.is_empty());
        let events = r
            .observe_snapshot(
                "1|alice|s|actual|RUNNING",
                accounting,
                "cluster",
                "alice",
                true,
            )
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state, State::Completed);
        assert!(r
            .observe_snapshot(
                "1|alice|s|actual|RUNNING",
                accounting,
                "cluster",
                "alice",
                true
            )
            .unwrap()
            .is_empty());
    }

    #[test]
    fn pending_estimates_promote_to_one_actual_attempt() {
        let mut r = Reconciler::default();
        assert!(r
            .observe(&[row("1", "estimate1", "PENDING")], false)
            .is_empty());
        assert!(r
            .observe(&[row("1", "estimate2", "PENDING")], false)
            .is_empty());
        assert_eq!(r.latest.len(), 1);
        let events = r.observe(&[row("1", "actual", "RUNNING")], false);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].identity.start, "actual");
        assert_eq!(r.latest.len(), 1);
    }

    #[test]
    fn active_then_accounting_deduplicates_and_resolves_delayed_completion() {
        let mut r = Reconciler::default();
        let active = parse_active(
            "1|alice|2026-10-10T10:00:00|actual|RUNNING",
            "cluster",
            "alice",
        )
        .unwrap();
        assert_eq!(r.observe(&active, true).len(), 1);
        assert!(r.observe(&[row("1", "actual", "RUNNING")], true).is_empty());
        assert!(r.observe(&[], true).is_empty());
        assert_eq!(r.observe(&[row("1", "actual", "COMPLETED")], true).len(), 1);
        assert!(r.observe(&active, true).is_empty());
    }

    #[test]
    fn reused_job_ids_with_different_submissions_remain_distinct() {
        let mut first = row("1", "actual", "COMPLETED");
        let mut second = first.clone();
        first.identity.submit = "submission1".into();
        second.identity.submit = "submission2".into();
        assert_eq!(
            Reconciler::default().observe(&[first, second], true).len(),
            2
        );
    }

    #[test]
    fn ambiguous_actual_identity_is_rejected_without_state_changes() {
        let mut r = Reconciler::default();
        r.observe(&[row("1", "actual", "RUNNING")], true);
        assert!(parse_accounting("cluster|1|alice|s|Unknown|COMPLETED|0:0", "alice").is_err());
        assert!(parse_accounting("cluster|1|alice|Unknown|actual|RUNNING|0:0", "alice").is_err());
        assert_eq!(r.latest.len(), 1);
    }

    #[test]
    fn active_records_filter_users_and_keep_exit_unknown() {
        let records = parse_active(
            include_str!("../../../tests/fixtures/slurm/active.psv"),
            "cluster",
            "alice",
        )
        .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].identity.job, "101_1");
        assert!(records.iter().all(|r| r.exit_code.is_empty()));
        assert!(parse_active("bad", "cluster", "alice").is_err());
        assert!(parse_active("", "", "alice").is_err());
    }

    #[test]
    fn fixture_filters_users_and_steps_and_separates_array_tasks() {
        let records = parse_accounting(FIXTURE, "alice").unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].identity.job, "100");
        assert_eq!(records[1].identity.job, "101_1");
        assert_eq!(records[2].identity.job, "101_2");
        assert_eq!(records[2].exit_code, "1:0");
    }

    #[test]
    fn malformed_snapshot_reports_line_without_echoing_input() {
        let error = parse_accounting("\nsecret|bad", "alice").unwrap_err();
        assert_eq!(
            error,
            ParseError {
                line: 2,
                reason: "expected seven accounting fields"
            }
        );
        for id in ["101_[1-5]", "101+1", "_1", "abc"] {
            assert!(parse_accounting(&format!("c|{id}|alice|s|t|RUNNING|0:0"), "alice").is_err());
        }
        assert!(parse_accounting("c|1|alice|s|t|RUNNING|bad", "alice").is_err());
    }

    #[test]
    fn state_mapping_preserves_unknown_and_truncated_states() {
        for (raw, expected) in [
            ("COMPLETED", State::Completed),
            ("FAILED", State::Failed),
            ("TIMEOUT", State::Timeout),
            ("CANCELLED by 123", State::Cancelled),
            ("OUT_OF_MEMORY", State::OutOfMemory),
        ] {
            assert_eq!(State::parse(raw), expected);
        }
        assert_eq!(
            State::parse("OUT_OF_ME+"),
            State::Unknown("OUT_OF_ME+".into())
        );
        assert!(!State::parse("PREEMPTED").terminal());
    }

    #[test]
    fn pending_running_completed_and_duplicate_observations() {
        let mut r = Reconciler::default();
        assert!(r.observe(&[row("1", "t", "PENDING")], true).is_empty());
        assert_eq!(r.observe(&[row("1", "t", "RUNNING")], true).len(), 1);
        assert!(r.observe(&[row("1", "t", "RUNNING")], true).is_empty());
        assert_eq!(r.observe(&[row("1", "t", "COMPLETED")], true).len(), 1);
        assert!(r.observe(&[row("1", "t", "RUNNING")], true).is_empty());
        assert!(r.observe(&[row("1", "t", "COMPLETED")], true).is_empty());
    }

    #[test]
    fn absent_and_unknown_records_never_invent_failures() {
        let mut r = Reconciler::default();
        r.observe(&[row("1", "t", "RUNNING")], true);
        assert!(r.observe(&[], true).is_empty());
        assert!(r.observe(&[row("1", "t", "FUTURE_STATE")], true).is_empty());
        assert_eq!(r.observe(&[row("1", "t", "FAILED")], true).len(), 1);
    }

    #[test]
    fn short_jobs_have_explicit_catch_up_policy() {
        let record = row("1", "t", "COMPLETED");
        assert_eq!(
            Reconciler::default().observe(&[record.clone()], true).len(),
            1
        );
        let mut r = Reconciler::default();
        assert!(r.observe(&[record.clone()], false).is_empty());
        assert!(r.observe(&[record], true).is_empty());
        assert!(r.observe(&[row("1", "t", "RUNNING")], true).is_empty());
        assert!(r.observe(&[row("1", "t", "FAILED")], true).is_empty());
    }

    #[test]
    fn clusters_array_tasks_and_new_start_attempts_are_distinct() {
        let mut records = vec![
            row("1_1", "t1", "RUNNING"),
            row("1_2", "t1", "RUNNING"),
            row("1_1", "t2", "RUNNING"),
        ];
        let mut other = records[0].clone();
        other.identity.cluster = "other".into();
        records.push(other);
        let mut r = Reconciler::default();
        assert_eq!(r.observe(&records, true).len(), 4);
        assert!(r.observe(&records, true).is_empty());
    }

    #[test]
    fn requeue_waits_for_distinct_attempt_context() {
        let mut r = Reconciler::default();
        assert_eq!(r.observe(&[row("1", "t1", "RUNNING")], true).len(), 1);
        assert!(r.observe(&[row("1", "t1", "REQUEUED")], true).is_empty());
        assert_eq!(r.observe(&[row("1", "t2", "RUNNING")], true).len(), 1);
    }
}

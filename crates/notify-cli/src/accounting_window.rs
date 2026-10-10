//! Private UTC accounting-window planner; limits are caller-supplied.
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

#[derive(Clone, Copy)]
pub(super) struct Policy {
    pub(super) lookback_seconds: i64,
    pub(super) overlap_seconds: i64,
}

impl Policy {
    pub(super) fn valid(&self) -> bool {
        self.lookback_seconds > 0
            && self.overlap_seconds > 0
            && self.overlap_seconds < self.lookback_seconds
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    InvalidPolicy,
    InvalidTime,
    ClockRegressed,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Window {
    pub(super) start: String,
    pub(super) end: String,
    // True when downtime extends before the bounded query's start, not proof
    // that jobs were lost. Clipped overlap is reported separately.
    pub(super) catch_up_gap: bool,
    pub(super) overlap_clipped: bool,
}

fn timestamp(seconds: i64) -> Result<String, Error> {
    if seconds < 0 {
        return Err(Error::InvalidTime);
    }
    let date = OffsetDateTime::from_unix_timestamp(seconds).map_err(|_| Error::InvalidTime)?;
    if !(1970..=9999).contains(&date.year()) {
        return Err(Error::InvalidTime);
    }
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        date.year(),
        u8::from(date.month()),
        date.day(),
        date.hour(),
        date.minute(),
        date.second()
    ))
}

pub(super) fn parse_timestamp(value: &str) -> Result<i64, Error> {
    if !super::scheduler::valid_timestamp(value) {
        return Err(Error::InvalidTime);
    }
    let number = |start: usize, end: usize| {
        value[start..end]
            .parse::<u32>()
            .map_err(|_| Error::InvalidTime)
    };
    let date = Date::from_calendar_date(
        number(0, 4)? as i32,
        Month::try_from(number(5, 7)? as u8).map_err(|_| Error::InvalidTime)?,
        number(8, 10)? as u8,
    )
    .map_err(|_| Error::InvalidTime)?;
    let time = Time::from_hms(
        number(11, 13)? as u8,
        number(14, 16)? as u8,
        number(17, 19)? as u8,
    )
    .map_err(|_| Error::InvalidTime)?;
    let seconds = PrimitiveDateTime::new(date, time)
        .assume_utc()
        .unix_timestamp();
    if seconds < 0 {
        return Err(Error::InvalidTime);
    }
    Ok(seconds)
}

pub(super) fn plan(now: i64, checkpoint: Option<&str>, policy: Policy) -> Result<Window, Error> {
    if !policy.valid() {
        return Err(Error::InvalidPolicy);
    }
    let end = timestamp(now)?;
    if now == 0 {
        return Err(Error::InvalidTime);
    }
    let floor = now.saturating_sub(policy.lookback_seconds).max(0);
    let (start, catch_up_gap, overlap_clipped) = match checkpoint {
        None => (floor, false, false),
        Some(value) => {
            let through = parse_timestamp(value)?;
            if through > now {
                return Err(Error::ClockRegressed);
            }
            let desired = through.saturating_sub(policy.overlap_seconds).max(0);
            (desired.max(floor), through < floor, desired < floor)
        }
    };
    Ok(Window {
        start: timestamp(start)?,
        end,
        catch_up_gap,
        overlap_clipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            lookback_seconds: 3600,
            overlap_seconds: 300,
        }
    }

    #[test]
    fn initial_overlap_and_bounded_downtime_windows_are_explicit() {
        let initial = plan(7200, None, policy()).unwrap();
        assert_eq!(initial.start, "1970-01-01T01:00:00");
        let resumed = plan(7200, Some("1970-01-01T01:50:00"), policy()).unwrap();
        assert_eq!(resumed.start, "1970-01-01T01:45:00");
        assert!(!resumed.catch_up_gap && !resumed.overlap_clipped);
        let clipped = plan(7200, Some("1970-01-01T01:02:00"), policy()).unwrap();
        assert!(clipped.overlap_clipped && !clipped.catch_up_gap);
        let gap = plan(7200, Some("1970-01-01T00:30:00"), policy()).unwrap();
        assert!(gap.catch_up_gap && gap.overlap_clipped);
        assert_eq!(gap.start, initial.start);
    }

    #[test]
    fn invalid_times_policies_and_regressed_clock_fail_visibly() {
        for policy in [
            Policy {
                lookback_seconds: 0,
                ..policy()
            },
            Policy {
                overlap_seconds: 0,
                ..policy()
            },
            Policy {
                overlap_seconds: 3600,
                ..policy()
            },
        ] {
            assert_eq!(plan(7200, None, policy), Err(Error::InvalidPolicy));
        }
        for now in [-1, 0, i64::MAX] {
            assert_eq!(plan(now, None, policy()), Err(Error::InvalidTime));
        }
        for checkpoint in ["bad", "2026-02-29T00:00:00", "1969-12-31T23:59:59"] {
            assert_eq!(
                plan(7200, Some(checkpoint), policy()),
                Err(Error::InvalidTime)
            );
        }
        assert_eq!(
            plan(7200, Some("1970-01-01T03:00:00"), policy()),
            Err(Error::ClockRegressed)
        );
    }

    #[test]
    fn utc_calendar_arithmetic_crosses_leap_day_and_epoch_safely() {
        let now = parse_timestamp("2024-03-01T00:02:00").unwrap();
        let window = plan(now, Some("2024-03-01T00:00:00"), policy()).unwrap();
        assert_eq!(window.start, "2024-02-29T23:55:00");
        let early = plan(60, Some("1970-01-01T00:00:30"), policy()).unwrap();
        assert_eq!(early.start, "1970-01-01T00:00:00");
        assert!(!early.catch_up_gap);
        assert_eq!(
            timestamp(parse_timestamp("9999-12-31T23:59:59").unwrap()).unwrap(),
            "9999-12-31T23:59:59"
        );
    }

    #[test]
    fn delayed_accounting_is_recovered_from_overlap_after_reopen() {
        use crate::{
            lifecycle::Reconciler,
            scheduler::{poll_durable_with, PollRequest, QueryError},
            storage::{tests::Directory, Store},
        };
        use std::time::Duration;
        let directory = Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut reconciler = Reconciler::with_capacity(10);
        let first_time = parse_timestamp("2026-10-10T01:00:00").unwrap();
        let first = plan(first_time, None, policy()).unwrap();
        fn request(window: &Window) -> PollRequest<'_> {
            PollRequest {
                active_program: std::path::Path::new("/fixture/squeue"),
                accounting_program: std::path::Path::new("/fixture/sacct"),
                cluster: "cluster",
                user: "alice",
                history_start: &window.start,
                history_end: &window.end,
                timeout: Duration::from_secs(1),
                output_limit: 4096,
                emit_discovered: true,
            }
        }
        poll_durable_with(
            &request(&first),
            &mut reconciler,
            &mut store,
            &["primary"],
            first_time,
            |_, _, _| Ok(String::new()),
        )
        .unwrap();
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let (mut reconciler, checkpoint) = store.restore("cluster", 10).unwrap();
        let second_time = first_time + 60;
        let second = plan(second_time, checkpoint.as_deref(), policy()).unwrap();
        assert_eq!(second.start, "2026-10-10T00:55:00");
        // Failed query must preserve the checkpoint so the same overlap is retried.
        assert!(poll_durable_with(
            &request(&second),
            &mut reconciler,
            &mut store,
            &["primary"],
            second_time,
            |_, _, _| Err(QueryError::Timeout)
        )
        .is_err());
        assert_eq!(store.restore("cluster", 10).unwrap().1, checkpoint);
        let fixture = "cluster|1|alice|2026-10-10T00:56:00|2026-10-10T00:57:00|COMPLETED|0:0";
        let query = |command: &mut std::process::Command, _, _| {
            if command.get_program() == "/fixture/squeue" {
                Ok(String::new())
            } else {
                assert!(command
                    .get_args()
                    .any(|arg| arg == "--starttime=2026-10-10T00:55:00"));
                Ok(fixture.into())
            }
        };
        assert_eq!(
            poll_durable_with(
                &request(&second),
                &mut reconciler,
                &mut store,
                &["primary"],
                second_time,
                query
            )
            .unwrap()
            .len(),
            1
        );
        assert!(poll_durable_with(
            &request(&second),
            &mut reconciler,
            &mut store,
            &["primary"],
            second_time,
            query
        )
        .unwrap()
        .is_empty());
        assert_eq!(
            store
                .due_deliveries("cluster", second_time, 10)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some(second.end.as_str())
        );
    }
}

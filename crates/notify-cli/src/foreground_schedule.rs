//! Private finite/continuous fixed-delay scheduling with monotonic time and waits.
use std::time::Duration;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Policy,
    Clock,
    ClockRegressed,
    DeadlineOverflow,
    Wait,
    NoProgress,
    WakeLimit,
    Tick,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Exit {
    Exhausted { ticks: usize },
    Cancelled { ticks: usize },
}
pub(super) enum Wake {
    Ready,
    Cancelled,
}
pub(super) struct Hooks<T, M, W, X> {
    pub(super) tick: T,
    pub(super) monotonic: M,
    pub(super) wait: W,
    pub(super) cancelled: X,
}
#[cfg(test)]
pub(super) fn run<T, M, W, X>(
    interval: Duration,
    limit: usize,
    hooks: Hooks<T, M, W, X>,
) -> Result<Exit, Error>
where
    T: FnMut() -> Result<bool, ()>,
    M: FnMut() -> Option<Duration>,
    W: FnMut(Duration) -> Result<Wake, ()>,
    X: FnMut() -> bool,
{
    if !(1..=1024).contains(&limit) {
        return Err(Error::Policy);
    }
    run_inner(interval, Some(limit), hooks)
}

pub(super) fn run_continuous<T, M, W, X>(
    interval: Duration,
    hooks: Hooks<T, M, W, X>,
) -> Result<Exit, Error>
where
    T: FnMut() -> Result<bool, ()>,
    M: FnMut() -> Option<Duration>,
    W: FnMut(Duration) -> Result<Wake, ()>,
    X: FnMut() -> bool,
{
    run_inner(interval, None, hooks)
}

fn run_inner<T, M, W, X>(
    interval: Duration,
    limit: Option<usize>,
    hooks: Hooks<T, M, W, X>,
) -> Result<Exit, Error>
where
    T: FnMut() -> Result<bool, ()>,
    M: FnMut() -> Option<Duration>,
    W: FnMut(Duration) -> Result<Wake, ()>,
    X: FnMut() -> bool,
{
    if interval < Duration::from_secs(1) || interval > Duration::from_secs(86400) {
        return Err(Error::Policy);
    }
    let Hooks {
        mut tick,
        mut monotonic,
        mut wait,
        mut cancelled,
    } = hooks;
    let mut previous = None;
    let mut clock = || {
        let now = monotonic().ok_or(Error::Clock)?;
        if previous.is_some_and(|last| now < last) {
            return Err(Error::ClockRegressed);
        }
        previous = Some(now);
        Ok(now)
    };
    let mut ticks = 0usize;
    loop {
        if cancelled() {
            return Ok(Exit::Cancelled { ticks });
        }
        clock()?;
        let stopped = tick().map_err(|_| Error::Tick)?;
        ticks = ticks.saturating_add(1);
        if stopped {
            return Ok(Exit::Cancelled { ticks });
        }
        if limit == Some(ticks) {
            return Ok(Exit::Exhausted { ticks });
        }
        if cancelled() {
            return Ok(Exit::Cancelled { ticks });
        }
        // Completion-relative scheduling avoids bursts after slow or blocked ticks.
        let mut now = clock()?;
        let deadline = now.checked_add(interval).ok_or(Error::DeadlineOverflow)?;
        let mut wakes = 0;
        while now < deadline {
            if cancelled() {
                return Ok(Exit::Cancelled { ticks });
            }
            if wakes == 1024 {
                return Err(Error::WakeLimit);
            }
            wakes += 1;
            match wait(deadline - now).map_err(|_| Error::Wait)? {
                Wake::Cancelled => return Ok(Exit::Cancelled { ticks }),
                Wake::Ready => {}
            }
            if cancelled() {
                return Ok(Exit::Cancelled { ticks });
            }
            let after = clock()?;
            if after == now {
                return Err(Error::NoProgress);
            }
            now = after;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn continuous_run_keeps_completion_delay_past_finite_budget_and_cancels() {
        let now = Cell::new(Duration::ZERO);
        let ticks = Cell::new(0);
        let waits = Cell::new(0);
        let exit = run_continuous(
            Duration::from_secs(2),
            Hooks {
                tick: || {
                    assert_eq!(now.get(), Duration::from_secs(ticks.get() * 5));
                    ticks.set(ticks.get() + 1);
                    now.set(now.get() + Duration::from_secs(3));
                    Ok(ticks.get() == 1050)
                },
                monotonic: || Some(now.get()),
                wait: |delay| {
                    assert_eq!(delay, Duration::from_secs(2));
                    waits.set(waits.get() + 1);
                    now.set(now.get() + delay);
                    Ok(Wake::Ready)
                },
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(exit, Exit::Cancelled { ticks: 1050 });
        assert_eq!(waits.get(), 1049);
    }

    #[test]
    fn continuous_run_retains_clock_history_and_bounds_partial_wakes() {
        let now = Cell::new(Duration::ZERO);
        let ticks = Cell::new(0);
        assert_eq!(
            run_continuous(
                Duration::from_secs(1),
                Hooks {
                    tick: || {
                        ticks.set(ticks.get() + 1);
                        if ticks.get() == 1025 {
                            now.set(Duration::ZERO);
                        }
                        Ok(false)
                    },
                    monotonic: || Some(now.get()),
                    wait: |delay| {
                        now.set(now.get() + delay);
                        Ok(Wake::Ready)
                    },
                    cancelled: || false,
                }
            ),
            Err(Error::ClockRegressed)
        );
        let now = Cell::new(Duration::ZERO);
        assert_eq!(
            run_continuous(
                Duration::from_secs(1),
                Hooks {
                    tick: || Ok(false),
                    monotonic: || Some(now.get()),
                    wait: |_| {
                        now.set(now.get() + Duration::from_nanos(1));
                        Ok(Wake::Ready)
                    },
                    cancelled: || false,
                }
            ),
            Err(Error::WakeLimit)
        );
    }

    #[test]
    fn delayed_ticks_partial_wakes_and_oversleep_use_completion_relative_deadlines() {
        let now = Cell::new(Duration::ZERO);
        let mut ticks = Vec::new();
        let mut waits = Vec::new();
        let mut wakes = 0;
        let result = run(
            Duration::from_secs(10),
            3,
            Hooks {
                tick: || {
                    ticks.push(now.get().as_secs());
                    now.set(now.get() + Duration::from_secs(25));
                    Ok(false)
                },
                monotonic: || Some(now.get()),
                wait: |remaining: Duration| {
                    waits.push(remaining.as_secs());
                    wakes += 1;
                    now.set(
                        now.get()
                            + Duration::from_secs(if wakes == 1 {
                                4
                            } else if wakes == 2 {
                                20
                            } else {
                                10
                            }),
                    );
                    Ok(Wake::Ready)
                },
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(result, Exit::Exhausted { ticks: 3 });
        assert_eq!(ticks, [0, 49, 84]);
        assert_eq!(waits, [10, 6, 10]);
    }
    #[test]
    fn tiny_progress_wakes_cannot_spin_past_the_wake_budget() {
        let now = Cell::new(Duration::ZERO);
        let mut waits = 0;
        assert_eq!(
            run(
                Duration::from_secs(1),
                2,
                Hooks {
                    tick: || Ok(false),
                    monotonic: || Some(now.get()),
                    wait: |_| {
                        waits += 1;
                        now.set(now.get() + Duration::from_nanos(1));
                        Ok(Wake::Ready)
                    },
                    cancelled: || false,
                }
            ),
            Err(Error::WakeLimit)
        );
        assert_eq!(waits, 1024);
    }
    #[test]
    fn invalid_policy_and_precancelled_run_never_call_io() {
        for (interval, limit) in [(0, 1), (86401, 1), (1, 0), (1, 1025)] {
            assert_eq!(
                run(
                    Duration::from_secs(interval),
                    limit,
                    Hooks {
                        tick: || panic!("tick"),
                        monotonic: || panic!("clock"),
                        wait: |_| panic!("wait"),
                        cancelled: || panic!("cancel"),
                    }
                ),
                Err(Error::Policy)
            );
        }
        assert_eq!(
            run(
                Duration::from_secs(1),
                1,
                Hooks {
                    tick: || panic!("tick"),
                    monotonic: || panic!("clock"),
                    wait: |_| panic!("wait"),
                    cancelled: || true,
                }
            ),
            Ok(Exit::Cancelled { ticks: 0 })
        );
    }
    #[test]
    fn cancellation_and_clock_wait_tick_failures_are_bounded_and_static() {
        for case in 0..9 {
            let samples = Cell::new(0);
            let expected = match case {
                0 => Ok(Exit::Cancelled { ticks: 1 }),
                1 => Ok(Exit::Cancelled { ticks: 1 }),
                2 => Err(Error::Clock),
                3 => Err(Error::ClockRegressed),
                4 => Err(Error::DeadlineOverflow),
                5 => Err(Error::Wait),
                6 => Err(Error::NoProgress),
                7 => Err(Error::Tick),
                _ => Ok(Exit::Cancelled { ticks: 1 }),
            };
            let cancelled = Cell::new(false);
            assert_eq!(
                run(
                    Duration::from_secs(1),
                    2,
                    Hooks {
                        tick: || if case == 7 {
                            Err(())
                        } else {
                            if case == 8 {
                                cancelled.set(true);
                            }
                            Ok(case == 0)
                        },
                        monotonic: || {
                            samples.set(samples.get() + 1);
                            match case {
                                2 => None,
                                3 if samples.get() == 2 => Some(Duration::ZERO),
                                4 => Some(Duration::MAX),
                                _ => Some(Duration::from_secs(1)),
                            }
                        },
                        wait: |_| match case {
                            1 => Ok(Wake::Cancelled),
                            5 => Err(()),
                            _ => Ok(Wake::Ready),
                        },
                        cancelled: || cancelled.get(),
                    }
                ),
                expected
            );
        }
    }
}

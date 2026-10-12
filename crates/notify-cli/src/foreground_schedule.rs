//! Private finite, fixed-delay scheduling with injected monotonic time and waits.
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
    if !(1..=1024).contains(&limit)
        || interval < Duration::from_secs(1)
        || interval > Duration::from_secs(86400)
    {
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
    for ticks in 0..limit {
        if cancelled() {
            return Ok(Exit::Cancelled { ticks });
        }
        clock()?;
        if tick().map_err(|_| Error::Tick)? {
            return Ok(Exit::Cancelled { ticks: ticks + 1 });
        }
        if ticks + 1 == limit {
            return Ok(Exit::Exhausted { ticks: limit });
        }
        if cancelled() {
            return Ok(Exit::Cancelled { ticks: ticks + 1 });
        }
        // Completion-relative scheduling avoids bursts after slow or blocked ticks.
        let mut now = clock()?;
        let deadline = now.checked_add(interval).ok_or(Error::DeadlineOverflow)?;
        let mut wakes = 0;
        while now < deadline {
            if cancelled() {
                return Ok(Exit::Cancelled { ticks: ticks + 1 });
            }
            if wakes == 1024 {
                return Err(Error::WakeLimit);
            }
            wakes += 1;
            match wait(deadline - now).map_err(|_| Error::Wait)? {
                Wake::Cancelled => return Ok(Exit::Cancelled { ticks: ticks + 1 }),
                Wake::Ready => {}
            }
            if cancelled() {
                return Ok(Exit::Cancelled { ticks: ticks + 1 });
            }
            let after = clock()?;
            if after == now {
                return Err(Error::NoProgress);
            }
            now = after;
        }
    }
    unreachable!("validated nonzero budget")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
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

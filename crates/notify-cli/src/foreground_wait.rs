//! Private caller-owned sticky cancellation and bounded condition-variable wait.
use crate::foreground_schedule::Wake;
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Duration,
    Poisoned,
}
#[derive(Clone)]
pub(super) struct Signal {
    shared: Arc<(Mutex<bool>, Condvar)>,
}
impl Signal {
    pub(super) fn new() -> Self {
        Self {
            shared: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }
    pub(super) fn cancel(&self) -> Result<(), Error> {
        let (state, wake) = &*self.shared;
        let mut cancelled = state.lock().map_err(|_| Error::Poisoned)?;
        *cancelled = true;
        wake.notify_all();
        Ok(())
    }
    pub(super) fn cancelled(&self) -> Result<bool, Error> {
        self.shared
            .0
            .lock()
            .map(|value| *value)
            .map_err(|_| Error::Poisoned)
    }
    pub(super) fn wait(&self, duration: Duration) -> Result<Wake, Error> {
        if duration.is_zero() || duration > Duration::from_secs(86400) {
            return Err(Error::Duration);
        }
        let (state, wake) = &*self.shared;
        let cancelled = state.lock().map_err(|_| Error::Poisoned)?;
        if *cancelled {
            return Ok(Wake::Cancelled);
        }
        // Checking and registering the wait share the cancel mutex: no lost wake.
        let (cancelled, _) = wake
            .wait_timeout(cancelled, duration)
            .map_err(|_| Error::Poisoned)?;
        Ok(if *cancelled {
            Wake::Cancelled
        } else {
            Wake::Ready
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread, time::Instant};
    #[test]
    fn duration_validation_and_sticky_cancel_are_shared_by_clones() {
        let signal = Signal::new();
        assert!(!signal.cancelled().unwrap());
        for duration in [Duration::ZERO, Duration::from_secs(86401), Duration::MAX] {
            assert!(matches!(signal.wait(duration), Err(Error::Duration)));
        }
        let clone = signal.clone();
        clone.cancel().unwrap();
        signal.cancel().unwrap();
        assert!(clone.cancelled().unwrap());
        assert!(matches!(
            signal.wait(Duration::from_secs(86400)),
            Ok(Wake::Cancelled)
        ));
    }
    #[test]
    fn uncancelled_wait_times_out_without_setting_cancellation() {
        let signal = Signal::new();
        let start = Instant::now();
        // Handle permitted spurious wakeups through the same remaining-time policy.
        let interval = Duration::from_millis(2);
        for _ in 0..1024 {
            let elapsed = start.elapsed();
            if elapsed >= interval {
                break;
            }
            assert!(matches!(signal.wait(interval - elapsed), Ok(Wake::Ready)));
        }
        assert!(start.elapsed() >= interval);
        assert!(!signal.cancelled().unwrap());
    }
    #[test]
    fn cancellation_wakes_all_waiters_without_resetting_the_signal() {
        let signal = Signal::new();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..3 {
            let waiter = signal.clone();
            let ready = ready_tx.clone();
            let done = done_tx.clone();
            workers.push(thread::spawn(move || {
                ready.send(()).unwrap();
                done.send(matches!(
                    waiter.wait(Duration::from_secs(2)),
                    Ok(Wake::Cancelled)
                ))
                .unwrap();
            }));
        }
        for _ in 0..3 {
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        signal.cancel().unwrap();
        for _ in 0..3 {
            assert!(done_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(matches!(
            signal.wait(Duration::from_millis(1)),
            Ok(Wake::Cancelled)
        ));
    }
    #[test]
    fn poisoned_state_fails_statically() {
        let signal = Signal::new();
        let shared = signal.shared.clone();
        assert!(thread::spawn(move || {
            let _held = shared.0.lock().unwrap();
            panic!("fixture");
        })
        .join()
        .is_err());
        assert_eq!(signal.cancelled(), Err(Error::Poisoned));
        assert_eq!(signal.cancel(), Err(Error::Poisoned));
        assert!(matches!(
            signal.wait(Duration::from_millis(1)),
            Err(Error::Poisoned)
        ));
    }
}

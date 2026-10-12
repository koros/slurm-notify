//! Private process-wide SIGINT/SIGTERM bridge; Linux/macOS only.
use crate::foreground_wait::Signal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{thread, time::Duration};

static OWNED: AtomicBool = AtomicBool::new(false);
static REQUESTED: AtomicBool = AtomicBool::new(false);
extern "C" fn request_cancel(_: libc::c_int) {
    // Only a lock-free atomic store in signal context: no locks, allocation or I/O.
    REQUESTED.store(true, Ordering::Relaxed);
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Owned,
    Install,
    Worker,
}

pub(super) struct Binding {
    previous: [libc::sigaction; 2],
    stop: std::sync::Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

pub(super) fn bind(signal: &Signal) -> Result<Binding, Error> {
    OWNED
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .map_err(|_| Error::Owned)?;
    REQUESTED.store(false, Ordering::Relaxed);
    // SAFETY: zero initializes valid sigaction storage; sigemptyset initializes
    // the mask. Handler has C ABI and only uses a lock-free atomic store.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = request_cancel as usize;
    action.sa_flags = libc::SA_RESTART;
    unsafe { libc::sigemptyset(&mut action.sa_mask) };
    let mut previous: [libc::sigaction; 2] = unsafe { std::mem::zeroed() };
    for (index, number) in [libc::SIGINT, libc::SIGTERM].into_iter().enumerate() {
        // SAFETY: action and prior-action output are live initialized storage.
        if unsafe { libc::sigaction(number, &action, &mut previous[index]) } != 0 {
            let mut restored = true;
            for (number, prior) in [libc::SIGINT, libc::SIGTERM]
                .iter()
                .zip(&previous)
                .take(index)
            {
                restored &= unsafe { libc::sigaction(*number, prior, std::ptr::null_mut()) } == 0;
            }
            if restored {
                OWNED.store(false, Ordering::Release);
            }
            return Err(Error::Install);
        }
    }
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let signal = signal.clone();
    let worker = thread::Builder::new()
        .name("private-signal-bridge".into())
        .spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                if REQUESTED.load(Ordering::Relaxed) {
                    let _ = signal.cancel();
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
    match worker {
        Ok(worker) => Ok(Binding {
            previous,
            stop,
            worker: Some(worker),
        }),
        Err(_) => {
            if restore(&previous) {
                OWNED.store(false, Ordering::Release);
            }
            Err(Error::Worker)
        }
    }
}
fn restore(previous: &[libc::sigaction; 2]) -> bool {
    let mut restored = true;
    for (number, prior) in [libc::SIGINT, libc::SIGTERM].iter().zip(previous) {
        // SAFETY: saved actions came from successful sigaction calls.
        restored &= unsafe { libc::sigaction(*number, prior, std::ptr::null_mut()) } == 0;
    }
    restored
}
impl Drop for Binding {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let restored = restore(&self.previous);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if restored {
            OWNED.store(false, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static RESTORED: AtomicBool = AtomicBool::new(false);
    extern "C" fn prior_handler(_: libc::c_int) {
        RESTORED.store(true, Ordering::Relaxed);
    }
    #[test]
    fn isolated_signals_cancel_wait_restore_handlers_and_allow_rebind() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "foreground_signals::tests::signal_child",
            ])
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "signal child failed");
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("signal child timed out");
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    #[ignore = "trusted signal child; never changes parent process handlers"]
    fn signal_child() {
        let mut prior: libc::sigaction = unsafe { std::mem::zeroed() };
        prior.sa_sigaction = prior_handler as usize;
        unsafe { libc::sigemptyset(&mut prior.sa_mask) };
        for number in [libc::SIGINT, libc::SIGTERM] {
            assert_eq!(
                unsafe { libc::sigaction(number, &prior, std::ptr::null_mut()) },
                0
            );
        }
        for number in [libc::SIGINT, libc::SIGTERM] {
            let signal = Signal::new();
            let binding = bind(&signal).unwrap();
            assert!(matches!(bind(&Signal::new()), Err(Error::Owned)));
            assert!(!signal.cancelled().unwrap());
            let sender = thread::spawn(move || {
                thread::sleep(Duration::from_millis(10));
                assert_eq!(unsafe { libc::kill(libc::getpid(), number) }, 0);
                assert_eq!(unsafe { libc::kill(libc::getpid(), number) }, 0);
            });
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            loop {
                let remaining = deadline
                    .checked_duration_since(std::time::Instant::now())
                    .expect("signal deadline");
                if matches!(
                    signal.wait(remaining).unwrap(),
                    crate::foreground_schedule::Wake::Cancelled
                ) {
                    break;
                }
            }
            sender.join().unwrap();
            assert!(signal.cancelled().unwrap());
            drop(binding);
            RESTORED.store(false, Ordering::Relaxed);
            assert_eq!(unsafe { libc::raise(number) }, 0);
            assert!(RESTORED.load(Ordering::Relaxed));
        }
        let fresh = Signal::new();
        drop(bind(&fresh).unwrap());
        assert!(!fresh.cancelled().unwrap());
    }
}

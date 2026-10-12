//! Trusted private foreground child fixtures; no installed flags or output protocol.
use crate::{
    foreground_schedule::{Exit, Hooks, Wake},
    foreground_session::Session,
};
use std::{
    cell::Cell,
    io::{Read, Write},
    path::Path,
    process::Command,
    time::Duration,
};
use zeroize::Zeroizing;

pub(super) fn child() -> bool {
    let requested = std::env::args().nth(1);
    let report_output = requested
        .as_deref()
        .is_some_and(|value| value.starts_with("--report-"));
    let mode = match requested.as_deref() {
        Some("--report-cancelled") => Some("--foreground-real-wait".to_owned()),
        Some("--report-exhausted") => Some("--foreground-replay".to_owned()),
        Some("--report-failed") => Some("--foreground-wait-error".to_owned()),
        _ => requested,
    };
    if !matches!(
        mode.as_deref(),
        Some(
            "--foreground-wait"
                | "--foreground-wait-error"
                | "--foreground-replay"
                | "--foreground-cancel-before"
                | "--foreground-real-wait"
                | "--foreground-real-replay"
                | "--foreground-os-int"
                | "--foreground-os-term"
        )
    ) {
        return false;
    }
    let Some(path) = std::env::args().nth(2) else {
        println!("FOREGROUND INVALID");
        return true;
    };
    if matches!(
        mode.as_deref(),
        Some("--foreground-os-int" | "--foreground-os-term")
    ) {
        signal_lifecycle(Path::new(&path));
        return true;
    }
    let started = Session::start(
        Path::new(&path),
        2,
        Duration::from_millis(50),
        |_| Ok(Zeroizing::new(b"alice".to_vec())),
        |name, _| {
            assert!(!name.contains("SMTP"));
            Ok(Zeroizing::new(if name.ends_with("_URL") {
                b"https://fixture.invalid/hook".to_vec()
            } else {
                b"Bearer fixture".to_vec()
            }))
        },
        || mode.as_deref() == Some("--foreground-cancel-before"),
    );
    let mut session = match started {
        Ok(session) => session,
        Err(crate::foreground_session::Error::Cancelled) => {
            println!("FOREGROUND CANCELLED");
            return true;
        }
        Err(_) => {
            println!("FOREGROUND INVALID");
            return true;
        }
    };
    let result = if matches!(
        mode.as_deref(),
        Some("--foreground-real-wait" | "--foreground-real-replay")
    ) {
        let signal = crate::foreground_wait::Signal::new();
        let mut control = None;
        let mut ticks = 0;
        let start = std::time::Instant::now();
        let result = session.run_interruptible(2, &signal, |session| {
            ticks += 1;
            let report = blocked_tick(session)?;
            if mode.as_deref() == Some("--foreground-real-wait") {
                assert_eq!(ticks, 1, "unexpected tick before cancellation");
                println!("READY");
                std::io::stdout().flush().unwrap();
                let cancelled = signal.clone();
                control = Some(std::thread::spawn(move || {
                    let _ = std::io::stdin().read_exact(&mut [0]);
                    cancelled.cancel().unwrap();
                }));
            }
            Ok(report)
        });
        if let Some(control) = control {
            control.join().unwrap();
        }
        if mode.as_deref() == Some("--foreground-real-replay") {
            assert_eq!(ticks, 2);
            assert!(start.elapsed() >= Duration::from_secs(2));
        }
        result
    } else {
        let now = Cell::new(Duration::ZERO);
        session.run_with(
            2,
            Hooks {
                tick: blocked_tick,
                monotonic: || Some(now.get()),
                wait: |remaining| {
                    if mode.as_deref() == Some("--foreground-replay") {
                        now.set(now.get() + remaining);
                        return Ok(Wake::Ready);
                    }
                    println!("READY");
                    std::io::stdout().flush().unwrap();
                    let _ = std::io::stdin().read_exact(&mut [0]);
                    if mode.as_deref() == Some("--foreground-wait-error") {
                        Err(())
                    } else {
                        Ok(Wake::Cancelled)
                    }
                },
                cancelled: || false,
            },
        )
    };
    assert!(matches!(
        session.status(7200),
        Err(crate::foreground_session::Error::Closed)
    ));
    if report_output {
        print!(
            "{}",
            crate::foreground_report::Report::classify(&result).render()
        );
        return true;
    }
    println!(
        "{}",
        match result {
            Ok(Exit::Cancelled { ticks: 1 }) => "FOREGROUND CANCELLED",
            Ok(Exit::Exhausted { ticks: 2 }) => "FOREGROUND EXHAUSTED",
            Err(crate::foreground_schedule::Error::Wait) => "FOREGROUND FAILED",
            _ => "FOREGROUND INVALID",
        }
    );
    true
}
fn signal_lifecycle(path: &Path) {
    let signal = crate::foreground_wait::Signal::new();
    let binding = crate::foreground_signals::bind(&signal).unwrap();
    let mut ticks = 0;
    let summary = crate::foreground_runtime::run(
        crate::foreground_runtime::Request {
            path,
            poll_limit: 2,
            tick_limit: 2,
            lock_timeout: Duration::from_millis(50),
        },
        &signal,
        crate::foreground_runtime::Hooks {
            identity: |_| Ok(Zeroizing::new(b"alice".to_vec())),
            lookup: |name: &str, _| {
                assert!(!name.contains("SMTP"));
                Ok(Zeroizing::new(if name.ends_with("_URL") {
                    b"https://fixture.invalid/hook".to_vec()
                } else {
                    b"Bearer fixture".to_vec()
                }))
            },
            tick: |session: &mut Session, _: &crate::foreground_wait::Signal| {
                ticks += 1;
                assert_eq!(ticks, 1, "unexpected signal lifecycle tick");
                let report = blocked_tick(session)?;
                println!("READY");
                std::io::stdout().flush().unwrap();
                Ok(report)
            },
        },
    );
    assert!(signal.cancelled().unwrap());
    drop(binding);
    // Verify bridge ownership/worker cleanup before emitting the bounded report.
    let fresh = crate::foreground_wait::Signal::new();
    drop(crate::foreground_signals::bind(&fresh).unwrap());
    assert!(!fresh.cancelled().unwrap());
    print!("{}", summary.render());
}

fn blocked_tick(
    session: &mut Session,
) -> Result<crate::polling_cycle::TickReport, crate::foreground_session::Error> {
    let report = session.tick_with(
        (Path::new("squeue"), Path::new("sacct")),
        crate::polling_cycle::Hooks {
            query: |command: &mut Command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    ""
                } else {
                    "research-cluster|1|alice|s|actual|COMPLETED|0:0"
                }
                .into())
            },
            send: (
                |_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> {
                    panic!("blocked credential access")
                },
                |_: &crate::storage::DueDelivery| -> Command { panic!("blocked helper access") },
                |_: &crate::storage::DueDelivery| -> Result<
                    crate::delivery::ProviderOutcome,
                    crate::supervised_provider::Diagnostic,
                > { panic!("unexpected ordinary send") },
            ),
            clock: || Some(7200),
            cancelled: || false,
        },
    )?;
    assert!(matches!(
        report.delivery,
        Some(Err(crate::polling_cycle::StageError::DispatchBlocked))
    ));
    Ok(report)
}
pub(super) fn one_shot(mode: &str, path: &Path, expected: &[u8]) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([std::ffi::OsStr::new(mode), path.as_os_str()]);
    let output = crate::transport::supervise_with(
        &mut command,
        Vec::new(),
        Duration::from_secs(5),
        0,
        64,
        |bytes| {
            if matches!(
                bytes,
                b"FOREGROUND CANCELLED\n"
                    | b"FOREGROUND EXHAUSTED\n"
                    | b"FOREGROUND FAILED\n"
                    | b"FOREGROUND INVALID\n"
            ) {
                Ok(bytes.to_vec())
            } else {
                Err(crate::transport::Error::InvalidInput)
            }
        },
    )
    .unwrap();
    assert_eq!(output, expected);
}
fn decode_report(bytes: &[u8]) -> Result<Vec<u8>, crate::transport::Error> {
    if bytes.len() <= crate::foreground_report::MAX_BYTES
        && matches!(
            bytes,
            b"foreground outcome=cancelled ticks=1\n"
                | b"foreground outcome=exhausted ticks=2\n"
                | b"foreground outcome=failed_wait ticks=unknown\n"
        )
    {
        Ok(bytes.to_vec())
    } else {
        Err(crate::transport::Error::InvalidInput)
    }
}
fn reported_exhaustion(path: &Path) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([std::ffi::OsStr::new("--report-exhausted"), path.as_os_str()]);
    let output = crate::transport::supervise_with(
        &mut command,
        Vec::new(),
        Duration::from_secs(2),
        0,
        crate::foreground_report::MAX_BYTES,
        decode_report,
    )
    .unwrap();
    assert_eq!(output, b"foreground outcome=exhausted ticks=2\n");
    assert!(decode_report(b"foreground outcome=failed_wait ticks=private-path\n").is_err());
    assert!(decode_report(&[b'x'; 97]).is_err());
}
fn pending(state: &Path) -> (String, i64, Vec<u8>, String, Option<String>) {
    let mut store = crate::storage::Store::open_private(state, Duration::from_millis(50)).unwrap();
    let due = store.due_deliveries("research-cluster", 7200, 10).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].destination, "research_email");
    let id = due[0].event_id.clone();
    let frozen = store
        .frozen_smtp(&id, "research_email")
        .unwrap()
        .unwrap()
        .data()
        .to_vec();
    let bound = store
        .bound_smtp_provider(&id, "research_email")
        .unwrap()
        .unwrap()
        .token()
        .to_owned();
    (
        id,
        due[0].attempt_number,
        frozen,
        bound,
        store
            .health_snapshot("research-cluster", 7200, 86400, (10, 10), 10)
            .unwrap()
            .1,
    )
}
pub(super) fn acceptance() {
    use std::os::unix::fs::OpenOptionsExt;
    let directory = crate::storage::tests::Directory::new();
    let root = directory.path().parent().unwrap().canonicalize().unwrap();
    let state = root.join("foreground-state");
    let config = root.join("foreground.json");
    one_shot(
        "--foreground-cancel-before",
        &config,
        b"FOREGROUND CANCELLED\n",
    );
    assert!(!state.exists());
    assert!(!config.exists());
    let trust = root.join("root.der");
    let certificate =
        rcgen::generate_simple_self_signed(vec!["smtp.example.invalid".into()]).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&trust)
        .unwrap()
        .write_all(certificate.cert.der())
        .unwrap();
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../docs/proposals/examples/local-mixed-config.json"
    ))
    .unwrap();
    source["storage"]["path"] = serde_json::json!(state.join("state.sqlite3"));
    source["providers"]["research_email"]["trust_der_files"] = serde_json::json!([trust]);
    source["monitor"]["initial_observation"] = serde_json::json!("emit");
    source["routes"] =
        serde_json::json!([{"events":["JOB_COMPLETED"],"send_to":["research_email"]}]);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&config)
        .unwrap()
        .write_all(&serde_json::to_vec(&source).unwrap())
        .unwrap();
    let mut previous = None;
    for (mode, kill, expected) in [
        (
            "--foreground-wait",
            false,
            b"FOREGROUND CANCELLED\n".as_slice(),
        ),
        ("--foreground-wait", true, b"".as_slice()),
        (
            "--report-cancelled",
            false,
            b"foreground outcome=cancelled ticks=1\n".as_slice(),
        ),
        (
            "--report-failed",
            false,
            b"foreground outcome=failed_wait ticks=unknown\n".as_slice(),
        ),
        (
            "--foreground-real-wait",
            false,
            b"FOREGROUND CANCELLED\n".as_slice(),
        ),
        ("--foreground-real-wait", true, b"".as_slice()),
        (
            "--foreground-os-int",
            false,
            b"foreground outcome=cancelled ticks=1\n".as_slice(),
        ),
        (
            "--foreground-os-term",
            false,
            b"foreground outcome=cancelled ticks=1\n".as_slice(),
        ),
        (
            "--foreground-wait-error",
            false,
            b"FOREGROUND FAILED\n".as_slice(),
        ),
    ] {
        let mut running = crate::status_boundary::Running::start(mode, &config);
        assert_eq!(
            crate::status_boundary::inspect("--status-inspect", &config),
            crate::status_boundary::Ack::Owned
        );
        let before = crate::status_boundary::files(&state);
        if matches!(mode, "--foreground-os-int" | "--foreground-os-term") {
            running.finish_signal(
                if mode == "--foreground-os-int" {
                    libc::SIGINT
                } else {
                    libc::SIGTERM
                },
                expected,
            );
        } else {
            running.finish(kill, expected);
        }
        assert_eq!(crate::status_boundary::files(&state), before);
        let durable = pending(&state);
        assert_eq!(durable.1, 1);
        assert_eq!(durable.4.as_deref(), Some("1970-01-01T02:00:00"));
        if let Some(previous) = &previous {
            assert_eq!(&durable, previous);
        }
        one_shot("--foreground-replay", &config, b"FOREGROUND EXHAUSTED\n");
        reported_exhaustion(&config);
        assert_eq!(pending(&state), durable);
        previous = Some(durable);
        let report = crate::status_composition::prepare_path(&config)
            .unwrap()
            .inspect(
                &crate::polling_cycle::Cycles::new(1).unwrap(),
                7200,
                Duration::from_millis(50),
            )
            .unwrap();
        assert!(!report.poll.stale);
    }
    // Real timeout/exhaustion uses a separate validated two-second fixture interval.
    source["monitor"]["poll_seconds"] = serde_json::json!(2);
    source["monitor"]["query_timeout_seconds"] = serde_json::json!(1);
    let next = config.with_extension("next");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&next)
        .unwrap()
        .write_all(&serde_json::to_vec(&source).unwrap())
        .unwrap();
    std::fs::rename(next, &config).unwrap();
    one_shot(
        "--foreground-real-replay",
        &config,
        b"FOREGROUND EXHAUSTED\n",
    );
    assert_eq!(pending(&state), previous.unwrap());
    // The fixture output decoder accepts no arbitrary diagnostic text.
    one_shot(
        "--foreground-replay",
        &root.join("missing.json"),
        b"FOREGROUND INVALID\n",
    );
}

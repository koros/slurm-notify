//! Trusted active-tick cancellation fixtures; no installed protocol or real send.
use crate::{
    foreground_session::Session,
    foreground_wait::Signal,
    polling_cycle::{Hooks, Outcome, StageError},
    storage::{DueDelivery, Store},
};
use std::{
    io::{Read, Write},
    path::Path,
    process::Command,
    time::Duration,
};
use zeroize::Zeroizing;
fn cancel_from_parent(signal: &Signal, os_signal: bool) {
    println!("READY");
    std::io::stdout().flush().unwrap();
    if os_signal {
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .expect("signal callback deadline");
            if matches!(
                signal.wait(remaining).unwrap(),
                crate::foreground_schedule::Wake::Cancelled
            ) {
                break;
            }
        }
    } else {
        let _ = std::io::stdin().read_exact(&mut [0]);
        signal.cancel().unwrap();
    }
}
pub(super) fn child() -> bool {
    let requested = std::env::args().nth(1);
    let os_signal = requested.as_deref().is_some_and(|value| {
        matches!(
            value,
            "--signal-query-int"
                | "--signal-query-term"
                | "--signal-send-int"
                | "--signal-send-term"
        )
    });
    let mode = requested.map(|value| {
        if os_signal {
            if value.contains("query") {
                "--active-query".to_owned()
            } else {
                "--active-send".to_owned()
            }
        } else {
            value.replace("--lifecycle-", "--active-")
        }
    });
    if !matches!(
        mode.as_deref(),
        Some("--active-query" | "--active-send" | "--active-query-replay" | "--active-send-replay")
    ) {
        return false;
    }
    let Some(path) = std::env::args().nth(2) else {
        println!("FOREGROUND INVALID");
        return true;
    };
    let signal = Signal::new();
    let binding = if os_signal {
        Some(crate::foreground_signals::bind(&signal).unwrap())
    } else {
        None
    };
    let send_case = mode.as_deref().is_some_and(|value| value.contains("send"));
    let replay = mode
        .as_deref()
        .is_some_and(|value| value.ends_with("replay"));
    let mut sends = 0;
    let summary = crate::foreground_runtime::run(
        crate::foreground_runtime::Request {
            path: Path::new(&path),
            poll_limit: 1,
            tick_limit: 1,
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
            tick: |session: &mut Session, signal: &Signal| {
                let report = session.tick_interruptible((Path::new("squeue"), Path::new("sacct")), Hooks {
            query: |command: &mut Command, _, _| {
                if command.get_program() == "squeue" { return Ok(String::new()); }
                if !send_case && !replay { cancel_from_parent(signal, os_signal); }
                Ok(if send_case {
                    "research-cluster|1|alice|s|actual|FAILED|1:0\nresearch-cluster|2|alice|s|actual|FAILED|1:0"
                } else { "research-cluster|1|alice|s|actual|COMPLETED|0:0" }.into())
            },
            send: (|_: &str, _| -> Result<Zeroizing<Vec<u8>>, ()> { panic!("unexpected credentials") },
                |_: &DueDelivery| -> Command { panic!("unexpected helper") },
                |due: &DueDelivery| {
                    assert!(send_case);
                    assert_eq!(due.destination, "primary");
                    sends += 1;
                    assert_eq!(sends, 1);
                    if !replay { cancel_from_parent(signal, os_signal); }
                    Ok(crate::delivery::ProviderOutcome::Delivered)
                }),
            clock: || Some(7200), cancelled: signal,
        })?;
                assert!(
                    matches!(report.poll, Some(Ok(Outcome::Committed { events, .. })) if events == if replay { 0 } else if send_case { 2 } else { 1 })
                );
                if !replay {
                    assert!(report.cancelled && report.cleanup.is_none());
                    if send_case {
                        assert!(matches!(report.delivery, Some(Err(StageError::Cancelled))));
                        assert_eq!(
                            report
                                .delivery_failure
                                .as_ref()
                                .unwrap()
                                .committed
                                .delivered,
                            1
                        );
                    } else {
                        assert!(report.delivery.is_none());
                    }
                } else if send_case {
                    assert_eq!(
                        report
                            .delivery
                            .as_ref()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .delivered,
                        1
                    );
                } else {
                    assert!(matches!(
                        report.delivery,
                        Some(Err(StageError::DispatchBlocked))
                    ));
                }
                Ok(report)
            },
        },
    );
    if let Some(binding) = binding {
        drop(binding);
        let fresh = Signal::new();
        drop(crate::foreground_signals::bind(&fresh).unwrap());
        assert!(!fresh.cancelled().unwrap());
    }
    print!("{}", summary.render());
    true
}
fn replace(path: &Path, bytes: &[u8]) {
    use std::os::unix::fs::OpenOptionsExt;
    let next = path.with_extension("next");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&next)
        .unwrap()
        .write_all(bytes)
        .unwrap();
    std::fs::rename(next, path).unwrap();
}
fn reported_replay(mode: &str, path: &Path, expected: &[u8]) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([std::ffi::OsStr::new(mode), path.as_os_str()]);
    let bytes = crate::transport::supervise_with(
        &mut command,
        Vec::new(),
        Duration::from_secs(2),
        0,
        crate::foreground_report::MAX_BYTES,
        |bytes| {
            if matches!(
                bytes,
                b"foreground startup=failed_preparation\n"
                    | b"foreground outcome=exhausted ticks=1\n"
            ) {
                Ok(bytes.to_vec())
            } else {
                Err(crate::transport::Error::InvalidInput)
            }
        },
    )
    .unwrap();
    assert_eq!(bytes, expected);
}
pub(super) fn acceptance() {
    use std::os::unix::fs::OpenOptionsExt;
    let directory = crate::storage::tests::Directory::new();
    let root = directory.path().parent().unwrap().canonicalize().unwrap();
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
    for (send_case, number) in [
        (false, None),
        (true, None),
        (false, Some(libc::SIGINT)),
        (false, Some(libc::SIGTERM)),
        (true, Some(libc::SIGINT)),
        (true, Some(libc::SIGTERM)),
    ] {
        let name = if send_case { "send" } else { "query" };
        let suffix = match number {
            Some(libc::SIGINT) => "int",
            Some(libc::SIGTERM) => "term",
            _ => "injected",
        };
        let state = root.join(format!("{name}-{suffix}"));
        let config = root.join(format!("{name}-{suffix}.json"));
        let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../docs/proposals/examples/local-mixed-config.json"
        ))
        .unwrap();
        source["storage"]["path"] = serde_json::json!(state.join("state.sqlite3"));
        source["providers"]["research_email"]["trust_der_files"] = serde_json::json!([trust]);
        source["monitor"]["initial_observation"] = serde_json::json!("emit");
        source["routes"] = if send_case {
            serde_json::json!([{"events":["JOB_FAILED"],"send_to":["primary"]}])
        } else {
            serde_json::json!([{"events":["JOB_COMPLETED"],"send_to":["research_email"]}])
        };
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&config)
            .unwrap()
            .write_all(&serde_json::to_vec(&source).unwrap())
            .unwrap();
        let mode = if number.is_some() {
            format!("--signal-{name}-{suffix}")
        } else {
            format!("--lifecycle-{name}")
        };
        let mut running = crate::status_boundary::Running::start(&mode, &config);
        assert_eq!(
            crate::status_boundary::inspect("--status-inspect", &config),
            crate::status_boundary::Ack::Owned
        );
        // Replace the protected source while the prepared callback is paused.
        let source_bytes = serde_json::to_vec(&source).unwrap();
        replace(&config, b"{invalid replacement fixture");
        if let Some(number) = number {
            running.finish_signal(number, b"foreground outcome=cancelled ticks=1\n");
        } else {
            running.finish(false, b"foreground outcome=cancelled ticks=1\n");
        }
        let mut store = Store::open_private(&state, Duration::from_millis(50)).unwrap();
        let due = store.due_deliveries("research-cluster", 7200, 10).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].attempt_number, 1);
        let frozen = if send_case {
            None
        } else {
            Some((
                store
                    .frozen_smtp(&due[0].event_id, "research_email")
                    .unwrap()
                    .unwrap()
                    .data()
                    .to_vec(),
                store
                    .bound_smtp_provider(&due[0].event_id, "research_email")
                    .unwrap()
                    .unwrap()
                    .token()
                    .to_owned(),
            ))
        };
        let checkpoint = store
            .health_snapshot("research-cluster", 7200, 86400, (10, 10), 10)
            .unwrap()
            .1;
        drop(store);
        let before_failed_restart = crate::status_boundary::files(&state);
        reported_replay(
            if send_case {
                "--lifecycle-send-replay"
            } else {
                "--lifecycle-query-replay"
            },
            &config,
            b"foreground startup=failed_preparation\n",
        );
        assert_eq!(crate::status_boundary::files(&state), before_failed_restart);
        replace(&config, &source_bytes);
        reported_replay(
            if send_case {
                "--lifecycle-send-replay"
            } else {
                "--lifecycle-query-replay"
            },
            &config,
            b"foreground outcome=exhausted ticks=1\n",
        );
        let mut store = Store::open_private(&state, Duration::from_millis(50)).unwrap();
        let after = store.due_deliveries("research-cluster", 7200, 10).unwrap();
        assert_eq!(
            store
                .health_snapshot("research-cluster", 7200, 86400, (10, 10), 10)
                .unwrap()
                .1,
            checkpoint
        );
        if let Some((bytes, provider)) = frozen {
            assert_eq!(after.len(), 1);
            assert_eq!(after[0].event_id, due[0].event_id);
            assert_eq!(after[0].attempt_number, 1);
            assert_eq!(
                store
                    .frozen_smtp(&after[0].event_id, "research_email")
                    .unwrap()
                    .unwrap()
                    .data(),
                bytes
            );
            assert_eq!(
                store
                    .bound_smtp_provider(&after[0].event_id, "research_email")
                    .unwrap()
                    .unwrap()
                    .token(),
                provider
            );
        } else {
            assert!(after.is_empty());
        }
        drop(store);
        assert!(
            !crate::status_composition::prepare_path(&config)
                .unwrap()
                .inspect(
                    &crate::polling_cycle::Cycles::new(1).unwrap(),
                    7200,
                    Duration::from_millis(50)
                )
                .unwrap()
                .poll
                .stale
        );
    }
}

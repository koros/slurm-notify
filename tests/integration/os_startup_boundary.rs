//! Trusted OS resource composition fixtures; no installed command or provider send.
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub(super) fn child() -> bool {
    let mode = std::env::args().nth(1);
    if !matches!(
        mode.as_deref(),
        Some("--os-runtime" | "--os-runtime-replay")
    ) {
        return false;
    }
    let Some(path) = std::env::args().nth(2) else {
        return true;
    };
    let replay = mode.as_deref() == Some("--os-runtime-replay");
    let signal = crate::foreground_wait::Signal::new();
    let summary = crate::foreground_runtime::run(
        crate::foreground_runtime::Request {
            path: Path::new(&path),
            poll_limit: 1,
            tick_limit: 1,
            lock_timeout: Duration::from_millis(50),
        },
        &signal,
        crate::foreground_runtime::Hooks {
            identity: crate::effective_user::lookup,
            lookup: crate::environment_resolution::lookup,
            tick: |session: &mut crate::foreground_session::Session,
                   signal: &crate::foreground_wait::Signal| {
                let report = session.tick_interruptible(
                    (Path::new("squeue"), Path::new("sacct")),
                    crate::polling_cycle::Hooks {
                        query: |command: &mut Command, _, _| {
                            let user = crate::effective_user::lookup(256).unwrap();
                            let user = std::str::from_utf8(&user).unwrap();
                            assert!(command
                                .get_args()
                                .any(|arg| arg == std::ffi::OsStr::new(&format!("--user={user}"))));
                            Ok(if command.get_program() == "squeue" {
                                String::new()
                            } else {
                                format!("research-cluster|1|{user}|s|actual|COMPLETED|0:0")
                            })
                        },
                        send: (
                            crate::environment_resolution::lookup,
                            |_: &crate::storage::DueDelivery| -> Command {
                                panic!("unexpected SMTP helper")
                            },
                            |_: &crate::storage::DueDelivery| -> Result<
                                crate::delivery::ProviderOutcome,
                                crate::supervised_provider::Diagnostic,
                            > { panic!("unexpected ordinary send") },
                        ),
                        clock: || Some(7200),
                        cancelled: signal,
                    },
                )?;
                assert!(
                    matches!(report.poll, Some(Ok(crate::polling_cycle::Outcome::Committed { events, .. })) if events == usize::from(!replay))
                );
                assert!(matches!(
                    report.delivery,
                    Some(Err(crate::polling_cycle::StageError::DispatchBlocked))
                ));
                Ok(report)
            },
        },
    );
    print!("{}", summary.render());
    true
}
struct OwnedChild {
    child: Child,
    reader: Option<thread::JoinHandle<()>>,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
fn run(path: &Path, replay: bool, url: Option<OsString>, auth: Option<OsString>, expected: &[u8]) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg(if replay {
            "--os-runtime-replay"
        } else {
            "--os-runtime"
        })
        .arg(path)
        .env_clear()
        .env("USER", "-invalid-fixture")
        .env("LOGNAME", "-invalid-fixture")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(url) = url {
        command.env("SLURM_NOTIFY_WEBHOOK_URL", url);
    }
    if let Some(auth) = auth {
        command.env("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION", auth);
    }
    let mut child = command.spawn().unwrap();
    let mut output = child.stdout.take().unwrap().take(97);
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        if output.read_to_end(&mut bytes).is_ok() {
            let _ = sender.send(bytes);
        }
    });
    let mut owned = OwnedChild {
        child,
        reader: Some(reader),
    };
    // NSS is synchronous inside the child; the parent enforces the fixture deadline.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = owned.child.try_wait().unwrap() {
            assert!(status.success(), "OS runtime child failed");
            break;
        }
        assert!(Instant::now() < deadline, "OS runtime child timed out");
        thread::sleep(Duration::from_millis(2));
    }
    let bytes = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(bytes.len() <= crate::foreground_report::MAX_BYTES);
    assert_eq!(bytes, expected);
    owned.reader.take().unwrap().join().unwrap();
}
fn valid(path: &Path, replay: bool) {
    run(
        path,
        replay,
        Some("https://fixture.invalid/private-fixture".into()),
        Some("Bearer fixture".into()),
        b"foreground outcome=exhausted ticks=1\n",
    );
}
fn failures(path: &Path, replay: bool) {
    let good = || Some(OsString::from("https://fixture.invalid/private-fixture"));
    let auth = || Some(OsString::from("Bearer fixture"));
    for value in [
        None,
        Some(OsString::new()),
        Some(OsString::from("x".repeat(4097))),
        Some(OsString::from("http://fixture.invalid")),
    ] {
        run(
            path,
            replay,
            value,
            auth(),
            b"foreground startup=failed_preparation\n",
        );
    }
    for value in [
        None,
        Some(OsString::new()),
        Some(OsString::from("x".repeat(4097))),
        Some(OsString::from("private-fixture\n")),
    ] {
        run(
            path,
            replay,
            good(),
            value,
            b"foreground startup=failed_preparation\n",
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        run(
            path,
            replay,
            Some(OsString::from_vec(vec![0xff])),
            auth(),
            b"foreground startup=failed_preparation\n",
        );
        run(
            path,
            replay,
            good(),
            Some(OsString::from_vec(vec![0xff])),
            b"foreground startup=failed_preparation\n",
        );
    }
}
pub(super) fn acceptance() {
    use std::os::unix::fs::OpenOptionsExt;
    let directory = crate::storage::tests::Directory::new();
    let root = directory.path().parent().unwrap().canonicalize().unwrap();
    let state = root.join("os-state");
    let config = root.join("os-runtime.json");
    let trust = root.join("os-root.der");
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
    failures(&config, false);
    assert!(!state.exists());
    valid(&config, false);
    let pending = || {
        let mut store =
            crate::storage::Store::open_private(&state, Duration::from_millis(50)).unwrap();
        let due = store.due_deliveries("research-cluster", 7200, 10).unwrap();
        assert_eq!(due.len(), 1);
        let item = &due[0];
        assert_eq!(item.attempt_number, 1);
        let bytes = Zeroizing::new(
            store
                .frozen_smtp(&item.event_id, "research_email")
                .unwrap()
                .unwrap()
                .data()
                .to_vec(),
        );
        let token = store
            .bound_smtp_provider(&item.event_id, "research_email")
            .unwrap()
            .unwrap()
            .token()
            .to_owned();
        (
            item.event_id.clone(),
            bytes,
            token,
            store
                .health_snapshot("research-cluster", 7200, 86400, (10, 10), 10)
                .unwrap()
                .1,
        )
    };
    let before = pending();
    let files = crate::status_boundary::files(&state);
    failures(&config, true);
    assert_eq!(crate::status_boundary::files(&state), files);
    let mut owner = crate::status_boundary::Running::start("--status-owner", &state);
    run(
        &config,
        true,
        Some("https://fixture.invalid/private-fixture".into()),
        Some("Bearer fixture".into()),
        b"foreground startup=failed_ownership\n",
    );
    assert_eq!(crate::status_boundary::files(&state), files);
    owner.finish(false, b"CLOSED\n");
    valid(&config, true);
    assert!(pending() == before);
}

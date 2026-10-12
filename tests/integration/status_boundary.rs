//! Trusted private status process fixtures, never an installed command or protocol.
use crate::{polling_cycle::Cycles, status_composition, storage::Store};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(super) fn child() -> bool {
    let mode = std::env::args().nth(1);
    if !matches!(
        mode.as_deref(),
        Some(
            "--status-inspect"
                | "--status-owner"
                | "--status-cancel-before"
                | "--status-cancel-held"
        )
    ) {
        return false;
    }
    let Some(path) = std::env::args().nth(2) else {
        println!("STATUS INVALID");
        return true;
    };
    if mode.as_deref() == Some("--status-owner") {
        let Ok(owner) = Store::open_private(Path::new(&path), Duration::from_millis(50)) else {
            println!("STATUS INVALID");
            return true;
        };
        ready();
        let _ = std::io::stdin().read_exact(&mut [0]);
        drop(owner);
        println!("CLOSED");
        return true;
    }
    let Ok(plan) = status_composition::prepare_path(Path::new(&path)) else {
        println!("STATUS INVALID");
        return true;
    };
    let mut samples = 0;
    let result = plan.inspect_with(
        &Cycles::new(1).unwrap(),
        10,
        Duration::from_millis(50),
        || {
            samples += 1;
            if mode.as_deref() == Some("--status-cancel-before") {
                return true;
            }
            if mode.as_deref() == Some("--status-cancel-held") && samples == 2 {
                ready();
                let _ = std::io::stdin().read_exact(&mut [0]);
                return true;
            }
            false
        },
    );
    use status_composition::InspectionError;
    let token = match result {
        Ok(report) if report.poll.stale => "STATUS STALE",
        Ok(_) => "STATUS FRESH",
        Err(InspectionError::Cancelled) => "STATUS CANCELLED",
        Err(InspectionError::State(crate::state_path::Error::AlreadyOwned)) => "STATUS OWNED",
        Err(_) => "STATUS INVALID",
    };
    println!("{token}");
    true
}
fn ready() {
    println!("READY");
    std::io::stdout().flush().unwrap();
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ack {
    Fresh,
    Stale,
    Owned,
    Cancelled,
    Invalid,
}
fn decode(bytes: &[u8]) -> Result<Ack, crate::transport::Error> {
    match bytes {
        b"STATUS FRESH\n" => Ok(Ack::Fresh),
        b"STATUS STALE\n" => Ok(Ack::Stale),
        b"STATUS OWNED\n" => Ok(Ack::Owned),
        b"STATUS CANCELLED\n" => Ok(Ack::Cancelled),
        b"STATUS INVALID\n" => Ok(Ack::Invalid),
        _ => Err(crate::transport::Error::InvalidInput),
    }
}
fn command(mode: &str, path: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([std::ffi::OsStr::new(mode), path.as_os_str()]);
    command
}
pub(super) fn inspect(mode: &str, path: &Path) -> Ack {
    crate::transport::supervise_with(
        &mut command(mode, path),
        Vec::new(),
        Duration::from_secs(2),
        0,
        64,
        decode,
    )
    .unwrap()
}
// Reap even if an assertion fails. The fixture never spawns descendants.
pub(super) struct Running {
    child: Child,
    messages: mpsc::Receiver<Vec<u8>>,
    reader: Option<thread::JoinHandle<()>>,
}
impl Running {
    pub(super) fn start(mode: &str, path: &Path) -> Self {
        let mut child = command(mode, path)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut output = child.stdout.take().unwrap().take(64);
        let (sender, messages) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut ready = vec![0; 6];
            if output.read_exact(&mut ready).is_err() {
                return;
            }
            let _ = sender.send(ready);
            let mut rest = Vec::new();
            if output.read_to_end(&mut rest).is_ok() {
                let _ = sender.send(rest);
            }
        });
        let running = Self {
            child,
            messages,
            reader: Some(reader),
        };
        assert_eq!(
            running
                .messages
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            b"READY\n"
        );
        running
    }
    pub(super) fn finish(&mut self, kill: bool, expected: &[u8]) {
        if kill {
            self.child.kill().unwrap();
        } else {
            self.child.stdin.take().unwrap().write_all(b"x").unwrap();
        }
        self.collect(!kill, expected);
    }
    pub(super) fn finish_signal(&mut self, number: libc::c_int, expected: &[u8]) {
        assert!(matches!(number, libc::SIGINT | libc::SIGTERM));
        // SAFETY: target is the owned, READY child PID; valid signal constants.
        assert_eq!(
            unsafe { libc::kill(self.child.id().try_into().unwrap(), number) },
            0
        );
        self.collect(true, expected);
    }
    fn collect(&mut self, success: bool, expected: &[u8]) {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.success(), success);
                break;
            }
            assert!(Instant::now() < until, "status child did not exit");
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            self.messages.recv_timeout(Duration::from_secs(2)).unwrap(),
            expected
        );
        self.reader.take().unwrap().join().unwrap();
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
pub(super) fn files(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries: Vec<_> = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_str().unwrap().to_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}
pub(super) fn acceptance() {
    use std::os::unix::fs::OpenOptionsExt;
    let directory = crate::storage::tests::Directory::new();
    let root = directory.path().parent().unwrap().canonicalize().unwrap();
    let state = root.join("state");
    let config = root.join("status.json");
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../docs/proposals/examples/local-mixed-config.json"
    ))
    .unwrap();
    source["storage"]["path"] = serde_json::json!(state.join("state.sqlite3"));
    source["providers"]["research_email"]["trust_der_files"] =
        serde_json::json!([root.join("absent.der")]);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&config)
        .unwrap()
        .write_all(&serde_json::to_vec(&source).unwrap())
        .unwrap();
    assert_eq!(inspect("--status-cancel-before", &config), Ack::Cancelled);
    assert!(!state.exists());
    assert_eq!(inspect("--status-inspect", &config), Ack::Invalid);
    assert!(!state.exists());
    let mut store = Store::open_private(&state, Duration::from_millis(50)).unwrap();
    let mut reconciler = crate::lifecycle::Reconciler::with_capacity(10);
    let events = reconciler
        .observe_snapshot(
            "",
            "research-cluster|1|alice|s|actual|COMPLETED|0:0",
            "research-cluster",
            "alice",
            true,
        )
        .unwrap();
    store
        .persist_reconciler(
            &reconciler,
            &events,
            &["primary"],
            "research-cluster",
            "1970-01-01T00:00:10",
            10,
        )
        .unwrap();
    drop(store);
    let before = files(&state);
    assert_eq!(inspect("--status-inspect", &config), Ack::Fresh);
    for kill in [false, true] {
        let mut owner = Running::start("--status-owner", &state);
        assert_eq!(inspect("--status-inspect", &config), Ack::Owned);
        owner.finish(kill, if kill { b"" } else { b"CLOSED\n" });
        assert_eq!(inspect("--status-inspect", &config), Ack::Fresh);
        assert_eq!(files(&state), before);
    }
    for kill in [false, true] {
        let mut held = Running::start("--status-cancel-held", &config);
        assert_eq!(inspect("--status-inspect", &config), Ack::Owned);
        held.finish(kill, if kill { b"" } else { b"STATUS CANCELLED\n" });
        assert_eq!(inspect("--status-inspect", &config), Ack::Fresh);
        assert_eq!(files(&state), before);
    }
    // Only exact static fixture tokens cross stdout; arbitrary diagnostics are rejected.
    assert!(decode(b"STATUS private metadata\n").is_err());
}

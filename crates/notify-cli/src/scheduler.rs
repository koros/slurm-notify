//! Shared bounded direct-process scheduler boundary; no polling defaults.
use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum QueryError {
    MissingCommand,
    Io,
    Timeout,
    OutputLimit,
    Exit(Option<i32>),
    InvalidUtf8,
    Cleanup,
}

fn read_bounded(reader: impl Read, limit: usize) -> Result<Vec<u8>, QueryError> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| QueryError::Io)?;
    if bytes.len() > limit {
        Err(QueryError::OutputLimit)
    } else {
        Ok(bytes)
    }
}

// Linux/macOS C ABI for kill(2). Keep the unsafe boundary small and private.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn kill_group(pid: u32) -> io::Result<()> {
    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let group = i32::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid process group"))?;
    if group <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid process group",
        ));
    }
    // SAFETY: kill takes two integer values, no pointers. The negative PID targets
    // only the group created for this child. SIGKILL is 9 on Linux and macOS.
    if unsafe { kill(-group, 9) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn prepare_environment(command: &mut Command) {
    let mut names: Vec<_> = std::env::vars_os().map(|(name, _)| name).collect();
    names.extend(command.get_envs().map(|(name, _)| name.to_owned()));
    for name in names {
        let text = name.to_string_lossy();
        if text.starts_with("SQUEUE_")
            || text.starts_with("SACCT_")
            || text == "SLURM_TIME_FORMAT"
            || text == "SLURM_BITSTR_LEN"
        {
            command.env_remove(name);
        }
    }
    command.env("LC_ALL", "C").env("TZ", "UTC");
}

// Each call owns one direct child. Callers must serialize queries. stderr is
// drained but never included in diagnostics because it may contain sensitive data.
pub(super) fn run(
    command: &mut Command,
    timeout: Duration,
    limit: usize,
) -> Result<String, QueryError> {
    prepare_environment(command);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .spawn()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                QueryError::MissingCommand
            } else {
                QueryError::Io
            }
        })?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let out_sender = sender.clone();
    let out_reader = thread::spawn(move || {
        let _ = out_sender.send((true, read_bounded(stdout, limit)));
    });
    let err_reader = thread::spawn(move || {
        let _ = sender.send((false, read_bounded(stderr, limit)));
    });
    let deadline = Instant::now() + timeout;
    let result = (|| {
        let mut output = None;
        let mut drained = 0;
        while drained < 2 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let (is_stdout, bytes) = receiver
                .recv_timeout(remaining)
                .map_err(|_| QueryError::Timeout)?;
            let bytes = bytes?;
            if is_stdout {
                output = Some(bytes);
            }
            drained += 1;
        }
        loop {
            let status = child.try_wait().map_err(|_| QueryError::Io)?;
            if let Some(status) = status {
                if !status.success() {
                    return Err(QueryError::Exit(status.code()));
                }
                return String::from_utf8(output.unwrap()).map_err(|_| QueryError::InvalidUtf8);
            }
            if Instant::now() >= deadline {
                return Err(QueryError::Timeout);
            }
            thread::sleep(Duration::from_millis(2));
        }
    })();
    // Terminate residual same-group descendants even when the direct child exits.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let _ = kill_group(child.id());
    if result.is_err() {
        let _ = child.kill();
    }
    child.wait().map_err(|_| QueryError::Cleanup)?;
    let cleanup_deadline = Instant::now() + Duration::from_secs(1);
    while !out_reader.is_finished() || !err_reader.is_finished() {
        if Instant::now() >= cleanup_deadline {
            return Err(QueryError::Cleanup);
        }
        thread::sleep(Duration::from_millis(2));
    }
    out_reader.join().map_err(|_| QueryError::Cleanup)?;
    err_reader.join().map_err(|_| QueryError::Cleanup)?;
    result
}

fn active_command(program: &std::path::Path, user: &str) -> Command {
    let mut command = Command::new(program);
    command
        .args(["--noheader", "--array", "--local"])
        .arg(format!("--user={user}"))
        .arg("--format=%i|%u|%V|%S|%T");
    command
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PollError {
    InvalidRequest,
    ClusterMismatch,
    Active(QueryError),
    Accounting(QueryError),
    Parse(super::lifecycle::ParseError),
}

// Deliberately accepts only fixed-width UTC calendar timestamps, not Slurm's
// relative date language. Lexical ordering is safe after calendar validation.
pub(super) fn valid_timestamp(value: &str) -> bool {
    crate::state_timestamp::valid(value)
}

pub(super) struct PollRequest<'a> {
    pub(super) active_program: &'a std::path::Path,
    pub(super) accounting_program: &'a std::path::Path,
    pub(super) cluster: &'a str,
    pub(super) user: &'a str,
    pub(super) history_start: &'a str,
    pub(super) history_end: &'a str,
    pub(super) timeout: Duration,
    pub(super) output_limit: usize,
    pub(super) emit_discovered: bool,
}

// Explicit windows and limits only; no public defaults. Injection lets ordinary
// tests run the complete query/parse/reconcile path without Slurm access.
pub(super) fn poll_with(
    request: &PollRequest<'_>,
    reconciler: &mut super::lifecycle::Reconciler,
    mut query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
) -> Result<Vec<super::lifecycle::Event>, PollError> {
    if !valid_timestamp(request.history_start)
        || !valid_timestamp(request.history_end)
        || request.history_start >= request.history_end
        || request.timeout.is_zero()
        || request.output_limit == 0
        || request.user.is_empty()
        || request.cluster.is_empty()
    {
        return Err(PollError::InvalidRequest);
    }
    let started = Instant::now();
    let mut active = active_command(request.active_program, request.user);
    let active_output =
        query(&mut active, request.timeout, request.output_limit).map_err(PollError::Active)?;
    let remaining = request.timeout.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(PollError::Accounting(QueryError::Timeout));
    }
    let mut accounting = Command::new(request.accounting_program);
    accounting
        .args([
            "--noheader",
            "--parsable2",
            "--allocations",
            "--array",
            "--local",
            "--duplicates",
        ])
        .arg(format!("--user={}", request.user))
        .arg(format!("--starttime={}", request.history_start))
        .arg(format!("--endtime={}", request.history_end))
        .arg("--format=Cluster,JobID,User,Submit,Start,State%32,ExitCode");
    let accounting_output =
        query(&mut accounting, remaining, request.output_limit).map_err(PollError::Accounting)?;
    if crate::lifecycle::parse_accounting(&accounting_output, request.user)
        .map_err(PollError::Parse)?
        .iter()
        .any(|record| record.cluster() != request.cluster)
    {
        return Err(PollError::ClusterMismatch);
    }
    reconciler
        .observe_snapshot(
            &active_output,
            &accounting_output,
            request.cluster,
            request.user,
            request.emit_discovered,
        )
        .map_err(PollError::Parse)
}

#[derive(Debug)]
pub(super) enum DurablePollError {
    Poll(PollError),
    Storage(super::storage::StorageError),
}

#[cfg(test)]
pub(super) fn poll_durable_with(
    request: &PollRequest<'_>,
    reconciler: &mut super::lifecycle::Reconciler,
    store: &mut super::storage::Store,
    destinations: &[&str],
    observed_at: i64,
    query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
) -> Result<Vec<super::lifecycle::Event>, DurablePollError> {
    let mut candidate = reconciler.clone();
    let events = poll_with(request, &mut candidate, query).map_err(DurablePollError::Poll)?;
    store
        .persist_reconciler(
            &candidate,
            &events,
            destinations,
            request.cluster,
            request.history_end,
            observed_at,
        )
        .map_err(DurablePollError::Storage)?;
    *reconciler = candidate;
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn durable_store(
        directory: &super::super::storage::tests::Directory,
    ) -> super::super::storage::Store {
        super::super::storage::Store::open(&directory.path(), Duration::from_millis(50)).unwrap()
    }

    fn durable_poll(
        req: &PollRequest<'_>,
        reconciler: &mut super::super::lifecycle::Reconciler,
        store: &mut super::super::storage::Store,
        active: &str,
        accounting: &str,
        destinations: &[&str],
    ) -> Result<Vec<super::super::lifecycle::Event>, DurablePollError> {
        poll_durable_with(req, reconciler, store, destinations, 10, |command, _, _| {
            Ok(if command.get_program() == "squeue" {
                active
            } else {
                accounting
            }
            .into())
        })
    }

    fn database_count(directory: &super::super::storage::tests::Directory, table: &str) -> i64 {
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    #[test]
    fn cluster_mismatch_is_explicit_and_never_mutates_reconciler() {
        let mut reconciler = super::super::lifecycle::Reconciler::with_capacity(10);
        let request = PollRequest {
            active_program: std::path::Path::new("/fixture/squeue"),
            accounting_program: std::path::Path::new("/fixture/sacct"),
            cluster: "configured",
            user: "alice",
            history_start: "2026-10-25T10:00:00",
            history_end: "2026-10-25T11:00:00",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        let error = poll_with(&request, &mut reconciler, |command, _, _| {
            Ok(if command.get_program() == "/fixture/squeue" {
                "".into()
            } else {
                "actual|1|alice|s|a|COMPLETED|0:0".into()
            })
        })
        .unwrap_err();
        assert_eq!(error, PollError::ClusterMismatch);
        assert!(reconciler.latest.is_empty());
        assert!(reconciler.emitted.is_empty());
    }
    #[test]
    fn durable_lifecycle_restores_deduplication_and_checkpoint_after_reopen() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = durable_store(&directory);
        let (mut reconciler, checkpoint) = store.restore("cluster", 20).unwrap();
        assert!(checkpoint.is_none());
        let active = "1|alice|s|actual|RUNNING";
        assert_eq!(
            durable_poll(
                &request(),
                &mut reconciler,
                &mut store,
                active,
                "",
                &["primary"]
            )
            .unwrap()
            .len(),
            1
        );
        drop(store);
        drop(reconciler);
        let mut store = durable_store(&directory);
        let (mut reconciler, checkpoint) = store.restore("cluster", 20).unwrap();
        assert_eq!(checkpoint.as_deref(), Some(request().history_end));
        assert!(durable_poll(
            &request(),
            &mut reconciler,
            &mut store,
            active,
            "",
            &["primary"]
        )
        .unwrap()
        .is_empty());
        let completed = "cluster|1|alice|s|actual|COMPLETED|0:0";
        assert_eq!(
            durable_poll(
                &request(),
                &mut reconciler,
                &mut store,
                "",
                completed,
                &["primary"]
            )
            .unwrap()
            .len(),
            1
        );
        drop(store);
        let mut store = durable_store(&directory);
        let (mut reconciler, _) = store.restore("cluster", 20).unwrap();
        assert!(durable_poll(
            &request(),
            &mut reconciler,
            &mut store,
            active,
            completed,
            &["primary"]
        )
        .unwrap()
        .is_empty());
        assert_eq!(database_count(&directory, "jobs"), 1);
        assert_eq!(database_count(&directory, "events"), 2);
        assert_eq!(database_count(&directory, "outbox"), 2);
    }

    #[test]
    fn pending_identity_promotion_is_persisted_without_ghost_rows() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = durable_store(&directory);
        let (mut reconciler, _) = store.restore("cluster", 20).unwrap();
        let mut req = request();
        req.emit_discovered = false;
        durable_poll(
            &req,
            &mut reconciler,
            &mut store,
            "1|alice|s|estimate|PENDING",
            "",
            &["primary"],
        )
        .unwrap();
        drop(store);
        let mut store = durable_store(&directory);
        let (mut reconciler, _) = store.restore("cluster", 20).unwrap();
        assert_eq!(
            durable_poll(
                &req,
                &mut reconciler,
                &mut store,
                "1|alice|s|actual|RUNNING",
                "",
                &["primary"]
            )
            .unwrap()
            .len(),
            1
        );
        drop(store);
        let mut store = durable_store(&directory);
        let (reconciler, _) = store.restore("cluster", 20).unwrap();
        assert_eq!(reconciler.latest.len(), 1);
        assert_eq!(database_count(&directory, "jobs"), 1);
    }

    #[test]
    fn suppressed_initial_events_remain_suppressed_after_restart() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = durable_store(&directory);
        let (mut reconciler, _) = store.restore("cluster", 20).unwrap();
        let mut req = request();
        req.emit_discovered = false;
        let active = "1|alice|s|actual|RUNNING";
        assert!(
            durable_poll(&req, &mut reconciler, &mut store, active, "", &["primary"])
                .unwrap()
                .is_empty()
        );
        assert_eq!(database_count(&directory, "events"), 1);
        assert_eq!(database_count(&directory, "outbox"), 0);
        drop(store);
        let mut store = durable_store(&directory);
        let (mut reconciler, _) = store.restore("cluster", 20).unwrap();
        assert!(durable_poll(
            &request(),
            &mut reconciler,
            &mut store,
            active,
            "",
            &["primary"]
        )
        .unwrap()
        .is_empty());
        assert_eq!(
            durable_poll(
                &request(),
                &mut reconciler,
                &mut store,
                "",
                "cluster|1|alice|s|actual|FAILED|1:0",
                &["primary"]
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(database_count(&directory, "events"), 2);
        assert_eq!(database_count(&directory, "outbox"), 1);
    }

    #[test]
    fn failed_commit_preserves_memory_checkpoint_and_retry_event() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = durable_store(&directory);
        let (mut reconciler, _) = store.restore("cluster", 20).unwrap();
        let active = "1|alice|s|actual|RUNNING";
        match durable_poll(&request(), &mut reconciler, &mut store, active, "", &[""]) {
            Err(DurablePollError::Storage(super::super::storage::StorageError::Database(
                error,
            ))) => assert!(matches!(error, rusqlite::Error::SqliteFailure(_, _))),
            other => panic!("expected transactional failure, got {other:?}"),
        }
        assert!(reconciler.latest.is_empty());
        assert!(reconciler.emitted.is_empty());
        assert!(store.restore("cluster", 20).unwrap().1.is_none());
        assert_eq!(database_count(&directory, "events"), 0);
        assert_eq!(
            durable_poll(
                &request(),
                &mut reconciler,
                &mut store,
                active,
                "",
                &["primary"]
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn durable_query_failure_does_not_advance_memory_or_database() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = durable_store(&directory);
        let (mut reconciler, _) = store.restore("cluster", 20).unwrap();
        match poll_durable_with(
            &request(),
            &mut reconciler,
            &mut store,
            &["primary"],
            10,
            |_, _, _| Err(QueryError::MissingCommand),
        ) {
            Err(DurablePollError::Poll(PollError::Active(QueryError::MissingCommand))) => {}
            other => panic!("unexpected query result {other:?}"),
        }
        assert!(reconciler.latest.is_empty());
        assert!(store.restore("cluster", 20).unwrap().1.is_none());
    }

    #[test]
    fn older_history_window_cannot_regress_durable_checkpoint() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = durable_store(&directory);
        let (mut reconciler, _) = store.restore("cluster", 20).unwrap();
        durable_poll(
            &request(),
            &mut reconciler,
            &mut store,
            "1|alice|s|actual|RUNNING",
            "",
            &["primary"],
        )
        .unwrap();
        let mut older = request();
        older.history_end = "2026-10-10T01:00:00";
        assert!(matches!(
            durable_poll(
                &older,
                &mut reconciler,
                &mut store,
                "",
                "cluster|1|alice|s|actual|COMPLETED|0:0",
                &["primary"]
            ),
            Err(DurablePollError::Storage(
                super::super::storage::StorageError::Conflict
            ))
        ));
        assert_eq!(
            store.restore("cluster", 20).unwrap().1.as_deref(),
            Some(request().history_end)
        );
        assert_eq!(
            reconciler.latest.values().next(),
            Some(&super::super::lifecycle::State::Running)
        );
        assert_eq!(database_count(&directory, "events"), 1);
    }

    #[test]
    fn restore_capacity_and_event_identity_errors_do_not_reset_database() {
        let directory = super::super::storage::tests::Directory::new();
        let mut store = durable_store(&directory);
        let (mut reconciler, _) = store.restore("cluster", 20).unwrap();
        let mut req = request();
        req.emit_discovered = false;
        durable_poll(
            &req,
            &mut reconciler,
            &mut store,
            "1|alice|s|actual|RUNNING",
            "",
            &[],
        )
        .unwrap();
        assert!(matches!(
            store.restore("cluster", 0),
            Err(super::super::storage::StorageError::InvalidInput)
        ));
        assert_eq!(database_count(&directory, "events"), 1);
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute("UPDATE events SET event_id='invalid'", [])
            .unwrap();
        assert!(matches!(
            store.restore("cluster", 20),
            Err(super::super::storage::StorageError::Conflict)
        ));
        assert_eq!(database_count(&directory, "events"), 1);
    }

    fn helper(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "scheduler::tests::child_helper",
                "--nocapture",
            ])
            .env("SLURM_NOTIFY_TEST_CHILD", mode);
        command
    }

    #[test]
    #[ignore = "invoked only as a subprocess by runner tests"]
    fn child_helper() {
        match std::env::var("SLURM_NOTIFY_TEST_CHILD").unwrap().as_str() {
            "descendant" => {
                let mut descendant = helper("sleep");
                // Inherit the runner's process group and both output pipes.
                let mut child = descendant.spawn().unwrap();
                let _ = child.wait();
            }
            "environment" => {
                for name in ["SQUEUE_FORMAT", "SACCT_FORMAT", "SLURM_TIME_FORMAT"] {
                    assert!(std::env::var_os(name).is_none());
                }
                assert_eq!(std::env::var("LC_ALL").unwrap(), "C");
                assert_eq!(std::env::var("TZ").unwrap(), "UTC");
                assert_eq!(std::env::var("SLURM_CONF").unwrap(), "/test/site.conf");
            }
            "fail" => {
                eprintln!("sensitive diagnostic");
                std::process::exit(7);
            }
            "sleep" => thread::sleep(Duration::from_secs(30)),
            "flood" => {
                use std::io::Write;
                let _ = std::io::stdout().write_all(&vec![b'x'; 100_000]);
            }
            "stderr" => {
                use std::io::Write;
                let _ = std::io::stderr().write_all(&vec![b'x'; 100_000]);
            }
            "invalid" => {
                use std::io::Write;
                std::io::stdout().write_all(&[0xff]).unwrap();
            }
            "ok" => println!("fixture-output"),
            _ => panic!("invalid helper mode"),
        }
    }

    #[test]
    fn calendar_validation_handles_leap_years_and_malformed_input() {
        for value in ["2024-02-29T23:59:59", "2000-02-29T00:00:00"] {
            assert!(valid_timestamp(value));
        }
        for value in [
            "1900-02-29T00:00:00",
            "2026-02-29T00:00:00",
            "2026-04-31T00:00:00",
            "2026-10-10T24:00:00",
            "2026-10-10T00:60:00",
            "2026-10-10T00:00:60",
            "0000-01-01T00:00:00",
            "now",
            "2026-10-10T00:00:00Z",
            "éééééééééa",
        ] {
            assert!(!valid_timestamp(value), "{value}");
        }
    }

    #[test]
    fn invalid_windows_and_limits_never_execute_queries() {
        let mut reconciler = super::super::lifecycle::Reconciler::default();
        for mode in 0..5 {
            let mut req = request();
            match mode {
                0 => req.history_start = req.history_end,
                1 => req.history_start = "now",
                2 => req.timeout = Duration::ZERO,
                3 => req.output_limit = 0,
                _ => req.user = "",
            }
            assert_eq!(
                poll_with(&req, &mut reconciler, |_, _, _| panic!(
                    "invalid request queried scheduler"
                ))
                .unwrap_err(),
                PollError::InvalidRequest
            );
        }
    }

    fn request() -> PollRequest<'static> {
        PollRequest {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(5),
            output_limit: 4096,
            emit_discovered: true,
        }
    }

    #[test]
    fn complete_mock_poll_deduplicates_then_reports_completion() {
        let mut reconciler = super::super::lifecycle::Reconciler::default();
        let mut terminal = false;
        for expected in [1, 0, 1, 0] {
            let events = poll_with(&request(), &mut reconciler, |command, budget, limit| {
                assert!(budget <= Duration::from_secs(5));
                assert_eq!(limit, 4096);
                if command.get_program() == "squeue" {
                    Ok(if terminal {
                        ""
                    } else {
                        "1|alice|s|actual|RUNNING"
                    }
                    .into())
                } else {
                    let args: Vec<_> = command.get_args().map(|v| v.to_str().unwrap()).collect();
                    assert!(args.contains(&"--starttime=2026-10-10T00:00:00"));
                    assert!(args.contains(&"--endtime=2026-10-11T00:00:00"));
                    assert!(args.contains(&"--duplicates"));
                    Ok(if terminal {
                        "cluster|1|alice|s|actual|COMPLETED|0:0"
                    } else {
                        ""
                    }
                    .into())
                }
            })
            .unwrap();
            assert_eq!(events.len(), expected);
            if expected == 0 {
                terminal = true;
            }
        }
    }

    #[test]
    fn failed_second_query_does_not_consume_active_events() {
        let mut reconciler = super::super::lifecycle::Reconciler::default();
        let error = poll_with(&request(), &mut reconciler, |command, _, _| {
            if command.get_program() == "squeue" {
                Ok("1|alice|s|actual|RUNNING".into())
            } else {
                Err(QueryError::MissingCommand)
            }
        })
        .unwrap_err();
        assert_eq!(error, PollError::Accounting(QueryError::MissingCommand));
        assert_eq!(
            poll_with(&request(), &mut reconciler, |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    "1|alice|s|actual|RUNNING"
                } else {
                    ""
                }
                .into())
            })
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn malformed_accounting_and_active_command_failure_are_distinct() {
        let mut reconciler = super::super::lifecycle::Reconciler::default();
        assert_eq!(
            poll_with(&request(), &mut reconciler, |_, _, _| Err(
                QueryError::Exit(Some(1))
            ))
            .unwrap_err(),
            PollError::Active(QueryError::Exit(Some(1)))
        );
        assert!(matches!(
            poll_with(&request(), &mut reconciler, |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    "1|alice|s|actual|RUNNING"
                } else {
                    "malformed"
                }
                .into())
            }),
            Err(PollError::Parse(_))
        ));
    }

    #[test]
    fn runner_can_be_injected_without_contacting_slurm() {
        let mut reconciler = super::super::lifecycle::Reconciler::default();
        let mut req = request();
        req.active_program = std::path::Path::new("/nonexistent-slurm-notify-test/squeue");
        assert_eq!(
            poll_with(&req, &mut reconciler, run).unwrap_err(),
            PollError::Active(QueryError::MissingCommand)
        );
    }

    #[test]
    fn formatting_overrides_are_removed_but_site_config_is_preserved() {
        let mut command = helper("environment");
        command
            .env("SQUEUE_FORMAT", "bad")
            .env("SACCT_FORMAT", "bad")
            .env("SLURM_TIME_FORMAT", "bad")
            .env("SLURM_CONF", "/test/site.conf");
        assert!(run(&mut command, Duration::from_secs(5), 4096).is_ok());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn timeout_closes_descendant_held_pipes() {
        let start = Instant::now();
        assert_eq!(
            run(&mut helper("descendant"), Duration::from_millis(200), 4096),
            Err(QueryError::Timeout)
        );
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(kill_group(0).is_err());
    }

    #[test]
    fn success_and_nonzero_exit_are_distinct() {
        assert!(run(&mut helper("ok"), Duration::from_secs(5), 4096)
            .unwrap()
            .contains("fixture-output"));
        assert_eq!(
            run(&mut helper("fail"), Duration::from_secs(5), 4096),
            Err(QueryError::Exit(Some(7)))
        );
    }

    #[test]
    fn missing_command_is_diagnosable() {
        let mut command = Command::new("/nonexistent-slurm-notify-test/squeue");
        assert_eq!(
            run(&mut command, Duration::from_secs(1), 4096),
            Err(QueryError::MissingCommand)
        );
    }

    #[test]
    fn timeout_kills_and_reaps_direct_child() {
        let start = Instant::now();
        assert_eq!(
            run(&mut helper("sleep"), Duration::from_millis(100), 4096),
            Err(QueryError::Timeout)
        );
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(run(&mut helper("ok"), Duration::from_secs(5), 4096).is_ok());
    }

    #[test]
    fn both_streams_are_bounded_and_invalid_encoding_is_rejected() {
        for mode in ["flood", "stderr"] {
            assert_eq!(
                run(&mut helper(mode), Duration::from_secs(5), 4096),
                Err(QueryError::OutputLimit)
            );
        }
        assert_eq!(
            run(&mut helper("invalid"), Duration::from_secs(5), 4096),
            Err(QueryError::InvalidUtf8)
        );
    }

    #[test]
    fn arguments_preserve_user_as_literal_and_expand_arrays() {
        let command = active_command(std::path::Path::new("squeue"), "alice;echo secret");
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "--noheader",
                "--array",
                "--local",
                "--user=alice;echo secret",
                "--format=%i|%u|%V|%S|%T"
            ]
        );
    }
}

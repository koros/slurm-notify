//! Custom test harness doubles as a trusted private child, never installed.
#![allow(dead_code)]
#[path = "../../crates/notify-cli/src/delivery.rs"]
mod delivery;
#[path = "../../crates/notify-cli/src/lifecycle.rs"]
mod lifecycle;
#[path = "../../crates/notify-cli/src/scheduler.rs"]
mod scheduler;
#[path = "../../crates/notify-cli/src/storage.rs"]
mod storage;
#[path = "../../crates/notify-cli/src/transport.rs"]
mod transport;
#[path = "../../crates/notify-cli/src/webhook.rs"]
mod webhook;

#[path = "../../crates/notify-cli/src/state_path.rs"]
mod state_path;

#[path = "../../crates/notify-cli/src/accounting_window.rs"]
mod accounting_window;
#[path = "../../crates/notify-cli/src/polling_cycle.rs"]
mod polling_cycle;

#[path = "../../crates/notify-cli/src/supervised_provider.rs"]
mod supervised_provider;

use delivery::ProviderOutcome;
use rusqlite::OptionalExtension;
use std::process::Command;
use std::time::{Duration, Instant};

fn main() {
    let mode = std::env::args().nth(1);
    if matches!(mode.as_deref(), Some("--child" | "--blocked-resolver")) {
        let blocked = mode.as_deref() == Some("--blocked-resolver");
        let result = transport::serve(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            1024 * 1024,
            |fields| transport::send_fields(fields, blocked),
        );
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    acceptance();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn request(
    endpoint: &str,
    mode: &str,
    deadline: Duration,
) -> Result<ProviderOutcome, transport::Error> {
    request_for(endpoint, "event-1", mode, deadline)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn request_for(
    endpoint: &str,
    event_id: &str,
    mode: &str,
    deadline: Duration,
) -> Result<ProviderOutcome, transport::Error> {
    let input = transport::frame(
        &[
            endpoint.as_bytes(),
            b"Bearer fixture-placeholder",
            event_id.as_bytes(),
            b"{}",
            b"1000",
        ],
        4096,
    )
    .unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.arg(mode);
    transport::supervise(&mut command, input, deadline, 4096, 64)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn acceptance() {
    for (response, expected) in [
        (
            "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
            ProviderOutcome::Delivered,
        ),
        (
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 60\r\nContent-Length: 0\r\n\r\n",
            ProviderOutcome::RetryAfter(60),
        ),
        (
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n",
            ProviderOutcome::Rejected,
        ),
        (
            "HTTP/1.1 503 Unavailable\r\nRetry-After: Thu, 01 Jan 1970 00:01:00 GMT\r\nContent-Length: 0\r\n\r\n",
            ProviderOutcome::RetryAfter(0),
        ),
    ] {
        let (endpoint, server) = webhook::tests::server(response, Duration::ZERO);
        assert_eq!(
            request(&endpoint, "--child", Duration::from_secs(3)),
            Ok(expected)
        );
        let received = server.join().unwrap();
        assert!(received.ends_with("\r\n\r\n{}"));
        assert!(received
            .to_ascii_lowercase()
            .contains("idempotency-key: event-1\r\n"));
    }
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::from_millis(300),
    );
    assert_eq!(
        request(&endpoint, "--child", Duration::from_millis(100)),
        Err(transport::Error::Timeout)
    );
    server.join().unwrap();
    let (endpoint, _certificate, server) =
        webhook::tests::tls_server(vec!["127.0.0.1".into()], false);
    assert_eq!(
        request(&endpoint, "--child", Duration::from_secs(3)),
        Ok(ProviderOutcome::Retry)
    );
    assert!(!server.join().unwrap());
    let start = Instant::now();
    assert_eq!(
        request(
            "https://example.invalid/",
            "--blocked-resolver",
            Duration::from_millis(100)
        ),
        Err(transport::Error::Timeout)
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    let mut child = Command::new(std::env::current_exe().unwrap());
    child.arg("--child");
    assert!(
        transport::supervise(&mut child, vec![255; 4], Duration::from_secs(2), 4096, 64).is_err()
    );
    owned_pipeline_restart();
    orchestrated_owned_http_restart();
    orchestrated_cleanup_failure_and_post_delivery_cancellation();
    orchestrated_timeout_retry_recovery();
    orchestrated_blocked_dispatch_restart();
    accepted_send_commit_failure_replays();
    println!("transport_boundary: HTTP acknowledgment, rate limit, rejection, past HTTP-date, slow HTTP, TLS rejection, blocked resolver, malformed frame, owned polling/restart, accepted-send rollback/replay, orchestrated owned HTTP restart/cancellation, cleanup failure/post-delivery cancellation, timeout retry recovery, blocked dispatch restart passed");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn poll_fixture(
    store: &mut storage::Store,
    reconciler: &mut lifecycle::Reconciler,
) -> Vec<lifecycle::Event> {
    let request = scheduler::PollRequest {
        active_program: std::path::Path::new("/fixture/squeue"),
        accounting_program: std::path::Path::new("/fixture/sacct"),
        cluster: "cluster",
        user: "alice",
        history_start: "2026-10-10T00:00:00",
        history_end: "2026-10-10T01:00:00",
        timeout: Duration::from_secs(1),
        output_limit: 4096,
        emit_discovered: true,
    };
    scheduler::poll_durable_with(
        &request,
        reconciler,
        store,
        &["primary"],
        10,
        |command, _, _| {
            if command.get_program() == "/fixture/squeue" {
                Ok(String::new())
            } else {
                Ok("cluster|1|alice|2026-10-10T00:00:01|2026-10-10T00:00:02|COMPLETED|0:0".into())
            }
        },
    )
    .unwrap()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn retry_policy() -> delivery::RetryPolicy {
    delivery::RetryPolicy {
        max_attempts: 3,
        initial_delay_seconds: 5,
        max_delay_seconds: 20,
        jitter_seconds: 0,
        jitter_seed: 0,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn state_directory(directory: &storage::tests::Directory) -> std::path::PathBuf {
    directory
        .path()
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join("pipeline-state")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn id_header(request: &str) -> String {
    request
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("idempotency-key:"))
        .unwrap()
        .to_owned()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_pipeline_restart() {
    let directory = storage::tests::Directory::new();
    let path = state_directory(&directory);
    let mut store = storage::Store::open_private(&path, Duration::from_millis(50)).unwrap();
    assert!(matches!(
        storage::Store::open_private(&path, Duration::from_millis(50)),
        Err(storage::StorageError::StatePath(
            state_path::Error::AlreadyOwned
        ))
    ));
    let mut reconciler = lifecycle::Reconciler::with_capacity(10);
    assert_eq!(poll_fixture(&mut store, &mut reconciler).len(), 1);
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 60\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let report = delivery::deliver_due_with_clock(
        &mut store,
        "cluster",
        10,
        10,
        retry_policy(),
        |item| request_for(&endpoint, &item.event_id, "--child", Duration::from_secs(3)).unwrap(),
        || Some(30),
    )
    .unwrap();
    assert_eq!(report.retrying, 1);
    let first_request = server.join().unwrap();
    drop(store);
    let mut store = storage::Store::open_private(&path, Duration::from_millis(50)).unwrap();
    let (mut reconciler, checkpoint) = store.restore("cluster", 10).unwrap();
    assert_eq!(checkpoint.as_deref(), Some("2026-10-10T01:00:00"));
    assert!(poll_fixture(&mut store, &mut reconciler).is_empty());
    assert!(store.due_deliveries("cluster", 89, 10).unwrap().is_empty());
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    assert_eq!(
        delivery::deliver_due_with_clock(
            &mut store,
            "cluster",
            90,
            10,
            retry_policy(),
            |item| {
                assert_eq!(item.attempt_number, 2);
                request_for(&endpoint, &item.event_id, "--child", Duration::from_secs(3)).unwrap()
            },
            || Some(100)
        )
        .unwrap()
        .delivered,
        1
    );
    let second_request = server.join().unwrap();
    assert_eq!(id_header(&first_request), id_header(&second_request));
    assert!(first_request.ends_with("\r\n\r\n{}"));
    assert!(second_request.ends_with("\r\n\r\n{}"));
    // Child execution must not release the parent's lifetime ownership.
    assert!(matches!(
        storage::Store::open_private(&path, Duration::from_millis(50)),
        Err(storage::StorageError::StatePath(
            state_path::Error::AlreadyOwned
        ))
    ));
    drop(store);
    let mut store = storage::Store::open_private(&path, Duration::from_millis(50)).unwrap();
    let (mut reconciler, _) = store.restore("cluster", 10).unwrap();
    assert!(poll_fixture(&mut store, &mut reconciler).is_empty());
    assert!(store.due_deliveries("cluster", 200, 10).unwrap().is_empty());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn accepted_send_commit_failure_replays() {
    let directory = storage::tests::Directory::new();
    let path = state_directory(&directory);
    let mut store = storage::Store::open_private(&path, Duration::from_millis(50)).unwrap();
    let mut reconciler = lifecycle::Reconciler::with_capacity(10);
    poll_fixture(&mut store, &mut reconciler);
    // A trusted fixture connection bypasses the cooperative lock only to inject failure.
    let database = rusqlite::Connection::open(path.join("state.sqlite3")).unwrap();
    database.execute_batch("CREATE TRIGGER fail_result BEFORE UPDATE ON outbox BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let result = delivery::deliver_due_with_clock(
        &mut store,
        "cluster",
        10,
        10,
        retry_policy(),
        |item| request_for(&endpoint, &item.event_id, "--child", Duration::from_secs(3)).unwrap(),
        || Some(30),
    );
    assert!(matches!(
        result,
        Err(delivery::WorkerError::Storage(
            storage::StorageError::Database(_)
        ))
    ));
    let first_request = server.join().unwrap();
    assert_eq!(
        database
            .query_row("SELECT count(*) FROM delivery_attempts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    database.execute_batch("DROP TRIGGER fail_result;").unwrap();
    drop(database);
    drop(store);
    let mut store = storage::Store::open_private(&path, Duration::from_millis(50)).unwrap();
    let (mut reconciler, _) = store.restore("cluster", 10).unwrap();
    assert!(poll_fixture(&mut store, &mut reconciler).is_empty());
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    assert_eq!(
        delivery::deliver_due_with_clock(
            &mut store,
            "cluster",
            40,
            10,
            retry_policy(),
            |item| {
                assert_eq!(item.attempt_number, 1);
                request_for(&endpoint, &item.event_id, "--child", Duration::from_secs(3)).unwrap()
            },
            || Some(50)
        )
        .unwrap()
        .delivered,
        1
    );
    assert_eq!(
        id_header(&first_request),
        id_header(&server.join().unwrap())
    );
    assert!(store.due_deliveries("cluster", 100, 10).unwrap().is_empty());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestrated_owned_http_restart() {
    use polling_cycle::{Cycles, Hooks, Maintenance, Outcome, Settings, StageError};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = Settings {
        active_program: std::path::Path::new("/fixture/squeue"),
        accounting_program: std::path::Path::new("/fixture/sacct"),
        cluster: "cluster",
        user: "alice",
        timeout: Duration::from_secs(1),
        output_limit: 4096,
        emit_discovered: true,
        capacity: 10,
        destinations: &["primary"],
        window: accounting_window::Policy {
            lookback_seconds: 3600,
            overlap_seconds: 300,
        },
    };
    let maintenance = Maintenance {
        delivery_limit: 10,
        retry: retry_policy(),
        retain_seconds: 50,
        cleanup_limit: 10,
        stale_after: 60,
        health_sample_limit: 10,
    };
    let fixture = |command: &mut Command, _, _| {
        Ok(if command.get_program() == "/fixture/squeue" {
            String::new()
        } else {
            "cluster|1|alice|s|actual|COMPLETED|0:0".into()
        })
    };
    let mut cycles = Cycles::new(1).unwrap();
    let mut checks = 0;
    let cancelled = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |_: &storage::DueDelivery| panic!("cancelled supervised send"),
            clock: || Some(7200),
            cancelled: || {
                checks += 1;
                checks == 2
            },
        },
    );
    assert!(cancelled.cancelled);
    assert!(matches!(
        cancelled.poll.unwrap().unwrap(),
        Outcome::Committed { events: 1, .. }
    ));
    assert!(cancelled.delivery.is_none());
    let id = store.due_deliveries("cluster", 7200, 10).unwrap()[0]
        .event_id
        .clone();
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let mut cycles = Cycles::new(1).unwrap();
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 60\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let mut times = [7200, 7200, 7210, 7210, 7210].into_iter();
    let retry = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |item: &storage::DueDelivery| {
                request_for(&endpoint, &item.event_id, "--child", Duration::from_secs(3)).unwrap()
            },
            clock: || times.next(),
            cancelled: || false,
        },
    );
    assert_eq!(retry.delivery.unwrap().unwrap().retrying, 1);
    assert!(retry.health.unwrap().is_ok());
    let first_request = server.join().unwrap();
    assert!(first_request.contains(&id));
    assert!(store
        .due_deliveries("cluster", 7269, 10)
        .unwrap()
        .is_empty());
    assert!(matches!(
        storage::Store::open_private(&root, Duration::from_millis(50)),
        Err(storage::StorageError::StatePath(
            state_path::Error::AlreadyOwned
        ))
    ));
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let mut cycles = Cycles::new(2).unwrap();
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let delivered = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: |_: &mut Command, _, _| Err(scheduler::QueryError::Timeout),
            send: |item: &storage::DueDelivery| {
                assert_eq!(item.event_id, id);
                assert_eq!(item.attempt_number, 2);
                request_for(&endpoint, &item.event_id, "--child", Duration::from_secs(3)).unwrap()
            },
            clock: || Some(7270),
            cancelled: || false,
        },
    );
    assert_eq!(delivered.poll.unwrap().unwrap_err(), StageError::Poll);
    assert_eq!(delivered.delivery.unwrap().unwrap().delivered, 1);
    assert_eq!(
        delivered.health.unwrap().unwrap().poll.consecutive_failures,
        1
    );
    let second_request = server.join().unwrap();
    assert!(second_request.contains(&id));
    assert!(first_request.ends_with("{}") && second_request.ends_with("{}"));
    let cleaned = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |_: &storage::DueDelivery| panic!("duplicate supervised send"),
            clock: || Some(7400),
            cancelled: || false,
        },
    );
    assert_eq!(cleaned.cleanup.unwrap().unwrap(), 1);
    assert_eq!(
        cleaned.health.unwrap().unwrap().poll.consecutive_failures,
        0
    );
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    assert!(store
        .due_deliveries("cluster", 7400, 10)
        .unwrap()
        .is_empty());
    assert_eq!(store.restore("cluster", 10).unwrap().0.emitted.len(), 1);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestrated_cleanup_failure_and_post_delivery_cancellation() {
    use polling_cycle::{Cycles, Hooks, Maintenance, Settings, StageError};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = Settings {
        active_program: std::path::Path::new("/fixture/squeue"),
        accounting_program: std::path::Path::new("/fixture/sacct"),
        cluster: "cluster",
        user: "alice",
        timeout: Duration::from_secs(1),
        output_limit: 4096,
        emit_discovered: true,
        capacity: 10,
        destinations: &["primary", "secondary"],
        window: accounting_window::Policy {
            lookback_seconds: 3600,
            overlap_seconds: 300,
        },
    };
    let maintenance = Maintenance {
        delivery_limit: 10,
        retry: retry_policy(),
        retain_seconds: 50,
        cleanup_limit: 10,
        stale_after: 60,
        health_sample_limit: 10,
    };
    let fixture = |command: &mut Command, _, _| {
        Ok(if command.get_program() == "/fixture/squeue" {
            String::new()
        } else {
            "cluster|1|alice|s|actual|COMPLETED|0:0".into()
        })
    };
    let (primary, first_server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let (secondary, second_server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let mut checks = 0;
    let mut sends = 0;
    let mut cycles = Cycles::new(1).unwrap();
    let cancelled = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |item: &storage::DueDelivery| {
                sends += 1;
                let endpoint = if item.destination == "primary" {
                    &primary
                } else {
                    &secondary
                };
                request_for(endpoint, &item.event_id, "--child", Duration::from_secs(3)).unwrap()
            },
            clock: || Some(7200),
            cancelled: || {
                checks += 1;
                checks == 3
            },
        },
    );
    assert!(cancelled.cancelled);
    assert_eq!(sends, 2);
    assert_eq!(cancelled.delivery.unwrap().unwrap().delivered, 2);
    assert!(cancelled.cleanup.is_none() && cancelled.health.is_none());
    assert_eq!(
        id_header(&first_server.join().unwrap()),
        id_header(&second_server.join().unwrap())
    );
    assert!(store
        .due_deliveries("cluster", 7200, 10)
        .unwrap()
        .is_empty());
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    // Deliberate test-only injection against the trusted owned database.
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_second_cleanup BEFORE DELETE ON outbox WHEN OLD.destination='secondary' BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
    let mut cycles = Cycles::new(2).unwrap();
    let failed = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |_: &storage::DueDelivery| panic!("delivered notification replayed"),
            clock: || Some(7300),
            cancelled: || false,
        },
    );
    assert_eq!(failed.delivery.unwrap().unwrap().delivered, 0);
    assert_eq!(failed.cleanup.unwrap().unwrap_err(), StageError::Storage);
    assert!(failed.health.unwrap().is_ok());
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM outbox WHERE status='delivered'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM delivery_attempts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(db);
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    db.execute_batch("DROP TRIGGER reject_second_cleanup;")
        .unwrap();
    let cleaned = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |_: &storage::DueDelivery| panic!("cleanup retry replayed notification"),
            clock: || Some(7400),
            cancelled: || false,
        },
    );
    assert_eq!(cleaned.cleanup.unwrap().unwrap(), 2);
    assert!(cleaned.health.unwrap().is_ok());
    assert_eq!(
        db.query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM delivery_attempts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(store.restore("cluster", 10).unwrap().0.emitted.len(), 1);
    assert_eq!(
        db.query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
            .optional()
            .unwrap(),
        None
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestrated_timeout_retry_recovery() {
    use polling_cycle::{Cycles, Hooks, Maintenance, Settings};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = Settings {
        active_program: std::path::Path::new("/fixture/squeue"),
        accounting_program: std::path::Path::new("/fixture/sacct"),
        cluster: "cluster",
        user: "alice",
        timeout: Duration::from_secs(1),
        output_limit: 4096,
        emit_discovered: true,
        capacity: 10,
        destinations: &["primary"],
        window: accounting_window::Policy {
            lookback_seconds: 3600,
            overlap_seconds: 300,
        },
    };
    let maintenance = Maintenance {
        delivery_limit: 10,
        retry: retry_policy(),
        retain_seconds: 50,
        cleanup_limit: 10,
        stale_after: 60,
        health_sample_limit: 10,
    };
    let fixture = |command: &mut Command, _, _| {
        Ok(if command.get_program() == "/fixture/squeue" {
            String::new()
        } else {
            "cluster|1|alice|s|actual|COMPLETED|0:0".into()
        })
    };
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::from_millis(1500),
    );
    let mut cycles = Cycles::new(1).unwrap();
    let mut times = [7200, 7200, 7210, 7300, 7300].into_iter();
    let mut timed_out_id = String::new();
    let tick_started = Instant::now();
    let timed_out = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |item: &storage::DueDelivery| {
                timed_out_id = item.event_id.clone();
                let result = request_for(
                    &endpoint,
                    &item.event_id,
                    "--child",
                    Duration::from_millis(500),
                );
                assert_eq!(result, Err(transport::Error::Timeout));
                let report = supervised_provider::classify(result);
                assert_eq!(report.diagnostic, supervised_provider::Diagnostic::Timeout);
                assert_eq!(report.acceptance, supervised_provider::Acceptance::Unknown);
                report.outcome.unwrap()
            },
            clock: || times.next(),
            cancelled: || false,
        },
    );
    assert!(tick_started.elapsed() < Duration::from_secs(2));
    assert_eq!(timed_out.delivery.unwrap().unwrap().retrying, 1);
    assert_eq!(timed_out.cleanup.unwrap().unwrap(), 0);
    assert!(timed_out.health.unwrap().is_ok());
    let first_request = server.join().unwrap();
    assert!(first_request.contains(&timed_out_id));
    assert!(store
        .due_deliveries("cluster", 7214, 10)
        .unwrap()
        .is_empty());
    let retry = store.due_deliveries("cluster", 7215, 10).unwrap();
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].attempt_number, 2);
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let mut cycles = Cycles::new(2).unwrap();
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let recovered = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |item: &storage::DueDelivery| {
                assert_eq!(item.event_id, timed_out_id);
                assert_eq!(item.attempt_number, 2);
                request_for(&endpoint, &item.event_id, "--child", Duration::from_secs(3)).unwrap()
            },
            clock: || Some(7300),
            cancelled: || false,
        },
    );
    assert_eq!(recovered.delivery.unwrap().unwrap().delivered, 1);
    let second_request = server.join().unwrap();
    assert_eq!(id_header(&first_request), id_header(&second_request));
    assert!(first_request.ends_with("{}") && second_request.ends_with("{}"));
    let cleaned = cycles.tick(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |_: &storage::DueDelivery| panic!("timeout recovery duplicate send"),
            clock: || Some(7400),
            cancelled: || false,
        },
    );
    assert_eq!(cleaned.cleanup.unwrap().unwrap(), 1);
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    assert!(store
        .due_deliveries("cluster", 7400, 10)
        .unwrap()
        .is_empty());
    assert_eq!(store.restore("cluster", 10).unwrap().0.emitted.len(), 1);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestrated_blocked_dispatch_restart() {
    use polling_cycle::{Cycles, Hooks, Maintenance, Settings, StageError};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = Settings {
        active_program: std::path::Path::new("/fixture/squeue"),
        accounting_program: std::path::Path::new("/fixture/sacct"),
        cluster: "cluster",
        user: "alice",
        timeout: Duration::from_secs(1),
        output_limit: 4096,
        emit_discovered: true,
        capacity: 10,
        destinations: &["a", "b", "c"],
        window: accounting_window::Policy {
            lookback_seconds: 3600,
            overlap_seconds: 300,
        },
    };
    let maintenance = Maintenance {
        delivery_limit: 10,
        retry: retry_policy(),
        retain_seconds: 50,
        cleanup_limit: 10,
        stale_after: 60,
        health_sample_limit: 10,
    };
    let fixture = |command: &mut Command, _, _| {
        Ok(if command.get_program() == "/fixture/squeue" {
            String::new()
        } else {
            "cluster|1|alice|s|actual|COMPLETED|0:0".into()
        })
    };
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let mut cycles = Cycles::new(1).unwrap();
    let mut calls = 0;
    let mut clocks = 0;
    let blocked = cycles.tick_fallible(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |item: &storage::DueDelivery| {
                calls += 1;
                let result = if item.destination == "a" {
                    request_for(&endpoint, &item.event_id, "--child", Duration::from_secs(3))
                } else {
                    assert_eq!(item.destination, "b");
                    let mut missing = Command::new(root.join("nonexistent-test-helper"));
                    transport::supervise(&mut missing, Vec::new(), Duration::from_secs(1), 4096, 64)
                };
                let report = supervised_provider::classify(result);
                if item.destination == "b" {
                    assert_eq!(report.diagnostic, supervised_provider::Diagnostic::Spawn);
                    assert_eq!(
                        report.acceptance,
                        supervised_provider::Acceptance::NotAttempted
                    );
                }
                report.into_dispatch()
            },
            clock: || {
                clocks += 1;
                Some(7200)
            },
            cancelled: || false,
        },
    );
    assert_eq!((calls, clocks), (2, 3));
    assert_eq!(
        blocked.delivery.unwrap().unwrap_err(),
        StageError::DispatchBlocked
    );
    assert!(blocked.cleanup.is_none() && blocked.health.is_none());
    let first_request = server.join().unwrap();
    let pending = store.due_deliveries("cluster", 7200, 10).unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|item| item.destination.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "c"]
    );
    let id = pending[0].event_id.clone();
    assert!(first_request.contains(&id));
    assert!(pending.iter().all(|item| item.attempt_number == 1));
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let (b_endpoint, b_server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let (c_endpoint, c_server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let recovered = Cycles::new(1).unwrap().tick_fallible(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |item: &storage::DueDelivery| {
                assert_eq!(item.event_id, id);
                assert_eq!(item.attempt_number, 1);
                let endpoint = match item.destination.as_str() {
                    "b" => &b_endpoint,
                    "c" => &c_endpoint,
                    _ => panic!("completed destination replayed"),
                };
                supervised_provider::classify(request_for(
                    endpoint,
                    &item.event_id,
                    "--child",
                    Duration::from_secs(3),
                ))
                .into_dispatch()
            },
            clock: || Some(7300),
            cancelled: || false,
        },
    );
    assert_eq!(recovered.delivery.unwrap().unwrap().delivered, 2);
    assert_eq!(recovered.cleanup.unwrap().unwrap(), 1);
    assert!(recovered.health.unwrap().is_ok());
    assert_eq!(
        id_header(&first_request),
        id_header(&b_server.join().unwrap())
    );
    assert_eq!(
        id_header(&first_request),
        id_header(&c_server.join().unwrap())
    );
    assert!(store
        .due_deliveries("cluster", 7300, 10)
        .unwrap()
        .is_empty());
    assert_eq!(store.restore("cluster", 10).unwrap().0.emitted.len(), 1);
}

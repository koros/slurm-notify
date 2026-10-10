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

use delivery::ProviderOutcome;
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
    accepted_send_commit_failure_replays();
    println!("transport_boundary: HTTP acknowledgment, rate limit, rejection, past HTTP-date, slow HTTP, TLS rejection, blocked resolver, malformed frame, owned polling/restart, accepted-send rollback/replay, orchestrated owned HTTP restart/cancellation passed");
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

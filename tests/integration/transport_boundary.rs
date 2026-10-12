//! Custom test harness doubles as a trusted private child, never installed.
#![allow(dead_code)]
#[path = "../../crates/notify-cli/src/config_snapshot.rs"]
mod config_snapshot;
#[path = "../../crates/notify-cli/src/credential_resolution.rs"]
mod credential_resolution;
#[path = "../../crates/notify-cli/src/delivery.rs"]
mod delivery;
#[path = "../../crates/notify-cli/src/lifecycle.rs"]
mod lifecycle;
#[path = "../../crates/notify-cli/src/provider_resolution.rs"]
mod provider_resolution;
#[path = "../../crates/notify-cli/src/scheduler.rs"]
mod scheduler;
#[path = "../../crates/notify-cli/src/smtp_bound_transport.rs"]
mod smtp_bound_transport;
#[path = "../../crates/notify-cli/src/smtp_frozen_frame.rs"]
mod smtp_frozen_frame;
#[path = "../../crates/notify-cli/src/smtp_metadata.rs"]
mod smtp_metadata;
#[path = "../../crates/notify-cli/src/smtp_poll.rs"]
mod smtp_poll;
#[path = "../../crates/notify-cli/src/smtp_provider_delivery.rs"]
mod smtp_provider_delivery;
#[path = "../../crates/notify-cli/src/smtp_provider_revision.rs"]
mod smtp_provider_revision;
#[path = "../../crates/notify-cli/src/smtp_render.rs"]
mod smtp_render;
#[path = "../../crates/notify-cli/src/smtp_revision.rs"]
mod smtp_revision;
#[path = "../../crates/notify-cli/src/smtp_revision_delivery.rs"]
mod smtp_revision_delivery;
#[path = "../../crates/notify-cli/src/smtp_routes.rs"]
mod smtp_routes;
#[path = "../../crates/notify-cli/src/storage.rs"]
mod storage;
#[path = "../../crates/notify-cli/src/transport.rs"]
mod transport;
#[path = "../../crates/notify-cli/src/trust_snapshot.rs"]
mod trust_snapshot;
#[path = "../../crates/notify-cli/src/webhook.rs"]
mod webhook;
#[path = "../../crates/notify-cli/src/webhook_resolution.rs"]
mod webhook_resolution;

#[path = "../../crates/notify-cli/src/state_path.rs"]
mod state_path;

#[path = "../../crates/notify-cli/src/accounting_window.rs"]
mod accounting_window;
#[path = "../../crates/notify-cli/src/polling_cycle.rs"]
mod polling_cycle;

#[path = "../../crates/notify-cli/src/supervised_provider.rs"]
mod supervised_provider;

#[path = "../../crates/notify-cli/src/smtp_ack.rs"]
mod smtp_ack;
#[path = "../../crates/notify-cli/src/smtp_auth.rs"]
mod smtp_auth;
mod smtp_auth_boundary;
#[path = "../../crates/notify-cli/src/smtp_auth_dispatch.rs"]
mod smtp_auth_dispatch;
#[path = "../../crates/notify-cli/src/smtp_auth_frame.rs"]
mod smtp_auth_frame;
#[path = "../../crates/notify-cli/src/smtp_dispatch.rs"]
mod smtp_dispatch;
#[path = "../../crates/notify-cli/src/smtp_message.rs"]
mod smtp_message;
mod smtp_negotiation_boundary;
#[path = "../../crates/notify-cli/src/smtp_reply.rs"]
mod smtp_reply;
#[path = "../../crates/notify-cli/src/smtp_submit.rs"]
mod smtp_submit;
mod smtp_submit_boundary;
#[path = "../../crates/notify-cli/src/smtp_submit_dispatch.rs"]
mod smtp_submit_dispatch;
#[path = "../../crates/notify-cli/src/smtp_submit_frame.rs"]
mod smtp_submit_frame;
#[path = "../../crates/notify-cli/src/smtp_tls.rs"]
mod smtp_tls;

#[path = "../../crates/notify-cli/src/config_decode.rs"]
mod config_decode;
#[allow(unused_imports)]
#[path = "../../crates/notify-cli/src/config_validate.rs"]
mod config_validate;
#[allow(unused_imports)]
#[path = "../../crates/notify-cli/src/route_selection.rs"]
mod route_selection;
#[path = "../../crates/notify-cli/src/runtime_mapping.rs"]
mod runtime_mapping;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod status_boundary;
#[path = "../../crates/notify-cli/src/status_composition.rs"]
mod status_composition;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod active_tick_boundary;
#[allow(unused_imports)]
#[path = "../../crates/notify-cli/src/config_composition.rs"]
mod config_composition;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod foreground_boundary;
#[allow(unused_imports)]
#[path = "../../crates/notify-cli/src/foreground_schedule.rs"]
mod foreground_schedule;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "../../crates/notify-cli/src/foreground_session.rs"]
mod foreground_session;
#[allow(unused_imports)]
#[path = "../../crates/notify-cli/src/foreground_wait.rs"]
mod foreground_wait;
#[allow(unused_imports)]
#[path = "../../crates/notify-cli/src/mixed_config_composition.rs"]
mod mixed_config_composition;
#[allow(unused_imports)]
#[path = "../../crates/notify-cli/src/observation_mapping.rs"]
mod observation_mapping;

#[allow(unused_imports)]
#[path = "../../crates/notify-cli/src/foreground_report.rs"]
mod foreground_report;

use delivery::ProviderOutcome;
use rusqlite::OptionalExtension;
use std::process::Command;
use std::time::{Duration, Instant};

fn main() {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if active_tick_boundary::child() || foreground_boundary::child() || status_boundary::child() {
        return;
    }
    if matches!(
        std::env::args().nth(1).as_deref(),
        Some("--smtp-frozen-fixture" | "--smtp-frozen-lost-result")
    ) {
        let lost = std::env::args().nth(1).as_deref() == Some("--smtp-frozen-lost-result");
        let result = if lost {
            smtp_submit_boundary::serve_frozen(std::io::stdin(), std::io::sink())
        } else {
            smtp_submit_boundary::serve_frozen(std::io::stdin(), std::io::stdout())
        };
        std::process::exit(if result.is_ok() && !lost { 0 } else { 1 });
    }

    if std::env::args().nth(1).as_deref() == Some("--smtp-bound-fixture") {
        std::process::exit(
            if smtp_bound_transport::serve(std::io::stdin(), std::io::stdout()).is_ok() {
                0
            } else {
                2
            },
        );
    }
    if std::env::args().nth(1).as_deref() == Some("--smtp-bound-lost-result") {
        let _ = smtp_bound_transport::serve(std::io::stdin(), std::io::sink());
        std::process::exit(7);
    }
    let mode = std::env::args().nth(1);
    if matches!(
        mode.as_deref(),
        Some("--smtp-wrong-ack" | "--smtp-failed-ack")
    ) {
        use std::io::Write;
        let failed = mode.as_deref() == Some("--smtp-failed-ack");
        let bytes = if failed {
            smtp_ack::Ack::Certificate.bytes()
        } else {
            b"OK\n"
        };
        std::io::stdout().write_all(bytes).unwrap();
        std::process::exit(if failed { 1 } else { 0 });
    }
    if matches!(
        mode.as_deref(),
        Some("--smtp-submit-wrong-ack" | "--smtp-submit-failed-ack")
    ) {
        use std::io::Write;
        let failed = mode.as_deref() == Some("--smtp-submit-failed-ack");
        let bytes: &[u8] = if failed {
            b"SMTP_SUBMIT FINAL ACCEPTED RA\n"
        } else {
            b"SMTP_AUTH AUTHENTICATED\n"
        };
        std::io::stdout().write_all(bytes).unwrap();
        std::process::exit(if failed { 1 } else { 0 });
    }
    if mode.as_deref() == Some("--smtp-submit-lost-result") {
        // Relay acceptance is observed normally, but its parent result is lost.
        let _ = smtp_submit_boundary::serve(std::io::stdin().lock(), std::io::sink());
        std::process::exit(1);
    }
    if mode.as_deref() == Some("--smtp-submit-fixture") {
        let result = smtp_submit_boundary::serve(std::io::stdin().lock(), std::io::stdout().lock());
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if mode.as_deref() == Some("--smtp-auth-fixture") {
        let result = smtp_auth_boundary::serve(std::io::stdin().lock(), std::io::stdout().lock());
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if mode.as_deref() == Some("--smtp-negotiation") {
        // Negotiation is not delivery: this probe never emits a delivery ACK.
        let result =
            smtp_negotiation_boundary::serve(std::io::stdin().lock(), std::io::stdout().lock());
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
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
    request_resources(
        Some(endpoint),
        Some("Bearer fixture-placeholder"),
        event_id,
        mode,
        deadline,
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn request_resources(
    endpoint: Option<&str>,
    authorization: Option<&str>,
    event_id: &str,
    mode: &str,
    deadline: Duration,
) -> Result<ProviderOutcome, transport::Error> {
    let resources = webhook_resolution::resolve(
        "URL",
        Some("AUTH"),
        Duration::from_secs(1),
        1024,
        |name, _| {
            let value = if name == "URL" {
                endpoint
            } else {
                authorization
            };
            value
                .map(|text| zeroize::Zeroizing::new(text.as_bytes().to_vec()))
                .ok_or(())
        },
    )
    .map_err(|_| transport::Error::InvalidInput)?;
    let input = resources
        .into_frame(event_id, b"{}")
        .map_err(|_| transport::Error::InvalidInput)?;
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
    status_boundary::acceptance();
    foreground_boundary::acceptance();
    active_tick_boundary::acceptance();
    owned_pipeline_restart();
    orchestrated_owned_http_restart();
    orchestrated_cleanup_failure_and_post_delivery_cancellation();
    orchestrated_timeout_retry_recovery();
    orchestrated_blocked_dispatch_restart();
    orchestrated_mid_batch_cancellation_restart();
    cancelled_preflight_preserves_owned_store();
    accepted_send_commit_failure_replays();
    owned_smtp_auth_progress_restart();
    owned_frozen_submission_restart();
    orchestrated_frozen_blocked_restart();
    orchestrated_mixed_render_failure_restart();
    owned_smtp_submission_restart();
    owned_smtp_lost_result_replay();
    owned_revision_delivery_restart();
    owned_bound_transport_restart();
    smtp_submit_boundary::acceptance();
    smtp_auth_boundary::acceptance();
    smtp_negotiation_boundary::acceptance();
    smtp_negotiation_boundary::handshake_acceptance();
    smtp_negotiation_boundary::acknowledgment_acceptance();
    owned_smtp_progress_restart();
    owned_smtp_upgrade_policy_restart();
    println!("transport_boundary: HTTP acknowledgment, rate limit, rejection, past HTTP-date, slow HTTP, TLS rejection, blocked resolver, malformed frame, owned polling/restart, accepted-send rollback/replay, orchestrated owned HTTP restart/cancellation, cleanup failure/post-delivery cancellation, timeout retry recovery, blocked dispatch restart, mid-batch cancellation restart, cancelled preflight ownership, SMTP stalled/drip-fed greeting and upgrade deadlines, SMTP refusal and malformed frame, SMTP stalled/drip-fed/disconnected TLS handshake, SMTP certificate/protocol diagnostics, SMTP wrong/failed acknowledgments, owned SMTP I/O/timeout retry and certificate block restart, precise STARTTLS refusal and owned retry/block restart, supervised verified-TLS AUTH responses and SMTP submission final-reply boundaries and owned prepared SMTP tick retry/partial-progress restart and mixed-route rendering-failure recovery and status inspection owner exit/cancellation/privacy and foreground wait cancellation/termination/recovery and active query/send cancellation/replay passed");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn poll_fixture(
    store: &mut storage::Store,
    reconciler: &mut lifecycle::Reconciler,
) -> Vec<lifecycle::Event> {
    poll_fixture_with_destinations(store, reconciler, &["primary"])
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn poll_fixture_with_destinations(
    store: &mut storage::Store,
    reconciler: &mut lifecycle::Reconciler,
    destinations: &[&str],
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
        destinations,
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

// Synthetic private policy fixtures, not operational defaults.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestration_settings<'a>(destinations: &'a [&'a str]) -> polling_cycle::Settings<'a> {
    polling_cycle::Settings {
        active_program: std::path::Path::new("/fixture/squeue"),
        accounting_program: std::path::Path::new("/fixture/sacct"),
        cluster: "cluster",
        user: "alice",
        timeout: Duration::from_secs(1),
        output_limit: 4096,
        emit_discovered: true,
        capacity: 10,
        event_capacity: 10,
        destinations,
        window: accounting_window::Policy {
            lookback_seconds: 3600,
            overlap_seconds: 300,
        },
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestration_maintenance() -> polling_cycle::Maintenance {
    polling_cycle::Maintenance {
        delivery_limit: 10,
        retry: retry_policy(),
        retain_seconds: 50,
        cleanup_limit: 10,
        stale_after: 60,
        health_sample_limit: 10,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestration_query(
    command: &mut Command,
    _: Duration,
    _: usize,
) -> Result<String, scheduler::QueryError> {
    Ok(if command.get_program() == "/fixture/squeue" {
        String::new()
    } else {
        "cluster|1|alice|s|actual|COMPLETED|0:0".into()
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestrated_owned_http_restart() {
    use polling_cycle::{Cycles, Hooks, Outcome, StageError};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = orchestration_settings(&["primary"]);
    let maintenance = orchestration_maintenance();
    let fixture = orchestration_query;
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
    let decoy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    decoy.set_nonblocking(true).unwrap();
    let decoy_endpoint = format!("http://{}/hook", decoy.local_addr().unwrap());
    for (url, auth) in [
        (None, Some("Bearer fixture")),
        (Some("invalid-fixture"), Some("Bearer fixture")),
        (Some(decoy_endpoint.as_str()), None),
        (Some(decoy_endpoint.as_str()), Some("fixture\r\n")),
    ] {
        let blocked = cycles.tick_fallible(
            &settings,
            &maintenance,
            &mut store,
            Hooks {
                query: fixture,
                send: |item: &storage::DueDelivery| {
                    supervised_provider::classify(request_resources(
                        url,
                        auth,
                        &item.event_id,
                        "--child",
                        Duration::from_secs(3),
                    ))
                    .into_dispatch()
                },
                clock: || Some(7200),
                cancelled: || false,
            },
        );
        assert!(matches!(
            blocked.delivery,
            Some(Err(StageError::DispatchBlocked))
        ));
        assert!(blocked.cleanup.is_none() && blocked.health.is_none());
        let failure = blocked.delivery_failure.unwrap();
        assert_eq!(
            failure.diagnostic,
            Some(supervised_provider::Diagnostic::InvalidInput)
        );
        assert!(!failure.acceptance_uncertain);
        assert_eq!(failure.committed.delivered, 0);
        drop(store);
        store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let pending = store.due_deliveries("cluster", 7200, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].event_id, id);
        assert_eq!(pending[0].attempt_number, 1);
        assert!(decoy.accept().is_err());
    }
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
    use polling_cycle::{Cycles, Hooks, StageError};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = orchestration_settings(&["primary", "secondary"]);
    let maintenance = orchestration_maintenance();
    let fixture = orchestration_query;
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
                checks == 6
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
    use polling_cycle::{Cycles, Hooks};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = orchestration_settings(&["primary"]);
    let maintenance = orchestration_maintenance();
    let fixture = orchestration_query;
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
    use polling_cycle::{Cycles, Hooks, StageError};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = orchestration_settings(&["a", "b", "c"]);
    let maintenance = orchestration_maintenance();
    let fixture = orchestration_query;
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
    let failure = blocked.delivery_failure.as_ref().unwrap();
    assert_eq!(failure.committed.delivered, 1);
    assert_eq!(
        failure.diagnostic,
        Some(supervised_provider::Diagnostic::Spawn)
    );
    assert!(!failure.acceptance_uncertain);
    let text = format!("{failure:?}");
    assert!(!text.contains("nonexistent") && !text.contains("internal-v1:"));
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestrated_mid_batch_cancellation_restart() {
    use polling_cycle::{Cycles, Hooks, StageError};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = orchestration_settings(&["a", "b", "c"]);
    let maintenance = orchestration_maintenance();
    let fixture = orchestration_query;
    let (endpoint, server) = webhook::tests::server(
        "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    );
    let mut cycles = Cycles::new(1).unwrap();
    let cancellation = std::cell::Cell::new(false);
    let mut calls = 0;
    let mut clocks = 0;
    let cancelled = cycles.tick_fallible(
        &settings,
        &maintenance,
        &mut store,
        Hooks {
            query: fixture,
            send: |item: &storage::DueDelivery| {
                calls += 1;
                assert_eq!(item.destination, "a");
                let classified = supervised_provider::classify(request_for(
                    &endpoint,
                    &item.event_id,
                    "--child",
                    Duration::from_secs(3),
                ))
                .into_dispatch();
                cancellation.set(true);
                classified
            },
            clock: || {
                clocks += 1;
                Some(7200)
            },
            cancelled: || cancellation.get(),
        },
    );
    assert_eq!((calls, clocks), (1, 3));
    assert!(cancelled.cancelled);
    let failure = cancelled.delivery_failure.as_ref().unwrap();
    assert_eq!(failure.committed.delivered, 1);
    assert_eq!(failure.kind, delivery::FailureKind::Cancelled);
    assert_eq!(failure.diagnostic, None);
    assert!(!failure.acceptance_uncertain);
    assert_eq!(
        cancelled.delivery.unwrap().unwrap_err(),
        StageError::Cancelled
    );
    assert!(cancelled.cleanup.is_none() && cancelled.health.is_none());
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn cancelled_preflight_preserves_owned_store() {
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let report = polling_cycle::Cycles::new(1).unwrap().tick(
        &orchestration_settings(&["primary"]),
        &orchestration_maintenance(),
        &mut store,
        polling_cycle::Hooks {
            query: |_: &mut Command, _, _| panic!("cancelled preflight query"),
            send: |_: &storage::DueDelivery| panic!("cancelled preflight send"),
            clock: || panic!("cancelled preflight clock"),
            cancelled: || true,
        },
    );
    assert!(report.cancelled && report.preflight.is_none() && report.poll.is_none());
    assert!(matches!(
        storage::Store::open_private(&root, Duration::from_millis(50)),
        Err(storage::StorageError::StatePath(
            state_path::Error::AlreadyOwned
        ))
    ));
    assert_eq!(store.restore("cluster", 10).unwrap().1, None);
    drop(store);
    let mut reopened = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    assert_eq!(reopened.restore("cluster", 10).unwrap().1, None);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_smtp_progress_restart() {
    use supervised_provider::Diagnostic;
    fn negotiated(mode: u8) -> Result<smtp_ack::Ack, transport::Error> {
        let (port, server) = smtp_negotiation_boundary::handshake_fixture(mode);
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.arg("--smtp-negotiation");
        let result = transport::supervise_with(
            &mut child,
            transport::frame(&[port.as_bytes()], 9).unwrap(),
            Duration::from_millis(500),
            9,
            32,
            smtp_ack::decode,
        );
        server.join().unwrap();
        result
    }
    for transient_mode in [2, 0] {
        let directory = storage::tests::Directory::new();
        let root = state_directory(&directory);
        let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let mut reconciler = lifecycle::Reconciler::with_capacity(10);
        poll_fixture_with_destinations(&mut store, &mut reconciler, &["a", "b", "c"]);
        let before = store.due_deliveries("cluster", 10, 10).unwrap();
        assert_eq!(before.len(), 3);
        let identity = before[0].event_id.clone();
        let mut calls = 0;
        let mut completions = 0;
        let failure = delivery::deliver_due_detailed(
            &mut store,
            "cluster",
            10,
            10,
            retry_policy(),
            |item| {
                calls += 1;
                assert_eq!(item.event_id, identity);
                let result = match item.destination.as_str() {
                    "a" => negotiated(transient_mode),
                    "b" => negotiated(3),
                    _ => panic!("later destination must remain untouched"),
                };
                if item.destination == "a" {
                    if transient_mode == 2 {
                        assert_eq!(result, Ok(smtp_ack::Ack::Io));
                    } else {
                        assert_eq!(result, Err(transport::Error::Timeout));
                    }
                } else {
                    assert_eq!(result, Ok(smtp_ack::Ack::Certificate));
                }
                smtp_dispatch::classify(result).into_dispatch()
            },
            || {
                completions += 1;
                Some(20)
            },
        )
        .unwrap_err();
        assert_eq!(calls, 2);
        assert_eq!(completions, 1);
        assert_eq!(failure.kind, delivery::FailureKind::DispatchBlocked);
        assert_eq!(failure.diagnostic, Some(Diagnostic::SmtpCertificate));
        assert_eq!(failure.committed.retrying, 1);
        assert_eq!(failure.committed.delivered, 0);
        assert_eq!(failure.committed.failed, 0);
        assert!(!failure.acceptance_uncertain);
        drop(store);
        let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        assert!(matches!(
            storage::Store::open_private(&root, Duration::from_millis(50)),
            Err(storage::StorageError::StatePath(
                state_path::Error::AlreadyOwned
            ))
        ));
        let pending = store.due_deliveries("cluster", 24, 10).unwrap();
        assert_eq!(
            pending
                .iter()
                .map(|item| item.destination.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
        assert!(pending
            .iter()
            .all(|item| item.attempt_number == 1 && item.event_id == identity));
        let due = store.due_deliveries("cluster", 25, 10).unwrap();
        let retry = due.iter().find(|item| item.destination == "a").unwrap();
        assert_eq!(retry.attempt_number, 2);
        assert_eq!(retry.event_id, identity);
        let restored = store.restore("cluster", 10).unwrap();
        assert_eq!(restored.0.emitted.len(), 1);
        let blocked_again = delivery::deliver_due_detailed(
            &mut store,
            "cluster",
            24,
            10,
            retry_policy(),
            |item| {
                assert_eq!(item.destination, "b");
                smtp_dispatch::classify(negotiated(3)).into_dispatch()
            },
            || panic!("certificate block must not sample completion clock"),
        )
        .unwrap_err();
        assert_eq!(blocked_again.committed, delivery::BatchReport::default());
        assert_eq!(blocked_again.diagnostic, Some(Diagnostic::SmtpCertificate));
        drop(store);
        let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM outbox WHERE status = 'delivered'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(db);
        let store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let pending = store.due_deliveries("cluster", 25, 10).unwrap();
        assert_eq!(pending.len(), 3);
        assert_eq!(
            pending
                .iter()
                .find(|item| item.destination == "a")
                .unwrap()
                .attempt_number,
            2
        );
        assert!(pending
            .iter()
            .filter(|item| item.destination != "a")
            .all(|item| item.attempt_number == 1));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_smtp_upgrade_policy_restart() {
    for (capabilities, refusal, expected, retry) in [
        (
            &b"250-relay\r\n250 STARTTLS\r\n"[..],
            Some(&b"454 unavailable\r\n"[..]),
            smtp_ack::Ack::UpgradeTemporary,
            true,
        ),
        (
            b"250-relay\r\n250 STARTTLS\r\n",
            Some(&b"550 refused\r\n"[..]),
            smtp_ack::Ack::UpgradePermanent,
            false,
        ),
        (
            b"250-relay\r\n250 AUTH PLAIN\r\n",
            None,
            smtp_ack::Ack::UpgradeMissing,
            false,
        ),
    ] {
        let directory = storage::tests::Directory::new();
        let root = state_directory(&directory);
        let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let mut reconciler = lifecycle::Reconciler::with_capacity(10);
        poll_fixture(&mut store, &mut reconciler);
        let identity = store
            .due_deliveries("cluster", 10, 10)
            .unwrap()
            .remove(0)
            .event_id;
        let (port, server) = smtp_negotiation_boundary::refusal_fixture(capabilities, refusal);
        let mut clocks = 0;
        let result = delivery::deliver_due_detailed(
            &mut store,
            "cluster",
            10,
            10,
            retry_policy(),
            |item| {
                assert_eq!(item.event_id, identity);
                let mut child = Command::new(std::env::current_exe().unwrap());
                child.arg("--smtp-negotiation");
                let ack = transport::supervise_with(
                    &mut child,
                    transport::frame(&[port.as_bytes()], 9).unwrap(),
                    Duration::from_secs(2),
                    9,
                    32,
                    smtp_ack::decode,
                );
                assert_eq!(ack, Ok(expected));
                smtp_dispatch::classify(ack).into_dispatch()
            },
            || {
                clocks += 1;
                Some(20)
            },
        );
        server.join().unwrap();
        if retry {
            let report = result.unwrap();
            assert_eq!(report.retrying, 1);
            assert_eq!(report.delivered, 0);
            assert_eq!(clocks, 1);
        } else {
            let failure = result.unwrap_err();
            assert_eq!(failure.kind, delivery::FailureKind::DispatchBlocked);
            assert_eq!(
                failure.diagnostic,
                Some(smtp_dispatch::classify(Ok(expected)).diagnostic)
            );
            assert_eq!(failure.committed, delivery::BatchReport::default());
            assert!(!failure.acceptance_uncertain);
            assert_eq!(clocks, 0);
        }
        drop(store);
        let store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let due = if retry {
            assert!(store.due_deliveries("cluster", 24, 10).unwrap().is_empty());
            store.due_deliveries("cluster", 25, 10).unwrap()
        } else {
            store.due_deliveries("cluster", 10, 10).unwrap()
        };
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].event_id, identity);
        assert_eq!(due[0].attempt_number, if retry { 2 } else { 1 });
        drop(store);
        let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            i64::from(retry)
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM outbox WHERE status = 'delivered'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_smtp_auth_progress_restart() {
    use supervised_provider::Diagnostic;
    for transient_mode in [1, 5] {
        let directory = storage::tests::Directory::new();
        let root = state_directory(&directory);
        let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let mut reconciler = lifecycle::Reconciler::with_capacity(10);
        poll_fixture_with_destinations(&mut store, &mut reconciler, &["a", "b", "c"]);
        let before = store.due_deliveries("cluster", 10, 10).unwrap();
        assert_eq!(before.len(), 3);
        let identity = before[0].event_id.clone();
        let mut calls = 0;
        let mut completions = 0;
        let failure = delivery::deliver_due_detailed(
            &mut store,
            "cluster",
            10,
            10,
            retry_policy(),
            |item| {
                calls += 1;
                assert_eq!(item.event_id, identity);
                let result = match item.destination.as_str() {
                    "a" => smtp_auth_boundary::fixture_result(transient_mode),
                    "b" => smtp_auth_boundary::fixture_result(2),
                    _ => panic!("later destination must remain untouched"),
                };
                if item.destination == "a" {
                    if transient_mode == 1 {
                        assert_eq!(result, Ok(smtp_auth_frame::Ack::Temporary));
                    } else {
                        assert_eq!(result, Err(transport::Error::Timeout));
                    }
                } else {
                    assert_eq!(result, Ok(smtp_auth_frame::Ack::Permanent));
                }
                smtp_auth_dispatch::classify(result).into_dispatch()
            },
            || {
                completions += 1;
                Some(20)
            },
        )
        .unwrap_err();
        assert_eq!(calls, 2);
        assert_eq!(completions, 1);
        assert_eq!(failure.kind, delivery::FailureKind::DispatchBlocked);
        assert_eq!(failure.diagnostic, Some(Diagnostic::SmtpAuthPermanent));
        assert_eq!(failure.committed.retrying, 1);
        assert_eq!(failure.committed.delivered, 0);
        assert_eq!(failure.committed.failed, 0);
        assert!(!failure.acceptance_uncertain);
        drop(store);
        let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        assert!(matches!(
            storage::Store::open_private(&root, Duration::from_millis(50)),
            Err(storage::StorageError::StatePath(
                state_path::Error::AlreadyOwned
            ))
        ));
        let pending = store.due_deliveries("cluster", 24, 10).unwrap();
        assert_eq!(
            pending
                .iter()
                .map(|item| item.destination.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
        assert!(pending
            .iter()
            .all(|item| item.attempt_number == 1 && item.event_id == identity));
        let due = store.due_deliveries("cluster", 25, 10).unwrap();
        let retry = due.iter().find(|item| item.destination == "a").unwrap();
        assert_eq!(retry.attempt_number, 2);
        assert_eq!(retry.event_id, identity);
        let restored = store.restore("cluster", 10).unwrap();
        assert_eq!(restored.0.emitted.len(), 1);
        let blocked_again = delivery::deliver_due_detailed(
            &mut store,
            "cluster",
            24,
            10,
            retry_policy(),
            |item| {
                assert_eq!(item.destination, "b");
                smtp_auth_dispatch::classify(smtp_auth_boundary::fixture_result(2)).into_dispatch()
            },
            || panic!("permanent AUTH block must not sample completion clock"),
        )
        .unwrap_err();
        assert_eq!(blocked_again.committed, delivery::BatchReport::default());
        assert_eq!(
            blocked_again.diagnostic,
            Some(Diagnostic::SmtpAuthPermanent)
        );
        drop(store);
        let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM outbox WHERE status = 'delivered'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(db);
        let store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let pending = store.due_deliveries("cluster", 25, 10).unwrap();
        assert_eq!(pending.len(), 3);
        assert_eq!(
            pending
                .iter()
                .find(|item| item.destination == "a")
                .unwrap()
                .attempt_number,
            2
        );
        assert!(pending
            .iter()
            .filter(|item| item.destination != "a")
            .all(|item| item.attempt_number == 1));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_smtp_submission_restart() {
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let mut reconciler = lifecycle::Reconciler::with_capacity(10);
    poll_fixture_with_destinations(&mut store, &mut reconciler, &["a", "b", "c", "d", "e"]);
    let before = store.due_deliveries("cluster", 10, 10).unwrap();
    let identity = before[0].event_id.clone();
    let mut completions = 0;
    let failure = delivery::deliver_due_detailed(
        &mut store,
        "cluster",
        10,
        10,
        retry_policy(),
        |item| {
            assert_eq!(item.event_id, identity);
            let result = match item.destination.as_str() {
                "a" => smtp_submit_boundary::fixture_result(0),
                "b" => smtp_submit_boundary::fixture_result(2),
                "c" => smtp_submit_boundary::fixture_result(1),
                "d" => {
                    let mut command = Command::new(std::env::current_exe().unwrap());
                    command.arg("--smtp-submit-fixture");
                    transport::supervise_with(
                        &mut command,
                        zeroize::Zeroizing::new(Vec::new()),
                        Duration::from_millis(500),
                        smtp_submit_frame::INPUT_LIMIT,
                        smtp_submit_frame::OUTPUT_LIMIT,
                        smtp_submit_frame::decode_ack,
                    )
                }
                _ => panic!("later destination must remain untouched"),
            };
            smtp_submit_dispatch::classify(result).into_dispatch()
        },
        || {
            completions += 1;
            Some(20)
        },
    )
    .unwrap_err();
    assert_eq!(completions, 3);
    assert_eq!(failure.kind, delivery::FailureKind::DispatchBlocked);
    assert_eq!(
        failure.diagnostic,
        Some(supervised_provider::Diagnostic::SmtpInput)
    );
    assert_eq!(failure.committed.delivered, 1);
    assert_eq!(failure.committed.failed, 1);
    assert_eq!(failure.committed.retrying, 1);
    assert!(!failure.acceptance_uncertain);
    drop(store);
    let store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let pending = store.due_deliveries("cluster", 24, 10).unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|item| item.destination.as_str())
            .collect::<Vec<_>>(),
        vec!["d", "e"]
    );
    assert!(pending
        .iter()
        .all(|item| item.event_id == identity && item.attempt_number == 1));
    let due = store.due_deliveries("cluster", 25, 10).unwrap();
    let retry = due.iter().find(|item| item.destination == "c").unwrap();
    assert_eq!(retry.attempt_number, 2);
    assert_eq!(retry.event_id, identity);
    assert!(matches!(
        storage::Store::open_private(&root, Duration::from_millis(50)),
        Err(storage::StorageError::StatePath(
            state_path::Error::AlreadyOwned
        ))
    ));
    drop(store);
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM outbox WHERE status='delivered'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_smtp_lost_result_replay() {
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let mut reconciler = lifecycle::Reconciler::with_capacity(10);
    poll_fixture(&mut store, &mut reconciler);
    let identity = store.due_deliveries("cluster", 10, 10).unwrap()[0]
        .event_id
        .clone();
    let report = delivery::deliver_due_fallible(
        &mut store,
        "cluster",
        10,
        10,
        retry_policy(),
        |_| {
            // Server verifies the full frozen synthetic bytes, sends 250 and observes
            // QUIT. Child discards its parent acknowledgment and exits unsuccessfully.
            let report = smtp_submit_dispatch::classify(smtp_submit_boundary::fixture_result(10));
            assert_eq!(report.acceptance, supervised_provider::Acceptance::Unknown);
            report.into_dispatch()
        },
        || Some(20),
    )
    .unwrap();
    assert_eq!(report.retrying, 1);
    assert_eq!(report.delivered, 0);
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    assert!(store.due_deliveries("cluster", 24, 10).unwrap().is_empty());
    let retry = store.due_deliveries("cluster", 25, 10).unwrap().remove(0);
    assert_eq!(retry.event_id, identity);
    assert_eq!(retry.attempt_number, 2);
    let report = delivery::deliver_due_fallible(
        &mut store,
        "cluster",
        25,
        10,
        retry_policy(),
        |_| smtp_submit_dispatch::classify(smtp_submit_boundary::fixture_result(0)).into_dispatch(),
        || Some(30),
    )
    .unwrap();
    assert_eq!(report.delivered, 1);
    drop(store);
    let store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    assert!(store.due_deliveries("cluster", 100, 10).unwrap().is_empty());
    drop(store);
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestrated_mixed_render_failure_restart() {
    use polling_cycle::{Cycles, Hooks, PreparedHooks, StageError};
    use smtp_render::{JobVisibility, Metadata};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = orchestration_settings(&["a", "b", "plain"]);
    let mut cycles = Cycles::new(1).unwrap();
    let mut prepared = 0;
    let failed = cycles.tick_prepared_fallible(
        &settings,
        &orchestration_maintenance(),
        &mut store,
        PreparedHooks {
            smtp_destinations: &["a", "b"],
            max_messages: 2,
            query: orchestration_query,
            prepare: |event: &lifecycle::Event, route: &str| {
                prepared += 1;
                smtp_render::prepare(
                    event,
                    &Metadata {
                        sender: "s@fixture.invalid",
                        recipient: &format!("{route}@fixture.invalid"),
                        identity: if route == "b" {
                            "invalid\r\nfixture"
                        } else {
                            "mixed_fixture"
                        },
                        timestamp: 123,
                    },
                    JobVisibility::Hidden,
                )
                .map_err(|_| ())
            },
        },
        Hooks {
            query: (),
            send: |_: &storage::DueDelivery| panic!("failed poll exposed partial outbox"),
            clock: || Some(7200),
            cancelled: || false,
        },
    );
    assert_eq!(prepared, 2);
    assert_eq!(failed.poll.unwrap().unwrap_err(), StageError::Poll);
    assert_eq!(
        failed.delivery.unwrap().unwrap(),
        delivery::BatchReport::default()
    );
    assert_eq!(failed.cleanup.unwrap().unwrap(), 0);
    let health = failed.health.unwrap().unwrap();
    assert_eq!(
        health.poll.last_failure,
        Some(polling_cycle::Failure::Preparation)
    );
    assert!(health.poll.checkpoint_age_seconds.is_none());
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let (restored, checkpoint) = store.restore("cluster", 10).unwrap();
    assert!(restored.latest.is_empty() && restored.emitted.is_empty() && checkpoint.is_none());
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM smtp_messages", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(db);
    let mut restarted = Cycles::new(2).unwrap();
    let mut clocks = [7210, 7210, 7220, 7221, 7222, 7222, 7222].into_iter();
    let mut sent = Vec::new();
    let recovered = restarted.tick_prepared_fallible(
        &settings,
        &orchestration_maintenance(),
        &mut store,
        PreparedHooks {
            smtp_destinations: &["a", "b"],
            max_messages: 2,
            query: orchestration_query,
            prepare: |event: &lifecycle::Event, route: &str| {
                smtp_render::prepare(
                    event,
                    &Metadata {
                        sender: "s@fixture.invalid",
                        recipient: &format!("{route}@fixture.invalid"),
                        identity: "mixed_fixture",
                        timestamp: 123,
                    },
                    if route == "a" {
                        JobVisibility::Hidden
                    } else {
                        JobVisibility::Identifier
                    },
                )
                .map_err(|_| ())
            },
        },
        Hooks {
            query: (),
            send: |item: &storage::DueDelivery| {
                assert_eq!(item.attempt_number, 1);
                let reader =
                    storage::Store::open(&root.join("state.sqlite3"), Duration::from_millis(50))
                        .unwrap();
                let snapshot = reader
                    .frozen_smtp(&item.event_id, &item.destination)
                    .unwrap();
                if item.destination == "plain" {
                    assert!(snapshot.is_none());
                } else {
                    let snapshot = snapshot.unwrap();
                    let text = std::str::from_utf8(snapshot.data()).unwrap();
                    assert_eq!(text.contains("Job: [redacted]"), item.destination == "a");
                    assert_eq!(text.contains("Job: 1\r\n"), item.destination == "b");
                    assert!(!text.contains("cluster") && !text.contains("actual"));
                    assert!(
                        snapshot.envelope().1 == format!("{}@fixture.invalid", item.destination)
                    );
                }
                drop(reader);
                sent.push(item.destination.clone());
                if item.destination == "plain" {
                    Ok(delivery::ProviderOutcome::Delivered)
                } else {
                    frozen_dispatch(&root, item, 0)
                }
            },
            clock: || clocks.next(),
            cancelled: || false,
        },
    );
    assert_eq!(sent, vec!["a", "b", "plain"]);
    assert!(clocks.next().is_none());
    assert!(matches!(
        recovered.poll,
        Some(Ok(polling_cycle::Outcome::Committed { events: 1, .. }))
    ));
    assert_eq!(recovered.delivery.unwrap().unwrap().delivered, 3);
    assert_eq!(recovered.cleanup.unwrap().unwrap(), 0);
    assert_eq!(recovered.health.unwrap().unwrap().poll.last_failure, None);
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let replay = restarted.tick_prepared_fallible(
        &settings,
        &orchestration_maintenance(),
        &mut store,
        PreparedHooks {
            smtp_destinations: &["a", "b"],
            max_messages: 2,
            query: orchestration_query,
            prepare: |_: &lifecycle::Event, _: &str| panic!("replay changed disclosure"),
        },
        Hooks {
            query: (),
            send: |_: &storage::DueDelivery| panic!("replay resent completed delivery"),
            clock: || Some(7223),
            cancelled: || false,
        },
    );
    assert!(matches!(
        replay.poll,
        Some(Ok(polling_cycle::Outcome::Committed { events: 0, .. }))
    ));
    assert_eq!(
        replay.delivery.unwrap().unwrap(),
        delivery::BatchReport::default()
    );
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM smtp_messages", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

// Fixture-only reader: the parent keeps the owned store guard for the entire tick.
// Opening a second connection here proves the snapshot committed before child I/O.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn frozen_dispatch(
    root: &std::path::Path,
    item: &storage::DueDelivery,
    mode: u8,
) -> Result<delivery::ProviderOutcome, supervised_provider::Diagnostic> {
    let reader =
        storage::Store::open(&root.join("state.sqlite3"), Duration::from_millis(50)).unwrap();
    let snapshot = smtp_frozen_frame::load(&reader, &item.event_id, &item.destination).unwrap();
    drop(reader);
    smtp_submit_dispatch::classify(smtp_submit_boundary::frozen_result(mode, &snapshot))
        .into_dispatch()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn orchestrated_frozen_blocked_restart() {
    use polling_cycle::{Cycles, Hooks, Rendering, StageError};
    let directory = storage::tests::Directory::new();
    let root = state_directory(&directory);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let settings = orchestration_settings(&["a", "b", "c"]);
    let routes: Vec<_> = ["a", "b", "c"]
        .into_iter()
        .map(|name| smtp_routes::Route {
            name,
            visibility: smtp_render::JobVisibility::Hidden,
            metadata: smtp_routes::Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: name,
            },
        })
        .collect();
    let mut cycles = Cycles::new(1).unwrap();
    let mut calls = 0;
    let mut clocks = [7200, 7200, 7210].into_iter();
    let report = cycles.tick_rendered_fallible(
        &settings,
        &orchestration_maintenance(),
        &mut store,
        Rendering {
            routes: &routes,
            max_messages: 3,
        },
        Hooks {
            query: orchestration_query,
            send: |item: &storage::DueDelivery| {
                calls += 1;
                if item.destination == "a" {
                    frozen_dispatch(&root, item, 0)
                } else {
                    assert_eq!(item.destination, "b");
                    let mut missing = Command::new(root.join("nonexistent-smtp-helper"));
                    smtp_submit_dispatch::classify(transport::supervise_with(
                        &mut missing,
                        Vec::new(),
                        Duration::from_secs(1),
                        4096,
                        64,
                        smtp_submit_frame::decode_ack,
                    ))
                    .into_dispatch()
                }
            },
            clock: || clocks.next(),
            cancelled: || false,
        },
    );
    assert_eq!(calls, 2);
    assert!(clocks.next().is_none());
    assert!(matches!(
        report.poll,
        Some(Ok(polling_cycle::Outcome::Committed { events: 1, .. }))
    ));
    assert_eq!(
        report.delivery.unwrap().unwrap_err(),
        StageError::DispatchBlocked
    );
    let failure = report.delivery_failure.unwrap();
    assert_eq!(failure.committed.delivered, 1);
    assert_eq!(
        failure.diagnostic,
        Some(supervised_provider::Diagnostic::Spawn)
    );
    assert!(!failure.acceptance_uncertain);
    assert!(report.cleanup.is_none() && report.health.is_none());
    let pending = store.due_deliveries("cluster", 7210, 10).unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|item| item.destination.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "c"]
    );
    let identity = pending[0].event_id.clone();
    let snapshots: Vec<_> = pending
        .iter()
        .map(|item| smtp_frozen_frame::load(&store, &item.event_id, &item.destination).unwrap())
        .collect();
    drop(store);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let mut restarted = Cycles::new(1).unwrap();
    let mut clocks = [7211, 7211, 7220, 7221, 7221, 7221].into_iter();
    let mut calls = 0;
    let recovered = restarted.tick_rendered_fallible(
        &settings,
        &orchestration_maintenance(),
        &mut store,
        Rendering {
            routes: &routes,
            max_messages: 3,
        },
        Hooks {
            query: orchestration_query,
            send: |item: &storage::DueDelivery| {
                assert_eq!(item.event_id, identity);
                assert_eq!(item.attempt_number, 1);
                assert_eq!(item.destination, if calls == 0 { "b" } else { "c" });
                let reader =
                    storage::Store::open(&root.join("state.sqlite3"), Duration::from_millis(50))
                        .unwrap();
                let loaded =
                    smtp_frozen_frame::load(&reader, &item.event_id, &item.destination).unwrap();
                assert!(loaded.data() == snapshots[calls].data());
                assert!(loaded.envelope() == snapshots[calls].envelope());
                drop(reader);
                calls += 1;
                frozen_dispatch(&root, item, 0)
            },
            clock: || clocks.next(),
            cancelled: || false,
        },
    );
    assert_eq!(calls, 2);
    assert!(clocks.next().is_none());
    assert!(matches!(
        recovered.poll,
        Some(Ok(polling_cycle::Outcome::Committed { events: 0, .. }))
    ));
    assert_eq!(recovered.delivery.unwrap().unwrap().delivered, 2);
    assert_eq!(recovered.cleanup.unwrap().unwrap(), 0);
    assert!(recovered.health.unwrap().is_ok());
    assert!(store
        .due_deliveries("cluster", 7300, 10)
        .unwrap()
        .is_empty());
    drop(store);
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_frozen_submission_restart() {
    for first_mode in [1, 10] {
        let directory = storage::tests::Directory::new();
        let root = state_directory(&directory);
        let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();

        let routes = [smtp_routes::Route {
            name: "smtp",
            visibility: smtp_render::JobVisibility::Hidden,
            metadata: smtp_routes::Metadata {
                sender: "frozen@fixture.invalid",
                recipient: "kept@fixture.invalid",
                namespace: "original",
            },
        }];
        let settings = polling_cycle::Settings {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            timeout: Duration::from_secs(5),
            output_limit: 4096,
            emit_discovered: true,
            capacity: 10,
            event_capacity: 10,
            destinations: &["smtp"],
            window: accounting_window::Policy {
                lookback_seconds: 3600,
                overlap_seconds: 300,
            },
        };
        let mut cycles = polling_cycle::Cycles::new(1).unwrap();
        let mut clocks = [10, 10, 20, 20, 20].into_iter();
        let first = cycles.tick_rendered_fallible(
            &settings,
            &orchestration_maintenance(),
            &mut store,
            polling_cycle::Rendering {
                routes: &routes,
                max_messages: 1,
            },
            polling_cycle::Hooks {
                query: |command: &mut Command, _, _| {
                    Ok(if command.get_program() == "squeue" {
                        ""
                    } else {
                        "cluster|1|alice|s|actual|COMPLETED|0:0"
                    }
                    .into())
                },
                send: |item: &storage::DueDelivery| frozen_dispatch(&root, item, first_mode),
                clock: || clocks.next(),
                cancelled: || false,
            },
        );
        assert!(matches!(
            first.poll,
            Some(Ok(polling_cycle::Outcome::Committed { events: 1, .. }))
        ));
        assert_eq!(first.delivery.unwrap().unwrap().retrying, 1);
        assert_eq!(first.cleanup.unwrap().unwrap(), 0);
        assert!(first.health.unwrap().is_ok());
        assert!(clocks.next().is_none());
        assert!(store.due_deliveries("cluster", 24, 10).unwrap().is_empty());
        let item = store.due_deliveries("cluster", 25, 10).unwrap().remove(0);
        assert_eq!(item.attempt_number, 2);
        let snapshot = smtp_frozen_frame::load(&store, &item.event_id, &item.destination).unwrap();
        drop(store);
        let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let reloaded = smtp_frozen_frame::load(&store, &item.event_id, &item.destination).unwrap();
        assert!(reloaded.data() == snapshot.data());
        assert!(reloaded.envelope() == snapshot.envelope());
        let selected = smtp_metadata::select(&item.event, "smtp", "original", 10).unwrap();
        let text = std::str::from_utf8(reloaded.data()).unwrap();
        assert!(text.contains(&format!(
            "Message-ID: <{}@fixture.invalid>\r\n",
            selected.identity
        )));
        assert!(text.contains("Date: Thu, 01 Jan 1970 00:00:10 +0000\r\n"));
        assert!(reloaded.envelope() == ("frozen@fixture.invalid", "kept@fixture.invalid"));
        let changed_routes = [smtp_routes::Route {
            name: "smtp",
            visibility: smtp_render::JobVisibility::Identifier,
            metadata: smtp_routes::Metadata {
                sender: "changed@fixture.invalid",
                recipient: "changed@fixture.invalid",
                namespace: "changed",
            },
        }];
        let changed_plan =
            smtp_routes::Plan::new(settings.destinations, &changed_routes, 1).unwrap();
        let changed = changed_plan.prepare(&item.event, "smtp", 25).unwrap();
        assert!(changed.data() != snapshot.data());
        assert_ne!(
            smtp_metadata::select(&item.event, "smtp", "changed", 25)
                .unwrap()
                .identity,
            selected.identity
        );
        assert!(store
            .freeze_smtp(&item.event_id, &item.destination, &changed)
            .is_err());
        let identity = item.event_id.clone();
        let mut clocks = [25, 25, 30, 30, 30].into_iter();
        let mut restarted = polling_cycle::Cycles::new(1).unwrap();
        let second = restarted.tick_rendered_fallible(
            &settings,
            &orchestration_maintenance(),
            &mut store,
            polling_cycle::Rendering {
                routes: &changed_routes,
                max_messages: 1,
            },
            polling_cycle::Hooks {
                query: |command: &mut Command, _, _| {
                    Ok(if command.get_program() == "squeue" {
                        ""
                    } else {
                        "cluster|1|alice|s|actual|COMPLETED|0:0"
                    }
                    .into())
                },
                send: |item: &storage::DueDelivery| {
                    assert_eq!(item.event_id, identity);
                    assert_eq!(item.attempt_number, 2);
                    let reader = storage::Store::open(
                        &root.join("state.sqlite3"),
                        Duration::from_millis(50),
                    )
                    .unwrap();
                    let kept = smtp_frozen_frame::load(&reader, &item.event_id, &item.destination)
                        .unwrap();
                    assert!(kept.data() == snapshot.data());
                    assert!(kept.envelope() == snapshot.envelope());
                    drop(reader);
                    frozen_dispatch(&root, item, 0)
                },
                clock: || clocks.next(),
                cancelled: || false,
            },
        );
        assert!(matches!(
            second.poll,
            Some(Ok(polling_cycle::Outcome::Committed { events: 0, .. }))
        ));
        assert_eq!(second.delivery.unwrap().unwrap().delivered, 1);
        assert_eq!(second.cleanup.unwrap().unwrap(), 0);
        assert!(second.health.unwrap().is_ok());
        assert!(clocks.next().is_none());
        assert!(store.due_deliveries("cluster", 100, 10).unwrap().is_empty());
        drop(store);
        let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
        let attempts: Vec<(i64, String)> = db
            .prepare("SELECT attempted_at,outcome FROM delivery_attempts ORDER BY attempt_number")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            attempts,
            vec![(20, "transient_failure".into()), (30, "success".into())]
        );
    }
    // Use a fresh pending event to verify local rejection without a network call.
    let other = storage::tests::Directory::new();
    let root = state_directory(&other);
    let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
    let mut reconciler = lifecycle::Reconciler::with_capacity(10);
    poll_fixture_with_destinations(&mut store, &mut reconciler, &["smtp"]);
    let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
    let message = smtp_message::prepare(
        "s@fixture.invalid",
        "r@fixture.invalid",
        "id",
        "original",
        "original",
        123,
    )
    .unwrap();
    for corrupt in [false, true] {
        if corrupt {
            store
                .freeze_smtp(&item.event_id, &item.destination, &message)
                .unwrap();
            let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
            db.execute(
                "UPDATE smtp_messages SET data=?1",
                [b"bad\n.\r\n".as_slice()],
            )
            .unwrap();
        }
        let loaded = smtp_frozen_frame::load(&store, &item.event_id, &item.destination);
        let failure = delivery::deliver_due_detailed(
            &mut store,
            "cluster",
            10,
            10,
            retry_policy(),
            |_| {
                let error = match &loaded {
                    Err(_) => transport::Error::InvalidInput,
                    Ok(_) => panic!("invalid snapshot reached sender"),
                };
                smtp_submit_dispatch::classify(Err(error)).into_dispatch()
            },
            || panic!("blocked snapshot sampled completion clock"),
        )
        .unwrap_err();
        assert_eq!(failure.kind, delivery::FailureKind::DispatchBlocked);
        assert_eq!(
            store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
            1
        );
    }
    drop(store);
    let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM delivery_attempts", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_revision_delivery_restart() {
    use smtp_revision_delivery::{Hooks, Permission, Permissions};
    for mode in [1, 10] {
        let directory = storage::tests::Directory::new();
        let root = state_directory(&directory);
        let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let routes = [smtp_routes::Route {
            name: "smtp",
            visibility: smtp_render::JobVisibility::Hidden,
            metadata: smtp_routes::Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: "fixture",
            },
        }];
        let plan = smtp_routes::Plan::new(&["smtp"], &routes, 4).unwrap();
        let request = scheduler::PollRequest {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        let mut reconciler = lifecycle::Reconciler::with_capacity(10);
        plan.poll_revision_with(
            &request,
            &mut reconciler,
            &mut store,
            10,
            |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    ""
                } else {
                    "cluster|1|alice|s|actual|COMPLETED|0:0"
                }
                .into())
            },
        )
        .unwrap();
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let original = store
            .bound_smtp_revision(&item.event_id, "smtp")
            .unwrap()
            .unwrap();
        let saved = store.frozen_smtp(&item.event_id, "smtp").unwrap().unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "smtp",
            revision: original.token(),
        }];
        let permitted = Permissions::new(&entries).unwrap();
        let report = smtp_revision_delivery::deliver_due(
            &mut store,
            "cluster",
            10,
            10,
            retry_policy(),
            Hooks {
                permissions: &permitted,
                send: |_: &storage::DueDelivery,
                       revision: &smtp_revision::Descriptor,
                       frozen: &storage::FrozenMessage| {
                    assert_eq!(revision.token(), original.token());
                    assert_eq!(frozen.data(), saved.data());
                    smtp_submit_dispatch::classify(smtp_submit_boundary::frozen_result(
                        mode, frozen,
                    ))
                    .into_dispatch()
                },
                clock: || Some(12),
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(report.retrying, 1);
        let due = 12 + retry_policy().initial_delay_seconds;
        drop(store);
        let mut store = storage::Store::open_private(&root, Duration::from_millis(50)).unwrap();
        let denied = Permissions::new(&[]).unwrap();
        let failure = smtp_revision_delivery::deliver_due(
            &mut store,
            "cluster",
            due,
            10,
            retry_policy(),
            Hooks {
                permissions: &denied,
                send: |_: &storage::DueDelivery,
                       _: &smtp_revision::Descriptor,
                       _: &storage::FrozenMessage| {
                    panic!("denied revision spawned child")
                },
                clock: || panic!("denied revision clock"),
                cancelled: || false,
            },
        )
        .unwrap_err();
        assert_eq!(
            failure.diagnostic,
            Some(supervised_provider::Diagnostic::SmtpRevisionDenied)
        );
        assert_eq!(
            store.due_deliveries("cluster", due, 10).unwrap()[0].attempt_number,
            2
        );
        assert_eq!(store.prune_smtp_revisions("cluster", 10).unwrap(), 0);
        let report = smtp_revision_delivery::deliver_due(
            &mut store,
            "cluster",
            due,
            10,
            retry_policy(),
            Hooks {
                permissions: &permitted,
                send: |item: &storage::DueDelivery,
                       revision: &smtp_revision::Descriptor,
                       frozen: &storage::FrozenMessage| {
                    assert_eq!(item.attempt_number, 2);
                    assert_eq!(revision.token(), original.token());
                    assert_eq!(frozen.data(), saved.data());
                    smtp_submit_dispatch::classify(smtp_submit_boundary::frozen_result(0, frozen))
                        .into_dispatch()
                },
                clock: || Some(due + 1),
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(report.delivered, 1);
        assert!(store
            .due_deliveries("cluster", due + 1, 10)
            .unwrap()
            .is_empty());
        drop(store);
        let db = rusqlite::Connection::open(root.join("state.sqlite3")).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM delivery_attempts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM smtp_revision_bindings", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_bound_transport_restart() {
    use base64::Engine;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use smtp_provider_delivery::{Permission, Permissions, Resolved};
    use std::os::unix::fs::OpenOptionsExt;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::Arc,
    };
    fn resolve_resources(
        bound: &smtp_provider_revision::Descriptor,
        profile: &smtp_provider_revision::Profile<'_>,
        path: &std::path::Path,
        password: &[u8],
    ) -> Result<Resolved, smtp_provider_delivery::Error> {
        provider_resolution::resolve(
            bound,
            profile,
            &[path.to_owned()],
            "USER",
            "PASSWORD",
            |name, _| {
                Ok(zeroize::Zeroizing::new(if name == "USER" {
                    b"fixture-user".to_vec()
                } else {
                    password.to_vec()
                }))
            },
        )
        .map_err(|error| match error {
            provider_resolution::Error::Profile => smtp_provider_delivery::Error::Profile,
            provider_resolution::Error::Trust(_) | provider_resolution::Error::TrustMismatch => {
                smtp_provider_delivery::Error::Trust
            }
            provider_resolution::Error::Credentials(_) => {
                smtp_provider_delivery::Error::Credentials
            }
        })
    }
    fn line(stream: &mut impl Read) -> zeroize::Zeroizing<Vec<u8>> {
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        loop {
            let mut byte = [0];
            match stream.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    bytes.push(byte[0]);
                    assert!(bytes.len() <= 16384);
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        bytes
    }
    for mode in [1, 10] {
        let cert = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()]).unwrap();
        let root = cert.cert.der().to_vec();
        let trust = smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
        let config = Arc::new(
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(root.clone())],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                        cert.signing_key.serialize_der(),
                    )),
                )
                .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let decoy = TcpListener::bind("127.0.0.1:0").unwrap();
        decoy.set_nonblocking(true).unwrap();
        let profile = smtp_provider_revision::Profile {
            destination: "smtp",
            relay: "127.0.0.1",
            port: listener.local_addr().unwrap().port(),
            tls: smtp_provider_revision::TlsMode::RequiredStartTls,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: &trust,
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        };
        let provider = smtp_provider_revision::Descriptor::new(&profile).unwrap();
        let directory = storage::tests::Directory::new();
        let root_path = state_directory(&directory);
        let mut store =
            storage::Store::open_private(&root_path, Duration::from_millis(50)).unwrap();
        let trust_path = root_path.canonicalize().unwrap().join("trust.der");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&trust_path)
            .unwrap()
            .write_all(&root)
            .unwrap();
        let routes = [smtp_routes::Route {
            name: "smtp",
            visibility: smtp_render::JobVisibility::Hidden,
            metadata: smtp_routes::Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: "fixture",
            },
        }];
        let rendering = smtp_routes::Plan::new(&["smtp", "webhook"], &routes, 1).unwrap();
        let plan = smtp_routes::ProviderPlan::new(&rendering, &[provider.profile()]).unwrap();
        let request = scheduler::PollRequest {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "1970-01-01T00:00:00",
            history_end: "1970-01-01T00:00:10",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        let query = |command: &mut Command, _, _| {
            Ok(if command.get_program() == "squeue" {
                ""
            } else {
                "cluster|1|alice|s|actual|COMPLETED|0:0"
            }
            .into())
        };
        let mut reconciler = lifecycle::Reconciler::with_capacity(10);
        plan.poll_with(&request, &mut reconciler, &mut store, 10, query)
            .unwrap();
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let bound_rendering = store
            .bound_smtp_revision(&item.event_id, "smtp")
            .unwrap()
            .unwrap();
        let saved = store.frozen_smtp(&item.event_id, "smtp").unwrap().unwrap();
        let expected = zeroize::Zeroizing::new(saved.data().to_vec());
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(15);
            for attempt in 0..2 {
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline);
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => panic!("bound listener failed"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket.write_all(b"220 bound fixture\r\n").unwrap();
                assert!(line(&mut socket).as_slice() == b"EHLO fixture.invalid\r\n");
                socket.write_all(b"250-bound\r\n250 STARTTLS\r\n").unwrap();
                assert!(line(&mut socket).as_slice() == b"STARTTLS\r\n");
                socket.write_all(b"220 upgrade\r\n").unwrap();
                let mut stream = rustls::StreamOwned::new(
                    rustls::ServerConnection::new(config.clone()).unwrap(),
                    socket,
                );
                assert!(line(&mut stream).as_slice() == b"EHLO fixture.invalid\r\n");
                stream
                    .write_all(b"250-secure\r\n250 AUTH PLAIN\r\n")
                    .unwrap();
                let password = if attempt == 0 {
                    "first-fixture"
                } else {
                    "rotated-fixture"
                };
                let plain = zeroize::Zeroizing::new(format!("\0fixture-user\0{password}"));
                let auth = zeroize::Zeroizing::new(format!(
                    "AUTH PLAIN {}\r\n",
                    base64::engine::general_purpose::STANDARD.encode(plain.as_bytes())
                ));
                assert!(line(&mut stream).as_slice() == auth.as_bytes());
                stream.write_all(b"235 authenticated\r\n").unwrap();
                for (command, reply) in [
                    (
                        b"MAIL FROM:<s@fixture.invalid>\r\n".as_slice(),
                        b"250 sender\r\n".as_slice(),
                    ),
                    (b"RCPT TO:<r@fixture.invalid>\r\n", b"250 recipient\r\n"),
                    (b"DATA\r\n", b"354 go\r\n"),
                ] {
                    assert!(line(&mut stream).as_slice() == command);
                    stream.write_all(reply).unwrap();
                }
                let mut received = zeroize::Zeroizing::new(Vec::new());
                loop {
                    let bytes = line(&mut stream);
                    assert!(!bytes.is_empty());
                    received.extend_from_slice(&bytes);
                    assert!(received.len() <= 16384);
                    if bytes.as_slice() == b".\r\n" {
                        break;
                    }
                }
                assert!(received.as_slice() == expected.as_slice());
                let reply = if attempt == 0 && mode == 1 {
                    b"450 temporary\r\n".as_slice()
                } else {
                    b"250 accepted\r\n"
                };
                stream.write_all(reply).unwrap();
                stream.flush().unwrap();
            }
        });
        let entries = [Permission {
            cluster: "cluster",
            destination: "smtp",
            rendering: bound_rendering.token(),
            provider: provider.token(),
        }];
        let permissions = Permissions::new(&entries).unwrap();
        let cycle_settings = polling_cycle::Settings {
            active_program: request.active_program,
            accounting_program: request.accounting_program,
            cluster: "cluster",
            user: "alice",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
            capacity: 10,
            event_capacity: 10,
            destinations: &["smtp", "webhook"],
            window: accounting_window::Policy {
                lookback_seconds: 10,
                overlap_seconds: 1,
            },
        };
        let maintenance = polling_cycle::Maintenance {
            delivery_limit: 10,
            retry: retry_policy(),
            retain_seconds: 100,
            cleanup_limit: 10,
            stale_after: 60,
            health_sample_limit: 10,
        };
        let profiles = [provider.profile()];
        let mut cycles = polling_cycle::Cycles::new(1).unwrap();
        let mut times = [10, 10, 12, 12, 12, 12].into_iter();
        let tick = cycles.tick_mixed_bound_smtp(
            &cycle_settings,
            &maintenance,
            &mut store,
            polling_cycle::MixedBoundSmtp {
                ordinary_destinations: &["webhook"],
                smtp: polling_cycle::BoundSmtp {
                    rendering: polling_cycle::Rendering {
                        routes: &routes,
                        max_messages: 1,
                    },
                    profiles: &profiles,
                    permissions: &permissions,
                    timeout: Duration::from_secs(3),
                },
            },
            polling_cycle::Hooks {
                query,
                send: (
                    |bound: &smtp_provider_revision::Descriptor| {
                        resolve_resources(bound, &profile, &trust_path, b"first-fixture")
                    },
                    |_: &storage::DueDelivery| {
                        let mut command = Command::new(std::env::current_exe().unwrap());
                        command.arg(if mode == 10 {
                            "--smtp-bound-lost-result"
                        } else {
                            "--smtp-bound-fixture"
                        });
                        command
                    },
                    |item: &storage::DueDelivery| {
                        assert_eq!(item.destination, "webhook");
                        Ok(delivery::ProviderOutcome::Delivered)
                    },
                ),
                clock: || times.next(),
                cancelled: || false,
            },
        );
        assert!(matches!(
            tick.poll,
            Some(Ok(polling_cycle::Outcome::Committed { events: 0, .. }))
        ));
        let report = tick.delivery.unwrap().unwrap();
        assert_eq!(report.retrying, 1);
        assert_eq!(report.delivered, 1);
        drop(store);
        let mut store =
            storage::Store::open_private(&root_path, Duration::from_millis(50)).unwrap();
        let (mut restored, _) = store.restore("cluster", 10).unwrap();
        for missing_trust in [true, false] {
            if missing_trust {
                std::fs::remove_file(&trust_path).unwrap();
            } else {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&trust_path)
                    .unwrap()
                    .write_all(&root)
                    .unwrap();
            }
            let mut cycles = polling_cycle::Cycles::new(1).unwrap();
            let mut times = [17, 17].into_iter();
            let tick = cycles.tick_mixed_bound_smtp(
                &cycle_settings,
                &maintenance,
                &mut store,
                polling_cycle::MixedBoundSmtp {
                    ordinary_destinations: &["webhook"],
                    smtp: polling_cycle::BoundSmtp {
                        rendering: polling_cycle::Rendering {
                            routes: &routes,
                            max_messages: 1,
                        },
                        profiles: &profiles,
                        permissions: &permissions,
                        timeout: Duration::from_secs(3),
                    },
                },
                polling_cycle::Hooks {
                    query,
                    send: (
                        |bound: &smtp_provider_revision::Descriptor| {
                            resolve_resources(bound, &profile, &trust_path, b"")
                        },
                        |_: &storage::DueDelivery| -> Command {
                            panic!("resource block spawned child")
                        },
                        |_: &storage::DueDelivery| panic!("completed ordinary route replayed"),
                    ),
                    clock: || times.next(),
                    cancelled: || false,
                },
            );
            assert!(matches!(
                tick.delivery,
                Some(Err(polling_cycle::StageError::DispatchBlocked))
            ));
            assert!(tick.cleanup.is_none() && tick.health.is_none());
            let expected = if missing_trust {
                smtp_provider_delivery::Error::Trust
            } else {
                smtp_provider_delivery::Error::Credentials
            };
            assert_eq!(
                tick.delivery_failure.unwrap().diagnostic,
                Some(supervised_provider::Diagnostic::SmtpProviderResolution(
                    expected
                ))
            );
            // Polling commits independently before dispatch is blocked.
            let checkpoint = store.restore("cluster", 10).unwrap().1;
            assert_eq!(checkpoint.as_deref(), Some("1970-01-01T00:00:17"));
            drop(store);
            store = storage::Store::open_private(&root_path, Duration::from_millis(50)).unwrap();
            assert_eq!(
                store.due_deliveries("cluster", 17, 10).unwrap()[0].attempt_number,
                2
            );
            assert!(
                store
                    .frozen_smtp(&item.event_id, "smtp")
                    .unwrap()
                    .unwrap()
                    .data()
                    == saved.data()
            );
            assert_eq!(
                store
                    .bound_smtp_provider(&item.event_id, "smtp")
                    .unwrap()
                    .unwrap()
                    .token(),
                provider.token()
            );
            assert_eq!(store.restore("cluster", 10).unwrap().1, checkpoint);
            assert!(decoy.accept().is_err());
        }
        let changed_profile = smtp_provider_revision::Profile {
            port: decoy.local_addr().unwrap().port(),
            ..provider.profile()
        };
        let changed = smtp_routes::ProviderPlan::new(&rendering, &[changed_profile]).unwrap();
        let changed_request = scheduler::PollRequest {
            history_end: "1970-01-01T00:00:17",
            ..request
        };
        assert!(changed
            .poll_with(&changed_request, &mut restored, &mut store, 17, query)
            .unwrap()
            .is_empty());
        let changed_profile = smtp_provider_revision::Profile {
            port: decoy.local_addr().unwrap().port(),
            ..provider.profile()
        };
        let changed_profiles = [changed_profile];
        let mut cycles = polling_cycle::Cycles::new(1).unwrap();
        let mut times = [17, 17].into_iter();
        let tick = cycles.tick_mixed_bound_smtp(
            &cycle_settings,
            &maintenance,
            &mut store,
            polling_cycle::MixedBoundSmtp {
                ordinary_destinations: &["webhook"],
                smtp: polling_cycle::BoundSmtp {
                    rendering: polling_cycle::Rendering {
                        routes: &routes,
                        max_messages: 1,
                    },
                    profiles: &changed_profiles,
                    permissions: &permissions,
                    timeout: Duration::from_secs(3),
                },
            },
            polling_cycle::Hooks {
                query,
                send: (
                    |bound: &smtp_provider_revision::Descriptor| {
                        resolve_resources(
                            bound,
                            &changed_profiles[0],
                            &trust_path,
                            b"rotated-fixture",
                        )
                    },
                    |_: &storage::DueDelivery| -> Command { panic!("changed endpoint spawned") },
                    |_: &storage::DueDelivery| panic!("completed ordinary route replayed"),
                ),
                clock: || times.next(),
                cancelled: || false,
            },
        );
        assert!(matches!(
            tick.delivery,
            Some(Err(polling_cycle::StageError::DispatchBlocked))
        ));
        assert!(tick.cleanup.is_none() && tick.health.is_none());
        let failure = tick.delivery_failure.unwrap();
        assert_eq!(
            failure.diagnostic,
            Some(supervised_provider::Diagnostic::SmtpProviderResolution(
                smtp_provider_delivery::Error::Profile
            ))
        );
        assert_eq!(
            store.due_deliveries("cluster", 17, 10).unwrap()[0].attempt_number,
            2
        );
        assert!(decoy.accept().is_err());
        let mut cycles = polling_cycle::Cycles::new(1).unwrap();
        let mut times = [17, 17, 18, 18, 18].into_iter();
        let tick = cycles.tick_mixed_bound_smtp(
            &cycle_settings,
            &maintenance,
            &mut store,
            polling_cycle::MixedBoundSmtp {
                ordinary_destinations: &["webhook"],
                smtp: polling_cycle::BoundSmtp {
                    rendering: polling_cycle::Rendering {
                        routes: &routes,
                        max_messages: 1,
                    },
                    profiles: &profiles,
                    permissions: &permissions,
                    timeout: Duration::from_secs(3),
                },
            },
            polling_cycle::Hooks {
                query,
                send: (
                    |bound: &smtp_provider_revision::Descriptor| {
                        resolve_resources(bound, &profile, &trust_path, b"rotated-fixture")
                    },
                    |item: &storage::DueDelivery| {
                        assert_eq!(item.attempt_number, 2);
                        let mut command = Command::new(std::env::current_exe().unwrap());
                        command.arg("--smtp-bound-fixture");
                        command
                    },
                    |item: &storage::DueDelivery| {
                        assert_eq!(item.destination, "webhook");
                        Ok(delivery::ProviderOutcome::Delivered)
                    },
                ),
                clock: || times.next(),
                cancelled: || false,
            },
        );
        let report = tick.delivery.unwrap().unwrap();
        assert!(
            store
                .frozen_smtp(&item.event_id, "smtp")
                .unwrap()
                .unwrap()
                .data()
                == saved.data()
        );
        assert_eq!(report.delivered, 1);
        assert!(store.due_deliveries("cluster", 18, 10).unwrap().is_empty());
        assert!(decoy.accept().is_err());
        server.join().unwrap();
        drop(store);
        let db = rusqlite::Connection::open(root_path.join("state.sqlite3")).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM delivery_attempts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM smtp_provider_revisions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
    }
}

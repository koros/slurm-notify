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
    let input = transport::frame(
        &[
            endpoint.as_bytes(),
            b"Bearer fixture-placeholder",
            b"event-1",
            b"{}",
            b"10",
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
    println!("transport_boundary: HTTP acknowledgment, rate limit, rejection, slow HTTP, TLS rejection, blocked resolver, malformed frame passed");
}

//! Private transport experiment. Caller supplies bytes; no event wire contract.
use super::delivery::ProviderOutcome;
use std::time::{Duration, UNIX_EPOCH};
use url::{Host, Url};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    InvalidConfiguration,
    InvalidRequest,
}

// Deliberately no Debug: endpoint queries and authorization may contain secrets.
pub(super) struct Webhook {
    endpoint: String,
    authorization: Option<String>,
    max_payload: usize,
    agent: ureq::Agent,
}

impl Webhook {
    pub(super) fn new(
        endpoint: &str,
        authorization: Option<String>,
        timeout: Duration,
        max_payload: usize,
    ) -> Result<Self, Error> {
        let url = Url::parse(endpoint).map_err(|_| Error::InvalidConfiguration)?;
        let loopback = match url.host() {
            Some(Host::Ipv4(ip)) => ip.is_loopback(),
            Some(Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if url.host().is_none()
            || !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || timeout.is_zero()
            || max_payload == 0
            || authorization
                .as_ref()
                .is_some_and(|value| !header_value(value))
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(Self {
            endpoint: url.to_string(),
            authorization,
            max_payload,
            agent: ureq::AgentBuilder::new()
                .timeout(timeout)
                .timeout_connect(timeout)
                .timeout_read(timeout)
                .timeout_write(timeout)
                .redirects(0)
                .try_proxy_from_env(false)
                .build(),
        })
    }

    // Private acceptance fixture used only by the custom test child.
    pub(super) fn block_resolver_fixture(&mut self) {
        self.agent = ureq::AgentBuilder::new()
            .resolver(|_: &str| {
                std::thread::sleep(Duration::from_secs(30));
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "fixture resolver",
                ))
            })
            .try_proxy_from_env(false)
            .build();
    }

    pub(super) fn send(
        &self,
        event_id: &str,
        payload: &[u8],
        now: i64,
    ) -> Result<ProviderOutcome, Error> {
        if !header_value(event_id)
            || event_id.len() > 1024
            || payload.len() > self.max_payload
            || now < 0
        {
            return Err(Error::InvalidRequest);
        }
        let mut request = self
            .agent
            .post(&self.endpoint)
            .set("Content-Type", "application/json")
            .set("Idempotency-Key", event_id);
        if let Some(value) = &self.authorization {
            request = request.set("Authorization", value);
        }
        let response = match request.send_bytes(payload) {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(ureq::Error::Transport(_)) => return Ok(ProviderOutcome::Retry),
        };
        // Do not read, retain, or log response bodies or raw transport errors.
        Ok(classify(
            response.status(),
            response.header("Retry-After"),
            now,
        ))
    }
}

fn header_value(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && value.bytes().all(|b| (32..=126).contains(&b))
}

fn retry_delay(value: &str, now: i64) -> Option<i64> {
    let value = value.trim();
    let seconds = if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        value.parse::<i64>().ok()?
    } else {
        let date = httpdate::parse_http_date(value).ok()?;
        let epoch = i64::try_from(date.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()?;
        epoch.saturating_sub(now).max(0)
    };
    now.checked_add(seconds)?;
    Some(seconds)
}

fn classify(status: u16, retry_after: Option<&str>, now: i64) -> ProviderOutcome {
    match status {
        200..=299 => ProviderOutcome::Delivered,
        408 | 429 | 500..=599 => retry_after
            .and_then(|value| retry_delay(value, now))
            .map(ProviderOutcome::RetryAfter)
            .unwrap_or(ProviderOutcome::Retry),
        _ => ProviderOutcome::Rejected,
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    pub(crate) fn server(
        response: &'static str,
        delay: Duration,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/hook?test=value", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let handle = thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(start.elapsed() < Duration::from_secs(3));
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 1024];
                let n = stream.read(&mut buffer).unwrap();
                assert!(n > 0 && bytes.len() + n <= 8192);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            thread::sleep(delay);
            let _ = stream.write_all(response.as_bytes());
            String::from_utf8(bytes).unwrap()
        });
        (endpoint, handle)
    }

    // Certificates and private keys exist only for this local server's lifetime.
    pub(crate) fn tls_server(
        names: Vec<String>,
        expired: bool,
    ) -> (
        String,
        rustls::pki_types::CertificateDer<'static>,
        thread::JoinHandle<bool>,
    ) {
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        use std::sync::Arc;
        let mut params = rcgen::CertificateParams::new(names);
        if expired {
            params.not_before = rcgen::date_time_ymd(2000, 1, 1);
            params.not_after = rcgen::date_time_ymd(2001, 1, 1);
        }
        let certificate = rcgen::Certificate::from_params(params).unwrap();
        let der = rustls::pki_types::CertificateDer::from(certificate.serialize_der().unwrap());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            certificate.serialize_private_key_der(),
        ));
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![der.clone()], key)
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("https://{}/hook", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let started = std::time::Instant::now();
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(started.elapsed() < Duration::from_secs(3));
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, socket);
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 1024];
                match stream.read(&mut buffer) {
                    Ok(0) => return false,
                    Err(_) => return false,
                    Ok(n) => request.extend_from_slice(&buffer[..n]),
                }
                assert!(request.len() <= 8192);
                if request.windows(4).any(|v| v == b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(
                    b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            stream.conn.send_close_notify();
            stream.flush().unwrap();
            // Drain TLS control records until the client closes; avoid a TCP reset
            // discarding the acknowledgment because unread bytes remain.
            let mut drain = [0; 1024];
            while matches!(stream.read(&mut drain), Ok(n) if n > 0) {}
            true
        });
        (endpoint, der, handle)
    }

    fn trusted_agent(certificate: rustls::pki_types::CertificateDer<'static>) -> ureq::Agent {
        use std::sync::Arc;
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        ureq::AgentBuilder::new()
            .tls_config(Arc::new(config))
            .timeout(Duration::from_secs(1))
            .redirects(0)
            .try_proxy_from_env(false)
            .build()
    }

    #[test]
    fn synchronous_resolver_exceeds_socket_timeout() {
        let mut webhook = Webhook::new(
            "https://example.invalid/",
            None,
            Duration::from_millis(10),
            16,
        )
        .unwrap();
        webhook.agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(10))
            .resolver(|_: &str| {
                thread::sleep(Duration::from_millis(100));
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "fixture resolver",
                ))
            })
            .try_proxy_from_env(false)
            .build();
        let started = std::time::Instant::now();
        assert_eq!(
            webhook.send("event", b"", 10).unwrap(),
            ProviderOutcome::Retry
        );
        assert!(started.elapsed() >= Duration::from_millis(100));
    }

    #[test]
    fn https_accepts_trusted_certificate_with_matching_ip() {
        let (endpoint, certificate, handle) = tls_server(vec!["127.0.0.1".into()], false);
        let mut webhook = Webhook::new(&endpoint, None, Duration::from_secs(1), 16).unwrap();
        // Trust injection exists only in this fixture, never in the adapter API.
        webhook.agent = trusted_agent(certificate);
        let outcome = webhook.send("event", b"", 10).unwrap();
        assert!(handle.join().unwrap(), "server received no HTTP");
        assert_eq!(outcome, ProviderOutcome::Delivered);
    }

    #[test]
    fn https_default_roots_reject_unknown_issuer_before_http() {
        let (endpoint, _certificate, handle) = tls_server(vec!["127.0.0.1".into()], false);
        let webhook = Webhook::new(&endpoint, None, Duration::from_secs(1), 16).unwrap();
        assert_eq!(
            webhook.send("event", b"", 10).unwrap(),
            ProviderOutcome::Retry
        );
        assert!(!handle.join().unwrap());
    }

    #[test]
    fn https_rejects_wrong_hostname_even_with_trusted_issuer() {
        let (endpoint, certificate, handle) = tls_server(vec!["example.invalid".into()], false);
        let mut webhook = Webhook::new(&endpoint, None, Duration::from_secs(1), 16).unwrap();
        webhook.agent = trusted_agent(certificate);
        assert_eq!(
            webhook.send("event", b"", 10).unwrap(),
            ProviderOutcome::Retry
        );
        assert!(!handle.join().unwrap());
    }

    #[test]
    fn https_rejects_expired_certificate_even_with_trusted_issuer() {
        let (endpoint, certificate, handle) = tls_server(vec!["127.0.0.1".into()], true);
        let mut webhook = Webhook::new(&endpoint, None, Duration::from_secs(1), 16).unwrap();
        webhook.agent = trusted_agent(certificate);
        assert_eq!(
            webhook.send("event", b"", 10).unwrap(),
            ProviderOutcome::Retry
        );
        assert!(!handle.join().unwrap());
    }

    #[test]
    fn posts_exact_bytes_and_stable_id_without_reading_body() {
        let (endpoint, handle) = server(
            "HTTP/1.1 202 Accepted\r\nContent-Length: 999999999\r\nConnection: close\r\n\r\n",
            Duration::ZERO,
        );
        let webhook = Webhook::new(
            &endpoint,
            Some("Bearer fixture-placeholder".into()),
            Duration::from_secs(1),
            1024,
        )
        .unwrap();
        assert_eq!(
            webhook.send("event-1", b"{}", 10).unwrap(),
            ProviderOutcome::Delivered
        );
        let request = handle.join().unwrap();
        assert!(request.starts_with("POST /hook?test=value HTTP/1.1\r\n"));
        let lower = request.to_ascii_lowercase();
        assert!(lower.contains("idempotency-key: event-1\r\n"));
        assert!(lower.contains("authorization: bearer fixture-placeholder\r\n"));
        assert!(lower.contains("content-type: application/json\r\n"));
        assert!(request.ends_with("\r\n\r\n{}"));
    }

    #[test]
    fn status_and_retry_header_classification() {
        for status in [200, 202, 204, 299] {
            assert_eq!(classify(status, None, 10), ProviderOutcome::Delivered);
        }
        for status in [301, 302, 400, 401, 403, 404] {
            assert_eq!(classify(status, Some("60"), 10), ProviderOutcome::Rejected);
        }
        for status in [408, 429, 500, 503, 599] {
            assert_eq!(
                classify(status, Some("60"), 10),
                ProviderOutcome::RetryAfter(60)
            );
            for value in ["bad", "-1", "9223372036854775807"] {
                assert_eq!(classify(status, Some(value), 10), ProviderOutcome::Retry);
            }
        }
        assert_eq!(retry_delay("Thu, 01 Jan 1970 00:01:00 GMT", 10), Some(50));
        assert_eq!(retry_delay("Thu, 01 Jan 1970 00:00:00 GMT", 10), Some(0));
    }

    #[test]
    fn rate_limit_header_is_read_from_http_response() {
        let (endpoint, handle) = server(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 60\r\nContent-Length: 0\r\n\r\n",
            Duration::ZERO,
        );
        let webhook = Webhook::new(&endpoint, None, Duration::from_secs(1), 16).unwrap();
        assert_eq!(
            webhook.send("event", b"{}", 10).unwrap(),
            ProviderOutcome::RetryAfter(60)
        );
        handle.join().unwrap();
    }

    #[test]
    fn redirects_are_rejected_and_timeouts_retry() {
        let (endpoint, handle) = server(
            "HTTP/1.1 302 Found\r\nLocation: https://example.invalid/\r\nContent-Length: 0\r\n\r\n",
            Duration::ZERO,
        );
        let webhook = Webhook::new(&endpoint, None, Duration::from_secs(1), 16).unwrap();
        assert_eq!(
            webhook.send("event", b"{}", 10).unwrap(),
            ProviderOutcome::Rejected
        );
        handle.join().unwrap();
        let (endpoint, handle) = server(
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
            Duration::from_millis(200),
        );
        let webhook = Webhook::new(&endpoint, None, Duration::from_millis(50), 16).unwrap();
        assert_eq!(
            webhook.send("event", b"{}", 10).unwrap(),
            ProviderOutcome::Retry
        );
        handle.join().unwrap();
    }

    #[test]
    fn refused_connection_is_retryable_without_raw_diagnostics() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
        drop(listener);
        let webhook = Webhook::new(&endpoint, None, Duration::from_millis(100), 16).unwrap();
        assert_eq!(
            webhook.send("event", b"{}", 10).unwrap(),
            ProviderOutcome::Retry
        );
    }

    #[test]
    fn validates_configuration_and_request_before_network() {
        for endpoint in [
            "http://example.invalid/",
            "http://localhost/",
            "https://user:password@example.invalid/",
            "https://example.invalid/#fragment",
            "file:///tmp/test",
        ] {
            assert!(matches!(
                Webhook::new(endpoint, None, Duration::from_secs(1), 16),
                Err(Error::InvalidConfiguration)
            ));
        }
        assert!(matches!(
            Webhook::new(
                "https://example.invalid/",
                Some("value\r\nInjected: yes".into()),
                Duration::from_secs(1),
                16
            ),
            Err(Error::InvalidConfiguration)
        ));
        let webhook =
            Webhook::new("https://example.invalid/", None, Duration::from_secs(1), 1).unwrap();
        for (id, payload, now) in [
            ("event", &b"{}"[..], 10),
            ("bad\r\n", &b""[..], 10),
            ("event", &b""[..], -1),
        ] {
            assert_eq!(webhook.send(id, payload, now), Err(Error::InvalidRequest));
        }
    }
}

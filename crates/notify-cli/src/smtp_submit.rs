//! Private authenticated single-recipient submission; no installed transport.
use super::{
    delivery::ProviderOutcome,
    smtp_auth::Authenticated,
    smtp_message::Prepared,
    smtp_reply::{self, Limits},
};
use std::io::{Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Mail,
    Recipient,
    Data,
    FinalData,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Diagnostic {
    RelayAccepted,
    Temporary,
    Permanent,
    Io,
    Protocol,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Acceptance {
    RelayAccepted,
    NotSubmitted,
    NotAccepted,
    Unknown,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Report {
    pub(super) outcome: ProviderOutcome,
    phase: Phase,
    diagnostic: Diagnostic,
    acceptance: Acceptance,
}

// Callers can only supply a stream obtained after verified STARTTLS and AUTH.
pub(super) fn submit(mut stream: Authenticated, message: &Prepared) -> Report {
    transaction(&mut stream, message)
}

fn transaction(stream: &mut (impl Read + Write), message: &Prepared) -> Report {
    fn exchange(
        stream: &mut (impl Read + Write),
        bytes: &[u8],
        phase: Phase,
    ) -> Result<(), Report> {
        let uncertain = phase == Phase::FinalData;
        let failure = |diagnostic| Report {
            outcome: ProviderOutcome::Retry,
            phase,
            diagnostic,
            acceptance: if uncertain {
                Acceptance::Unknown
            } else {
                Acceptance::NotSubmitted
            },
        };
        stream
            .write_all(bytes)
            .and_then(|()| stream.flush())
            .map_err(|_| failure(Diagnostic::Io))?;
        let reply = smtp_reply::read_reply(
            stream,
            Limits {
                line_bytes: 128,
                total_bytes: 256,
                lines: 4,
            },
        )
        .map_err(|error| {
            failure(match error {
                smtp_reply::Error::Io | smtp_reply::Error::Truncated => Diagnostic::Io,
                _ => Diagnostic::Protocol,
            })
        })?;
        let accepted = match phase {
            Phase::Mail | Phase::FinalData => reply.code == 250,
            Phase::Recipient => matches!(reply.code, 250 | 251),
            Phase::Data => reply.code == 354,
        };
        if accepted {
            return Ok(());
        }
        let (outcome, diagnostic) = match reply.code {
            400..=499 => (ProviderOutcome::Retry, Diagnostic::Temporary),
            500..=599 => (ProviderOutcome::Rejected, Diagnostic::Permanent),
            _ => return Err(failure(Diagnostic::Protocol)),
        };
        Err(Report {
            outcome,
            phase,
            diagnostic,
            acceptance: if uncertain {
                Acceptance::NotAccepted
            } else {
                Acceptance::NotSubmitted
            },
        })
    }
    for (bytes, phase) in [
        (message.mail_command().into_bytes(), Phase::Mail),
        (message.recipient_command().into_bytes(), Phase::Recipient),
        (b"DATA\r\n".to_vec(), Phase::Data),
    ] {
        if let Err(report) = exchange(stream, &bytes, phase) {
            return report;
        }
    }
    if let Err(report) = exchange(stream, message.data(), Phase::FinalData) {
        return report;
    }
    // No QUIT reply is needed to preserve the already observed relay acceptance.
    let _ = stream.write_all(b"QUIT\r\n").and_then(|()| stream.flush());
    Report {
        outcome: ProviderOutcome::Delivered,
        phase: Phase::FinalData,
        diagnostic: Diagnostic::RelayAccepted,
        acceptance: Acceptance::RelayAccepted,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{smtp_auth, smtp_message};
    use super::*;
    use std::{
        io::Cursor,
        net::{TcpListener, TcpStream},
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };
    use zeroize::Zeroizing;
    fn message() -> Prepared {
        smtp_message::prepare(
            "s@fixture.invalid",
            "r@fixture.invalid",
            "event_1",
            "done",
            "done\n.\n.dot",
            0,
        )
        .unwrap()
    }
    struct Script {
        replies: Cursor<Vec<u8>>,
        sent: Zeroizing<Vec<u8>>,
        fail_quit: bool,
        fail_write: Option<usize>,
        writes: usize,
    }
    impl Read for Script {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.replies.read(bytes)
        }
    }
    impl Write for Script {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.writes += 1;
            if self.fail_write == Some(self.writes) {
                return Err(std::io::ErrorKind::BrokenPipe.into());
            }
            if self.fail_quit && bytes == b"QUIT\r\n" {
                return Err(std::io::ErrorKind::BrokenPipe.into());
            }
            self.sent.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn script(replies: &[u8]) -> Script {
        Script {
            replies: Cursor::new(replies.to_vec()),
            sent: Zeroizing::new(Vec::new()),
            fail_quit: false,
            fail_write: None,
            writes: 0,
        }
    }
    #[test]
    fn each_phase_stops_on_temporary_permanent_or_unexpected_reply() {
        let stages = [
            (Phase::Mail, b"".as_slice()),
            (Phase::Recipient, b"250 ok\r\n".as_slice()),
            (Phase::Data, b"250 ok\r\n251 forward\r\n".as_slice()),
            (
                Phase::FinalData,
                b"250 ok\r\n250 ok\r\n354 go\r\n".as_slice(),
            ),
        ];
        for (phase, prefix) in stages {
            for (reply, outcome, diagnostic) in [
                (
                    b"450 temporary\r\n".as_slice(),
                    ProviderOutcome::Retry,
                    Diagnostic::Temporary,
                ),
                (
                    b"550 permanent\r\n".as_slice(),
                    ProviderOutcome::Rejected,
                    Diagnostic::Permanent,
                ),
                (
                    b"235 unexpected\r\n".as_slice(),
                    ProviderOutcome::Retry,
                    Diagnostic::Protocol,
                ),
                (
                    b"malformed\r\n".as_slice(),
                    ProviderOutcome::Retry,
                    Diagnostic::Protocol,
                ),
                (
                    b"250 truncated".as_slice(),
                    ProviderOutcome::Retry,
                    Diagnostic::Io,
                ),
            ] {
                let mut replies = prefix.to_vec();
                replies.extend_from_slice(reply);
                let mut stream = script(&replies);
                let report = transaction(&mut stream, &message());
                assert_eq!(report.phase, phase);
                assert_eq!(report.outcome, outcome);
                assert_eq!(report.diagnostic, diagnostic);
                let acceptance = if phase != Phase::FinalData {
                    Acceptance::NotSubmitted
                } else if matches!(diagnostic, Diagnostic::Temporary | Diagnostic::Permanent) {
                    Acceptance::NotAccepted
                } else {
                    Acceptance::Unknown
                };
                assert_eq!(report.acceptance, acceptance);
                assert!(!stream.sent.ends_with(b"QUIT\r\n"));
                if phase != Phase::FinalData {
                    assert!(!stream.sent.windows(11).any(|b| b == b"Message-ID:"));
                }
            }
        }
    }
    #[test]
    fn only_final_data_acceptance_delivers_and_quit_failure_cannot_reverse_it() {
        for fail_quit in [false, true] {
            let mut stream = script(b"250 sender\r\n251 recipient\r\n354 data\r\n250 accepted\r\n");
            stream.fail_quit = fail_quit;
            let report = transaction(&mut stream, &message());
            assert_eq!(report.outcome, ProviderOutcome::Delivered);
            assert_eq!(report.acceptance, Acceptance::RelayAccepted);
            assert_eq!(report.diagnostic, Diagnostic::RelayAccepted);
            assert!(stream.sent.starts_with(
                b"MAIL FROM:<s@fixture.invalid>\r\nRCPT TO:<r@fixture.invalid>\r\nDATA\r\n"
            ));
            assert!(stream
                .sent
                .windows(b"..\r\n..dot\r\n.\r\n".len())
                .any(|b| b == b"..\r\n..dot\r\n.\r\n"));
        }
        let mut stream = script(b"250 sender\r\n250 recipient\r\n354 data\r\n");
        assert_eq!(
            transaction(&mut stream, &message()).acceptance,
            Acceptance::Unknown
        );
    }
    #[test]
    fn write_failures_before_and_during_data_preserve_acceptance_uncertainty() {
        for (write, phase) in [
            (1, Phase::Mail),
            (2, Phase::Recipient),
            (3, Phase::Data),
            (4, Phase::FinalData),
        ] {
            let mut stream = script(b"250 sender\r\n250 recipient\r\n354 data\r\n250 accepted\r\n");
            stream.fail_write = Some(write);
            let report = transaction(&mut stream, &message());
            assert_eq!(report.outcome, ProviderOutcome::Retry);
            assert_eq!(report.phase, phase);
            assert_eq!(report.diagnostic, Diagnostic::Io);
            assert_eq!(
                report.acceptance,
                if phase == Phase::FinalData {
                    Acceptance::Unknown
                } else {
                    Acceptance::NotSubmitted
                }
            );
            assert_eq!(stream.writes, write);
        }
    }
    #[test]
    fn real_verified_authenticated_tls_observes_one_envelope_and_one_data_message() {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let cert = rcgen::generate_simple_self_signed(vec!["relay.invalid".into()]).unwrap();
        let der = CertificateDer::from(cert.serialize_der().unwrap());
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![der.clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.serialize_private_key_der())),
            )
            .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(der).unwrap();
        let client = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        fn line(stream: &mut impl Read) -> Zeroizing<Vec<u8>> {
            let mut bytes = Zeroizing::new(Vec::with_capacity(1000));
            let mut byte = [0];
            while bytes.len() < 1000 && matches!(stream.read(&mut byte), Ok(1)) {
                bytes.push(byte[0]);
                if bytes.ends_with(b"\r\n") {
                    break;
                }
            }
            bytes
        }
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => panic!("submission fixture accept failed"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            socket.write_all(b"220 ready\r\n").unwrap();
            assert!(&*line(&mut socket) == b"EHLO fixture.invalid\r\n");
            socket.write_all(b"250-relay\r\n250 STARTTLS\r\n").unwrap();
            assert!(&*line(&mut socket) == b"STARTTLS\r\n");
            socket.write_all(b"220 upgrade\r\n").unwrap();
            let mut stream = rustls::StreamOwned::new(
                rustls::ServerConnection::new(Arc::new(config)).unwrap(),
                socket,
            );
            for (expected, reply) in [
                (
                    b"EHLO fixture.invalid\r\n".as_slice(),
                    b"250-secure\r\n250 AUTH PLAIN\r\n".as_slice(),
                ),
                (b"AUTH PLAIN AHUAcA==\r\n", b"235 ready\r\n"),
                (b"MAIL FROM:<s@fixture.invalid>\r\n", b"250 sender\r\n"),
                (b"RCPT TO:<r@fixture.invalid>\r\n", b"250 recipient\r\n"),
                (b"DATA\r\n", b"354 go\r\n"),
            ] {
                assert!(*line(&mut stream) == expected);
                stream
                    .write_all(reply)
                    .and_then(|()| stream.flush())
                    .unwrap();
            }
            let mut observed = Zeroizing::new(Vec::new());
            loop {
                let bytes = line(&mut stream);
                assert!(!bytes.is_empty());
                assert!(observed.len() + bytes.len() <= 16384);
                observed.extend_from_slice(&bytes);
                if &*bytes == b".\r\n" {
                    break;
                }
            }
            assert!(*observed == message().data());
            stream
                .write_all(b"250 accepted\r\n")
                .and_then(|()| stream.flush())
                .unwrap();
            assert!(&*line(&mut stream) == b"QUIT\r\n");
        });
        let stream = smtp_auth::connect_authenticated(
            smtp_auth::Credentials::new(Some("u"), Some("p")).unwrap(),
            || {
                let socket = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
                socket.set_read_timeout(Some(Duration::from_secs(1)))?;
                socket.set_write_timeout(Some(Duration::from_secs(1)))?;
                Ok(socket)
            },
            client,
            "relay.invalid".try_into().unwrap(),
        )
        .unwrap();
        assert_eq!(
            submit(stream, &message()).outcome,
            ProviderOutcome::Delivered
        );
        server.join().unwrap();
    }
}

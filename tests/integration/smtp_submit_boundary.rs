//! Harness-only bounded submission child; ephemeral test trust, loopback only.
use super::{
    smtp_auth, smtp_auth_boundary, smtp_auth_frame, smtp_submit,
    smtp_submit_frame::{self, Ack},
};
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::Arc,
    time::Duration,
};

pub(super) fn serve(input: impl Read, mut output: impl Write) -> std::io::Result<()> {
    let result = (|| {
        let request =
            smtp_submit_frame::decode_request(input).map_err(|_| smtp_auth_frame::Ack::Input)?;
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(request.root))
            .map_err(|_| smtp_auth_frame::Ack::Configuration)?;
        let config = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], request.port));
        let stream = smtp_auth::connect_authenticated(
            request.credentials,
            || {
                let socket = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
                socket.set_read_timeout(Some(Duration::from_secs(2)))?;
                socket.set_write_timeout(Some(Duration::from_secs(2)))?;
                Ok(socket)
            },
            config,
            "relay.invalid".try_into().unwrap(),
        )
        .map_err(smtp_auth_boundary::classify)?;
        Ok(smtp_submit::submit(stream, &request.message))
    })();
    let ack = match result {
        Ok(report) => Ack::Submission(report),
        Err(error) => Ack::Auth(error),
    };
    let bytes = smtp_submit_frame::encode_ack(ack)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    output.write_all(&bytes)?;
    output.flush()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn acceptance() {
    use super::{
        delivery::ProviderOutcome,
        smtp_message,
        smtp_submit::{Acceptance, Diagnostic, Phase, Report},
        transport,
    };
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::{net::TcpListener, process::Command, thread, time::Instant};
    use zeroize::Zeroizing;
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
    fn request(root: &[u8], port: &[u8], body: &[u8]) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(
            transport::frame(
                &[
                    root,
                    port,
                    b"u",
                    b"p",
                    b"s@fixture.invalid",
                    b"r@fixture.invalid",
                    b"event_1",
                    b"done",
                    body,
                    b"0",
                ],
                smtp_submit_frame::INPUT_LIMIT,
            )
            .unwrap(),
        )
    }
    fn run(input: Zeroizing<Vec<u8>>, mode: &str) -> Result<Ack, transport::Error> {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.arg(mode);
        transport::supervise_with(
            &mut command,
            input,
            Duration::from_millis(500),
            smtp_submit_frame::INPUT_LIMIT,
            smtp_submit_frame::OUTPUT_LIMIT,
            smtp_submit_frame::decode_ack,
        )
    }
    // Final acceptance/refusal/protocol, EOF, stall, drip, unexpected positive,
    // and early recipient/sender refusals over a verified authenticated TLS session.
    for mode in 0..10 {
        let cert = rcgen::generate_simple_self_signed(vec!["relay.invalid".into()]).unwrap();
        let root = cert.serialize_der().unwrap();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(root.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.serialize_private_key_der())),
            )
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
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
                .set_read_timeout(Some(Duration::from_secs(2)))
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
                let refusal = (mode == 9 && expected.starts_with(b"MAIL "))
                    || (mode == 8 && expected.starts_with(b"RCPT "));
                let reply = if refusal {
                    if mode == 9 {
                        b"450 temporary\r\n".as_slice()
                    } else {
                        b"550 rejected\r\n".as_slice()
                    }
                } else {
                    reply
                };
                stream
                    .write_all(reply)
                    .and_then(|()| stream.flush())
                    .unwrap();
                if refusal {
                    assert!(line(&mut stream).is_empty());
                    return 0;
                }
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
            let message = smtp_message::prepare(
                "s@fixture.invalid",
                "r@fixture.invalid",
                "event_1",
                "done",
                "done\n.\n.dot",
                0,
            )
            .unwrap();
            assert!(*observed == message.data());
            if mode == 4 {
                return 0;
            }
            if !matches!(mode, 5 | 6) {
                let reply: &[u8] = match mode {
                    0 => b"250 accepted\r\n",
                    1 => b"450 temporary\r\n",
                    2 => b"550 rejected\r\n",
                    3 => b"malformed\r\n",
                    _ => b"251 unexpected\r\n",
                };
                stream
                    .write_all(reply)
                    .and_then(|()| stream.flush())
                    .unwrap();
                if mode == 0 {
                    assert!(&*line(&mut stream) == b"QUIT\r\n");
                } else {
                    assert!(line(&mut stream).is_empty());
                }
                return 0;
            }
            let mut writes = 0;
            if mode == 6 {
                stream
                    .write_all(b"250 ")
                    .and_then(|()| stream.flush())
                    .unwrap();
                loop {
                    if stream
                        .write_all(b" ")
                        .and_then(|()| stream.flush())
                        .is_err()
                    {
                        break;
                    }
                    writes += 1;
                    thread::sleep(Duration::from_millis(20));
                    assert!(writes < 100);
                }
            }
            let mut byte = [0];
            match stream.read(&mut byte) {
                Ok(0) => (),
                Err(error) => assert!(matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                )),
                _ => panic!("unexpected command while awaiting final reply"),
            }
            writes
        });
        let start = Instant::now();
        let result = run(
            request(&root, port.as_bytes(), b"done\n.\n.dot"),
            "--smtp-submit-fixture",
        );
        if matches!(mode, 5 | 6) {
            assert_eq!(result, Err(transport::Error::Timeout));
        } else {
            let (outcome, phase, diagnostic, acceptance) = match mode {
                0 => (
                    ProviderOutcome::Delivered,
                    Phase::FinalData,
                    Diagnostic::RelayAccepted,
                    Acceptance::RelayAccepted,
                ),
                1 => (
                    ProviderOutcome::Retry,
                    Phase::FinalData,
                    Diagnostic::Temporary,
                    Acceptance::NotAccepted,
                ),
                2 => (
                    ProviderOutcome::Rejected,
                    Phase::FinalData,
                    Diagnostic::Permanent,
                    Acceptance::NotAccepted,
                ),
                4 => (
                    ProviderOutcome::Retry,
                    Phase::FinalData,
                    Diagnostic::Io,
                    Acceptance::Unknown,
                ),
                8 => (
                    ProviderOutcome::Rejected,
                    Phase::Recipient,
                    Diagnostic::Permanent,
                    Acceptance::NotSubmitted,
                ),
                9 => (
                    ProviderOutcome::Retry,
                    Phase::Mail,
                    Diagnostic::Temporary,
                    Acceptance::NotSubmitted,
                ),
                _ => (
                    ProviderOutcome::Retry,
                    Phase::FinalData,
                    Diagnostic::Protocol,
                    Acceptance::Unknown,
                ),
            };
            assert_eq!(
                result,
                Ok(Ack::Submission(Report {
                    outcome,
                    phase,
                    diagnostic,
                    acceptance
                }))
            );
        }
        assert!(start.elapsed() < Duration::from_millis(1500));
        let writes = server.join().unwrap();
        if mode == 6 {
            assert!(writes >= 2);
        }
    }
    // No-connect controls: all validation completes before attempting TCP.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let cert = rcgen::generate_simple_self_signed(vec!["relay.invalid".into()]).unwrap();
    let root = cert.serialize_der().unwrap();
    for (root, body, expected) in [
        (
            &root[..],
            b"invalid\0".as_slice(),
            smtp_auth_frame::Ack::Input,
        ),
        (
            &b"invalid-der"[..],
            b"done",
            smtp_auth_frame::Ack::Configuration,
        ),
    ] {
        assert_eq!(
            run(
                request(root, port.as_bytes(), body),
                "--smtp-submit-fixture"
            ),
            Ok(Ack::Auth(expected))
        );
        assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    }
    let mut extra = request(&root, port.as_bytes(), b"done");
    extra.push(0);
    assert_eq!(
        run(extra, "--smtp-submit-fixture"),
        Ok(Ack::Auth(smtp_auth_frame::Ack::Input))
    );
    assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    for (mode, expected) in [
        ("--smtp-submit-wrong-ack", transport::Error::InvalidResult),
        ("--smtp-submit-failed-ack", transport::Error::ChildExit),
    ] {
        assert_eq!(run(Zeroizing::new(Vec::new()), mode), Err(expected));
    }
}

//! Harness-only authentication child. Explicit ephemeral test trust, loopback only.
use super::{smtp_auth, smtp_auth_frame, smtp_auth_frame::Ack, smtp_tls};
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::Arc,
    time::Duration,
};

const ROOT_LIMIT: usize = 4096;
const INPUT_LIMIT: usize = 4 + ROOT_LIMIT + smtp_auth_frame::LIMIT;

pub(super) fn serve(mut input: impl Read, mut output: impl Write) -> std::io::Result<()> {
    let result = (|| {
        // Separate test envelope, not an operational trust override. Parse all
        // input before connecting. Certificate verification remains enabled.
        let mut prefix = [0; 4];
        input.read_exact(&mut prefix).map_err(|_| Ack::Input)?;
        let size = u32::from_be_bytes(prefix) as usize;
        if size == 0 || size > ROOT_LIMIT {
            return Err(Ack::Input);
        }
        let mut root = vec![0; size];
        input.read_exact(&mut root).map_err(|_| Ack::Input)?;
        let request = smtp_auth_frame::decode(input).map_err(|_| Ack::Input)?;
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(root))
            .map_err(|_| Ack::Configuration)?;
        let config = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], request.port));
        smtp_auth::authenticate_credentials(
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
        .map_err(classify)
    })();
    output.write_all(result.err().unwrap_or(Ack::Authenticated).bytes())?;
    output.flush()
}

fn classify(error: smtp_auth::Error) -> Ack {
    match error {
        smtp_auth::Error::Credentials => Ack::Credentials,
        smtp_auth::Error::Unsupported => Ack::Unsupported,
        smtp_auth::Error::Temporary => Ack::Temporary,
        smtp_auth::Error::Permanent => Ack::Permanent,
        smtp_auth::Error::Reply => Ack::Reply,
        smtp_auth::Error::Io => Ack::Io,
        smtp_auth::Error::Transport(error) => match error {
            smtp_tls::Error::Certificate => Ack::Certificate,
            smtp_tls::Error::TlsConfiguration => Ack::Configuration,
            smtp_tls::Error::TlsProtocol => Ack::Protocol,
            smtp_tls::Error::HandshakeIo | smtp_tls::Error::Io => Ack::Io,
            smtp_tls::Error::Reply => Ack::Reply,
            smtp_tls::Error::UpgradeMissing => Ack::UpgradeMissing,
            smtp_tls::Error::UpgradeTemporary => Ack::UpgradeTemporary,
            smtp_tls::Error::UpgradePermanent => Ack::UpgradePermanent,
        },
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn acceptance() {
    use super::transport;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::{net::TcpListener, process::Command, thread, time::Instant};
    use zeroize::Zeroizing;
    fn line(stream: &mut impl Read) -> Zeroizing<Vec<u8>> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(273));
        let mut byte = [0];
        while bytes.len() < 273 && matches!(stream.read(&mut byte), Ok(1)) {
            bytes.push(byte[0]);
            if bytes.ends_with(b"\r\n") {
                break;
            }
        }
        bytes
    }
    // Explicit success/refusals, malformed reply, disconnect, stall, drip.
    for mode in 0..7 {
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
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => panic!("AUTH fixture accept failed"),
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
            assert!(&*line(&mut stream) == b"EHLO fixture.invalid\r\n");
            stream
                .write_all(b"250-secure\r\n250 AUTH PLAIN\r\n")
                .and_then(|()| stream.flush())
                .unwrap();
            assert!(&*line(&mut stream) == b"AUTH PLAIN AHUAcA==\r\n"); // Synthetic u/p only; boolean assertion.
            if mode == 4 {
                return 0;
            } // Abrupt EOF after exactly one AUTH.
            if mode < 4 {
                let reply: &[u8] = match mode {
                    0 => b"235 authenticated\r\n",
                    1 => b"454 temporary\r\n",
                    2 => b"535 rejected\r\n",
                    _ => b"garbage\r\n",
                };
                stream
                    .write_all(reply)
                    .and_then(|()| stream.flush())
                    .unwrap();
                assert!(line(&mut stream).is_empty()); // No MAIL or repeated AUTH.
                return 0;
            }
            let mut writes = 0;
            if mode == 6 {
                stream
                    .write_all(b"235 ")
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
                    assert!(writes < 100); // Fixture cannot run forever.
                }
            }
            // Parent termination closes the TLS socket; timeout would fail here.
            let mut byte = [0];
            match stream.read(&mut byte) {
                Ok(0) => (),
                Err(error) => assert!(matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                )),
                _ => panic!("unexpected command after AUTH"),
            }
            writes
        });
        let frame = smtp_auth_frame::encode(port, "u", "p").unwrap();
        let mut input = Zeroizing::new(Vec::with_capacity(4 + root.len() + frame.len()));
        input.extend_from_slice(&(root.len() as u32).to_be_bytes());
        input.extend_from_slice(&root);
        input.extend_from_slice(&frame);
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.arg("--smtp-auth-fixture");
        let start = Instant::now();
        let result = transport::supervise_with(
            &mut command,
            input,
            Duration::from_millis(500),
            INPUT_LIMIT,
            32,
            smtp_auth_frame::decode_ack,
        );
        let expected = match mode {
            0 => Ok(Ack::Authenticated),
            1 => Ok(Ack::Temporary),
            2 => Ok(Ack::Permanent),
            3 => Ok(Ack::Reply),
            4 => Ok(Ack::Io),
            _ => Err(transport::Error::Timeout),
        };
        assert_eq!(result, expected);
        assert!(start.elapsed() < Duration::from_millis(1500));
        let writes = server.join().unwrap();
        if mode == 6 {
            assert!(writes >= 2);
        }
    }
    for input in [
        vec![],
        0u32.to_be_bytes().to_vec(),
        4097u32.to_be_bytes().to_vec(),
    ] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.arg("--smtp-auth-fixture");
        assert_eq!(
            transport::supervise_with(
                &mut command,
                Zeroizing::new(input),
                Duration::from_secs(1),
                INPUT_LIMIT,
                32,
                smtp_auth_frame::decode_ack
            ),
            Ok(Ack::Input)
        );
    }
    // Valid fixture trust plus malformed credentials/frame must never connect;
    // invalid trust is configuration evidence, never a bypass of verification.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cert = rcgen::generate_simple_self_signed(vec!["relay.invalid".into()]).unwrap();
    let root = cert.serialize_der().unwrap();
    let frame = smtp_auth_frame::encode(listener.local_addr().unwrap().port(), "u", "p").unwrap();
    for (root, trailing, expected) in [
        (&root[..], true, Ack::Input),
        (&b"invalid-der"[..], false, Ack::Configuration),
    ] {
        let mut input = Zeroizing::new(Vec::new());
        input.extend_from_slice(&(root.len() as u32).to_be_bytes());
        input.extend_from_slice(root);
        input.extend_from_slice(&frame);
        if trailing {
            input.push(0);
        }
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.arg("--smtp-auth-fixture");
        assert_eq!(
            transport::supervise_with(
                &mut command,
                input,
                Duration::from_secs(1),
                INPUT_LIMIT,
                32,
                smtp_auth_frame::decode_ack
            ),
            Ok(expected)
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }
}

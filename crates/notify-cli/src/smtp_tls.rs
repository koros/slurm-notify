//! Private implicit-TLS and required-STARTTLS experiments; no AUTH or submission.
use super::smtp_reply::{read_reply, read_reply_with_text, Limits};
use std::{io::Write, net::TcpStream, sync::Arc};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Certificate,
    TlsConfiguration,
    TlsProtocol,
    HandshakeIo,
    Io,
    Reply,
    UpgradeMissing,
    UpgradeTemporary,
    UpgradePermanent,
}

// Inspect typed causes only; never retain or stringify remote diagnostics.
fn handshake_error(error: std::io::Error) -> Error {
    match error
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<rustls::Error>())
    {
        Some(rustls::Error::InvalidCertificate(_) | rustls::Error::NoCertificatesPresented) => {
            Error::Certificate
        }
        Some(_) => Error::TlsProtocol,
        None => Error::HandshakeIo,
    }
}

// Socket deadlines must already be set. Parent supervision is still required
// for an absolute deadline; repeated successful reads can extend socket waits.
fn greeting(
    socket: TcpStream,
    config: Arc<rustls::ClientConfig>,
    name: rustls::pki_types::ServerName<'static>,
) -> Result<(), Error> {
    let mut connection =
        rustls::ClientConnection::new(config, name).map_err(|_| Error::TlsConfiguration)?;
    let mut socket = socket;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut socket)
            .map_err(handshake_error)?;
    }
    // No SMTP bytes are written until certificate verification completes.
    let mut stream = rustls::StreamOwned::new(connection, socket);
    let limits = || Limits {
        line_bytes: 128,
        total_bytes: 256,
        lines: 4,
    };
    if read_reply(&mut stream, limits())
        .map_err(|_| Error::Reply)?
        .code
        != 220
    {
        return Err(Error::Reply);
    }
    stream
        .write_all(b"EHLO fixture.invalid\r\n")
        .and_then(|()| stream.flush())
        .map_err(|_| Error::Io)?;
    if read_reply(&mut stream, limits())
        .map_err(|_| Error::Reply)?
        .code
        != 250
    {
        return Err(Error::Reply);
    }
    stream.conn.send_close_notify();
    stream.flush().map_err(|_| Error::Io)?;
    Ok(())
}

pub(super) fn required_starttls(
    mut socket: TcpStream,
    config: Arc<rustls::ClientConfig>,
    name: rustls::pki_types::ServerName<'static>,
) -> Result<(), Error> {
    let limits = || Limits {
        line_bytes: 128,
        total_bytes: 256,
        lines: 4,
    };
    if read_reply(&mut socket, limits())
        .map_err(|_| Error::Reply)?
        .code
        != 220
    {
        return Err(Error::Reply);
    }
    socket
        .write_all(b"EHLO fixture.invalid\r\n")
        .map_err(|_| Error::Io)?;
    let mut advertised = false;
    let reply = read_reply_with_text(&mut socket, limits(), |number, text| {
        // EHLO's first line identifies the server; subsequent lines are extensions.
        if number > 1 && text.eq_ignore_ascii_case(b"STARTTLS") {
            advertised = true;
        }
    })
    .map_err(|_| Error::Reply)?;
    if reply.code != 250 {
        return Err(Error::Reply);
    }
    if !advertised {
        return Err(Error::UpgradeMissing);
    }
    socket.write_all(b"STARTTLS\r\n").map_err(|_| Error::Io)?;
    match read_reply(&mut socket, limits())
        .map_err(|_| Error::Reply)?
        .code
    {
        220 => {}
        400..=499 => return Err(Error::UpgradeTemporary),
        500..=599 => return Err(Error::UpgradePermanent),
        _ => return Err(Error::Reply),
    }
    let mut connection =
        rustls::ClientConnection::new(config, name).map_err(|_| Error::TlsConfiguration)?;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut socket)
            .map_err(handshake_error)?;
    }
    let mut stream = rustls::StreamOwned::new(connection, socket);
    // No capabilities survive the plaintext EHLO. Obtain a fresh verified reply.
    stream
        .write_all(b"EHLO fixture.invalid\r\n")
        .and_then(|()| stream.flush())
        .map_err(|_| Error::Io)?;
    if read_reply(&mut stream, limits())
        .map_err(|_| Error::Reply)?
        .code
        != 250
    {
        return Err(Error::Reply);
    }
    stream.conn.send_close_notify();
    stream.flush().map_err(|_| Error::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::{
        io::Read,
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    // Reports only a byte count and exact EHLO match, never raw command content.
    struct Observation {
        bytes: usize,
        ehlo_only: bool,
    }

    fn fixture(
        names: Vec<String>,
        expired: bool,
        trust: bool,
        greeting_bytes: &'static [u8],
        ehlo_reply: &'static [u8],
    ) -> (Result<(), Error>, Observation) {
        let mut params = rcgen::CertificateParams::new(names);
        if expired {
            params.not_before = rcgen::date_time_ymd(2000, 1, 1);
            params.not_after = rcgen::date_time_ymd(2001, 1, 1);
        }
        let certificate = rcgen::Certificate::from_params(params).unwrap();
        let der = CertificateDer::from(certificate.serialize_der().unwrap());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            certificate.serialize_private_key_der(),
        ));
        let server = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![der.clone()], key)
            .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        if trust {
            roots.add(der).unwrap();
        }
        let client = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => panic!("fixture accept failed"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let connection = rustls::ServerConnection::new(Arc::new(server)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, socket);
            // TLS rejection may fail this write or the following read.
            let _ = stream
                .write_all(greeting_bytes)
                .and_then(|()| stream.flush());
            let mut observed = Vec::new();
            let mut byte = [0];
            while observed.len() < 64 {
                match stream.read(&mut byte) {
                    Ok(1) => observed.push(byte[0]),
                    _ => break,
                }
                if observed.ends_with(b"\r\n") {
                    let _ = stream.write_all(ehlo_reply).and_then(|()| stream.flush());
                    break;
                }
            }
            // Observe any unexpected AUTH/envelope bytes after EHLO as well.
            while observed.len() < 64 && matches!(stream.read(&mut byte), Ok(1)) {
                observed.push(byte[0]);
            }
            Observation {
                bytes: observed.len(),
                ehlo_only: observed == b"EHLO fixture.invalid\r\n",
            }
        });
        let socket = TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let result = greeting(
            socket,
            Arc::new(client),
            "relay.invalid".try_into().unwrap(),
        );
        (result, handle.join().unwrap())
    }

    #[test]
    fn trusted_tls_exchanges_only_greeting_and_multiline_ehlo() {
        let (result, observation) = fixture(
            vec!["relay.invalid".into()],
            false,
            true,
            b"220 fixture ready\r\n",
            b"250-fixture\r\n250 AUTH PLAIN\r\n",
        );
        assert_eq!(result, Ok(()));
        assert!(observation.ehlo_only);
    }

    #[test]
    fn untrusted_wrong_name_and_expired_tls_send_no_smtp_commands() {
        for (names, expired, trust) in [
            (vec!["relay.invalid".into()], false, false),
            (vec!["other.invalid".into()], false, true),
            (vec!["relay.invalid".into()], true, true),
        ] {
            let (result, observation) = fixture(
                names,
                expired,
                trust,
                b"220 fixture ready\r\n",
                b"250 fixture\r\n",
            );
            assert_eq!(result, Err(Error::Certificate));
            assert_eq!(observation.bytes, 0);
        }
    }

    #[test]
    fn invalid_greeting_stops_before_ehlo_and_bad_ehlo_stops_before_auth() {
        for bytes in [&b"550 unavailable\r\n"[..], &b"220 malformed\n"[..]] {
            let (result, observation) = fixture(
                vec!["relay.invalid".into()],
                false,
                true,
                bytes,
                b"250 fixture\r\n",
            );
            assert_eq!(result, Err(Error::Reply));
            assert_eq!(observation.bytes, 0);
        }
        let (result, observation) = fixture(
            vec!["relay.invalid".into()],
            false,
            true,
            b"220 ready\r\n",
            b"250-first\r\n550 inconsistent\r\n",
        );
        assert_eq!(result, Err(Error::Reply));
        assert!(observation.ehlo_only);
    }
    fn starttls_fixture(
        capabilities: &'static [u8],
        upgrade: &'static [u8],
        certificate_name: &str,
        expired: bool,
        trust: bool,
        secure_reply: &'static [u8],
    ) -> (Result<(), Error>, usize, usize) {
        let mut params = rcgen::CertificateParams::new(vec![certificate_name.into()]);
        if expired {
            params.not_before = rcgen::date_time_ymd(2000, 1, 1);
            params.not_after = rcgen::date_time_ymd(2001, 1, 1);
        }
        let cert = rcgen::Certificate::from_params(params).unwrap();
        let der = CertificateDer::from(cert.serialize_der().unwrap());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.serialize_private_key_der()));
        let server = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![der.clone()], key)
            .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        if trust {
            roots.add(der).unwrap();
        }
        let client = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        fn command(stream: &mut impl Read) -> Vec<u8> {
            let mut bytes = Vec::new();
            let mut byte = [0];
            while bytes.len() < 64 && matches!(stream.read(&mut byte), Ok(1)) {
                bytes.push(byte[0]);
                if bytes.ends_with(b"\r\n") {
                    break;
                }
            }
            bytes
        }
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => panic!("fixture accept failed"),
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
            assert!(command(&mut socket) == b"EHLO fixture.invalid\r\n");
            socket.write_all(capabilities).unwrap();
            let next = command(&mut socket);
            if next.is_empty() {
                return (1, 0);
            }
            assert!(next == b"STARTTLS\r\n");
            socket.write_all(upgrade).unwrap();
            if upgrade != b"220 upgrade\r\n" {
                assert!(command(&mut socket).is_empty());
                return (2, 0);
            }
            let connection = rustls::ServerConnection::new(Arc::new(server)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, socket);
            let secure = command(&mut stream);
            if secure.is_empty() {
                return (2, 0);
            }
            assert!(secure == b"EHLO fixture.invalid\r\n");
            stream
                .write_all(secure_reply)
                .and_then(|()| stream.flush())
                .unwrap();
            assert!(command(&mut stream).is_empty());
            (2, 1)
        });
        let socket = TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let result = required_starttls(
            socket,
            Arc::new(client),
            "relay.invalid".try_into().unwrap(),
        );
        let (plain, secure) = handle.join().unwrap();
        (result, plain, secure)
    }

    #[test]
    fn required_upgrade_rejects_absent_misleading_and_refused_starttls() {
        for capabilities in [
            &b"250-relay\r\n250 AUTH PLAIN\r\n"[..],
            &b"250 STARTTLS\r\n"[..],
            &b"250-relay\r\n250 XSTARTTLS\r\n"[..],
            &b"250-relay\r\n250 STARTTLS extra\r\n"[..],
        ] {
            assert_eq!(
                starttls_fixture(
                    capabilities,
                    b"220 upgrade\r\n",
                    "relay.invalid",
                    false,
                    true,
                    b"250 ready\r\n"
                ),
                (Err(Error::UpgradeMissing), 1, 0)
            );
        }
        for (refusal, expected) in [
            (&b"454 unavailable\r\n"[..], Error::UpgradeTemporary),
            (&b"550 refused\r\n"[..], Error::UpgradePermanent),
        ] {
            assert_eq!(
                starttls_fixture(
                    b"250-relay\r\n250 STARTTLS\r\n",
                    refusal,
                    "relay.invalid",
                    false,
                    true,
                    b"250 ready\r\n"
                ),
                (Err(expected), 2, 0)
            );
        }
    }

    #[test]
    fn upgrade_verifies_certificate_before_fresh_ehlo() {
        for (name, expired, trust, expected) in [
            ("relay.invalid", false, true, Ok(())),
            ("relay.invalid", false, false, Err(Error::Certificate)),
            ("other.invalid", false, true, Err(Error::Certificate)),
            ("relay.invalid", true, true, Err(Error::Certificate)),
        ] {
            let (result, plain, secure) = starttls_fixture(
                b"250-relay\r\n250 starttls\r\n",
                b"220 upgrade\r\n",
                name,
                expired,
                trust,
                b"250-secure\r\n250 AUTH PLAIN\r\n",
            );
            assert_eq!(secure, usize::from(expected.is_ok()));
            assert_eq!(result, expected);
            assert_eq!(plain, 2);
        }
    }

    #[test]
    fn failed_secure_ehlo_does_not_fall_back_to_plaintext_capabilities() {
        assert_eq!(
            starttls_fixture(
                b"250-relay\r\n250-AUTH PLAIN\r\n250 STARTTLS\r\n",
                b"220 upgrade\r\n",
                "relay.invalid",
                false,
                true,
                b"550 unavailable\r\n"
            ),
            (Err(Error::Reply), 2, 1)
        );
    }
    #[test]
    fn handshake_categories_use_typed_causes_without_retaining_details() {
        for (cause, expected) in [
            (
                rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer),
                Error::Certificate,
            ),
            (rustls::Error::NoCertificatesPresented, Error::Certificate),
            (
                rustls::Error::General("private-fixture-detail".into()),
                Error::TlsProtocol,
            ),
        ] {
            let error =
                handshake_error(std::io::Error::new(std::io::ErrorKind::InvalidData, cause));
            assert_eq!(error, expected);
            assert!(!format!("{error:?}").contains("private-fixture-detail"));
        }
        for kind in [
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::UnexpectedEof,
            std::io::ErrorKind::ConnectionReset,
        ] {
            assert_eq!(
                handshake_error(std::io::Error::new(kind, "private-fixture-detail")),
                Error::HandshakeIo
            );
        }
    }
}

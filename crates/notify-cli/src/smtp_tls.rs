//! Private implicit-TLS greeting experiment, with no submission or AUTH path.
use super::smtp_reply::{read_reply, Limits};
use std::{io::Write, net::TcpStream, sync::Arc};

#[derive(Debug, PartialEq, Eq)]
enum Error {
    Tls,
    Io,
    Reply,
}

// Socket deadlines must already be set. Parent supervision is still required
// for an absolute deadline; repeated successful reads can extend socket waits.
fn greeting(
    socket: TcpStream,
    config: Arc<rustls::ClientConfig>,
    name: rustls::pki_types::ServerName<'static>,
) -> Result<(), Error> {
    let mut connection = rustls::ClientConnection::new(config, name).map_err(|_| Error::Tls)?;
    let mut socket = socket;
    while connection.is_handshaking() {
        connection
            .complete_io(&mut socket)
            .map_err(|_| Error::Tls)?;
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
            assert_eq!(result, Err(Error::Tls));
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
}

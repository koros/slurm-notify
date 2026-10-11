//! Private bounded AUTH PLAIN experiment; no message or submission outcome.
use super::{
    smtp_reply::{read_reply, read_reply_with_text, Limits},
    smtp_tls,
};
use base64::Engine;
use std::{io::Write, net::TcpStream, sync::Arc};
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Credentials,
    Transport(smtp_tls::Error),
    Unsupported,
    Temporary,
    Permanent,
    Reply,
    Io,
}

// Intentionally no Debug/Clone implementation or credential accessors.
pub(super) struct Credentials {
    plain: Zeroizing<Vec<u8>>,
}
impl Credentials {
    pub(super) fn new(username: Option<&str>, password: Option<&str>) -> Result<Self, Error> {
        fn valid(text: Option<&str>, bound: usize) -> Option<&str> {
            text.filter(|value| {
                !value.is_empty()
                    && value.len() <= bound
                    && value.bytes().all(|byte| (32..=126).contains(&byte))
            })
        }
        let username = valid(username, 64).ok_or(Error::Credentials)?;
        let password = valid(password, 128).ok_or(Error::Credentials)?;
        let mut plain = Zeroizing::new(Vec::with_capacity(username.len() + password.len() + 2));
        plain.push(0); // Empty authorization identity, explicit authentication identity.
        plain.extend_from_slice(username.as_bytes());
        plain.push(0);
        plain.extend_from_slice(password.as_bytes());
        Ok(Self { plain })
    }
    fn command(&self) -> Zeroizing<Vec<u8>> {
        // Maximum payload 194 bytes gives a bounded 260-byte encoded value.
        let size = base64::encoded_len(self.plain.len(), true).unwrap();
        let mut command = Zeroizing::new(vec![0; 11 + size + 2]);
        command[..11].copy_from_slice(b"AUTH PLAIN ");
        base64::engine::general_purpose::STANDARD
            .encode_slice(&self.plain, &mut command[11..11 + size])
            .unwrap();
        command[11 + size..].copy_from_slice(b"\r\n");
        command
    }
}

// Validate before calling the injected connector. Credential sources and parent
// deadlines are caller responsibilities; no environment lookup or plaintext mode.
fn authenticate(
    username: Option<&str>,
    password: Option<&str>,
    connect: impl FnOnce() -> std::io::Result<TcpStream>,
    config: Arc<rustls::ClientConfig>,
    name: rustls::pki_types::ServerName<'static>,
) -> Result<(), Error> {
    let credentials = Credentials::new(username, password)?;
    let mut stream = smtp_tls::verified_starttls(connect().map_err(|_| Error::Io)?, config, name)
        .map_err(Error::Transport)?;
    stream
        .write_all(b"EHLO fixture.invalid\r\n")
        .and_then(|()| stream.flush())
        .map_err(|_| Error::Io)?;
    let limits = || Limits {
        line_bytes: 128,
        total_bytes: 256,
        lines: 4,
    };
    let mut plain = false;
    let reply = read_reply_with_text(&mut stream, limits(), |number, text| {
        let mut words = text
            .split(|byte| *byte == b' ' || *byte == b'\t')
            .filter(|word| !word.is_empty());
        if number > 1
            && words
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case(b"AUTH"))
        {
            plain |= words.any(|word| word.eq_ignore_ascii_case(b"PLAIN"));
        }
    })
    .map_err(|_| Error::Reply)?;
    if reply.code != 250 {
        return Err(Error::Reply);
    }
    if !plain {
        return Err(Error::Unsupported);
    }
    let command = credentials.command();
    stream
        .write_all(&command)
        .and_then(|()| stream.flush())
        .map_err(|_| Error::Io)?;
    drop(command);
    drop(credentials);
    let result = match read_reply(&mut stream, limits())
        .map_err(|_| Error::Reply)?
        .code
    {
        235 => Ok(()),
        400..=499 => Err(Error::Temporary),
        500..=599 => Err(Error::Permanent),
        _ => Err(Error::Reply), // No challenge, fallback mechanism, or internal retry.
    };
    stream.conn.send_close_notify();
    // Teardown must not replace an explicit authentication result.
    let _ = stream.flush();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Read,
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    fn client_config() -> Arc<rustls::ClientConfig> {
        Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth(),
        )
    }
    #[test]
    fn invalid_credentials_never_call_connector_and_errors_are_static() {
        for (username, password) in [
            (None, Some("fixture")),
            (Some("fixture"), None),
            (Some(""), Some("fixture")),
            (Some("fixture"), Some("")),
            (Some("bad\r\n"), Some("fixture")),
            (Some("fixture"), Some("bad\0")),
            (Some("non-ascii-ä"), Some("fixture")),
        ] {
            assert_eq!(
                authenticate(
                    username,
                    password,
                    || panic!("invalid credentials connected"),
                    client_config(),
                    "relay.invalid".try_into().unwrap()
                ),
                Err(Error::Credentials)
            );
        }
        for (u, p) in [
            ("x".repeat(65), "y".repeat(128)),
            ("x".repeat(64), "y".repeat(129)),
        ] {
            assert!(matches!(
                Credentials::new(Some(&u), Some(&p)),
                Err(Error::Credentials)
            ));
        }
        let valid = Credentials::new(Some(&"x".repeat(64)), Some(&"y".repeat(128))).unwrap();
        assert_eq!(valid.command().len(), 273);
        assert_eq!(
            &*Credentials::new(Some("u"), Some("p")).unwrap().command(),
            b"AUTH PLAIN AHUAcA==\r\n"
        );
        assert!(!format!("{:?}", Error::Credentials).contains("fixture"));
    }

    fn fixture(
        trust: bool,
        capabilities: &'static [u8],
        response: &'static [u8],
    ) -> (Result<(), Error>, usize) {
        fixture_certificate(trust, "relay.invalid", false, capabilities, response)
    }

    fn fixture_certificate(
        trust: bool,
        name: &str,
        expired: bool,
        capabilities: &'static [u8],
        response: &'static [u8],
    ) -> (Result<(), Error>, usize) {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let mut params = rcgen::CertificateParams::new(vec![name.into()]);
        if expired {
            params.not_before = rcgen::date_time_ymd(2000, 1, 1);
            params.not_after = rcgen::date_time_ymd(2001, 1, 1);
        }
        let certificate = rcgen::Certificate::from_params(params).unwrap();
        let der = CertificateDer::from(certificate.serialize_der().unwrap());
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![der.clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                    certificate.serialize_private_key_der(),
                )),
            )
            .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        if trust {
            roots.add(der).unwrap();
        }
        let client = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        fn command(stream: &mut impl Read) -> Zeroizing<Vec<u8>> {
            let mut bytes = Zeroizing::new(Vec::with_capacity(512));
            let mut byte = [0];
            while bytes.len() < 512 && matches!(stream.read(&mut byte), Ok(1)) {
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
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => panic!("AUTH fixture accept failed"),
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
            assert!(&*command(&mut socket) == b"EHLO fixture.invalid\r\n");
            // Pre-TLS PLAIN advertising must never authorize secure AUTH.
            socket
                .write_all(b"250-relay\r\n250-AUTH PLAIN\r\n250 STARTTLS\r\n")
                .unwrap();
            assert!(&*command(&mut socket) == b"STARTTLS\r\n");
            socket.write_all(b"220 upgrade\r\n").unwrap();
            let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, socket);
            let ehlo = command(&mut stream);
            if ehlo.is_empty() {
                return 0;
            }
            assert!(&*ehlo == b"EHLO fixture.invalid\r\n");
            stream
                .write_all(capabilities)
                .and_then(|()| stream.flush())
                .unwrap();
            let auth = command(&mut stream);
            if auth.is_empty() {
                return 0;
            }
            let expected = Credentials::new(Some("fixture-user"), Some("fixture-placeholder"))
                .unwrap()
                .command();
            assert!(*auth == *expected); // Boolean assertion never prints credential bytes.
            stream
                .write_all(response)
                .and_then(|()| stream.flush())
                .unwrap();
            assert!(command(&mut stream).is_empty());
            1
        });
        let result = authenticate(
            Some("fixture-user"),
            Some("fixture-placeholder"),
            || {
                let socket = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
                socket.set_read_timeout(Some(Duration::from_secs(1)))?;
                socket.set_write_timeout(Some(Duration::from_secs(1)))?;
                Ok(socket)
            },
            client,
            "relay.invalid".try_into().unwrap(),
        );
        (result, server.join().unwrap())
    }
    #[test]
    fn verified_tls_authenticates_once_and_classifies_replies_without_submission() {
        for (response, expected) in [
            (&b"235 authenticated\r\n"[..], Ok(())),
            (b"454 temporary\r\n", Err(Error::Temporary)),
            (b"535 rejected\r\n", Err(Error::Permanent)),
            (b"334 challenge\r\n", Err(Error::Reply)),
            (b"250 unexpected\r\n", Err(Error::Reply)),
        ] {
            assert_eq!(
                fixture(true, b"250-secure\r\n250 AUTH LOGIN plain\r\n", response),
                (expected, 1)
            );
        }
    }
    #[test]
    fn certificate_and_secure_capability_failures_send_no_auth_or_fallback() {
        assert_eq!(
            fixture(false, b"250-secure\r\n250 AUTH PLAIN\r\n", b"235 ready\r\n"),
            (Err(Error::Transport(smtp_tls::Error::Certificate)), 0)
        );
        for (name, expired) in [("other.invalid", false), ("relay.invalid", true)] {
            assert_eq!(
                fixture_certificate(
                    true,
                    name,
                    expired,
                    b"250-secure\r\n250 AUTH PLAIN\r\n",
                    b"235 ready\r\n"
                ),
                (Err(Error::Transport(smtp_tls::Error::Certificate)), 0)
            );
        }
        for capabilities in [
            &b"250-secure\r\n250 AUTH LOGIN\r\n"[..],
            b"250 AUTH PLAIN\r\n",
            b"250-secure\r\n250 XAUTH PLAIN\r\n",
            b"250-secure\r\n250 AUTH XPLAIN\r\n",
        ] {
            assert_eq!(
                fixture(true, capabilities, b"235 ready\r\n"),
                (Err(Error::Unsupported), 0)
            );
        }
        assert_eq!(
            fixture(
                true,
                b"250-AUTH PLAIN\r\n550 inconsistent\r\n",
                b"235 ready\r\n"
            ),
            (Err(Error::Reply), 0)
        );
    }
}

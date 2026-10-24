//! Controlled one-message verified-STARTTLS relay; no external services.
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
pub(super) fn start() -> (Vec<u8>, u16, thread::JoinHandle<Vec<u8>>) {
    let cert = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()]).unwrap();
    let root = cert.cert.der().to_vec();
    let config = Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(root.clone())],
                rustls::pki_types::PrivateKeyDer::Pkcs8(
                    rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()),
                ),
            )
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let task = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(2))
                }
                Err(error) => panic!("relay fixture accept failed: {error:?}"),
            }
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut plain = BufReader::new(socket);
        plain.get_mut().write_all(b"220 fixture\r\n").unwrap();
        let line = |reader: &mut dyn BufRead| {
            let mut value = Vec::new();
            assert!(
                std::io::Read::take(reader, 16385)
                    .read_until(b'\n', &mut value)
                    .unwrap()
                    > 0
            );
            assert!(value.len() <= 16384);
            value
        };
        assert!(line(&mut plain).starts_with(b"EHLO "));
        plain
            .get_mut()
            .write_all(b"250-fixture\r\n250 STARTTLS\r\n")
            .unwrap();
        assert_eq!(line(&mut plain), b"STARTTLS\r\n");
        assert!(plain.buffer().is_empty());
        plain.get_mut().write_all(b"220 upgrade\r\n").unwrap();
        let mut tls = BufReader::new(rustls::StreamOwned::new(
            rustls::ServerConnection::new(config).unwrap(),
            plain.into_inner(),
        ));
        assert!(line(&mut tls).starts_with(b"EHLO "));
        tls.get_mut()
            .write_all(b"250-fixture\r\n250 AUTH PLAIN\r\n")
            .unwrap();
        tls.get_mut().flush().unwrap();
        use base64::Engine;
        assert_eq!(
            line(&mut tls),
            format!(
                "AUTH PLAIN {}\r\n",
                base64::engine::general_purpose::STANDARD
                    .encode(b"\0fixture-user\0fixture-password")
            )
            .as_bytes()
        );
        tls.get_mut().write_all(b"235 authenticated\r\n").unwrap();
        tls.get_mut().flush().unwrap();
        for (command, reply) in [
            (
                b"MAIL FROM:<s@fixture.invalid>\r\n".as_slice(),
                b"250 mail\r\n".as_slice(),
            ),
            (b"RCPT TO:<r@fixture.invalid>\r\n", b"250 recipient\r\n"),
            (b"DATA\r\n", b"354 data\r\n"),
        ] {
            assert_eq!(line(&mut tls), command);
            tls.get_mut().write_all(reply).unwrap();
            tls.get_mut().flush().unwrap();
        }
        let mut data = Vec::new();
        loop {
            let part = line(&mut tls);
            let end = part == b".\r\n";
            data.extend(part);
            assert!(data.len() <= 16384);
            if end {
                break;
            }
        }
        tls.get_mut().write_all(b"250 accepted\r\n").unwrap();
        tls.get_mut().flush().unwrap();
        let mut tail = Vec::new();
        let _ = std::io::Read::take(&mut tls, 256).read_until(b'\n', &mut tail);
        data
    });
    (root, port, task)
}

//! Bounded two-request HTTPS fixture with an ephemeral certificate.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
type Requests = Vec<(String, Vec<u8>)>;
pub(super) fn start() -> (Vec<u8>, u16, thread::JoinHandle<Requests>) {
    start_statuses(vec!["503 Service Unavailable", "204 No Content"])
}
pub(super) fn start_statuses(
    statuses: Vec<&'static str>,
) -> (Vec<u8>, u16, thread::JoinHandle<Requests>) {
    start_with_authorization(statuses, true)
}
pub(super) fn start_with_authorization(
    statuses: Vec<&'static str>,
    authorization: bool,
) -> (Vec<u8>, u16, thread::JoinHandle<Requests>) {
    let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
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
        let mut received = Vec::new();
        for status in statuses {
            let deadline = Instant::now() + Duration::from_secs(15);
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("HTTPS fixture accept failed: {error:?}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let tls = rustls::StreamOwned::new(
                rustls::ServerConnection::new(config.clone()).unwrap(),
                socket,
            );
            let mut reader = BufReader::new(tls);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                Read::take(&mut reader, 8193).read_line(&mut line).unwrap();
                assert!(!line.is_empty());
                headers.push_str(&line);
                assert!(headers.len() <= 8192);
                if line == "\r\n" {
                    break;
                }
            }
            assert!(headers.starts_with("POST /hook HTTP/1.1\r\n"));
            let lower = headers.to_ascii_lowercase();
            if authorization {
                assert!(lower.contains("authorization: bearer fixture\r\n"));
            } else {
                assert!(!lower.contains("authorization:"));
            }
            let length: usize = lower
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap()
                .parse()
                .unwrap();
            assert!(length <= 16384);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            received.push((headers, body));
            write!(
                reader.get_mut(),
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            reader.get_mut().flush().unwrap();
        }
        received
    });
    (root, port, task)
}

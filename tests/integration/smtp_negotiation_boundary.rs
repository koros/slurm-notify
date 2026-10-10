//! Harness-only loopback negotiation probe; no installed frame or delivery ACK.
use std::{io::Read, net::TcpStream, sync::Arc, time::Duration};

// A separate single-field frame holds only a decimal loopback fixture port.
// Strict size/EOF checks precede network access; no ambient trust or credentials.
pub(super) fn serve(mut input: impl Read) -> Result<(), ()> {
    let mut prefix = [0; 4];
    input.read_exact(&mut prefix).map_err(|_| ())?;
    let size = u32::from_be_bytes(prefix) as usize;
    if !(1..=5).contains(&size) {
        return Err(());
    }
    let mut bytes = [0; 5];
    input.read_exact(&mut bytes[..size]).map_err(|_| ())?;
    let mut extra = [0];
    if input.read(&mut extra).map_err(|_| ())? != 0 || !bytes[..size].iter().all(u8::is_ascii_digit)
    {
        return Err(());
    }
    let port = std::str::from_utf8(&bytes[..size])
        .map_err(|_| ())?
        .parse::<u16>()
        .map_err(|_| ())?;
    if port == 0 {
        return Err(());
    }
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let socket = TcpStream::connect_timeout(&address, Duration::from_secs(2)).map_err(|_| ())?;
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| ())?;
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| ())?;
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    super::smtp_tls::required_starttls(
        socket,
        Arc::new(config),
        "relay.invalid".try_into().map_err(|_| ())?,
    )
    .map_err(|_| ())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn acceptance() {
    use super::transport;
    use std::{io::Write, net::TcpListener, process::Command, thread, time::Instant};
    fn command(socket: &mut TcpStream, expected: &[u8]) {
        let mut buffer = vec![0; expected.len()];
        socket.read_exact(&mut buffer).unwrap();
        assert!(buffer == expected);
    }
    for (upgrade, drip) in [(false, false), (false, true), (true, false), (true, true)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => panic!("SMTP fixture accept failed"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            if upgrade {
                socket.write_all(b"220 ready\r\n").unwrap();
                command(&mut socket, b"EHLO fixture.invalid\r\n");
                socket.write_all(b"250-relay\r\n250 STARTTLS\r\n").unwrap();
                command(&mut socket, b"STARTTLS\r\n");
            }
            let mut writes = 0;
            if drip {
                // Never complete the line; each byte arrives well inside the
                // child's two-second socket timeout and below reply byte limits.
                socket.write_all(b"220 ").unwrap();
                for _ in 0..100 {
                    if socket.write_all(b" ").is_err() {
                        break;
                    }
                    writes += 1;
                    thread::sleep(Duration::from_millis(20));
                }
            }
            let mut unexpected = [0; 1];
            match socket.read(&mut unexpected) {
                Ok(0) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                _ => panic!("SMTP fixture did not observe closed connection"),
            }
            writes
        });
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.arg("--smtp-negotiation");
        let input = transport::frame(&[port.as_bytes()], 9).unwrap();
        let start = Instant::now();
        assert_eq!(
            transport::supervise(&mut child, input, Duration::from_millis(500), 9, 32),
            Err(transport::Error::Timeout)
        );
        assert!(start.elapsed() < Duration::from_millis(1500));
        let writes = server.join().unwrap();
        if drip {
            assert!(writes >= 2);
        }
    }
    // A complete refusal returns promptly as child failure, never relay acceptance.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline);
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => panic!("SMTP fixture accept failed"),
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
        command(&mut socket, b"EHLO fixture.invalid\r\n");
        socket.write_all(b"250-relay\r\n250 STARTTLS\r\n").unwrap();
        command(&mut socket, b"STARTTLS\r\n");
        socket.write_all(b"454 unavailable\r\n").unwrap();
        assert!(matches!(socket.read(&mut [0]), Ok(0)));
    });
    let mut child = Command::new(std::env::current_exe().unwrap());
    child.arg("--smtp-negotiation");
    assert_eq!(
        transport::supervise(
            &mut child,
            transport::frame(&[port.as_bytes()], 9).unwrap(),
            Duration::from_secs(2),
            9,
            32
        ),
        Err(transport::Error::ChildExit)
    );
    server.join().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap());
    child.arg("--smtp-negotiation");
    assert_eq!(
        transport::supervise(&mut child, vec![255; 4], Duration::from_secs(2), 9, 32),
        Err(transport::Error::ChildExit)
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn handshake_acceptance() {
    use super::transport;
    use std::{io::Write, net::TcpListener, process::Command, thread, time::Instant};
    // Stall, partial TLS record drip, and immediate disconnect after ClientHello.
    for mode in 0..3 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => panic!("TLS fixture accept failed"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            fn command(socket: &mut TcpStream, expected: &[u8]) {
                let mut bytes = vec![0; expected.len()];
                socket.read_exact(&mut bytes).unwrap();
                assert!(bytes == expected);
            }
            socket.write_all(b"220 ready\r\n").unwrap();
            command(&mut socket, b"EHLO fixture.invalid\r\n");
            socket.write_all(b"250-relay\r\n250 STARTTLS\r\n").unwrap();
            command(&mut socket, b"STARTTLS\r\n");
            socket.write_all(b"220 upgrade\r\n").unwrap();
            let mut header = [0; 5];
            socket.read_exact(&mut header).unwrap();
            assert_eq!(header[0], 22); // Handshake record, not plaintext EHLO.
            let length = usize::from(u16::from_be_bytes([header[3], header[4]]));
            assert!((1..=4096).contains(&length));
            let mut hello = vec![0; length];
            socket.read_exact(&mut hello).unwrap();
            assert_eq!(hello[0], 1); // ClientHello handshake message.
            if mode == 2 {
                return 0;
            }
            let mut writes = 0;
            if mode == 1 {
                // A partial handshake record stays incomplete beyond the parent
                // deadline; each byte arrives within the socket inactivity bound.
                socket.write_all(&[22, 3, 3, 0, 100]).unwrap();
                for _ in 0..100 {
                    if socket.write_all(&[0]).is_err() {
                        break;
                    }
                    writes += 1;
                    thread::sleep(Duration::from_millis(20));
                }
            }
            match socket.read(&mut [0]) {
                Ok(0) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                _ => panic!("TLS fixture did not observe closed connection"),
            }
            writes
        });
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.arg("--smtp-negotiation");
        let start = Instant::now();
        assert_eq!(
            transport::supervise(
                &mut child,
                transport::frame(&[port.as_bytes()], 9).unwrap(),
                Duration::from_millis(500),
                9,
                32
            ),
            Err(if mode == 2 {
                transport::Error::ChildExit
            } else {
                transport::Error::Timeout
            })
        );
        assert!(start.elapsed() < Duration::from_millis(1500));
        let writes = server.join().unwrap();
        if mode == 1 {
            assert!(writes >= 2);
        }
    }
}

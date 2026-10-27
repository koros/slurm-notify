//! Controlled TLS/API fixtures; fixed production hostnames resolve only to owned loopback.
use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
    time::Instant,
};
type Captured = Vec<(String, Vec<u8>)>;
pub(crate) fn fixture(
    replies: Vec<(u16, Vec<u8>, &'static str)>,
) -> (ureq::Agent, thread::JoinHandle<Captured>) {
    fixture_tls(replies, true, true)
}
fn fixture_tls(
    replies: Vec<(u16, Vec<u8>, &'static str)>,
    trusted: bool,
    valid_name: bool,
) -> (ureq::Agent, thread::JoinHandle<Captured>) {
    let cert = rcgen::generate_simple_self_signed(if valid_name {
        vec![
            "api.sendgrid.com".into(),
            "api.mailgun.net".into(),
            "api.eu.mailgun.net".into(),
            "gmail.googleapis.com".into(),
            "openidconnect.googleapis.com".into(),
            "oauth2.googleapis.com".into(),
        ]
    } else {
        vec!["wrong.fixture.invalid".into()]
    })
    .unwrap();
    let der = cert.cert.der().to_vec();
    let mut roots = rustls::RootCertStore::empty();
    if trusted {
        roots
            .add(rustls::pki_types::CertificateDer::from(der.clone()))
            .unwrap();
    }
    let client = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );
    let server = Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(der)],
                rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())
                    .into(),
            )
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let agent = ureq::AgentBuilder::new()
        .tls_config(client)
        .resolver(move |_: &str| Ok(vec![address]))
        .timeout(Duration::from_secs(3))
        .redirects(0)
        .try_proxy_from_env(false)
        .build();
    let task = thread::spawn(move || {
        let mut captured = Vec::new();
        for (status, body, extra) in replies {
            let deadline = Instant::now() + Duration::from_secs(5);
            let socket = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("owned fixture: {e:?}"),
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
                rustls::ServerConnection::new(server.clone()).unwrap(),
                socket,
            );
            let mut reader = BufReader::new(tls);
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                if Read::take(&mut reader, 8193).read_line(&mut line).is_err() {
                    return captured;
                }
                assert!(!line.is_empty());
                headers.push_str(&line);
                assert!(headers.len() <= 8192);
                if line == "\r\n" {
                    break;
                }
            }
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|s| s.parse::<usize>().ok())
                })
                .unwrap_or(0);
            assert!(length <= 16384);
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            captured.push((headers, request));
            write!(reader.get_mut(),"HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",body.len()).unwrap();
            // An intentionally oversized body may be cut off by the bounded reader.
            let _ = reader.get_mut().write_all(&body);
            let _ = reader.get_mut().flush();
        }
        captured
    });
    (agent, task)
}
fn profile(kind: &str) -> Profile {
    Profile::parse(&serde_json::to_vec(&crate::email_profile::tests::fixture(kind)).unwrap())
        .unwrap()
}
fn request(p: &Profile) -> Request {
    crate::email_wire::prepare(
        p,
        &crate::email_content::Content::synthetic(&crate::public_event::identifier([0; 16]), 0)
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn controlled_tls_submissions_use_fixed_targets_and_authentication() {
    for (kind, status, body, expected) in [
        ("sendgrid", 202, b"".as_slice(), "bearer fixture"),
        (
            "mailgun",
            200,
            b"{\"id\":\"<fixture-id>\"}",
            "basic YXBpOmZpeHR1cmU=",
        ),
    ] {
        let p = profile(kind);
        let wire = request(&p);
        let (agent, server) = fixture(vec![(status, body.to_vec(), "")]);
        assert_eq!(send(&p, b"fixture", &wire, &agent, 0), Outcome::Accepted);
        let captured = server.join().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].1, wire.body.as_slice());
        assert!(captured[0].0.to_ascii_lowercase().contains(&format!(
            "authorization: {}\r\n",
            expected.to_ascii_lowercase()
        )));
        assert!(captured[0].0.starts_with(if kind == "sendgrid" {
            "POST /v3/mail/send "
        } else {
            "POST /v3/mg.fixture.invalid/messages "
        }));
    }
    let p = profile("gmail_api");
    let (agent,server)=fixture(vec![(200,b"{\"sub\":\"fixture-account\",\"email\":\"s@fixture.invalid\",\"email_verified\":true}".to_vec(),""),(200,b"{\"id\":\"fixture-gmail-id\"}".to_vec(),"")]);
    assert_eq!(
        send(
            &p,
            b"{\"access_token\":\"fixture\",\"expires_at\":100}",
            &request(&p),
            &agent,
            0
        ),
        Outcome::Accepted
    );
    let captured = server.join().unwrap();
    assert_eq!(captured.len(), 2);
    assert!(captured[0].0.starts_with("GET /v1/userinfo "));
    assert!(captured[1]
        .0
        .starts_with("POST /gmail/v1/users/me/messages/send "));
}
#[test]
fn refusal_unknown_success_and_oversized_compressed_responses_are_bounded() {
    let p = profile("mailgun");
    let wire = request(&p);
    for (status, body, extra, want) in [
        (
            302,
            vec![],
            "Location: https://attacker.invalid/\r\n",
            Outcome::Rejected,
        ),
        (200, b"malformed".to_vec(), "", Outcome::Unknown),
        (200, vec![b'x'; 8193], "", Outcome::Unknown),
        (
            200,
            b"encoded".to_vec(),
            "Content-Encoding: gzip\r\n",
            Outcome::Unknown,
        ),
        (401, vec![b'x'; 8193], "", Outcome::Authentication),
        (429, vec![], "Retry-After: 600\r\n", Outcome::Retry(600)),
    ] {
        let (agent, server) = fixture(vec![(status, body, extra)]);
        assert_eq!(send(&p, b"fixture", &wire, &agent, 0), want);
        assert_eq!(server.join().unwrap().len(), 1);
    }
    let p = profile("gmail_api");
    let (agent, server) = fixture(vec![(
        200,
        b"{\"sub\":\"different\",\"email\":\"s@fixture.invalid\",\"email_verified\":true}".to_vec(),
        "",
    )]);
    assert_eq!(
        send(
            &p,
            b"{\"access_token\":\"fixture\",\"expires_at\":100}",
            &request(&p),
            &agent,
            0
        ),
        Outcome::NotAttempted
    );
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn untrusted_or_wrong_hostname_tls_never_submits_email() {
    for (trusted, name) in [(false, true), (true, false)] {
        let p = profile("sendgrid");
        let (agent, server) = fixture_tls(vec![(202, vec![], "")], trusted, name);
        assert_eq!(
            send(&p, b"fixture", &request(&p), &agent, 0),
            Outcome::Unknown
        );
        assert!(server.join().unwrap().is_empty());
    }
}

#[test]
fn preflighted_gmail_dispatch_makes_only_one_email_post() {
    let mut p = profile("gmail_api");
    p.client_id = Zeroizing::new("desktop-fixture".into());
    let wire = request(&p);
    for (status, body, expected) in [
        (
            200,
            br#"{"id":"gmail-fixture"}"#.to_vec(),
            Outcome::Accepted,
        ),
        (401, Vec::new(), Outcome::Authentication),
        (
            403,
            br#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#.to_vec(),
            Outcome::Policy,
        ),
    ] {
        let (agent, server) = fixture(vec![(status, body, "")]);
        assert_eq!(
            send_gmail_checked(&p, b"fake-access-token", &wire, &agent, 0),
            expected
        );
        let captured = server.join().unwrap();
        assert_eq!(captured.len(), 1);
        assert!(captured[0]
            .0
            .starts_with("POST /gmail/v1/users/me/messages/send "));
        assert_eq!(captured[0].1, wire.body.as_slice());
    }
}

#[test]
fn mailgun_region_selection_uses_exact_fixed_host_without_failover() {
    for (region, host) in [("us", "api.mailgun.net"), ("eu", "api.eu.mailgun.net")] {
        let mut p = profile("mailgun");
        p.region = region;
        let wire = request(&p);
        let (agent, server) = fixture(vec![(200, br#"{"id":"<fixture-id>"}"#.to_vec(), "")]);
        assert_eq!(send(&p, b"fake-key", &wire, &agent, 0), Outcome::Accepted);
        let captured = server.join().unwrap();
        assert_eq!(captured.len(), 1);
        assert!(captured[0]
            .0
            .to_ascii_lowercase()
            .contains(&format!("host: {host}\r\n")));
    }
}

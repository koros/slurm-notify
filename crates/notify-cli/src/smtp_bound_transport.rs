//! Shared bound STARTTLS child codec and private supervised parent adapter.
use super::{
    smtp_auth, smtp_auth_frame,
    smtp_message::Prepared,
    smtp_provider_revision::{Descriptor, TlsMode},
    smtp_resolved::Resolved,
    smtp_submit,
    smtp_submit_frame::{self, Ack},
};
use super::{
    smtp_submit_dispatch, storage::FrozenMessage, supervised_provider::Diagnostic, transport,
};

use std::process::Command;
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    sync::Arc,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
const BOUNDS: [usize; 8] = [4096, 64, 65664, 194, 254, 254, 16384, 5];
pub(super) const INPUT_LIMIT: usize = 32 + 4096 + 64 + 65664 + 194 + 254 + 254 + 16384 + 5;
fn invalid() -> transport::Error {
    transport::Error::InvalidInput
}

pub(super) fn encode(
    resolved: Resolved,
    message: &FrozenMessage,
    timeout: Duration,
) -> Result<Zeroizing<Vec<u8>>, transport::Error> {
    let (sender, recipient) = message.envelope();
    encode_parts(resolved, sender, recipient, message.data(), timeout)
}
pub(super) fn encode_prepared(
    resolved: Resolved,
    message: &Prepared,
    timeout: Duration,
) -> Result<Zeroizing<Vec<u8>>, transport::Error> {
    let (sender, recipient) = message.envelope();
    encode_parts(resolved, &sender, &recipient, message.data(), timeout)
}
fn encode_parts(
    resolved: Resolved,
    sender: &str,
    recipient: &str,
    data: &[u8],
    timeout: Duration,
) -> Result<Zeroizing<Vec<u8>>, transport::Error> {
    let millis = timeout.as_millis();
    if !(1..=30000).contains(&millis)
        || timeout != Duration::from_millis(millis as u64)
        || resolved.provider().profile().tls != TlsMode::RequiredStartTls
    {
        return Err(invalid());
    }
    let (provider, roots, credentials) = resolved.into_parts();
    let refs: Vec<_> = roots.iter().map(|root| root.as_slice()).collect();
    let roots = transport::frame(&refs, 65664)?;
    let credentials = credentials.into_plain();
    let millis = millis.to_string();
    transport::frame(
        &[
            provider.canonical(),
            provider.token().as_bytes(),
            &roots,
            &credentials,
            sender.as_bytes(),
            recipient.as_bytes(),
            data,
            millis.as_bytes(),
        ],
        INPUT_LIMIT,
    )
    .map(Zeroizing::new)
}
struct Request {
    resolved: Resolved,
    message: Prepared,
    timeout: Duration,
}
fn decode(mut input: impl Read) -> Result<Request, transport::Error> {
    let mut fields = Vec::with_capacity(8);
    for bound in BOUNDS {
        let mut size = [0; 4];
        input.read_exact(&mut size).map_err(|_| invalid())?;
        let size = u32::from_be_bytes(size) as usize;
        if size == 0 || size > bound {
            return Err(invalid());
        }
        let mut field = Zeroizing::new(vec![0; size]);
        input.read_exact(&mut field).map_err(|_| invalid())?;
        fields.push(field);
    }
    let mut trailing = [0];
    if input.read(&mut trailing).map_err(|_| invalid())? != 0 {
        return Err(invalid());
    }
    let token = std::str::from_utf8(&fields[1]).map_err(|_| invalid())?;
    let provider = Descriptor::from_stored(token, Zeroizing::new(fields[0].to_vec()))
        .map_err(|_| invalid())?;
    if provider.profile().tls != TlsMode::RequiredStartTls {
        return Err(invalid());
    }
    let mut rest = fields[2].as_slice();
    let mut roots = Vec::new();
    let mut total = 0usize;
    while !rest.is_empty() {
        let size = rest.get(..4).ok_or_else(invalid)?;
        let size = u32::from_be_bytes(size.try_into().map_err(|_| invalid())?) as usize;
        rest = &rest[4..];
        total = total
            .checked_add(size)
            .filter(|n| *n <= 65536)
            .ok_or_else(invalid)?;
        if roots.len() == 32 || size == 0 {
            return Err(invalid());
        }
        roots.push(rest.get(..size).ok_or_else(invalid)?);
        rest = &rest[size..];
    }
    let plain = fields[3].as_slice();
    let parts: Vec<_> = plain.split(|b| *b == 0).collect();
    if parts.len() != 3 || !parts[0].is_empty() {
        return Err(invalid());
    }
    let text = |bytes| std::str::from_utf8(bytes).map_err(|_| invalid());
    let resolved = Resolved::new(
        &provider,
        &provider.profile(),
        &roots,
        text(parts[1])?,
        text(parts[2])?,
    )
    .map_err(|_| invalid())?;
    let millis = text(&fields[7])?.parse::<u64>().map_err(|_| invalid())?;
    if !(1..=30000).contains(&millis) || millis.to_string().as_bytes() != fields[7].as_slice() {
        return Err(invalid());
    }
    let message = Prepared::from_frozen(
        text(&fields[4])?,
        text(&fields[5])?,
        Zeroizing::new(fields[6].to_vec()),
    )
    .map_err(|_| invalid())?;
    Ok(Request {
        resolved,
        message,
        timeout: Duration::from_millis(millis),
    })
}

// Call only in a supervised child: parent deadlines cover DNS, TLS and socket I/O.
pub(super) fn serve(input: impl Read, mut output: impl Write) -> std::io::Result<()> {
    let result = (|| {
        let request = decode(input).map_err(|_| smtp_auth_frame::Ack::Input)?;
        let (provider, roots, credentials) = request.resolved.into_parts();
        let profile = provider.profile();
        let mut store = rustls::RootCertStore::empty();
        for root in roots {
            store
                .add(rustls::pki_types::CertificateDer::from(root))
                .map_err(|_| smtp_auth_frame::Ack::Configuration)?;
        }
        let config = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(store)
                .with_no_client_auth(),
        );
        let name = rustls::pki_types::ServerName::try_from(profile.server_name.to_owned())
            .map_err(|_| smtp_auth_frame::Ack::Configuration)?;
        let stream = smtp_auth::connect_authenticated(
            credentials,
            || {
                let deadline = Instant::now()
                    .checked_add(request.timeout)
                    .ok_or(std::io::ErrorKind::InvalidInput)?;
                let addresses = (profile.relay, profile.port).to_socket_addrs()?;
                for address in addresses.take(16) {
                    let remaining = deadline
                        .checked_duration_since(Instant::now())
                        .filter(|d| !d.is_zero())
                        .ok_or(std::io::ErrorKind::TimedOut)?;
                    if let Ok(socket) = TcpStream::connect_timeout(&address, remaining) {
                        socket.set_read_timeout(Some(request.timeout))?;
                        socket.set_write_timeout(Some(request.timeout))?;
                        return Ok(socket);
                    }
                }
                Err(std::io::ErrorKind::ConnectionRefused.into())
            },
            config,
            name,
        )
        .map_err(auth_ack)?;
        Ok(smtp_submit::submit(stream, &request.message))
    })();
    let ack = match result {
        Ok(report) => Ack::Submission(report),
        Err(error) => Ack::Auth(error),
    };
    let bytes = smtp_submit_frame::encode_ack(ack).map_err(|_| std::io::ErrorKind::InvalidData)?;
    output.write_all(&bytes)?;
    output.flush()
}
fn auth_ack(error: smtp_auth::Error) -> smtp_auth_frame::Ack {
    use smtp_auth_frame::Ack;
    match error {
        smtp_auth::Error::Credentials => Ack::Credentials,
        smtp_auth::Error::Unsupported => Ack::Unsupported,
        smtp_auth::Error::Temporary => Ack::Temporary,
        smtp_auth::Error::Permanent => Ack::Permanent,
        smtp_auth::Error::Reply => Ack::Reply,
        smtp_auth::Error::Io => Ack::Io,
        smtp_auth::Error::Transport(error) => match error {
            super::smtp_tls::Error::Certificate => Ack::Certificate,
            super::smtp_tls::Error::TlsConfiguration => Ack::Configuration,
            super::smtp_tls::Error::TlsProtocol => Ack::Protocol,
            super::smtp_tls::Error::HandshakeIo | super::smtp_tls::Error::Io => Ack::Io,
            super::smtp_tls::Error::Reply => Ack::Reply,
            super::smtp_tls::Error::UpgradeMissing => Ack::UpgradeMissing,
            super::smtp_tls::Error::UpgradeTemporary => Ack::UpgradeTemporary,
            super::smtp_tls::Error::UpgradePermanent => Ack::UpgradePermanent,
        },
    }
}
pub(super) fn dispatch(
    command: &mut Command,
    resolved: Resolved,
    message: &FrozenMessage,
    timeout: Duration,
) -> Result<super::delivery::ProviderOutcome, Diagnostic> {
    if resolved.provider().profile().tls != TlsMode::RequiredStartTls {
        return Err(Diagnostic::SmtpConfiguration);
    }
    let result = encode(resolved, message, timeout).and_then(|frame| {
        transport::supervise_with(
            command,
            frame,
            timeout,
            INPUT_LIMIT,
            128,
            smtp_submit_frame::decode_ack,
        )
    });
    smtp_submit_dispatch::classify(result).into_dispatch()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)] // Custom process harness compiles helpers without unit tests.
    use crate::{
        lifecycle::Reconciler,
        scheduler::PollRequest,
        smtp_provider_revision::{self, Profile},
        smtp_render::JobVisibility,
        smtp_routes::{Metadata, Plan, Route},
        storage::{self, Store},
    };
    fn fixture(mode: TlsMode) -> (Resolved, FrozenMessage) {
        let root = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()])
            .unwrap()
            .cert
            .der()
            .to_vec();
        let hash = smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
        let profile = Profile {
            destination: "smtp",
            relay: "127.0.0.1",
            port: 465,
            tls: mode,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: &hash,
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        };
        let provider = Descriptor::new(&profile).unwrap();
        let resolved = Resolved::new(
            &provider,
            &profile,
            &[&root],
            "fixture-user",
            "synthetic-password",
        )
        .unwrap();
        let directory = storage::tests::Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let routes = [Route {
            name: "smtp",
            visibility: JobVisibility::Hidden,
            metadata: Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: "fixture",
            },
        }];
        let plan = Plan::new(&["smtp"], &routes, 1).unwrap();
        let request = PollRequest {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        plan.poll_with(
            &request,
            &mut Reconciler::with_capacity(10),
            &mut store,
            10,
            |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    ""
                } else {
                    "cluster|1|alice|s|actual|COMPLETED|0:0"
                }
                .into())
            },
        )
        .unwrap();
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        (
            resolved,
            store.frozen_smtp(&item.event_id, "smtp").unwrap().unwrap(),
        )
    }
    #[test]
    fn bound_frame_round_trip_preserves_profile_credentials_trust_and_frozen_bytes() {
        let (resolved, frozen) = fixture(TlsMode::RequiredStartTls);
        let token = resolved.provider().token().to_owned();
        let bytes = encode(resolved, &frozen, Duration::from_millis(1234)).unwrap();
        let request = decode(bytes.as_slice()).unwrap();
        assert_eq!(request.resolved.provider().token(), token);
        assert_eq!(request.timeout, Duration::from_millis(1234));
        assert_eq!(request.message.data(), frozen.data());
        assert_eq!(
            request.message.envelope(),
            (
                "s@fixture.invalid".to_owned(),
                "r@fixture.invalid".to_owned()
            )
        );
        let (profile, roots, credentials) = request.resolved.into_parts();
        assert_eq!(profile.profile().relay, "127.0.0.1");
        assert_eq!(profile.profile().port, 465);
        assert_eq!(profile.profile().server_name, "smtp.fixture.invalid");
        assert_eq!(roots.len(), 1);
        assert!(credentials.into_plain().starts_with(b"\0fixture-user\0"));
    }
    #[test]
    fn bound_frame_rejects_truncation_trailing_and_oversized_fields_before_network() {
        let (resolved, frozen) = fixture(TlsMode::RequiredStartTls);
        let bytes = encode(resolved, &frozen, Duration::from_secs(1)).unwrap();
        for size in 0..bytes.len() {
            assert!(decode(&bytes[..size]).is_err());
        }
        assert!(decode([bytes.as_slice(), b"x"].concat().as_slice()).is_err());
        let mut offset = 0;
        for bound in BOUNDS {
            let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            for length in [0, (bound + 1) as u32, u32::MAX] {
                let mut invalid = bytes.to_vec();
                invalid[offset..offset + 4].copy_from_slice(&length.to_be_bytes());
                assert!(decode(invalid.as_slice()).is_err());
            }
            offset += 4 + size;
        }
        let mut output = Vec::new();
        serve(&b""[..], &mut output).unwrap();
        assert_eq!(
            smtp_submit_frame::decode_ack(&output).unwrap(),
            Ack::Auth(smtp_auth_frame::Ack::Input)
        );
    }
    #[test]
    fn unsupported_tls_and_invalid_deadlines_block_before_child_spawn() {
        let mut missing = Command::new("/private/missing/slurm-notify-bound-helper");
        let (resolved, frozen) = fixture(TlsMode::Implicit);
        assert_eq!(
            dispatch(&mut missing, resolved, &frozen, Duration::from_secs(1)),
            Err(Diagnostic::SmtpConfiguration)
        );
        for duration in [
            Duration::ZERO,
            Duration::from_nanos(1),
            Duration::from_millis(30001),
        ] {
            let (resolved, frozen) = fixture(TlsMode::RequiredStartTls);
            assert_eq!(
                dispatch(&mut missing, resolved, &frozen, duration),
                Err(Diagnostic::InvalidInput)
            );
        }
        let (resolved, frozen) = fixture(TlsMode::RequiredStartTls);
        assert_eq!(
            dispatch(&mut missing, resolved, &frozen, Duration::from_secs(1)),
            Err(Diagnostic::Spawn)
        );
    }
}

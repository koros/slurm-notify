//! Private fixture submission envelope and canonical sanitized results.
use super::{
    delivery::ProviderOutcome,
    smtp_auth::Credentials,
    smtp_auth_frame, smtp_message,
    smtp_submit::{Acceptance, Diagnostic, Phase, Report},
    transport,
};
use std::io::Read;
use zeroize::Zeroizing;

const BOUNDS: [usize; 10] = [4096, 5, 64, 128, 254, 254, 64, 128, 4096, 10];
pub(super) const INPUT_LIMIT: usize = 40 + 4096 + 5 + 64 + 128 + 254 + 254 + 64 + 128 + 4096 + 10;
pub(super) const OUTPUT_LIMIT: usize = 48;
// No Debug or raw credential accessors. Root is fixture-only, never ambient trust.
pub(super) struct Request {
    pub(super) root: Vec<u8>,
    pub(super) port: u16,
    pub(super) credentials: Credentials,
    pub(super) message: smtp_message::Prepared,
}

pub(super) fn decode_request(mut input: impl Read) -> Result<Request, transport::Error> {
    let invalid = || transport::Error::InvalidInput;
    let mut fields = Vec::with_capacity(10);
    for (index, bound) in BOUNDS.into_iter().enumerate() {
        let mut prefix = [0; 4];
        input.read_exact(&mut prefix).map_err(|_| invalid())?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length > bound || (length == 0 && index != 8) {
            return Err(invalid());
        }
        let mut field = Zeroizing::new(vec![0; length]);
        input.read_exact(&mut field).map_err(|_| invalid())?;
        fields.push(field);
    }
    let mut extra = [0];
    if input.read(&mut extra).map_err(|_| invalid())? != 0 {
        return Err(invalid());
    }
    let text = |index: usize| std::str::from_utf8(&fields[index]).map_err(|_| invalid());
    let decimal = |index| -> Result<&str, transport::Error> {
        let value = text(index)?;
        if !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid());
        }
        Ok(value)
    };
    let port = decimal(1)?
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(invalid)?;
    let credentials = Credentials::new(Some(text(2)?), Some(text(3)?)).map_err(|_| invalid())?;
    let timestamp = decimal(9)?.parse::<u64>().map_err(|_| invalid())?;
    let message =
        smtp_message::prepare(text(4)?, text(5)?, text(6)?, text(7)?, text(8)?, timestamp)
            .map_err(|_| invalid())?;
    Ok(Request {
        root: fields[0].to_vec(),
        port,
        credentials,
        message,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ack {
    Auth(smtp_auth_frame::Ack),
    Submission(Report),
}

// Enumerate only semantically valid reports. Arbitrary outcome/phase/acceptance
// combinations and authentication success cannot be encoded as submission evidence.
fn canonical() -> Vec<Ack> {
    use smtp_auth_frame::Ack as Auth;
    let mut values: Vec<_> = [
        Auth::Input,
        Auth::Credentials,
        Auth::Unsupported,
        Auth::Temporary,
        Auth::Permanent,
        Auth::Reply,
        Auth::Io,
        Auth::Certificate,
        Auth::Configuration,
        Auth::Protocol,
        Auth::UpgradeMissing,
        Auth::UpgradeTemporary,
        Auth::UpgradePermanent,
    ]
    .into_iter()
    .map(Ack::Auth)
    .collect();
    for phase in [Phase::Mail, Phase::Recipient, Phase::Data, Phase::FinalData] {
        for diagnostic in [
            Diagnostic::Temporary,
            Diagnostic::Permanent,
            Diagnostic::Io,
            Diagnostic::Protocol,
        ] {
            values.push(Ack::Submission(Report {
                outcome: if diagnostic == Diagnostic::Permanent {
                    ProviderOutcome::Rejected
                } else {
                    ProviderOutcome::Retry
                },
                phase,
                diagnostic,
                acceptance: if phase != Phase::FinalData {
                    Acceptance::NotSubmitted
                } else if matches!(diagnostic, Diagnostic::Temporary | Diagnostic::Permanent) {
                    Acceptance::NotAccepted
                } else {
                    Acceptance::Unknown
                },
            }));
        }
    }
    values.push(Ack::Submission(Report {
        outcome: ProviderOutcome::Delivered,
        phase: Phase::FinalData,
        diagnostic: Diagnostic::RelayAccepted,
        acceptance: Acceptance::RelayAccepted,
    }));
    values
}
fn token(ack: Ack) -> Vec<u8> {
    match ack {
        Ack::Auth(auth) => {
            let suffix = auth.bytes().strip_prefix(b"SMTP_AUTH ").unwrap();
            let mut bytes = b"SMTP_SUBMIT AUTH ".to_vec();
            bytes.extend_from_slice(suffix);
            bytes
        }
        Ack::Submission(report) => {
            let phase = match report.phase {
                Phase::Mail => "MAIL",
                Phase::Recipient => "RCPT",
                Phase::Data => "DATA",
                Phase::FinalData => "FINAL",
            };
            let diagnostic = match report.diagnostic {
                Diagnostic::RelayAccepted => "ACCEPTED",
                Diagnostic::Temporary => "TEMP",
                Diagnostic::Permanent => "PERM",
                Diagnostic::Io => "IO",
                Diagnostic::Protocol => "PROTOCOL",
            };
            let acceptance = match report.acceptance {
                Acceptance::RelayAccepted => "RA",
                Acceptance::NotSubmitted => "NS",
                Acceptance::NotAccepted => "NA",
                Acceptance::Unknown => "U",
            };
            format!("SMTP_SUBMIT {phase} {diagnostic} {acceptance}\n").into_bytes()
        }
    }
}
pub(super) fn encode_ack(ack: Ack) -> Result<Vec<u8>, transport::Error> {
    if !canonical().contains(&ack) {
        return Err(transport::Error::InvalidResult);
    }
    Ok(token(ack))
}
pub(super) fn decode_ack(bytes: &[u8]) -> Result<Ack, transport::Error> {
    canonical()
        .into_iter()
        .find(|ack| token(*ack) == bytes)
        .ok_or(transport::Error::InvalidResult)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    fn fields() -> [&'static [u8]; 10] {
        [
            b"fixture-root",
            b"1234",
            b"u",
            b"p",
            b"s@fixture.invalid",
            b"r@fixture.invalid",
            b"event_1",
            b"done",
            b"done\n.\n.dot",
            b"0",
        ]
    }
    #[test]
    fn bounded_request_rejects_every_truncation_and_oversize_field() {
        let fields = fields();
        let bytes = Zeroizing::new(transport::frame(&fields, INPUT_LIMIT).unwrap());
        let request = decode_request(&bytes[..]).unwrap();
        assert_eq!(request.port, 1234);
        assert!(request.root == b"fixture-root");
        assert!(request.message.data().ends_with(b"..\r\n..dot\r\n.\r\n"));
        drop(request.credentials);
        let root = vec![0; 4096];
        let username = "u".repeat(64);
        let password = "p".repeat(128);
        let mailbox = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        let identity = "i".repeat(64);
        let subject = "s".repeat(128);
        let body = "\n".repeat(4096);
        let maximum = Zeroizing::new(
            transport::frame(
                &[
                    &root,
                    b"65535",
                    username.as_bytes(),
                    password.as_bytes(),
                    mailbox.as_bytes(),
                    mailbox.as_bytes(),
                    identity.as_bytes(),
                    subject.as_bytes(),
                    body.as_bytes(),
                    b"4102444799",
                ],
                INPUT_LIMIT,
            )
            .unwrap(),
        );
        assert_eq!(maximum.len(), INPUT_LIMIT);
        assert!(decode_request(&maximum[..]).is_ok());
        for end in 0..bytes.len() {
            assert!(decode_request(&bytes[..end]).is_err());
        }
        let mut extra = Zeroizing::new(bytes.to_vec());
        extra.push(0);
        assert!(decode_request(&extra[..]).is_err());
        for index in 0..10 {
            let mut bytes =
                Zeroizing::new(transport::frame(&fields[..index], INPUT_LIMIT).unwrap());
            bytes.extend_from_slice(&((BOUNDS[index] + 1) as u32).to_be_bytes());
            assert!(decode_request(&bytes[..]).is_err());
            let prefix_length = bytes.len() - 4;
            bytes.truncate(prefix_length);
            bytes.extend_from_slice(&0u32.to_be_bytes());
            if index != 8 {
                assert!(decode_request(&bytes[..]).is_err());
            }
        }
    }
    #[test]
    fn structured_inputs_validate_before_returning_a_request() {
        for (index, invalid) in [
            (1, b"0".as_slice()),
            (1, b"65536"),
            (1, b"+1"),
            (2, b"u\0"),
            (3, b"p\r"),
            (4, b"s@fixture.invalid\r\nBcc: x"),
            (5, b"invalid"),
            (6, b"id@domain"),
            (7, b"s\n"),
            (8, b"body\0"),
            (9, b"-1"),
            (9, b"4102444800"),
            (8, b"\xff"),
        ] {
            let mut fields = fields();
            fields[index] = invalid;
            let bytes = Zeroizing::new(transport::frame(&fields, INPUT_LIMIT).unwrap());
            assert!(decode_request(&bytes[..]).is_err());
        }
        let mut fields = fields();
        fields[8] = b"";
        assert!(decode_request(&transport::frame(&fields, INPUT_LIMIT).unwrap()[..]).is_ok());
    }
    #[test]
    fn acknowledgments_are_canonical_and_isolated_from_other_modes() {
        for ack in canonical() {
            let bytes = encode_ack(ack).unwrap();
            assert!(bytes.len() <= OUTPUT_LIMIT);
            assert_eq!(decode_ack(&bytes), Ok(ack));
            let mut extra = bytes.clone();
            extra.push(0);
            assert!(decode_ack(&extra).is_err());
            assert!(decode_ack(&bytes[..bytes.len() - 1]).is_err());
        }
        for bytes in [
            b"OK\n".as_slice(),
            b"SMTP_AUTH AUTHENTICATED\n",
            b"SMTP_NEGOTIATION READY\n",
            b"SMTP_SUBMIT MAIL ACCEPTED RA\n",
            b"SMTP_SUBMIT FINAL PERM U\n",
            b"SMTP_SUBMIT FINAL ACCEPTED RA\r\n",
            b"SMTP_SUBMIT FINAL IO U secret\n",
        ] {
            assert!(decode_ack(bytes).is_err());
        }
        assert!(encode_ack(Ack::Auth(smtp_auth_frame::Ack::Authenticated)).is_err());
        assert!(encode_ack(Ack::Submission(Report {
            outcome: ProviderOutcome::Delivered,
            phase: Phase::Mail,
            diagnostic: Diagnostic::RelayAccepted,
            acceptance: Acceptance::RelayAccepted
        }))
        .is_err());
    }
}

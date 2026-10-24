//! Shared sanitized AUTH acknowledgments and private fixture credential framing.
#[cfg(test)]
use super::{smtp_auth::Credentials, transport};
#[cfg(test)]
use std::io::Read;
#[cfg(test)]
use zeroize::Zeroizing;

#[cfg(test)]
const BOUNDS: [usize; 3] = [5, 64, 128];
#[cfg(test)]
pub(super) const LIMIT: usize = 12 + 5 + 64 + 128;

// Deliberately no Debug/Clone; decoding never returns raw credentials.
#[cfg(test)]
pub(super) struct Request {
    pub(super) port: u16,
    pub(super) credentials: Credentials,
}

#[cfg(test)]
pub(super) fn decode(mut input: impl Read) -> Result<Request, transport::Error> {
    let mut fields = Vec::with_capacity(3);
    for bound in BOUNDS {
        let mut prefix = [0; 4];
        input
            .read_exact(&mut prefix)
            .map_err(|_| transport::Error::InvalidInput)?;
        let size = u32::from_be_bytes(prefix) as usize;
        if size == 0 || size > bound {
            return Err(transport::Error::InvalidInput);
        }
        let mut field = Zeroizing::new(vec![0; size]);
        input
            .read_exact(&mut field)
            .map_err(|_| transport::Error::InvalidInput)?;
        fields.push(field);
    }
    let mut trailing = [0];
    if input
        .read(&mut trailing)
        .map_err(|_| transport::Error::InvalidInput)?
        != 0
    {
        return Err(transport::Error::InvalidInput);
    }
    let text = |index: usize| {
        std::str::from_utf8(&fields[index]).map_err(|_| transport::Error::InvalidInput)
    };
    let port_text = text(0)?;
    if !port_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(transport::Error::InvalidInput);
    }
    let port = port_text
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or(transport::Error::InvalidInput)?;
    let credentials = Credentials::new(Some(text(1)?), Some(text(2)?))
        .map_err(|_| transport::Error::InvalidInput)?;
    Ok(Request { port, credentials })
}

// Own and erase the outgoing frame too; callers must preserve this wrapper when
// eventually moving it into a supervised writer. No argv/environment transport.
#[cfg(test)]
pub(super) fn encode(
    port: u16,
    username: &str,
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, transport::Error> {
    if port == 0 {
        return Err(transport::Error::InvalidInput);
    }
    Credentials::new(Some(username), Some(password)).map_err(|_| transport::Error::InvalidInput)?;
    let port = port.to_string();
    transport::frame(
        &[port.as_bytes(), username.as_bytes(), password.as_bytes()],
        LIMIT,
    )
    .map(Zeroizing::new)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ack {
    #[cfg(test)]
    Authenticated,
    Credentials,
    Unsupported,
    Temporary,
    Permanent,
    Reply,
    Io,
    Input,
    Certificate,
    Configuration,
    Protocol,
    UpgradeMissing,
    UpgradeTemporary,
    UpgradePermanent,
}
impl Ack {
    pub(super) fn bytes(self) -> &'static [u8] {
        match self {
            #[cfg(test)]
            Self::Authenticated => b"SMTP_AUTH AUTHENTICATED\n",
            Self::Credentials => b"SMTP_AUTH CREDENTIALS\n",
            Self::Unsupported => b"SMTP_AUTH UNSUPPORTED\n",
            Self::Temporary => b"SMTP_AUTH TEMPORARY\n",
            Self::Permanent => b"SMTP_AUTH PERMANENT\n",
            Self::Reply => b"SMTP_AUTH REPLY\n",
            Self::Io => b"SMTP_AUTH IO\n",
            Self::Input => b"SMTP_AUTH INPUT\n",
            Self::Certificate => b"SMTP_AUTH CERTIFICATE\n",
            Self::Configuration => b"SMTP_AUTH CONFIGURATION\n",
            Self::Protocol => b"SMTP_AUTH PROTOCOL\n",
            Self::UpgradeMissing => b"SMTP_AUTH TLS_MISSING\n",
            Self::UpgradeTemporary => b"SMTP_AUTH TLS_TEMP\n",
            Self::UpgradePermanent => b"SMTP_AUTH TLS_PERM\n",
        }
    }
}
#[cfg(test)]
const ACKS: [Ack; 14] = [
    Ack::Authenticated,
    Ack::Credentials,
    Ack::Unsupported,
    Ack::Temporary,
    Ack::Permanent,
    Ack::Reply,
    Ack::Io,
    Ack::Input,
    Ack::Certificate,
    Ack::Configuration,
    Ack::Protocol,
    Ack::UpgradeMissing,
    Ack::UpgradeTemporary,
    Ack::UpgradePermanent,
];
#[cfg(test)]
pub(super) fn decode_ack(bytes: &[u8]) -> Result<Ack, transport::Error> {
    ACKS.into_iter()
        .find(|ack| ack.bytes() == bytes)
        .ok_or(transport::Error::InvalidResult)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[test]
    fn bounded_roundtrip_and_invalid_inputs() {
        let frame = encode(65535, &"u".repeat(64), &"p".repeat(128)).unwrap();
        assert_eq!(frame.len(), LIMIT);
        let request = decode(&frame[..]).unwrap();
        assert_eq!(request.port, 65535);
        drop(request.credentials);
        for (port, user, password) in [
            (0, "u", "p"),
            (1, "", "p"),
            (1, "u", ""),
            (1, "u\n", "p"),
            (1, "u", "p\0"),
            (1, "ü", "p"),
        ] {
            assert!(matches!(
                encode(port, user, password),
                Err(transport::Error::InvalidInput)
            ));
        }
        assert!(encode(1, &"u".repeat(65), "p").is_err());
        assert!(encode(1, "u", &"p".repeat(129)).is_err());
    }
    #[test]
    fn malformed_frames_fail_before_a_request_exists() {
        let valid = encode(1234, "u", "p").unwrap();
        for end in 0..valid.len() {
            assert!(matches!(
                decode(&valid[..end]),
                Err(transport::Error::InvalidInput)
            ));
        }
        let mut extra = Zeroizing::new(valid.to_vec());
        extra.push(0);
        assert!(decode(&extra[..]).is_err());
        for fields in [
            [&b"0"[..], b"u", b"p"],
            [b"65536", b"u", b"p"],
            [b"+1", b"u", b"p"],
            [b"1", b"u\r", b"p"],
            [b"1", b"u", b"\xff"],
        ] {
            let frame = Zeroizing::new(transport::frame(&fields, LIMIT).unwrap());
            assert!(decode(&frame[..]).is_err());
        }
        for prefix in [0u32, 6, u32::MAX] {
            assert!(decode(&prefix.to_be_bytes()[..]).is_err());
        }
    }
    #[test]
    fn acknowledgments_are_exact_and_separate_from_delivery_and_negotiation() {
        for ack in ACKS {
            assert_eq!(decode_ack(ack.bytes()), Ok(ack));
            assert!(ack.bytes().len() <= 32);
            let mut extra = ack.bytes().to_vec();
            extra.push(0);
            assert_eq!(decode_ack(&extra), Err(transport::Error::InvalidResult));
        }
        for bytes in [
            &b"OK\n"[..],
            b"SMTP_NEGOTIATION READY\n",
            b"SMTP_AUTH AUTHENTICATED",
            b"SMTP_AUTH AUTHENTICATED\r\n",
            b"SMTP_AUTH PERMANENT secret\n",
        ] {
            assert_eq!(decode_ack(bytes), Err(transport::Error::InvalidResult));
        }
    }
}

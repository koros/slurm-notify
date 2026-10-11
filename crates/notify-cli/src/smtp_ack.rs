//! Private negotiation diagnostics: Ready never means a message was submitted.
use super::transport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ack {
    Ready,
    Input,
    Certificate,
    Configuration,
    Io,
    Protocol,
    Reply,
    Upgrade,
}

impl Ack {
    pub(super) fn bytes(self) -> &'static [u8] {
        match self {
            Self::Ready => b"SMTP_NEGOTIATION READY\n",
            Self::Input => b"SMTP_NEGOTIATION INPUT\n",
            Self::Certificate => b"SMTP_NEGOTIATION CERTIFICATE\n",
            Self::Configuration => b"SMTP_NEGOTIATION CONFIGURATION\n",
            Self::Io => b"SMTP_NEGOTIATION IO\n",
            Self::Protocol => b"SMTP_NEGOTIATION PROTOCOL\n",
            Self::Reply => b"SMTP_NEGOTIATION REPLY\n",
            Self::Upgrade => b"SMTP_NEGOTIATION UPGRADE\n",
        }
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<Ack, transport::Error> {
    [
        Ack::Ready,
        Ack::Input,
        Ack::Certificate,
        Ack::Configuration,
        Ack::Io,
        Ack::Protocol,
        Ack::Reply,
        Ack::Upgrade,
    ]
    .into_iter()
    .find(|ack| ack.bytes() == bytes)
    .ok_or(transport::Error::InvalidResult)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness does not run unit tests.
    use super::*;
    #[test]
    fn diagnostics_roundtrip_without_delivery_acceptance() {
        for ack in [
            Ack::Ready,
            Ack::Input,
            Ack::Certificate,
            Ack::Configuration,
            Ack::Io,
            Ack::Protocol,
            Ack::Reply,
            Ack::Upgrade,
        ] {
            assert_eq!(decode(ack.bytes()), Ok(ack));
            assert!(ack.bytes().len() <= 32);
        }
    }
    #[test]
    fn delivery_tokens_and_noncanonical_diagnostics_are_rejected() {
        for bytes in [
            &b"OK\n"[..],
            b"RETRY\n",
            b"REJECT\n",
            b"SMTP_NEGOTIATION READY",
            b"SMTP_NEGOTIATION READY\r\n",
            b"SMTP_NEGOTIATION READY\nextra",
            b"SMTP_NEGOTIATION CERTIFICATE private\n",
            b"\xff",
        ] {
            assert_eq!(decode(bytes), Err(transport::Error::InvalidResult));
        }
    }
}

//! Private single-recipient message experiment; no user-facing email contract.
use lettre::{
    message::{header::ContentType, Mailbox},
    Address, Message,
};
use std::time::{Duration, UNIX_EPOCH};
use zeroize::Zeroizing;

const BODY_LIMIT: usize = 4096;
const MESSAGE_LIMIT: usize = 16384;
const WIRE_LIMIT: usize = 16384;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Address,
    Header,
    Body,
    Date,
    Encoding,
    Limit,
}

// No Debug/Clone: addresses and message bytes are not diagnostics.
pub(super) struct Prepared {
    sender: Address,
    recipient: Address,
    data: Zeroizing<Vec<u8>>,
}

impl Prepared {
    // Owned private snapshot transport only; do not render or sample a clock.
    pub(super) fn from_frozen(
        sender: &str,
        recipient: &str,
        data: Zeroizing<Vec<u8>>,
    ) -> Result<Self, Error> {
        let sender = address(sender)?;
        let recipient = address(recipient)?;
        validate_frozen_data(&data)?;
        Ok(Self {
            sender,
            recipient,
            data,
        })
    }

    pub(super) fn mail_command(&self) -> String {
        format!("MAIL FROM:<{}>\r\n", self.sender)
    }
    pub(super) fn recipient_command(&self) -> String {
        format!("RCPT TO:<{}>\r\n", self.recipient)
    }
    pub(super) fn envelope(&self) -> (String, String) {
        (self.sender.to_string(), self.recipient.to_string())
    }
    pub(super) fn data(&self) -> &[u8] {
        &self.data
    }
}

pub(super) fn address(value: &str) -> Result<Address, Error> {
    // Deliberately narrower than the library: ASCII addr-spec only, no display
    // names, quoted local parts, address literals, lists, or SMTPUTF8.
    if value.is_empty()
        || value.len() > 254
        || !value.bytes().all(|b| (33..=126).contains(&b))
        || value.bytes().any(|b| b"<>[](),;:\\\"".contains(&b))
    {
        return Err(Error::Address);
    }
    let parsed: Address = value.parse().map_err(|_| Error::Address)?;
    if parsed.user().len() > 64 || parsed.domain().len() > 253 {
        return Err(Error::Address);
    }
    Ok(parsed)
}

pub(super) fn prepare(
    sender: &str,
    recipient: &str,
    identity: &str,
    subject: &str,
    body: &str,
    timestamp: u64,
) -> Result<Prepared, Error> {
    let sender = address(sender)?;
    let recipient = address(recipient)?;
    if identity.is_empty()
        || identity.len() > 64
        || !identity
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || subject.is_empty()
        || subject.len() > 128
        || !subject.bytes().all(|b| (32..=126).contains(&b))
    {
        return Err(Error::Header);
    }
    // Fixture text accepts LF and printable ASCII only. No logs, arbitrary MIME,
    // attachments, Unicode, CR, or binary data are admitted by this prototype.
    if body.len() > BODY_LIMIT
        || !body.bytes().all(|b| b == b'\n' || (32..=126).contains(&b))
        || body.split('\n').any(|line| line.len() > 998)
    {
        return Err(Error::Body);
    }
    // Explicit frozen timestamp; never sample wall clock or generate a random ID.
    if timestamp > 4_102_444_799 {
        return Err(Error::Date);
    } // Through year 2099.
    let message = Message::builder()
        .from(Mailbox::new(None, sender.clone()))
        .to(Mailbox::new(None, recipient.clone()))
        .date(UNIX_EPOCH + Duration::from_secs(timestamp))
        .message_id(Some(format!("<{identity}@fixture.invalid>")))
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_owned())
        .map_err(|_| Error::Encoding)?;
    let mut formatted = Zeroizing::new(message.formatted());
    if !formatted.ends_with(b"\r\n") {
        formatted.extend_from_slice(b"\r\n");
    }
    let data = data_frame(&formatted)?;
    Ok(Prepared {
        sender,
        recipient,
        data,
    })
}

// The message builder handles headers/MIME/CRLF. SMTP transparency is a transport
// operation: duplicate a leading dot on every DATA line, then append one terminator.
fn data_frame(message: &[u8]) -> Result<Zeroizing<Vec<u8>>, Error> {
    if message.len() > MESSAGE_LIMIT {
        return Err(Error::Limit);
    }
    let mut dots = 0;
    for line in message.split_inclusive(|byte| *byte == b'\n') {
        if !line.ends_with(b"\r\n")
            || line.len() > 1000
            || !line[..line.len() - 2]
                .iter()
                .all(|b| *b == b'\t' || (32..=126).contains(b))
        {
            return Err(Error::Encoding);
        }
        dots += usize::from(line.starts_with(b"."));
    }
    if message.is_empty() {
        return Err(Error::Encoding);
    }
    let length = message.len() + dots + 3;
    if length > WIRE_LIMIT {
        return Err(Error::Limit);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(length));
    for line in message.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b".") {
            bytes.push(b'.');
        }
        bytes.extend_from_slice(line);
    }
    bytes.extend_from_slice(b".\r\n");
    Ok(bytes)
}

// Validate stored transport bytes without invoking a renderer or generating fields.
// This checks framing, not cryptographic integrity or arbitrary MIME semantics.
pub(super) fn validate_frozen_data(data: &[u8]) -> Result<(), Error> {
    if data.len() > WIRE_LIMIT || !data.ends_with(b".\r\n") {
        return Err(Error::Encoding);
    }
    let mut message = Zeroizing::new(Vec::with_capacity(data.len()));
    for line in data[..data.len() - 3].split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b".") {
            if !line.starts_with(b"..") {
                return Err(Error::Encoding);
            }
            message.extend_from_slice(&line[1..]);
        } else {
            message.extend_from_slice(line);
        }
    }
    if *data_frame(&message)? != data {
        return Err(Error::Encoding);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    fn sample(body: &str) -> Result<Prepared, Error> {
        prepare(
            "sender@fixture.invalid",
            "recipient@fixture.invalid",
            "event_123",
            "Job completed",
            body,
            0,
        )
    }
    #[test]
    fn frozen_message_and_envelope_are_deterministic_and_single_recipient() {
        let first = sample("done\n.dot\n.\n..two").unwrap();
        let second = sample("done\n.dot\n.\n..two").unwrap();
        assert!(*first.data == *second.data);
        assert!(first.sender.to_string() == "sender@fixture.invalid");
        assert!(first.recipient.to_string() == "recipient@fixture.invalid");
        let text = std::str::from_utf8(&first.data).unwrap();
        assert!(text.contains("Message-ID: <event_123@fixture.invalid>\r\n"));
        assert!(text.contains("Date: Thu, 01 Jan 1970 00:00:00 +0000\r\n"));
        assert!(text.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(text.ends_with("done\r\n..dot\r\n..\r\n...two\r\n.\r\n"));
        for body in ["", "one", "one\n", "\n"] {
            assert!(sample(body).unwrap().data.ends_with(b"\r\n.\r\n"));
        }
    }
    #[test]
    fn structured_injection_and_unsupported_addresses_never_build_messages() {
        for value in [
            "",
            "a",
            "a@@fixture.invalid",
            ".a@fixture.invalid",
            "a..b@fixture.invalid",
            "a@",
            "a@fixture.invalid\r\nBcc: other@fixture.invalid",
            "a@fixture.invalid,b@fixture.invalid",
            "Name <a@fixture.invalid>",
            "\"a\"@fixture.invalid",
            "a@[127.0.0.1]",
            "ä@fixture.invalid",
        ] {
            assert!(matches!(address(value), Err(Error::Address)));
            assert!(prepare(value, "r@fixture.invalid", "id", "s", "b", 0).is_err());
            assert!(prepare("s@fixture.invalid", value, "id", "s", "b", 0).is_err());
        }
        assert!(address(&format!("{}@fixture.invalid", "a".repeat(65))).is_err());
        assert!(address(&"a".repeat(255)).is_err());
        for header in ["", "x\r\nTo: injected", "x\0", "ü"] {
            assert!(matches!(
                prepare(
                    "s@fixture.invalid",
                    "r@fixture.invalid",
                    "id",
                    header,
                    "b",
                    0
                ),
                Err(Error::Header)
            ));
        }
        for id in ["", "a@evil.invalid", "a\r\n", "<id>", "a b"] {
            assert!(matches!(
                prepare("s@fixture.invalid", "r@fixture.invalid", id, "s", "b", 0),
                Err(Error::Header)
            ));
        }
    }
    #[test]
    fn limits_precede_builder_and_data_framing_bounds_lines_and_terminator() {
        for body in [
            "x\r\ny",
            "x\0",
            "ü",
            "\t",
            &"x".repeat(999),
            &"\n".repeat(BODY_LIMIT + 1),
        ] {
            assert!(matches!(sample(body), Err(Error::Body)));
        }
        assert!(sample(&"x".repeat(998)).is_ok());
        assert!(sample(&"\n".repeat(BODY_LIMIT)).is_ok());
        assert!(prepare(
            "s@fixture.invalid",
            "r@fixture.invalid",
            &"i".repeat(64),
            &"s".repeat(128),
            "b",
            4_102_444_799
        )
        .is_ok());
        assert!(matches!(
            prepare(
                "s@fixture.invalid",
                "r@fixture.invalid",
                &"i".repeat(65),
                "s",
                "b",
                0
            ),
            Err(Error::Header)
        ));
        assert!(matches!(
            prepare(
                "s@fixture.invalid",
                "r@fixture.invalid",
                "i",
                &"s".repeat(129),
                "b",
                0
            ),
            Err(Error::Header)
        ));
        assert!(matches!(
            prepare(
                "s@fixture.invalid",
                "r@fixture.invalid",
                "i",
                "s",
                "b",
                4_102_444_800
            ),
            Err(Error::Date)
        ));
        for bytes in [&b""[..], b"bare\n", b"bare\r", b"x\0\r\n", b"\xff\r\n"] {
            assert!(matches!(data_frame(bytes), Err(Error::Encoding)));
        }
        assert!(matches!(
            data_frame(&vec![b'x'; MESSAGE_LIMIT + 1]),
            Err(Error::Limit)
        ));
        assert!(matches!(
            data_frame(format!("{}\r\n", "x".repeat(999)).as_bytes()),
            Err(Error::Encoding)
        ));
        assert!(matches!(
            data_frame(".\r\n".repeat(MESSAGE_LIMIT / 3).as_bytes()),
            Err(Error::Limit)
        ));
        assert!(&*data_frame(b".\r\n..\r\n\r\n").unwrap() == b"..\r\n...\r\n\r\n.\r\n");
        assert!(!format!("{:?}", Error::Header).contains("fixture.invalid"));
    }
}

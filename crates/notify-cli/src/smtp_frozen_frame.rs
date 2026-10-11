//! Separate private frozen-wire frame; no renderer fields or public raw-message API.
use super::{
    smtp_auth::Credentials, smtp_message, smtp_submit_frame::Request, storage::FrozenMessage,
    transport,
};
use std::io::Read;
use zeroize::Zeroizing;
const BOUNDS: [usize; 7] = [4096, 5, 64, 128, 254, 254, 16384];
pub(super) const INPUT_LIMIT: usize = 28 + 4096 + 5 + 64 + 128 + 254 + 254 + 16384;

// Resolve before child construction; legacy missing and corrupt snapshots both block.
pub(super) fn load(
    store: &super::storage::Store,
    event: &str,
    destination: &str,
) -> Result<FrozenMessage, transport::Error> {
    store
        .frozen_smtp(event, destination)
        .map_err(|_| transport::Error::InvalidInput)?
        .ok_or(transport::Error::InvalidInput)
}

pub(super) fn encode(
    root: &[u8],
    port: u16,
    username: &str,
    password: &str,
    message: &FrozenMessage,
) -> Result<Zeroizing<Vec<u8>>, transport::Error> {
    let (sender, recipient) = message.envelope();
    let port = port.to_string();
    let fields = [
        root,
        port.as_bytes(),
        username.as_bytes(),
        password.as_bytes(),
        sender.as_bytes(),
        recipient.as_bytes(),
        message.data(),
    ];
    if fields
        .iter()
        .zip(BOUNDS)
        .any(|(field, bound)| field.is_empty() || field.len() > bound)
        || port == "0"
    {
        return Err(transport::Error::InvalidInput);
    }
    Credentials::new(Some(username), Some(password)).map_err(|_| transport::Error::InvalidInput)?;
    transport::frame(&fields, INPUT_LIMIT).map(Zeroizing::new)
}

pub(super) fn decode_request(mut input: impl Read) -> Result<Request, transport::Error> {
    let invalid = || transport::Error::InvalidInput;
    let mut fields = Vec::with_capacity(7);
    for bound in BOUNDS {
        let mut prefix = [0; 4];
        input.read_exact(&mut prefix).map_err(|_| invalid())?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length == 0 || length > bound {
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
    let port = text(1)?;
    if !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(invalid)?;
    let credentials = Credentials::new(Some(text(2)?), Some(text(3)?)).map_err(|_| invalid())?;
    let message =
        smtp_message::Prepared::from_frozen(text(4)?, text(5)?, Zeroizing::new(fields[6].to_vec()))
            .map_err(|_| invalid())?;
    Ok(Request {
        root: fields[0].to_vec(),
        port,
        credentials,
        message,
    })
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[test]
    fn maximum_frozen_frame_preserves_every_wire_byte() {
        let address = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        let mut data = Zeroizing::new(Vec::new());
        for _ in 0..16 {
            data.extend_from_slice(&vec![b'a'; 998]);
            data.extend_from_slice(b"\r\n");
        }
        data.extend_from_slice(&vec![b'b'; 379]);
        data.extend_from_slice(b"\r\n.\r\n");
        assert_eq!(data.len(), 16384);
        let root = vec![0; 4096];
        let username = "u".repeat(64);
        let password = "p".repeat(128);
        let frame = Zeroizing::new(
            transport::frame(
                &[
                    &root,
                    b"65535",
                    username.as_bytes(),
                    password.as_bytes(),
                    address.as_bytes(),
                    address.as_bytes(),
                    &data,
                ],
                INPUT_LIMIT,
            )
            .unwrap(),
        );
        assert_eq!(frame.len(), INPUT_LIMIT);
        let decoded = decode_request(frame.as_slice()).unwrap();
        assert_eq!(decoded.port, 65535);
        assert!(decoded.message.data() == data.as_slice());
        assert!(decoded.message.envelope() == (address.clone(), address));
    }

    #[test]
    fn frozen_frame_rejects_truncation_bounds_and_invalid_wire() {
        let message = smtp_message::prepare(
            "s@fixture.invalid",
            "r@fixture.invalid",
            "id",
            "subject",
            "original\n.\n.dot",
            123,
        )
        .unwrap();
        let (sender, recipient) = message.envelope();
        let fields = [
            b"root".as_slice(),
            b"25",
            b"u",
            b"p",
            sender.as_bytes(),
            recipient.as_bytes(),
            message.data(),
        ];
        let frame = Zeroizing::new(transport::frame(&fields, INPUT_LIMIT).unwrap());
        let decoded = decode_request(frame.as_slice()).unwrap();
        assert!(decoded.message.data() == message.data());
        assert!(decoded.message.envelope() == message.envelope());
        for end in 0..frame.len() {
            assert!(decode_request(&frame[..end]).is_err());
        }
        let mut extra = frame.to_vec();
        extra.push(0);
        assert!(decode_request(extra.as_slice()).is_err());
        for (index, bound) in BOUNDS.into_iter().enumerate() {
            for length in [0, bound + 1] {
                let mut bytes = Vec::new();
                for field in &fields[..index] {
                    bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
                    bytes.extend_from_slice(field);
                }
                bytes.extend_from_slice(&(length as u32).to_be_bytes());
                assert!(decode_request(bytes.as_slice()).is_err());
            }
        }
        for (index, bad) in [
            (1, b"0".as_slice()),
            (1, b"+25"),
            (1, b"65536"),
            (2, b"bad\0"),
            (4, b"bad\r\n"),
            (5, b"bad"),
            (6, b"early\r\n.\r\nextra\r\n.\r\n"),
            (6, b"raw\n.\r\n"),
        ] {
            let mut changed = fields;
            changed[index] = bad;
            assert!(
                decode_request(transport::frame(&changed, INPUT_LIMIT).unwrap().as_slice())
                    .is_err()
            );
        }
    }
}

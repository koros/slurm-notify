//! Private installed child entry point; parent supervision supplies the deadline.
use crate::{
    provider_frame::{self, Error},
    provider_outcome, public_event, webhook,
};
use std::{
    io::{Read, Write},
    time::Duration,
};
pub(super) fn serve(input: impl Read, output: impl Write) -> Result<(), Error> {
    provider_frame::serve_optional(
        input,
        output,
        20 + 4096 + 4096 + 128 + 16384 + 5 + 4 + 65664,
        true,
        |fields| {
            if fields[0].len() > 4096 || fields[1].len() > 4096 || fields[3].len() > 16384 {
                return Err(Error::InvalidInput);
            }
            let text = |i: usize| std::str::from_utf8(&fields[i]).map_err(|_| Error::InvalidInput);
            let endpoint = text(0)?;
            if webhook::endpoint_url(endpoint)
                .map_err(|_| Error::InvalidInput)?
                .scheme()
                != "https"
            {
                return Err(Error::InvalidInput);
            }
            let id = text(2)?;
            if !public_event::valid_id(id) {
                return Err(Error::InvalidInput);
            }
            let payload: serde_json::Value =
                serde_json::from_slice(&fields[3]).map_err(|_| Error::InvalidInput)?;
            if payload.get("event_id").and_then(serde_json::Value::as_str) != Some(id) {
                return Err(Error::InvalidInput);
            }
            let millis = text(4)?;
            let timeout = millis.parse::<u64>().map_err(|_| Error::InvalidInput)?;
            if !(1..=30000).contains(&timeout) || timeout.to_string() != millis {
                return Err(Error::InvalidInput);
            }
            let authorization = text(1)?;
            let mut client = webhook::Webhook::new(
                endpoint,
                (!authorization.is_empty()).then(|| authorization.to_owned()),
                Duration::from_millis(timeout),
                16384,
            )
            .map_err(|_| Error::InvalidInput)?;
            if let Some(encoded) = fields.get(5) {
                let mut remaining = encoded.as_slice();
                let mut roots = Vec::new();
                while !remaining.is_empty() {
                    let prefix: [u8; 4] = remaining
                        .get(..4)
                        .ok_or(Error::InvalidInput)?
                        .try_into()
                        .map_err(|_| Error::InvalidInput)?;
                    let length = u32::from_be_bytes(prefix) as usize;
                    remaining = &remaining[4..];
                    roots.push(remaining.get(..length).ok_or(Error::InvalidInput)?);
                    remaining = &remaining[length..];
                    if roots.len() > 32 {
                        return Err(Error::InvalidInput);
                    }
                }
                client.use_roots(&roots).map_err(|_| Error::InvalidInput)?;
            }
            client
                .send_with_clock(id, &fields[3], provider_outcome::unix_time)
                .map_err(|_| Error::InvalidInput)
        },
    )
}

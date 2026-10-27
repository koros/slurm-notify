//! Shared bounded private provider frame codec.
use crate::provider_outcome::ProviderOutcome;
use std::io::{Read, Write};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    InvalidInput,
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    UnsupportedPlatform,
    Spawn,
    Timeout,
    Io,
    OutputLimit,
    ChildExit,
    InvalidResult,
    Cleanup,
}

// Length prefixes permit arbitrary payload bytes without delimiters or escaping.
// No Debug representation of the frame, endpoint, or authorization is provided.
pub(super) fn frame(fields: &[&[u8]], limit: usize) -> Result<Vec<u8>, Error> {
    let length = fields
        .iter()
        .try_fold(0usize, |total, field| {
            total.checked_add(4)?.checked_add(field.len())
        })
        .ok_or(Error::InvalidInput)?;
    if length > limit || fields.iter().any(|f| f.len() > u32::MAX as usize) {
        return Err(Error::InvalidInput);
    }
    let mut bytes = Vec::with_capacity(length);
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    Ok(bytes)
}

pub(super) fn parse_result(bytes: &[u8]) -> Result<ProviderOutcome, Error> {
    match bytes {
        b"OK\n" => Ok(ProviderOutcome::Delivered),
        b"RETRY\n" => Ok(ProviderOutcome::Retry),
        b"REJECT\n" => Ok(ProviderOutcome::Rejected),
        _ => {
            let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidResult)?;
            let seconds = text
                .strip_prefix("RETRY_AFTER ")
                .and_then(|v| v.strip_suffix('\n'))
                .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or(Error::InvalidResult)?;
            Ok(ProviderOutcome::RetryAfter(seconds))
        }
    }
}

// Decode exactly five fields, enforcing the total byte budget before allocation.
// Fields: endpoint, authorization (empty means absent), ID, body,
// socket timeout milliseconds. This remains a private prototype wire format.
#[cfg(test)]
pub(super) fn serve(
    input: impl Read,
    output: impl Write,
    limit: usize,
    send: impl FnOnce(Vec<Vec<u8>>) -> Result<ProviderOutcome, Error>,
) -> Result<(), Error> {
    serve_optional(input, output, limit, false, send)
}

pub(super) fn serve_optional(
    mut input: impl Read,
    mut output: impl Write,
    limit: usize,
    trust_extension: bool,
    send: impl FnOnce(Vec<Vec<u8>>) -> Result<ProviderOutcome, Error>,
) -> Result<(), Error> {
    let mut fields = Vec::with_capacity(5);
    let mut used = 0usize;
    for _ in 0..5 {
        let mut prefix = [0; 4];
        input
            .read_exact(&mut prefix)
            .map_err(|_| Error::InvalidInput)?;
        let length = u32::from_be_bytes(prefix) as usize;
        used = used
            .checked_add(4)
            .and_then(|v| v.checked_add(length))
            .filter(|v| *v <= limit)
            .ok_or(Error::InvalidInput)?;
        let mut field = vec![0; length];
        input
            .read_exact(&mut field)
            .map_err(|_| Error::InvalidInput)?;
        fields.push(field);
    }
    let mut extra = [0; 1];
    if input.read(&mut extra).map_err(|_| Error::Io)? != 0 {
        if !trust_extension {
            return Err(Error::InvalidInput);
        }
        let mut prefix = [0; 4];
        prefix[0] = extra[0];
        input
            .read_exact(&mut prefix[1..])
            .map_err(|_| Error::InvalidInput)?;
        let length = u32::from_be_bytes(prefix) as usize;
        used.checked_add(4)
            .and_then(|used| used.checked_add(length))
            .filter(|used| *used <= limit)
            .ok_or(Error::InvalidInput)?;
        let mut field = vec![0; length];
        input
            .read_exact(&mut field)
            .map_err(|_| Error::InvalidInput)?;
        fields.push(field);
        if input.read(&mut extra).map_err(|_| Error::Io)? != 0 {
            return Err(Error::InvalidInput);
        }
    }
    let result = match send(fields)? {
        ProviderOutcome::Delivered => "OK\n".to_owned(),
        ProviderOutcome::Retry => "RETRY\n".to_owned(),
        ProviderOutcome::Rejected => "REJECT\n".to_owned(),
        ProviderOutcome::RetryAfter(seconds) if seconds >= 0 => format!("RETRY_AFTER {seconds}\n"),
        ProviderOutcome::RetryAfter(_)
        | ProviderOutcome::PauseAuthentication
        | ProviderOutcome::PausePolicy => return Err(Error::InvalidResult),
    };
    output.write_all(result.as_bytes()).map_err(|_| Error::Io)?;
    output.flush().map_err(|_| Error::Io)
}

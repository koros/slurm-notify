//! Shared canonical rendering fingerprint codec; no messages or delivery.
use zeroize::Zeroizing;
const PREFIX: &[u8] = b"slurm-notify-private-smtp-render-revision-v1\0";
const LIMIT: usize = 4096;
pub(super) fn encode(fields: &[(&str, &str)]) -> Result<Zeroizing<Vec<u8>>, ()> {
    let length = fields
        .iter()
        .try_fold(PREFIX.len(), |length, (tag, value)| {
            if tag.len() > u16::MAX as usize || value.len() > u32::MAX as usize {
                return None;
            }
            length
                .checked_add(2)?
                .checked_add(tag.len())?
                .checked_add(4)?
                .checked_add(value.len())
                .filter(|length| *length <= LIMIT)
        })
        .ok_or(())?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(length));
    bytes.extend_from_slice(PREFIX);
    for (tag, value) in fields {
        bytes.extend_from_slice(&(tag.len() as u16).to_be_bytes());
        bytes.extend_from_slice(tag.as_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    Ok(bytes)
}

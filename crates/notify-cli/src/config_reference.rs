//! Shared local resource reference fingerprints; contains no resolved secrets.
pub(super) fn reference(purpose: &[u8], values: &[&str]) -> String {
    use ring::digest::{Context, SHA256};
    let mut hash = Context::new(&SHA256);
    hash.update(b"slurm-notify-private-config-reference-v1\0");
    hash.update(purpose);
    for value in values {
        hash.update(&(value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    hash.finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

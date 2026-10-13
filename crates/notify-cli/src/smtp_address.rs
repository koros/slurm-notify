//! Shared strict ASCII addr-spec validation.
use lettre::Address;
pub(super) fn address(value: &str) -> Result<Address, ()> {
    // Deliberately narrower than the library: ASCII addr-spec only, no display
    // names, quoted local parts, address literals, lists, or SMTPUTF8.
    if value.is_empty()
        || value.len() > 254
        || !value.bytes().all(|b| (33..=126).contains(&b))
        || value.bytes().any(|b| b"<>[](),;:\\\"".contains(&b))
    {
        return Err(());
    }
    let parsed: Address = value.parse().map_err(|_| ())?;
    if parsed.user().len() > 64 || parsed.domain().len() > 253 {
        return Err(());
    }
    Ok(parsed)
}

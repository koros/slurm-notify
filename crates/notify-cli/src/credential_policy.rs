//! Shared bounded AUTH input policy; no lookup or network operations.
pub(super) fn valid(text: Option<&str>, bound: usize) -> Option<&str> {
    text.filter(|value| {
        !value.is_empty()
            && value.len() <= bound
            && value.bytes().all(|byte| (32..=126).contains(&byte))
    })
}

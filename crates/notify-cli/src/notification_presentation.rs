//! Private presentation content; destination renderers own their wire format.
pub(super) struct Notification {
    pub(super) heading: String,
    pub(super) fields: Vec<String>,
    pub(super) context: Vec<String>,
}
impl Notification {
    pub(super) fn fallback(&self) -> String {
        std::iter::once(self.heading.as_str())
            .chain(self.fields.iter().map(String::as_str))
            .chain(self.context.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

//! Explicit HTTP destination dispatch; generic webhooks never become email implicitly.
use crate::{email_resources, supervised_provider::Diagnostic, webhook_resolution};
use zeroize::Zeroizing;
pub(super) enum Resources {
    Webhook(webhook_resolution::Resources),
    Email(email_resources::Resources),
}
pub(super) enum Request {
    Webhook(webhook_resolution::PublicRequest),
    Email(email_resources::Request),
}
impl From<webhook_resolution::Resources> for Resources {
    fn from(v: webhook_resolution::Resources) -> Self {
        Self::Webhook(v)
    }
}
impl Resources {
    #[cfg(test)]
    pub(super) fn into_frame(
        self,
        id: &str,
        body: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, webhook_resolution::Error> {
        match self {
            Self::Webhook(v) => v.into_frame(id, body),
            _ => Err(webhook_resolution::Error::Request),
        }
    }

    pub(super) fn public_revision(&self) -> Option<&str> {
        match self {
            Self::Webhook(v) => v.public_revision(),
            Self::Email(v) => Some(v.revision()),
        }
    }
    pub(super) fn email(&self) -> Option<&email_resources::Resources> {
        match self {
            Self::Email(v) => Some(v),
            _ => None,
        }
    }
    pub(super) fn email_profile(&self) -> Option<&crate::email_profile::Profile> {
        self.email().map(|v| &v.profile)
    }
    pub(super) fn slack_options(&self) -> Option<crate::job_enrichment::SlackOptions> {
        match self {
            Self::Webhook(v) => v.slack_options(),
            _ => None,
        }
    }
    pub(super) fn safe_origin(&self) -> Option<String> {
        match self {
            Self::Webhook(v) => v.safe_origin(),
            Self::Email(v) => url::Url::parse(&v.profile.endpoint())
                .ok()
                .map(|u| u.origin().ascii_serialization()),
        }
    }
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Webhook(v) => v.kind(),
            Self::Email(v) => v.profile.kind(),
        }
    }
    pub(super) fn wire_payload(&self, bytes: &[u8]) -> Result<Vec<u8>, webhook_resolution::Error> {
        match self {
            Self::Webhook(v) => v.wire_payload(bytes),
            Self::Email(v) => v
                .wire_payload(bytes)
                .map_err(|_| webhook_resolution::Error::Request),
        }
    }
    pub(super) fn accepts_public_payload(&self, bytes: &[u8]) -> bool {
        match self {
            Self::Webhook(v) => v.accepts_public_payload(bytes),
            Self::Email(v) => v.wire_payload(bytes).is_ok(),
        }
    }
    pub(super) fn synthetic_request(
        &self,
        id: &str,
        bytes: &[u8],
    ) -> Result<webhook_resolution::PublicRequest, webhook_resolution::Error> {
        match self {
            Self::Webhook(v) => v.synthetic_request(id, bytes),
            _ => Err(webhook_resolution::Error::Request),
        }
    }
    pub(super) fn prepare_public(
        &self,
        store: &crate::storage::Store,
        item: &crate::storage::DueDelivery,
        lookup: impl FnOnce(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    ) -> Result<Request, Diagnostic> {
        match self {
            Self::Webhook(v) => v.prepare_public(store, item).map(Request::Webhook),
            Self::Email(v) => v.prepare(store, item, lookup).map(Request::Email),
        }
    }
}
impl Request {
    pub(super) fn supervise(
        self,
        command: &mut std::process::Command,
    ) -> crate::supervised_provider::Report {
        match self {
            Self::Webhook(v) => v.supervise(command),
            Self::Email(v) => v.supervise(command),
        }
    }
}

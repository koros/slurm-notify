//! Shared validated bound SMTP resources; no ambient resource lookup.
use crate::{
    smtp_auth::Credentials,
    smtp_provider_revision::{self, Descriptor, Profile},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Policy,
    Profile,
    Trust,
    Credentials,
}

// No Debug/Clone: credentials and resolved identities must not enter diagnostics.
// Constructor validates actual DER trust content and authentication identity;
// a profile fingerprint alone is insufficient evidence of resolved resources.
pub(super) struct Resolved {
    provider: Descriptor,
    roots: Vec<Vec<u8>>,
    credentials: Credentials,
}
impl Resolved {
    pub(super) fn new(
        bound: &Descriptor,
        profile: &Profile<'_>,
        roots: &[&[u8]],
        username: &str,
        password: &str,
    ) -> Result<Self, Error> {
        if !bound.matches(profile).map_err(|_| Error::Profile)?
            || username != profile.authentication_identity
        {
            return Err(Error::Profile);
        }
        if smtp_provider_revision::trust_fingerprint(roots).map_err(|_| Error::Trust)?
            != profile.trust_fingerprint
        {
            return Err(Error::Trust);
        }
        let credentials =
            Credentials::new(Some(username), Some(password)).map_err(|_| Error::Credentials)?;
        Ok(Self {
            provider: Descriptor::new(profile).map_err(|_| Error::Profile)?,
            roots: roots.iter().map(|root| root.to_vec()).collect(),
            credentials,
        })
    }
    pub(super) fn provider(&self) -> &Descriptor {
        &self.provider
    }
    pub(super) fn from_owned(
        bound: &Descriptor,
        profile: &Profile<'_>,
        roots: &[&[u8]],
        credentials: Credentials,
    ) -> Result<Self, Error> {
        if !bound.matches(profile).map_err(|_| Error::Profile)?
            || !credentials.matches_identity(profile.authentication_identity)
        {
            return Err(Error::Profile);
        }
        if smtp_provider_revision::trust_fingerprint(roots).map_err(|_| Error::Trust)?
            != profile.trust_fingerprint
        {
            return Err(Error::Trust);
        }
        Ok(Self {
            provider: Descriptor::new(profile).map_err(|_| Error::Profile)?,
            roots: roots.iter().map(|root| root.to_vec()).collect(),
            credentials,
        })
    }
    pub(super) fn into_parts(self) -> (Descriptor, Vec<Vec<u8>>, Credentials) {
        (self.provider, self.roots, self.credentials)
    }
}

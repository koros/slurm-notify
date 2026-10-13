//! Private provider identity only; no credential lookup, persistence or network I/O.
use ring::digest::{digest, Context, SHA256};
use zeroize::Zeroizing;

const PREFIX: &[u8] = b"slurm-notify-private-smtp-provider-revision-v1\0";
const TAGS: [&str; 10] = [
    "destination",
    "relay",
    "port",
    "tls",
    "server-name",
    "trust-reference",
    "trust-sha256",
    "authentication",
    "authentication-identity",
    "credential-reference",
];
const LIMIT: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TlsMode {
    #[cfg(test)]
    Implicit,
    RequiredStartTls,
}

// References identify caller-managed profiles, never paths or secret values.
// No Debug/Clone: identities and canonical bytes are private metadata.
pub(super) struct Profile<'a> {
    pub(super) destination: &'a str,
    pub(super) relay: &'a str,
    pub(super) port: u16,
    pub(super) tls: TlsMode,
    pub(super) server_name: &'a str,
    pub(super) trust_reference: &'a str,
    pub(super) trust_fingerprint: &'a str,
    pub(super) authentication_identity: &'a str,
    pub(super) credential_reference: &'a str,
}
pub(super) struct Descriptor {
    #[cfg(test)]
    canonical: Zeroizing<Vec<u8>>,
    token: String,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Policy,
    Trust,
    #[cfg(test)]
    Format,
    Limit,
    #[cfg(test)]
    Fingerprint,
}

impl Descriptor {
    pub(super) fn new(profile: &Profile<'_>) -> Result<Self, Error> {
        validate(profile)?;
        let port = profile.port.to_string();
        let values = [
            profile.destination,
            profile.relay,
            port.as_str(),
            match profile.tls {
                #[cfg(test)]
                TlsMode::Implicit => "implicit",
                TlsMode::RequiredStartTls => "required-starttls",
            },
            profile.server_name,
            profile.trust_reference,
            profile.trust_fingerprint,
            "plain-v1",
            profile.authentication_identity,
            profile.credential_reference,
        ];
        let size = values
            .iter()
            .zip(TAGS)
            .try_fold(PREFIX.len(), |size, (value, tag)| {
                size.checked_add(2 + tag.len() + 4 + value.len())
                    .filter(|n| *n <= LIMIT)
            })
            .ok_or(Error::Limit)?;
        let mut canonical = Zeroizing::new(Vec::with_capacity(size));
        canonical.extend_from_slice(PREFIX);
        for (tag, value) in TAGS.iter().zip(values) {
            canonical.extend_from_slice(&(tag.len() as u16).to_be_bytes());
            canonical.extend_from_slice(tag.as_bytes());
            canonical.extend_from_slice(&(value.len() as u32).to_be_bytes());
            canonical.extend_from_slice(value.as_bytes());
        }
        let token = hex(digest(&SHA256, &canonical).as_ref());
        Ok(Self {
            #[cfg(test)]
            canonical,
            token,
        })
    }
    #[cfg(test)]
    pub(super) fn from_stored(token: &str, canonical: Zeroizing<Vec<u8>>) -> Result<Self, Error> {
        let profile = decode(&canonical)?;
        let validated = Self::new(&profile)?;
        if validated.token() != token || validated.canonical() != canonical.as_slice() {
            return Err(Error::Fingerprint);
        }
        Ok(Self {
            canonical,
            token: token.to_owned(),
        })
    }
    #[cfg(test)]
    pub(super) fn canonical(&self) -> &[u8] {
        &self.canonical
    }
    pub(super) fn token(&self) -> &str {
        &self.token
    }
    #[cfg(test)]
    pub(super) fn profile(&self) -> Profile<'_> {
        decode(&self.canonical).expect("validated provider descriptor")
    }
    #[cfg(test)]
    pub(super) fn matches(&self, profile: &Profile<'_>) -> Result<bool, Error> {
        let candidate = Self::new(profile)?;
        Ok(candidate.token() == self.token() && candidate.canonical() == self.canonical())
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8] = b"0123456789abcdef";
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 15) as usize] as char);
    }
    text
}
fn reference(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn host(value: &str) -> bool {
    if value.is_empty() || value.len() > 253 {
        return false;
    }
    if let Ok(address) = value.parse::<std::net::IpAddr>() {
        return address.to_string() == value;
    }
    // Numeric-looking invalid IPv4 addresses must not become DNS names.
    if value.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return false;
    }
    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}
fn validate(profile: &Profile<'_>) -> Result<(), Error> {
    if !reference(profile.destination, 64)
        || !host(profile.relay)
        || profile.port == 0
        || !host(profile.server_name)
        || !reference(profile.trust_reference, 128)
        || !fingerprint(profile.trust_fingerprint)
        || !reference(profile.credential_reference, 128)
        || profile.authentication_identity.is_empty()
        || profile.authentication_identity.len() > 64
        || !profile
            .authentication_identity
            .bytes()
            .all(|b| (32..=126).contains(&b))
    {
        return Err(Error::Policy);
    }
    Ok(())
}
#[cfg(test)]
fn decode(bytes: &[u8]) -> Result<Profile<'_>, Error> {
    if bytes.len() > LIMIT {
        return Err(Error::Limit);
    }
    let mut rest = bytes.strip_prefix(PREFIX).ok_or(Error::Format)?;
    let mut values = [""; 10];
    for (index, tag) in TAGS.iter().enumerate() {
        let size = rest.get(..2).ok_or(Error::Format)?;
        let size = u16::from_be_bytes([size[0], size[1]]) as usize;
        rest = &rest[2..];
        if rest.get(..size) != Some(tag.as_bytes()) {
            return Err(Error::Format);
        }
        rest = &rest[size..];
        let size = rest.get(..4).ok_or(Error::Format)?;
        let size = u32::from_be_bytes(size.try_into().map_err(|_| Error::Format)?) as usize;
        rest = &rest[4..];
        values[index] = std::str::from_utf8(rest.get(..size).ok_or(Error::Format)?)
            .map_err(|_| Error::Format)?;
        rest = &rest[size..];
    }
    if !rest.is_empty() || values[7] != "plain-v1" {
        return Err(Error::Format);
    }
    let port = values[2].parse::<u16>().map_err(|_| Error::Format)?;
    if port.to_string() != values[2] {
        return Err(Error::Format);
    }
    let tls = match values[3] {
        "implicit" => TlsMode::Implicit,
        "required-starttls" => TlsMode::RequiredStartTls,
        _ => return Err(Error::Format),
    };
    Ok(Profile {
        destination: values[0],
        relay: values[1],
        port,
        tls,
        server_name: values[4],
        trust_reference: values[5],
        trust_fingerprint: values[6],
        authentication_identity: values[8],
        credential_reference: values[9],
    })
}

// Fingerprint the exact validated DER trust-anchor set, independent of input
// order. Bounds precede allocation/parsing; references/passwords are excluded.
pub(super) fn trust_fingerprint(roots: &[&[u8]]) -> Result<String, Error> {
    if roots.is_empty()
        || roots.len() > 32
        || roots
            .iter()
            .try_fold(0usize, |size, root| {
                size.checked_add(root.len()).filter(|n| *n <= 65536)
            })
            .is_none()
    {
        return Err(Error::Limit);
    }
    let mut ordered = roots.to_vec();
    ordered.sort_unstable();
    let mut store = rustls::RootCertStore::empty();
    let mut hash = Context::new(&SHA256);
    hash.update(b"slurm-notify-private-smtp-trust-der-v1\0");
    let mut previous = None;
    for root in ordered {
        if previous == Some(root) || root.is_empty() {
            return Err(Error::Trust);
        }
        store
            .add(rustls::pki_types::CertificateDer::from(root))
            .map_err(|_| Error::Trust)?;
        hash.update(&(root.len() as u64).to_be_bytes());
        hash.update(root);
        previous = Some(root);
    }
    Ok(hex(hash.finish().as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Profile<'static> {
        Profile {
            destination: "smtp",
            relay: "relay.fixture.invalid",
            port: 465,
            tls: TlsMode::Implicit,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        }
    }
    #[test]
    fn canonical_golden_and_stored_round_trip_preserve_explicit_profile() {
        let original = Descriptor::new(&profile()).unwrap();
        assert_eq!(
            original.token(),
            "be09b8e7e291b2234e475705c1e4208213db2e4bd9f8e5144da9f4a0fbda9c74"
        );
        let stored = Descriptor::from_stored(
            original.token(),
            Zeroizing::new(original.canonical().to_vec()),
        )
        .unwrap();
        assert!(stored.matches(&profile()).unwrap());
        let loaded = stored.profile();
        assert_eq!(loaded.relay, "relay.fixture.invalid");
        assert_eq!(loaded.port, 465);
        assert!(loaded.tls == TlsMode::Implicit);
        assert_eq!(loaded.server_name, "smtp.fixture.invalid");
        assert_eq!(loaded.trust_reference, "fixture-trust");
        assert_eq!(loaded.authentication_identity, "fixture-user");
        assert_eq!(loaded.credential_reference, "fixture-secret");
    }
    #[test]
    fn each_provider_policy_field_changes_identity_and_mismatches_resolution() {
        let original = Descriptor::new(&profile()).unwrap();
        for field in 0..9 {
            let mut changed = profile();
            match field {
                0 => changed.destination = "other",
                1 => changed.relay = "other.fixture.invalid",
                2 => changed.port = 587,
                3 => changed.tls = TlsMode::RequiredStartTls,
                4 => changed.server_name = "other.fixture.invalid",
                5 => changed.trust_reference = "other-trust",
                6 => {
                    changed.trust_fingerprint =
                        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                }
                7 => changed.authentication_identity = "other-user",
                _ => changed.credential_reference = "other-secret",
            }
            let revised = Descriptor::new(&changed).unwrap();
            assert_ne!(original.token(), revised.token());
            assert!(!original.matches(&changed).unwrap());
        }
        let mut invalid = profile();
        invalid.port = 0;
        assert!(matches!(original.matches(&invalid), Err(Error::Policy)));
    }
    #[test]
    fn invalid_hostnames_references_identity_and_fingerprints_are_rejected() {
        for value in [
            "",
            "RELAY.invalid",
            "relay.invalid.",
            "a..invalid",
            "-a.invalid",
            "a-.invalid",
            "relay:465",
            "https://relay.invalid",
            "999.1.1.1",
            "127.00.0.1",
            "::FFFF:127.0.0.1",
            "private\r\nfixture",
            "rélais.invalid",
        ] {
            let mut invalid = profile();
            invalid.relay = value;
            assert!(matches!(Descriptor::new(&invalid), Err(Error::Policy)));
            invalid = profile();
            invalid.server_name = value;
            assert!(matches!(Descriptor::new(&invalid), Err(Error::Policy)));
        }
        for value in ["127.0.0.1", "::1", "2001:db8::1", "xn--fixture.invalid"] {
            let mut valid = profile();
            valid.relay = value;
            valid.server_name = value;
            assert!(Descriptor::new(&valid).is_ok());
        }
        for value in ["", "a/b", "../fixture", "a b", "private\r\nfixture"] {
            for field in 0..3 {
                let mut invalid = profile();
                match field {
                    0 => invalid.destination = value,
                    1 => invalid.trust_reference = value,
                    _ => invalid.credential_reference = value,
                }
                assert!(matches!(Descriptor::new(&invalid), Err(Error::Policy)));
            }
        }
        for value in ["", "a\0b", "a\nb", "λ"] {
            let mut invalid = profile();
            invalid.authentication_identity = value;
            assert!(Descriptor::new(&invalid).is_err());
        }
        for value in ["", "AAAA", "gggg"] {
            let mut invalid = profile();
            invalid.trust_fingerprint = value;
            assert!(Descriptor::new(&invalid).is_err());
        }
    }
    #[test]
    fn maximum_fields_fit_and_overlong_inputs_fail_before_encoding() {
        let name = "n".repeat(64);
        let reference = "r".repeat(128);
        let host = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        assert_eq!(host.len(), 253);
        let valid = Profile {
            destination: &name,
            relay: &host,
            port: u16::MAX,
            tls: TlsMode::RequiredStartTls,
            server_name: &host,
            trust_reference: &reference,
            trust_fingerprint: profile().trust_fingerprint,
            authentication_identity: &name,
            credential_reference: &reference,
        };
        let descriptor = Descriptor::new(&valid).unwrap();
        assert!(descriptor.canonical().len() <= LIMIT);
        assert!(Descriptor::from_stored(
            descriptor.token(),
            Zeroizing::new(descriptor.canonical().to_vec())
        )
        .is_ok());
        for field in 0..7 {
            let overlong = "x".repeat(match field {
                0 | 5 => 65,
                1 | 2 => 254,
                3 | 6 => 129,
                _ => 65,
            });
            let mut invalid = profile();
            match field {
                0 => invalid.destination = &overlong,
                1 => invalid.relay = &overlong,
                2 => invalid.server_name = &overlong,
                3 => invalid.trust_reference = &overlong,
                4 => invalid.trust_fingerprint = &overlong,
                5 => invalid.authentication_identity = &overlong,
                _ => invalid.credential_reference = &overlong,
            }
            assert!(Descriptor::new(&invalid).is_err());
        }
    }
    #[test]
    fn stored_corruption_truncation_unknown_versions_and_noncanonical_port_fail() {
        let original = Descriptor::new(&profile()).unwrap();
        for length in 0..original.canonical().len() {
            assert!(Descriptor::from_stored(
                original.token(),
                Zeroizing::new(original.canonical()[..length].to_vec())
            )
            .is_err());
        }
        for bytes in [vec![0; LIMIT + 1], [original.canonical(), b"x"].concat()] {
            assert!(Descriptor::from_stored(original.token(), Zeroizing::new(bytes)).is_err());
        }
        assert!(matches!(
            Descriptor::from_stored(
                &"b".repeat(64),
                Zeroizing::new(original.canonical().to_vec())
            ),
            Err(Error::Fingerprint)
        ));
        for (old, new) in [
            (b"plain-v1".as_slice(), b"plain-v2".as_slice()),
            (b"implicit", b"insecure"),
            (b"465", b"000"),
        ] {
            let mut bytes = original.canonical().to_vec();
            let at = bytes
                .windows(old.len())
                .position(|value| value == old)
                .unwrap();
            bytes[at..at + old.len()].copy_from_slice(new);
            assert!(Descriptor::from_stored(original.token(), Zeroizing::new(bytes)).is_err());
        }
        let mut bytes = original.canonical().to_vec();
        *bytes.last_mut().unwrap() = 255;
        assert!(Descriptor::from_stored(original.token(), Zeroizing::new(bytes)).is_err());
        assert_eq!(format!("{:?}", Error::Fingerprint), "Fingerprint");
    }
    #[test]
    fn trust_set_fingerprint_is_order_stable_bounded_and_content_sensitive() {
        let first = rcgen::generate_simple_self_signed(vec!["first.fixture.invalid".into()])
            .unwrap()
            .cert
            .der()
            .to_vec();
        let second = rcgen::generate_simple_self_signed(vec!["second.fixture.invalid".into()])
            .unwrap()
            .cert
            .der()
            .to_vec();
        let a = trust_fingerprint(&[&first, &second]).unwrap();
        assert_eq!(a, trust_fingerprint(&[&second, &first]).unwrap());
        assert_ne!(a, trust_fingerprint(&[&first]).unwrap());
        assert!(matches!(
            trust_fingerprint(&[&first, &first]),
            Err(Error::Trust)
        ));
        assert!(matches!(
            trust_fingerprint(&[b"invalid"]),
            Err(Error::Trust)
        ));
        assert!(matches!(trust_fingerprint(&[b""]), Err(Error::Trust)));
        assert!(matches!(trust_fingerprint(&[]), Err(Error::Limit)));
        assert!(matches!(
            trust_fingerprint(&[&vec![0; 65537]]),
            Err(Error::Limit)
        ));
        assert!(matches!(
            trust_fingerprint(&vec![first.as_slice(); 33]),
            Err(Error::Limit)
        ));
        assert!(matches!(
            trust_fingerprint(&[&vec![0; 32769], &vec![1; 32768]]),
            Err(Error::Limit)
        ));
        let descriptor = Descriptor::new(&Profile {
            trust_fingerprint: &a,
            ..profile()
        })
        .unwrap();
        assert!(descriptor.profile().trust_fingerprint == a);
    }
}

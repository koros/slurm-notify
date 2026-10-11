//! Private rendering fingerprint only; no durable binding or provider revision.
use super::{
    smtp_message, smtp_metadata,
    smtp_render::{self, JobVisibility},
    smtp_routes::Plan,
};
use ring::digest::{digest, SHA256};
use zeroize::Zeroizing;

const PREFIX: &[u8] = b"slurm-notify-private-smtp-render-revision-v1\0";
const LIMIT: usize = 4096;

// Addresses/canonical policy bytes are private data, not diagnostics.
pub(super) struct Descriptor {
    canonical: Zeroizing<Vec<u8>>,
    token: String,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Route,
    Limit,
}

impl Descriptor {
    pub(super) fn from_plan(plan: &Plan<'_>, name: &str) -> Result<Self, Error> {
        let route = plan.route(name).ok_or(Error::Route)?;
        let disclosure = match route.visibility {
            JobVisibility::Hidden => "hidden",
            JobVisibility::Identifier => "identifier",
        };
        let canonical = encode(&[
            ("destination", route.name),
            ("sender", route.metadata.sender),
            ("recipient", route.metadata.recipient),
            ("namespace", route.metadata.namespace),
            ("disclosure", disclosure),
            ("renderer", smtp_render::VERSION),
            ("identity", smtp_metadata::VERSION),
            ("domain", smtp_message::ID_DOMAIN),
        ])?;
        let fingerprint = digest(&SHA256, &canonical);
        let mut token = String::with_capacity(64);
        const HEX: &[u8] = b"0123456789abcdef";
        for byte in fingerprint.as_ref() {
            token.push(HEX[(byte >> 4) as usize] as char);
            token.push(HEX[(byte & 15) as usize] as char);
        }
        Ok(Self { canonical, token })
    }
    pub(super) fn canonical(&self) -> &[u8] {
        &self.canonical
    }
    pub(super) fn token(&self) -> &str {
        &self.token
    }
}

// Fixed field order; unsigned big-endian u16 tag/u32 value byte lengths.
// Check the complete bound before allocation, copying or hashing.
fn encode(fields: &[(&str, &str)]) -> Result<Zeroizing<Vec<u8>>, Error> {
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
        .ok_or(Error::Limit)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smtp_routes::{Metadata, Route};
    fn route(name: &'static str) -> Route<'static> {
        Route {
            name,
            visibility: JobVisibility::Hidden,
            metadata: Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: "fixture",
            },
        }
    }
    fn descriptor(route: &Route<'_>) -> Descriptor {
        let names = [route.name];
        let plan = Plan::new(&names, std::slice::from_ref(route), 1).unwrap();
        Descriptor::from_plan(&plan, route.name).unwrap()
    }
    #[test]
    fn canonical_encoding_and_fingerprint_match_independent_golden_vectors() {
        assert!(
            encode(&[("a", "bc"), ("de", "f")]).unwrap().as_slice()
                == [PREFIX, b"\0\x01a\0\0\0\x02bc\0\x02de\0\0\0\x01f"].concat()
        );
        let descriptor = descriptor(&route("smtp"));
        assert_eq!(
            descriptor.token(),
            "59108550b9f079f7dee2905a234ec85e3d26ac2afea376ef718c2e178ce5b9a0"
        );
        assert_eq!(descriptor.token().len(), 64);
        assert!(descriptor
            .token()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }
    #[test]
    fn every_route_policy_field_changes_the_revision() {
        let original = descriptor(&route("smtp"));
        for field in 0..5 {
            let mut changed = route("smtp");
            match field {
                0 => changed.name = "other",
                1 => changed.metadata.sender = "other@fixture.invalid",
                2 => changed.metadata.recipient = "other@fixture.invalid",
                3 => changed.metadata.namespace = "changed",
                _ => changed.visibility = JobVisibility::Identifier,
            }
            let revised = descriptor(&changed);
            assert!(revised.canonical() != original.canonical());
            assert_ne!(revised.token(), original.token());
        }
        let mut fields = [
            ("destination", "smtp"),
            ("sender", "s@fixture.invalid"),
            ("recipient", "r@fixture.invalid"),
            ("namespace", "fixture"),
            ("disclosure", "hidden"),
            ("renderer", smtp_render::VERSION),
            ("identity", smtp_metadata::VERSION),
            ("domain", smtp_message::ID_DOMAIN),
        ];
        assert!(encode(&fields).unwrap().as_slice() == original.canonical());
        for field in 5..8 {
            let saved = fields[field].1;
            fields[field].1 = "changed-v2";
            let changed = encode(&fields).unwrap();
            assert!(
                digest(&SHA256, &changed).as_ref()
                    != digest(&SHA256, original.canonical()).as_ref()
            );
            fields[field].1 = saved;
        }
    }
    #[test]
    fn equivalent_plans_ignore_order_plain_routes_and_operational_limits() {
        let forward = [route("a"), route("b")];
        let reverse = [route("b"), route("a")];
        let first = Plan::new(&["a", "b", "plain"], &forward, 2).unwrap();
        let second = Plan::new(&["b", "a"], &reverse, 64).unwrap();
        for name in ["a", "b"] {
            let first = Descriptor::from_plan(&first, name).unwrap();
            let second = Descriptor::from_plan(&second, name).unwrap();
            assert_eq!(first.token(), second.token());
            assert!(first.canonical() == second.canonical());
        }
        assert!(matches!(
            Descriptor::from_plan(&first, "plain"),
            Err(Error::Route)
        ));
        assert!(matches!(
            Descriptor::from_plan(&first, "private-missing-fixture"),
            Err(Error::Route)
        ));
        assert_eq!(format!("{:?}", Error::Route), "Route");
    }
    #[test]
    fn maximum_validated_fields_fit_and_invalid_inputs_never_form_descriptors() {
        let name = "n".repeat(64);
        let namespace = "x".repeat(64);
        let address = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        assert_eq!(address.len(), 254);
        let routes = [Route {
            name: &name,
            visibility: JobVisibility::Identifier,
            metadata: Metadata {
                sender: &address,
                recipient: &address,
                namespace: &namespace,
            },
        }];
        let names = [name.as_str()];
        let plan = Plan::new(&names, &routes, 1).unwrap();
        let descriptor = Descriptor::from_plan(&plan, &name).unwrap();
        assert!(descriptor.canonical().len() <= LIMIT);
        for case in 0..3 {
            let mut invalid = route("smtp");
            match case {
                0 => invalid.name = "private\r\nfixture",
                1 => invalid.metadata.sender = "private-invalid-fixture",
                _ => invalid.metadata.namespace = "private\r\nfixture",
            }
            let names = [invalid.name];
            let routes = [invalid];
            let failure = match Plan::new(&names, &routes, 1) {
                Err(error) => error,
                Ok(_) => panic!("invalid descriptor policy accepted"),
            };
            assert!(!format!("{failure:?}").contains("private"));
        }
    }

    #[test]
    fn encoding_checks_full_byte_limit_and_ambiguous_boundaries() {
        let size = LIMIT - PREFIX.len() - 2 - 1 - 4;
        let at_limit = "x".repeat(size);
        assert_eq!(encode(&[("a", &at_limit)]).unwrap().len(), LIMIT);
        let over_limit = "x".repeat(size + 1);
        assert!(matches!(encode(&[("a", &over_limit)]), Err(Error::Limit)));
        let oversized_tag = "x".repeat(u16::MAX as usize + 1);
        assert!(matches!(encode(&[(&oversized_tag, "")]), Err(Error::Limit)));
        assert!(encode(&[("ab", "c")]).unwrap() != encode(&[("a", "bc")]).unwrap());
        assert!(matches!(
            encode(&[("a", &at_limit), ("b", "")]),
            Err(Error::Limit)
        ));
        assert_eq!(format!("{:?}", Error::Limit), "Limit");
    }
}

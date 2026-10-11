//! Private injected SMTP credential resolution; no ambient environment access.
use crate::smtp_auth::Credentials;
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Reference,
    Identity,
    Lookup,
    Credentials,
}
fn reference(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic() || byte == b'_' || index > 0 && byte.is_ascii_digit()
        })
}
// The injected source owns lookup allocation/deadline policy. Returned values are
// bounded again here; no source error text or secret values enter diagnostics.
pub(super) fn resolve(
    identity: &str,
    username_reference: &str,
    password_reference: &str,
    mut lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
) -> Result<Credentials, Error> {
    if !reference(username_reference) || !reference(password_reference) {
        return Err(Error::Reference);
    }
    if identity.is_empty()
        || identity.len() > 64
        || !identity.bytes().all(|byte| (32..=126).contains(&byte))
    {
        return Err(Error::Identity);
    }
    let username = lookup(username_reference, 64).map_err(|_| Error::Lookup)?;
    if username.as_slice() != identity.as_bytes() {
        return Err(Error::Identity);
    }
    let password = lookup(password_reference, 128).map_err(|_| Error::Lookup)?;
    if password.is_empty() || password.len() > 128 {
        return Err(Error::Credentials);
    }
    let password = std::str::from_utf8(&password).map_err(|_| Error::Credentials)?;
    Credentials::new(Some(identity), Some(password)).map_err(|_| Error::Credentials)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits the unit test bodies.
    use super::*;
    #[test]
    fn owned_credentials_preserve_exact_bytes_and_allow_restart_rotation() {
        let mut source = Zeroizing::new(b"first fixture ".to_vec());
        let mut requests = Vec::new();
        let mut load = |source: &[u8]| {
            resolve(
                "fixture-user",
                "USER_REF",
                "PASSWORD_REF",
                |name, ceiling| {
                    requests.push((name.to_owned(), ceiling));
                    Ok(Zeroizing::new(if name == "USER_REF" {
                        b"fixture-user".to_vec()
                    } else {
                        source.to_vec()
                    }))
                },
            )
            .unwrap()
        };
        let first = load(&source);
        source.clear();
        source.extend_from_slice(b"rotated fixture");
        let second = load(&source);
        drop(source);
        assert!(first.into_plain().as_slice() == b"\0fixture-user\0first fixture ");
        assert!(second.into_plain().as_slice() == b"\0fixture-user\0rotated fixture");
        assert_eq!(
            requests,
            vec![
                ("USER_REF".into(), 64),
                ("PASSWORD_REF".into(), 128),
                ("USER_REF".into(), 64),
                ("PASSWORD_REF".into(), 128),
            ]
        );
    }
    #[test]
    fn references_and_expected_identity_fail_before_lookup() {
        for bad in ["", "1REF", "BAD-NAME", "BAD\nNAME", "é", &"x".repeat(129)] {
            for (username, password) in [(bad, "PASSWORD"), ("USER", bad)] {
                assert!(matches!(
                    resolve("fixture", username, password, |_, _| {
                        panic!("invalid reference must not resolve")
                    }),
                    Err(Error::Reference)
                ));
            }
        }
        for bad in ["", "bad\nidentity", "é", &"x".repeat(65)] {
            assert!(matches!(
                resolve(bad, "USER", "PASSWORD", |_, _| {
                    panic!("invalid identity must not resolve")
                }),
                Err(Error::Identity)
            ));
        }
        let name = "x".repeat(128);
        let identity = "u".repeat(64);
        let credentials = resolve(&identity, &name, "_PASSWORD9", |_, ceiling| {
            Ok(Zeroizing::new(vec![
                if ceiling == 64 { b'u' } else { b'p' };
                ceiling
            ]))
        })
        .unwrap();
        assert_eq!(credentials.into_plain().len(), 194);
    }
    #[test]
    fn username_lookup_and_mismatch_never_request_password() {
        let mut calls = 0;
        assert!(matches!(
            resolve("fixture", "USER", "PASSWORD", |_, _| {
                calls += 1;
                Err(())
            }),
            Err(Error::Lookup)
        ));
        assert_eq!(calls, 1);
        for wrong in [vec![], b"other".to_vec(), vec![b'x'; 65], vec![0xff]] {
            let mut calls = 0;
            assert!(matches!(
                resolve("fixture", "USER", "PASSWORD", |name, _| {
                    calls += 1;
                    assert_eq!(name, "USER");
                    Ok(Zeroizing::new(wrong.clone()))
                }),
                Err(Error::Identity)
            ));
            assert_eq!(calls, 1);
        }
    }
    #[test]
    fn password_failures_are_static_and_do_not_trim_or_fallback() {
        for password in [
            None,
            Some(vec![]),
            Some(vec![b'p'; 129]),
            Some(vec![0xff]),
            Some(b"private-fixture\n".to_vec()),
            Some(b"private-fixture\0".to_vec()),
        ] {
            let result = resolve("fixture", "USER", "PASSWORD", |name, _| {
                if name == "USER" {
                    Ok(Zeroizing::new(b"fixture".to_vec()))
                } else {
                    password.clone().map(Zeroizing::new).ok_or(())
                }
            });
            let expected = if password.is_none() {
                "Lookup"
            } else {
                "Credentials"
            };
            assert_eq!(format!("{:?}", result.err().unwrap()), expected);
        }
    }
}

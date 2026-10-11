//! Private injected webhook resources; no installed environment/config contract.
use crate::{credential_resolution::reference, transport, webhook};
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Reference,
    Policy,
    Lookup,
    Endpoint,
    Authorization,
    Request,
}
// No Debug/Clone; keep original URL/header bytes, including secret paths/queries.
pub(super) struct Resources {
    endpoint: Zeroizing<Vec<u8>>,
    authorization: Option<Zeroizing<Vec<u8>>>,
    timeout: Duration,
    payload_limit: usize,
}
impl Resources {
    pub(super) fn into_frame(
        self,
        event_id: &str,
        payload: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        if event_id.is_empty()
            || event_id.len() > 128
            || !event_id.bytes().all(|byte| (32..=126).contains(&byte))
            || payload.len() > self.payload_limit
        {
            return Err(Error::Request);
        }
        let millis = self.timeout.as_millis().to_string();
        transport::frame(
            &[
                &self.endpoint,
                self.authorization
                    .as_deref()
                    .map(|bytes| bytes.as_slice())
                    .unwrap_or_default(),
                event_id.as_bytes(),
                payload,
                millis.as_bytes(),
            ],
            20 + 4096 + 4096 + 128 + 16384 + 5,
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::Request)
    }
}
pub(super) fn resolve(
    endpoint_reference: &str,
    authorization_reference: Option<&str>,
    timeout: Duration,
    payload_limit: usize,
    mut lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
) -> Result<Resources, Error> {
    if !reference(endpoint_reference)
        || authorization_reference.is_some_and(|name| !reference(name))
    {
        return Err(Error::Reference);
    }
    if !(1..=30).contains(&timeout.as_secs())
        || timeout.subsec_nanos() != 0
        || !(1024..=16384).contains(&payload_limit)
    {
        return Err(Error::Policy);
    }
    let endpoint = lookup(endpoint_reference, 4096).map_err(|_| Error::Lookup)?;
    if endpoint.is_empty()
        || endpoint.len() > 4096
        || !endpoint.iter().all(|byte| (33..=126).contains(byte))
    {
        return Err(Error::Endpoint);
    }
    let text = std::str::from_utf8(&endpoint).map_err(|_| Error::Endpoint)?;
    webhook::endpoint_url(text).map_err(|_| Error::Endpoint)?;
    let authorization = if let Some(name) = authorization_reference {
        let bytes = lookup(name, 4096).map_err(|_| Error::Lookup)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| Error::Authorization)?;
        if !webhook::header_value(text) {
            return Err(Error::Authorization);
        }
        Some(bytes)
    } else {
        None
    };
    Ok(Resources {
        endpoint,
        authorization,
        timeout,
        payload_limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_owned_resources_survive_source_rotation_and_use_existing_frame() {
        let mut endpoint = Zeroizing::new(b"https://fixture.invalid/hook?token=fixture".to_vec());
        let mut calls = vec![];
        let resources = resolve(
            "URL",
            Some("AUTH"),
            Duration::from_secs(2),
            1024,
            |name, limit| {
                calls.push((name.to_owned(), limit));
                Ok(Zeroizing::new(if name == "URL" {
                    endpoint.to_vec()
                } else {
                    b"Bearer fixture ".to_vec()
                }))
            },
        )
        .unwrap();
        endpoint.clear();
        drop(endpoint);
        assert_eq!(calls, vec![("URL".into(), 4096), ("AUTH".into(), 4096)]);
        let frame = resources
            .into_frame("opaque-fixture", b"fixture body")
            .unwrap();
        transport::serve(frame.as_slice(), std::io::sink(), frame.len(), |fields| {
            assert!(fields[0] == b"https://fixture.invalid/hook?token=fixture");
            assert!(fields[1] == b"Bearer fixture ");
            assert!(
                fields[2] == b"opaque-fixture"
                    && fields[3] == b"fixture body"
                    && fields[4] == b"2000"
            );
            Ok(crate::delivery::ProviderOutcome::Delivered)
        })
        .unwrap();
    }
    #[test]
    fn preflight_rejects_references_and_policy_without_lookup() {
        for (url, auth) in [("1URL", None), ("URL", Some("BAD-NAME"))] {
            assert!(matches!(
                resolve(url, auth, Duration::from_secs(1), 1024, |_, _| panic!(
                    "preflight lookup"
                )),
                Err(Error::Reference)
            ));
        }
        for (timeout, limit) in [
            (Duration::ZERO, 1024),
            (Duration::from_secs(31), 1024),
            (Duration::from_millis(1500), 1024),
            (Duration::from_secs(1), 1023),
            (Duration::from_secs(1), 16385),
        ] {
            assert!(matches!(
                resolve("URL", None, timeout, limit, |_, _| panic!("policy lookup")),
                Err(Error::Policy)
            ));
        }
    }
    #[test]
    fn invalid_endpoint_stops_before_authorization_with_static_errors() {
        for bad in [
            b"".to_vec(),
            vec![b'x'; 4097],
            vec![0xff],
            b"http://remote.invalid/".to_vec(),
            b"https://user:fixture@fixture.invalid/".to_vec(),
            b"https://fixture.invalid/#fragment".to_vec(),
            b"https://fixture.invalid/\n".to_vec(),
        ] {
            let error = resolve(
                "URL",
                Some("AUTH"),
                Duration::from_secs(1),
                1024,
                |name, _| {
                    assert_eq!(name, "URL");
                    Ok(Zeroizing::new(bad.clone()))
                },
            )
            .err()
            .unwrap();
            assert_eq!(format!("{error:?}"), "Endpoint");
        }
        assert!(matches!(
            resolve("URL", None, Duration::from_secs(1), 1024, |_, _| Err(())),
            Err(Error::Lookup)
        ));
    }
    #[test]
    fn configured_auth_never_falls_back_and_frame_checks_request_budget() {
        for bad in [
            None,
            Some(vec![]),
            Some(vec![b'x'; 4097]),
            Some(vec![0xff]),
            Some(b"fixture\r\n".to_vec()),
        ] {
            let error = resolve(
                "URL",
                Some("AUTH"),
                Duration::from_secs(1),
                1024,
                |name, _| {
                    if name == "URL" {
                        Ok(Zeroizing::new(b"http://127.0.0.1/hook".to_vec()))
                    } else {
                        bad.clone().map(Zeroizing::new).ok_or(())
                    }
                },
            )
            .err()
            .unwrap();
            assert_eq!(
                error,
                if bad.is_none() {
                    Error::Lookup
                } else {
                    Error::Authorization
                }
            );
        }
        let load = || {
            resolve("URL", None, Duration::from_secs(30), 16384, |name, _| {
                assert_eq!(name, "URL");
                Ok(Zeroizing::new(b"http://127.0.0.1/hook".to_vec()))
            })
            .unwrap()
        };
        for id in ["", "bad\n", &"x".repeat(129)] {
            assert!(matches!(load().into_frame(id, b"x"), Err(Error::Request)));
        }
        assert!(matches!(
            load().into_frame("fixture", &vec![0; 16385]),
            Err(Error::Request)
        ));
        assert!(load().into_frame(&"x".repeat(128), &vec![0; 16384]).is_ok());
    }
}

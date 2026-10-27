//! API-specific acceptance states; never equate a generic 2xx with inbox delivery.
use crate::email_profile::Provider;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Accepted,
    Rejected,
    Unknown,
    Retry(u64),
    Authentication,
    Policy,
    NotAttempted,
}
impl Outcome {
    pub(super) fn encode(self) -> String {
        match self {
            Self::Accepted => "ACCEPTED\n".into(),
            Self::Rejected => "REJECTED\n".into(),
            Self::Unknown => "UNKNOWN\n".into(),
            Self::Retry(n) => format!("RETRY {n}\n"),
            Self::Authentication => "AUTHENTICATION\n".into(),
            Self::Policy => "POLICY\n".into(),
            Self::NotAttempted => "NOT_ATTEMPTED\n".into(),
        }
    }
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, crate::provider_frame::Error> {
        let outcome = match bytes {
            b"ACCEPTED\n" => Self::Accepted,
            b"REJECTED\n" => Self::Rejected,
            b"UNKNOWN\n" => Self::Unknown,
            b"AUTHENTICATION\n" => Self::Authentication,
            b"POLICY\n" => Self::Policy,
            b"NOT_ATTEMPTED\n" => Self::NotAttempted,
            _ => {
                let text = std::str::from_utf8(bytes)
                    .map_err(|_| crate::provider_frame::Error::InvalidResult)?;
                let n = text
                    .strip_prefix("RETRY ")
                    .and_then(|s| s.strip_suffix('\n'))
                    .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|n| *n <= 4_102_444_799)
                    .ok_or(crate::provider_frame::Error::InvalidResult)?;
                Self::Retry(n)
            }
        };
        if outcome.encode().as_bytes() != bytes {
            return Err(crate::provider_frame::Error::InvalidResult);
        }
        Ok(outcome)
    }
}
pub(super) fn classify(
    kind: Provider,
    status: u16,
    body: &[u8],
    retry_after: Option<&str>,
    now: i64,
) -> Outcome {
    let json = if body.len() <= 8192 {
        crate::config_decode::decode(body).ok()
    } else {
        None
    };
    if status == 401 {
        return Outcome::Authentication;
    }
    let quota = kind == Provider::Gmail
        && status == 403
        && json
            .as_ref()
            .and_then(|v| crate::config_validate::object(v).ok())
            .and_then(|v| crate::config_validate::get(v, "error").ok())
            .and_then(|v| crate::config_validate::object(v).ok())
            .and_then(|v| crate::config_validate::get(v, "errors").ok())
            .is_some_and(|v| {
                if let crate::config_decode::Value::Array(errors) = v {
                    errors.iter().any(|e| {
                        crate::config_validate::object(e)
                            .ok()
                            .and_then(|v| crate::config_validate::get(v, "reason").ok())
                            .and_then(|v| crate::config_validate::text(v).ok())
                            .is_some_and(|r| {
                                matches!(
                                    r,
                                    "rateLimitExceeded"
                                        | "userRateLimitExceeded"
                                        | "dailyLimitExceeded"
                                )
                            })
                    })
                } else {
                    false
                }
            });
    if status == 403 && !quota {
        return Outcome::Policy;
    }
    if quota || status == 429 || status == 408 || (500..=599).contains(&status) {
        if let Some(value) = retry_after {
            let seconds = if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
                value.parse::<u64>().ok()
            } else {
                httpdate::parse_http_date(value)
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs().saturating_sub(now.max(0) as u64))
            };
            if let Some(seconds) = seconds {
                if now < 0
                    || (now as u64)
                        .checked_add(seconds)
                        .is_none_or(|n| n > 4_102_444_799)
                {
                    return Outcome::Policy;
                }
                return Outcome::Retry(seconds);
            }
        }
        return if status == 429 || quota {
            Outcome::Retry(0)
        } else {
            Outcome::Unknown
        };
    }
    if kind == Provider::SendGrid && status == 202 {
        return Outcome::Accepted;
    }
    if kind != Provider::SendGrid && status == 200 {
        let id = json
            .as_ref()
            .and_then(|v| crate::config_validate::object(v).ok())
            .and_then(|v| crate::config_validate::get(v, "id").ok())
            .and_then(|v| crate::config_validate::text(v).ok());
        return if id.is_some_and(|s| {
            !s.is_empty() && s.len() <= 512 && s.bytes().all(|b| (33..=126).contains(&b))
        }) {
            Outcome::Accepted
        } else {
            Outcome::Unknown
        };
    }
    if (200..=299).contains(&status) {
        Outcome::Unknown
    } else {
        Outcome::Rejected
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_acceptance_and_strict_acknowledgement() {
        for kind in [Provider::SendGrid, Provider::Mailgun, Provider::Gmail] {
            assert_eq!(
                classify(
                    kind,
                    if kind == Provider::SendGrid { 202 } else { 200 },
                    b"{\"id\":\"fixture-id\"}",
                    None,
                    0
                ),
                Outcome::Accepted
            );
            for body in [
                b"{}".as_slice(),
                b"not-json",
                b"{\"id\":\"\"}",
                b"{\"id\":\"a\",\"id\":\"b\"}",
            ] {
                if kind != Provider::SendGrid {
                    assert_eq!(classify(kind, 200, body, None, 0), Outcome::Unknown);
                }
            }
            assert_eq!(classify(kind, 204, b"", None, 0), Outcome::Unknown);
            for (code, want) in [
                (400, Outcome::Rejected),
                (401, Outcome::Authentication),
                (403, Outcome::Policy),
                (429, Outcome::Retry(0)),
                (500, Outcome::Unknown),
                (302, Outcome::Rejected),
            ] {
                assert_eq!(classify(kind, code, b"", None, 0), want);
            }
            assert_eq!(
                classify(kind, 429, b"", Some("600"), 0),
                Outcome::Retry(600)
            );
            assert_eq!(
                classify(kind, 429, b"", Some("4102444800"), 0),
                Outcome::Policy
            );
            assert_eq!(
                classify(kind, 503, b"", Some("Thu, 01 Jan 1970 00:10:00 GMT"), 0),
                Outcome::Retry(600)
            );
            assert_eq!(classify(kind, 503, b"", Some("bad"), 0), Outcome::Unknown);
        }
        assert_eq!(
            classify(
                Provider::Gmail,
                403,
                b"{\"error\":{\"errors\":[{\"reason\":\"userRateLimitExceeded\"}]}}",
                None,
                0
            ),
            Outcome::Retry(0)
        );
        for o in [
            Outcome::Accepted,
            Outcome::Rejected,
            Outcome::Unknown,
            Outcome::Retry(600),
            Outcome::Authentication,
            Outcome::Policy,
            Outcome::NotAttempted,
        ] {
            assert_eq!(Outcome::decode(o.encode().as_bytes()).unwrap(), o);
        }
        for bad in [
            b"OK\n".as_slice(),
            b"RETRY -1\n",
            b"RETRY 00\n",
            b"RETRY 4102444800\n",
            b"ACCEPTED\nextra",
        ] {
            assert!(Outcome::decode(bad).is_err());
        }
    }
}

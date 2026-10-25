//! Shared local configuration validation; no environment or filesystem lookup.
use crate::{
    config_decode::{self, Value},
    control_validation,
};
use zeroize::Zeroizing;

type Object = [(Zeroizing<String>, Value)];
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Decode(config_decode::Error),
    Fields,
    Version,
    Policy,
    Text,
    Structure,
    Reference,
    Duplicate,
    Controls,
}
// The tree retains zeroizing metadata; no Debug/Clone or public config contract.
pub(super) struct Draft {
    pub(super) value: Value,
    #[cfg(any(test, target_os = "linux", target_os = "macos"))]
    pub(super) controls: crate::runtime_mapping::Plan,
}
pub(super) fn object(value: &Value) -> Result<&Object, Error> {
    match value {
        Value::Object(fields) => Ok(fields),
        _ => Err(Error::Fields),
    }
}
pub(super) fn get<'a>(fields: &'a Object, key: &str) -> Result<&'a Value, Error> {
    fields
        .iter()
        .find(|(name, _)| name.as_str() == key)
        .map(|(_, v)| v)
        .ok_or(Error::Fields)
}
fn fields<'a>(value: &'a Value, required: &[&str], optional: &[&str]) -> Result<&'a Object, Error> {
    let values = object(value)?;
    if required.iter().any(|key| get(values, key).is_err())
        || values
            .iter()
            .any(|(key, _)| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(Error::Fields);
    }
    Ok(values)
}
pub(super) fn text(value: &Value) -> Result<&str, Error> {
    match value {
        Value::Text(s) => Ok(s),
        _ => Err(Error::Text),
    }
}
fn number(value: &Value, low: u64, high: u64) -> Result<u64, Error> {
    match value {
        Value::Integer(n) if (low..=high).contains(n) => Ok(*n),
        _ => Err(Error::Controls),
    }
}
fn alias(s: &str) -> Result<(), Error> {
    if s.is_empty()
        || s.len() > 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::Text);
    }
    Ok(())
}
fn env(value: &Value) -> Result<(), Error> {
    let s = text(value)?;
    if s.is_empty()
        || s.len() > 128
        || !s
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(Error::Text);
    }
    Ok(())
}
fn path(value: &Value) -> Result<(), Error> {
    let s = text(value)?;
    if !s.starts_with('/') || s.len() > 4096 || s.chars().any(char::is_control) {
        return Err(Error::Text);
    }
    Ok(())
}
pub(super) fn list(value: &Value, minimum: usize, maximum: usize) -> Result<&[Value], Error> {
    match value {
        Value::Array(a) if (minimum..=maximum).contains(&a.len()) => Ok(a),
        _ => Err(Error::Structure),
    }
}
fn strings(values: &[Value]) -> Result<Vec<&str>, Error> {
    let mut result = Vec::new();
    for value in values {
        let s = text(value)?;
        if result.contains(&s) {
            return Err(Error::Duplicate);
        }
        result.push(s);
    }
    Ok(result)
}
fn host(value: &Value) -> Result<(), Error> {
    let s = text(value)?;
    if s.is_empty()
        || s.len() > 253
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".:-".contains(&b))
    {
        return Err(Error::Text);
    }
    Ok(())
}
fn mailbox(value: &Value) -> Result<(), Error> {
    let s = text(value)?;
    if s.len() > 254
        || !s.bytes().all(|b| (33..=126).contains(&b))
        || !s
            .split_once('@')
            .is_some_and(|(left, right)| !left.is_empty() && !right.is_empty())
    {
        return Err(Error::Text);
    }
    Ok(())
}
fn token(value: &Value) -> Result<&str, Error> {
    let s = text(value)?;
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Text);
    }
    Ok(s)
}
pub(super) fn validate(bytes: &[u8]) -> Result<Draft, Error> {
    let value = config_decode::decode(bytes).map_err(Error::Decode)?;
    let root = fields(
        &value,
        &[
            "schema_version",
            "monitor",
            "storage",
            "providers",
            "routes",
            "delivery",
            "maintenance",
        ],
        &[],
    )?;
    if !matches!(get(root, "schema_version")?, Value::Integer(1)) {
        return Err(Error::Version);
    }
    let monitor = fields(
        get(root, "monitor")?,
        &[
            "mode",
            "cluster_id",
            "user_jobs_only",
            "initial_observation",
            "poll_seconds",
            "query_timeout_seconds",
            "accounting_lookback_seconds",
            "accounting_overlap_seconds",
            "command_output_limit_bytes",
            "retained_identity_limit",
            "retained_event_limit",
            "stale_after_seconds",
        ],
        &[],
    )?;
    if text(get(monitor, "mode")?)? != "local"
        || !matches!(get(monitor, "user_jobs_only")?, Value::Bool(true))
        || !matches!(
            text(get(monitor, "initial_observation")?)?,
            "suppress" | "emit"
        )
    {
        return Err(Error::Policy);
    }
    let cluster = text(get(monitor, "cluster_id")?)?;
    alias(cluster)?;
    let storage = fields(get(root, "storage")?, &["path"], &[])?;
    path(get(storage, "path")?)?;
    let providers = object(get(root, "providers")?)?;
    if providers.is_empty() || providers.len() > 64 {
        return Err(Error::Structure);
    }
    let mut smtp = Vec::new();
    let mut aliases = Vec::new();
    for (name, value) in providers {
        alias(name)?;
        aliases.push(name.as_str());
        let kind = text(get(object(value)?, "type")?)?;
        match kind {
            "webhook" | "slack" => {
                let p = fields(
                    value,
                    &[
                        "type",
                        "url_env",
                        "request_timeout_seconds",
                        "payload_limit_bytes",
                    ],
                    &["authorization_env", "trust_der_files"],
                )?;
                env(get(p, "url_env")?)?;
                if let Ok(auth) = get(p, "authorization_env") {
                    env(auth)?;
                }
                if let Ok(roots) = get(p, "trust_der_files") {
                    let roots = list(roots, 1, 32)?;
                    strings(roots)?;
                    for root in roots {
                        path(root)?;
                    }
                }
                number(get(p, "request_timeout_seconds")?, 1, 30)?;
                number(get(p, "payload_limit_bytes")?, 1024, 16384)?;
            }
            "smtp" => {
                smtp.push(name.as_str());
                let p = fields(
                    value,
                    &[
                        "type",
                        "relay",
                        "port",
                        "tls_mode",
                        "server_name",
                        "trust_der_files",
                        "authentication",
                        "sender",
                        "recipient",
                        "visibility",
                        "identity_namespace",
                        "request_timeout_milliseconds",
                    ],
                    &[],
                )?;
                if text(get(p, "tls_mode")?)? != "required_starttls"
                    || !matches!(text(get(p, "visibility")?)?, "hidden" | "included")
                {
                    return Err(Error::Policy);
                }
                host(get(p, "relay")?)?;
                host(get(p, "server_name")?)?;
                mailbox(get(p, "sender")?)?;
                mailbox(get(p, "recipient")?)?;
                alias(text(get(p, "identity_namespace")?)?)?;
                let roots = list(get(p, "trust_der_files")?, 1, 32)?;
                strings(roots)?;
                for root in roots {
                    path(root)?;
                }
                let auth = fields(
                    get(p, "authentication")?,
                    &["identity", "username_env", "password_env"],
                    &[],
                )?;
                let identity = text(get(auth, "identity")?)?;
                if identity.is_empty()
                    || identity.len() > 64
                    || !identity.bytes().all(|b| (32..=126).contains(&b))
                {
                    return Err(Error::Text);
                }
                env(get(auth, "username_env")?)?;
                env(get(auth, "password_env")?)?;
                number(get(p, "port")?, 1, 65535)?;
                number(get(p, "request_timeout_milliseconds")?, 1, 30000)?;
            }
            _ => return Err(Error::Policy),
        }
    }
    let mut rules = Vec::new();
    let mut events = Vec::new();
    let mut destinations = Vec::new();
    for route in list(get(root, "routes")?, 0, 128)? {
        let r = fields(route, &["events", "send_to"], &[])?;
        events.push(strings(list(get(r, "events")?, 1, 9)?)?);
        destinations.push(strings(list(get(r, "send_to")?, 1, 64)?)?);
    }
    for (events, destinations) in events.iter().zip(&destinations) {
        rules.push(crate::routing_validation::Rule {
            events,
            destinations,
        });
    }
    crate::routing_validation::validate(&aliases, &rules).map_err(|e| match e {
        crate::routing_validation::Error::Duplicate => Error::Duplicate,
        crate::routing_validation::Error::Reference => Error::Reference,
        crate::routing_validation::Error::Limit => Error::Structure,
    })?;
    let mut required = vec![
        "max_attempts",
        "initial_backoff_seconds",
        "max_backoff_seconds",
        "jitter_seconds",
        "jitter_seed",
        "batch_limit",
    ];
    if !smtp.is_empty() {
        required.extend(["prepared_smtp_message_limit", "smtp_permissions"]);
    }
    let delivery = fields(get(root, "delivery")?, &required, &[])?;
    if !smtp.is_empty() {
        number(get(delivery, "prepared_smtp_message_limit")?, 1, 256)?;
        let mut seen = std::collections::HashSet::new();
        for permission in list(get(delivery, "smtp_permissions")?, 0, 64)? {
            let p = fields(
                permission,
                &[
                    "cluster_id",
                    "destination",
                    "rendering_revision",
                    "provider_revision",
                ],
                &[],
            )?;
            let c = text(get(p, "cluster_id")?)?;
            let d = text(get(p, "destination")?)?;
            alias(c)?;
            alias(d)?;
            if c != cluster || !smtp.contains(&d) {
                return Err(Error::Reference);
            }
            let rendering = token(get(p, "rendering_revision")?)?;
            let provider = token(get(p, "provider_revision")?)?;
            if !seen.insert((c, d, rendering, provider)) {
                return Err(Error::Duplicate);
            }
        }
    }
    let maintenance = fields(
        get(root, "maintenance")?,
        &[
            "completed_delivery_retention_seconds",
            "cleanup_batch_limit",
            "health_sample_limit",
            "smtp_revision_cleanup_limit",
        ],
        &[],
    )?;
    let n = |o, k| number(get(o, k)?, 0, u64::MAX);
    let seed = text(get(delivery, "jitter_seed")?)?;
    if seed.len() > 1 && seed.starts_with('0') {
        return Err(Error::Controls);
    }
    let input = control_validation::Controls {
        poll: n(monitor, "poll_seconds")?,
        query_timeout: n(monitor, "query_timeout_seconds")?,
        lookback: n(monitor, "accounting_lookback_seconds")?,
        overlap: n(monitor, "accounting_overlap_seconds")?,
        output: n(monitor, "command_output_limit_bytes")?,
        identities: n(monitor, "retained_identity_limit")?,
        events: n(monitor, "retained_event_limit")?,
        stale: n(monitor, "stale_after_seconds")?,
        attempts: n(delivery, "max_attempts")?,
        initial_backoff: n(delivery, "initial_backoff_seconds")?,
        max_backoff: n(delivery, "max_backoff_seconds")?,
        jitter: n(delivery, "jitter_seconds")?,
        jitter_seed: seed,
        batch: n(delivery, "batch_limit")?,
        retention: n(maintenance, "completed_delivery_retention_seconds")?,
        cleanup: n(maintenance, "cleanup_batch_limit")?,
        health_sample: n(maintenance, "health_sample_limit")?,
        revision_cleanup: n(maintenance, "smtp_revision_cleanup_limit")?,
    };
    control_validation::validate(input).map_err(|_| Error::Controls)?;
    #[cfg(any(test, target_os = "linux", target_os = "macos"))]
    let controls = crate::runtime_mapping::map(input).map_err(|_| Error::Controls)?;
    Ok(Draft {
        value,
        #[cfg(any(test, target_os = "linux", target_os = "macos"))]
        controls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value as Json};
    fn fixture() -> Json {
        serde_json::from_slice(include_bytes!(
            "../../../docs/proposals/examples/local-mixed-config.json"
        ))
        .unwrap()
    }
    fn check(source: &Json) -> Result<Draft, Error> {
        validate(&serde_json::to_vec(source).unwrap())
    }
    fn rejected(source: &Json, error: Error) {
        assert!(matches!(check(source), Err(actual) if actual == error));
    }
    fn set(source: &mut Json, path: &str, value: Json) {
        *source.pointer_mut(path).unwrap() = value;
    }
    #[test]
    fn examples_produce_owned_tree_and_operational_plan_without_resources() {
        let mut bytes =
            include_bytes!("../../../docs/proposals/examples/local-mixed-config.json").to_vec();
        let draft = validate(&bytes).unwrap();
        bytes.fill(0);
        drop(bytes);
        assert_eq!(draft.controls.poll.as_secs(), 60);
        assert_eq!(draft.controls.maintenance.retry.jitter_seed, 42);
        let root = object(&draft.value).unwrap();
        let monitor = object(get(root, "monitor").unwrap()).unwrap();
        assert_eq!(
            text(get(monitor, "cluster_id").unwrap()).unwrap(),
            "research-cluster"
        );
        assert!(validate(include_bytes!(
            "../../../docs/proposals/examples/local-webhook-config.json"
        ))
        .is_ok());
        let mut optional = fixture();
        optional["providers"]["primary"]
            .as_object_mut()
            .unwrap()
            .remove("authorization_env");
        assert!(check(&optional).is_ok());
        let mut empty = fixture();
        empty["routes"] = json!([]);
        empty["delivery"]["smtp_permissions"] = json!([]);
        assert!(check(&empty).is_ok());
    }
    #[test]
    fn accepts_explicit_slack_example_without_changing_generic_shape() {
        let bytes = include_bytes!("../../../contracts/config/slack-v1.example.json");
        assert!(validate(bytes).is_ok());
        let mut source: Json = serde_json::from_slice(bytes).unwrap();
        source["providers"]["slack"]["unapproved_private_field"] = json!("fixture");
        rejected(&source, Error::Fields);
    }
    #[test]
    fn rejects_missing_unknown_and_wrong_types_at_each_object_boundary() {
        for path in [
            "",
            "/monitor",
            "/storage",
            "/providers/primary",
            "/providers/research_email",
            "/providers/research_email/authentication",
            "/routes/0",
            "/delivery",
            "/delivery/smtp_permissions/0",
            "/maintenance",
        ] {
            let mut source = fixture();
            let target = source.pointer_mut(path).unwrap().as_object_mut().unwrap();
            target.insert("unapproved_private_field".into(), json!("fixture"));
            rejected(&source, Error::Fields);
            let mut source = fixture();
            let target = source.pointer_mut(path).unwrap().as_object_mut().unwrap();
            let key = target
                .keys()
                .find(|key| key.as_str() != "authorization_env")
                .unwrap()
                .clone();
            target.remove(&key);
            assert!(check(&source).is_err());
            let mut source = fixture();
            set(&mut source, path, json!([]));
            assert!(check(&source).is_err());
        }
        for version in [json!(2), json!(true), json!("1")] {
            let mut source = fixture();
            source["schema_version"] = version;
            rejected(&source, Error::Version);
        }
        for (path, value) in [
            ("/monitor/mode", json!("remote")),
            ("/monitor/user_jobs_only", json!(false)),
            ("/monitor/initial_observation", json!("default")),
            ("/providers/primary/type", json!("unknown_provider")),
            ("/providers/research_email/tls_mode", json!("implicit_tls")),
            ("/providers/research_email/visibility", json!("all")),
        ] {
            let mut source = fixture();
            set(&mut source, path, value);
            rejected(&source, Error::Policy);
        }
    }
    #[test]
    fn validates_numeric_ranges_types_seed_and_cross_field_relationships() {
        let manifest: Json = serde_json::from_slice(include_bytes!(
            "../../../docs/proposals/examples/config-limits-v1.json"
        ))
        .unwrap();
        for group in [
            "integer_ranges",
            "webhook_integer_ranges",
            "smtp_integer_ranges",
        ] {
            for (key, bounds) in manifest[group].as_object().unwrap() {
                let path = match group {
                    "integer_ranges" => format!("/{}", key.replace('.', "/")),
                    "webhook_integer_ranges" => format!("/providers/primary/{key}"),
                    _ => format!("/providers/research_email/{key}"),
                };
                for value in [
                    json!(true),
                    json!("10"),
                    json!(bounds["maximum"].as_u64().unwrap() + 1),
                ] {
                    let mut source = fixture();
                    set(&mut source, &path, value);
                    rejected(&source, Error::Controls);
                }
                let min = bounds["minimum"].as_u64().unwrap();
                if min > 0 {
                    let mut source = fixture();
                    set(&mut source, &path, json!(min - 1));
                    rejected(&source, Error::Controls);
                }
            }
        }
        for (path, value) in [
            ("/monitor/query_timeout_seconds", json!(60)),
            ("/monitor/accounting_overlap_seconds", json!(3600)),
            ("/monitor/stale_after_seconds", json!(59)),
            ("/delivery/max_backoff_seconds", json!(4)),
            ("/delivery/jitter_seconds", json!(301)),
            ("/delivery/jitter_seed", json!("01")),
            ("/delivery/jitter_seed", json!("18446744073709551616")),
            ("/delivery/jitter_seed", json!(42)),
        ] {
            let mut source = fixture();
            set(&mut source, path, value);
            assert!(check(&source).is_err());
        }
        let mut source = fixture();
        source["delivery"]["jitter_seed"] = json!(u64::MAX.to_string());
        assert_eq!(
            check(&source)
                .unwrap()
                .controls
                .maintenance
                .retry
                .jitter_seed,
            u64::MAX
        );
    }
    #[test]
    fn rejects_route_permission_and_trust_reference_mismatches() {
        for (path, value, error) in [
            ("/routes/0/events", json!(["JOB_UNKNOWN"]), Error::Reference),
            (
                "/routes/0/events",
                json!(["JOB_FAILED", "JOB_FAILED"]),
                Error::Duplicate,
            ),
            ("/routes/0/send_to", json!(["missing"]), Error::Reference),
            (
                "/routes/0/send_to",
                json!(["primary", "primary"]),
                Error::Duplicate,
            ),
            (
                "/delivery/smtp_permissions/0/destination",
                json!("primary"),
                Error::Reference,
            ),
            (
                "/delivery/smtp_permissions/0/cluster_id",
                json!("other"),
                Error::Reference,
            ),
            (
                "/delivery/smtp_permissions/0/rendering_revision",
                json!("A".repeat(64)),
                Error::Text,
            ),
            (
                "/providers/research_email/trust_der_files",
                json!(["/fixture/a", "/fixture/a"]),
                Error::Duplicate,
            ),
        ] {
            let mut source = fixture();
            set(&mut source, path, value);
            rejected(&source, error);
        }
        let mut source = fixture();
        let pair = source["delivery"]["smtp_permissions"][0].clone();
        source["delivery"]["smtp_permissions"]
            .as_array_mut()
            .unwrap()
            .push(pair);
        rejected(&source, Error::Duplicate);
        let mut source = fixture();
        source["providers"]
            .as_object_mut()
            .unwrap()
            .remove("research_email");
        source["routes"][0]["send_to"] = json!(["primary"]);
        // SMTP-only delivery fields must not silently survive a webhook-only configuration.
        rejected(&source, Error::Fields);
    }
    #[test]
    fn rejects_bad_text_and_specific_collection_limits_with_static_errors() {
        for (path, value) in [
            ("/monitor/cluster_id", json!("bad alias")),
            ("/storage/path", json!("relative")),
            ("/storage/path", json!("/fixture/\u{001b}")),
            ("/providers/primary/url_env", json!("1REFERENCE")),
            ("/providers/primary/authorization_env", json!("")),
            ("/providers/research_email/relay", json!("host\nfixture")),
            ("/providers/research_email/sender", json!("missing-at")),
            (
                "/providers/research_email/authentication/identity",
                json!("x".repeat(65)),
            ),
        ] {
            let mut source = fixture();
            set(&mut source, path, value);
            rejected(&source, Error::Text);
        }
        for (path, value) in [
            ("/providers", json!({})),
            ("/routes/0/events", json!([])),
            (
                "/providers/research_email/trust_der_files",
                json!((0..33).map(|i| format!("/fixture/{i}")).collect::<Vec<_>>()),
            ),
            (
                "/delivery/smtp_permissions",
                json!((0..65)
                    .map(|_| fixture()["delivery"]["smtp_permissions"][0].clone())
                    .collect::<Vec<_>>()),
            ),
        ] {
            let mut source = fixture();
            set(&mut source, path, value);
            rejected(&source, Error::Structure);
        }
        assert_eq!(
            format!("{:?}", validate(b"{\"private\":false}").err().unwrap()),
            "Fields"
        );
        rejected(&json!(null), Error::Decode(config_decode::Error::Root));
    }
}

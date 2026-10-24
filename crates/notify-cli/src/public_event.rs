//! Private implementation of the adopted minimal public event payload; no sender.
#[cfg(test)]
use ring::rand::{SecureRandom, SystemRandom};
#[cfg(test)]
use serde_json::json;

#[cfg(test)]
pub(super) fn random() -> Result<[u8; 16], ()> {
    let mut bytes = [0; 16];
    SystemRandom::new().fill(&mut bytes).map_err(|_| ())?;
    Ok(bytes)
}
#[cfg(test)]
pub(super) fn identifier(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut id = String::from("evt_");
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}
pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.starts_with("evt_")
        && id.as_bytes()[4..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        && id.as_bytes()[16] == b'4'
        && b"89ab".contains(&id.as_bytes()[20])
}
#[cfg(test)]
pub(super) fn payload(
    id: &str,
    cluster: &str,
    job: &str,
    state: &str,
    observed_at: i64,
) -> Result<Vec<u8>, ()> {
    if !valid_id(id)
        || cluster.is_empty()
        || cluster.len() > 64
        || !cluster
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(());
    }
    let (allocation, task) = match job.split_once('_') {
        Some((allocation, task)) => (allocation, Some(task)),
        None => (job, None),
    };
    let decimal = |value: &str| {
        !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && (value.len() == 1 || !value.starts_with('0'))
    };
    if !decimal(allocation)
        || allocation == "0"
        || task.is_some_and(|task| !decimal(task))
        || job.len() > 16384
    {
        return Err(());
    }
    let (event_type, severity) = match state {
        "RUNNING" => ("JOB_STARTED", "INFO"),
        "COMPLETED" => ("JOB_COMPLETED", "INFO"),
        "FAILED" => ("JOB_FAILED", "ERROR"),
        "TIMEOUT" => ("JOB_TIMEOUT", "ERROR"),
        "CANCELLED" => ("JOB_CANCELLED", "WARN"),
        "OUT_OF_MEMORY" => ("JOB_OUT_OF_MEMORY", "ERROR"),
        "NODE_FAIL" => ("JOB_NODE_FAILURE", "ERROR"),
        "BOOT_FAIL" => ("JOB_BOOT_FAILURE", "ERROR"),
        "DEADLINE" => ("JOB_DEADLINE", "ERROR"),
        _ => return Err(()),
    };
    if observed_at < 0 {
        return Err(());
    }
    let observed = time::OffsetDateTime::from_unix_timestamp(observed_at).map_err(|_| ())?;
    if !(1970..=9999).contains(&observed.year()) {
        return Err(());
    }
    let observed = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        observed.year(),
        u8::from(observed.month()),
        observed.day(),
        observed.hour(),
        observed.minute(),
        observed.second()
    );
    // Private attempt-context strings have no independently established timezone.
    // Do not manufacture source event/submission/start timestamps from them.
    let bytes = serde_json::to_vec(&json!({
        "schema_version":"1.0", "event_id":id, "source":"slurm", "cluster_id":cluster,
        "event_type":event_type, "severity":severity, "observed_at":observed, "occurred_at":null,
        "job":{"id":allocation,"array_task_id":task,"submitted_at":null,"started_at":null,"state":state}
    })).map_err(|_| ())?;
    if bytes.len() > 16384 {
        return Err(());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits unit-test bodies.
    use super::*;
    #[test]
    fn uuid_bits_and_payload_allowlist_do_not_export_private_attempt_context() {
        let id = identifier([0; 16]);
        assert_eq!(id, "evt_00000000000040008000000000000000");
        let schema: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../contracts/events/webhook-v1.schema.json"
        ))
        .unwrap();
        for state in [
            "RUNNING",
            "COMPLETED",
            "FAILED",
            "TIMEOUT",
            "CANCELLED",
            "OUT_OF_MEMORY",
            "NODE_FAIL",
            "BOOT_FAIL",
            "DEADLINE",
        ] {
            let frame = payload(&id, "cluster", "123_0", state, 0).unwrap();
            let value: serde_json::Value = serde_json::from_slice(&frame).unwrap();
            assert_eq!(value["job"]["array_task_id"], "0");
            assert_eq!(value["observed_at"], "1970-01-01T00:00:00Z");
            assert!(
                value["job"]["submitted_at"].is_null()
                    && value["job"]["started_at"].is_null()
                    && value["occurred_at"].is_null()
            );
            let rule = schema["oneOf"]
                .as_array()
                .unwrap()
                .iter()
                .find(|rule| rule["properties"]["job"]["properties"]["state"]["const"] == state)
                .unwrap();
            assert_eq!(
                value["event_type"],
                rule["properties"]["event_type"]["const"]
            );
            assert_eq!(value["severity"], rule["properties"]["severity"]["const"]);
            let expected: std::collections::BTreeSet<_> = schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap())
                .collect();
            let actual: std::collections::BTreeSet<_> = value
                .as_object()
                .unwrap()
                .keys()
                .map(|name| name.as_str())
                .collect();
            assert_eq!(actual, expected);
            assert_eq!(value.as_object().unwrap().len(), 9);
            assert_eq!(value["job"].as_object().unwrap().len(), 5);
        }
        let example: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../contracts/events/webhook-v1.example.json"
        ))
        .unwrap();
        let actual = payload(
            "evt_6ea80b9f47bd40d89c0b2a190d53028f",
            "research-cluster",
            "12345_7",
            "COMPLETED",
            1791637212,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&actual).unwrap(),
            example
        );
        assert!(valid_id(&identifier(random().unwrap())));
        for job in ["0", "01", "1_00", "1_-1", "1_1_2", "1.batch", "1_[0-4]", ""] {
            assert!(payload(&id, "cluster", job, "COMPLETED", 0).is_err());
        }
        for state in ["PENDING", "PREEMPTED", "UNKNOWN"] {
            assert!(payload(&id, "cluster", "1", state, 0).is_err());
        }
        assert!(payload(&id, "cluster", "1", "COMPLETED", -1).is_err());
        assert!(payload(&id, "cluster", "1", "COMPLETED", i64::MAX).is_err());
        assert!(payload("internal-v1:private", "cluster", "1", "COMPLETED", 0).is_err());
        assert!(payload(&id, "private\ncluster", "1", "COMPLETED", 0).is_err());
    }
}

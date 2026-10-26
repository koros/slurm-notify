//! Deterministic v1 Slack rendering of frozen public events; no new private fields.
use serde_json::{json, Value};

pub(super) fn render(payload: &[u8]) -> Result<Vec<u8>, ()> {
    if payload.len() > 16384 {
        return Err(());
    }
    let value: Value = serde_json::from_slice(payload).map_err(|_| ())?;
    let field = |key| value.get(key).and_then(Value::as_str).ok_or(());
    let id = field("event_id")?;
    if !crate::public_event::valid_id(id) || field("schema_version")? != "1.0" {
        return Err(());
    }
    let text = if field("event_type")? == "TEST" {
        format!("Slurm Notify test\nSynthetic Slurm Notify test; no scheduler data.\nEvent: {id}")
    } else {
        let cluster = field("cluster_id")?;
        let job = value.get("job").ok_or(())?;
        let allocation = job.get("id").and_then(Value::as_str).ok_or(())?;
        let state = job.get("state").and_then(Value::as_str).ok_or(())?;
        let task = match job.get("array_task_id").ok_or(())? {
            Value::Null => String::new(),
            Value::String(task) => format!("_{task}"),
            _ => return Err(()),
        };
        let identifier = format!("{allocation}{task}");
        // Reuse the canonical allowlist to reject malformed IDs/states/cluster names.
        crate::public_event::payload(id, cluster, &identifier, state, 0)?;
        let observed = field("observed_at")?;
        if observed.len() != 20
            || !observed
                .bytes()
                .all(|b| b.is_ascii_digit() || b"-:TZ".contains(&b))
        {
            return Err(());
        }
        format!("Slurm Notify: {state}\nCluster: {cluster}\nJob: {identifier}\nObserved: {observed}\nEvent: {id}")
    };
    // Keep well below Slack's text truncation threshold and suppress markup/unfurls.
    if text.len() > 4000 {
        return Err(());
    }
    serde_json::to_vec(
        &json!({"text":text,"mrkdwn":false,"unfurl_links":false,"unfurl_media":false}),
    )
    .map_err(|_| ())
}

// The existing private HTTPS frame binds the ID separately from the JSON body.
// Slack carries it in plain text; keep the generic event_id binding mandatory
// unless the body is exactly the bounded Slack shape.
pub(super) fn valid_wire(value: &Value, id: &str) -> bool {
    crate::public_event::valid_id(id)
        && value.as_object().is_some_and(|object| {
            object.len() == 4 || (object.len() == 5 && crate::slack_blocks::valid_wire(value))
        })
        && ["mrkdwn", "unfurl_links", "unfurl_media"]
            .iter()
            .all(|key| value.get(key) == Some(&Value::Bool(false)))
        && value
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.len() <= 4000 && text.ends_with(&format!("\nEvent: {id}")))
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits unit-test bodies.
    use super::*;
    #[test]
    fn deterministic_allowlist_and_synthetic_rendering() {
        let id = crate::public_event::identifier([0; 16]);
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
            let bytes = crate::public_event::payload(&id, "cluster_a", "123_0", state, 0).unwrap();
            let mut event: Value = serde_json::from_slice(&bytes).unwrap();
            event["private_script"] = json!("secret-fixture");
            let rendered = render(&serde_json::to_vec(&event).unwrap()).unwrap();
            assert_eq!(rendered, render(&bytes).unwrap());
            let message: Value = serde_json::from_slice(&rendered).unwrap();
            assert!(message["text"].as_str().unwrap().contains("Job: 123_0"));
            assert!(message["text"].as_str().unwrap().contains(state));
            assert!(valid_wire(&message, &id));
            assert!(!valid_wire(
                &message,
                &crate::public_event::identifier([1; 16])
            ));
            let mut invalid = message.clone();
            invalid["mrkdwn"] = json!(true);
            assert!(!valid_wire(&invalid, &id));
            invalid = message.clone();
            invalid["extra"] = json!("field");
            assert!(!valid_wire(&invalid, &id));
            assert_eq!(message["mrkdwn"], false);
            assert_eq!(message.as_object().unwrap().len(), 4);
            assert!(!String::from_utf8(rendered)
                .unwrap()
                .contains("secret-fixture"));
        }
        let test = json!({"schema_version":"1.0","event_id":id,"event_type":"TEST"});
        assert!(
            String::from_utf8(render(&serde_json::to_vec(&test).unwrap()).unwrap())
                .unwrap()
                .contains("no scheduler data")
        );
    }
    #[test]
    fn rejects_invalid_and_oversized_events() {
        for bytes in [b"".as_slice(), b"{}", b"not-json"] {
            assert!(render(bytes).is_err());
        }
        assert!(render(&vec![b' '; 16385]).is_err());
        let bytes = crate::public_event::payload(
            &crate::public_event::identifier([0; 16]),
            "cluster",
            "1",
            "COMPLETED",
            0,
        )
        .unwrap();
        let original: Value = serde_json::from_slice(&bytes).unwrap();
        for (pointer, bad) in [
            ("/schema_version", json!("2")),
            ("/event_id", json!("bad")),
            ("/cluster_id", json!("<!channel>")),
            ("/job/id", json!("0")),
            ("/job/state", json!("bad")),
            ("/job/array_task_id", json!(12)),
            ("/observed_at", json!("bad")),
        ] {
            let mut event = original.clone();
            *event.pointer_mut(pointer).unwrap() = bad;
            assert!(render(&serde_json::to_vec(&event).unwrap()).is_err());
        }
        let mut event = original;
        event["job"]["id"] = json!("1".repeat(4000));
        assert!(render(&serde_json::to_vec(&event).unwrap()).is_err());
    }
}

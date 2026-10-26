//! Deterministic plain-text Block Kit, with an equivalent accessible fallback.
use crate::notification_presentation::Notification;
use serde_json::{json, Value};
fn blocks(message: &Notification) -> Value {
    let plain = |text: &str| json!({"type":"plain_text","text":text,"emoji":false});
    let mut blocks = vec![json!({"type":"header","text":plain(&message.heading)})];
    for fields in message.fields.chunks(10) {
        blocks.push(
            json!({"type":"section","fields":fields.iter().map(|s|plain(s)).collect::<Vec<_>>()}),
        );
    }
    blocks.push(json!({"type":"context","elements":message.context.iter().map(|s|plain(s)).collect::<Vec<_>>()}));
    Value::Array(blocks)
}
pub(super) fn render(message: &Notification) -> Result<Vec<u8>, ()> {
    let text = message.fallback();
    if message.fields.is_empty()
        || message.fields.len() > 10
        || message.context.is_empty()
        || message.context.len() > 2
        || text.len() > 4000
        || message.heading.is_empty()
        || message.heading.len() > 150
        || message
            .fields
            .iter()
            .chain(&message.context)
            .any(|s| s.is_empty() || s.len() > 2000 || s.chars().any(char::is_control))
        || message.heading.chars().any(char::is_control)
    {
        return Err(());
    }
    serde_json::to_vec(&json!({"text":text,"mrkdwn":false,"unfurl_links":false,"unfurl_media":false,"blocks":blocks(message)})).map_err(|_|())
}
// Reconstruct the only permitted block layout from its complete fallback. This
// rejects extra block types, links, markup, mentions and inconsistent hidden text.
pub(super) fn valid_wire(value: &Value) -> bool {
    let Some(text) = value["text"].as_str() else {
        return false;
    };
    let mut lines: Vec<_> = text.split('\n').map(str::to_owned).collect();
    if lines.len() < 3 {
        return false;
    }
    let heading = lines.remove(0);
    let footer = if lines[lines.len() - 2].starts_with("Observed: ") {
        2
    } else {
        1
    };
    let context = lines.split_off(lines.len() - footer);
    let message = Notification {
        heading,
        fields: lines,
        context,
    };
    render(&message)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .as_ref()
        == Some(value)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[test]
    fn complete_fallback_and_strict_plain_text_blocks() {
        let id = crate::public_event::identifier([0; 16]);
        let message = Notification {
            heading: "✅ Job completed".into(),
            fields: vec![
                "Job: 123 · Array task: 2".into(),
                "Name: &lt;!channel&gt; *literal*".into(),
            ],
            context: vec![
                "Observed: 2026-10-25T11:42:20Z".into(),
                format!("Event: {id}"),
            ],
        };
        let bytes = render(&message).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["text"], message.fallback());
        assert_eq!(value["blocks"][0]["text"]["text"], message.heading);
        assert_eq!(value["blocks"][1]["fields"][1]["type"], "plain_text");
        assert_eq!(
            value["blocks"][2]["elements"][1]["text"],
            format!("Event: {id}")
        );
        assert!(crate::slack_message::valid_wire(&value, &id));
        for (pointer, bad) in [
            ("/blocks/0/text/type", json!("mrkdwn")),
            ("/blocks/1/fields/0/text", json!("different job")),
            ("/blocks/2/elements/1/text", json!("Event: different")),
            ("/blocks/0/text/emoji", json!(true)),
            ("/mrkdwn", json!(true)),
            ("/text", json!(format!("x\nEvent: {id}"))),
        ] {
            let mut forged = value.clone();
            *forged.pointer_mut(pointer).unwrap() = bad;
            assert!(!crate::slack_message::valid_wire(&forged, &id));
        }
        let mut forged = value;
        forged["blocks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"actions"}));
        assert!(!crate::slack_message::valid_wire(&forged, &id));
        assert!(!valid_wire(&json!({})));
        for (heading, field) in [
            ("", "ok"),
            ("ok", ""),
            ("bad\nheading", "ok"),
            ("ok", "bad\tfield"),
        ] {
            assert!(render(&Notification {
                heading: heading.into(),
                fields: vec![field.into()],
                context: vec![format!("Event: {id}")]
            })
            .is_err());
        }
        for (heading, fields) in [
            ("h".repeat(151), vec!["ok".into()]),
            ("ok".into(), vec!["x".repeat(2001)]),
            ("ok".into(), vec!["x".repeat(1990); 3]),
        ] {
            assert!(render(&Notification {
                heading,
                fields,
                context: vec![format!("Event: {id}")]
            })
            .is_err());
        }
    }
    #[test]
    fn lifecycle_headings_and_started_fields_and_synthetic_test() {
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
            let payload = crate::public_event::payload(&id, "cluster", "123_0", state, 0).unwrap();
            let bytes = crate::job_enrichment::render(&payload, None, false).unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(crate::slack_message::valid_wire(&value, &id));
            let text = value["text"].as_str().unwrap();
            assert_eq!(text.contains("Finished:"), state != "RUNNING");
            assert_eq!(text.contains("Exit code"), state != "RUNNING");
            assert!(text.contains("Duration: unavailable"));
            assert!(!text.contains("Name:"));
        }
        let payload =
            serde_json::to_vec(&json!({"schema_version":"1.0","event_type":"TEST","event_id":id}))
                .unwrap();
        let bytes = crate::job_enrichment::render(&payload, None, true).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(crate::slack_message::valid_wire(&value, &id));
        assert!(value["text"]
            .as_str()
            .unwrap()
            .contains("no scheduler data"));
        assert!(!value["text"].as_str().unwrap().contains("Job:"));
    }
}

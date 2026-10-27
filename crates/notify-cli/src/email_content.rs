//! Private immutable API-email content. No provider credentials or Slack metadata.
use serde_json::Value;
use zeroize::Zeroizing;
pub(super) struct Content {
    pub(super) id: String,
    pub(super) subject: String,
    pub(super) text: Zeroizing<String>,
    pub(super) timestamp: u64,
}
impl Content {
    pub(super) fn synthetic(id: &str, timestamp: u64) -> Result<Self, ()> {
        if !crate::public_event::valid_id(id) || timestamp > 4_102_444_799 {
            return Err(());
        }
        Ok(Self {
            id: id.into(),
            subject: "Slurm Notify test".into(),
            text: Zeroizing::new(format!(
                "Synthetic Slurm Notify test; no scheduler data.\nEvent: {id}\n"
            )),
            timestamp,
        })
    }
    pub(super) fn from_event(payload: &[u8]) -> Result<Self, ()> {
        crate::slack_message::render(payload)?;
        let v: Value = serde_json::from_slice(payload).map_err(|_| ())?;
        if v["event_type"] == "TEST" {
            return Err(());
        }
        let field = |key| v[key].as_str().ok_or(());
        let observed = field("observed_at")?;
        let timestamp = crate::state_timestamp::parse(observed.strip_suffix('Z').ok_or(())?)?;
        let id = field("event_id")?;
        let state = v["job"]["state"].as_str().ok_or(())?;
        let job = v["job"]["id"].as_str().ok_or(())?;
        let task = v["job"]["array_task_id"]
            .as_str()
            .map(|s| format!("_{s}"))
            .unwrap_or_default();
        let text=format!("Slurm Notify: {state}\nCluster: {}\nJob: {job}{task}\nObserved: {observed}\nEvent: {id}\n",field("cluster_id")?);
        if text.len() > 4096 {
            return Err(());
        }
        Ok(Self {
            id: id.into(),
            subject: format!("Slurm Notify: {state}"),
            text: Zeroizing::new(text),
            timestamp: timestamp.try_into().map_err(|_| ())?,
        })
    }
    pub(super) fn mime(&self, sender: &str, recipient: &str) -> Result<Zeroizing<Vec<u8>>, ()> {
        // The SMTP builder returns DATA transparency/terminator, not raw MIME.
        // Remove exactly that framing before supplying an HTTP email API.
        let prepared = crate::smtp_message::prepare(
            sender,
            recipient,
            &self.id,
            &self.subject,
            &self.text,
            self.timestamp,
        )
        .map_err(|_| ())?;
        let data = prepared.data().strip_suffix(b".\r\n").ok_or(())?;
        let mut mime = Zeroizing::new(Vec::with_capacity(data.len()));
        for line in data.split_inclusive(|b| *b == b'\n') {
            mime.extend_from_slice(if line.starts_with(b"..") {
                &line[1..]
            } else {
                line
            });
        }
        Ok(mime)
    }
}
#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits unit-test bodies.
    use super::*;
    #[test]
    fn minimal_frozen_content_ignores_private_fields_and_has_real_mime() {
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
            let bytes = crate::public_event::payload(&id, "cluster", "123_2", state, 0).unwrap();
            let mut v: Value = serde_json::from_slice(&bytes).unwrap();
            v["job_name"] = serde_json::json!("private-fixture");
            let a = Content::from_event(&bytes).unwrap();
            let b = Content::from_event(&serde_json::to_vec(&v).unwrap()).unwrap();
            assert_eq!(a.text, b.text);
            assert!(a.text.contains("Job: 123_2"));
            assert!(!a.text.contains("private-fixture"));
            assert_eq!(
                a.mime("s@fixture.invalid", "r@fixture.invalid").unwrap(),
                b.mime("s@fixture.invalid", "r@fixture.invalid").unwrap()
            );
        }
        let mut c = Content::synthetic(&id, 0).unwrap();
        c.text = Zeroizing::new(".line\n..two\n".into());
        let mime = c.mime("s@fixture.invalid", "r@fixture.invalid").unwrap();
        assert!(mime.ends_with(b".line\r\n..two\r\n"));
        assert!(!mime.ends_with(b"\r\n.\r\n"));
        assert!(c
            .mime("bad\r\n@fixture.invalid", "r@fixture.invalid")
            .is_err());
        assert!(Content::synthetic("bad", 0).is_err());
        assert!(Content::synthetic(&id, 4_102_444_800).is_err());
        for payload in [b"{}".as_slice(), b"not-json", b"{\"event_type\":\"TEST\"}"] {
            assert!(Content::from_event(payload).is_err());
        }
        let mut v: Value = serde_json::from_slice(
            &crate::public_event::payload(&id, "cluster", "1", "FAILED", 0).unwrap(),
        )
        .unwrap();
        v["observed_at"] = serde_json::json!("2026-02-30T00:00:00Z");
        assert!(Content::from_event(&serde_json::to_vec(&v).unwrap()).is_err());
    }
}

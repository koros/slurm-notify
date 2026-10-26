//! Bounded optional Slack job detail collection; generic event contracts stay frozen.
use std::{ffi::OsString, process::Command};
#[derive(Clone, Copy)]
pub(super) struct SlackOptions {
    pub(super) include_job_name: bool,
    pub(super) payload_limit: usize,
}
pub(super) fn accounting_command(command: &mut Command, names: bool) {
    let base = "--format=Cluster,JobID,User,Submit,Start,State%32,ExitCode";
    if !command.get_args().any(|arg| arg == base) {
        return;
    }
    let format = format!(
        "{base},ElapsedRaw,End{}",
        if names { ",JobName%256" } else { "" }
    );
    let args: Vec<OsString> = command
        .get_args()
        .map(|arg| {
            if arg == base {
                format.clone().into()
            } else {
                arg.to_owned()
            }
        })
        .collect();
    let mut replacement = Command::new(command.get_program());
    replacement.args(args);
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            replacement.env(key, value);
        } else {
            replacement.env_remove(key);
        }
    }
    if let Some(path) = command.get_current_dir() {
        replacement.current_dir(path);
    }
    *command = replacement;
}
pub(super) fn render(
    payload: &[u8],
    details: Option<&crate::lifecycle::JobDetails>,
    names: bool,
) -> Result<Vec<u8>, ()> {
    let value: serde_json::Value = serde_json::from_slice(payload).map_err(|_| ())?;
    if value["event_type"] == "TEST" {
        crate::slack_message::render(payload)?;
        return crate::slack_blocks::render(&crate::notification_presentation::Notification {
            heading: "🧪 Slurm Notify test".into(),
            fields: vec!["Synthetic Slurm Notify test; no scheduler data.".into()],
            context: vec![format!("Event: {}", value["event_id"].as_str().ok_or(())?)],
        });
    }
    // Validate the base frame before using its allowlisted fields.
    crate::slack_message::render(payload)?;
    let default = crate::lifecycle::JobDetails::default();
    let details = details.unwrap_or(&default);
    let field = |key: &str| value.get(key).and_then(serde_json::Value::as_str).ok_or(());
    let state = value["job"]["state"].as_str().ok_or(())?;
    let title = match state {
        "COMPLETED" => "✅ Job completed",
        "FAILED" => "❌ Job failed",
        "TIMEOUT" => "⏱️ Job exceeded its time limit",
        "CANCELLED" => "🚫 Job cancelled",
        "OUT_OF_MEMORY" => "🧠 Job ran out of memory",
        "NODE_FAIL" => "⚠️ Job stopped: node failure",
        "BOOT_FAIL" => "⚠️ Job could not launch: boot failure",
        "DEADLINE" => "📅 Job terminated: deadline reached",
        "RUNNING" => "▶️ Job started",
        _ => return Err(()),
    };
    let mut fields = Vec::new();
    if names {
        let name = details.name.as_deref().unwrap_or("unavailable");
        if name.len() > 512 || name.chars().any(char::is_control) {
            return Err(());
        }
        let name = name
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        fields.push(format!("Name: {name}"));
    }
    let job = value["job"]["id"].as_str().ok_or(())?;
    let mut identity = format!("Job: {job}");
    if let Some(task) = value["job"]["array_task_id"].as_str() {
        identity.push_str(&format!(" · Array task: {task}"));
    }
    fields.push(identity);
    fields.push(format!("Cluster: {}", field("cluster_id")?));
    let duration = details
        .elapsed_seconds
        .map(|seconds| {
            let (hours, minutes, seconds) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
            if hours > 0 {
                format!("{hours}h {minutes}m {seconds}s")
            } else {
                format!("{minutes}m {seconds}s")
            }
        })
        .unwrap_or_else(|| "unavailable".into());
    fields.push(format!("Duration: {duration}"));
    if state != "RUNNING" {
        fields.push(format!(
            "Exit code (status:signal): {}",
            details.exit_code.as_deref().unwrap_or("unavailable")
        ));
    }
    fields.push(format!(
        "Started: {}",
        details.started_at.as_deref().unwrap_or("unavailable")
    ));
    if state != "RUNNING" {
        fields.push(format!(
            "Finished: {}",
            details.finished_at.as_deref().unwrap_or("unavailable")
        ));
    }
    crate::slack_blocks::render(&crate::notification_presentation::Notification {
        heading: title.into(),
        fields,
        context: vec![
            format!("Observed: {}", field("observed_at")?),
            format!("Event: {}", field("event_id")?),
        ],
    })
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[test]
    fn richer_rendering_is_allowlisted_and_name_disclosure_is_explicit() {
        let id = crate::public_event::identifier([0; 16]);
        let payload = crate::public_event::payload(&id, "cluster", "123_2", "FAILED", 0).unwrap();
        let details = crate::lifecycle::JobDetails {
            name: Some("sweep<&>".into()),
            elapsed_seconds: Some(754),
            started_at: Some("2026-10-25T10:01:00Z".into()),
            finished_at: Some("2026-10-25T10:13:34Z".into()),
            exit_code: Some("2:9".into()),
        };
        let completed =
            crate::public_event::payload(&id, "cluster", "123_2", "COMPLETED", 0).unwrap();
        assert!(
            String::from_utf8(render(&completed, Some(&details), false).unwrap())
                .unwrap()
                .contains("✅ Job completed")
        );
        let hidden = render(&payload, Some(&details), false).unwrap();
        assert!(!String::from_utf8(hidden).unwrap().contains("sweep"));
        let shown = render(&payload, Some(&details), true).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&shown).unwrap();
        let text = value["text"].as_str().unwrap();
        assert!(text.starts_with("❌ Job failed"));
        assert!(text.contains("Name: sweep&lt;&amp;&gt;"));
        assert!(text.contains("Job: 123 · Array task: 2"));
        assert!(text.contains("Duration: 12m 34s"));
        assert!(text.contains("Exit code (status:signal): 2:9"));
        assert!(crate::slack_message::valid_wire(&value, &id));
        let missing = String::from_utf8(render(&payload, None, true).unwrap()).unwrap();
        assert!(missing.contains("unavailable"));
        let mut invalid = details.clone();
        invalid.name = Some("bad\nname".into());
        assert!(render(&payload, Some(&invalid), true).is_err());
        invalid.name = Some("x".repeat(513));
        assert!(render(&payload, Some(&invalid), true).is_err());
        invalid.name = Some("safe".into());
        invalid.elapsed_seconds = Some(3601);
        assert!(
            String::from_utf8(render(&payload, Some(&invalid), false).unwrap())
                .unwrap()
                .contains("1h 0m 1s")
        );
        assert!(render(b"{}", None, false).is_err());
    }
    #[test]
    fn collection_changes_only_accounting_format_and_keeps_arguments() {
        let mut command = Command::new("/fixture/sacct");
        command
            .args([
                "--user=alice",
                "--format=Cluster,JobID,User,Submit,Start,State%32,ExitCode",
            ])
            .env("KEEP", "yes")
            .env_remove("REMOVE")
            .current_dir("/fixture");
        accounting_command(&mut command, true);
        assert!(command.get_args().any(|arg|arg=="--format=Cluster,JobID,User,Submit,Start,State%32,ExitCode,ElapsedRaw,End,JobName%256"));
        assert!(command.get_args().any(|arg| arg == "--user=alice"));
        assert_eq!(
            command.get_current_dir(),
            Some(std::path::Path::new("/fixture"))
        );
        let mut active = Command::new("/fixture/squeue");
        active.arg("--format=%i|%u|%V|%S|%T");
        accounting_command(&mut active, false);
        assert_eq!(active.get_args().count(), 1);
    }
}

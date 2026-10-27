//! Explicit terminal review and exact current SMTP enrollment; never sends or monitors.
use std::{
    io::{BufRead, IsTerminal, Read, Write},
    path::Path,
};
use zeroize::Zeroizing;
#[derive(Debug)]
pub(super) enum Error {
    Terminal,
    Input,
    Destination,
    Write(crate::config_create::Error),
}
struct Review {
    bytes: Zeroizing<Vec<u8>>,
    summary: String,
    api_enrollment: bool,
}
fn review(bytes: &[u8]) -> Result<Review, Error> {
    crate::config_validate::validate(bytes).map_err(|_| Error::Input)?;
    let plan = crate::mixed_config_composition::prepare(
        bytes,
        crate::effective_user::lookup,
        crate::environment_resolution::lookup,
    )
    .map_err(|_| Error::Input)?;
    let origins = plan.webhook_origins().ok_or(Error::Input)?;
    let mut source: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Error::Input)?;
    let cluster = source["monitor"]["cluster_id"]
        .as_str()
        .ok_or(Error::Input)?
        .to_owned();
    let policy = source["monitor"]["initial_observation"]
        .as_str()
        .ok_or(Error::Input)?;
    let providers = source["providers"].as_object().ok_or(Error::Input)?;
    let mut summary=format!("Configuration review: schema_version=1 cluster={cluster} initial_observation={policy} destinations={} routes={}\n",providers.len(),source["routes"].as_array().ok_or(Error::Input)?.len());
    summary.push_str("Initial observation suppress avoids catch-up alerts; emit can notify selected existing history.\n");
    for (alias, provider) in providers {
        if matches!(
            provider["type"].as_str(),
            Some("sendgrid" | "mailgun" | "gmail_api")
        ) {
            summary.push_str(&format!("API email: alias={alias} provider={} sender={} recipient={} account_identity={} fields=state,job_identifier,cluster_id,observed_at,event_id retention=state_lifetime\n",provider["type"].as_str().ok_or(Error::Input)?,provider["sender"].as_str().ok_or(Error::Input)?,provider["recipient"].as_str().ok_or(Error::Input)?,provider["account_identity"].as_str().ok_or(Error::Input)?));
            if provider["type"] == "gmail_api" {
                summary.push_str(&format!(
                    "Gmail binding: oauth_client_id={} verified_sender_required=true\n",
                    provider["oauth_client_id"].as_str().ok_or(Error::Input)?
                ));
            }
            if provider["type"] == "mailgun" {
                summary.push_str(&format!(
                    "Mailgun routing: domain={} region={}\n",
                    provider["domain"].as_str().ok_or(Error::Input)?,
                    provider["region"].as_str().ok_or(Error::Input)?
                ));
            }
            continue;
        }
        if provider["type"] != "smtp" {
            let origin = &origins
                .iter()
                .find(|(name, _)| *name == alias)
                .ok_or(Error::Input)?
                .1;
            if provider["type"] == "slack" {
                if provider["include_job_details"] == true || provider["include_job_name"] == true {
                    summary.push_str(&format!("Slack detail disclosure: alias={alias} fields=duration,exit_status_and_signal,started_at,finished_at job_name={} retention=state_lifetime rendering=plain_text_v2\n",if provider["include_job_name"]==true {"included"} else {"hidden"}));
                }
                summary.push_str(&format!("Slack: alias={alias} origin={origin} fields=cluster_id,job_identifier,state,observed_at,event_id rendering={}\n",if provider["include_job_details"]==true || provider["include_job_name"]==true {"plain_text_v2"} else {"plain_text_v1"}));
            } else {
                summary.push_str(&format!("Webhook: alias={alias} origin={origin} fields=schema_version,event_id,source,cluster_id,event_type,severity,observed_at,occurred_at,job.id,job.array_task_id,job.submitted_at,job.started_at,job.state\n"));
            }
        } else {
            summary.push_str(&format!("SMTP: alias={alias} relay={}:{} tls=required_starttls verification_name={} fields=state,job_identifier visibility={} envelope=configured_sender_and_recipient\n",provider["relay"].as_str().ok_or(Error::Input)?,provider["port"].as_u64().ok_or(Error::Input)?,provider["server_name"].as_str().ok_or(Error::Input)?,provider["visibility"].as_str().ok_or(Error::Input)?));
        }
    }
    if source["routes"].as_array().ok_or(Error::Input)?.is_empty() {
        summary.push_str("Routes are empty: monitoring will send no alerts.\n");
    }
    let revisions = plan.revisions();
    if !revisions.is_empty() {
        summary.push_str(
            "Only these current SMTP pairs will be enrolled; historical pairs are not retained.\n",
        );
        for (alias, rendering, provider) in &revisions {
            summary.push_str(&format!("SMTP permission: alias={alias} rendering_revision={rendering} provider_revision={provider}\n"));
        }
        source["delivery"]["smtp_permissions"]=serde_json::json!(revisions.iter().map(|(alias,rendering,provider)|serde_json::json!({"cluster_id":cluster,"destination":alias,"rendering_revision":rendering,"provider_revision":provider})).collect::<Vec<_>>());
    }
    let api = plan.api_revisions();
    if !api.is_empty() {
        summary.push_str("Only these current API-email pairs will be enrolled; SMTP permissions do not authorize API email.\n");
        for (alias, rendering, provider) in &api {
            summary.push_str(&format!("API email permission: alias={alias} rendering_revision={rendering} provider_revision={provider}\n"));
        }
        source["delivery"]["api_email_permissions"]=serde_json::json!(api.iter().map(|(alias,rendering,provider)|serde_json::json!({"cluster_id":cluster,"destination":alias,"rendering_revision":rendering,"provider_revision":provider})).collect::<Vec<_>>());
    }
    let mut bytes = Zeroizing::new(serde_json::to_vec_pretty(&source).map_err(|_| Error::Input)?);
    bytes.push(b'\n');
    crate::config_validate::validate(&bytes).map_err(|_| Error::Input)?;
    Ok(Review {
        bytes,
        summary,
        api_enrollment: !api.is_empty(),
    })
}
fn line(input: &mut impl BufRead, limit: usize) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut bytes = Zeroizing::new(Vec::new());
    input
        .take((limit + 2) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| Error::Input)?;
    if bytes.pop() != Some(b'\n') {
        return Err(Error::Input);
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    if bytes.len() > limit {
        return Err(Error::Input);
    }
    Ok(bytes)
}
pub(super) fn run(path: &Path) -> Result<(), Error> {
    if !std::io::stdin().is_terminal() {
        return Err(Error::Terminal);
    }
    crate::config_create::check(path).map_err(|_| Error::Destination)?;
    println!("Enter the absolute path to a protected JSON draft (maximum 65536 bytes); use environment references, never secret values.");
    std::io::stdout().flush().map_err(|_| Error::Input)?;
    let mut input = std::io::stdin().lock();
    let source = line(&mut input, 4096)?;
    let source = std::str::from_utf8(&source).map_err(|_| Error::Input)?;
    let snapshot = crate::config_snapshot::read(Path::new(source)).map_err(|_| Error::Input)?;
    let review = review(snapshot.bytes())?;
    print!("{}", review.summary);
    println!("No provider test or monitoring will run. Validate site polling/execution policy before start.");
    if review.api_enrollment {
        println!("Type yes to enroll the displayed provider pairs and create the new configuration; anything else cancels.");
    } else {
        println!("Type yes to enroll the displayed SMTP pairs and create the new configuration; anything else cancels.");
    }
    std::io::stdout().flush().map_err(|_| Error::Input)?;
    if line(&mut input, 16)?.as_slice() != b"yes" {
        println!("Configuration creation cancelled; no file was written.");
        return Ok(());
    }
    crate::config_create::write_new(path, &review.bytes).map_err(Error::Write)?;
    print!(
        "{}",
        include_str!("../../../contracts/cli/init-success.stdout")
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_lines_are_bounded_and_incomplete_input_is_rejected() {
        assert_eq!(line(&mut &b"yes\r\n"[..], 3).unwrap().as_slice(), b"yes");
        assert!(line(&mut &b"yes"[..], 3).is_err());
        assert!(line(&mut &b"long\n"[..], 3).is_err());
        assert!(line(&mut &b""[..], 3).is_err());
        assert!(review(b"{\"private\":\"fixture\"}").is_err());
    }
}

#[cfg(test)]
mod api_tests {
    #[test]
    fn api_review_enrolls_only_current_pairs_without_api_key_lookup() {
        for kind in ["sendgrid", "mailgun", "gmail-api"] {
            let bytes =
                std::fs::read(format!("../../contracts/config/{kind}-v1.example.json")).unwrap();
            let reviewed = super::review(&bytes).unwrap();
            assert!(reviewed.api_enrollment);
            assert!(reviewed.summary.contains("API email permission:"));
            let v: serde_json::Value = serde_json::from_slice(&reviewed.bytes).unwrap();
            assert_eq!(
                v["delivery"]["api_email_permissions"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            let again = super::review(&reviewed.bytes).unwrap();
            assert_eq!(reviewed.bytes.as_slice(), again.bytes.as_slice());
        }
    }
}

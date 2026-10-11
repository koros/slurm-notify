//! Private allowlist renderer; no public template or privacy defaults.
use super::{
    lifecycle::{Event, State},
    smtp_message::{self, Prepared},
};

// Every caller must choose disclosure explicitly. Cluster and attempt contexts
// are always omitted; no arbitrary scheduler text enters headers or body.
#[derive(Clone, Copy)]
pub(super) enum JobVisibility {
    Hidden,
    Identifier,
}

// No Debug/Clone: envelope and caller-supplied metadata are not diagnostics.
pub(super) struct Metadata<'a> {
    pub(super) sender: &'a str,
    pub(super) recipient: &'a str,
    pub(super) identity: &'a str,
    pub(super) timestamp: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    State,
    Identifier,
    Message(smtp_message::Error),
}

pub(super) fn prepare(
    event: &Event,
    metadata: &Metadata<'_>,
    visibility: JobVisibility,
) -> Result<Prepared, Error> {
    let state = match event.state {
        State::Running => "RUNNING",
        State::Completed => "COMPLETED",
        State::Failed => "FAILED",
        State::Timeout => "TIMEOUT",
        State::Cancelled => "CANCELLED",
        State::OutOfMemory => "OUT_OF_MEMORY",
        State::NodeFailure => "NODE_FAIL",
        State::BootFailure => "BOOT_FAIL",
        State::Deadline => "DEADLINE",
        State::Pending | State::Unknown(_) => return Err(Error::State),
    };
    let job = match visibility {
        JobVisibility::Hidden => "[redacted]",
        JobVisibility::Identifier => {
            let value = event.identity.job.as_str();
            // Allocation IDs and optional array indices only. Bound before scan
            // or formatting; never truncate identities or replace unsafe bytes.
            if value.is_empty() || value.len() > 32 {
                return Err(Error::Identifier);
            }
            let mut parts = value.split('_');
            if !parts.next().is_some_and(digits)
                || parts.next().is_some_and(|part| !digits(part))
                || parts.next().is_some()
            {
                return Err(Error::Identifier);
            }
            value
        }
    };
    let subject = format!("Slurm job {state}");
    let body = format!("Scheduler lifecycle notification.\nState: {state}\nJob: {job}\n");
    smtp_message::prepare(
        metadata.sender,
        metadata.recipient,
        metadata.identity,
        &subject,
        &body,
        metadata.timestamp,
    )
    .map_err(Error::Message)
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::Identity;

    fn event(state: State, job: &str) -> Event {
        Event {
            state,
            identity: Identity {
                cluster: "private-cluster-fixture".into(),
                job: job.into(),
                submit: "private-submit-fixture".into(),
                start: "private-start-fixture".into(),
            },
        }
    }
    fn metadata() -> Metadata<'static> {
        Metadata {
            sender: "s@fixture.invalid",
            recipient: "r@fixture.invalid",
            identity: "render_fixture",
            timestamp: 123,
        }
    }
    fn error(result: Result<Prepared, Error>) -> Error {
        match result {
            Err(error) => error,
            Ok(_) => panic!("unexpected successful rendering"),
        }
    }

    #[test]
    fn every_emitted_state_has_fixed_minimal_text_and_explicit_job_disclosure() {
        for state in [
            State::Running,
            State::Completed,
            State::Failed,
            State::Timeout,
            State::Cancelled,
            State::OutOfMemory,
            State::NodeFailure,
            State::BootFailure,
            State::Deadline,
        ] {
            for visibility in [JobVisibility::Hidden, JobVisibility::Identifier] {
                let event = event(state.clone(), "123456_7");
                let message = prepare(&event, &metadata(), visibility).unwrap();
                let text = std::str::from_utf8(message.data()).unwrap();
                assert!(text.contains(&format!("Subject: Slurm job {}", state.code())));
                assert!(text.contains(&format!("State: {}", state.code())));
                assert!(!text.contains("private-"));
                assert_eq!(
                    text.contains("123456_7"),
                    matches!(visibility, JobVisibility::Identifier)
                );
                assert_eq!(
                    text.contains("[redacted]"),
                    matches!(visibility, JobVisibility::Hidden)
                );
                assert!(message.data().len() < 1024);
            }
        }
    }

    #[test]
    fn hidden_fields_are_never_validated_or_copied_and_output_is_deterministic() {
        let mut event = event(State::Failed, &"sensitive-fixture".repeat(10000));
        event.identity.cluster = "\r\nBcc: injection@fixture.invalid".into();
        event.identity.submit = "\0\u{1b}[31m".into();
        event.identity.start = "Unicode fixture: café".into();
        let first = prepare(&event, &metadata(), JobVisibility::Hidden).unwrap();
        let second = prepare(&event, &metadata(), JobVisibility::Hidden).unwrap();
        assert!(first.data() == second.data());
        assert!(first.envelope() == second.envelope());
        let text = std::str::from_utf8(first.data()).unwrap();
        assert!(!text.contains("sensitive") && !text.contains("Bcc:") && !text.contains("café"));
    }

    #[test]
    fn invalid_disclosed_ids_and_non_emitted_states_fail_without_raw_diagnostics() {
        for job in [
            "",
            "1_",
            "_1",
            "1_2_3",
            "a",
            "1\r\nBcc: fixture",
            "1\0",
            "1\t",
            "１２",
            "123456789012345678901234567890123",
        ] {
            let failure = error(prepare(
                &event(State::Completed, job),
                &metadata(),
                JobVisibility::Identifier,
            ));
            assert_eq!(failure, Error::Identifier);
            assert_eq!(format!("{failure:?}"), "Identifier");
        }
        for state in [
            State::Pending,
            State::Unknown("private-unknown-fixture\r\n".into()),
        ] {
            assert_eq!(
                error(prepare(
                    &event(state, "1"),
                    &metadata(),
                    JobVisibility::Hidden
                )),
                Error::State
            );
        }
        for job in ["1", "1_2", "12345678901234567890123456789012"] {
            assert!(prepare(
                &event(State::Running, job),
                &metadata(),
                JobVisibility::Identifier
            )
            .is_ok());
        }
    }

    #[test]
    fn invalid_envelope_metadata_and_date_fail_with_static_builder_categories() {
        for (sender, recipient, identity, timestamp, expected) in [
            (
                "private-invalid",
                "r@fixture.invalid",
                "id",
                123,
                smtp_message::Error::Address,
            ),
            (
                "s@fixture.invalid",
                "private-invalid",
                "id",
                123,
                smtp_message::Error::Address,
            ),
            (
                "s@fixture.invalid",
                "r@fixture.invalid",
                "private\r\ninjection",
                123,
                smtp_message::Error::Header,
            ),
            (
                "s@fixture.invalid",
                "r@fixture.invalid",
                "id",
                u64::MAX,
                smtp_message::Error::Date,
            ),
        ] {
            let metadata = Metadata {
                sender,
                recipient,
                identity,
                timestamp,
            };
            let failure = error(prepare(
                &event(State::Failed, "1"),
                &metadata,
                JobVisibility::Hidden,
            ));
            assert_eq!(failure, Error::Message(expected));
            assert!(!format!("{failure:?}").contains("private"));
        }
    }
}

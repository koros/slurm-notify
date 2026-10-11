//! Private parent submission policy: strict evidence before durable bookkeeping.
use super::{
    smtp_auth_dispatch, smtp_submit,
    smtp_submit_frame::{self, Ack},
    supervised_provider::{self, Acceptance, Diagnostic, Report},
    transport,
};

pub(super) fn classify(result: Result<Ack, transport::Error>) -> Report {
    let ack = match result {
        Ok(ack) => ack,
        Err(error) => return supervised_provider::classify(Err(error)),
    };
    // Defense in depth for injected callers, in addition to the child decoder.
    if smtp_submit_frame::encode_ack(ack).is_err() {
        return supervised_provider::classify(Err(transport::Error::InvalidResult));
    }
    match ack {
        Ack::Auth(auth) => smtp_auth_dispatch::classify(Ok(auth)),
        Ack::Submission(report) => Report {
            outcome: Some(report.outcome),
            diagnostic: Diagnostic::SmtpSubmission(report.phase, report.diagnostic),
            acceptance: match report.acceptance {
                smtp_submit::Acceptance::RelayAccepted => Acceptance::Accepted,
                smtp_submit::Acceptance::NotSubmitted => Acceptance::NotSubmitted,
                smtp_submit::Acceptance::NotAccepted => Acceptance::NotAccepted,
                smtp_submit::Acceptance::Unknown => Acceptance::Unknown,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[test]
    fn decoded_submission_evidence_preserves_phase_and_acceptance() {
        for (bytes, outcome, acceptance) in [
            (
                b"SMTP_SUBMIT FINAL ACCEPTED RA\n".as_slice(),
                super::super::delivery::ProviderOutcome::Delivered,
                Acceptance::Accepted,
            ),
            (
                b"SMTP_SUBMIT FINAL TEMP NA\n",
                super::super::delivery::ProviderOutcome::Retry,
                Acceptance::NotAccepted,
            ),
            (
                b"SMTP_SUBMIT FINAL PERM NA\n",
                super::super::delivery::ProviderOutcome::Rejected,
                Acceptance::NotAccepted,
            ),
            (
                b"SMTP_SUBMIT RCPT PERM NS\n",
                super::super::delivery::ProviderOutcome::Rejected,
                Acceptance::NotSubmitted,
            ),
            (
                b"SMTP_SUBMIT FINAL IO U\n",
                super::super::delivery::ProviderOutcome::Retry,
                Acceptance::Unknown,
            ),
        ] {
            let ack = smtp_submit_frame::decode_ack(bytes).unwrap();
            let report = classify(Ok(ack));
            assert_eq!(report.acceptance, acceptance);
            assert_eq!(report.outcome, Some(outcome));
            if let Ack::Submission(child) = ack {
                assert_eq!(
                    report.diagnostic,
                    Diagnostic::SmtpSubmission(child.phase, child.diagnostic)
                );
            }
        }
    }
    #[test]
    fn auth_blocks_and_untrusted_results_never_invent_success() {
        use super::super::{delivery::ProviderOutcome, smtp_auth_frame};
        let block = classify(Ok(Ack::Auth(smtp_auth_frame::Ack::Permanent)));
        assert_eq!(block.acceptance, Acceptance::NotSubmitted);
        assert_eq!(block.into_dispatch(), Err(Diagnostic::SmtpAuthPermanent));
        let invalid = classify(Ok(Ack::Auth(smtp_auth_frame::Ack::Authenticated)));
        assert_eq!(invalid.acceptance, Acceptance::Unknown);
        assert_eq!(invalid.into_dispatch(), Ok(ProviderOutcome::Retry));
        let lost = classify(Err(transport::Error::ChildExit));
        assert_eq!(lost.acceptance, Acceptance::Unknown);
        assert_eq!(lost.into_dispatch(), Ok(ProviderOutcome::Retry));
        assert_eq!(
            classify(Err(transport::Error::Spawn)).into_dispatch(),
            Err(Diagnostic::Spawn)
        );
    }
}

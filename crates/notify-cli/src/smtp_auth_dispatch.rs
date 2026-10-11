//! Authentication-only parent policy; cannot classify any evidence as Delivered.
use super::{
    delivery::ProviderOutcome,
    smtp_auth_frame::Ack,
    supervised_provider::{self, Acceptance, Diagnostic, Report},
    transport,
};

pub(super) fn classify(result: Result<Ack, transport::Error>) -> Report {
    let ack = match result {
        Ok(ack) => ack,
        // Failed or missing child acknowledgment cannot prove SMTP phase.
        Err(error) => return supervised_provider::classify(Err(error)),
    };
    let (outcome, diagnostic) = match ack {
        // Successful authentication is incomplete delivery, so leave the row pending.
        Ack::Authenticated => (None, Diagnostic::SmtpAuthenticated),
        Ack::Credentials => (None, Diagnostic::SmtpCredentials),
        Ack::Unsupported => (None, Diagnostic::SmtpAuthUnsupported),
        Ack::Permanent => (None, Diagnostic::SmtpAuthPermanent),
        Ack::Temporary => (Some(ProviderOutcome::Retry), Diagnostic::SmtpAuthTemporary),
        Ack::Input => (None, Diagnostic::SmtpInput),
        Ack::Certificate => (None, Diagnostic::SmtpCertificate),
        Ack::Configuration => (None, Diagnostic::SmtpConfiguration),
        // Required TLS absence/permanent refusal blocks without message submission.
        Ack::UpgradeMissing => (None, Diagnostic::SmtpUpgradeMissing),
        Ack::UpgradePermanent => (None, Diagnostic::SmtpUpgradePermanent),
        Ack::UpgradeTemporary => (
            Some(ProviderOutcome::Retry),
            Diagnostic::SmtpUpgradeTemporary,
        ),
        Ack::Io => (Some(ProviderOutcome::Retry), Diagnostic::SmtpIo),
        Ack::Protocol => (Some(ProviderOutcome::Retry), Diagnostic::SmtpProtocol),
        Ack::Reply => (Some(ProviderOutcome::Retry), Diagnostic::SmtpReply),
    };
    Report {
        outcome,
        diagnostic,
        acceptance: Acceptance::NotSubmitted,
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom harness does not run unit tests.
    use super::*;
    #[test]
    fn authentication_blocks_and_ready_never_claim_delivery() {
        for (ack, diagnostic) in [
            (Ack::Authenticated, Diagnostic::SmtpAuthenticated),
            (Ack::Credentials, Diagnostic::SmtpCredentials),
            (Ack::Unsupported, Diagnostic::SmtpAuthUnsupported),
            (Ack::Permanent, Diagnostic::SmtpAuthPermanent),
            (Ack::Input, Diagnostic::SmtpInput),
            (Ack::Certificate, Diagnostic::SmtpCertificate),
            (Ack::Configuration, Diagnostic::SmtpConfiguration),
            (Ack::UpgradeMissing, Diagnostic::SmtpUpgradeMissing),
            (Ack::UpgradePermanent, Diagnostic::SmtpUpgradePermanent),
        ] {
            let report = classify(Ok(ack));
            assert_eq!(report.acceptance, Acceptance::NotSubmitted);
            assert_eq!(report.into_dispatch(), Err(diagnostic));
        }
    }
    #[test]
    fn transient_authentication_errors_retry_without_submission_claim() {
        for ack in [
            Ack::Io,
            Ack::Protocol,
            Ack::Reply,
            Ack::UpgradeTemporary,
            Ack::Temporary,
        ] {
            let report = classify(Ok(ack));
            assert_eq!(report.acceptance, Acceptance::NotSubmitted);
            assert_eq!(report.into_dispatch(), Ok(ProviderOutcome::Retry));
        }
    }
    #[test]
    fn unacknowledged_child_errors_preserve_conservative_parent_policy() {
        assert_eq!(
            classify(Err(transport::Error::Spawn)).into_dispatch(),
            Err(Diagnostic::Spawn)
        );
        for error in [
            transport::Error::Timeout,
            transport::Error::ChildExit,
            transport::Error::InvalidResult,
            transport::Error::Cleanup,
        ] {
            let report = classify(Err(error));
            assert_eq!(report.acceptance, Acceptance::Unknown);
            assert_eq!(report.into_dispatch(), Ok(ProviderOutcome::Retry));
        }
    }
}

//! Private parent-side result policy; no installed-helper or public status API.
use super::{delivery::ProviderOutcome, transport};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Diagnostic {
    Acknowledged,
    #[cfg(test)]
    SmtpReady,
    SmtpRevisionMissing,
    SmtpRevisionInvalid,
    #[cfg(test)]
    SmtpRevisionDenied,
    SmtpProviderMissing,
    SmtpProviderInvalid,
    SmtpProviderDenied,
    SmtpProviderResolution(super::smtp_provider_delivery::Error),
    SmtpSubmission(super::smtp_submit::Phase, super::smtp_submit::Diagnostic),
    #[cfg(test)]
    SmtpAuthenticated,
    SmtpCredentials,
    SmtpAuthUnsupported,
    SmtpAuthTemporary,
    SmtpAuthPermanent,
    SmtpInput,
    SmtpCertificate,
    SmtpConfiguration,
    SmtpIo,
    SmtpProtocol,
    SmtpReply,
    SmtpUpgradeMissing,
    SmtpUpgradeTemporary,
    SmtpUpgradePermanent,
    InvalidInput,
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    UnsupportedPlatform,
    Spawn,
    Timeout,
    Io,
    OutputLimit,
    ChildExit,
    InvalidResult,
    Cleanup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Acceptance {
    Accepted,
    Rejected,
    Unknown,
    NotAttempted,
    NotSubmitted,
    NotAccepted,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Report {
    // None blocks dispatch: do not fabricate a provider attempt or mark it failed.
    pub(super) outcome: Option<ProviderOutcome>,
    pub(super) diagnostic: Diagnostic,
    pub(super) acceptance: Acceptance,
}

impl Report {
    pub(super) fn into_dispatch(self) -> Result<ProviderOutcome, Diagnostic> {
        self.outcome.ok_or(self.diagnostic)
    }
}

pub(super) fn classify(result: Result<ProviderOutcome, transport::Error>) -> Report {
    use transport::Error;
    let (outcome, diagnostic, acceptance) = match result {
        Ok(ProviderOutcome::Delivered) => (
            Some(ProviderOutcome::Delivered),
            Diagnostic::Acknowledged,
            Acceptance::Accepted,
        ),
        Ok(ProviderOutcome::Rejected) => (
            Some(ProviderOutcome::Rejected),
            Diagnostic::Acknowledged,
            Acceptance::Rejected,
        ),
        Ok(ProviderOutcome::RetryAfter(seconds)) if seconds < 0 => (
            Some(ProviderOutcome::Retry),
            Diagnostic::InvalidResult,
            Acceptance::Unknown,
        ),
        Ok(outcome) => (Some(outcome), Diagnostic::Acknowledged, Acceptance::Unknown),
        Err(Error::InvalidInput) => (None, Diagnostic::InvalidInput, Acceptance::NotAttempted),
        Err(Error::Spawn) => (None, Diagnostic::Spawn, Acceptance::NotAttempted),
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        Err(Error::UnsupportedPlatform) => (
            None,
            Diagnostic::UnsupportedPlatform,
            Acceptance::NotAttempted,
        ),
        Err(error) => {
            let diagnostic = match error {
                Error::Timeout => Diagnostic::Timeout,
                Error::Io => Diagnostic::Io,
                Error::OutputLimit => Diagnostic::OutputLimit,
                Error::ChildExit => Diagnostic::ChildExit,
                Error::InvalidResult => Diagnostic::InvalidResult,
                Error::Cleanup => Diagnostic::Cleanup,
                _ => unreachable!("pre-dispatch errors handled above"),
            };
            (
                Some(ProviderOutcome::Retry),
                diagnostic,
                Acceptance::Unknown,
            )
        }
    };
    Report {
        outcome,
        diagnostic,
        acceptance,
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom harness imports the module without running unit tests.
    use super::*;

    #[test]
    fn acknowledged_outcomes_preserve_retry_hints_and_acceptance_limits() {
        for (outcome, acceptance) in [
            (ProviderOutcome::Delivered, Acceptance::Accepted),
            (ProviderOutcome::Rejected, Acceptance::Rejected),
            (ProviderOutcome::Retry, Acceptance::Unknown),
            (ProviderOutcome::RetryAfter(60), Acceptance::Unknown),
        ] {
            assert_eq!(
                classify(Ok(outcome)),
                Report {
                    outcome: Some(outcome),
                    diagnostic: Diagnostic::Acknowledged,
                    acceptance
                }
            );
        }
        assert_eq!(
            classify(Ok(ProviderOutcome::RetryAfter(-1))).diagnostic,
            Diagnostic::InvalidResult
        );
    }

    #[test]
    fn local_dispatch_failures_block_without_inventing_provider_attempts() {
        for error in [transport::Error::InvalidInput, transport::Error::Spawn] {
            let report = classify(Err(error));
            assert_eq!(report.outcome, None);
            assert_eq!(report.acceptance, Acceptance::NotAttempted);
        }
    }

    #[test]
    fn post_spawn_failures_retry_with_uncertain_acceptance_and_static_diagnostics() {
        for (error, diagnostic) in [
            (transport::Error::Timeout, Diagnostic::Timeout),
            (transport::Error::Io, Diagnostic::Io),
            (transport::Error::OutputLimit, Diagnostic::OutputLimit),
            (transport::Error::ChildExit, Diagnostic::ChildExit),
            (transport::Error::InvalidResult, Diagnostic::InvalidResult),
            (transport::Error::Cleanup, Diagnostic::Cleanup),
        ] {
            assert_eq!(
                classify(Err(error)),
                Report {
                    outcome: Some(ProviderOutcome::Retry),
                    diagnostic,
                    acceptance: Acceptance::Unknown
                }
            );
        }
    }
}

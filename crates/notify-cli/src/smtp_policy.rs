//! Private synthetic SMTP transaction policy, not a wire parser or SMTP client.
use super::delivery::ProviderOutcome;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Greeting,
    Ehlo,
    StartTls,
    Tls,
    SecureEhlo,
    Auth,
    Mail,
    Recipient,
    Data,
    FinalData,
    Quit,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Evidence {
    Reply(u16),
    TlsVerified,
    TlsBlocked,
    ConnectionLost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Diagnostic {
    RelayAccepted,
    TemporaryReply,
    PermanentReply,
    Authentication,
    TlsPolicy,
    Connection,
    Protocol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Acceptance {
    RelayAccepted,
    NotSubmitted,
    NotAccepted,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Continue,
    Outcome(ProviderOutcome),
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Report {
    action: Action,
    diagnostic: Option<Diagnostic>,
    acceptance: Acceptance,
}

struct Session {
    phase: Phase,
    authenticate: bool,
    accepted: bool,
    terminal: Option<Report>,
}

impl Session {
    fn new(authenticate: bool) -> Self {
        Self {
            phase: Phase::Greeting,
            authenticate,
            accepted: false,
            terminal: None,
        }
    }

    fn observe(&mut self, evidence: Evidence) -> Report {
        if let Some(report) = &self.terminal {
            return report.clone();
        }
        // Relay acceptance is final even if QUIT subsequently fails.
        if self.accepted {
            self.phase = Phase::Finished;
            return Report {
                action: Action::Outcome(ProviderOutcome::Delivered),
                diagnostic: Some(Diagnostic::RelayAccepted),
                acceptance: Acceptance::RelayAccepted,
            };
        }
        let uncertain = self.phase == Phase::FinalData;
        let retry = |diagnostic| Report {
            action: Action::Outcome(ProviderOutcome::Retry),
            diagnostic: Some(diagnostic),
            acceptance: if uncertain {
                Acceptance::Unknown
            } else {
                Acceptance::NotSubmitted
            },
        };
        if self.phase == Phase::Finished {
            return retry(Diagnostic::Protocol);
        }
        let report = match evidence {
            Evidence::TlsBlocked
                if matches!(self.phase, Phase::Ehlo | Phase::StartTls | Phase::Tls) =>
            {
                Report {
                    action: Action::Blocked,
                    diagnostic: Some(Diagnostic::TlsPolicy),
                    acceptance: Acceptance::NotSubmitted,
                }
            }
            Evidence::TlsVerified if self.phase == Phase::Tls => {
                self.phase = Phase::SecureEhlo;
                return Report {
                    action: Action::Continue,
                    diagnostic: None,
                    acceptance: Acceptance::NotSubmitted,
                };
            }
            Evidence::ConnectionLost => retry(Diagnostic::Connection),
            Evidence::Reply(code) if (400..500).contains(&code) => Report {
                action: Action::Outcome(ProviderOutcome::Retry),
                diagnostic: Some(Diagnostic::TemporaryReply),
                acceptance: if uncertain {
                    Acceptance::NotAccepted
                } else {
                    Acceptance::NotSubmitted
                },
            },
            Evidence::Reply(code) if (500..600).contains(&code) => {
                if self.phase == Phase::Auth {
                    Report {
                        action: Action::Blocked,
                        diagnostic: Some(Diagnostic::Authentication),
                        acceptance: Acceptance::NotSubmitted,
                    }
                } else if self.phase == Phase::StartTls {
                    Report {
                        action: Action::Blocked,
                        diagnostic: Some(Diagnostic::TlsPolicy),
                        acceptance: Acceptance::NotSubmitted,
                    }
                } else {
                    Report {
                        action: Action::Outcome(ProviderOutcome::Rejected),
                        diagnostic: Some(Diagnostic::PermanentReply),
                        acceptance: if uncertain {
                            Acceptance::NotAccepted
                        } else {
                            Acceptance::NotSubmitted
                        },
                    }
                }
            }
            Evidence::Reply(code) => {
                let next = match (self.phase, code) {
                    (Phase::Greeting, 220) => Some(Phase::Ehlo),
                    (Phase::Ehlo, 250) => Some(Phase::StartTls),
                    (Phase::StartTls, 220) => Some(Phase::Tls),
                    (Phase::SecureEhlo, 250) => Some(if self.authenticate {
                        Phase::Auth
                    } else {
                        Phase::Mail
                    }),
                    (Phase::Auth, 235) => Some(Phase::Mail),
                    (Phase::Mail, 250) => Some(Phase::Recipient),
                    (Phase::Recipient, 250 | 251) => Some(Phase::Data),
                    (Phase::Data, 354) => Some(Phase::FinalData),
                    (Phase::FinalData, 250) => {
                        self.accepted = true;
                        self.phase = Phase::Quit;
                        return Report {
                            action: Action::Outcome(ProviderOutcome::Delivered),
                            diagnostic: Some(Diagnostic::RelayAccepted),
                            acceptance: Acceptance::RelayAccepted,
                        };
                    }
                    _ => None,
                };
                if let Some(next) = next {
                    self.phase = next;
                    return Report {
                        action: Action::Continue,
                        diagnostic: None,
                        acceptance: Acceptance::NotSubmitted,
                    };
                }
                retry(Diagnostic::Protocol)
            }
            _ => retry(Diagnostic::Protocol),
        };
        self.phase = Phase::Finished;
        self.terminal = Some(report.clone());
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn through_data(authenticate: bool) -> Session {
        let mut session = Session::new(authenticate);
        for evidence in [
            Evidence::Reply(220),
            Evidence::Reply(250),
            Evidence::Reply(220),
            Evidence::TlsVerified,
            Evidence::Reply(250),
        ] {
            assert_eq!(session.observe(evidence).action, Action::Continue);
        }
        if authenticate {
            assert_eq!(
                session.observe(Evidence::Reply(235)).action,
                Action::Continue
            );
        }
        for code in [250, 251, 354] {
            assert_eq!(
                session.observe(Evidence::Reply(code)).action,
                Action::Continue
            );
        }
        session
    }

    // Envelope-only plaintext parser fixture: TLS evidence is synthetic and no
    // credentials are sent. This is not an SMTP client or verified TLS session.
    fn loopback_dialogue(
        final_reply: &'static [u8],
        delay: std::time::Duration,
    ) -> (Report, Option<super::super::smtp_reply::Error>) {
        use std::{
            io::{Read, Write},
            net::{TcpListener, TcpStream},
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(1))
                    }
                    _ => panic!("fixture accept failed"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            for (expected, response) in [
                (
                    &b"MAIL FROM:<sender@example.invalid>\r\n"[..],
                    &b"250 sender ok\r\n"[..],
                ),
                (
                    &b"RCPT TO:<recipient@example.invalid>\r\n"[..],
                    &b"250-recipient fixture\r\n250 ok\r\n"[..],
                ),
                (&b"DATA\r\n"[..], &b"354 continue\r\n"[..]),
                (&b"fixture body\r\n.\r\n"[..], final_reply),
            ] {
                // Exact bounded fixture reads; assert without echoing message bytes.
                let mut request = vec![0; expected.len()];
                stream.read_exact(&mut request).unwrap();
                assert!(request == expected, "fixture command mismatch");
                if expected.starts_with(b"fixture body") {
                    std::thread::sleep(delay);
                }
                let _ = stream.write_all(response); // Timeout may close the client first.
            }
        });
        let mut session = Session::new(false);
        for evidence in [
            Evidence::Reply(220),
            Evidence::Reply(250),
            Evidence::Reply(220),
            Evidence::TlsVerified,
            Evidence::Reply(250),
        ] {
            assert_eq!(session.observe(evidence).action, Action::Continue);
        }
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut last = None;
        let mut report = None;
        for command in [
            &b"MAIL FROM:<sender@example.invalid>\r\n"[..],
            &b"RCPT TO:<recipient@example.invalid>\r\n"[..],
            &b"DATA\r\n"[..],
            &b"fixture body\r\n.\r\n"[..],
        ] {
            stream.write_all(command).unwrap();
            let parsed = super::super::smtp_reply::read_reply(
                &mut stream,
                super::super::smtp_reply::Limits {
                    line_bytes: 64,
                    total_bytes: 128,
                    lines: 4,
                },
            );
            let current = match parsed {
                Ok(reply) => session.observe(Evidence::Reply(reply.code)),
                Err(error) => {
                    let evidence = match error {
                        super::super::smtp_reply::Error::Malformed
                        | super::super::smtp_reply::Error::Limit => Evidence::Reply(999),
                        _ => Evidence::ConnectionLost,
                    };
                    last = Some(error);
                    session.observe(evidence)
                }
            };
            if current.action != Action::Continue {
                report = Some(current);
                break;
            }
        }
        drop(stream);
        server.join().unwrap();
        (report.unwrap(), last)
    }

    #[test]
    fn loopback_envelope_dialogues_compose_parser_and_final_reply_policy() {
        for (reply, expected, acceptance) in [
            (
                &b"250 queued\r\n"[..],
                ProviderOutcome::Delivered,
                Acceptance::RelayAccepted,
            ),
            (
                &b"451 temporary\r\n"[..],
                ProviderOutcome::Retry,
                Acceptance::NotAccepted,
            ),
            (
                &b"550 rejected\r\n"[..],
                ProviderOutcome::Rejected,
                Acceptance::NotAccepted,
            ),
        ] {
            let (report, error) = loopback_dialogue(reply, std::time::Duration::ZERO);
            assert_eq!(error, None);
            assert_eq!(report.action, Action::Outcome(expected));
            assert_eq!(report.acceptance, acceptance);
        }
    }

    #[test]
    fn truncated_and_delayed_loopback_acknowledgments_preserve_unknown_acceptance() {
        for (reply, delay, expected) in [
            (
                &b"250 truncated"[..],
                std::time::Duration::ZERO,
                super::super::smtp_reply::Error::Truncated,
            ),
            (
                &b"250 queued\r\n"[..],
                std::time::Duration::from_millis(400),
                super::super::smtp_reply::Error::Io,
            ),
            (
                &b"250-bad\r\n550 changed\r\n"[..],
                std::time::Duration::ZERO,
                super::super::smtp_reply::Error::Malformed,
            ),
        ] {
            let (report, error) = loopback_dialogue(reply, delay);
            assert_eq!(error, Some(expected));
            assert_eq!(report.action, Action::Outcome(ProviderOutcome::Retry));
            assert_eq!(report.acceptance, Acceptance::Unknown);
        }
    }

    #[test]
    fn verified_tls_and_secure_ehlo_precede_submission_and_final_ack_is_required() {
        for authenticate in [false, true] {
            let mut session = through_data(authenticate);
            assert_eq!(session.phase, Phase::FinalData);
            let delivered = session.observe(Evidence::Reply(250));
            assert_eq!(
                delivered.action,
                Action::Outcome(ProviderOutcome::Delivered)
            );
            assert_eq!(delivered.acceptance, Acceptance::RelayAccepted);
            assert_eq!(session.observe(Evidence::ConnectionLost), delivered);
        }
        let mut session = Session::new(true);
        session.observe(Evidence::Reply(220));
        session.observe(Evidence::Reply(250));
        session.observe(Evidence::Reply(220));
        assert_eq!(session.phase, Phase::Tls);
        assert_eq!(
            session.observe(Evidence::Reply(235)).diagnostic,
            Some(Diagnostic::Protocol)
        );
    }

    #[test]
    fn temporary_permanent_and_uncertain_final_results_are_distinct() {
        for (evidence, outcome, acceptance) in [
            (
                Evidence::Reply(451),
                ProviderOutcome::Retry,
                Acceptance::NotAccepted,
            ),
            (
                Evidence::Reply(550),
                ProviderOutcome::Rejected,
                Acceptance::NotAccepted,
            ),
            (
                Evidence::ConnectionLost,
                ProviderOutcome::Retry,
                Acceptance::Unknown,
            ),
            (
                Evidence::Reply(999),
                ProviderOutcome::Retry,
                Acceptance::Unknown,
            ),
        ] {
            let report = through_data(true).observe(evidence);
            assert_eq!(report.action, Action::Outcome(outcome));
            assert_eq!(report.acceptance, acceptance);
            let mut session = through_data(true);
            let first = session.observe(evidence);
            assert_eq!(session.observe(Evidence::ConnectionLost), first);
        }
        assert_eq!(
            Session::new(false)
                .observe(Evidence::ConnectionLost)
                .acceptance,
            Acceptance::NotSubmitted
        );
    }

    #[test]
    fn tls_and_authentication_blocks_never_claim_message_submission() {
        for phase in [Phase::Ehlo, Phase::StartTls, Phase::Tls] {
            let mut session = Session {
                phase,
                authenticate: true,
                accepted: false,
                terminal: None,
            };
            let report = session.observe(Evidence::TlsBlocked);
            assert_eq!(report.action, Action::Blocked);
            assert_eq!(report.diagnostic, Some(Diagnostic::TlsPolicy));
            assert_eq!(report.acceptance, Acceptance::NotSubmitted);
        }
        for (phase, code, diagnostic) in [
            (Phase::StartTls, 550, Diagnostic::TlsPolicy),
            (Phase::Auth, 535, Diagnostic::Authentication),
        ] {
            let mut session = Session {
                phase,
                authenticate: true,
                accepted: false,
                terminal: None,
            };
            let report = session.observe(Evidence::Reply(code));
            assert_eq!(report.action, Action::Blocked);
            assert_eq!(report.diagnostic, Some(diagnostic));
        }
    }
}

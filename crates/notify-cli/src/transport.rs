//! Private Unix transport-supervision experiment; no installed helper contract.
use super::delivery::ProviderOutcome;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
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

// Length prefixes permit arbitrary payload bytes without delimiters or escaping.
// No Debug representation of the frame, endpoint, or authorization is provided.
fn frame(fields: &[&[u8]], limit: usize) -> Result<Vec<u8>, Error> {
    let length = fields
        .iter()
        .try_fold(0usize, |total, field| {
            total.checked_add(4)?.checked_add(field.len())
        })
        .ok_or(Error::InvalidInput)?;
    if length > limit || fields.iter().any(|f| f.len() > u32::MAX as usize) {
        return Err(Error::InvalidInput);
    }
    let mut bytes = Vec::with_capacity(length);
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    Ok(bytes)
}

fn parse_result(bytes: &[u8]) -> Result<ProviderOutcome, Error> {
    match bytes {
        b"OK\n" => Ok(ProviderOutcome::Delivered),
        b"RETRY\n" => Ok(ProviderOutcome::Retry),
        b"REJECT\n" => Ok(ProviderOutcome::Rejected),
        _ => {
            let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidResult)?;
            let seconds = text
                .strip_prefix("RETRY_AFTER ")
                .and_then(|v| v.strip_suffix('\n'))
                .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|v| v.parse::<i64>().ok())
                .ok_or(Error::InvalidResult)?;
            Ok(ProviderOutcome::RetryAfter(seconds))
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn supervise(
    command: &mut Command,
    input: Vec<u8>,
    timeout: Duration,
    input_limit: usize,
    output_limit: usize,
) -> Result<ProviderOutcome, Error> {
    use std::os::unix::process::CommandExt;
    if input.len() > input_limit || timeout.is_zero() || output_limit == 0 {
        return Err(Error::InvalidInput);
    }
    // Start the deadline before spawning. Local process creation and OS reaping
    // are synchronous; cleanup can add latency beyond the request deadline.
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(Error::InvalidInput)?;
    let mut child = command
        .process_group(0)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error::Spawn)?;
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let writer_sender = sender.clone();
    let writer = thread::spawn(move || {
        let result = stdin.write_all(&input).map_err(|_| Error::Io);
        drop(stdin); // EOF marks the end of the bounded request.
        let _ = writer_sender.send((false, result.map(|_| Vec::new())));
    });
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(output_limit.saturating_add(1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Io)
            .and({
                if bytes.len() > output_limit {
                    Err(Error::OutputLimit)
                } else {
                    Ok(bytes)
                }
            });
        let _ = sender.send((true, result));
    });
    let result = (|| {
        let mut output = None;
        for _ in 0..2 {
            let (is_output, bytes) = receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| Error::Timeout)?;
            let bytes = bytes?;
            if is_output {
                output = Some(bytes);
            }
        }
        loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            if let Some(status) = child.try_wait().map_err(|_| Error::Io)? {
                if !status.success() {
                    return Err(Error::ChildExit);
                }
                return parse_result(&output.unwrap());
            }
            thread::sleep(Duration::from_millis(2));
        }
    })();
    // Always remove remaining descendants, including after a successful exit.
    let _ = super::scheduler::kill_group(child.id());
    let _ = child.kill();
    child.wait().map_err(|_| Error::Cleanup)?;
    let cleanup_deadline = Instant::now() + Duration::from_secs(1);
    while !writer.is_finished() || !reader.is_finished() {
        if Instant::now() >= cleanup_deadline {
            return Err(Error::Cleanup);
        }
        thread::sleep(Duration::from_millis(2));
    }
    writer.join().map_err(|_| Error::Cleanup)?;
    reader.join().map_err(|_| Error::Cleanup)?;
    result
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn supervise(
    _command: &mut Command,
    _input: Vec<u8>,
    _timeout: Duration,
    _input_limit: usize,
    _output_limit: usize,
) -> Result<ProviderOutcome, Error> {
    Err(Error::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_arbitrary_bytes_and_checks_limits_before_allocation() {
        assert_eq!(
            frame(&[b"url", b"", b"a\0b\n"], 19).unwrap(),
            [0, 0, 0, 3, b'u', b'r', b'l', 0, 0, 0, 0, 0, 0, 0, 4, b'a', 0, b'b', b'\n']
        );
        assert_eq!(frame(&[b"value"], 8), Err(Error::InvalidInput));
        assert_eq!(
            parse_result(b"RETRY_AFTER 60\n"),
            Ok(ProviderOutcome::RetryAfter(60))
        );
        for bad in [
            &b"OK\nextra"[..],
            b"RETRY_AFTER -1\n",
            b"RETRY_AFTER 9223372036854775808\n",
            b"secret",
            b"\xff",
        ] {
            assert_eq!(parse_result(bad), Err(Error::InvalidResult));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn python(script: &str) -> Command {
        let mut command = Command::new("python3");
        command.args(["-c", script]);
        command
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn invalid_limits_fail_before_spawn_and_missing_helper_is_visible() {
        let mut missing = Command::new("/missing/slurm-notify-transport-fixture");
        assert_eq!(
            supervise(&mut missing, vec![], Duration::ZERO, 16, 32),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            supervise(&mut missing, vec![0; 17], Duration::from_secs(1), 16, 32),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            supervise(&mut missing, vec![], Duration::from_secs(1), 16, 0),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            supervise(&mut missing, vec![], Duration::from_secs(1), 16, 32),
            Err(Error::Spawn)
        );
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn sends_stdin_only_and_accepts_exact_acknowledgment() {
        let mut command = python("import sys,os; data=sys.stdin.buffer.read(); assert data==b'fixture'; assert 'TRANSPORT_FIXTURE' not in os.environ; sys.stdout.write('OK\\n')");
        command.env("TRANSPORT_FIXTURE", "must-be-cleared");
        assert_eq!(
            supervise(
                &mut command,
                b"fixture".to_vec(),
                Duration::from_secs(2),
                16,
                32
            ),
            Ok(ProviderOutcome::Delivered)
        );
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn blocked_input_and_response_are_terminated() {
        for input in [vec![], vec![0; 1024 * 1024]] {
            let mut command = python("import time; time.sleep(30)");
            let started = Instant::now();
            assert_eq!(
                supervise(
                    &mut command,
                    input,
                    Duration::from_millis(100),
                    1024 * 1024,
                    32
                ),
                Err(Error::Timeout)
            );
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn limits_output_and_rejects_nonzero_or_malformed_acknowledgment() {
        for (script, expected) in [
            (
                "import sys; sys.stdin.buffer.read(); sys.stdout.write('x'*1024)",
                Error::OutputLimit,
            ),
            (
                "import sys; sys.stdin.buffer.read(); sys.stdout.write('OK\\n'); sys.exit(1)",
                Error::ChildExit,
            ),
            (
                "import sys; sys.stdin.buffer.read(); sys.stdout.write('not-a-result')",
                Error::InvalidResult,
            ),
        ] {
            assert_eq!(
                supervise(&mut python(script), vec![], Duration::from_secs(2), 16, 32),
                Err(expected)
            );
        }
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn descendants_holding_pipes_cannot_keep_supervisor_waiting() {
        let mut command =
            python("import os,time; pid=os.fork(); time.sleep(30) if pid==0 else None");
        let started = Instant::now();
        assert_eq!(
            supervise(&mut command, vec![], Duration::from_millis(100), 16, 32),
            Err(Error::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

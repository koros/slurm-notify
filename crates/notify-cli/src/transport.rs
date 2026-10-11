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
pub(super) fn frame(fields: &[&[u8]], limit: usize) -> Result<Vec<u8>, Error> {
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

// Decode exactly five fields, enforcing the total byte budget before allocation.
// Fields: endpoint, authorization (empty means absent), ID, body,
// socket timeout milliseconds. This remains a private prototype wire format.
pub(super) fn serve(
    mut input: impl Read,
    mut output: impl Write,
    limit: usize,
    send: impl FnOnce(Vec<Vec<u8>>) -> Result<ProviderOutcome, Error>,
) -> Result<(), Error> {
    let mut fields = Vec::with_capacity(5);
    let mut used = 0usize;
    for _ in 0..5 {
        let mut prefix = [0; 4];
        input
            .read_exact(&mut prefix)
            .map_err(|_| Error::InvalidInput)?;
        let length = u32::from_be_bytes(prefix) as usize;
        used = used
            .checked_add(4)
            .and_then(|v| v.checked_add(length))
            .filter(|v| *v <= limit)
            .ok_or(Error::InvalidInput)?;
        let mut field = vec![0; length];
        input
            .read_exact(&mut field)
            .map_err(|_| Error::InvalidInput)?;
        fields.push(field);
    }
    let mut extra = [0; 1];
    if input.read(&mut extra).map_err(|_| Error::Io)? != 0 {
        return Err(Error::InvalidInput);
    }
    let result = match send(fields)? {
        ProviderOutcome::Delivered => "OK\n".to_owned(),
        ProviderOutcome::Retry => "RETRY\n".to_owned(),
        ProviderOutcome::Rejected => "REJECT\n".to_owned(),
        ProviderOutcome::RetryAfter(seconds) if seconds >= 0 => format!("RETRY_AFTER {seconds}\n"),
        ProviderOutcome::RetryAfter(_) => return Err(Error::InvalidResult),
    };
    output.write_all(result.as_bytes()).map_err(|_| Error::Io)?;
    output.flush().map_err(|_| Error::Io)
}

pub(super) fn send_fields(
    fields: Vec<Vec<u8>>,
    blocked_resolver_fixture: bool,
) -> Result<ProviderOutcome, Error> {
    use super::webhook::Webhook;
    if fields.len() != 5 {
        return Err(Error::InvalidInput);
    }
    let text = |index: usize| std::str::from_utf8(&fields[index]).map_err(|_| Error::InvalidInput);
    let endpoint = text(0)?;
    let authorization = text(1)?;
    let id = text(2)?;
    let milliseconds = text(4)?.parse::<u64>().map_err(|_| Error::InvalidInput)?;
    // Fixture ceilings bound requests even when the parent supplies bad values.
    if milliseconds == 0 || milliseconds > 60_000 {
        return Err(Error::InvalidInput);
    }
    let mut webhook = Webhook::new(
        endpoint,
        (!authorization.is_empty()).then(|| authorization.to_owned()),
        Duration::from_millis(milliseconds),
        1024 * 1024,
    )
    .map_err(|_| Error::InvalidInput)?;
    if blocked_resolver_fixture {
        webhook.block_resolver_fixture();
    }
    webhook
        .send_with_clock(id, &fields[3], super::delivery::unix_time)
        .map_err(|_| Error::InvalidInput)
}

pub(super) fn supervise(
    command: &mut Command,
    input: Vec<u8>,
    timeout: Duration,
    input_limit: usize,
    output_limit: usize,
) -> Result<ProviderOutcome, Error> {
    supervise_with(
        command,
        input,
        timeout,
        input_limit,
        output_limit,
        parse_result,
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn supervise_with<T>(
    command: &mut Command,
    input: Vec<u8>,
    timeout: Duration,
    input_limit: usize,
    output_limit: usize,
    decode: fn(&[u8]) -> Result<T, Error>,
) -> Result<T, Error> {
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
                return decode(&output.unwrap());
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
pub(super) fn supervise_with<T>(
    _command: &mut Command,
    _input: Vec<u8>,
    _timeout: Duration,
    _input_limit: usize,
    _output_limit: usize,
    _decode: fn(&[u8]) -> Result<T, Error>,
) -> Result<T, Error> {
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

    #[test]
    fn decoding_rejects_truncation_extra_bytes_and_length_bombs_before_send() {
        let valid = frame(&[b"url", b"", b"id", b"{}", b"100"], 128).unwrap();
        let mut invalid = vec![valid[..valid.len() - 1].to_vec(), vec![255; 4]];
        let mut extra = valid.clone();
        extra.push(0);
        invalid.push(extra);
        for bytes in invalid {
            assert_eq!(
                serve(&bytes[..], Vec::new(), 128, |_| panic!(
                    "invalid request sent"
                )),
                Err(Error::InvalidInput)
            );
        }
        let mut output = Vec::new();
        serve(&valid[..], &mut output, 128, |fields| {
            assert_eq!(fields[3], b"{}");
            Ok(ProviderOutcome::RetryAfter(60))
        })
        .unwrap();
        assert_eq!(output, b"RETRY_AFTER 60\n");
    }

    #[test]
    fn invalid_child_fields_cannot_send() {
        for fields in [
            vec![],
            vec![
                b"https://example.invalid/".to_vec(),
                vec![],
                b"id".to_vec(),
                vec![],
                b"-1".to_vec(),
            ],
        ] {
            assert_eq!(send_fields(fields, false), Err(Error::InvalidInput));
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

//! Explicit one-shot API-email test. No scheduler, state, refresh or automatic resend.
use crate::{
    email_content::Content,
    email_profile::{Profile, Provider},
    email_result::Outcome,
    provider_frame::Error,
};
use std::{
    io::{Read, Write},
    path::Path,
    process::Command,
};
use zeroize::Zeroizing;
pub(super) const INPUT_LIMIT: usize = 103000;
fn read_fields(input: impl Read) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut bytes = Zeroizing::new(Vec::new());
    input
        .take((INPUT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::InvalidInput)?;
    if bytes.len() > INPUT_LIMIT {
        return Err(Error::InvalidInput);
    }
    Ok(bytes)
}
fn fields(bytes: &[u8]) -> Result<Vec<&[u8]>, Error> {
    let mut remaining = bytes;
    let mut result = Vec::new();
    for _ in 0..5 {
        let prefix: [u8; 4] = remaining
            .get(..4)
            .ok_or(Error::InvalidInput)?
            .try_into()
            .map_err(|_| Error::InvalidInput)?;
        let size = u32::from_be_bytes(prefix) as usize;
        remaining = &remaining[4..];
        result.push(remaining.get(..size).ok_or(Error::InvalidInput)?);
        remaining = &remaining[size..];
    }
    if !remaining.is_empty() {
        return Err(Error::InvalidInput);
    }
    Ok(result)
}
pub(super) fn serve(input: impl Read, mut output: impl Write) -> Result<(), Error> {
    let bytes = read_fields(input)?;
    let fields = fields(&bytes)?;
    let profile = Profile::parse(fields[0]).map_err(|_| Error::InvalidInput)?;
    if fields[1].len() > 16384
        || fields[2].len() != 36
        || fields[3].len() > 10
        || fields[4].len() > 65664
    {
        return Err(Error::InvalidInput);
    }
    let id = std::str::from_utf8(fields[2]).map_err(|_| Error::InvalidInput)?;
    let time = std::str::from_utf8(fields[3]).map_err(|_| Error::InvalidInput)?;
    let timestamp = time.parse::<u64>().map_err(|_| Error::InvalidInput)?;
    if timestamp.to_string() != time {
        return Err(Error::InvalidInput);
    }
    let content = Content::synthetic(id, timestamp).map_err(|_| Error::InvalidInput)?;
    let request =
        crate::email_wire::prepare(&profile, &content).map_err(|_| Error::InvalidInput)?;
    let mut roots = Vec::new();
    let mut remaining = fields[4];
    while !remaining.is_empty() {
        let prefix: [u8; 4] = remaining
            .get(..4)
            .ok_or(Error::InvalidInput)?
            .try_into()
            .map_err(|_| Error::InvalidInput)?;
        let len = u32::from_be_bytes(prefix) as usize;
        remaining = &remaining[4..];
        roots.push(remaining.get(..len).ok_or(Error::InvalidInput)?);
        remaining = &remaining[len..];
        if roots.len() > 32 {
            return Err(Error::InvalidInput);
        }
    }
    if profile.roots.is_empty() != roots.is_empty() {
        return Err(Error::InvalidInput);
    }
    let agent =
        crate::email_http::agent(profile.timeout, &roots).map_err(|_| Error::InvalidInput)?;
    let now = crate::provider_outcome::unix_time().ok_or(Error::InvalidInput)?;
    let outcome = crate::email_http::send(&profile, fields[1], &request, &agent, now);
    output
        .write_all(outcome.encode().as_bytes())
        .map_err(|_| Error::Io)?;
    output.flush().map_err(|_| Error::Io)
}
pub(super) fn run(path: &Path) -> Result<Outcome, ()> {
    let snapshot = crate::config_snapshot::read_limit(path, 16384).map_err(|_| ())?;
    let profile = Profile::parse(snapshot.bytes())?;
    let credential = match profile.provider {
        Provider::Gmail => Zeroizing::new(
            crate::config_snapshot::read_limit(Path::new(profile.credential.as_str()), 16384)
                .map_err(|_| ())?
                .bytes()
                .to_vec(),
        ),
        _ => crate::environment_resolution::lookup(&profile.credential, 4096)?,
    };
    let roots = if profile.roots.is_empty() {
        None
    } else {
        Some(crate::trust_snapshot::load(&profile.roots).map_err(|_| ())?)
    };
    let trust = roots
        .as_ref()
        .map(|r| crate::provider_frame::frame(&r.roots(), 65664))
        .transpose()
        .map_err(|_| ())?
        .unwrap_or_default();
    let id = crate::public_event::identifier(crate::public_event::random()?);
    let timestamp = crate::provider_outcome::unix_time()
        .and_then(|n| u64::try_from(n).ok())
        .ok_or(())?;
    let content = Content::synthetic(&id, timestamp)?;
    crate::email_wire::prepare(&profile, &content)?;
    let time = timestamp.to_string();
    let frame = Zeroizing::new(
        crate::provider_frame::frame(
            &[
                snapshot.bytes(),
                &credential,
                id.as_bytes(),
                time.as_bytes(),
                &trust,
            ],
            INPUT_LIMIT,
        )
        .map_err(|_| ())?,
    );
    println!(
        "Synthetic HTTPS email test: provider={} profile_revision={} subject=Slurm Notify test",
        profile.kind(),
        profile.revision()
    );
    println!("Synthetic Slurm Notify test; no scheduler data.\nEvent: {id}");
    std::io::stdout().flush().map_err(|_| ())?;
    let executable = std::env::current_exe().map_err(|_| ())?;
    let mut command = Command::new(executable);
    command.args(["--internal-email-test-v1"]);
    match crate::transport::supervise_with(
        &mut command,
        frame,
        profile.timeout,
        INPUT_LIMIT,
        64,
        Outcome::decode,
    ) {
        Ok(outcome) => Ok(outcome),
        Err(Error::Spawn | Error::InvalidInput) => Err(()),
        Err(_) => Ok(Outcome::Unknown),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_child_frames_never_reach_network() {
        for data in [vec![], vec![0; 20], vec![1; INPUT_LIMIT + 1]] {
            assert!(serve(data.as_slice(), Vec::new()).is_err());
        }
        let v = crate::email_profile::tests::fixture("gmail_api");
        let bytes = serde_json::to_vec(&v).unwrap();
        let id = crate::public_event::identifier([0; 16]);
        let frame =
            crate::provider_frame::frame(&[&bytes, b"{}", id.as_bytes(), b"0", b""], INPUT_LIMIT)
                .unwrap();
        let mut output = Vec::new();
        serve(frame.as_slice(), &mut output).unwrap();
        assert_eq!(output, b"NOT_ATTEMPTED\n");
        let mut extra = frame.clone();
        extra.push(0);
        assert!(serve(extra.as_slice(), Vec::new()).is_err());
        assert!(run(Path::new("/absent-protected-profile.json")).is_err());
    }
}

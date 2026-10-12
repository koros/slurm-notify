//! Private explicit environment source; never enumerates or changes environment.
use crate::credential_resolution::reference;
use std::ffi::OsString;
use zeroize::Zeroizing;

const MAX_BYTES: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Reference,
    Limit,
    Missing,
    Encoding,
    Value,
}

// std::env clones the selected OS value before we can check its length. This caps
// accepted/retained bytes, not that OS allocation or synchronous lookup duration.
pub(super) fn lookup(name: &str, ceiling: usize) -> Result<Zeroizing<Vec<u8>>, ()> {
    read_with(name, ceiling, |key| std::env::var_os(key)).map_err(|_| ())
}

fn read_with(
    name: &str,
    ceiling: usize,
    source: impl FnOnce(&str) -> Option<OsString>,
) -> Result<Zeroizing<Vec<u8>>, Error> {
    if !reference(name) {
        return Err(Error::Reference);
    }
    if !(1..=MAX_BYTES).contains(&ceiling) {
        return Err(Error::Limit);
    }
    let value = source(name).ok_or(Error::Missing)?;
    // Take ownership of the OS allocation, including on rejection. On Windows
    // encoded bytes are WTF-8; UTF-8 validation rejects unpaired surrogates.
    let bytes = Zeroizing::new(value.into_encoded_bytes());
    if bytes.is_empty() || bytes.len() > ceiling {
        return Err(Error::Value);
    }
    std::str::from_utf8(&bytes).map_err(|_| Error::Encoding)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits unit test bodies.
    use super::*;
    #[test]
    fn validation_precedes_access_and_errors_are_static() {
        for name in ["", "1NAME", "PRIVATE-NAME", "NAME\n", &"a".repeat(129)] {
            assert_eq!(
                read_with(name, 64, |_| panic!("no access")).err(),
                Some(Error::Reference)
            );
        }
        for ceiling in [0, 4097, usize::MAX] {
            assert_eq!(
                read_with("REF", ceiling, |_| panic!("no access")).err(),
                Some(Error::Limit)
            );
        }
        for (value, expected) in [
            (None, Error::Missing),
            (Some(OsString::new()), Error::Value),
            (Some(OsString::from("private-fixture")), Error::Value),
        ] {
            let error = read_with("REF", 2, |_| value).err().unwrap();
            assert_eq!(error, expected);
            assert!(!format!("{error:?}").contains("private-fixture"));
        }
    }
    #[test]
    fn exact_bytes_and_independent_owned_snapshots() {
        let value = read_with("_REF9", 4096, |_| Some(OsString::from("x".repeat(4096)))).unwrap();
        assert_eq!(value.len(), 4096);
        assert_eq!(
            read_with("REF", 64, |_| Some(OsString::from(" fixture \t")))
                .unwrap()
                .as_slice(),
            b" fixture \t"
        );
        assert_eq!(
            read_with("REF", 2, |_| Some(OsString::from("é")))
                .unwrap()
                .as_slice(),
            "é".as_bytes()
        );
        assert_eq!(
            read_with("REF", 1, |_| Some(OsString::from("é"))).err(),
            Some(Error::Value)
        );
    }
    #[cfg(unix)]
    #[test]
    fn non_unicode_is_rejected_without_lossy_conversion() {
        use std::os::unix::ffi::OsStringExt;
        assert_eq!(
            read_with("REF", 128, |_| Some(OsString::from_vec(vec![0xff]))).err(),
            Some(Error::Encoding)
        );
    }
    #[test]
    fn isolated_process_environment_resolves_credentials_and_webhook() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "environment_resolution::tests::environment_child",
            ])
            .env_clear()
            .env("FIXTURE_USER", "fixture-user")
            .env("FIXTURE_PASSWORD", "fixture password ")
            .env("FIXTURE_URL", "https://example.invalid/private-fixture")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "environment child failed");
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("environment child timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    #[test]
    #[ignore = "trusted environment child invoked with an isolated environment"]
    fn environment_child() {
        let credentials = crate::credential_resolution::resolve(
            "fixture-user",
            "FIXTURE_USER",
            "FIXTURE_PASSWORD",
            lookup,
        )
        .unwrap();
        assert!(credentials.into_plain().as_slice() == b"\0fixture-user\0fixture password ");
        let resources = crate::webhook_resolution::resolve(
            "FIXTURE_URL",
            None,
            std::time::Duration::from_secs(1),
            1024,
            lookup,
        )
        .unwrap();
        assert!(resources.into_frame("event", b"{}").is_ok());
        assert!(lookup("MISSING_REF", 64).is_err());
    }
}

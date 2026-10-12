//! Trusted startup reporting fixtures; no installed flags or success grammar.
use std::{io::Write, path::Path, process::Command, time::Duration};
use zeroize::Zeroizing;
pub(super) fn child() -> bool {
    let mode = std::env::args().nth(1);
    if !matches!(
        mode.as_deref(),
        Some("--startup-check" | "--startup-cancel" | "--startup-limit")
    ) {
        return false;
    }
    let Some(path) = std::env::args().nth(2) else {
        return true;
    };
    let result = crate::foreground_session::Session::start_reported(
        Path::new(&path),
        if mode.as_deref() == Some("--startup-limit") {
            0
        } else {
            1
        },
        Duration::from_millis(50),
        |_| Ok(Zeroizing::new(b"alice".to_vec())),
        |name, _| {
            assert!(!name.contains("SMTP"));
            Ok(Zeroizing::new(if name.ends_with("_URL") {
                b"https://fixture.invalid/hook".to_vec()
            } else {
                b"Bearer fixture".to_vec()
            }))
        },
        || mode.as_deref() == Some("--startup-cancel"),
    );
    match result {
        Err(report) => print!("{}", report.render()),
        Ok(mut session) => {
            session.stop();
            println!("foreground startup=ready");
        }
    }
    true
}
fn decode(bytes: &[u8]) -> Result<Vec<u8>, crate::transport::Error> {
    if bytes.len() <= crate::foreground_report::MAX_BYTES
        && matches!(
            bytes,
            b"foreground startup=cancelled\n"
                | b"foreground startup=failed_limit\n"
                | b"foreground startup=failed_preparation\n"
                | b"foreground startup=failed_ownership\n"
                | b"foreground startup=ready\n"
        )
    {
        Ok(bytes.to_vec())
    } else {
        Err(crate::transport::Error::InvalidInput)
    }
}
fn inspect(mode: &str, path: &Path, expected: &[u8]) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([std::ffi::OsStr::new(mode), path.as_os_str()]);
    assert_eq!(
        crate::transport::supervise_with(
            &mut command,
            Vec::new(),
            Duration::from_secs(2),
            0,
            crate::foreground_report::MAX_BYTES,
            decode
        )
        .unwrap(),
        expected
    );
}
fn write(path: &Path, bytes: &[u8]) {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}
pub(super) fn acceptance() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let directory = crate::storage::tests::Directory::new();
    let root = directory.path().parent().unwrap().canonicalize().unwrap();
    let config = root.join("startup.json");
    let state = root.join("owned");
    inspect(
        "--startup-cancel",
        &config,
        b"foreground startup=cancelled\n",
    );
    inspect(
        "--startup-limit",
        &config,
        b"foreground startup=failed_limit\n",
    );
    inspect(
        "--startup-check",
        &config,
        b"foreground startup=failed_preparation\n",
    );
    assert!(!config.exists() && !state.exists());
    let trust = root.join("root.der");
    let certificate =
        rcgen::generate_simple_self_signed(vec!["smtp.example.invalid".into()]).unwrap();
    write(&trust, certificate.cert.der());
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../docs/proposals/examples/local-mixed-config.json"
    ))
    .unwrap();
    source["storage"]["path"] = serde_json::json!(state.join("state.sqlite3"));
    source["providers"]["research_email"]["trust_der_files"] = serde_json::json!([trust]);
    let bytes = serde_json::to_vec(&source).unwrap();
    write(&config, &bytes);
    let malformed = root.join("malformed.json");
    write(&malformed, b"{private fixture malformed");
    let alias = root.join("alias.json");
    symlink(&config, &alias).unwrap();
    for path in [&malformed, &alias] {
        inspect(
            "--startup-check",
            path,
            b"foreground startup=failed_preparation\n",
        );
        assert!(!state.exists());
    }
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).unwrap();
    inspect(
        "--startup-check",
        &config,
        b"foreground startup=failed_preparation\n",
    );
    assert_eq!(std::fs::read(&config).unwrap(), bytes);
    assert!(!state.exists());
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
    // Unsafe state symlink is never followed or repaired.
    let target = root.join("untouched");
    std::fs::create_dir(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&target, &state).unwrap();
    inspect(
        "--startup-check",
        &config,
        b"foreground startup=failed_ownership\n",
    );
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    assert!(std::fs::symlink_metadata(&state)
        .unwrap()
        .file_type()
        .is_symlink());
    std::fs::remove_file(&state).unwrap();
    inspect("--startup-check", &config, b"foreground startup=ready\n");
    let before = crate::status_boundary::files(&state);
    let mut owner = crate::status_boundary::Running::start("--status-owner", &state);
    inspect(
        "--startup-check",
        &config,
        b"foreground startup=failed_ownership\n",
    );
    assert_eq!(crate::status_boundary::files(&state), before);
    owner.finish(false, b"CLOSED\n");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
    inspect(
        "--startup-check",
        &config,
        b"foreground startup=failed_ownership\n",
    );
    assert_eq!(crate::status_boundary::files(&state), before);
    assert_eq!(
        std::fs::metadata(&state).unwrap().permissions().mode() & 0o777,
        0o755
    );
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    inspect("--startup-check", &config, b"foreground startup=ready\n");
    assert_eq!(crate::status_boundary::files(&state), before);
    assert!(decode(b"foreground startup=private-path\n").is_err());
    assert!(decode(&[b'x'; 97]).is_err());
}

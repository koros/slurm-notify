use std::ffi::OsStr;
use std::process::{Command, Output};

const HELP: &[u8] = include_bytes!("../../../contracts/cli/help.stdout");
const VERSION: &[u8] = include_bytes!("../../../contracts/cli/version.stdout");
const USAGE: &[u8] = include_bytes!("../../../contracts/cli/no-arguments.stderr");
const INVALID: &[u8] = include_bytes!("../../../contracts/cli/invalid-invocation.stderr");

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_slurm-notify"));
    command.env_clear();
    // Keep instrumentation outside the resource sandbox during coverage runs.
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
}

fn run(args: &[&OsStr]) -> Output {
    command().args(args).output().unwrap()
}

fn expect(output: Output, code: i32, stdout: &[u8], stderr: &[u8]) {
    assert_eq!(output.status.code(), Some(code));
    assert_eq!(output.stdout, stdout);
    assert_eq!(output.stderr, stderr);
}

#[test]
fn approved_help_version_and_empty_invocation() {
    expect(run(&[OsStr::new("--help")]), 0, HELP, b"");
    expect(run(&[OsStr::new("--version")]), 0, VERSION, b"");
    expect(run(&[]), 2, b"", USAGE);
}

#[test]
fn rejects_other_grammars_without_echoing_values() {
    for args in [
        vec!["--help", "--help"],
        vec!["--version", "--version"],
        vec!["--help", "--version"],
        vec!["--version", "--help"],
        vec!["-h"],
        vec!["-V"],
        vec!["watch"],
        vec!["--unknown"],
        vec!["--"],
        vec!["--help", "secret-path"],
        vec!["--version=secret"],
        vec!["--config", "secret"],
        vec![""],
    ] {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        expect(run(&args), 2, b"", INVALID);
    }
}

#[cfg(unix)]
#[test]
fn rejects_non_unicode_arguments_without_lossy_conversion() {
    use std::os::unix::ffi::OsStrExt;
    let invalid = OsStr::from_bytes(b"private-\xff");
    expect(run(&[invalid]), 2, b"", INVALID);
    expect(run(&[OsStr::new("--help"), invalid]), 2, b"", INVALID);
}

#[test]
fn ignores_unusable_environment_and_does_not_create_resources() {
    let root = std::env::temp_dir().join(format!("slurm-notify-cli-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let blocker = root.join("not-a-directory");
    std::fs::write(&blocker, b"unchanged").unwrap();
    for (args, code, stdout, stderr) in [
        (vec!["--help"], 0, HELP, b"".as_slice()),
        (vec!["--version"], 0, VERSION, b"".as_slice()),
        (vec![], 2, b"".as_slice(), USAGE),
        (vec!["watch"], 2, b"".as_slice(), INVALID),
    ] {
        let output = command()
            .args(args)
            .current_dir(&root)
            .env("HOME", &blocker)
            .env("XDG_CONFIG_HOME", blocker.join("config"))
            .env("XDG_STATE_HOME", root.join("absent-state"))
            .env("PATH", root.join("absent-tools"))
            .env("SLURM_NOTIFY_CONFIG", blocker.join("private-config"))
            .output()
            .unwrap();
        expect(output, code, stdout, stderr);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(std::fs::read(&blocker).unwrap(), b"unchanged");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn installed_config_validation_checks_resources_and_preserves_files() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("slurm-notify-config-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("private.json");
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../contracts/config/webhook-v1.example.json"
    ))
    .unwrap();
    source["storage"]["path"] = serde_json::json!(root.join("absent-state/state.sqlite3"));
    let bytes = serde_json::to_vec(&source).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let invalid = include_bytes!("../../../contracts/cli/config-invalid.stderr");
    let valid = include_bytes!("../../../contracts/cli/config-valid.stdout");
    let invoke = |args: &[&OsStr], url: Option<&OsStr>, auth: Option<&OsStr>| {
        let mut process = command();
        process.args(args).current_dir(&root);
        if let Some(value) = url {
            process.env("SLURM_NOTIFY_WEBHOOK_URL", value);
        }
        if let Some(value) = auth {
            process.env("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION", value);
        }
        process.output().unwrap()
    };
    let config_args = [
        OsStr::new("config"),
        OsStr::new("validate"),
        OsStr::new("--config"),
        path.as_os_str(),
    ];
    for args in [
        config_args,
        [
            OsStr::new("--config"),
            path.as_os_str(),
            OsStr::new("config"),
            OsStr::new("validate"),
        ],
        [
            OsStr::new("config"),
            OsStr::new("--config"),
            path.as_os_str(),
            OsStr::new("validate"),
        ],
    ] {
        expect(
            invoke(
                &args,
                Some(OsStr::new(
                    "https://unreachable.invalid/private-secret?q=secret",
                )),
                Some(OsStr::new("Bearer private-secret")),
            ),
            0,
            valid,
            b"",
        );
    }
    for url in [
        None,
        Some("http://127.0.0.1/private"),
        Some("https://user:secret@invalid.test"),
        Some("https://invalid.test/#secret"),
        Some("https://invalid.test/\n"),
        Some(""),
    ] {
        expect(
            invoke(
                &config_args,
                url.map(OsStr::new),
                Some(OsStr::new("Bearer private-secret")),
            ),
            2,
            b"",
            invalid,
        );
    }
    expect(
        invoke(&config_args, Some(OsStr::new("https://invalid.test")), None),
        2,
        b"",
        invalid,
    );
    expect(
        invoke(
            &config_args,
            Some(OsStr::new("https://invalid.test")),
            Some(OsStr::new("Bearer secret\n")),
        ),
        2,
        b"",
        invalid,
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    expect(invoke(&config_args, None, None), 2, b"", invalid);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = root.join("symlink.json");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    expect(
        invoke(
            &[
                OsStr::new("config"),
                OsStr::new("validate"),
                OsStr::new("--config"),
                link.as_os_str(),
            ],
            None,
            None,
        ),
        2,
        b"",
        invalid,
    );
    std::fs::remove_file(link).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    assert!(!root.join("absent-state").exists());
    std::fs::write(
        &path,
        br#"{"schema_version":1,"schema_version":1,"secret":"private"}"#,
    )
    .unwrap();
    expect(invoke(&config_args, None, None), 2, b"", invalid);
    std::fs::write(
        &path,
        include_bytes!("../../../docs/proposals/examples/local-mixed-config.json"),
    )
    .unwrap();
    expect(
        invoke(&config_args, None, None),
        2,
        b"",
        include_bytes!("../../../contracts/cli/config-invalid.stderr"),
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_grammar_and_help_require_no_resources() {
    let help = include_bytes!("../../../contracts/cli/config-help.stdout");
    for args in [
        ["config", "validate", "--help"],
        ["--help", "config", "validate"],
    ] {
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        expect(run(&args), 0, help, b"");
    }
    for args in [
        vec!["config", "validate"],
        vec!["config", "validate", "--config", "relative"],
        vec![
            "config", "validate", "--config", "/private", "--config", "/private",
        ],
        vec!["config", "validate", "--config", "/private", "--help"],
        vec!["start", "--config", "/private"],
        vec!["config", "validate", "--unknown", "/private"],
    ] {
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        expect(run(&args), 2, b"", INVALID);
    }
}

#[path = "../src/config_reference.rs"]
mod config_reference;
#[path = "../src/rendering_fingerprint.rs"]
mod rendering_fingerprint;
#[path = "../src/smtp_provider_revision.rs"]
mod smtp_provider_revision;

#[cfg(unix)]
#[test]
fn installed_smtp_validation_approves_exact_pairs_without_state_or_network() {
    use ring::digest::{digest, SHA256};
    use std::os::unix::fs::PermissionsExt;
    let directory =
        std::env::temp_dir().join(format!("slurm-notify-smtp-cli-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.join("config.json");
    let trust_path = directory.join("trust.der");
    let certificate =
        rcgen::generate_simple_self_signed(vec!["smtp.example.invalid".into()]).unwrap();
    std::fs::write(&trust_path, certificate.cert.der()).unwrap();
    std::fs::set_permissions(&trust_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../docs/proposals/examples/local-mixed-config.json"
    ))
    .unwrap();
    source["providers"]
        .as_object_mut()
        .unwrap()
        .remove("primary");
    source["routes"] =
        serde_json::json!([{"events":["JOB_COMPLETED"],"send_to":["research_email"]}]);
    source["storage"]["path"] = serde_json::json!(directory.join("absent-state/state.sqlite3"));
    source["providers"]["research_email"]["trust_der_files"] = serde_json::json!([trust_path]);
    let trust_hash =
        smtp_provider_revision::trust_fingerprint(&[certificate.cert.der().as_ref()]).unwrap();
    let trust_reference = config_reference::reference(b"trust\0", &[trust_path.to_str().unwrap()]);
    let credential_reference = config_reference::reference(
        b"credentials\0",
        &["SLURM_NOTIFY_SMTP_USERNAME", "SLURM_NOTIFY_SMTP_PASSWORD"],
    );
    let provider = smtp_provider_revision::Descriptor::new(&smtp_provider_revision::Profile {
        destination: "research_email",
        relay: "smtp.example.invalid",
        port: 587,
        tls: smtp_provider_revision::TlsMode::RequiredStartTls,
        server_name: "smtp.example.invalid",
        trust_reference: &trust_reference,
        trust_fingerprint: &trust_hash,
        authentication_identity: "researcher",
        credential_reference: &credential_reference,
    })
    .unwrap();
    let canonical = rendering_fingerprint::encode(&[
        ("destination", "research_email"),
        ("sender", "researcher@example.invalid"),
        ("recipient", "researcher@example.invalid"),
        ("namespace", "research_email"),
        ("disclosure", "hidden"),
        ("renderer", "allowlist-v1"),
        ("identity", "sha256-event-route-revision-v2"),
        ("domain", "fixture.invalid"),
    ])
    .unwrap();
    let rendering: String = digest(&SHA256, &canonical)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    source["delivery"]["smtp_permissions"] = serde_json::json!([{
        "cluster_id":"research-cluster", "destination":"research_email",
        "rendering_revision":rendering, "provider_revision":provider.token()
    }]);
    let write = |source: &serde_json::Value| {
        std::fs::write(&path, serde_json::to_vec(source).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    };
    let invoke = |credentials: bool| {
        let mut process = command();
        process
            .args([
                OsStr::new("config"),
                OsStr::new("validate"),
                OsStr::new("--config"),
                path.as_os_str(),
            ])
            .current_dir(&directory);
        if credentials {
            process
                .env("SLURM_NOTIFY_SMTP_USERNAME", "researcher")
                .env("SLURM_NOTIFY_SMTP_PASSWORD", "private-fixture-password");
        }
        process.output().unwrap()
    };
    write(&source);
    expect(invoke(true),0,b"Configuration valid: schema_version=1 cluster=research-cluster destinations=1 smtp_permissions=approved\n",b"");
    expect(
        invoke(false),
        2,
        b"",
        include_bytes!("../../../contracts/cli/config-invalid.stderr"),
    );
    let mut denied = source.clone();
    denied["delivery"]["smtp_permissions"] = serde_json::json!([]);
    write(&denied);
    expect(
        invoke(false),
        3,
        b"",
        include_bytes!("../../../contracts/cli/config-permission.stderr"),
    );
    let mut changed = source.clone();
    changed["providers"]["research_email"]["relay"] = serde_json::json!("changed.example.invalid");
    write(&changed);
    expect(
        invoke(false),
        3,
        b"",
        include_bytes!("../../../contracts/cli/config-permission.stderr"),
    );
    write(&source);
    std::fs::set_permissions(&trust_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    expect(
        invoke(true),
        2,
        b"",
        include_bytes!("../../../contracts/cli/config-invalid.stderr"),
    );
    std::fs::set_permissions(&trust_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&trust_path, b"invalid private certificate").unwrap();
    expect(
        invoke(true),
        2,
        b"",
        include_bytes!("../../../contracts/cli/config-invalid.stderr"),
    );
    assert!(!directory.join("absent-state").exists());
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn installed_status_is_existing_only_read_only_private_and_bounded() {
    use std::os::{fd::AsRawFd, unix::fs::PermissionsExt};
    let root = std::env::temp_dir().join(format!("slurm-notify-status-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("config.json");
    let state = root.join("state");
    let database = state.join("state.sqlite3");
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../contracts/config/local-mixed-v1.example.json"
    ))
    .unwrap();
    source["storage"]["path"] = serde_json::json!(database);
    let write_config = |source: &serde_json::Value| {
        std::fs::write(&path, serde_json::to_vec(source).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    };
    write_config(&source);
    let invoke = || {
        command()
            .args([
                OsStr::new("status"),
                OsStr::new("--config"),
                path.as_os_str(),
            ])
            .current_dir(&root)
            .output()
            .unwrap()
    };
    expect(
        invoke(),
        1,
        b"",
        include_bytes!("../../../contracts/cli/status-missing.stderr"),
    );
    assert!(!state.exists());
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let lock_path = state.join(".owner.lock");
    std::fs::write(&lock_path, b"").unwrap();
    std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let db = rusqlite::Connection::open(&database).unwrap();
    for migration in [
        include_str!("../../notify-storage/migrations/0001.sql"),
        include_str!("../../notify-storage/migrations/0002.sql"),
        include_str!("../../notify-storage/migrations/0003.sql"),
        include_str!("../../notify-storage/migrations/0004.sql"),
    ] {
        db.execute_batch(migration).unwrap();
    }
    drop(db);
    std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o600)).unwrap();
    let before = std::fs::read(&database).unwrap();
    expect(
        invoke(),
        0,
        include_bytes!("../../../contracts/cli/status-empty.stdout"),
        b"",
    );
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert_eq!(std::fs::read_dir(&state).unwrap().count(), 2);
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch("BEGIN IMMEDIATE;").unwrap();
    db.execute_batch(include_str!("../../notify-storage/migrations/0005.sql"))
        .unwrap();
    db.execute_batch("COMMIT;").unwrap();
    drop(db);
    let version_five = std::fs::read(&database).unwrap();
    expect(
        invoke(),
        0,
        include_bytes!("../../../contracts/cli/status-empty.stdout"),
        b"",
    );
    assert_eq!(std::fs::read(&database).unwrap(), version_five);
    let owner = std::fs::File::open(&lock_path).unwrap();
    assert_eq!(
        unsafe { libc::flock(owner.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    expect(
        invoke(),
        1,
        b"",
        include_bytes!("../../../contracts/cli/status-busy.stderr"),
    );
    drop(owner);
    expect(
        invoke(),
        0,
        include_bytes!("../../../contracts/cli/status-empty.stdout"),
        b"",
    );
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch("INSERT INTO jobs VALUES ('research-cluster','1','s','actual','COMPLETED',1);
        INSERT INTO events VALUES ('private-job-id','research-cluster','1','s','actual','JOB_COMPLETED',1);
        INSERT INTO outbox VALUES ('private-job-id','private-destination','pending',1);
        INSERT INTO delivery_attempts VALUES ('private-job-id','private-destination',1,1,'transient_failure');").unwrap();
    drop(db);
    let before = std::fs::read(&database).unwrap();
    let output = invoke();
    assert!(output.status.success());
    let text = std::str::from_utf8(&output.stdout).unwrap();
    assert!(
        text.contains("queued=1 due=1 retrying=1 failed=0 delivered=0 sampled=1 truncated=false")
    );
    assert!(!text.contains("private-job") && !text.contains("private-destination"));
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read(&database).unwrap(), before);
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch(
        "INSERT INTO outbox VALUES ('private-job-id','second-private','failed',1);
        INSERT INTO checkpoints VALUES ('research-cluster','1970-01-01T00:00:00');",
    )
    .unwrap();
    drop(db);
    let report = invoke();
    assert!(report.status.success());
    let text = String::from_utf8(report.stdout).unwrap();
    assert!(text.contains("failed=1") && text.contains("completion_unproven=1"));
    let now = time::OffsetDateTime::now_utc();
    let checkpoint = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute("UPDATE checkpoints SET history_through=?1", [checkpoint])
        .unwrap();
    drop(db);
    let report = invoke();
    assert!(report.status.success());
    assert!(String::from_utf8(report.stdout)
        .unwrap()
        .contains("stale=false"));
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute(
        "UPDATE checkpoints SET history_through='1970-01-01T00:00:00'",
        [],
    )
    .unwrap();
    drop(db);
    source["maintenance"]["health_sample_limit"] = serde_json::json!(1);
    write_config(&source);
    let output = invoke();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("stale=true") && text.contains("truncated=true"));
    assert!(!text.contains("age_seconds=unknown"));
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute(
        "UPDATE checkpoints SET history_through='9999-01-01T00:00:00'",
        [],
    )
    .unwrap();
    drop(db);
    expect(
        invoke(),
        1,
        b"",
        include_bytes!("../../../contracts/cli/status-failed.stderr"),
    );
    let db = rusqlite::Connection::open(&database).unwrap();
    db.pragma_update(None, "user_version", 99).unwrap();
    drop(db);
    expect(
        invoke(),
        1,
        b"",
        include_bytes!("../../../contracts/cli/status-version.stderr"),
    );
    std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o644)).unwrap();
    expect(
        invoke(),
        1,
        b"",
        include_bytes!("../../../contracts/cli/status-unsafe.stderr"),
    );
    std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(state.join("state.sqlite3-wal"), b"private-sidecar").unwrap();
    std::fs::set_permissions(
        state.join("state.sqlite3-wal"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    expect(
        invoke(),
        1,
        b"",
        include_bytes!("../../../contracts/cli/status-unsafe.stderr"),
    );
    assert_eq!(
        std::fs::read(state.join("state.sqlite3-wal")).unwrap(),
        b"private-sidecar"
    );
    std::fs::remove_file(state.join("state.sqlite3-wal")).unwrap();
    std::fs::remove_file(&database).unwrap();
    let target = root.join("private-target");
    std::fs::write(&target, b"private target bytes").unwrap();
    std::os::unix::fs::symlink(&target, &database).unwrap();
    expect(
        invoke(),
        1,
        b"",
        include_bytes!("../../../contracts/cli/status-unsafe.stderr"),
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"private target bytes");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn status_grammar_and_help_do_not_resolve_resources() {
    for args in [["status", "--help"], ["--help", "status"]] {
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        expect(
            run(&args),
            0,
            include_bytes!("../../../contracts/cli/status-help.stdout"),
            b"",
        );
    }
    for args in [
        vec!["status"],
        vec!["status", "--config", "relative"],
        vec!["status", "--config", "/private", "--config", "/private"],
        vec!["status", "--config", "/private", "extra"],
        vec!["status", "--unknown", "/private"],
        vec!["status", "--config", "/private", "--help"],
    ] {
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        expect(run(&args), 2, b"", INVALID);
    }
    expect(
        run(&[
            OsStr::new("--config"),
            OsStr::new("/slurm-notify-missing/config"),
            OsStr::new("status"),
        ]),
        2,
        b"",
        include_bytes!("../../../contracts/cli/config-invalid.stderr"),
    );
}

#[test]
fn installed_webhook_helper_rejects_malformed_private_input_without_output_or_state() {
    use std::io::Write;
    use std::process::Stdio;
    let id = b"evt_00000000000040008000000000000000";
    let payload = br#"{"event_id":"evt_00000000000040008000000000000000"}"#;
    let frame = |fields: &[&[u8]]| {
        let mut bytes = Vec::new();
        for field in fields {
            bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
            bytes.extend_from_slice(field);
        }
        bytes
    };
    let mut rejected = vec![
        vec![],
        vec![0],
        u32::MAX.to_be_bytes().to_vec(),
        frame(&[b"https://fixture.invalid/private-token", b"", id, payload]),
        frame(&[
            b"http://127.0.0.1:9/private-token",
            b"",
            id,
            payload,
            b"1000",
        ]),
        frame(&[
            b"https://fixture.invalid/private-token",
            b"",
            b"private-attempt",
            payload,
            b"1000",
        ]),
        frame(&[
            b"https://fixture.invalid/private-token",
            b"",
            id,
            b"{}",
            b"1000",
        ]),
        frame(&[
            b"https://fixture.invalid/private-token",
            b"",
            id,
            b"not-json",
            b"1000",
        ]),
        frame(&[
            b"https://fixture.invalid/private-token",
            b"Bearer secret\r\n",
            id,
            payload,
            b"1000",
        ]),
    ];
    for timeout in [b"0".as_slice(), b"30001", b"01000", b"invalid"] {
        rejected.push(frame(&[
            b"https://fixture.invalid/private-token",
            b"",
            id,
            payload,
            timeout,
        ]));
    }
    for (endpoint, auth, body) in [
        (vec![b'x'; 4097], vec![], payload.to_vec()),
        (
            b"https://fixture.invalid/private-token".to_vec(),
            vec![b'x'; 4097],
            payload.to_vec(),
        ),
        (
            b"https://fixture.invalid/private-token".to_vec(),
            vec![],
            vec![b'x'; 16385],
        ),
    ] {
        rejected.push(frame(&[&endpoint, &auth, id, &body, b"1000"]));
    }
    let mut trailing = frame(&[
        b"https://fixture.invalid/private-token",
        b"",
        id,
        payload,
        b"1000",
    ]);
    trailing.push(0);
    rejected.push(trailing);
    for roots in [
        vec![],
        vec![0],
        vec![0, 0, 0, 5, 1],
        vec![0; 4 * 33],
        vec![0, 0, 0, 1, 0],
    ] {
        rejected.push(frame(&[
            b"https://fixture.invalid/private-token",
            b"",
            id,
            payload,
            b"1000",
            &roots,
        ]));
    }
    for input in rejected {
        let mut child = command()
            .args(["--internal-provider-v1", "webhook"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child.stdin.take().unwrap().write_all(&input);
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }
    for args in [
        ["--internal-provider-v1", "webhook", "extra"],
        ["--internal-provider-v1", "smtp-starttls", "extra"],
    ] {
        let output = command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(output.stderr, INVALID);
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn installed_smtp_helper_validates_bound_frames_and_never_authenticates_without_starttls() {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        process::Stdio,
        time::Duration,
    };
    let frame = |fields: &[&[u8]]| {
        let mut bytes = Vec::new();
        for field in fields {
            bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
            bytes.extend_from_slice(field);
        }
        bytes
    };
    let certificate =
        rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()]).unwrap();
    let root = certificate.cert.der();
    let hash = smtp_provider_revision::trust_fingerprint(&[root.as_ref()]).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let profile = smtp_provider_revision::Profile {
        destination: "smtp",
        relay: "127.0.0.1",
        port: listener.local_addr().unwrap().port(),
        tls: smtp_provider_revision::TlsMode::RequiredStartTls,
        server_name: "smtp.fixture.invalid",
        trust_reference: "fixture-trust",
        trust_fingerprint: &hash,
        authentication_identity: "fixture-user",
        credential_reference: "fixture-secret",
    };
    let descriptor = smtp_provider_revision::Descriptor::new(&profile).unwrap();
    let roots = frame(&[root.as_ref()]);
    let credentials = b"\0fixture-user\0fixture-password";
    let data = b"Subject: fixture\r\n\r\nfixture\r\n.\r\n";
    let fields: [&[u8]; 8] = [
        descriptor.canonical(),
        descriptor.token().as_bytes(),
        &roots,
        credentials,
        b"s@fixture.invalid",
        b"r@fixture.invalid",
        data,
        b"1000",
    ];
    let run_helper = |input: &[u8]| {
        let mut child = command()
            .args(["--internal-provider-v1", "smtp-starttls"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child.stdin.take().unwrap().write_all(input);
        child.wait_with_output().unwrap()
    };
    let mut invalid = vec![vec![], vec![0], u32::MAX.to_be_bytes().to_vec()];
    for (index, value) in [
        (0, b"invalid-profile".as_slice()),
        (1, b"invalid-fingerprint"),
        (2, b"bad-roots"),
        (3, b"bad-credentials"),
        (3, b"\0wrong-user\0fixture-password"),
        (4, b"bad address"),
        (6, b"invalid frozen message"),
        (7, b"0"),
        (7, b"01000"),
        (7, b"30001"),
    ] {
        let mut changed = fields;
        changed[index] = value;
        invalid.push(frame(&changed));
    }
    let oversized = vec![0; 65665];
    let mut changed = fields;
    changed[2] = &oversized;
    invalid.push(frame(&changed));
    let mut trailing = frame(&fields);
    trailing.push(0);
    invalid.push(trailing);
    let implicit = smtp_provider_revision::Descriptor::new(&smtp_provider_revision::Profile {
        tls: smtp_provider_revision::TlsMode::Implicit,
        ..profile
    })
    .unwrap();
    let mut changed = fields;
    changed[0] = implicit.canonical();
    changed[1] = implicit.token().as_bytes();
    invalid.push(frame(&changed));
    for input in invalid {
        let output = run_helper(&input);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"SMTP_SUBMIT AUTH INPUT\n");
        assert!(output.stderr.is_empty());
    }
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket.write_all(b"220 fixture\r\n").unwrap();
        let mut reader = BufReader::new(socket);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("EHLO "));
        reader.get_mut().write_all(b"250 fixture\r\n").unwrap();
        assert_eq!(
            reader.read(&mut [0; 1]).unwrap(),
            0,
            "AUTH must not be sent without STARTTLS"
        );
    });
    let output = run_helper(&frame(&fields));
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"SMTP_SUBMIT AUTH TLS_MISSING\n");
    assert!(output.stderr.is_empty());
    server.join().unwrap();
}

#[test]
fn installed_smtp_helpers_replay_identical_frozen_message_after_temporary_final_reply() {
    use base64::Engine;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        process::Stdio,
        sync::Arc,
        time::{Duration, Instant},
    };
    let frame = |fields: &[&[u8]]| {
        let mut bytes = Vec::new();
        for field in fields {
            bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
            bytes.extend_from_slice(field);
        }
        bytes
    };
    let cert = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()]).unwrap();
    let root = cert.cert.der().to_vec();
    let server_config = Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(root.clone())],
                rustls::pki_types::PrivateKeyDer::Pkcs8(
                    rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()),
                ),
            )
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let hash = smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
    let descriptor = smtp_provider_revision::Descriptor::new(&smtp_provider_revision::Profile {
        destination: "smtp",
        relay: "127.0.0.1",
        port: listener.local_addr().unwrap().port(),
        tls: smtp_provider_revision::TlsMode::RequiredStartTls,
        server_name: "smtp.fixture.invalid",
        trust_reference: "fixture-trust",
        trust_fingerprint: &hash,
        authentication_identity: "fixture-user",
        credential_reference: "fixture-secret",
    })
    .unwrap();
    let data = b"Date: Thu, 01 Jan 1970 00:00:10 +0000\r\nMessage-ID: <frozen-fixture@fixture.invalid>\r\nSubject: fixture\r\n\r\nfixture\r\n..dot\r\n.\r\n";
    let roots = frame(&[&root]);
    let request = frame(&[
        descriptor.canonical(),
        descriptor.token().as_bytes(),
        &roots,
        b"\0fixture-user\0fixture-password",
        b"s@fixture.invalid",
        b"r@fixture.invalid",
        data,
        b"2000",
    ]);
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let mut received = Vec::new();
        for attempt in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(10);
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("fixture accept failed: {error:?}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut plain = BufReader::new(socket);
            plain.get_mut().write_all(b"220 fixture\r\n").unwrap();
            let mut line = String::new();
            plain.read_line(&mut line).unwrap();
            assert!(line.starts_with("EHLO "));
            plain
                .get_mut()
                .write_all(b"250-fixture\r\n250 STARTTLS\r\n")
                .unwrap();
            line.clear();
            plain.read_line(&mut line).unwrap();
            assert_eq!(line, "STARTTLS\r\n");
            assert!(plain.buffer().is_empty());
            plain.get_mut().write_all(b"220 upgrade\r\n").unwrap();
            let stream = rustls::StreamOwned::new(
                rustls::ServerConnection::new(server_config.clone()).unwrap(),
                plain.into_inner(),
            );
            let mut tls = BufReader::new(stream);
            line.clear();
            tls.read_line(&mut line).unwrap();
            assert!(line.starts_with("EHLO "));
            tls.get_mut()
                .write_all(b"250-fixture\r\n250 AUTH PLAIN\r\n")
                .unwrap();
            tls.get_mut().flush().unwrap();
            line.clear();
            tls.read_line(&mut line).unwrap();
            assert_eq!(
                line,
                format!(
                    "AUTH PLAIN {}\r\n",
                    base64::engine::general_purpose::STANDARD
                        .encode(b"\0fixture-user\0fixture-password")
                )
            );
            tls.get_mut().write_all(b"235 authenticated\r\n").unwrap();
            tls.get_mut().flush().unwrap();
            for (expected, reply) in [
                (
                    "MAIL FROM:<s@fixture.invalid>\r\n",
                    b"250 mail\r\n".as_slice(),
                ),
                ("RCPT TO:<r@fixture.invalid>\r\n", b"250 recipient\r\n"),
                ("DATA\r\n", b"354 data\r\n"),
            ] {
                line.clear();
                tls.read_line(&mut line).unwrap();
                assert_eq!(line, expected);
                tls.get_mut().write_all(reply).unwrap();
                tls.get_mut().flush().unwrap();
            }
            let mut body = Vec::new();
            loop {
                let mut part = Vec::new();
                assert!(tls.read_until(b'\n', &mut part).unwrap() > 0);
                let end = part == b".\r\n";
                body.extend(part);
                if end {
                    break;
                }
            }
            received.push(body);
            tls.get_mut()
                .write_all(if attempt == 0 {
                    b"451 temporary\r\n"
                } else {
                    b"250 accepted\r\n"
                })
                .unwrap();
            tls.get_mut().flush().unwrap();
            // Complete the client's orderly end before closing the fixture socket.
            if attempt == 1 {
                line.clear();
                let _ = tls.read_line(&mut line);
            }
        }
        received
    });
    for expected in [
        b"SMTP_SUBMIT FINAL TEMP NA\n".as_slice(),
        b"SMTP_SUBMIT FINAL ACCEPTED RA\n",
    ] {
        let mut child = command()
            .args(["--internal-provider-v1", "smtp-starttls"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&request).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("SMTP helper exceeded fixture deadline");
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let mut stdout = Vec::new();
        child
            .stdout
            .take()
            .unwrap()
            .take(129)
            .read_to_end(&mut stdout)
            .unwrap();
        let mut stderr = Vec::new();
        child
            .stderr
            .take()
            .unwrap()
            .take(129)
            .read_to_end(&mut stderr)
            .unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(0));
        assert_eq!(stdout, expected);
        assert!(stderr.is_empty());
    }
    let received = server.join().unwrap();
    assert_eq!(received, vec![data.to_vec(), data.to_vec()]);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn installed_foreground_grammar_and_help_are_explicit_and_resource_free() {
    let help = include_bytes!("../../../contracts/cli/start-help.stdout");
    for args in [["start", "--help"], ["--help", "start"]] {
        expect(command().args(args).output().unwrap(), 0, help, b"");
    }
    for args in [
        vec!["start"],
        vec!["start", "--foreground"],
        vec!["start", "--config", "/private"],
        vec!["start", "--foreground", "--config", "relative"],
        vec!["start", "--foreground", "--foreground", "/private"],
        vec!["start", "--foreground", "--config", "/private", "extra"],
    ] {
        expect(command().args(args).output().unwrap(), 2, b"", INVALID);
    }
    for args in [
        vec![
            "start",
            "--foreground",
            "--config",
            "/nonexistent-private-start-config",
        ],
        vec![
            "--config",
            "/nonexistent-private-start-config",
            "--foreground",
            "start",
        ],
    ] {
        expect(
            command().args(args).output().unwrap(),
            2,
            b"",
            include_bytes!("../../../contracts/cli/config-invalid.stderr"),
        );
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn installed_foreground_keeps_blocked_smtp_frozen_across_signals_and_restart() {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
        process::Stdio,
        sync::mpsc,
        time::{Duration, Instant},
    };
    struct Reap(std::process::Child);
    impl std::ops::Deref for Reap {
        type Target = std::process::Child;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl std::ops::DerefMut for Reap {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root = std::env::temp_dir().join(format!("slurm-notify-start-cli-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = root.join("config.json");
    let trust = root.join("trust.der");
    let state = root.join("state");
    let cert = rcgen::generate_simple_self_signed(vec!["smtp.example.invalid".into()]).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&trust)
        .unwrap()
        .write_all(cert.cert.der())
        .unwrap();
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../docs/proposals/examples/local-mixed-config.json"
    ))
    .unwrap();
    source["providers"]
        .as_object_mut()
        .unwrap()
        .remove("primary");
    source["providers"]["research_email"]["trust_der_files"] = serde_json::json!([trust]);
    source["routes"] =
        serde_json::json!([{"events":["JOB_COMPLETED"],"send_to":["research_email"]}]);
    source["storage"]["path"] = serde_json::json!(state.join("state.sqlite3"));
    source["monitor"]["initial_observation"] = serde_json::json!("emit");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&config)
        .unwrap()
        .write_all(&serde_json::to_vec(&source).unwrap())
        .unwrap();
    for (name,body) in [("squeue","#!/bin/sh\nexit 0\n"),
        ("sacct","#!/bin/sh\nfor arg in \"$@\"; do case \"$arg\" in --user=*) user=${arg#--user=};; esac; done\nprintf 'research-cluster|1|%s|s|a|COMPLETED|0:0\\n' \"$user\"\n")] {
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o700).open(root.join(name)).unwrap().write_all(body.as_bytes()).unwrap();
    }
    // Missing scheduler resources fail before state creation.
    expect(
        command()
            .args(["start", "--foreground", "--config"])
            .arg(&config)
            .output()
            .unwrap(),
        2,
        b"",
        b"Required Slurm executables unavailable in absolute PATH directories.\n",
    );
    assert!(!state.exists());
    let mut previous = None;
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let child = command()
            .args(["start", "--foreground", "--config"])
            .arg(&config)
            .env("PATH", &root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut child = Reap(child);
        let stderr = child.stderr.take().unwrap();
        let (send, receive) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr.take(4096));
            loop {
                let mut line = Vec::new();
                if reader.read_until(b'\n', &mut line).unwrap() == 0 {
                    break;
                }
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            b"Foreground monitoring started.\n"
        );
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            b"Foreground delivery blocked; work remains pending.\n"
        );
        let busy = command()
            .args(["status", "--config"])
            .arg(&config)
            .output()
            .unwrap();
        expect(
            busy,
            1,
            b"",
            include_bytes!("../../../contracts/cli/status-busy.stderr"),
        );
        expect(
            command()
                .args(["start", "--foreground", "--config"])
                .arg(&config)
                .env("PATH", &root)
                .output()
                .unwrap(),
            1,
            b"",
            b"Foreground monitoring could not acquire or initialize safe state.\n",
        );
        // SAFETY: signal is a valid constant and PID belongs to this test's child.
        assert_eq!(
            unsafe { libc::kill(child.id().try_into().unwrap(), signal) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("foreground shutdown exceeded fixture deadline");
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(
            status.code(),
            Some(if signal == libc::SIGINT { 130 } else { 143 })
        );
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            if signal == libc::SIGINT {
                b"Foreground monitoring stopped by SIGINT.\n".as_slice()
            } else {
                b"Foreground monitoring stopped by SIGTERM.\n"
            }
        );
        reader.join().unwrap();
        let mut stdout = Vec::new();
        child
            .stdout
            .take()
            .unwrap()
            .take(4096)
            .read_to_end(&mut stdout)
            .unwrap();
        assert!(stdout.is_empty());
        let db = rusqlite::Connection::open_with_flags(
            state.join("state.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let frozen:(String,Vec<u8>,i64,i64)=db.query_row("SELECT m.event_id,m.data,(SELECT COUNT(*) FROM outbox),(SELECT COUNT(*) FROM delivery_attempts) FROM smtp_messages m",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        assert_eq!((frozen.2, frozen.3), (1, 0));
        if let Some(previous) = &previous {
            assert_eq!(&frozen, previous);
        }
        previous = Some(frozen);
    }
    // A malformed checkpoint is fatal rather than an endless retry loop.
    let db = rusqlite::Connection::open(state.join("state.sqlite3")).unwrap();
    db.execute("UPDATE checkpoints SET history_through='corrupt'", [])
        .unwrap();
    drop(db);
    let output = command()
        .args(["start", "--foreground", "--config"])
        .arg(&config)
        .env("PATH", &root)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"Foreground monitoring started.\nForeground monitoring stopped after a runtime or state failure.\n"
    );
    // An unmatched event still advances the checkpoint and emits labelled sampled health.
    source["storage"]["path"] = serde_json::json!(root.join("healthy-state/state.sqlite3"));
    source["routes"] = serde_json::json!([{"events":["JOB_FAILED"],"send_to":["research_email"]}]);
    std::fs::write(&config, serde_json::to_vec(&source).unwrap()).unwrap();
    let mut healthy = Reap(
        command()
            .args(["start", "--foreground", "--config"])
            .arg(&config)
            .env("PATH", &root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = healthy.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = Vec::new();
        BufReader::new(stdout.take(1024))
            .read_until(b'\n', &mut line)
            .unwrap();
        let _ = send.send(line);
    });
    assert_eq!(receive.recv_timeout(Duration::from_secs(5)).unwrap(),b"Foreground health: stale=false pending_sampled=0 failed_sampled=0 sample_truncated=false\n");
    // SAFETY: PID belongs to the live owned fixture process.
    assert_eq!(
        unsafe { libc::kill(healthy.id().try_into().unwrap(), libc::SIGTERM) },
        0
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = healthy.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "healthy foreground shutdown timed out"
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(status.code(), Some(143));
    reader.join().unwrap();
    let mut stderr = Vec::new();
    healthy
        .stderr
        .take()
        .unwrap()
        .take(4096)
        .read_to_end(&mut stderr)
        .unwrap();
    assert_eq!(
        stderr,
        b"Foreground monitoring started.\nForeground monitoring stopped by SIGTERM.\n"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn installed_foreground_webhook_only_persists_and_reopens_without_smtp() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
        process::Stdio,
        sync::mpsc,
        time::{Duration, Instant},
    };
    struct Reap(std::process::Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root =
        std::env::temp_dir().join(format!("slurm-notify-start-webhook-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("config.json");
    let state = root.join("state/state.sqlite3");
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../docs/proposals/examples/local-webhook-config.json"
    ))
    .unwrap();
    source["storage"]["path"] = serde_json::json!(state);
    source["monitor"]["initial_observation"] = serde_json::json!("emit");
    let write = |path: &std::path::Path, bytes: &[u8], mode| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    };
    write(&path, &serde_json::to_vec(&source).unwrap(), 0o600);
    for (name,body) in [("squeue","#!/bin/sh\nexit 0\n"),
        ("sacct","#!/bin/sh\nfor arg in \"$@\"; do case \"$arg\" in --user=*) user=${arg#--user=};; esac; done\nprintf 'research-cluster|1|%s|s|a|COMPLETED|0:0\\n' \"$user\"\n")] {
        write(&root.join(name), body.as_bytes(), 0o700);
    }
    expect(
        command()
            .args(["start", "--foreground", "--config"])
            .arg(&path)
            .env("PATH", &root)
            .output()
            .unwrap(),
        2,
        b"",
        b"Configuration invalid or required resource unavailable.\n",
    );
    assert!(!state.exists());
    // An owned, closed loopback port gives a controlled connection failure.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "https://127.0.0.1:{}/hook",
        listener.local_addr().unwrap().port()
    );
    drop(listener);
    let mut previous = None;
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let mut child = Reap(
            command()
                .args(["start", "--foreground", "--config"])
                .arg(&path)
                .env("PATH", &root)
                .env("SLURM_NOTIFY_WEBHOOK_URL", &url)
                .env("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION", "Bearer fixture")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stderr = child.0.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().take(10) {
                if tx.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "Foreground monitoring started."
        );
        // Health follows the first poll and failed attempt; no SMTP credentials exist.
        let stdout = child.0.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let output = std::thread::spawn(move || {
            let mut line = String::new();
            BufReader::new(stdout).read_line(&mut line).unwrap();
            let _ = tx.send(line);
        });
        assert!(rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .starts_with("Foreground health:"));
        // SAFETY: this signal targets the owned child process.
        assert_eq!(
            unsafe { libc::kill(child.0.id().try_into().unwrap(), signal) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert_eq!(
                    status.code(),
                    Some(if signal == libc::SIGINT { 130 } else { 143 })
                );
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        reader.join().unwrap();
        output.join().unwrap();
        let db = rusqlite::Connection::open_with_flags(
            &state,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let frozen: (String, Vec<u8>) = db
            .query_row(
                "SELECT internal_event_id, payload FROM public_event_frames",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        if let Some(old) = &previous {
            assert_eq!(&frozen, old);
        }
        previous = Some(frozen);
        assert!(
            db.query_row(
                "SELECT COUNT(*) FROM delivery_attempts WHERE outcome='transient_failure'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap()
                >= 1
        );

        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM smtp_messages", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/smtp_relay.rs"]
mod smtp_relay;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn installed_foreground_delivers_through_selected_helper_and_does_not_replay_after_restart() {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
        process::Stdio,
        sync::mpsc,
        time::{Duration, Instant},
    };
    struct Reap(std::process::Child);
    impl std::ops::Deref for Reap {
        type Target = std::process::Child;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl std::ops::DerefMut for Reap {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root = std::env::temp_dir().join(format!("slurm-notify-start-send-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = root.join("config.json");
    let trust = root.join("trust.der");
    let state = root.join("state/state.sqlite3");
    let (cert, port, server) = smtp_relay::start();
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&trust)
        .unwrap()
        .write_all(&cert)
        .unwrap();
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../docs/proposals/examples/local-mixed-config.json"
    ))
    .unwrap();
    source["providers"]["research_email"]["trust_der_files"] = serde_json::json!([trust]);
    source["providers"]["research_email"]["relay"] = serde_json::json!("127.0.0.1");
    source["providers"]["research_email"]["port"] = serde_json::json!(port);
    source["providers"]["research_email"]["server_name"] =
        serde_json::json!("smtp.fixture.invalid");
    source["providers"]["research_email"]["authentication"]["identity"] =
        serde_json::json!("fixture-user");
    source["providers"]["research_email"]["sender"] = serde_json::json!("s@fixture.invalid");
    source["providers"]["research_email"]["recipient"] = serde_json::json!("r@fixture.invalid");
    source["routes"] =
        serde_json::json!([{"events":["JOB_COMPLETED"],"send_to":["research_email"]}]);
    source["storage"]["path"] = serde_json::json!(state);
    source["monitor"]["initial_observation"] = serde_json::json!("emit");
    let trust_hash = smtp_provider_revision::trust_fingerprint(&[&cert]).unwrap();
    let trust_ref = config_reference::reference(b"trust\0", &[trust.to_str().unwrap()]);
    let credentials_ref = config_reference::reference(
        b"credentials\0",
        &["SLURM_NOTIFY_SMTP_USERNAME", "SLURM_NOTIFY_SMTP_PASSWORD"],
    );
    let descriptor = smtp_provider_revision::Descriptor::new(&smtp_provider_revision::Profile {
        destination: "research_email",
        relay: "127.0.0.1",
        port,
        tls: smtp_provider_revision::TlsMode::RequiredStartTls,
        server_name: "smtp.fixture.invalid",
        trust_reference: &trust_ref,
        trust_fingerprint: &trust_hash,
        authentication_identity: "fixture-user",
        credential_reference: &credentials_ref,
    })
    .unwrap();
    let rendering = rendering_fingerprint::encode(&[
        ("destination", "research_email"),
        ("sender", "s@fixture.invalid"),
        ("recipient", "r@fixture.invalid"),
        ("namespace", "research_email"),
        ("disclosure", "hidden"),
        ("renderer", "allowlist-v1"),
        ("identity", "sha256-event-route-revision-v2"),
        ("domain", "fixture.invalid"),
    ])
    .unwrap();
    let digest = ring::digest::digest(&ring::digest::SHA256, &rendering);
    let revision: String = digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    source["delivery"]["smtp_permissions"][0]["rendering_revision"] = serde_json::json!(revision);
    source["delivery"]["smtp_permissions"][0]["provider_revision"] =
        serde_json::json!(descriptor.token());
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&config)
        .unwrap()
        .write_all(&serde_json::to_vec(&source).unwrap())
        .unwrap();
    for (name,body) in [("squeue","#!/bin/sh\nexit 0\n"),("sacct","#!/bin/sh\nfor arg in \"$@\"; do case \"$arg\" in --user=*) user=${arg#--user=};; esac; done\nprintf 'research-cluster|1|%s|s|a|COMPLETED|0:0\\n' \"$user\"\n")] {
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o700).open(root.join(name)).unwrap().write_all(body.as_bytes()).unwrap();
    }
    let mut frozen = None;
    for attempt in 0..2 {
        let mut child = Reap(
            command()
                .args(["start", "--foreground", "--config"])
                .arg(&config)
                .env("PATH", &root)
                .env(
                    "SLURM_NOTIFY_WEBHOOK_URL",
                    "https://fixture.invalid/private-hook-token",
                )
                .env("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION", "Bearer fixture")
                .env("SLURM_NOTIFY_SMTP_USERNAME", "fixture-user")
                .env("SLURM_NOTIFY_SMTP_PASSWORD", "fixture-password")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut line = Vec::new();
            BufReader::new(stdout.take(1024))
                .read_until(b'\n', &mut line)
                .unwrap();
            let _ = send.send(line);
        });
        assert_eq!(receive.recv_timeout(Duration::from_secs(10)).unwrap(),b"Foreground health: stale=false pending_sampled=0 failed_sampled=0 sample_truncated=false\n");
        // SAFETY: PID belongs to this test's running fixture child.
        assert_eq!(
            unsafe { libc::kill(child.id().try_into().unwrap(), libc::SIGTERM) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "foreground send shutdown timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(status.code(), Some(143));
        reader.join().unwrap();
        let mut stderr = Vec::new();
        child
            .stderr
            .take()
            .unwrap()
            .take(4096)
            .read_to_end(&mut stderr)
            .unwrap();
        assert_eq!(
            stderr,
            b"Foreground monitoring started.\nForeground monitoring stopped by SIGTERM.\n"
        );
        let db = rusqlite::Connection::open_with_flags(
            &state,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let result:(Vec<u8>,String,i64)=db.query_row("SELECT m.data,o.status,(SELECT COUNT(*) FROM delivery_attempts) FROM smtp_messages m JOIN outbox o ON m.event_id=o.event_id AND m.destination=o.destination",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap();
        assert_eq!(result.1, "delivered");
        assert_eq!(result.2, 1);
        if attempt == 0 {
            frozen = Some(result.0);
        } else {
            assert_eq!(Some(result.0), frozen);
        }
    }
    assert_eq!(Some(server.join().unwrap()), frozen);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/https_relay.rs"]
mod https_relay;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn installed_https_delivery_retries_frozen_public_frame_after_restart() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
        process::Stdio,
        sync::mpsc,
        time::{Duration, Instant},
    };
    struct Reap(std::process::Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root = std::env::temp_dir().join(format!("slurm-notify-https-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (der, port, server) = https_relay::start();
    let write = |path: &std::path::Path, bytes: &[u8], mode| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    };
    let trust = root.join("trust.der");
    write(&trust, &der, 0o600);
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../docs/proposals/examples/local-webhook-config.json"
    ))
    .unwrap();
    let state = root.join("state/state.sqlite3");
    let config = root.join("config.json");
    source["storage"]["path"] = serde_json::json!(state);
    source["providers"]["primary"]["trust_der_files"] = serde_json::json!([trust]);
    source["monitor"]["initial_observation"] = serde_json::json!("emit");
    source["delivery"]["initial_backoff_seconds"] = serde_json::json!(1);
    source["delivery"]["jitter_seconds"] = serde_json::json!(0);
    write(&config, &serde_json::to_vec(&source).unwrap(), 0o600);
    for (name,body) in [("squeue","#!/bin/sh\nexit 0\n"),("sacct","#!/bin/sh\nfor arg in \"$@\"; do case \"$arg\" in --user=*) user=${arg#--user=};; esac; done\nprintf 'research-cluster|1|%s|s|a|COMPLETED|0:0\\n' \"$user\"\n")] {write(&root.join(name),body.as_bytes(),0o700);}
    expect(command().args(["config","validate","--config"]).arg(&config).env("SLURM_NOTIFY_WEBHOOK_URL",format!("https://127.0.0.1:{port}/hook")).env("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION","Bearer fixture").output().unwrap(),0,b"Configuration valid: schema_version=1 cluster=research-cluster destinations=1 smtp_permissions=not_applicable\n",b"");
    let mut frozen = None;
    for run in 0..3 {
        let mut child = Reap(
            command()
                .args(["start", "--foreground", "--config"])
                .arg(&config)
                .env("PATH", &root)
                .env(
                    "SLURM_NOTIFY_WEBHOOK_URL",
                    format!("https://127.0.0.1:{port}/hook"),
                )
                .env("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION", "Bearer fixture")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = child.0.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut line = String::new();
            BufReader::new(stdout).read_line(&mut line).unwrap();
            let _ = tx.send(line);
        });
        assert!(rx
            .recv_timeout(Duration::from_secs(6))
            .unwrap()
            .starts_with("Foreground health:"));
        // SAFETY: PID belongs to the owned child.
        assert_eq!(
            unsafe { libc::kill(child.0.id().try_into().unwrap(), libc::SIGTERM) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert_eq!(status.code(), Some(143));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        reader.join().unwrap();
        let db = rusqlite::Connection::open_with_flags(
            &state,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let frame: (String, Vec<u8>) = db
            .query_row(
                "SELECT public_id,payload FROM public_event_frames",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        if let Some(previous) = &frozen {
            assert_eq!(&frame, previous);
        }
        frozen = Some(frame);
        let counts: (String, i64) = db
            .query_row(
                "SELECT status,(SELECT COUNT(*) FROM delivery_attempts) FROM outbox",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            counts,
            if run == 0 {
                ("pending".into(), 1)
            } else {
                ("delivered".into(), 2)
            }
        );
        if run == 0 {
            std::thread::sleep(Duration::from_millis(1100));
        }
    }
    let received = server.join().unwrap();
    let (id, body) = frozen.unwrap();
    assert_eq!(received.len(), 2);
    for (headers, payload) in received {
        assert_eq!(payload, body);
        assert!(headers
            .to_ascii_lowercase()
            .contains(&format!("idempotency-key: {id}\r\n")));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn doctor_checks_resources_and_bounded_probes_without_state_or_provider_activity() {
    use std::{
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };
    for args in [["doctor", "--help"], ["--help", "doctor"]] {
        expect(
            command().args(args).output().unwrap(),
            0,
            include_bytes!("../../../contracts/cli/doctor-help.stdout"),
            b"",
        );
    }
    for args in [
        vec!["doctor"],
        vec!["doctor", "--config", "relative"],
        vec!["doctor", "doctor", "--config", "/absent"],
        vec!["doctor", "--config", "/absent", "--unknown"],
    ] {
        expect(command().args(args).output().unwrap(), 2, b"", INVALID);
    }
    let root = std::env::temp_dir().join(format!("slurm-notify-doctor-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = root.join("config.json");
    let state = root.join("state/state.sqlite3");
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../docs/proposals/examples/local-webhook-config.json"
    ))
    .unwrap();
    source["storage"]["path"] = serde_json::json!(state);
    source["monitor"]["query_timeout_seconds"] = serde_json::json!(3);
    source["monitor"]["command_output_limit_bytes"] = serde_json::json!(1024);
    let write = |path: &std::path::Path, bytes: &[u8], mode| {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    };
    write(&config, &serde_json::to_vec(&source).unwrap(), 0o600);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let run = || {
        command()
            .args(["--config"])
            .arg(&config)
            .arg("doctor")
            .env("PATH", &root)
            .env(
                "SLURM_NOTIFY_WEBHOOK_URL",
                format!(
                    "https://127.0.0.1:{}/hook",
                    listener.local_addr().unwrap().port()
                ),
            )
            .env("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION", "Bearer fixture")
            .output()
            .unwrap()
    };
    expect(
        run(),
        2,
        b"",
        b"Required Slurm executables unavailable in absolute PATH directories.\n",
    );
    for name in ["squeue", "sacct"] {
        write(&root.join(name),b"#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = --version ] || exit 7\nprintf 'slurm synthetic\\n'\n",0o700);
    }
    expect(
        run(),
        0,
        include_bytes!("../../../contracts/cli/doctor-success.stdout"),
        b"",
    );
    source["monitor"]["query_timeout_seconds"] = serde_json::json!(1);
    write(&config, &serde_json::to_vec(&source).unwrap(), 0o600);
    for body in [
        "#!/bin/sh\nexit 7\n",
        "#!/bin/sh\nexec /bin/sleep 3\n",
        "#!/bin/sh\ni=0; while [ \"$i\" -lt 2048 ]; do printf x; i=$((i+1)); done\n",
        "#!/bin/sh\nexit 0\n",
    ] {
        write(&root.join("sacct"), body.as_bytes(), 0o700);
        expect(
            run(),
            1,
            b"",
            b"Slurm capability probe failed or exceeded its budget.\n",
        );
    }
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).unwrap();
    expect(
        run(),
        2,
        b"",
        b"Configuration invalid or required resource unavailable.\n",
    );
    assert!(!state.exists());
    assert!(matches!(listener.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn explicit_provider_tests_send_once_without_scheduler_or_state() {
    use std::{
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };
    for args in [["test", "--help"], ["--help", "test"]] {
        expect(
            command().args(args).output().unwrap(),
            0,
            include_bytes!("../../../contracts/cli/test-help.stdout"),
            b"",
        );
    }
    for args in [
        vec!["test"],
        vec!["test", "primary", "--config", "relative"],
        vec!["test", "bad!", "--config", "/absent"],
        vec!["test", "primary", "extra", "--config", "/absent"],
    ] {
        expect(command().args(args).output().unwrap(), 2, b"", INVALID);
    }
    let root =
        std::env::temp_dir().join(format!("slurm-notify-provider-test-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = root.join("config.json");
    let trust = root.join("trust.der");
    let state = root.join("state/state.sqlite3");
    let write = |path: &std::path::Path, bytes: &[u8]| {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    };
    for status in ["503 Service Unavailable", "204 No Content"] {
        let (der, port, server) = https_relay::start_statuses(vec![status]);
        write(&trust, &der);
        let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../docs/proposals/examples/local-webhook-config.json"
        ))
        .unwrap();
        source["storage"]["path"] = serde_json::json!(state);
        source["providers"]["primary"]["trust_der_files"] = serde_json::json!([trust]);
        write(&config, &serde_json::to_vec(&source).unwrap());
        let run = |alias: &str| {
            command()
                .args(["test", alias, "--config"])
                .arg(&config)
                .env(
                    "SLURM_NOTIFY_WEBHOOK_URL",
                    format!("https://127.0.0.1:{port}/hook"),
                )
                .env("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION", "Bearer fixture")
                .output()
                .unwrap()
        };
        expect(
            run("missing"),
            2,
            b"",
            b"Configuration invalid or required resource unavailable.\n",
        );
        let result = run("primary");
        assert_eq!(
            result.status.code(),
            Some(if status.starts_with("204") { 0 } else { 1 })
        );
        let text = String::from_utf8(result.stdout).unwrap();
        let preview = text
            .lines()
            .next()
            .unwrap()
            .strip_prefix("Synthetic test: provider=primary type=webhook payload=")
            .unwrap();
        let preview: serde_json::Value = serde_json::from_str(preview).unwrap();
        assert_eq!(preview["event_type"], "TEST");
        assert_eq!(preview.as_object().unwrap().len(), 4);
        assert_eq!(preview["schema_version"], "1.0");
        assert_eq!(
            preview["message"],
            "Synthetic Slurm Notify test; no scheduler data."
        );
        assert_eq!(preview["event_id"].as_str().unwrap().len(), 36);
        assert!(preview.get("job").is_none());
        if status.starts_with("204") {
            assert!(text.ends_with(
                std::str::from_utf8(include_bytes!("../../../contracts/cli/test-success.stdout"))
                    .unwrap()
            ));
            assert!(result.stderr.is_empty());
        } else {
            assert_eq!(result.stderr,b"Synthetic test was not acknowledged; acceptance may be unknown. No automatic retry was made.\n");
        }
        let received = server.join().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&received[0].1).unwrap(),
            preview
        );
        assert!(!state.exists());
    }
    let (der, port, server) = smtp_relay::start();
    write(&trust, &der);
    let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../docs/proposals/examples/local-mixed-config.json"
    ))
    .unwrap();
    source["providers"]
        .as_object_mut()
        .unwrap()
        .remove("primary");
    source["routes"] = serde_json::json!([]);
    source["storage"]["path"] = serde_json::json!(state);
    let provider = &mut source["providers"]["research_email"];
    provider["trust_der_files"] = serde_json::json!([trust]);
    provider["relay"] = serde_json::json!("127.0.0.1");
    provider["port"] = serde_json::json!(port);
    provider["server_name"] = serde_json::json!("smtp.fixture.invalid");
    provider["authentication"]["identity"] = serde_json::json!("fixture-user");
    provider["sender"] = serde_json::json!("s@fixture.invalid");
    provider["recipient"] = serde_json::json!("r@fixture.invalid");
    let trust_ref = config_reference::reference(b"trust\0", &[trust.to_str().unwrap()]);
    let creds_ref = config_reference::reference(
        b"credentials\0",
        &["SLURM_NOTIFY_SMTP_USERNAME", "SLURM_NOTIFY_SMTP_PASSWORD"],
    );
    let hash = smtp_provider_revision::trust_fingerprint(&[&der]).unwrap();
    let descriptor = smtp_provider_revision::Descriptor::new(&smtp_provider_revision::Profile {
        destination: "research_email",
        relay: "127.0.0.1",
        port,
        tls: smtp_provider_revision::TlsMode::RequiredStartTls,
        server_name: "smtp.fixture.invalid",
        trust_reference: &trust_ref,
        trust_fingerprint: &hash,
        authentication_identity: "fixture-user",
        credential_reference: &creds_ref,
    })
    .unwrap();
    let rendering = rendering_fingerprint::encode(&[
        ("destination", "research_email"),
        ("sender", "s@fixture.invalid"),
        ("recipient", "r@fixture.invalid"),
        ("namespace", "research_email"),
        ("disclosure", "hidden"),
        ("renderer", "allowlist-v1"),
        ("identity", "sha256-event-route-revision-v2"),
        ("domain", "fixture.invalid"),
    ])
    .unwrap();
    let rendering: String = ring::digest::digest(&ring::digest::SHA256, &rendering)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    source["delivery"]["smtp_permissions"] = serde_json::json!([]);
    write(&config, &serde_json::to_vec(&source).unwrap());
    expect(
        command()
            .args(["test", "research_email", "--config"])
            .arg(&config)
            .output()
            .unwrap(),
        3,
        b"",
        b"SMTP delivery permission is not approved for the current profile.\n",
    );
    source["delivery"]["smtp_permissions"] = serde_json::json!([{"cluster_id":"research-cluster","destination":"research_email","rendering_revision":rendering,"provider_revision":descriptor.token()}]);
    write(&config, &serde_json::to_vec(&source).unwrap());
    expect(
        command()
            .args(["test", "research_email", "--config"])
            .arg(&config)
            .output()
            .unwrap(),
        2,
        b"",
        b"Configuration invalid or required resource unavailable.\n",
    );
    let output = command()
        .args(["--config"])
        .arg(&config)
        .args(["test", "research_email"])
        .env("SLURM_NOTIFY_SMTP_USERNAME", "fixture-user")
        .env("SLURM_NOTIFY_SMTP_PASSWORD", "fixture-password")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Synthetic Slurm Notify test."));
    assert!(!text.contains("fixture-password"));
    let body = server.join().unwrap();
    assert!(body
        .windows(b"Synthetic Slurm Notify test.".len())
        .any(|bytes| bytes == b"Synthetic Slurm Notify test."));
    assert!(!state.exists());
    std::fs::remove_dir_all(root).unwrap();
}

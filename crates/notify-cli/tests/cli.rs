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

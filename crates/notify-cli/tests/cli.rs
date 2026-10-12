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
        include_bytes!("../../../contracts/cli/config-smtp-unavailable.stderr"),
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
        vec!["status", "--config", "/private"],
        vec!["config", "validate", "--unknown", "/private"],
    ] {
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        expect(run(&args), 2, b"", INVALID);
    }
}

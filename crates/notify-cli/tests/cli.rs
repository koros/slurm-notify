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

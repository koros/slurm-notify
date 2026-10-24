//! Local prototype CLI with finite commands and foreground SMTP/mixed monitoring.
mod config_reference;
mod control_validation;
mod credential_policy;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod foreground_report;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod foreground_runtime;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod foreground_schedule;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod foreground_session;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod foreground_signals;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod foreground_wait;
mod installed_config;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod installed_monitor;
mod installed_smtp;
mod installed_status;
mod installed_webhook_helper;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod mixed_config_composition;
mod provider_frame;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod provider_helpers;
mod provider_outcome;
mod public_event;
mod rendering_fingerprint;
mod routing_validation;
mod smtp_address;
mod state_timestamp;
#[cfg(test)]
mod status_composition;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod status_store;
mod storage_health;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod config_composition;

mod config_validate;

mod config_decode;

mod config_snapshot;

mod trust_snapshot;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod environment_resolution;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod credential_resolution;

#[cfg(test)]
mod provider_resolution;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod webhook_resolution;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod runtime_mapping;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod effective_user;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod observation_mapping;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod route_selection;

#[cfg(test)]
mod smtp_frozen_frame;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod delivery;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod lifecycle;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod scheduler;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod smtp_poll;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod storage;

mod webhook;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod transport;

mod state_path;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod accounting_window;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod polling_cycle;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod supervised_provider;

#[cfg(test)]
mod smtp_policy;

mod smtp_reply;

mod smtp_tls;

#[cfg(test)]
mod smtp_ack;

#[cfg(test)]
mod smtp_dispatch;

mod smtp_auth;

mod smtp_auth_frame;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod smtp_auth_dispatch;

mod smtp_message;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod smtp_render;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod smtp_routes;

mod smtp_bound_transport;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod smtp_provider_delivery;
mod smtp_provider_revision;
mod smtp_resolved;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod smtp_revision;
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod smtp_revision_delivery;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod smtp_metadata;

mod smtp_submit;

mod smtp_submit_frame;

#[cfg(any(test, target_os = "linux", target_os = "macos"))]
mod smtp_submit_dispatch;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod installed_doctor;
const HELP: &str = include_str!("../../../contracts/cli/help.stdout");
const CONFIG_HELP: &str = include_str!("../../../contracts/cli/config-help.stdout");

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).take(8).collect();
    if args.len() == 2 && args[0] == "--internal-provider-v1" && args[1] == "webhook" {
        return std::process::ExitCode::from(
            if installed_webhook_helper::serve(std::io::stdin(), std::io::stdout()).is_ok() {
                0
            } else {
                2
            },
        );
    }

    if args.len() == 2 && args[0] == "--internal-provider-v1" && args[1] == "smtp-starttls" {
        return std::process::ExitCode::from(
            if smtp_bound_transport::serve(std::io::stdin(), std::io::stdout()).is_ok() {
                0
            } else {
                2
            },
        );
    }
    if args.len() == 2
        && args.iter().any(|arg| arg == "start")
        && args.iter().any(|arg| arg == "--help")
    {
        print!(
            "{}",
            include_str!("../../../contracts/cli/start-help.stdout")
        );
        return std::process::ExitCode::SUCCESS;
    }
    if let Some(path) = start_path(&args) {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        return std::process::ExitCode::from(installed_monitor::run(&path));
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            eprintln!("Foreground monitoring is unavailable on this platform.");
            return std::process::ExitCode::from(2);
        }
    }
    if args.len() == 2
        && args.iter().any(|arg| arg == "doctor")
        && args.iter().any(|arg| arg == "--help")
    {
        print!(
            "{}",
            include_str!("../../../contracts/cli/doctor-help.stdout")
        );
        return std::process::ExitCode::SUCCESS;
    }
    if let Some(path) = finite_path(&args, "doctor") {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        return match installed_doctor::run(&path) {
            Ok(output) => {
                print!("{output}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                let (code, message) = match error {
                    installed_doctor::Error::Config => {
                        (2, "Configuration invalid or required resource unavailable.")
                    }
                    installed_doctor::Error::Permission => (
                        3,
                        "SMTP delivery permission is not approved for the current profile.",
                    ),
                    installed_doctor::Error::Programs => (
                        2,
                        "Required Slurm executables unavailable in absolute PATH directories.",
                    ),
                    installed_doctor::Error::Probe => {
                        (1, "Slurm capability probe failed or exceeded its budget.")
                    }
                };
                eprintln!("{message}");
                std::process::ExitCode::from(code)
            }
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            eprintln!("Doctor is unavailable on this platform.");
            return std::process::ExitCode::from(2);
        }
    }
    if args.len() == 1 && args[0] == "--help" {
        print!("{HELP}");
        return std::process::ExitCode::SUCCESS;
    }
    if args.len() == 1 && args[0] == "--version" {
        println!("slurm-notify {}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }
    if args.is_empty() {
        eprintln!("Usage: slurm-notify --help | --version");
        return std::process::ExitCode::from(2);
    }
    if let Some(path) = status_path(&args) {
        return match crate::installed_status::inspect(&path) {
            Ok(output) => {
                print!("{output}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                let (code, message) = match error {
                    crate::installed_status::Error::Config => (2, "Configuration invalid or required resource unavailable."),
                    crate::installed_status::Error::Missing => (1, "Existing state is missing or incomplete; no state was created."),
                    crate::installed_status::Error::Busy => (1, "State is busy; stop the owner before inspecting historical state."),
                    crate::installed_status::Error::State => (1, "State is unsafe, unreadable, or requires recovery; no repair was attempted."),
                    crate::installed_status::Error::Version => (1, "State schema is unsupported; no migration was attempted."),
                    crate::installed_status::Error::Database => (1, "State inspection failed; no repair was attempted."),
                };
                eprintln!("{message}");
                std::process::ExitCode::from(code)
            }
        };
    }
    if args.len() == 2
        && args.iter().any(|arg| arg == "--help")
        && args.iter().any(|arg| arg == "status")
    {
        print!(
            "{}",
            include_str!("../../../contracts/cli/status-help.stdout")
        );
        return std::process::ExitCode::SUCCESS;
    }
    if let Some(path) = config_path(&args) {
        return match crate::installed_config::validate(&path) {
            Ok(output) => {
                print!("{output}");
                std::process::ExitCode::SUCCESS
            }
            Err(crate::installed_config::Error::Invalid) => {
                eprintln!("Configuration invalid or required resource unavailable.");
                std::process::ExitCode::from(2)
            }
            Err(crate::installed_config::Error::Permission) => {
                eprintln!("SMTP delivery permission is not approved for the current profile.");
                std::process::ExitCode::from(3)
            }
        };
    }
    if args.len() == 3 && args.iter().filter(|v| *v == "--help").count() == 1 {
        let words: Vec<_> = args.iter().filter(|v| *v != "--help").collect();
        if words[0] == "config" && words[1] == "validate" {
            print!("{CONFIG_HELP}");
            return std::process::ExitCode::SUCCESS;
        }
    }
    eprintln!("Invalid invocation. Use --help for usage.");
    std::process::ExitCode::from(2)
}

fn start_path(args: &[std::ffi::OsString]) -> Option<std::path::PathBuf> {
    if args.len() != 4 {
        return None;
    }
    let mut start = false;
    let mut foreground = false;
    let mut path = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "start" && !start {
            start = true;
        } else if arg == "--foreground" && !foreground {
            foreground = true;
        } else if arg == "--config" && path.is_none() {
            path = Some(std::path::PathBuf::from(iter.next()?));
        } else {
            return None;
        }
    }
    path.filter(|path| start && foreground && path.is_absolute() && path.to_str().is_some())
}

fn config_path(args: &[std::ffi::OsString]) -> Option<std::path::PathBuf> {
    if args.len() != 4 {
        return None;
    }
    let mut words = Vec::new();
    let mut path = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--config" {
            if path.is_some() {
                return None;
            }
            let value = args.next()?;
            value.to_str()?;
            let candidate = std::path::PathBuf::from(value);
            if !candidate.is_absolute() {
                return None;
            }
            path = Some(candidate);
        } else {
            words.push(arg);
        }
    }
    if words.len() == 2 && words[0] == "config" && words[1] == "validate" {
        path
    } else {
        None
    }
}

fn status_path(args: &[std::ffi::OsString]) -> Option<std::path::PathBuf> {
    finite_path(args, "status")
}

fn finite_path(args: &[std::ffi::OsString], name: &str) -> Option<std::path::PathBuf> {
    if args.len() != 3 {
        return None;
    }
    let mut path = None;
    let mut command = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--config" {
            if path.is_some() {
                return None;
            }
            let value = args.next()?;
            value.to_str()?;
            let candidate = std::path::PathBuf::from(value);
            if !candidate.is_absolute() {
                return None;
            }
            path = Some(candidate);
        } else if command.replace(arg).is_some() {
            return None;
        }
    }
    if command.is_some_and(|arg| arg == name) {
        path
    } else {
        None
    }
}

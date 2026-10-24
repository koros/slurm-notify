//! Private same-executable provider selection; no PATH or environment override.
use std::{ffi::OsString, path::PathBuf, process::Command};
const FLAG: &str = "--internal-provider-v1";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Webhook,
    Smtp,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Error;
// No configurable helper pathname. Capture the running executable once before
// provider dispatch; stdin carries all per-request resources and private bytes.
pub(super) struct Selection {
    executable: PathBuf,
}
impl Selection {
    pub(super) fn current() -> Result<Self, Error> {
        Self::from_path(std::env::current_exe().map_err(|_| Error)?)
    }
    fn from_path(executable: PathBuf) -> Result<Self, Error> {
        if !executable.is_absolute() || !executable.is_file() {
            return Err(Error);
        }
        Ok(Self { executable })
    }
    pub(super) fn command(&self, kind: Kind) -> Command {
        let mut command = Command::new(&self.executable);
        command.args([
            FLAG,
            match kind {
                Kind::Webhook => "webhook",
                Kind::Smtp => "smtp-starttls",
            },
        ]);
        command
    }
}
pub(super) fn mode(args: &[OsString]) -> Option<Kind> {
    if args.len() != 2 || args[0] != FLAG {
        return None;
    }
    if args[1] == "webhook" {
        Some(Kind::Webhook)
    } else if args[1] == "smtp-starttls" {
        Some(Kind::Smtp)
    } else {
        None
    }
}
#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom acceptance harness omits unit-test bodies.
    use super::*;
    #[test]
    fn selection_uses_only_current_absolute_executable_and_fixed_arguments() {
        let selection = Selection::current().unwrap();
        for kind in [Kind::Webhook, Kind::Smtp] {
            let command = selection.command(kind);
            assert_eq!(command.get_program(), std::env::current_exe().unwrap());
            let args: Vec<_> = command.get_args().map(|arg| arg.to_owned()).collect();
            assert_eq!(mode(&args), Some(kind));
            assert!(command.get_envs().next().is_none());
        }
        assert!(Selection::from_path(PathBuf::from("relative-helper")).is_err());
        assert!(Selection::from_path(std::env::temp_dir()).is_err());
        assert!(Selection::from_path(std::env::temp_dir().join("absent-private-helper")).is_err());
        for args in [
            vec![],
            vec![FLAG],
            vec![FLAG, "webhook", "extra"],
            vec!["--other", "webhook"],
            vec![FLAG, "smtp"],
            vec![FLAG, "WEBHOOK"],
        ] {
            let args: Vec<_> = args.into_iter().map(OsString::from).collect();
            assert!(mode(&args).is_none());
        }
    }
}

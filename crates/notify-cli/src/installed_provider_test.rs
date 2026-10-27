//! Explicit one-shot synthetic delivery, without scheduler or durable state.
use crate::{
    provider_helpers::{Kind, Selection},
    provider_outcome::ProviderOutcome,
};
use std::{io::Write, path::Path};
pub(super) enum Error {
    Config,
    Permission,
    ApiPermission,
    Transport,
}
pub(super) fn run(path: &Path, alias: &str) -> Result<(), Error> {
    let plan = crate::mixed_config_composition::prepare_path(
        path,
        crate::effective_user::lookup,
        crate::environment_resolution::lookup,
    )
    .map_err(|_| Error::Config)?;
    let helpers = Selection::current().map_err(|_| Error::Config)?;
    let id =
        crate::public_event::identifier(crate::public_event::random().map_err(|_| Error::Config)?);
    let now = crate::provider_outcome::unix_time()
        .and_then(|time| u64::try_from(time).ok())
        .ok_or(Error::Config)?;
    let request = plan
        .synthetic_test(alias, &id, now)
        .map_err(|error| match error {
            crate::mixed_config_composition::SyntheticError::Config => Error::Config,
            crate::mixed_config_composition::SyntheticError::Permission => Error::Permission,
            crate::mixed_config_composition::SyntheticError::ApiPermission => Error::ApiPermission,
        })?;
    use crate::mixed_config_composition::{SyntheticRequest as Request, SMTP_TEST_BODY};
    let report = match request {
        Request::Email { request, kind, id } => {
            println!("Synthetic test: provider={alias} type={kind} subject=Slurm Notify test");
            println!("Synthetic Slurm Notify test; no scheduler data.\nEvent: {id}");
            std::io::stdout().flush().map_err(|_| Error::Transport)?;
            request.supervise(&mut helpers.command(Kind::Webhook))
        }
        Request::Webhook {
            request,
            preview,
            kind,
        } => {
            println!("Synthetic test: provider={alias} type={kind} payload={preview}");
            std::io::stdout().flush().map_err(|_| Error::Transport)?;
            request.supervise(&mut helpers.command(Kind::Webhook))
        }
        Request::Smtp { frame, timeout } => {
            println!("Synthetic test: provider={alias} type=smtp subject=Slurm Notify test");
            print!("{SMTP_TEST_BODY}");
            std::io::stdout().flush().map_err(|_| Error::Transport)?;
            crate::smtp_submit_dispatch::classify(crate::transport::supervise_with(
                &mut helpers.command(Kind::Smtp),
                frame,
                timeout,
                crate::smtp_bound_transport::INPUT_LIMIT,
                128,
                crate::smtp_submit_frame::decode_ack,
            ))
        }
    };
    if report.outcome != Some(ProviderOutcome::Delivered) {
        return Err(Error::Transport);
    }
    print!(
        "{}",
        include_str!("../../../contracts/cli/test-success.stdout")
    );
    Ok(())
}

//! Repository foundation executable; monitoring is not implemented.
#[cfg(test)]
mod foreground_report;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod foreground_runtime;
#[cfg(test)]
mod foreground_schedule;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod foreground_session;
#[cfg(test)]
mod foreground_wait;
#[cfg(test)]
mod mixed_config_composition;
#[cfg(test)]
mod status_composition;

#[cfg(test)]
mod config_composition;

#[cfg(test)]
mod config_validate;

#[cfg(test)]
mod config_decode;

#[cfg(test)]
mod config_snapshot;

#[cfg(test)]
mod trust_snapshot;

#[cfg(test)]
mod credential_resolution;

#[cfg(test)]
mod provider_resolution;

#[cfg(test)]
mod webhook_resolution;

#[cfg(test)]
mod runtime_mapping;

#[cfg(test)]
mod observation_mapping;

#[cfg(test)]
mod route_selection;

#[cfg(test)]
mod smtp_frozen_frame;

#[cfg(test)]
mod delivery;
#[cfg(test)]
mod lifecycle;
#[cfg(test)]
mod scheduler;
#[cfg(test)]
mod smtp_poll;
#[cfg(test)]
mod storage;

#[cfg(test)]
mod webhook;

#[cfg(test)]
mod transport;

#[cfg(test)]
mod state_path;

#[cfg(test)]
mod accounting_window;

#[cfg(test)]
mod polling_cycle;

#[cfg(test)]
mod supervised_provider;

#[cfg(test)]
mod smtp_policy;

#[cfg(test)]
mod smtp_reply;

#[cfg(test)]
mod smtp_tls;

#[cfg(test)]
mod smtp_ack;

#[cfg(test)]
mod smtp_dispatch;

#[cfg(test)]
mod smtp_auth;

#[cfg(test)]
mod smtp_auth_frame;

#[cfg(test)]
mod smtp_auth_dispatch;

#[cfg(test)]
mod smtp_message;

#[cfg(test)]
mod smtp_render;

#[cfg(test)]
mod smtp_routes;

#[cfg(test)]
mod smtp_bound_transport;
#[cfg(test)]
mod smtp_provider_delivery;
#[cfg(test)]
mod smtp_provider_revision;
#[cfg(test)]
mod smtp_revision;
#[cfg(test)]
mod smtp_revision_delivery;

#[cfg(test)]
mod smtp_metadata;

#[cfg(test)]
mod smtp_submit;

#[cfg(test)]
mod smtp_submit_frame;

#[cfg(test)]
mod smtp_submit_dispatch;

fn main() {
    eprintln!("Slurm Notify is under development; monitoring is not implemented.");
    std::process::exit(1);
}

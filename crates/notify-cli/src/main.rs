//! Repository foundation executable; monitoring is not implemented.

#[cfg(test)]
mod delivery;
#[cfg(test)]
mod lifecycle;
#[cfg(test)]
mod scheduler;
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

fn main() {
    eprintln!("Slurm Notify is under development; monitoring is not implemented.");
    std::process::exit(1);
}

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

fn main() {
    eprintln!("Slurm Notify is under development; monitoring is not implemented.");
    std::process::exit(1);
}

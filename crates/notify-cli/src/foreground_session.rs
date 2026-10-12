//! Private owned foreground session; caller-driven ticks, no installed loop.
use crate::{mixed_config_composition, polling_cycle, storage};
use std::{path::Path, time::Duration};
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    InvalidLimit,
    Cancelled,
    Preparation,
    Ownership,
    Closed,
    Cycle,
    Status,
}
struct Active {
    plan: mixed_config_composition::Plan,
    store: storage::Store,
    cycles: polling_cycle::Cycles,
}
// No Debug/Clone; one session owns one plan and one guarded state connection.
pub(super) struct Session {
    active: Option<Active>,
}
impl Session {
    pub(super) fn start(
        path: &Path,
        poll_limit: usize,
        lock_timeout: Duration,
        identity: impl FnOnce(usize) -> Result<Zeroizing<Vec<u8>>, ()>,
        lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, Error> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let cycles = polling_cycle::Cycles::new(poll_limit).map_err(|_| Error::InvalidLimit)?;
        let plan = mixed_config_composition::prepare_path(path, identity, lookup)
            .map_err(|_| Error::Preparation)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let store = plan
            .open_owned(lock_timeout)
            .map_err(|_| Error::Ownership)?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(Self {
            active: Some(Active {
                plan,
                store,
                cycles,
            }),
        })
    }
    // Every scheduling exit closes ownership; stage reports remain caller-observed.
    pub(super) fn run_with<T, M, W, X>(
        &mut self,
        limit: usize,
        hooks: crate::foreground_schedule::Hooks<T, M, W, X>,
    ) -> Result<crate::foreground_schedule::Exit, crate::foreground_schedule::Error>
    where
        T: FnMut(&mut Self) -> Result<polling_cycle::TickReport, Error>,
        M: FnMut() -> Option<Duration>,
        W: FnMut(Duration) -> Result<crate::foreground_schedule::Wake, ()>,
        X: FnMut() -> bool,
    {
        let interval = self
            .active
            .as_ref()
            .ok_or(crate::foreground_schedule::Error::Tick)?
            .plan
            .poll_interval();
        let crate::foreground_schedule::Hooks {
            mut tick,
            monotonic,
            wait,
            cancelled,
        } = hooks;
        let result = crate::foreground_schedule::run(
            interval,
            limit,
            crate::foreground_schedule::Hooks {
                tick: || tick(self).map(|report| report.cancelled).map_err(|_| ()),
                monotonic,
                wait,
                cancelled,
            },
        );
        self.stop();
        result
    }
    pub(super) fn stop(&mut self) {
        self.active.take();
    }
    pub(super) fn status(&mut self, now: i64) -> Result<polling_cycle::HealthReport, Error> {
        let active = self.active.as_mut().ok_or(Error::Closed)?;
        active
            .plan
            .status(&active.cycles, &mut active.store, now)
            .map_err(|_| Error::Status)
    }
    pub(super) fn tick_with<Q, L, H, O, C, X>(
        &mut self,
        programs: (&std::path::Path, &std::path::Path),
        hooks: crate::polling_cycle::Hooks<Q, (L, H, O), C, X>,
    ) -> Result<crate::polling_cycle::TickReport, Error>
    where
        Q: FnMut(
            &mut std::process::Command,
            Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
        L: FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
        H: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
        O: FnMut(
            &crate::storage::DueDelivery,
        ) -> Result<
            crate::delivery::ProviderOutcome,
            crate::supervised_provider::Diagnostic,
        >,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        let active = self.active.as_mut().ok_or(Error::Closed)?;
        let result = active
            .plan
            .tick_with(programs, &mut active.cycles, &mut active.store, hooks)
            .map_err(|_| Error::Cycle);
        if result.as_ref().is_ok_and(|report| report.cancelled) {
            self.stop();
        }
        result
    }
}

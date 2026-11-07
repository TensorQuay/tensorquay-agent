//! One large streaming buffer per search; waiting observes cancellation and elapsed limits.
use crate::{ToolError, builtin::search::budget::Budget};
use std::{
    sync::{Condvar, Mutex},
    time::Duration,
};

// The synchronous pool polls cancellation while a different worker owns the permit.
const PERMIT_CHECK_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Default)]
pub(super) struct LargeFiles {
    busy: Mutex<bool>,
    released: Condvar,
}
impl LargeFiles {
    pub(super) fn acquire(
        &self,
        budget: &mut Budget<'_>,
        #[cfg(test)] mut probe: Option<&super::pool::tests::Probe>,
    ) -> Result<Option<Permit<'_>>, ToolError> {
        loop {
            if !budget.check()? {
                return Ok(None);
            }
            let mut busy = self.busy.lock().map_err(|_| permit_error())?;
            if !*busy {
                *busy = true;
                return Ok(Some(Permit(self)));
            }
            #[cfg(test)]
            if let Some(probe) = probe.take() {
                probe.large.waiting();
            }
            drop(
                self.released
                    .wait_timeout(busy, PERMIT_CHECK_INTERVAL)
                    .map_err(|_| permit_error())?,
            );
        }
    }
}
pub(super) struct Permit<'a>(&'a LargeFiles);
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut busy = self
            .0
            .busy
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *busy = false;
        self.0.released.notify_one();
    }
}
fn permit_error() -> ToolError {
    ToolError::Io("search large-file permit failed".into())
}

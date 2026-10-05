use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

const ENABLED_BIT: u64 = 1 << 63;
const EPOCH_MASK: u64 = !ENABLED_BIT;

/// A single atomic epoch plus enabled bit makes invalidation and reopening
/// totally ordered. A stale refresh can never reopen a newer disabled epoch.
#[derive(Clone)]
pub(crate) struct SaveSyncCommandGate {
    state: Arc<AtomicU64>,
}

impl SaveSyncCommandGate {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn current_epoch(&self) -> u64 {
        self.state.load(Ordering::SeqCst) & EPOCH_MASK
    }

    pub(crate) fn invalidate(&self) -> u64 {
        let mut current = self.state.load(Ordering::SeqCst);
        loop {
            let epoch = current & EPOCH_MASK;
            let next_epoch = epoch.saturating_add(1).min(EPOCH_MASK);
            match self.state.compare_exchange(
                current,
                next_epoch,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return next_epoch,
                Err(updated) => current = updated,
            }
        }
    }

    pub(crate) fn open(&self, epoch: u64) -> bool {
        if epoch > EPOCH_MASK {
            return false;
        }
        let disabled = epoch;
        let enabled = epoch | ENABLED_BIT;
        match self
            .state
            .compare_exchange(disabled, enabled, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => true,
            Err(current) => current == enabled,
        }
    }

    pub(crate) fn close(&self) {
        self.state.fetch_and(EPOCH_MASK, Ordering::SeqCst);
    }

    pub(crate) fn enabled_for(&self, epoch: u64) -> bool {
        self.state.load(Ordering::SeqCst) == (epoch | ENABLED_BIT)
    }

    pub(crate) fn enablement(&self, epoch: u64) -> SaveSyncEnablement {
        SaveSyncEnablement::Command {
            state: Arc::clone(&self.state),
            epoch,
        }
    }
}

#[derive(Clone)]
pub(crate) enum SaveSyncEnablement {
    Command { state: Arc<AtomicU64>, epoch: u64 },
    Test(Arc<AtomicBool>),
}

impl SaveSyncEnablement {
    pub(crate) fn is_enabled(&self) -> bool {
        match self {
            Self::Command { state, epoch } => {
                state.load(Ordering::SeqCst) == (*epoch | ENABLED_BIT)
            }
            Self::Test(enabled) => enabled.load(Ordering::SeqCst),
        }
    }
}

impl From<Arc<AtomicBool>> for SaveSyncEnablement {
    fn from(enabled: Arc<AtomicBool>) -> Self {
        Self::Test(enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidation_and_reopen_are_atomic_across_epochs() {
        let gate = SaveSyncCommandGate::new();
        let old_epoch = gate.current_epoch();
        assert!(gate.open(old_epoch));
        assert!(gate.enabled_for(old_epoch));

        let new_epoch = gate.invalidate();
        assert!(!gate.enabled_for(old_epoch));
        assert!(!gate.open(old_epoch));
        assert!(gate.open(new_epoch));
        assert!(gate.enabled_for(new_epoch));
    }
}

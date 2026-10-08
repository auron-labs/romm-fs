//! Application ownership independent of the visible window's lifetime.
#[cfg(windows)]
use crate::controller::{ConnState, MountState};
use crate::controller::{Controller, UiState};
use rommfs_core::events::AppEvent;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

pub struct ApplicationSession {
    controller: Rc<Controller>,
    events: Receiver<AppEvent>,
    state: UiState,
}
impl ApplicationSession {
    pub fn new() -> Self {
        let (controller, events) = Controller::spawn();
        Self {
            controller: Rc::new(controller),
            events,
            state: UiState::new(2000),
        }
    }
    pub fn controller(&self) -> Rc<Controller> {
        self.controller.clone()
    }
    pub fn drain(&mut self) -> Vec<AppEvent> {
        let events: Vec<_> = self.events.try_iter().collect();
        for event in &events {
            self.state.apply(event);
        }
        events
    }
    #[cfg(windows)]
    pub fn status(&self) -> String {
        if self.state.conn_error.is_some()
            || self.state.mount_error.is_some()
            || self.state.startup_error.is_some()
            || self.state.save_sync_authentication_required
            || self.state.retrobat_setup_error.is_some()
            || self.state.installer_error.is_some()
            || self.state.save_sync_queue.as_ref().is_some_and(|q| {
                q.attention_games > 0 || q.pending_incoming > 0 || q.failure.is_some()
            })
        {
            return "RomMFS · needs attention — Open for details".into();
        }
        let connection = match self.state.conn {
            ConnState::Connected => "connected",
            ConnState::Connecting => "connecting",
            _ => "not connected",
        };
        let mount = match self.state.mount {
            MountState::Mounted => "mounted",
            MountState::Mounting => "mounting",
            _ => "not mounted",
        };
        format!(
            "RomMFS · {connection} · {mount} · saves {}",
            if self.state.save_sync_enabled {
                "on"
            } else {
                "off"
            }
        )
    }
    pub fn shutdown(&self) {
        self.controller.shutdown();
    }
}

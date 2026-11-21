use tokio::sync::broadcast;

use crate::application::ports::{RealtimeEventPort, SystemEvent};

#[derive(Clone)]
pub struct BroadcastEventHub {
    tx: broadcast::Sender<SystemEvent>,
}

impl BroadcastEventHub {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(32));
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SystemEvent> {
        self.tx.subscribe()
    }
}

impl Default for BroadcastEventHub {
    fn default() -> Self {
        Self::new(256)
    }
}

impl RealtimeEventPort for BroadcastEventHub {
    fn publish(&self, event: SystemEvent) {
        let _ = self.tx.send(event);
    }
}

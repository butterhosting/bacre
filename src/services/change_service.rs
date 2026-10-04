//! Where the daemon says "something a page shows has changed": a job started or ended, a
//! listing began or came in. It carries no detail on purpose; a page that hears it fetches
//! what it shows again.

use tokio::sync::broadcast;

pub struct ChangeService {
    sender: broadcast::Sender<()>,
}

impl Default for ChangeService {
    fn default() -> Self {
        // a listener that falls this far behind has missed nothing it cannot fetch
        let (sender, _) = broadcast::channel(16);
        Self { sender }
    }
}

impl ChangeService {
    pub fn changed(&self) {
        // nobody listening is fine
        let _ = self.sender.send(());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.sender.subscribe()
    }
}

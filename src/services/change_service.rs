use tokio::sync::broadcast;

pub struct ChangeService {
    sender: broadcast::Sender<()>,
}

impl Default for ChangeService {
    fn default() -> Self {
        let (sender, _) = broadcast::channel(16);
        Self { sender }
    }
}

impl ChangeService {
    pub fn changed(&self) {
        let _ = self.sender.send(());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.sender.subscribe()
    }
}

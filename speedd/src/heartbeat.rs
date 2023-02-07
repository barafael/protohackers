use std::time::Duration;
use tokio::sync::mpsc;

pub async fn heartbeat(dur: Duration, sender: mpsc::Sender<()>) -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(dur);
    loop {
        // Terminate if receivers dropped
        sender.send(()).await?;
        interval.tick().await;
    }
}

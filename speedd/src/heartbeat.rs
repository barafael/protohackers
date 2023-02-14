use std::time::Duration;
use tokio::sync::mpsc;

/// Sends heartbeat signals at the specified interval on the provided `mpsc::Sender<()>`.
/// When the `mpsc::Receiver<()>` handle is dropped or the channel is closed, it will drop.
///
/// # Panics
/// Panics if the provided duration is zero.
pub async fn run(dur: Duration, sender: mpsc::Sender<()>) {
    let mut interval = tokio::time::interval(dur);
    loop {
        interval.tick().await;
        tracing::trace!("Sending heartbeat");
        if sender.send(()).await.is_err() {
            tracing::info!("Dropping heartbeat");
            return;
        }
    }
}

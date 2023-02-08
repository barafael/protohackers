use std::time::Duration;
use tokio::sync::mpsc;

pub async fn heartbeat(dur: Duration, sender: mpsc::Sender<()>) -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(dur);
    loop {
        interval.tick().await;
        println!("Sending heartbeat");
        if sender.send(()).await.is_err() {
            println!("Dropping heartbeat");
            break;
        }
    }
    Ok(())
}

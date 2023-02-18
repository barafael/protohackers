use lrcp_codec::Frame;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, DuplexStream, ReadHalf},
    sync::{broadcast, mpsc},
    time::{sleep, Instant},
};

#[derive(Debug)]
pub struct Writer {
    id: u32,
    length: u32,
}

impl Writer {
    pub fn with_id(id: u32) -> Self {
        Self { id, length: 0 }
    }

    pub async fn run(
        mut self,
        mut channel: ReadHalf<DuplexStream>,
        writer: mpsc::Sender<Frame>,
        mut reader: broadcast::Receiver<Frame>,
        close: broadcast::Sender<Frame>,
    ) -> anyhow::Result<()> {
        let session_timer = sleep(Duration::from_secs(60));
        let repeat_timer = sleep(Duration::from_secs(3));
        tokio::pin!(session_timer);
        tokio::pin!(repeat_timer);

        let mut awaiting_ack = None;
        let mut current_item: Option<Frame> = None;
        let mut buffer = [0u8; 1024];
        loop {
            tokio::select! {
                Ok(msg) = reader.recv() => {
                    if msg.session_id() != self.id {
                        continue;
                    }
                    tracing::info!("Resetting session timer"); // Why sometimes twice?
                    dbg!(&msg);
                    session_timer.as_mut().reset(Instant::now() + Duration::from_secs(60));
                    match msg {
                        Frame::Ack { length, .. } => {
                            if Some(Frame::Ack{session: self.id, length}) == awaiting_ack {
                                self.length = length;
                                current_item = None;
                                tracing::info!("Received matching ack! State: {self:?}");
                            }
                        }
                        Frame::Close(_) => {
                            break;
                        }
                        _ => {}
                    }
                }
                () = &mut repeat_timer, if current_item.is_some() => {
                    if let Some(ref curr) = current_item {
                        tracing::info!("Re-sending {current_item:?}");
                        writer.send(curr.clone()).await?;
                        repeat_timer.as_mut().reset(Instant::now() + Duration::from_secs(3));
                    }
                }
                () = &mut session_timer, if current_item.is_some() => {
                    tracing::info!("No traffic, ending session");
                    close.send(Frame::Close(self.id))?;
                    // TODO writer.send(Frame::Close...))?
                    break;
                }
                Ok(len) = channel.read(&mut buffer), if current_item.is_none() => {
                    awaiting_ack = Some(Frame::Ack {
                        session: self.id,
                        length: self.length + len as u32,
                    });
                    let data = String::from_utf8(buffer[..len].to_vec())?;
                    let frame = Frame::Data {
                        session: self.id,
                        position: self.length,
                        data,
                    };
                    current_item = Some(frame.clone());
                    tracing::info!("Sending data {frame:?}");
                    tracing::info!("Waiting for ack: {awaiting_ack:?}");
                    writer.send(frame).await?;
                    repeat_timer
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(3));
                    session_timer
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(60));
                        }
            }
        }
        tracing::info!("Exiting writer for session {}", self.id);
        Ok(())
    }
}

use lrcp_codec::Frame;
use tokio::{
    io::{AsyncWriteExt, DuplexStream, WriteHalf},
    sync::{broadcast, mpsc},
};

#[derive(Debug)]
pub struct Reader {
    id: u32,
    length: u32,
}

impl Reader {
    pub fn with_id(id: u32) -> Self {
        Self { id, length: 0 }
    }

    pub async fn run(
        mut self,
        mut channel: WriteHalf<DuplexStream>,
        writer: mpsc::Sender<Frame>,
        mut reader: broadcast::Receiver<Frame>,
    ) -> anyhow::Result<()> {
        while let Ok(msg) = reader.recv().await {
            if msg.session_id() != self.id {
                continue;
            }
            match msg {
                Frame::Connect(_) => {
                    tracing::info!(
                        "Sending repeated ACK for existing session (id: {})",
                        self.id
                    );
                    self.ack(&writer).await?;
                }
                Frame::Ack { .. } => {
                    // Don't care about Ack in reader
                }
                Frame::Data { position, data, .. } => {
                    let frame = self.handle_data(position, data, &mut channel).await?;
                    writer.send(frame).await?;
                }
                Frame::Close(id) => {
                    tracing::info!("Stopping reader for id {id}");
                    break;
                }
            }
        }
        tracing::info!("Exiting reader for session {}", self.id);
        Ok(())
    }

    async fn ack(&self, writer: &mpsc::Sender<Frame>) -> anyhow::Result<()> {
        let ack = Frame::Ack {
            session: self.id,
            length: self.length,
        };
        writer
            .send(ack)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send acknowledgement, dropping {e}"))
    }

    async fn handle_data(
        &mut self,
        position: u32,
        data: String,
        channel: &mut WriteHalf<DuplexStream>,
    ) -> anyhow::Result<Frame> {
        if position == self.length {
            tracing::info!("Accepting data for session {}", self.id);
            self.length += data.len() as u32;
            channel.write_all(data.as_bytes()).await?;
            //channel.write(b"\n").await?;
        } else {
            tracing::info!(
                "Ignoring data for session {}, position: {position}, actual received bytes: {}",
                self.id,
                self.length
            );
        }
        Ok(Frame::Ack {
            session: self.id,
            length: self.length,
        })
    }
}

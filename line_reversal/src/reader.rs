use futures_util::{Sink, SinkExt};
use lrcp_codec::Frame;
use std::sync::Arc;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

/// The receiving half of a session: acknowledges data from the peer and passes it to the application.
///
/// It *is* its data: the session id, and how much data it has received.
#[derive(Debug, PartialEq, Eq)]
pub struct Reader {
    id: u32,
    length: u32,
}

impl Reader {
    pub fn with_id(id: u32) -> Self {
        Self { id, length: 0 }
    }

    pub async fn event_loop<A, S>(
        mut self,
        mut application: A,
        mut frames: S,
        mut inbox: mpsc::Receiver<Arc<Frame>>,
    ) -> Self
    where
        A: AsyncWrite + Unpin,
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        if let Err(error) = self.run(&mut application, &mut frames, &mut inbox).await {
            tracing::warn!(?error, "Reader for session {} failed", self.id);
        }
        tracing::info!("Exiting reader for session {}", self.id);
        self
    }

    async fn run<A, S>(
        &mut self,
        channel: &mut A,
        writer: &mut S,
        reader: &mut mpsc::Receiver<Arc<Frame>>,
    ) -> anyhow::Result<()>
    where
        A: AsyncWrite + Unpin,
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        while let Some(msg) = reader.recv().await {
            match *msg {
                Frame::Connect(_) => {
                    tracing::info!(
                        "Sending repeated ACK for existing session (id: {})",
                        self.id
                    );
                    self.ack(writer).await?;
                }
                Frame::Ack { .. } => {
                    // Don't care about Ack in reader
                }
                Frame::Data {
                    position, ref data, ..
                } => {
                    let frame = self.handle_data(position, data, channel).await?;
                    writer.send(frame).await.map_err(Into::into)?;
                }
                Frame::Close(id) => {
                    tracing::info!("Stopping reader for id {id}");
                    break;
                }
            }
        }
        Ok(())
    }

    async fn ack<S>(&self, writer: &mut S) -> anyhow::Result<()>
    where
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        let ack = Frame::Ack {
            session: self.id,
            length: self.length,
        };
        writer.send(ack).await.map_err(|e| {
            anyhow::anyhow!(
                "Failed to send acknowledgement, dropping {}",
                Into::<anyhow::Error>::into(e)
            )
        })
    }

    async fn handle_data<A>(
        &mut self,
        position: u32,
        data: &str,
        channel: &mut A,
    ) -> anyhow::Result<Frame>
    where
        A: AsyncWrite + Unpin,
    {
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

#[cfg(test)]
mod test {
    use super::*;

    fn data(position: u32, data: &str) -> Arc<Frame> {
        Arc::new(Frame::Data {
            session: 1,
            position,
            data: data.to_string(),
        })
    }

    fn ack(length: u32) -> Frame {
        Frame::Ack { session: 1, length }
    }

    #[tokio::test]
    async fn acknowledges_data_in_order_only() {
        let (tx, rx) = mpsc::channel(8);
        for frame in [
            data(0, "hello\n"),
            data(0, "hello\n"),
            data(10, "from the future\n"),
            Arc::new(Frame::Connect(1)),
            Arc::new(ack(3)),
            data(6, "world\n"),
        ] {
            tx.send(frame).await.unwrap();
        }

        // Important for this test:
        drop(tx);

        let mut application = Vec::new();
        let mut sent = Vec::new();
        let reader = Reader::with_id(1)
            .event_loop(&mut application, &mut sent, rx)
            .await;

        assert_eq!(application, b"hello\nworld\n");
        assert_eq!(sent, [ack(6), ack(6), ack(6), ack(6), ack(12)]);
        assert_eq!(reader, Reader { id: 1, length: 12 });
    }

    #[tokio::test]
    async fn stops_at_close() {
        let (tx, rx) = mpsc::channel(8);
        for frame in [data(0, "a\n"), Arc::new(Frame::Close(1)), data(2, "b\n")] {
            tx.send(frame).await.unwrap();
        }

        let mut application = Vec::new();
        let mut sent = Vec::new();
        let reader = Reader::with_id(1)
            .event_loop(&mut application, &mut sent, rx)
            .await;

        assert_eq!(application, b"a\n");
        assert_eq!(sent, [ack(2)]);
        assert_eq!(reader, Reader { id: 1, length: 2 });
    }
}

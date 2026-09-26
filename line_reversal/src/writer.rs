use futures_util::{Sink, SinkExt};
use itertools::Itertools;
use lrcp_codec::Frame;
use std::iter::Iterator;
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::mpsc,
    time::{sleep, Instant},
};

/// The sending half of a session: sends the application's output to the peer, until acknowledged.
///
/// It *is* its data: the session id, how much data the peer acknowledged, and what is in flight.
#[derive(Debug, PartialEq, Eq)]
pub struct Writer {
    id: u32,
    length: u32,
    awaiting_ack: Option<Frame>,
    current_item: Option<Frame>,
    chunks: VecDeque<String>,
}

impl Writer {
    pub fn with_id(id: u32) -> Self {
        Self {
            id,
            length: 0,
            awaiting_ack: None,
            current_item: None,
            chunks: VecDeque::new(),
        }
    }

    /// Runs until the application closes, the session times out, the peer closes, or the inbox closes.
    /// On the first two, the session's reader is told to close.
    pub async fn event_loop<A, S>(
        mut self,
        mut application: A,
        mut frames: S,
        mut inbox: mpsc::Receiver<Arc<Frame>>,
        reader: mpsc::Sender<Arc<Frame>>,
    ) -> Self
    where
        A: AsyncRead + Unpin,
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        if let Err(error) = self
            .run(&mut application, &mut frames, &mut inbox, &reader)
            .await
        {
            tracing::warn!(?error, "Writer for session {} failed", self.id);
        }
        tracing::info!("Exiting writer for session {}", self.id);
        self
    }

    async fn run<A, S>(
        &mut self,
        channel: &mut A,
        writer: &mut S,
        reader: &mut mpsc::Receiver<Arc<Frame>>,
        close: &mpsc::Sender<Arc<Frame>>,
    ) -> anyhow::Result<()>
    where
        A: AsyncRead + Unpin,
        S: Sink<Frame> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        let session_timer = sleep(Duration::from_secs(60));
        let repeat_timer = sleep(Duration::from_secs(3));
        tokio::pin!(session_timer);
        tokio::pin!(repeat_timer);

        let mut buffer = [0u8; 1024];
        loop {
            tokio::select! {
                // Receive acknowledgement and close messages
                msg = reader.recv() => {
                    let Some(msg) = msg else {
                        break;
                    };
                    tracing::info!("Resetting session timer"); // Why sometimes twice?
                    tracing::debug!(?msg);
                    session_timer.as_mut().reset(Instant::now() + Duration::from_secs(60));
                    match *msg {
                        Frame::Ack { length, .. } => {
                            if Some(Frame::Ack { session: self.id, length }) == self.awaiting_ack {
                                self.length = length;
                                tracing::info!("Received matching ack! State: {self:?}");
                                self.current_item = if self.chunks.is_empty() {
                                    self.awaiting_ack = None;
                                    None
                                } else if let Some(data) = self.chunks.pop_front() {
                                    self.awaiting_ack = Some(Frame::Ack {
                                        session: self.id,
                                        length: self.length + data.len() as u32,
                                    });
                                    tracing::info!("Waiting for ack: {:?}", self.awaiting_ack);
                                    let frame = Frame::Data { session: self.id, position: self.length, data };
                                    Some(frame)
                                } else {
                                    self.awaiting_ack = None;
                                    None
                                }
                            }
                            repeat_timer
                                .as_mut()
                                .reset(Instant::now() + Duration::from_secs(3));
                            session_timer
                                .as_mut()
                                .reset(Instant::now() + Duration::from_secs(60));
                        }
                        Frame::Close(_) => {
                            break;
                        }
                        _ => {}
                    }
                }
                // Receive new chunks of data, if not currently sending
                Ok(len) = channel.read(&mut buffer), if self.current_item.is_none() => {
                    if len == 0 {
                        tracing::info!("Application channel closed, ending session {}", self.id);
                        tell(close, self.id).await;
                        break;
                    }
                    self.chunks.extend(buffer[..len].iter().map(|c| *c as char).chunks(1000 - 17).into_iter().map(Iterator::collect::<String>).map(|s| lrcp_codec::escape::escape(&s)));
                    let item = self.chunks.pop_front().unwrap();
                    tracing::debug!(?item);
                    self.awaiting_ack = Some(Frame::Ack {
                        session: self.id,
                        length: self.length + item.len() as u32,
                    });
                    let frame = Frame::Data {
                        session: self.id,
                        position: self.length,
                        data: item,
                    };
                    self.current_item = Some(frame.clone());
                    tracing::info!("Sending data {frame:?}");
                    tracing::info!("Waiting for ack: {:?}", self.awaiting_ack);
                    writer.send(frame).await.map_err(Into::into)?;
                    repeat_timer
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(3));
                    session_timer
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(60));
                }
                // Repeat sending at specified rate, if currently sending
                // TODO use time::Interval here?
                () = &mut repeat_timer, if self.current_item.is_some() => {
                    if let Some(ref curr) = self.current_item {
                        tracing::info!("Re-sending {:?}", self.current_item);
                        writer.send(curr.clone()).await.map_err(Into::into)?;
                        repeat_timer.as_mut().reset(Instant::now() + Duration::from_secs(3));
                    }
                }
                // Close session if no traffic while currently sending
                // TODO use time::Interval here?
                () = &mut session_timer, if self.current_item.is_some() => {
                    tracing::info!("No traffic, ending session");
                    tell(close, self.id).await;
                    // TODO writer.send(Frame::Close...))?
                    break;
                }
            }
        }
        Ok(())
    }
}

/// Tell the session's reader to close.
async fn tell(reader: &mpsc::Sender<Arc<Frame>>, id: u32) {
    if reader.send(Arc::new(Frame::Close(id))).await.is_err() {
        tracing::info!("Reader for session {id} is already gone");
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures_util::StreamExt;

    fn data(position: u32, data: &str) -> Frame {
        Frame::Data {
            session: 1,
            position,
            data: data.to_string(),
        }
    }

    fn ack(length: u32) -> Arc<Frame> {
        Arc::new(Frame::Ack { session: 1, length })
    }

    #[tokio::test]
    async fn sends_until_acknowledged_then_closes_with_application() {
        let application: &[u8] = b"olleh\n";
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, mut reader_rx) = mpsc::channel(8);

        let peer = async move {
            let frame = peer.next().await;
            inbox.send(ack(6)).await.unwrap();
            // Keep the inbox open: if it closed, too, the writer might end for that reason instead.
            (frame, inbox)
        };
        let (writer, (frame, _inbox)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        assert_eq!(frame, Some(data(0, "olleh\n")));
        assert_eq!(*reader_rx.recv().await.unwrap(), Frame::Close(1));
        assert_eq!(
            writer,
            Writer {
                length: 6,
                ..Writer::with_id(1)
            }
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retransmits_until_acknowledged() {
        let application: &[u8] = b"olleh\n";
        let (sent, mut peer) = futures::channel::mpsc::unbounded();
        let (inbox, rx) = mpsc::channel(8);
        let (reader, _reader_rx) = mpsc::channel(8);

        let peer = async move {
            let first = peer.next().await;
            let start = Instant::now();
            let second = peer.next().await;
            let elapsed = start.elapsed();
            inbox.send(ack(6)).await.unwrap();
            (first, second, elapsed)
        };
        let (_, (first, second, elapsed)) = tokio::join!(
            Writer::with_id(1).event_loop(application, sent, rx, reader),
            peer,
        );

        assert_eq!(first, Some(data(0, "olleh\n")));
        assert_eq!(second, first);
        assert_eq!(elapsed, Duration::from_secs(3));
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_without_acknowledgement() {
        let application: &[u8] = b"olleh\n";
        let mut sent = Vec::new();
        // The inbox stays open, but no acknowledgement ever arrives.
        let (_inbox, rx) = mpsc::channel(8);
        let (reader, mut reader_rx) = mpsc::channel(8);
        let start = Instant::now();

        let writer = Writer::with_id(1)
            .event_loop(application, &mut sent, rx, reader)
            .await;

        assert_eq!(start.elapsed(), Duration::from_secs(60));
        assert_eq!(*reader_rx.recv().await.unwrap(), Frame::Close(1));
        // Sent every 3 seconds; at 60 seconds, retransmission and timeout coincide.
        assert!((20..=21).contains(&sent.len()));
        assert!(sent.iter().all(|frame| *frame == data(0, "olleh\n")));
        assert_eq!(writer.current_item, Some(data(0, "olleh\n")));
    }
}

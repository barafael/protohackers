use itertools::Itertools;
use lrcp_codec::Frame;
use std::iter::Iterator;
use std::{collections::VecDeque, sync::Arc, time::Duration};
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
        mut reader: broadcast::Receiver<Arc<Frame>>,
        close: broadcast::Sender<Arc<Frame>>,
    ) -> anyhow::Result<()> {
        let session_timer = sleep(Duration::from_secs(60));
        let repeat_timer = sleep(Duration::from_secs(3));
        tokio::pin!(session_timer);
        tokio::pin!(repeat_timer);

        let mut awaiting_ack = None;
        let mut current_item: Option<Frame> = None;
        let mut chunks = VecDeque::<String>::new();

        let mut buffer = [0u8; 1024];
        loop {
            tokio::select! {
                // Receive acknowledgement and close broadcasts
                Ok(msg) = reader.recv() => {
                    if msg.session_id() != self.id {
                        continue;
                    }
                    tracing::info!("Resetting session timer"); // Why sometimes twice?
                    dbg!(&msg);
                    session_timer.as_mut().reset(Instant::now() + Duration::from_secs(60));
                    match *msg {
                        Frame::Ack { length, .. } => {
                            if Some(Frame::Ack { session: self.id, length }) == awaiting_ack {
                                self.length = length;
                                tracing::info!("Received matching ack! State: {self:?}");
                                current_item = if chunks.is_empty() {
                                    awaiting_ack = None;
                                    None
                                } else if let Some(data) = chunks.pop_front() {
                                    awaiting_ack = Some(Frame::Ack {
                                        session: self.id,
                                        length: self.length + data.len() as u32,
                                    });
                                    tracing::info!("Waiting for ack: {awaiting_ack:?}");
                                    let frame = Frame::Data { session: self.id, position: self.length, data };
                                    Some(frame)
                                } else {
                                    awaiting_ack = None;
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
                Ok(len) = channel.read(&mut buffer), if current_item.is_none() => {
                    if len == 0 {
                        tracing::info!("Application channel closed, ending session {}", self.id);
                        close.send(Arc::new(Frame::Close(self.id)))?;
                        break;
                    }
                    chunks.extend(buffer[..len].iter().map(|c| *c as char).chunks(1000 - 17).into_iter().map(Iterator::collect::<String>).map(|s| lrcp_codec::escape::escape(&s)));
                    let item = chunks.pop_front().unwrap();
                    dbg!(&item);
                    awaiting_ack = Some(Frame::Ack {
                        session: self.id,
                        length: self.length + item.len() as u32,
                    });
                    let frame = Frame::Data {
                        session: self.id,
                        position: self.length,
                        data: item,
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
                // Repeat sending at specified rate, if currently sending
                // TODO use time::Interval here?
                () = &mut repeat_timer, if current_item.is_some() => {
                    if let Some(ref curr) = current_item {
                        tracing::info!("Re-sending {current_item:?}");
                        writer.send(curr.clone()).await?;
                        repeat_timer.as_mut().reset(Instant::now() + Duration::from_secs(3));
                    }
                }
                // Close session if no traffic while currently sending
                // TODO use time::Interval here?
                () = &mut session_timer, if current_item.is_some() => {
                    tracing::info!("No traffic, ending session");
                    close.send(Arc::new(Frame::Close(self.id)))?;
                    // TODO writer.send(Frame::Close...))?
                    break;
                }
            }
        }
        tracing::info!("Exiting writer for session {}", self.id);
        Ok(())
    }
}

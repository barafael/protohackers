use crate::{
    heartbeat,
    server::{self, TicketRecord},
    Road,
};
use async_channel as mpmc;
use futures::{stream::SelectAll, Sink, SinkExt, Stream, StreamExt};
use speedd_codecs::client;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub struct Dispatcher {
    tickets: SelectAll<mpmc::Receiver<TicketRecord>>,
}

impl Dispatcher {
    pub async fn new(
        roads: &[u16],
        dispatchers: &mpsc::Sender<(Road, oneshot::Sender<mpmc::Receiver<TicketRecord>>)>,
    ) -> anyhow::Result<Self> {
        let mut handles = Vec::new();
        for road in roads {
            let (tx, rx) = oneshot::channel();
            dispatchers.send((*road, tx)).await?;
            let receiver = rx.await?;
            handles.push(receiver);
        }
        // TODO try with just iterator, no vector
        let tickets = futures::stream::select_all(handles.into_iter());
        Ok(Self { tickets })
    }

    pub async fn run<R, W>(
        mut self,
        mut reader: R,
        mut writer: W,
        mut heartbeat_sender: Option<mpsc::Sender<()>>,
        mut heartbeats: mpsc::Receiver<()>,
    ) -> anyhow::Result<()>
    where
        R: Stream<Item = Result<client::Message, anyhow::Error>> + Send + Unpin,
        W: Sink<server::Message, Error = anyhow::Error> + Send + Unpin,
    {
        tracing::info!("Starting Dispatcher Client loop");
        loop {
            tokio::select! {
                Some(Ok(msg)) = reader.next() => {
                    tracing::info!("Received dispatcher message {msg:?}");
                    self.handle_client_message(msg, &mut writer, &mut heartbeat_sender).await?;
                }
                Some(msg) = self.tickets.next() => {
                    tracing::info!("Received ticket {msg:?}");
                    writer.send(server::Message::Ticket(msg)).await?;
                }
                Some(()) = heartbeats.recv() => {
                    writer.send(server::Message::Heartbeat).await?;
                }
                else => break
            }
        }
        tracing::info!("Leaving Dispatcher Client loop");
        Ok(())
    }

    async fn handle_client_message<W>(
        &self,
        msg: client::Message,
        writer: &mut W,
        heartbeat_sender: &mut Option<mpsc::Sender<()>>,
    ) -> anyhow::Result<()>
    where
        W: Sink<server::Message, Error = anyhow::Error> + Unpin,
    {
        match msg {
            client::Message::Plate(_) => {
                writer
                    .send(server::Message::Error(
                        "You Sir Dispatcher are confused".to_string(),
                    ))
                    .await?;
            }
            client::Message::WantHeartbeat(dur) => {
                if let Some(heartbeat_sender) = heartbeat_sender.take() {
                    if !dur.is_zero() {
                        tracing::info!("Spawning a new heartbeat");
                        tokio::spawn(heartbeat::heartbeat(dur, heartbeat_sender));
                    } else {
                        tracing::warn!("Ignoring zero-duration heartbeat");
                    }
                } else {
                    tracing::info!("Ignoring repeated heartbeat request");
                    writer
                        .send(server::Message::Error(
                            "You already specified a heartbeat".to_string(),
                        ))
                        .await?;
                }
            }
            client::Message::IAmCamera { .. } => {
                writer
                    .send(server::Message::Error(
                        "No you're not (a camera)".to_string(),
                    ))
                    .await?;
            }
            client::Message::IAmDispatcher(_) => {
                writer
                    .send(server::Message::Error(
                        "Yes, you are (a dispatcher)".to_string(),
                    ))
                    .await?;
            }
        }
        Ok(())
    }
}

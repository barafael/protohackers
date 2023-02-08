use crate::{
    heartbeat,
    server::{self, TicketRecord},
    Road,
};
use futures::{Sink, SinkExt, Stream, StreamExt};
use speedd_codecs::client;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct Dispatcher {
    receiver: mpsc::Receiver<TicketRecord>,
}

impl Dispatcher {
    pub async fn new(
        roads: &[u16],
        dispatchers: &mpsc::Sender<(Road, mpsc::Sender<TicketRecord>)>,
    ) -> anyhow::Result<Self> {
        let (ticket_tx, receiver) = mpsc::channel(32);
        for road in roads {
            dispatchers.send((*road, ticket_tx.clone())).await?;
        }
        Ok(Self { receiver })
    }

    pub async fn run<R, W>(
        mut self,
        mut reader: R,
        mut writer: W,
        mut heartbeat_sender: Option<mpsc::Sender<()>>,
        mut heartbeat_receiver: mpsc::Receiver<()>,
    ) -> anyhow::Result<()>
    where
        R: Stream<Item = Result<client::Message, anyhow::Error>> + Unpin,
        W: Sink<server::Message, Error = anyhow::Error> + Unpin,
    {
        loop {
            tokio::select! {
                Some(Ok(msg)) = reader.next() => {
                    println!("Received dispatcher message {msg:?}");
                    Self::handle_client_message(msg, &mut writer, &mut heartbeat_sender).await?;
                }
                Some(msg) = self.receiver.recv() => {
                    writer.send(server::Message::Ticket(msg)).await?;
                }
                Some(()) = heartbeat_receiver.recv() => {
                    writer.send(server::Message::Heartbeat).await?;
                }
            }
        }
    }

    async fn handle_client_message<W>(
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
                    println!("Spawning a new heartbeat");
                    tokio::spawn(heartbeat::heartbeat(dur, heartbeat_sender));
                } else {
                    println!("Ignoring repeated heartbeat request");
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

use crate::{
    client, heartbeat,
    server::{self, TicketRecord},
    Dispatchers,
};
use futures::{Sink, SinkExt, Stream, StreamExt};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct Dispatcher {
    roads: Vec<u16>,
    receiver: mpsc::Receiver<TicketRecord>,
}

impl Dispatcher {
    pub fn new(roads: Vec<u16>, dispatchers: Arc<RwLock<Dispatchers>>) -> Self {
        let (ticket_tx, receiver) = mpsc::channel(32);
        let mut map = dispatchers.write().unwrap();
        for road in &roads {
            let ticket_tx = ticket_tx.clone();
            map.entry(*road).or_default().push(ticket_tx);
        }
        Self { roads, receiver }
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
                Some(msg) = self.receiver.recv() => {
                    writer.send(server::Message::Ticket(msg)).await?;
                }
                Some(()) = heartbeat_receiver.recv() => {
                    writer.send(server::Message::Heartbeat).await?;
                }
                Some(Ok(msg)) = reader.next() => {
                    Self::handle_client_message(msg, &mut writer, &mut heartbeat_sender).await?;
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
                    writer.send(server::Message::Heartbeat).await?;
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

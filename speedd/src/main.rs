#![feature(iter_array_chunks)]

use crate::camera::CameraClient;
use crate::client::{client_action, Action};
use crate::dispatcher::Dispatcher;
use collector::Collector;
use futures::{Sink, SinkExt, Stream, StreamExt};
use speedd_codecs::camera::Camera;
use speedd_codecs::client::decoder::MessageDecoder;
use speedd_codecs::client::Message as ClientMessage;
use speedd_codecs::plate::PlateRecord;
use speedd_codecs::server::{self, TicketRecord};
use speedd_codecs::Road;
use std::env;
use tokio::{net::TcpListener, sync::mpsc};
use tokio_util::codec::{FramedRead, FramedWrite};

mod camera;
mod client;
mod collector;
mod dispatcher;
mod heartbeat;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8000".to_string());

    let listener = TcpListener::bind(listen_addr).await?;

    let (reporting_tx, reporting_rx) = mpsc::channel(256);
    let (dispatcher_subscription_tx, dispatcher_subscription_rx) = mpsc::channel(16);

    tokio::spawn(Collector::new().run(reporting_rx, dispatcher_subscription_rx));

    while let Ok((inbound, addr)) = listener.accept().await {
        println!("Accepted connection from {addr}");
        let (reader, writer) = inbound.into_split();
        let reader = FramedRead::new(reader, MessageDecoder::default());
        let writer = FramedWrite::new(writer, server::encoder::MessageEncoder::default());
        let reporting_tx = reporting_tx.clone();
        let dispatcher_tx = dispatcher_subscription_tx.clone();
        tokio::spawn(async move {
            handle_connection(reader, writer, reporting_tx, dispatcher_tx).await
        });
    }

    Ok(())
}

async fn handle_connection<R, W>(
    mut reader: R,
    mut writer: W,
    plate_tx: mpsc::Sender<(PlateRecord, Camera)>,
    dispatcher_tx: mpsc::Sender<(Road, mpsc::Sender<TicketRecord>)>,
) -> anyhow::Result<()>
where
    R: Stream<Item = Result<ClientMessage, anyhow::Error>> + Send + Unpin,
    W: Sink<server::Message, Error = anyhow::Error> + Send + Unpin,
{
    let (heartbeat_sender, mut heartbeat_receiver) = mpsc::channel(16);
    let mut heartbeat_sender = Some(heartbeat_sender);

    loop {
        tokio::select! {
            Some(Ok(msg)) = reader.next() => {
                let action = client_action(msg, &mut heartbeat_sender);
                match action {
                    Action::None => {},
                    Action::Reply(r) => writer.send(r).await?,
                    Action::SpawnCamera(c) => {
                        let client = CameraClient::new(c);
                        CameraClient::run(client, reader, writer, plate_tx, heartbeat_sender, heartbeat_receiver).await?;
                        break;
                    }
                    Action::SpawnDispatcher(r) => {
                        let dispatcher = Dispatcher::new(&r, &dispatcher_tx).await?;
                        Dispatcher::run(dispatcher, reader, writer, heartbeat_sender, heartbeat_receiver).await?;
                        break;
                    }
                }
            }
            Some(()) = heartbeat_receiver.recv() => {
                writer.send(server::Message::Heartbeat).await?;
            }
        }
    }
    Ok(())
}

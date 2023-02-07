#![feature(iter_array_chunks)]

use crate::camera::Camera;
use crate::client::{client_action, Action};
use crate::dispatcher::Dispatcher;
use camera::PlateRecord;
use collector::Collector;
use futures::{Sink, SinkExt, Stream, StreamExt};
use server::TicketRecord;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, RwLock};
use tokio::{net::TcpListener, sync::mpsc};
use tokio_util::codec::{FramedRead, FramedWrite};

mod camera;
mod client;
mod collector;
mod dispatcher;
mod heartbeat;
mod server;

pub type Dispatchers = HashMap<Road, Vec<mpsc::Sender<TicketRecord>>>;

pub type Timestamp = u32;
pub type Mile = u16;
pub type Road = u16;
pub type Limit = u16;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8000".to_string());

    let listener = TcpListener::bind(listen_addr).await?;

    let (plate_tx, plate_rx) = mpsc::channel(256);
    let dispatchers = Arc::new(RwLock::new(Dispatchers::default()));

    tokio::spawn(Collector::new(plate_rx, dispatchers.clone()).run());

    while let Ok((inbound, addr)) = listener.accept().await {
        println!("Accepted connection from {addr}");
        let (reader, writer) = inbound.into_split();
        let reader = FramedRead::new(reader, client::decoder::MessageDecoder::default());
        let writer = FramedWrite::new(writer, server::encoder::MessageEncoder::default());
        let plate_tx = plate_tx.clone();
        let dispatchers = dispatchers.clone();
        tokio::spawn(async move {
            handle_connection(reader, writer, plate_tx, dispatchers)
                .await
                .unwrap();
        });
    }

    Ok(())
}

async fn handle_connection<R, W>(
    mut reader: R,
    mut writer: W,
    plate_tx: mpsc::Sender<(PlateRecord, Camera)>,
    dispatchers: Arc<RwLock<Dispatchers>>,
) -> anyhow::Result<()>
where
    R: Stream<Item = Result<client::Message, anyhow::Error>> + Unpin,
    W: Sink<server::Message, Error = anyhow::Error> + Unpin,
{
    let (heartbeat_sender, mut heartbeat_receiver) = mpsc::channel(16);
    let mut heartbeat_sender = Some(heartbeat_sender);

    loop {
        tokio::select! {
            Some(Ok(msg)) = reader.next() => {
                let action = client_action(msg, &mut heartbeat_sender);
                match action {
                    Action::Reply(r) => writer.send(r).await?,
                    Action::SpawnCamera(c) => {
                        Camera::run(c, reader, writer, plate_tx, heartbeat_sender, heartbeat_receiver).await?;
                        break;
                    }
                    Action::SpawnDispatcher(r) => {
                        let dispatcher = Dispatcher::new(r, dispatchers.clone());
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::camera::Camera;
    use tokio_test::io::Builder;

    #[tokio::test]
    async fn example() {
        let server_encoder = server::encoder::MessageEncoder::default();
        let client_decoder = client::decoder::MessageDecoder::default();

        let client_1 = Builder::new()
            .read(&[0x80, 0x00, 0x7b, 0x00, 0x08, 0x00, 0x3c])
            .read(&[0x20, 0x04, 0x55, 0x4e, 0x31, 0x58, 0x00, 0x00, 0x00, 0x00])
            .build();
        let mut client_1 = tokio_util::codec::FramedRead::new(
            client_1,
            client::decoder::MessageDecoder::default(),
        );

        let client_2 = Builder::new()
            .read(&[0x80, 0x00, 0x7b, 0x00, 0x09, 0x00, 0x3c])
            .read(&[0x20, 0x04, 0x55, 0x4e, 0x31, 0x58, 0x00, 0x00, 0x00, 0x2d])
            .build();
        let mut client_2 = tokio_util::codec::FramedRead::new(
            client_2,
            client::decoder::MessageDecoder::default(),
        );

        assert_eq!(
            client_1.next().await.unwrap().unwrap(),
            client::Message::IAmCamera(Camera {
                road: 123,
                mile: 8,
                limit: 60,
            })
        );
        assert_eq!(
            client_1.next().await.unwrap().unwrap(),
            client::Message::Plate(PlateRecord {
                plate: "UN1X".to_string(),
                timestamp: 0,
            })
        );
        assert_eq!(
            client_2.next().await.unwrap().unwrap(),
            client::Message::IAmCamera(Camera {
                road: 123,
                mile: 9,
                limit: 60,
            })
        );
        assert_eq!(
            client_2.next().await.unwrap().unwrap(),
            client::Message::Plate(PlateRecord {
                plate: "UN1X".to_string(),
                timestamp: 45,
            })
        );
    }

    #[ignore]
    #[allow(unused)]
    #[tokio::test]
    async fn example_2() {
        let dispatcher_message = client::Message::IAmDispatcher(vec![123]);
        let ticket = server::Message::Ticket(TicketRecord {
            plate: "UN1X".to_string(),
            road: 123,
            mile1: 8,
            timestamp1: 0,
            mile2: 9,
            timestamp2: 45,
            speed: 8000,
        });

        let dispatcher = Builder::new()
            .read(&[0x81, 0x01, 0x00, 0x7b])
            .write(&[
                0x21, 0x04, 0x55, 0x4e, 0x31, 0x58, 0x00, 0x7b, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x09, 0x00, 0x00, 0x00, 0x2d, 0x1f, 0x40,
            ])
            .build();
        todo!()
    }
}

#![feature(iter_array_chunks)]

use client::PlateRecord;
use futures::{Sink, SinkExt, Stream, StreamExt};
use server::TicketRecord;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use std::{collections::HashMap, sync::RwLock};
use tokio::{net::TcpListener, sync::mpsc};

mod client;
mod server;

pub type Dispatchers = HashMap<u16, Vec<mpsc::Sender<TicketRecord>>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8000".to_string());

    let listener = TcpListener::bind(listen_addr).await?;

    let (plate_tx, plate_rx) = mpsc::channel(256);
    let dispatchers = Arc::new(RwLock::new(Dispatchers::default()));

    while let Ok((inbound, addr)) = listener.accept().await {
        println!("Accepted connection from {addr}");
        let (reader, writer) = inbound.into_split();
        let reader = tokio_util::codec::FramedRead::new(reader, client::MessageDecoder::default());
        let writer = tokio_util::codec::FramedWrite::new(writer, server::MessageEncoder::default());
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
    plate_tx: mpsc::Sender<PlateRecord>,
    dispatchers: Arc<RwLock<Dispatchers>>,
) -> anyhow::Result<()>
where
    R: Stream<Item = Result<client::Message, anyhow::Error>> + Unpin,
    W: Sink<server::Message, Error = anyhow::Error> + Unpin,
{
    let mut heartbeat: Option<Duration> = None;
    while let Some(Ok(msg)) = reader.next().await {
        match msg {
            client::Message::Plate(record) => {
                println!("Ignoring {record:?} due to client not having specialized as camera");
                writer
                    .send(server::Message::Error("You are no camera".to_string()))
                    .await
                    .unwrap();
            }
            client::Message::WantHeartbeat(dur) => {
                if heartbeat.is_some() {
                    writer
                        .send(server::Message::Error(
                            "You already specified a heartbeat".to_string(),
                        ))
                        .await
                        .unwrap();
                } else {
                    heartbeat = Some(dur);
                    todo!("Heartbeats");
                }
            }
            client::Message::IAmCamera { road, mile, limit } => {
                return handle_camera(reader, writer, road, mile, limit, plate_tx).await;
            }
            client::Message::IAmDispatcher(roads) => {
                return handle_dispatcher(&roads, dispatchers).await;
            }
        }
    }
    Ok(())
}

async fn handle_camera<R, W>(
    mut reader: R,
    mut writer: W,
    road: u16,
    mile: u16,
    limit: u16,
    plate_tx: mpsc::Sender<PlateRecord>,
) -> anyhow::Result<()>
where
    R: Stream<Item = Result<client::Message, anyhow::Error>> + Unpin,
    W: Sink<server::Message, Error = anyhow::Error> + Unpin,
{
    while let Some(Ok(msg)) = reader.next().await {
        match msg {
            client::Message::Plate(record) => {
                plate_tx.send(record).await.unwrap();
            }
            client::Message::WantHeartbeat(_) => todo!(),
            client::Message::IAmCamera { .. } => {
                writer
                    .send(server::Message::Error(
                        "You are already a camera".to_string(),
                    ))
                    .await
                    .unwrap();
            }
            client::Message::IAmDispatcher(_roads) => {
                writer
                    .send(server::Message::Error("No you're not".to_string()))
                    .await
                    .unwrap();
            }
        }
    }
    Ok(())
}

async fn handle_dispatcher(
    roads: &[u16],
    dispatchers: Arc<RwLock<Dispatchers>>,
) -> anyhow::Result<()> {
    todo!()
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashSet;
    use tokio_stream::StreamMap;
    use tokio_test::io::Builder;

    #[tokio::test]
    async fn example() {
        let server_encoder = server::MessageEncoder::default();
        let client_decoder = client::MessageDecoder::default();

        let client_1 = Builder::new()
            .read(&[0x80, 0x00, 0x7b, 0x00, 0x08, 0x00, 0x3c])
            .read(&[0x20, 0x04, 0x55, 0x4e, 0x31, 0x58, 0x00, 0x00, 0x00, 0x00])
            .build();
        let client_1 =
            tokio_util::codec::FramedRead::new(client_1, client::MessageDecoder::default());

        let client_2 = Builder::new()
            .read(&[0x80, 0x00, 0x7b, 0x00, 0x09, 0x00, 0x3c])
            .read(&[0x20, 0x04, 0x55, 0x4e, 0x31, 0x58, 0x00, 0x00, 0x00, 0x2d])
            .build();
        let client_2 =
            tokio_util::codec::FramedRead::new(client_2, client::MessageDecoder::default());

        let mut map = StreamMap::new();
        map.insert(1, client_1);
        map.insert(2, client_2);

        let messages = [
            client::Message::IAmCamera {
                road: 123,
                mile: 8,
                limit: 60,
            },
            client::Message::Plate(PlateRecord {
                plate: "UN1X".to_string(),
                timestamp: 0,
            }),
            client::Message::IAmCamera {
                road: 123,
                mile: 9,
                limit: 60,
            },
            client::Message::Plate(PlateRecord {
                plate: "UN1X".to_string(),
                timestamp: 45,
            }),
        ]
        .into_iter()
        .collect::<HashSet<_>>();

        while let Some((i, Ok(msg))) = map.next().await {
            assert!(messages.contains(&msg))
        }
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

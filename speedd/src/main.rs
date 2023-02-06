#![feature(iter_array_chunks)]

use futures::{FutureExt, StreamExt};
use std::env;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::StreamMap;
use tokio_util::codec::{BytesCodec, FramedRead, FramedWrite};

mod client;
mod server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let legitimate_origin = "chat.protohackers.com:16963";

    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8000".to_string());

    let listener = TcpListener::bind(listen_addr).await?;

    let mut map = StreamMap::new();
    loop {
        tokio::select! {
            Ok((mut inbound, addr)) = listener.accept() => {
                println!("Accepted connection from {addr}");
                let codec = client::MessageDecoder::default();
                let (reader, writer) = inbound.into_split();
                let client = tokio_util::codec::FramedRead::new(reader, codec);
                map.insert(addr, client);
            },
            Some((addr, r)) = map.next() => {
                match r {
                    Ok(msg) => {
                        println!("got message {msg:?}");
                        dbg!(&map);
                        let result = handle_message(msg).await;
                    },
                    Err(e) => {
                        println!("removing client {addr}");
                        map.remove(&addr);
                    },
                }
            },
            else => break
        }
    }

    Ok(())
}

async fn handle_message(msg: client::Message) -> Option<server::Message> {
    None
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
            client::Message::Plate {
                plate: "UN1X".to_string(),
                timestamp: 0,
            },
            client::Message::IAmCamera {
                road: 123,
                mile: 9,
                limit: 60,
            },
            client::Message::Plate {
                plate: "UN1X".to_string(),
                timestamp: 45,
            },
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
        let ticket = server::Message::Ticket {
            plate: "UN1X".to_string(),
            road: 123,
            mile1: 8,
            timestamp1: 0,
            mile2: 9,
            timestamp2: 45,
            speed: 8000,
        };

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

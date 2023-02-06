use futures::StreamExt;
use std::env;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{BytesCodec, FramedRead, FramedWrite};

mod bogus;
mod message;
#[allow(unused)]
mod proxy;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let legitimate_origin = "chat.protohackers.com:16963";

    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8000".to_string());
    let server_addr = env::args()
        .nth(2)
        .unwrap_or_else(|| legitimate_origin.to_string());

    println!("Listening on: {listen_addr}!");
    println!("Proxying to: {server_addr}!");

    let listener = TcpListener::bind(listen_addr).await?;

    while let Ok((mut inbound, _)) = listener.accept().await {
        let server_addr = server_addr.clone();
        tokio::spawn(async move {
            let (reader, writer) = inbound.split();
            let codec = message::MessageDecoder::default();
            let reader = FramedRead::new(reader, codec);
            let mut writer = FramedWrite::new(writer, BytesCodec::new());
            let mut remote = TcpStream::connect(server_addr).await.unwrap();
            let (remote_reader, remote_writer) = remote.split();
            let mut remote_writer = FramedWrite::new(remote_writer, BytesCodec::new());
            let remote_reader = FramedRead::new(remote_reader, BytesCodec::new());
            let upstream = reader.forward(&mut remote_writer);
            let downstream = remote_reader.forward(&mut writer);
            dbg!(tokio::try_join!(upstream, downstream));
        });
    }

    Ok(())
}

use futures::{FutureExt, StreamExt};
use std::env;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{BytesCodec, FramedRead, FramedWrite};

mod bogus;
mod proxy;
mod request;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let legitimate_origin = "chat.protohackers.com:16963";

    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8000".to_string());
    let server_addr = env::args()
        .nth(2)
        .unwrap_or_else(|| legitimate_origin.to_string());

    println!("Listening on: {listen_addr}");
    println!("Proxying to: {server_addr}");

    let listener = TcpListener::bind(listen_addr).await?;

    while let Ok((mut inbound, _)) = listener.accept().await {
        tokio::spawn(async move {
            let (reader, writer) = inbound.split();
            let codec = request::RequestDecoder::default();
            let reader = FramedRead::new(reader, codec);
            let writer = FramedWrite::new(writer, BytesCodec::new());
            reader.forward(writer).await.unwrap();
        });
    }

    Ok(())
}

async fn transfer(mut inbound: TcpStream, proxy_addr: String) -> anyhow::Result<()> {
    let mut outbound = TcpStream::connect(proxy_addr).await?;

    let (mut ri, mut wi) = inbound.split();
    let (mut ro, mut wo) = outbound.split();

    let client_to_server = async {
        proxy::proxy(&mut ri, &mut wo).await?;
        wo.shutdown().await
    };

    let server_to_client = async {
        proxy::proxy(&mut ro, &mut wi).await?;
        wi.shutdown().await
    };

    tokio::try_join!(client_to_server, server_to_client)?;

    Ok(())
}

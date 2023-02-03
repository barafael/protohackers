use futures::{FutureExt, SinkExt, StreamExt};
use std::env;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

use bogus::TONYS_ADDRESS;

mod bogus;

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

    while let Ok((inbound, _)) = listener.accept().await {
        let transfer = transfer(inbound, server_addr.clone()).map(|r| {
            if let Err(e) = r {
                println!("Failed to transfer; error={e}");
            }
        });

        tokio::spawn(transfer);
    }

    Ok(())
}

async fn transfer(mut inbound: TcpStream, proxy_addr: String) -> anyhow::Result<()> {
    let mut outbound = TcpStream::connect(proxy_addr).await?;

    let (ri, wi) = inbound.split();
    let (ro, wo) = outbound.split();

    let codec = LinesCodec::new();

    let mut ri = FramedRead::new(ri, codec.clone());
    let mut wi = FramedWrite::new(wi, codec.clone());

    let mut ro = FramedRead::new(ro, codec.clone());
    let mut wo = FramedWrite::new(wo, codec);

    let client_to_server = async {
        loop {
            let msg = ri.next().await;
            if let Some(Ok(msg)) = msg {
                let replaced = bogus::RE
                    .replace_all(&msg, format!("{TONYS_ADDRESS}$2"))
                    .to_string();
                dbg!(&replaced);
                if let Err(_e) = wo.send(replaced).await {
                    break;
                }
            } else {
                break;
            }
        }
        wo.into_inner().shutdown().await
    };

    let server_to_client = async {
        loop {
            let msg = ro.next().await;
            if let Some(Ok(msg)) = msg {
                let replaced = bogus::RE
                    .replace_all(&msg, format!("{TONYS_ADDRESS}$2"))
                    .to_string();
                dbg!(&replaced);
                if let Err(_e) = wi.send(replaced).await {
                    break;
                }
            } else {
                break;
            }
        }
        wi.into_inner().shutdown().await
    };

    tokio::try_join!(client_to_server, server_to_client)?;

    Ok(())
}

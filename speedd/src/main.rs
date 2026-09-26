#![feature(iter_array_chunks)]

use crate::client::Client;
use crate::collector::Collector;
use speedd_codecs::client::decoder::MessageDecoder;
use speedd_codecs::server::encoder::MessageEncoder;
use std::env;
use tokio::{net::TcpListener, sync::mpsc};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod client;
mod collector;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        //.with_ansi(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8000".to_string());

    let listener = TcpListener::bind(listen_addr).await?;

    let (collector_tx, collector_rx) = mpsc::channel(256);

    // for termination when collecting pgo profiles
    //tokio::spawn(async move {
    //tokio::time::sleep(std::time::Duration::from_secs(100)).await;
    //std::process::exit(0);
    //});

    tokio::spawn(Collector::default().event_loop(collector_rx));

    while let Ok((inbound, addr)) = listener.accept().await {
        tracing::info!("Accepted connection from {addr}");
        let (reader, writer) = inbound.into_split();
        let reader = FramedRead::new(reader, MessageDecoder);
        let writer = FramedWrite::new(writer, MessageEncoder);
        tokio::spawn(Client::default().event_loop(reader, writer, collector_tx.clone()));
    }

    Ok(())
}

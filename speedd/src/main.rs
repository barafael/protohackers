#![feature(iter_array_chunks)]

use crate::client::Client;
use crate::collector::Collector;
use anyhow::Context;
use speedd_codecs::client::decoder::MessageDecoder;
use speedd_codecs::server::encoder::MessageEncoder;
use std::{env, net::SocketAddr};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
    task::JoinSet,
};
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

    let mut collector = tokio::spawn(Collector::default().event_loop(collector_rx));
    let mut clients = JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (inbound, addr) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        tracing::error!(?error, "Failed to accept");
                        break;
                    }
                };
                tracing::info!("Accepted connection from {addr}");
                clients.spawn(serve(inbound, addr, collector_tx.clone()));
            }
            Some(finished) = clients.join_next() => match finished {
                Ok((addr, client)) => tracing::info!(%addr, ?client, "Client left"),
                Err(error) => tracing::error!(?error, "Client task failed"),
            },
            finished = &mut collector => {
                // This loop holds a sender, so the collector only stops if it panics.
                let collector = finished.context("Collector failed")?;
                anyhow::bail!("Collector stopped with {collector:?}");
            }
        }
    }

    Ok(())
}

async fn serve(
    inbound: TcpStream,
    addr: SocketAddr,
    collector: mpsc::Sender<collector::Message>,
) -> (SocketAddr, Client) {
    let (reader, writer) = inbound.into_split();
    let reader = FramedRead::new(reader, MessageDecoder);
    let writer = FramedWrite::new(writer, MessageEncoder);
    let client = Client::default()
        .event_loop(reader, writer, collector)
        .await;
    (addr, client)
}

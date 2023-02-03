use crate::message::Message;
use db::Store;
use std::str::FromStr;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

mod db;
mod message;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut store = Store::default();
    store
        .0
        .insert("version".to_string(), env!("CARGO_PKG_NAME").to_string());
    let socket = UdpSocket::bind("0.0.0.0:8000".parse::<SocketAddr>().unwrap()).await?;
    let receiver = Arc::new(socket);
    let sender = receiver.clone();
    let (tx, mut rx) = mpsc::channel::<(Vec<u8>, SocketAddr)>(1_000);

    tokio::spawn(async move {
        while let Some((bytes, addr)) = rx.recv().await {
            let len = sender.send_to(&bytes, &addr).await.unwrap();
            println!("{len:?} bytes sent");
        }
    });

    let mut buf = [0; 1024];
    loop {
        let (len, addr) = receiver.recv_from(&mut buf).await?;
        let data = String::from_utf8(buf[..len].to_vec())?;
        match Message::from_str(&data) {
            Ok(m) => match m {
                Message::Insert { key, value } => {
                    if key != "version" {
                        println!("Insert value: {key} -> {value}");
                        store.0.insert(key, value);
                    }
                }
                Message::Query(key) => {
                    let value = store.0.get(&key);
                    println!("Retrieved value: {value:#?}");
                    if let Some(v) = value {
                        let response = format!("{key}={v}");
                        tx.send((response.as_bytes().to_vec(), addr)).await?;
                    }
                }
            },
            Err(e) => {
                println!("Failed to parse message: {e:#?}");
            }
        }
    }
}

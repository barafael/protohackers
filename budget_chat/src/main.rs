use anyhow::Context;
use futures::{stream::StreamExt, Sink, SinkExt, Stream};
use itertools::Itertools;
use std::{
    collections::HashMap,
    net::SocketAddr,
    ops::ControlFlow,
    sync::{Arc, Mutex},
};
use tokio::{net::TcpListener, sync::broadcast};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec, LinesCodecError};

#[derive(Clone, Debug, Default)]
struct Users(Arc<Mutex<HashMap<SocketAddr, String>>>);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8000").await?;
    let (tx, _) = broadcast::channel(256);
    let users = Users::default();
    loop {
        let (mut stream, addr) = listener.accept().await?;
        let codec = LinesCodec::default();
        let (tx, rx) = (tx.clone(), tx.subscribe());
        let users = users.clone();
        tokio::spawn(async move {
            let (reader, writer) = stream.split();
            let reader = FramedRead::new(reader, codec.clone());
            let writer = FramedWrite::new(writer, codec.clone());
            handle_connection(reader, writer, tx, rx, addr, users).await
        });
    }
}

async fn handle_connection<R, W>(
    mut reader: R,
    mut writer: W,
    tx: broadcast::Sender<String>,
    rx: broadcast::Receiver<String>,
    addr: SocketAddr,
    users: Users,
) -> anyhow::Result<()>
where
    R: Stream<Item = Result<String, LinesCodecError>> + Unpin,
    W: Sink<String, Error = LinesCodecError> + Unpin,
{
    writer
        .send("Welcome to budgetchat! What shall I call you?".to_string())
        .await?;
    let name = reader
        .next()
        .await
        .context("Connection closed while awaiting name")?
        .context("Failed to receive name")?;
    anyhow::ensure!(name.len() >= 1);
    anyhow::ensure!(name.chars().all(|c| c.is_ascii_alphanumeric()));

    users.0.lock().unwrap().insert(addr, name.clone());

    let msg = format!(
        "* The room contains: {}",
        users
            .0
            .lock()
            .unwrap()
            .values()
            .filter(|n| n.as_str() != name)
            .join(", ")
    );
    println!("{msg}");
    writer.send(msg).await?;

    let msg = format!("* {name} has joined the room");
    println!("{msg}");
    tx.send(msg)?;
    let mut rx = rx.resubscribe();

    loop {
        tokio::select! {
            msg = reader.next() => {
                let flow = handle_client_message(&name, &addr, msg, &tx, &users)?;
                if flow.is_break() {
                    break;
                }
            }
            Ok(msg) = rx.recv() => {
                println!("Received {msg}");
                if !msg.starts_with(&format!("[{name}]")) {
                    writer.send(msg).await?;
                }
            }
        }
    }
    Ok(())
}

fn handle_client_message(
    name: &str,
    addr: &SocketAddr,
    msg: Option<Result<String, LinesCodecError>>,
    tx: &broadcast::Sender<String>,
    users: &Users,
) -> anyhow::Result<ControlFlow<()>> {
    match msg {
        Some(Ok(msg)) => {
            let msg = format!("[{name}] {msg}");
            println!("{msg}");
            tx.send(msg)?;
            Ok(ControlFlow::Continue(()))
        }
        Some(Err(e)) => {
            let msg = format!("* {name} has left the room, with error {e:#?}");
            println!("{msg}");
            tx.send(msg)?;
            users.0.lock().unwrap().remove(addr);
            Ok(ControlFlow::Break(()))
        }
        None => {
            let msg = format!("* {name} has left the room");
            println!("{msg}");
            tx.send(msg)?;
            users.0.lock().unwrap().remove(addr);
            Ok(ControlFlow::Break(()))
        }
    }
}

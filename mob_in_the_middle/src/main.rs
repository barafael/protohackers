use std::{
    env,
    net::{SocketAddr, ToSocketAddrs},
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter},
    net::{TcpListener, TcpStream},
};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

const LEGITIMATE_ORIGIN: &str = "chat.protohackers.com:16963";

const TONYS_ADDRESS: &str = "7YWHMfk9JZe0LM0g1ZauHuiSxhI";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .with_ansi(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8000".to_string());
    let server_addr: SocketAddr = env::args()
        .nth(2)
        .and_then(|s| s.to_socket_addrs().ok())
        .and_then(|mut s| s.next())
        .unwrap_or_else(|| LEGITIMATE_ORIGIN.to_socket_addrs().unwrap().next().unwrap());

    tracing::info!("Listening on: {listen_addr}!");
    tracing::info!("Proxying to: {server_addr}!");

    let listener = TcpListener::bind(listen_addr).await?;

    while let Ok((mut client, addr)) = listener.accept().await {
        tracing::info!("Accepted client from {}", addr);

        tokio::spawn(async move {
            let (client_reader, client_writer) = client.split();
            let (client_reader, client_writer) =
                (BufReader::new(client_reader), BufWriter::new(client_writer));

            let mut upstream = TcpStream::connect(server_addr).await.unwrap();
            let (upstream_reader, upstream_writer) = upstream.split();
            let (upstream_reader, upstream_writer) = (
                BufReader::new(upstream_reader),
                BufWriter::new(upstream_writer),
            );

            let client_to_upstream = forward(client_reader, upstream_writer);

            let upstream_to_client = forward(upstream_reader, client_writer);

            tokio::select! {
                err = client_to_upstream => {
                    tracing::error!("Client to upstream: {:?}", err);
                },
                err = upstream_to_client => {
                    tracing::error!("Upstream to client: {:?}", err);
                },
            }
            tracing::info!("Disconnected");
        });
    }
    Ok(())
}

async fn forward<R, W>(mut reader: R, mut writer: W) -> Result<(), anyhow::Error>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut line = String::with_capacity(1024);
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            tracing::warn!("EOF");
            break;
        }
        if !line.ends_with('\n') {
            tracing::warn!("Disconnected without sending newline");
            break;
        }
        let rewritten = replace(&line);
        writer
            .write_all(format!("{rewritten}\n").as_bytes())
            .await?;
        writer.flush().await?;
    }
    Err(anyhow::anyhow!("Connection closed"))
}

fn replace(line: &str) -> String {
    let mut words: Vec<&str> = line.split_ascii_whitespace().collect();

    for word in &mut words {
        if word.starts_with('7')
            && word.len() >= 26
            && word.len() <= 35
            && word.chars().all(|ch| ch.is_ascii_alphanumeric())
        {
            *word = TONYS_ADDRESS;
        }
    }
    words.join(" ")
}

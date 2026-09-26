use std::{io, net::SocketAddr};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8000").await?;
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, addr) = accepted?;
                connections.spawn(serve(stream, addr));
            }
            Some(finished) = connections.join_next() => match finished {
                Ok((addr, Ok(bytes))) => println!("Echoed {bytes} bytes to {addr}"),
                Ok((addr, Err(error))) => eprintln!("Connection {addr} failed: {error}"),
                Err(error) => eprintln!("Connection task failed: {error}"),
            },
        }
    }
}

async fn serve(mut stream: TcpStream, addr: SocketAddr) -> (SocketAddr, io::Result<u64>) {
    let (mut reader, mut writer) = stream.split();
    (addr, tokio::io::copy(&mut reader, &mut writer).await)
}

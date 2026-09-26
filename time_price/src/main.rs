use db::Db;
use request::RequestDecoder;
use response::ResponseEncoder;
use std::net::SocketAddr;
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinSet,
};
use tokio_util::codec::{FramedRead, FramedWrite};

mod db;
mod mean;
mod request;
mod response;

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
                Ok((addr, db)) => println!("{addr} left with {} prices", db.count()),
                Err(error) => eprintln!("Connection task failed: {error}"),
            },
        }
    }
}

async fn serve(mut stream: TcpStream, addr: SocketAddr) -> (SocketAddr, Db) {
    let (reader, writer) = stream.split();
    let reader = FramedRead::new(reader, RequestDecoder);
    let writer = FramedWrite::new(writer, ResponseEncoder);
    (addr, Db::default().event_loop(reader, writer).await)
}

use db::Db;
use request::RequestDecoder;
use response::ResponseEncoder;
use tokio::net::TcpListener;
use tokio_util::codec::{FramedRead, FramedWrite};

mod db;
mod mean;
mod request;
mod response;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8000").await?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            let (reader, writer) = stream.split();
            let reader = FramedRead::new(reader, RequestDecoder);
            let writer = FramedWrite::new(writer, ResponseEncoder);
            Db::default().event_loop(reader, writer).await
        });
    }
}

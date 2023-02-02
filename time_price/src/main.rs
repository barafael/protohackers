use db::Db;
use futures::{stream::StreamExt, Sink, SinkExt, Stream};
use request::{Request, RequestDecoder};
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
            handle_connection(reader, writer).await
        });
    }
}

async fn handle_connection<R, W>(mut reader: R, mut writer: W) -> anyhow::Result<()>
where
    R: Stream<Item = Result<Request, anyhow::Error>> + Unpin,
    W: Sink<i32, Error = anyhow::Error> + Unpin,
{
    let mut db = Db::default();
    while let Some(Ok(request)) = reader.next().await {
        match request {
            Request::Insert { time, price } => {
                //println!("Inserting {time}: {price}");
                db.insert(time, price);
            }
            Request::Query { min, max } => {
                let avg = db.query(min, max);
                //println!("Queried {min}..{max}: {avg}");
                writer.send(avg).await?;
            }
        }
    }
    Ok(())
}

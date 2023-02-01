use serde::{Deserialize, Serialize};
use serde_json::Number;
use serde_jsonlines::{AsyncJsonLinesReader, AsyncJsonLinesWriter};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::net::TcpListener;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub number: Number,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Response {
    method: String,
    prime: bool,
}

impl Response {
    pub fn wellformed(prime: bool) -> Self {
        Self {
            method: "isPrime".to_string(),
            prime,
        }
    }

    pub fn malformed(method: &str) -> Self {
        Self {
            method: method.to_string(),
            prime: false,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8000").await?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            let (reader, writer) = stream.split();
            handle_connection(reader, writer).await
        });
    }
}

async fn handle_connection<R, W>(reader: R, writer: W) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = AsyncJsonLinesReader::new(BufReader::new(reader));
    let mut writer = AsyncJsonLinesWriter::new(writer);

    loop {
        let request = reader.read::<Request>().await;
        match request {
            Ok(Some(item)) => {
                if item.method != "isPrime" {
                    let response = Response::malformed("Invalid method");
                    writer.write(&response).await?;
                    break;
                }
                if let Some(number) = item.number.as_u64() {
                    let prime = primal::is_prime(number);
                    let response = Response::wellformed(prime);
                    writer.write(&response).await?;
                } else if let Some(number) = item.number.as_i64() {
                    let prime = if number < 0 {
                        false
                    } else {
                        primal::is_prime(number.unsigned_abs())
                    };
                    let response = Response::wellformed(prime);
                    writer.write(&response).await?;
                } else {
                    let response = Response::wellformed(false);
                    writer.write(&response).await?;
                    break;
                };
            }
            Ok(None) => {
                let response = Response::malformed("None");
                writer.write(&response).await?;
                break;
            }
            Err(e) => {
                let response = Response::malformed(&format!("{:#?}", e.kind()));
                writer.write(&response).await?;
                break;
            }
        }
    }
    Ok(())
}

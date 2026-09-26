use serde::{Deserialize, Serialize};
use serde_json::Number;
use serde_jsonlines::{AsyncJsonLinesReader, AsyncJsonLinesWriter};
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

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
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, addr) = accepted?;
                connections.spawn(serve(stream, addr));
            }
            Some(finished) = connections.join_next() => match finished {
                Ok((_, Ok(()))) => {}
                Ok((addr, Err(error))) => eprintln!("Connection {addr} failed: {error:#}"),
                Err(error) => eprintln!("Connection task failed: {error}"),
            },
        }
    }
}

async fn serve(mut stream: TcpStream, addr: SocketAddr) -> (SocketAddr, anyhow::Result<()>) {
    let (reader, writer) = stream.split();
    (addr, handle_connection(reader, writer).await)
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
                    // Not an integer, so not prime, but still well-formed.
                    let response = Response::wellformed(false);
                    writer.write(&response).await?;
                };
            }
            // The client hung up: nothing to answer.
            Ok(None) => break,
            Err(e) => {
                let response = Response::malformed(&format!("{:#?}", e.kind()));
                writer.write(&response).await?;
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    async fn session(input: &str) -> Vec<serde_json::Value> {
        let mut output = Vec::new();
        handle_connection(input.as_bytes(), &mut output)
            .await
            .unwrap();
        output
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect()
    }

    fn prime(prime: bool) -> serde_json::Value {
        serde_json::json!({"method": "isPrime", "prime": prime})
    }

    #[tokio::test]
    async fn answers_until_malformed_request() {
        let responses = session(concat!(
            r#"{"method":"isPrime","number":7}"#,
            "\n",
            r#"{"method":"isPrime","number":8,"extra":"ignored"}"#,
            "\n",
            r#"{"method":"isPrime","number":-7}"#,
            "\n",
            r#"{"method":"isComposite","number":7}"#,
            "\n",
            r#"{"method":"isPrime","number":7}"#,
            "\n",
        ))
        .await;

        assert_eq!(
            responses,
            [
                prime(true),
                prime(false),
                prime(false),
                serde_json::json!({"method": "Invalid method", "prime": false}),
            ]
        );
    }

    #[tokio::test]
    async fn stops_silently_at_end_of_input() {
        let responses = session(concat!(r#"{"method":"isPrime","number":7}"#, "\n")).await;
        assert_eq!(responses, [prime(true)]);

        assert!(session("").await.is_empty());
    }

    #[tokio::test]
    async fn non_integers_are_not_prime() {
        let responses = session(concat!(
            r#"{"method":"isPrime","number":3.5}"#,
            "\n",
            r#"{"method":"isPrime","number":7}"#,
            "\n",
        ))
        .await;

        // Well-formed, so the session goes on.
        assert_eq!(responses, [prime(false), prime(true)]);
    }

    #[tokio::test]
    async fn garbage_gets_one_malformed_response() {
        let responses =
            session("{\"method\":\"isPrime\"}\n{\"method\":\"isPrime\",\"number\":7}\n").await;

        assert_eq!(responses.len(), 1);
        assert_ne!(responses[0]["method"], "isPrime");
    }
}

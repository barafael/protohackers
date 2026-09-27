use std::{
    env,
    future::Future,
    net::{SocketAddr, ToSocketAddrs},
};
use tokio::{
    io::{
        self, AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader,
        BufWriter,
    },
    net::{TcpListener, TcpStream},
    task::JoinSet,
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

    let mut sessions = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (client, addr) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        tracing::error!(?error, "Failed to accept");
                        break;
                    }
                };
                tracing::info!("Accepted client from {}", addr);
                sessions.spawn(proxy(client, addr, TcpStream::connect(server_addr)));
            }
            Some(finished) = sessions.join_next() => match finished {
                Ok(addr) => tracing::info!("Disconnected {addr}"),
                Err(error) => tracing::error!(?error, "Proxy task failed"),
            },
        }
    }
    Ok(())
}

/// Relay one client's chat through its own upstream connection, until either side hangs up.
async fn proxy<C, U>(
    client: C,
    addr: SocketAddr,
    upstream: impl Future<Output = io::Result<U>>,
) -> SocketAddr
where
    C: AsyncRead + AsyncWrite,
    U: AsyncRead + AsyncWrite,
{
    let upstream = match upstream.await {
        Ok(upstream) => upstream,
        Err(error) => {
            // Dropping the client closes its connection.
            tracing::error!(?error, "Cannot connect upstream, closing client connection");
            return addr;
        }
    };
    let (client_reader, client_writer) = io::split(client);
    let (upstream_reader, upstream_writer) = io::split(upstream);

    let client_to_upstream = forward(
        BufReader::new(client_reader),
        BufWriter::new(upstream_writer),
    );
    let upstream_to_client = forward(
        BufReader::new(upstream_reader),
        BufWriter::new(client_writer),
    );

    tokio::select! {
        err = client_to_upstream => {
            tracing::error!("Client to upstream: {:?}", err);
        },
        err = upstream_to_client => {
            tracing::error!("Upstream to client: {:?}", err);
        },
    }
    addr
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
        let Some(message) = line.strip_suffix('\n') else {
            tracing::warn!("Disconnected without sending newline");
            break;
        };
        let rewritten = replace(message);
        writer
            .write_all(format!("{rewritten}\n").as_bytes())
            .await?;
        writer.flush().await?;
    }
    Err(anyhow::anyhow!("Connection closed"))
}

/// Replace the Boguscoin addresses in a chat message (without its newline), and nothing else.
///
/// An address is delimited by spaces or the ends of the message.
fn replace(message: &str) -> String {
    message
        .split(' ')
        .map(|word| {
            if is_boguscoin_address(word) {
                TONYS_ADDRESS
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_boguscoin_address(word: &str) -> bool {
    word.starts_with('7')
        && (26..=35).contains(&word.len())
        && word.chars().all(|ch| ch.is_ascii_alphanumeric())
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[test]
    fn rewrites_boguscoin_addresses() {
        assert_eq!(
            replace("Hi alice, please send payment to 7iKDZEwPZSqIvDnHvVN2r0hUWXD5rHX"),
            format!("Hi alice, please send payment to {TONYS_ADDRESS}")
        );
        assert_eq!(
            replace("7F1u3wSD5RbOHQmupo9nx4TnhQ or 7LOrwbDlS8NujgjddyogWgIM93MV5N2VR"),
            format!("{TONYS_ADDRESS} or {TONYS_ADDRESS}")
        );
    }

    #[test]
    fn leaves_other_words_alone() {
        for line in [
            "too short: 7F1u3wSD5RbOHQmupo9nx4Tnh",
            "too long: 7LOrwbDlS8NujgjddyogWgIM93MV5N2VRxyz",
            "not a 7: 8F1u3wSD5RbOHQmupo9nx4TnhQ",
            "not alphanumeric: 7F1u3wSD5RbOHQmupo9nx4TnhQ-1234",
        ] {
            assert_eq!(replace(line), line);
        }
    }

    #[test]
    fn leaves_whitespace_alone() {
        assert_eq!(
            replace("  two  spaces 7F1u3wSD5RbOHQmupo9nx4TnhQ  and\ta tab "),
            format!("  two  spaces {TONYS_ADDRESS}  and\ta tab ")
        );
        // Only spaces delimit an address.
        for line in [
            "tab\t7F1u3wSD5RbOHQmupo9nx4TnhQ",
            "7F1u3wSD5RbOHQmupo9nx4TnhQ\ttab",
        ] {
            assert_eq!(replace(line), line);
        }
    }

    fn addr() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 1))
    }

    #[tokio::test]
    async fn relays_both_ways() {
        let (client, proxied_client) = io::duplex(1024);
        let (proxied_upstream, upstream) = io::duplex(1024);
        let connected = std::future::ready(Ok(proxied_upstream));

        let conversation = async move {
            let (mut client, mut upstream) = (BufReader::new(client), BufReader::new(upstream));
            let mut from_upstream = String::new();
            let mut from_client = String::new();

            upstream
                .write_all(b"[bob] Send to 7F1u3wSD5RbOHQmupo9nx4TnhQ\n")
                .await
                .unwrap();
            client.read_line(&mut from_upstream).await.unwrap();
            client
                .write_all(b"Send to  7iKDZEwPZSqIvDnHvVN2r0hUWXD5rHX, not bob\n")
                .await
                .unwrap();
            upstream.read_line(&mut from_client).await.unwrap();
            // Hanging up ends the proxy.
            (from_upstream, from_client)
        };
        let (proxied, (from_upstream, from_client)) =
            tokio::join!(proxy(proxied_client, addr(), connected), conversation);

        assert_eq!(proxied, addr());

        assert_eq!(from_upstream, format!("[bob] Send to {TONYS_ADDRESS}\n"));
        // Followed by a comma, so not an address.
        assert_eq!(
            from_client,
            "Send to  7iKDZEwPZSqIvDnHvVN2r0hUWXD5rHX, not bob\n"
        );
    }

    #[tokio::test]
    async fn closes_client_when_upstream_is_unreachable() {
        let (mut client, proxied_client) = io::duplex(64);
        let unreachable = std::future::ready(Err::<io::DuplexStream, _>(io::Error::from(
            io::ErrorKind::ConnectionRefused,
        )));

        assert_eq!(proxy(proxied_client, addr(), unreachable).await, addr());

        let mut received = Vec::new();
        client.read_to_end(&mut received).await.unwrap();
        assert!(received.is_empty());
    }

    #[tokio::test]
    async fn forwards_complete_lines_only() {
        let input: &[u8] = b"Hi, send to 7F1u3wSD5RbOHQmupo9nx4TnhQ\nnot a complete line";
        let mut output = Vec::new();

        let result = forward(input, &mut output).await;

        assert!(result.is_err());
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("Hi, send to {TONYS_ADDRESS}\n")
        );
    }
}

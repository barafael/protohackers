use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LinesCodec};

/// The application: reverses each line, until the stream ends.
pub async fn reverse_lines<S>(stream: S) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut framed = Framed::new(stream, LinesCodec::default());
    loop {
        match framed.next().await {
            Some(Ok(msg)) => {
                tracing::trace!("Got some data {msg}, reversing it");
                let reversed = msg.chars().rev().collect::<String>();
                tracing::trace!("Reversed: {reversed:?}");
                framed.send(reversed).await?;
            }
            Some(Err(e)) => {
                tracing::warn!("{e:?}");
            }
            _ => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn reverses_lines() {
        let (mut peer, stream) = duplex(64);
        peer.write_all(b"hello\nworld\n").await.unwrap();
        peer.shutdown().await.unwrap();

        reverse_lines(stream).await.unwrap();

        let mut reversed = String::new();
        peer.read_to_string(&mut reversed).await.unwrap();
        assert_eq!(reversed, "olleh\ndlrow\n");
    }
}

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Framed, LinesCodec};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reverse;

impl Reverse {
    pub fn new() -> Self {
        Self
    }

    pub async fn run<S>(self, stream: S) -> anyhow::Result<()>
    where
        S: AsyncRead,
        S: AsyncWrite,
        S: Unpin,
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
}

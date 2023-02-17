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
                    let reversed = msg.chars().rev().collect::<String>();
                    framed.send(reversed).await?;
                }
                Some(Err(e)) => {
                    println!("{e:?}");
                }
                _ => break,
            }
        }
        Ok(())
    }
}

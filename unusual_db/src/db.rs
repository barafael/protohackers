use crate::message::Message;
use futures::{Sink, SinkExt, Stream, StreamExt};
use std::{collections::HashMap, fmt::Debug, net::SocketAddr};

/// The key-value store. It *is* its data; requests and responses are fed to its event loop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Store(pub(crate) HashMap<String, String>);

impl Store {
    /// A store which knows its version.
    pub fn new() -> Self {
        Self(HashMap::from([(
            "version".to_string(),
            env!("CARGO_PKG_NAME").to_string(),
        )]))
    }

    pub async fn event_loop<R, W>(mut self, mut requests: R, mut responses: W) -> Self
    where
        R: Stream<Item = (Message, SocketAddr)> + Unpin,
        W: Sink<(String, SocketAddr)> + Unpin,
        W::Error: Debug,
    {
        while let Some((message, addr)) = requests.next().await {
            if let Some(response) = self.on_message(message) {
                if let Err(error) = responses.send((response, addr)).await {
                    println!("Failed to send response: {error:#?}");
                    break;
                }
            }
        }
        self
    }

    fn on_message(&mut self, message: Message) -> Option<String> {
        match message {
            Message::Insert { key, value } => {
                if key != "version" {
                    println!("Insert value: {key} -> {value}");
                    self.0.insert(key, value);
                }
                None
            }
            Message::Query(key) => {
                let value = self.0.get(&key);
                println!("Retrieved value: {value:#?}");
                value.map(|v| format!("{key}={v}"))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::stream;
    use std::str::FromStr;

    fn from(port: u16, request: &str) -> (Message, SocketAddr) {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        (Message::from_str(request).unwrap(), addr)
    }

    fn to(port: u16, response: &str) -> (String, SocketAddr) {
        (
            response.to_string(),
            SocketAddr::from(([127, 0, 0, 1], port)),
        )
    }

    #[tokio::test]
    async fn insert_and_query() {
        let requests = stream::iter([
            from(1, "foo=bar"),
            from(2, "foo"),
            from(2, "missing"),
            from(1, "foo=baz"),
            from(3, "foo"),
            from(1, "=empty key"),
            from(1, ""),
        ]);
        let mut responses = Vec::new();

        let store = Store::new().event_loop(requests, &mut responses).await;

        assert_eq!(
            responses,
            [to(2, "foo=bar"), to(3, "foo=baz"), to(1, "=empty key")]
        );
        assert_eq!(store.0["foo"], "baz");
    }

    #[tokio::test]
    async fn version_is_read_only() {
        let requests = stream::iter([from(1, "version=hacked"), from(1, "version")]);
        let mut responses = Vec::new();

        let store = Store::new().event_loop(requests, &mut responses).await;

        assert_eq!(responses, [to(1, "version=unusual_db")]);
        assert_eq!(store, Store::new());
    }
}

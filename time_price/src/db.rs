use crate::{mean::Mean, request::Request};
use futures::{Sink, SinkExt, Stream, StreamExt};
use std::{collections::BTreeMap, fmt::Debug};

/// The prices one client has told us about. It *is* its data; the connection is fed to its event loop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Db(BTreeMap<i32, i32>);

impl Db {
    pub async fn event_loop<R, W>(mut self, mut requests: R, mut responses: W) -> Self
    where
        R: Stream<Item = Result<Request, anyhow::Error>> + Unpin,
        W: Sink<i32> + Unpin,
        W::Error: Debug,
    {
        while let Some(Ok(request)) = requests.next().await {
            match request {
                Request::Insert { time, price } => self.insert(time, price),
                Request::Query { min, max } => {
                    if let Err(error) = responses.send(self.query(min, max)).await {
                        eprintln!("Failed to respond: {error:?}");
                        break;
                    }
                }
            }
        }
        self
    }

    pub fn insert(&mut self, time: i32, price: i32) {
        self.0.insert(time, price);
    }

    pub fn query(&self, min: i32, max: i32) -> i32 {
        let interval = min..=max;
        self.0
            .iter()
            .filter_map(|(k, v)| if interval.contains(k) { Some(v) } else { None })
            .mean() as i32
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{request::RequestDecoder, response::ResponseEncoder};
    use futures::stream;
    use tokio_util::codec::{FramedRead, FramedWrite};

    #[tokio::test]
    async fn example_session() {
        let requests = stream::iter([
            Ok(Request::Insert {
                time: 12345,
                price: 101,
            }),
            Ok(Request::Insert {
                time: 12346,
                price: 102,
            }),
            Ok(Request::Insert {
                time: 12347,
                price: 100,
            }),
            Ok(Request::Insert {
                time: 40960,
                price: 5,
            }),
            Ok(Request::Query {
                min: 12288,
                max: 16384,
            }),
        ]);
        let mut responses = Vec::new();

        let db = Db::default().event_loop(requests, &mut responses).await;

        assert_eq!(responses, [101]);
        assert_eq!(db.0.len(), 4);
    }

    #[tokio::test]
    async fn example_session_on_the_wire() {
        let input: &[u8] = &[
            0x49, 0x00, 0x00, 0x30, 0x39, 0x00, 0x00, 0x00, 0x65, // I 12345 101
            0x49, 0x00, 0x00, 0x30, 0x3a, 0x00, 0x00, 0x00, 0x66, // I 12346 102
            0x49, 0x00, 0x00, 0x30, 0x3b, 0x00, 0x00, 0x00, 0x64, // I 12347 100
            0x49, 0x00, 0x00, 0xa0, 0x00, 0x00, 0x00, 0x00, 0x05, // I 40960 5
            0x51, 0x00, 0x00, 0x30, 0x00, 0x00, 0x00, 0x40, 0x00, // Q 12288 16384
        ];
        let mut output = Vec::new();

        Db::default()
            .event_loop(
                FramedRead::new(input, RequestDecoder),
                FramedWrite::new(&mut output, ResponseEncoder),
            )
            .await;

        assert_eq!(output, [0x00, 0x00, 0x00, 0x65]);
    }

    #[test]
    fn insert_and_query() {
        let mut db = Db::default();
        db.insert(1, 2);
        db.insert(2, 3);
        db.insert(0, 4);
        assert_eq!(3, { db.query(0, 4) });
        assert_eq!(0, { db.query(10, 9) });
    }
}

use bytes::{Buf, BytesMut};
use tokio_util::codec::Decoder;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Insert { time: i32, price: i32 },
    Query { min: i32, max: i32 },
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestDecoder;

impl Decoder for RequestDecoder {
    type Item = Request;

    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 9 {
            src.reserve(9 - src.len());
            return Ok(None);
        }
        let marker = src.get_u8();
        if marker == b'I' {
            let time = src.get_i32();
            let price = src.get_i32();
            let request = Request::Insert { time, price };
            Ok(Some(request))
        } else if marker == b'Q' {
            let min = src.get_i32();
            let max = src.get_i32();
            let request = Request::Query { min, max };
            Ok(Some(request))
        } else {
            anyhow::bail!("Invalid marker byte")
        }
    }
}

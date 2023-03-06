use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::Decoder;

use crate::bogus;

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub enum MessageDecoder {
    #[default]
    Forwarding,
    Buffering(BytesMut),
}

impl Decoder for MessageDecoder {
    type Item = Bytes;

    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut response = BytesMut::new();
        while src.has_remaining() {
            let ch = src.get_u8();
            tracing::trace!("nom {}", ch as char);
            match self {
                Self::Forwarding => {
                    if ch == b'7' {
                        tracing::trace!("found a seven, flushing");
                        let mut bytes = BytesMut::new();
                        bytes.put_u8(ch);
                        *self = Self::Buffering(bytes);
                        let result = Ok(Some(response.clone().freeze()));
                        response.clear();
                        return result;
                    }
                    tracing::trace!("appending byte {}", ch as char);
                    response.put_u8(ch);
                    continue;
                }
                Self::Buffering(bytes) => match (ch, bytes.len()) {
                    (b' ' | b'\n', 0..=25) => {
                        bytes.put_u8(ch);
                        tracing::trace!("too short {bytes:?}");
                        response = bytes.clone();
                        *self = Self::Forwarding;
                        continue;
                    }
                    (b' ' | b'\n', 26..) => {
                        tracing::trace!("ending it! {bytes:?}");
                        *bytes = bogus::TONYS_ADDRESS.as_bytes().iter().collect::<BytesMut>();
                        bytes.put_u8(ch);
                        response = bytes.clone();
                        *self = Self::Forwarding;
                        continue;
                    }
                    (c, 0..=35) => {
                        bytes.put_u8(c);
                        tracing::trace!("stuffed it: {bytes:?}");
                        continue;
                    }
                    (c, 36..) => {
                        bytes.put_u8(c);
                        response.put_slice(bytes);
                        *self = Self::Forwarding;
                        continue;
                    }
                    (c, l) => unreachable!("char: {} len: {l}", c as char),
                },
            }
        }
        if response.is_empty() {
            tracing::trace!("sending nothing");
            Ok(None)
        } else {
            Ok(Some(response.freeze()))
        }
    }
}

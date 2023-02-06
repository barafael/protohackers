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
            println!("nom {ch}");
            match self {
                MessageDecoder::Forwarding => {
                    if ch != b'7' {
                        println!("appending byte {ch:?}");
                        response.put_u8(ch);
                        continue;
                    } else {
                        println!("found a seven, flushing");
                        let mut bytes = BytesMut::new();
                        bytes.put_u8(ch);
                        *self = MessageDecoder::Buffering(bytes);
                        let result = Ok(Some(response.clone().freeze()));
                        response.clear();
                        return result;
                    }
                }
                MessageDecoder::Buffering(bytes) => match (ch, bytes.len()) {
                    (b' ' | b'\n', 0..=25) => {
                        bytes.put_u8(ch);
                        println!("too short {bytes:?}");
                        response = bytes.clone();
                        *self = MessageDecoder::Forwarding;
                        continue;
                    }
                    (b' ' | b'\n', 26..) => {
                        println!("ending it! {bytes:?}");
                        *bytes = BytesMut::from_iter(bogus::TONYS_ADDRESS.as_bytes().iter());
                        bytes.put_u8(ch);
                        response = bytes.clone();
                        *self = MessageDecoder::Forwarding;
                        continue;
                    }
                    (c, 0..=35) => {
                        bytes.put_u8(c);
                        println!("stuffed it: {bytes:?}");
                        continue;
                    }
                    (c, 36..) => {
                        bytes.put_u8(c);
                        response.put_slice(bytes);
                        *self = MessageDecoder::Forwarding;
                        continue;
                    }
                    (c, l) => unreachable!("char: {c} len: {l}"),
                },
            }
        }
        if !response.is_empty() {
            println!("sending it");
            Ok(Some(response.freeze()))
        } else {
            println!("sending nothing");
            Ok(None)
        }
    }
}

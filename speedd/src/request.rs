use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::Decoder;

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub enum RequestDecoder {
    #[default]
    Clean,
    Possible(BytesMut),
}

impl Decoder for RequestDecoder {
    type Item = Bytes;

    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut response = BytesMut::new();
        while src.has_remaining() {
            let ch = src.get_u8();
            println!("nom {ch}");
            match self {
                RequestDecoder::Clean => {
                    if ch != b'7' {
                        println!("appending byte {ch:?}");
                        response.put_u8(ch);
                        continue;
                    } else {
                        println!("found a seven, flushing");
                        let mut bytes = BytesMut::new();
                        bytes.put_u8(ch);
                        *self = RequestDecoder::Possible(bytes);
                        return Ok(Some(response.clone().freeze()));
                    }
                }
                RequestDecoder::Possible(bytes) => match (ch, bytes.len()) {
                    (b' ' | b'\n', 0..=24) => {
                        bytes.put_u8(ch);
                        println!("too short {bytes:?}");
                        response = bytes.clone();
                        *self = RequestDecoder::Clean;
                        continue;
                    }
                    (b' ' | b'\n', 25..=35) => {
                        bytes.put_u8(ch);
                        println!("ending it! {bytes:?}");
                        let result = Ok(Some(bytes.clone().freeze()));
                        *self = RequestDecoder::Clean;
                        return result;
                    }
                    (c, 0..=24) => {
                        if c == b' '
                        /*|| c == b'\n'*/
                        {
                            bytes.put_u8(c);
                            println!("too short {bytes:?}");
                            let result = Ok(Some(bytes.clone().freeze()));
                            *self = RequestDecoder::Clean;
                            return result;
                        } else {
                            bytes.put_u8(c);
                            println!("stuffing it: {bytes:?}");
                            continue;
                        }
                    }
                    (c, 35..) => {
                        bytes.put_u8(c);
                        println!("looong looong maaaan");
                        let result = Ok(Some(bytes.clone().freeze()));
                        *self = RequestDecoder::Clean;
                        return result;
                    }
                    (_, _) => unreachable!(),
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

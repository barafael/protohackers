use bytes::Buf;
use itertools::Itertools;
use std::time::Duration;
use tokio_util::codec::Decoder;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Message {
    Plate { plate: String, timestamp: u32 },
    WantHeartbeat(Duration),
    IAmCamera { road: u16, mile: u16, limit: u16 },
    IAmDispatcher(Vec<u16>),
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct MessageDecoder;

impl Decoder for MessageDecoder {
    type Item = Message;

    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(first) = src.first() {
            match first {
                0x20 => {
                    let Some(len) = src.get(1) else {
                        return Ok(None);
                    };
                    if src.remaining() >= 1 + 1 + *len as usize + 4 {
                        let bytes = std::str::from_utf8(&src[2..2 + *len as usize]).unwrap();
                        let timestamp = u32::from_be_bytes(
                            src[2 + *len as usize..2 + *len as usize + 4]
                                .try_into()
                                .unwrap(),
                        );
                        let plate = Message::Plate {
                            plate: bytes.to_string(),
                            timestamp,
                        };
                        src.advance(1 + 1 + *len as usize + 4);
                        return Ok(Some(plate));
                    } else {
                        Ok(None)
                    }
                }
                0x40 => {
                    if src.remaining() >= 1 + 4 {
                        let _ = src.get_u8();
                        let deciseconds = src.get_u32();
                        let millis = deciseconds * 100;
                        let dur = Duration::from_millis(millis as u64);
                        return Ok(Some(Message::WantHeartbeat(dur)));
                    } else {
                        Ok(None)
                    }
                }
                0x80 => {
                    if src.remaining() >= 1 + 6 {
                        let _ = src.get_u8();
                        let road = src.get_u16();
                        let mile = src.get_u16();
                        let limit = src.get_u16();
                        return Ok(Some(Message::IAmCamera { road, mile, limit }));
                    } else {
                        Ok(None)
                    }
                }
                0x81 => {
                    let Some(len) = src.get(1) else {
                        return Ok(None);
                    };
                    if src.remaining() < 1 + 1 + *len as usize * 2 {
                        return Ok(None);
                    }
                    let bytes = src
                        .iter()
                        .skip(2)
                        .cloned()
                        .array_chunks::<2>()
                        .into_iter()
                        .take(*len as usize)
                        .map(|a| u16::from_be_bytes(a.into()))
                        .collect_vec();
                    src.advance(1 + 1 + *len as usize);
                    return Ok(Some(Message::IAmDispatcher(bytes)));
                }
                n => unreachable!("{n}"),
            }
        } else {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn example() {
        let mut input = BytesMut::from(
            &[
                0x20, 0x07, 0x52, 0x45, 0x30, 0x35, 0x42, 0x4b, 0x47, 0x00, 0x01, 0xe2, 0x40,
            ][..],
        );

        let mut decoder = MessageDecoder::default();
        let first = decoder.decode(&mut BytesMut::from(&input[0..5][..]));
        assert!(matches!(first, Ok(None)));

        let second = decoder.decode(&mut input).unwrap().unwrap();
        let expected = Message::Plate {
            plate: "RE05BKG".to_string(),
            timestamp: 123456,
        };
        assert_eq!(expected, second);
    }

    #[test]
    fn dispatcher_example() {
        let mut input = BytesMut::from(&[0x81, 0x03, 0x00, 0x42, 0x01, 0x70, 0x13, 0x88][..]);

        let mut decoder = MessageDecoder::default();
        let first = decoder.decode(&mut BytesMut::from(&input[0..5][..]));
        assert!(matches!(first, Ok(None)));

        let second = decoder.decode(&mut input).unwrap().unwrap();
        let expected = Message::IAmDispatcher(vec![66, 368, 5000]);
        assert_eq!(expected, second);
    }
}

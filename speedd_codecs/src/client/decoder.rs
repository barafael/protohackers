use crate::{camera::Camera, plate::PlateRecord};
use bytes::Buf;
use itertools::Itertools;
use std::time::Duration;
use tokio_util::codec::Decoder;

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct MessageDecoder;

impl Decoder for MessageDecoder {
    type Item = crate::client::Message;

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
                        let plate = Self::Item::Plate(PlateRecord {
                            plate: bytes.to_string(),
                            timestamp,
                        });
                        src.advance(1 + 1 + *len as usize + 4);
                        Ok(Some(plate))
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
                        Ok(Some(Self::Item::WantHeartbeat(dur)))
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
                        Ok(Some(Self::Item::IAmCamera(Camera { road, mile, limit })))
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
                        .copied()
                        .array_chunks::<2>()
                        .take(*len as usize)
                        .map(u16::from_be_bytes)
                        .collect_vec();
                    src.advance(1 + 1 + *len as usize * 2);
                    Ok(Some(Self::Item::IAmDispatcher(bytes)))
                }
                n => anyhow::bail!("Invalid opcode 0x{n:x}"),
            }
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::client;
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
        let expected = client::Message::Plate(PlateRecord {
            plate: "RE05BKG".to_string(),
            timestamp: 123456,
        });
        assert_eq!(expected, second);
    }

    #[test]
    fn dispatcher_example() {
        let mut input = BytesMut::from(&[0x81, 0x03, 0x00, 0x42, 0x01, 0x70, 0x13, 0x88][..]);

        let mut decoder = MessageDecoder::default();
        let first = decoder.decode(&mut BytesMut::from(&input[0..5][..]));
        assert!(matches!(first, Ok(None)));

        let second = decoder.decode(&mut input).unwrap().unwrap();
        let expected = client::Message::IAmDispatcher(vec![66, 368, 5000]);
        assert_eq!(expected, second);
    }
}

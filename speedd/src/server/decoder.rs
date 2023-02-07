use anyhow::Ok;
use bytes::Buf;
use tokio_util::codec::Decoder;

use super::TicketRecord;

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct MessageDecoder;

impl Decoder for MessageDecoder {
    type Item = crate::server::Message;

    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(first) = src.first() {
            match first {
                0x10 => {
                    let Some(len) = src.get(1) else {
                        return Ok(None);
                    };
                    if src.remaining() <= 1 + 1 + *len as usize {
                        return Ok(None);
                    }
                    let bytes = String::from_utf8((src[2..2 + *len as usize]).to_vec())?;
                    Ok(Some(super::Message::Error(bytes)))
                }
                0x21 => {
                    let Some(len) = src.get(1) else {
                        return Ok(None);
                    };
                    if src.remaining() < 1 + 1 + *len as usize + 16 {
                        return Ok(None);
                    }
                    let plate = String::from_utf8((src[2..2 + *len as usize]).to_vec())?;
                    src.advance(1 + 1 + *len as usize);
                    let road = src.get_u16();
                    let mile1 = src.get_u16();
                    let timestamp1 = src.get_u32();
                    let mile2 = src.get_u16();
                    let timestamp2 = src.get_u32();
                    let speed = src.get_u16();
                    Ok(Some(super::Message::Ticket(TicketRecord {
                        plate,
                        road,
                        mile1,
                        timestamp1,
                        mile2,
                        timestamp2,
                        speed,
                    })))
                }
                0x41 => Ok(Some(super::Message::Heartbeat)),
                n => anyhow::bail!("Invalid opcode {n}"),
            }
        } else {
            Ok(None)
        }
    }
}

// TODO proptest

#[cfg(test)]
mod test {
    use bytes::BytesMut;

    use super::*;

    #[test]
    fn decodes_example() {
        let bytes = [
            0x21, 0x07, 0x52, 0x45, 0x30, 0x35, 0x42, 0x4b, 0x47, 0x01, 0x70, 0x04, 0xd2, 0x00,
            0x0f, 0x42, 0x40, 0x04, 0xd3, 0x00, 0x0f, 0x42, 0x7c, 0x17, 0x70,
        ];

        let mut decoder = MessageDecoder::default();
        let none = decoder
            .decode(&mut BytesMut::from(&bytes.clone()[0..5][..]))
            .unwrap();
        assert!(none.is_none());
        let message = decoder
            .decode(&mut BytesMut::from_iter(bytes))
            .unwrap()
            .unwrap();

        let expected = crate::server::Message::Ticket(TicketRecord {
            plate: "RE05BKG".to_string(),
            road: 368,
            mile1: 1234,
            timestamp1: 1000000,
            mile2: 1235,
            timestamp2: 1000060,
            speed: 6000,
        });
        assert_eq!(expected, message);
    }
}

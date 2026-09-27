use super::{Message, TicketRecord};
use crate::str_len;
use bytes::BufMut;
use tokio_util::codec::Encoder;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageEncoder;

impl Encoder<Message> for MessageEncoder {
    type Error = anyhow::Error;

    fn encode(&mut self, item: Message, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        match item {
            Message::Error(msg) => {
                // Better a truncated error message than a corrupt stream.
                let msg = &msg[..msg.floor_char_boundary(u8::MAX.into())];
                let len = str_len(msg)?;
                dst.put_u8(0x10);
                dst.put_u8(len);
                dst.put_slice(msg.as_bytes());
            }
            Message::Ticket(TicketRecord {
                plate,
                road,
                mile1,
                timestamp1,
                mile2,
                timestamp2,
                speed,
            }) => {
                // Check before writing anything, so that a failure leaves no partial frame behind.
                let len = str_len(&plate)?;
                dst.put_u8(0x21);
                dst.put_u8(len);
                dst.put_slice(plate.as_bytes());
                dst.put_u16(road);
                dst.put_u16(mile1);
                dst.put_u32(timestamp1);
                dst.put_u16(mile2);
                dst.put_u32(timestamp2);
                dst.put_u16(speed);
            }
            Message::Heartbeat => {
                dst.put_u8(0x41);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::server::decoder::MessageDecoder;
    use bytes::BytesMut;
    use tokio_util::codec::Decoder;

    #[test]
    fn example() {
        let ticket = Message::Ticket(TicketRecord {
            plate: "RE05BKG".to_string(),
            road: 368,
            mile1: 1234,
            timestamp1: 1000000,
            mile2: 1235,
            timestamp2: 1000060,
            speed: 6000,
        });
        let mut buffer = BytesMut::with_capacity(5);
        let mut encoder = MessageEncoder;
        encoder.encode(ticket, &mut buffer).unwrap();
        let expected = [
            0x21, 0x07, 0x52, 0x45, 0x30, 0x35, 0x42, 0x4b, 0x47, 0x01, 0x70, 0x04, 0xd2, 0x00,
            0x0f, 0x42, 0x40, 0x04, 0xd3, 0x00, 0x0f, 0x42, 0x7c, 0x17, 0x70,
        ];
        assert_eq!(&expected, &buffer.freeze()[..]);
    }

    #[test]
    fn truncates_long_error_messages() {
        let mut buffer = BytesMut::new();
        let mut encoder = MessageEncoder;
        encoder
            .encode(Message::Error("x".repeat(300)), &mut buffer)
            .unwrap();
        encoder.encode(Message::Heartbeat, &mut buffer).unwrap();

        // The next message still starts where it should.
        let mut decoder = MessageDecoder;
        assert_eq!(
            decoder.decode(&mut buffer).unwrap(),
            Some(Message::Error("x".repeat(255)))
        );
        assert_eq!(
            decoder.decode(&mut buffer).unwrap(),
            Some(Message::Heartbeat)
        );

        // Not in the middle of a character.
        encoder
            .encode(Message::Error("é".repeat(200)), &mut buffer)
            .unwrap();
        assert_eq!(
            decoder.decode(&mut buffer).unwrap(),
            Some(Message::Error("é".repeat(127)))
        );
    }

    #[test]
    fn rejects_plates_which_do_not_fit() {
        let ticket = Message::Ticket(TicketRecord {
            plate: "X".repeat(256),
            road: 368,
            mile1: 1234,
            timestamp1: 1000000,
            mile2: 1235,
            timestamp2: 1000060,
            speed: 6000,
        });
        let mut buffer = BytesMut::new();
        assert!(MessageEncoder.encode(ticket, &mut buffer).is_err());
        assert!(buffer.is_empty());
    }
}

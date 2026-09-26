use crate::{escape::escape, frame::Frame, MAX_MESSAGE_LEN};
use anyhow::Context;
use bytes::{Buf, BufMut, BytesMut};
use std::str::FromStr;
use tokio_util::codec::{Decoder, Encoder};

#[derive(Debug, Copy, Clone, Default)]
pub struct Lrcp;

impl Decoder for Lrcp {
    type Item = Frame;

    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let bytes = src.to_vec();
        src.advance(bytes.len());
        if bytes.is_empty() {
            return Ok(None);
        }
        let frame = String::from_utf8(bytes)?;
        log::info!("Decoding frame: {frame}");
        let frame =
            Frame::from_str(&frame).with_context(|| format!("Failed to parse frame: {frame}"))?;
        Ok(Some(frame))
    }
}

impl Encoder<Frame> for Lrcp {
    type Error = anyhow::Error;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        log::info!("Encoding item {item:?}");
        let message = match item {
            Frame::Connect(session) => format!("/connect/{session}/"),
            Frame::Ack { session, length } => format!("/ack/{session}/{length}/"),
            Frame::Data {
                session,
                position,
                data,
            } => {
                // Like the decoder, the frame holds the data as the application sees it.
                format!("/data/{session}/{position}/{}/", escape(&data))
            }
            Frame::Close(session) => format!("/close/{session}/"),
        };
        anyhow::ensure!(
            message.len() <= MAX_MESSAGE_LEN,
            "Message of {} bytes exceeds the limit of {MAX_MESSAGE_LEN} bytes",
            message.len()
        );
        dst.put_slice(message.as_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn escapes_data() {
        let frame = Frame::Data {
            session: 1,
            position: 0,
            data: r"a/b\c".to_string(),
        };
        let mut buffer = BytesMut::new();
        Lrcp.encode(frame.clone(), &mut buffer).unwrap();
        assert_eq!(&buffer[..], br"/data/1/0/a\/b\\c/");
        assert_eq!(Lrcp.decode(&mut buffer).unwrap(), Some(frame));
    }

    #[test]
    fn rejects_data_which_does_not_fit_once_escaped() {
        let frame = Frame::Data {
            session: 1,
            position: 0,
            data: "/".repeat(600),
        };
        let mut buffer = BytesMut::new();
        assert!(Lrcp.encode(frame, &mut buffer).is_err());
        assert!(buffer.is_empty());
    }

    #[test]
    fn data_frames_fit_once_escaped() {
        let data = format!("{}\n{}", "/".repeat(1000), "é".repeat(500));
        let frames = Frame::data(2_000_000_000, 2_000_000_000, &data);

        let mut position = 2_000_000_000;
        let mut received = String::new();
        for frame in frames {
            let mut buffer = BytesMut::new();
            Lrcp.encode(frame.clone(), &mut buffer).unwrap();
            assert!(buffer.len() < 1000);
            let Frame::Data {
                position: at, data, ..
            } = frame
            else {
                panic!("{frame:?} is no data frame");
            };
            assert_eq!(at, position);
            position += data.len() as u32;
            received.push_str(&data);
        }
        assert_eq!(received, data);
    }
}

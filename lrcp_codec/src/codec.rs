use crate::frame::Frame;
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
        let frame =
            Frame::from_str(&frame).with_context(|| format!("Failed to parse frame: {frame}"))?;
        Ok(Some(frame))
    }
}

impl Encoder<Frame> for Lrcp {
    type Error = anyhow::Error;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            Frame::Connect { session } => {
                let data = format!("/connect/{session}/");
                dst.put_slice(data.as_bytes());
                Ok(())
            }
            Frame::Ack { session, length } => {
                let data = format!("/ack/{session}/{length}/");
                dst.put_slice(data.as_bytes());
                Ok(())
            }
            Frame::Data {
                session,
                position,
                data,
            } => {
                let data = format!("/data/{session}/{position}/{data}/");
                dst.put_slice(data.as_bytes());
                Ok(())
            }
            Frame::Close(session) => {
                let data = format!("/data/{session}/");
                dst.put_slice(data.as_bytes());
                Ok(())
            }
        }
    }
}

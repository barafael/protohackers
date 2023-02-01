use bytes::{BufMut, BytesMut};
use tokio_util::codec::Encoder;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Response(i32);

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ResponseEncoder;

impl Encoder<i32> for ResponseEncoder {
    type Error = anyhow::Error;

    fn encode(&mut self, item: i32, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.put_i32(item);
        Ok(())
    }
}

mod codec;
pub mod escape;
mod frame;
mod unescape;

pub use crate::codec::Lrcp;
pub use crate::frame::Frame;

pub const ESCAPE: char = '\\';

/// LRCP messages must be smaller than 1000 bytes.
pub const MAX_MESSAGE_LEN: usize = 999;

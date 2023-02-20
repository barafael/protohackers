mod codec;
pub mod escape;
mod frame;
mod unescape;

pub use crate::codec::Lrcp;
pub use crate::frame::Frame;

pub const ESCAPE: char = '\\';

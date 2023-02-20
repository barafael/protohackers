use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::unescape::unescape;

macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    Connect(u32),
    Ack {
        session: u32,
        length: u32,
    },
    Data {
        session: u32,
        position: u32,
        data: String, // or Bytes
    },
    Close(u32),
}

impl Frame {
    pub fn session_id(&self) -> u32 {
        match self {
            Frame::Connect(s) => *s,
            Frame::Ack { session, .. } => *session,
            Frame::Data { session, .. } => *session,
            Frame::Close(s) => *s,
        }
    }
}

impl FromStr for Frame {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let connect = regex!(r"^/connect/(\d+)/$");
        let data = regex!(r"^/data/(\d+)/(\d+)/((\\/|\\\\|[^/])+)/$");
        let ack = regex!(r"^/ack/(\d+)/(\d+)/$");
        let close = regex!(r"^/close/(\d+)/$");

        if let Some(caps) = connect.captures(s) {
            let session = parse_number(&caps[1])?;
            Ok(Frame::Connect(session))
        } else if let Some(caps) = data.captures(s) {
            let session = parse_number(&caps[1])?;
            let position = parse_number(&caps[2])?;
            let data = caps[3].to_string();
            let data = unescape(&data);
            Ok(Frame::Data {
                session,
                position,
                data,
            })
        } else if let Some(caps) = ack.captures(s) {
            let session = parse_number(&caps[1])?;
            let length = parse_number(&caps[2])?;
            Ok(Frame::Ack { session, length })
        } else if let Some(caps) = close.captures(s) {
            let session = parse_number(&caps[1])?;
            Ok(Frame::Close(session))
        } else {
            Err(anyhow!("Invalid message {s}"))
        }
    }
}

fn parse_number(n: &str) -> anyhow::Result<u32> {
    let number = n.parse::<u32>()?;
    anyhow::ensure!(number < 2147483648);
    Ok(number)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parses_connect() {
        let string = r"/connect/123/";
        let frame = Frame::from_str(&string).unwrap();
        assert_eq!(Frame::Connect(123), frame);
    }

    #[test]
    fn parses_data() {
        let string = r"/data/1234567/0/hello
/";
        let frame = Frame::from_str(&string).unwrap();
        assert_eq!(
            Frame::Data {
                session: 1234567,
                position: 0,
                data: "hello\n".to_string()
            },
            frame
        );
    }

    #[test]
    fn parses_single_slash_message() {
        let string = r"/data/1234568/0/\//";
        let frame = Frame::from_str(&string).unwrap();
        assert_eq!(
            Frame::Data {
                session: 1234568,
                position: 0,
                data: r"/".to_string()
            },
            frame
        );
    }

    #[test]
    fn backslashes() {
        let string = r"/data/1981800348/0/foo\/bar\/baz
foo\\bar\\baz
/";
        let frame = Frame::from_str(&string).unwrap();
        assert_eq!(
            frame,
            Frame::Data {
                session: 1981800348,
                position: 0,
                data: r"foo/bar/baz
foo\bar\baz
"
                .to_string()
            }
        );
    }

    #[test]
    fn unescapes_backslashes() {
        let string = r"foo\/bar\\baz";
        let result = unescape(string);
        assert_eq!(result, r"foo/bar\baz");
    }

    #[test]
    fn unescapes_backslashes_2() {
        let string = "some data with a slash\n";
        let result = unescape(string);
        assert_eq!(result, "some data with a slash\n");
    }

    #[test]
    fn unescapes_backslashes_3() {
        let string = r"foo\/bar\/baz
foo\\\\bar\\baz
";
        let result = unescape(string);
        assert_eq!(
            result,
            r"foo/bar/baz
foo\\bar\baz
"
        );
    }
}

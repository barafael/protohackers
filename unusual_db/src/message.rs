use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Insert { key: String, value: String },
    Query(String),
}

impl FromStr for Message {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((key, value)) = s.split_once('=') {
            Ok(Self::Insert {
                key: key.to_string(),
                value: value.to_string(),
            })
        } else {
            Ok(Self::Query(s.to_string()))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn parses() {
        assert_eq!(
            Message::from_str("foo=bar").unwrap(),
            Message::Insert {
                key: "foo".to_string(),
                value: "bar".to_string()
            }
        );
        assert_eq!(
            Message::from_str("foo=bar=baz").unwrap(),
            Message::Insert {
                key: "foo".to_string(),
                value: "bar=baz".to_string()
            }
        );
        assert_eq!(
            Message::from_str("foo=").unwrap(),
            Message::Insert {
                key: "foo".to_string(),
                value: "".to_string()
            }
        );
        assert_eq!(
            Message::from_str("foo===").unwrap(),
            Message::Insert {
                key: "foo".to_string(),
                value: "==".to_string()
            }
        );
        assert_eq!(
            Message::from_str("=foo").unwrap(),
            Message::Insert {
                key: "".to_string(),
                value: "foo".to_string()
            }
        );
    }
}

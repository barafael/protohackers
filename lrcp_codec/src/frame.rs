use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

const SEPARATOR: char = '/';
const ESCAPE: char = '\\';

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    Connect {
        session: u32,
    },
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

impl FromStr for Frame {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        anyhow::ensure!(s.starts_with('/'), "Message does not start with '/'");
        anyhow::ensure!(s.ends_with('/'), "Message does not end with '/'");
        // todo: ensure that last slash is not escaped?
        let tokens = tokenize(s);
        let tokens = &tokens[1..tokens.len() - 1];
        match tokens.first() {
            None => Err(anyhow::anyhow!("No tokens")),
            Some(c) if c == "connect" => {
                anyhow::ensure!(
                    tokens.len() == 2,
                    "Invalid argument count for connect: {tokens:?}"
                );
                let session = tokens.get(1).context("Missing session id field")?;
                let session = parse_number(session).context("Failed to parse session id")?;
                Ok(Frame::Connect { session })
            }
            Some(d) if d == "data" => {
                anyhow::ensure!(
                    tokens.len() == 4,
                    "Invalid argument count for data: {tokens:?}"
                );
                let session = tokens.get(1).context("Missing session id field")?;
                let session = parse_number(session).context("Failed to parse session id")?;
                let position = tokens.get(2).context("Missing pos field")?;
                let position = parse_number(position).context("Failed to parse data position")?;
                let data = tokens.get(3).context("Missing data field")?;
                Ok(Frame::Data {
                    session,
                    position,
                    data: data.to_string(),
                })
            }
            Some(a) if a == "ack" => {
                anyhow::ensure!(
                    tokens.len() == 3,
                    "Invalid argument count for ack: {tokens:?}"
                );
                let session = tokens.get(1).context("Missing session id field")?;
                let session = parse_number(session).context("Failed to parse session id")?;
                let length = tokens.get(2).context("Missing pos field")?;
                let length = parse_number(length).context("Failed to parse length")?;
                Ok(Frame::Ack { session, length })
            }
            Some(c) if c == "close" => {
                anyhow::ensure!(
                    tokens.len() == 2,
                    "Invalid argument count for close: {tokens:?}"
                );
                let session = tokens.get(1).context("Missing session id field")?;
                let session = parse_number(session).context("Failed to parse session id")?;
                Ok(Frame::Close(session))
            }
            Some(c) => Err(anyhow!("Invalid command {c}")),
        }
    }
}

fn tokenize(string: &str) -> Vec<String> {
    let mut token = String::new();
    let mut tokens: Vec<String> = Vec::new();
    let mut chars = string.chars();
    while let Some(ch) = chars.next() {
        match ch {
            SEPARATOR => {
                tokens.push(token.clone());
                token.clear();
            }
            ESCAPE => {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
            _ => token.push(ch),
        }
    }
    tokens.push(token);
    tokens
}

fn parse_number(n: &str) -> anyhow::Result<u32> {
    let number = n.parse::<u32>()?;
    anyhow::ensure!(number < 2147483648);
    Ok(number)
}

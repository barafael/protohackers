use anyhow::Context;
use clap::{arg, Parser, Subcommand, ValueEnum};
use std::{fmt::Display, net::SocketAddr};

pub fn parse_hex_digit(s: &str) -> anyhow::Result<u8> {
    u8::from_str_radix(s, 16).context("Failed to parse hex byte")
}

#[derive(Copy, Debug, Clone, Default, ValueEnum)]
pub enum Delimiter {
    #[default]
    Newline,
    CrLf,
    None,
}

impl Display for Delimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format!("{self:?}").to_lowercase())
    }
}

#[derive(Parser, Debug)]
#[command(author, version)]
pub struct Arguments {
    /// Socket to bind on
    #[arg(short, long, default_value = "127.0.0.1:8000")]
    pub socket: SocketAddr,

    /// Newlines
    #[arg(short, long, default_value_t)]
    pub delimiter: Delimiter,

    /// Interpret input as hexadecimal
    #[arg(long, default_value_t)]
    pub hex: bool,

    #[command(subcommand)]
    pub action: Action,
}

#[derive(Debug, Subcommand)]
pub enum Action {
    /// raw hex bytes
    Hex {
        /// Bytes to send (hexadecimal)
        #[arg(num_args = 1.., value_delimiter = ' ', value_parser = parse_hex_digit)]
        bytes: Vec<u8>,
    },
    Repl,
}

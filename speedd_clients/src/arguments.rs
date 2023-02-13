use anyhow::Context;
use clap::{Parser, Subcommand};
use std::{net::SocketAddr, time::Duration};

fn parse_hex_digit(s: &str) -> anyhow::Result<u16> {
    u16::from_str_radix(s, 16).context("Failed to parse hex")
}

#[derive(Debug, Parser)]
#[command(author, version)]
pub struct Arguments {
    /// Address to connect to
    #[arg(short, long, default_value = "0.0.0.0:8000")]
    pub address: SocketAddr,

    /// Heartbeat interval duration (off by default)
    #[arg(short, long, default_value_t = Duration::ZERO.into())]
    pub interval: humantime::Duration,

    #[command(subcommand)]
    pub mode: Mode,
}

#[derive(Debug, Subcommand)]
pub enum Mode {
    Client,
    Dispatcher {
        #[arg(value_delimiter = ' ', value_parser = parse_hex_digit)]
        roads: Vec<u16>,
    },
    Camera {
        /// Road ID
        #[arg(short, long)]
        road: u16,

        /// Camera position
        #[arg(short, long)]
        mile: u16,

        /// Speed limits in `mp/h x 100` (yes, I know, but that's the problem statetment)
        #[arg(short, long)]
        limit: u16,
    },
}

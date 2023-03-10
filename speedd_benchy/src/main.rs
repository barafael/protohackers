#![feature(iter_array_chunks)]

use anyhow::Context;
use arguments::{Arguments, Mode};
use clap::Parser;
use futures::future::try_join_all;
use landscape::Landscape;
use sequence::Sequence;

mod arguments;
mod camera_client;
mod landscape;
mod sequence;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Arguments::parse();

    match args.mode {
        Mode::Generate { input, output } => {
            let landscape = Landscape::from_file(input)?;
            let sequence = Sequence::new(&landscape);
            std::fs::write(
                output,
                ron::to_string(&sequence).context("Failed to serialize sequence to toml string")?,
            )
            .context("Failed to write sequence file")?;
        }
        Mode::Replay { server, instance } => {
            let input = std::fs::read_to_string(instance)?;
            let sequence: Sequence = ron::from_str(&input)?;

            let handles = sequence.run(server).await?;
            try_join_all(handles)
                .await
                .context("Failed to join")?
                .into_iter()
                .collect::<anyhow::Result<Vec<_>>>()
                .context("Failed to run camera tasks")?;
        }
    }

    Ok(())
}

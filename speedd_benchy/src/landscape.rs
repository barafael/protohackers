use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Landscape {
    pub number_of_samples: u16,
    pub number_of_roads: u16,
    pub number_of_cameras: u16,
    pub number_of_cars: u16,
    pub number_of_dispatchers: u16,
    pub ticket_likelihood: f64,
    #[serde(with = "humantime_serde")]
    pub duration: Duration,
    pub shuffle_reports: bool,
}

impl Landscape {
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let landscape = std::fs::read_to_string(path)?;
        toml::from_str(&landscape).context("Failed to read landscape toml file")
    }
}

use std::fs;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct InitConfig {
    pub tty_sessions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct VerdantdConfig {
    pub service_dir: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub init: InitConfig,

    #[serde(default)]
    pub verdantd: Option<VerdantdConfig>,
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let config = toml::from_str(&contents)?;
        Ok(config)
    }
}


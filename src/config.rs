use serde::Deserialize;
use std::{fs::File, io::BufReader};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub processes: Vec<ProcessConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProcessConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
}

pub fn load_config(file_path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let file = File::open(file_path).expect("Failed to open config file");
    let reader = BufReader::new(file);
    let config: Config = serde_yaml::from_reader(reader)?;
    if config.processes.len() < 1 || config.processes.len() > 6 {
        return Err("Number of processes must be between 1 and 6".into());
    }
    Ok(config)
}


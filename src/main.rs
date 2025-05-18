mod config;
mod tui;
mod process;

use config::load_config;
use tui::run_tui;
use process::spawn_process;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let config_file = args.get(1).map(|s| s.as_str()).unwrap_or("config.yaml");
    let config = load_config(config_file).expect("Failed to load config");
    run_tui(spawn_process(&config).await?).await
}

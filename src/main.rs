mod config;
mod process;
mod tui;

use config::load_config;
use process::spawn_process;
use tui::run_tui;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let config_file = args.get(1).map(|s| s.as_str()).unwrap_or("runner.yaml");
    let config = load_config(config_file).expect("Failed to load config");
    let (channels, mut manager) = spawn_process(&config).await?;
    run_tui(channels).await?;
    manager.stop_all();
    Ok(())
}

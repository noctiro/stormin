mod app;
mod config;
mod data_generator;
mod generator;
mod logger;
mod template;
mod ui;
mod worker;

use app::App;
use std::{env, error::Error, io};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config_path = env::args()
        .find(|arg| arg.starts_with("--config="))
        .map(|arg| arg.trim_start_matches("--config=").to_string())
        .unwrap_or_else(|| "config.toml".to_string());
    let mut app = match App::new(&config_path) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Failed to initialize application: {}", e);
            // Attempt to disable raw mode if it was enabled
            let _ = crossterm::terminal::disable_raw_mode();
            // Attempt to leave alternate screen if entered
            let _ = crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
            return Err(e);
        }
    };

    // Spawn background threads
    app.spawn_log_receiver(); // Log receiver is still std::thread
    app.spawn_workers(); // Workers are now tokio tasks

    // Run the main application loop async
    let run_result = app.run().await;

    if let Err(e) = &run_result {
        eprintln!("Application runtime error: {}", e);
    }

    eprintln!("Restoring terminal before shutdown...");
    if let Err(e) = app.cleanup() {
        eprintln!("Error during pre-shutdown cleanup: {}", e);
    }

    run_result
}

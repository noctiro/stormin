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
    let args: Vec<String> = env::args().collect();
    let cli_mode = args.contains(&"--cli".to_string());

    let config_path = args
        .iter()
        .find(|arg| arg.starts_with("--config="))
        .map(|arg| arg.trim_start_matches("--config=").to_string())
        .unwrap_or_else(|| "config.toml".to_string());

    let mut app = match App::new(&config_path, cli_mode).await {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Failed to initialize application: {}", e);
            if !cli_mode {
                // Attempt to disable raw mode if it was enabled (only in TUI mode)
                let _ = crossterm::terminal::disable_raw_mode();
                // Attempt to leave alternate screen if entered (only in TUI mode)
                let _ =
                    crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
            }
            return Err(e);
        }
    }; // Spawn background threads

    if !cli_mode {
        app.spawn_log_receiver(); // Log receiver is TUI specific
    }
    app.spawn_data_generators(); // First spawn data generators
    app.spawn_workers(); // Then spawn workers

    // Run the main application loop async (App::run handles TUI/CLI internally)
    let run_result = app.run().await;

    // 打印最终统计信息
    // Always print final stats regardless of mode or exit status
    app.print_final_stats().await;

    if let Err(e) = &run_result {
        eprintln!("Application runtime error: {}", e);
    }

    run_result
}

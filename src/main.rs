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

    // Cleanup should happen regardless of whether run_result is Ok or Err,
    // especially for TUI mode to restore the terminal.
    if !cli_mode {
        // Attempt to perform cleanup first.
        // If cleanup fails, we still want to report the original run_result error if it exists.
        if let Err(cleanup_err) = app.cleanup() {
            eprintln!("Error during pre-shutdown cleanup: {}", cleanup_err);
            // If run_result was also an error, prioritize returning the run_result error.
            // Otherwise, return the cleanup error.
            return run_result
                .map_err(|run_e| {
                    eprintln!("Original application runtime error: {}", run_e);
                    run_e // return original error
                })
                .and_then(|_| Err(cleanup_err)); // if run_result was Ok, return cleanup_err
        } else {
            eprintln!("Terminal restored successfully.");
        }
    }
    // 打印最终统计信息
    if cli_mode || run_result.is_ok() {
        app.print_final_stats().await;
    }

    if let Err(e) = &run_result {
        eprintln!("Application runtime error: {}", e);
    }

    run_result
}

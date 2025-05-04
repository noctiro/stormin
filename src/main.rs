mod app;
mod config;
mod generator;
mod logger;
mod template;
mod ui;
mod worker;

use app::App;
use std::{error::Error, io};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Create and initialize the application
    // App::new remains synchronous
    let mut app = match App::new("config.toml") {
        Ok(app) => app,
        Err(e) => {
            // If App creation fails (e.g., config error, TUI setup error),
            // print the error and exit gracefully.
            // Ensure terminal state is cleaned up if possible, though App::drop handles this too.
            eprintln!("Failed to initialize application: {}", e);
            // Attempt to disable raw mode if it was enabled
            let _ = crossterm::terminal::disable_raw_mode();
            // Attempt to leave alternate screen if entered
            let _ = crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
            return Err(e); // Propagate the error
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

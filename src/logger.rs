use std::sync::mpsc::Sender;
use chrono::Utc;

use crate::ui::DebugInfo; // Assuming DebugInfo is defined in ui module and includes a timestamp and message
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    // Debug, // Consider adding a Debug level if not already present
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warning => "WARN",
            LogLevel::Error => "ERROR",
            // LogLevel::Debug => "DEBUG",
        }
    }
}

#[derive(Clone)]
pub struct Logger {
    sender: Option<Sender<DebugInfo>>, // For TUI mode
    cli_mode: bool,                   // To distinguish between TUI and CLI
}

impl Logger {
    // Constructor now takes an Option for the sender and the cli_mode flag
    pub fn new(sender: Option<Sender<DebugInfo>>, cli_mode: bool) -> Self {
        Logger { sender, cli_mode }
    }

    // close_sender remains the same, useful if TUI mode was active and needs to stop sending
    pub fn close_sender(&mut self) {
        self.sender.take();
    }

    pub fn log(&self, level: LogLevel, message: &str) {
        if self.cli_mode {
            // In CLI mode, print directly to stdout/stderr
            let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S");
            let formatted_message = format!("[{}] [{}] {}", timestamp, level.as_str(), message);
            if level == LogLevel::Error || level == LogLevel::Warning {
                eprintln!("{}", formatted_message);
            } else {
                println!("{}", formatted_message);
            }
        } else if let Some(sender) = &self.sender {
            // In TUI mode, send through the channel
            // The existing send_log logic can be adapted or reused here.
            // For simplicity, directly creating DebugInfo and sending.
            let debug_info = DebugInfo {
                // Assuming DebugInfo from ui.rs has these fields.
                // If DebugInfo in ui.rs uses chrono::DateTime<Utc>, use that.
                // Otherwise, adjust as needed. For now, using Instant for consistency with original send_log.
                timestamp: Instant::now(), // Or Utc::now() if DebugInfo expects DateTime<Utc>
                message: format!("[{}] {}", level.as_str(), message),
            };
            // The complex error handling from the original send_log can be kept if desired.
            // For this diff, simplifying to a direct send.
            if sender.send(debug_info).is_err() {
                // Fallback if TUI channel is closed, print to stderr
                let timestamp_fallback = Utc::now().format("%Y-%m-%d %H:%M:%S");
                eprintln!(
                    "[Fallback] [{}] [{}] {}",
                    timestamp_fallback,
                    level.as_str(),
                    message
                );
            }
        }
        // If not cli_mode and sender is None, logs are effectively dropped,
        // which is consistent with close_sender behavior.
    }

    pub fn info(&self, message: &str) {
        self.log(LogLevel::Info, message);
    }

    pub fn warning(&self, message: &str) {
        self.log(LogLevel::Warning, message);
    }

    pub fn error(&self, message: &str) {
        self.log(LogLevel::Error, message);
    }

    // The original send_log method is removed as its logic is integrated into log()
    // or simplified. If the detailed error handling (FAILURE_COUNT, LAST_WARNING)
    // is crucial, it should be re-integrated into the TUI path of the log() method.
}

// Macros remain unchanged but will now call the modified Logger methods.
// Note: The log_debug macro points to a $logger.debug method which is not defined.
// It should either be removed or a debug method added to Logger.
// For now, I will comment it out to avoid compilation errors.

/*
#[macro_export]
macro_rules! log_debug {
    ($logger:expr, $($arg:tt)*) => {
        $logger.debug(&format!($($arg)*))
    };
}
*/

#[macro_export]
macro_rules! log_info {
    ($logger:expr, $($arg:tt)*) => {
        $logger.info(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warning {
    ($logger:expr, $($arg:tt)*) => {
        $logger.warning(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($logger:expr, $($arg:tt)*) => {
        $logger.error(&format!($($arg)*))
    };
}

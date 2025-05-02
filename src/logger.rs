use crate::ui::DebugInfo;
use crossbeam_channel::Sender;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warning => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

#[derive(Clone)] // Add Clone derive
pub struct Logger {
    sender: Sender<DebugInfo>,
}

impl Logger {
    pub fn new(sender: Sender<DebugInfo>) -> Self {
        Logger { sender }
    }

    pub fn log(&self, level: LogLevel, message: &str) {
        let formatted_message = format!("[{}] {}", level.as_str(), message);
        self.send_log(formatted_message);
    }

    pub fn debug(&self, message: &str) {
        self.log(LogLevel::Debug, message);
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

    fn send_log(&self, message: String) {
        let debug_info = DebugInfo {
            timestamp: Instant::now(),
            message,
        };
        if let Err(e) = self.sender.send(debug_info) {
            // Fallback to stderr if the channel is closed
            eprintln!("Failed to send log message: {}", e);
        }
    }
}

// Convenience function for backward compatibility
pub fn log_debug(message: String, log_tx: &Sender<DebugInfo>) {
    let logger = Logger::new(log_tx.clone());
    logger.debug(&message);
}

// Macros to simplify logging
#[macro_export]
macro_rules! log_debug {
    ($logger:expr, $($arg:tt)*) => {
        $logger.debug(&format!($($arg)*))
    };
}

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

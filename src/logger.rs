use std::sync::mpsc::Sender; // Changed to std::sync::mpsc

use crate::ui::DebugInfo;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
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
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        
        static FAILURE_COUNT: AtomicUsize = AtomicUsize::new(0);
        static LAST_WARNING: AtomicUsize = AtomicUsize::new(0);
        
        let debug_info = DebugInfo {
            timestamp: Instant::now(),
            message,
        };

        // std::sync::mpsc::Sender::send is blocking, but usually fine for logging.
        // It returns Err if the channel is disconnected.
        if let Err(e) = self.sender.send(debug_info) {
            // Log channel is likely disconnected, meaning the log_receiver thread has panicked or exited.
            // This is a critical situation for logging.
            let failures = FAILURE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default() // Fallback to 0 if system time is before UNIX_EPOCH
                .as_millis() as usize;
            
            let last = LAST_WARNING.load(Ordering::Relaxed);
            // Throttle critical warnings to avoid flooding stderr if logging is broken.
            if failures == 1 || (now - last > 5000) { // Log first failure and then every 5s
                eprintln!(
                    "CRITICAL: Failed to send log, channel might be closed. Error: {}. Failures: {}",
                    e, failures
                );
                LAST_WARNING.store(now, Ordering::Relaxed);
            }
        } else {
            // Reset failure count on successful send
            FAILURE_COUNT.store(0, Ordering::Relaxed);
        }
    }
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

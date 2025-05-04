use tokio::sync::mpsc::Sender;

use crate::ui::DebugInfo;
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
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        
        static FAILURE_COUNT: AtomicUsize = AtomicUsize::new(0);
        static LAST_WARNING: AtomicUsize = AtomicUsize::new(0);
        
        let debug_info = DebugInfo {
            timestamp: Instant::now(),
            message,
        };

        match self.sender.try_send(debug_info) {
            Ok(_) => {
                FAILURE_COUNT.store(0, Ordering::Relaxed);
            }
            Err(e) => {
                // Only count full channel errors
                if let tokio::sync::mpsc::error::TrySendError::Full(_) = e {
                    let failures = FAILURE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                    
                    // Get current timestamp in milliseconds
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as usize;

                    // Log warning at most every 5 seconds
                    if failures % 100 == 0 {
                        let last = LAST_WARNING.load(Ordering::Relaxed);
                        if now - last > 5000 { // 5 seconds
                            LAST_WARNING.store(now, Ordering::Relaxed);
                        }
                    }
                } else {
                    // Handle closed channel error
                    eprintln!("CRITICAL: Log channel closed - {}", e);
                }
            }
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

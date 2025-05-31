use chrono::Utc;
use std::sync::mpsc::Sender;

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
            // LogLevel::Debug => "DEBUG",
        }
    }
}

#[derive(Clone)]
pub struct Logger {
    sender: Option<Sender<DebugInfo>>, // For TUI mode
    cli_mode: bool,                    // To distinguish between TUI and CLI
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
            // CLI模式：直接打印
            let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S");
            let formatted = format!("[{}] [{}] {}", timestamp, level.as_str(), message);
            match level {
                LogLevel::Info | LogLevel::Warning => println!("{}", formatted),
                LogLevel::Error => eprintln!("{}", formatted),
            }
        } else if let Some(sender) = &self.sender {
            // TUI模式：通过通道发送
            let debug_info = DebugInfo {
                timestamp: Instant::now(),
                message: format!("[{}] {}", level.as_str(), message),
            };

            // 使用try_send避免阻塞，失败时回退到标准错误
            match sender.send(debug_info) {
                Ok(_) => {}
                Err(_) => {
                    // 通道已断开，回退到标准错误
                    let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S");
                    eprintln!(
                        "[TUI Console] [{}] [{}] {}",
                        timestamp,
                        level.as_str(),
                        message
                    );
                }
            }
        }
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

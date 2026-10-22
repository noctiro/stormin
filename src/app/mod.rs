mod lifecycle;
mod reporting;
mod terminal;

use self::terminal::TerminalGuard;
use crate::config::loader;
use crate::logger::Logger;
use crate::types::{
    DebugInfo, PreGeneratedRequest, RunningState, Stats, TargetStats, TargetUpdate, WorkerMessage,
};
use crate::ui::LayoutRects;
use crate::ui::stats_updater::StatsUpdater;
use ratatui::DefaultTerminal;
use std::sync::mpsc as std_mpsc;
use std::{
    collections::VecDeque,
    error::Error,
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::Instant,
};
use sysinfo::System;
use tokio::sync::Mutex;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

pub struct App {
    pub config: loader::AttackConfig,
    pub stats: Arc<Mutex<Stats>>,
    pub logger: Logger,
    pub terminal: Option<DefaultTerminal>,
    terminal_guard: Option<TerminalGuard>,
    pub control_tx: broadcast::Sender<WorkerMessage>,
    pub(crate) data_pool_tx: Option<mpsc::Sender<PreGeneratedRequest>>,
    pub(crate) data_pool_rx: Option<Arc<Mutex<mpsc::Receiver<PreGeneratedRequest>>>>,
    pub target_stats_tx: mpsc::Sender<TargetUpdate>,
    pub target_stats_rx: mpsc::Receiver<TargetUpdate>,
    pub(crate) log_rx: Option<std_mpsc::Receiver<DebugInfo>>,
    pub(crate) worker_handles: Vec<JoinHandle<()>>,
    pub data_generator_handles: Vec<JoinHandle<()>>,
    pub data_generator_stop_signal: Arc<AtomicBool>,
    pub(crate) log_receiver_handle: Option<thread::JoinHandle<()>>,
    pub layout_rects: LayoutRects,
    pub stats_updater: StatsUpdater,
    pub cli_mode: bool,
}

impl App {
    pub async fn new(config_path: &str, cli_mode: bool) -> Result<Self, Box<dyn Error>> {
        let (terminal, terminal_guard) = if !cli_mode {
            let (terminal, guard) = terminal::setup_terminal()?;
            (Some(terminal), Some(guard))
        } else {
            (None, None)
        };

        let (logger_tx, mut log_rx) = if !cli_mode {
            let (tx, rx) = std_mpsc::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let logger = Logger::new(logger_tx.clone(), cli_mode);

        let (control_tx, _) = broadcast::channel(128);
        let (target_stats_tx, target_stats_rx) = mpsc::channel(256);

        let log_receiver_handle = if !cli_mode {
            let log_rx = log_rx.take().expect("Log receiver not initialized");
            let debug_logs_tx = target_stats_tx.clone();
            let logger_clone = logger.clone();

            Some(thread::spawn(move || {
                logger_clone.info("Log receiver thread started");
                for log_entry in log_rx {
                    let update = TargetUpdate {
                        id: 0,
                        url: String::new(),
                        success: false,
                        timestamp: log_entry.timestamp,
                        debug: Some(log_entry.message),
                        network_error: None,
                        thread_id: std::thread::current().id(),
                    };
                    if debug_logs_tx.blocking_send(update).is_err() {
                        break;
                    }
                }
                logger_clone.info("Log receiver thread finished");
            }))
        } else {
            None
        };

        let config = loader::load_config_and_compile(config_path, &logger).await?;

        let stats = Arc::new(Mutex::new(Stats {
            targets: config
                .targets
                .iter()
                .map(|t| TargetStats {
                    id: t.id,
                    url: t.url.clone(),
                    success: 0,
                    failure: 0,
                    last_success_time: None,
                    last_failure_time: None,
                    last_network_error: None,
                    error_details: Vec::new(),
                    error_rate: 0.0,
                })
                .collect(),
            threads: Vec::new(),
            success: std::sync::atomic::AtomicU64::new(0),
            failure: std::sync::atomic::AtomicU64::new(0),
            total: std::sync::atomic::AtomicU64::new(0),
            start_time: Instant::now(),
            last_success_time: None,
            last_failure_time: None,
            sys: System::new_all(),
            cpu_usage: 0.0,
            memory_usage: 0,
            proxy_count: config.proxies.len(),
            running_state: RunningState::Running,
            debug_logs: VecDeque::with_capacity(if cli_mode { 0 } else { 1000 }),
            rps_history: VecDeque::with_capacity(120),
            successful_requests_per_second_history: VecDeque::with_capacity(120),
            success_rate_history: VecDeque::with_capacity(120),
        }));

        Ok(App {
            config,
            stats,
            logger,
            terminal,
            terminal_guard,
            control_tx,
            data_pool_tx: None,
            data_pool_rx: None,
            target_stats_tx,
            target_stats_rx,
            log_rx,
            worker_handles: Vec::new(),
            data_generator_handles: Vec::new(),
            data_generator_stop_signal: Arc::new(AtomicBool::new(false)),
            log_receiver_handle,
            layout_rects: LayoutRects::default(),
            stats_updater: StatsUpdater::new(),
            cli_mode,
        })
    }

    pub fn update_layout_rects(&mut self, new_rects: LayoutRects) {
        if !self.cli_mode {
            self.layout_rects = new_rects;
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.spawn_workers();
        let result = if self.cli_mode {
            crate::ui::cli::run_cli(self).await
        } else {
            crate::ui::run_tui(self).await
        };
        self.restore_terminal();
        result?;
        self.shutdown_components().await;
        Ok(())
    }

    fn restore_terminal(&mut self) {
        self.terminal.take();
        self.terminal_guard.take();
    }

    pub fn stats_arc(&self) -> Arc<Mutex<Stats>> {
        self.stats.clone()
    }
}

use crate::config::loader::{self, load_config_and_compile};
use crate::data_generator;
use crate::logger::Logger;
use crate::ui::stats_updater::StatsUpdater;
use crate::ui::{DebugInfo, LayoutRects};
use crate::ui::{RunningState, Stats, TargetStats};
use crate::worker::{PreGeneratedRequest, TargetUpdate, WorkerMessage, worker_loop};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::sync::mpsc as std_mpsc;
use std::{
    collections::VecDeque,
    error::Error,
    io::{self, Stdout},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use sysinfo::System;
use tokio::runtime::Handle;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

pub struct App {
    pub config: loader::AttackConfig,
    pub stats: Arc<TokioMutex<Stats>>,
    pub logger: Logger,
    pub terminal: Option<Terminal<CrosstermBackend<Stdout>>>,
    pub control_tx: broadcast::Sender<WorkerMessage>,
    data_pool_tx: Option<mpsc::Sender<PreGeneratedRequest>>,
    data_pool_rx: Option<Arc<TokioMutex<mpsc::Receiver<PreGeneratedRequest>>>>, // Use TokioMutex
    pub target_stats_tx: mpsc::Sender<TargetUpdate>,
    pub target_stats_rx: mpsc::Receiver<TargetUpdate>,
    log_rx: Option<std_mpsc::Receiver<DebugInfo>>,
    worker_handles: Vec<JoinHandle<()>>,
    pub data_generator_handles: Vec<JoinHandle<()>>,
    pub data_generator_stop_signal: Arc<AtomicBool>,
    log_receiver_handle: Option<thread::JoinHandle<()>>,
    pub layout_rects: LayoutRects,
    pub stats_updater: StatsUpdater,
    pub cli_mode: bool,
}

impl App {
    pub fn new(config_path: &str, cli_mode: bool) -> Result<Self, Box<dyn Error>> {
        let config = load_config_and_compile(config_path)?;

        let mut terminal = None;
        let mut log_rx = None;
        let log_tx_for_logger;

        if !cli_mode {
            enable_raw_mode()?;
            let mut stdout = io::stdout();
            execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
            let backend = CrosstermBackend::new(stdout);
            let mut term_instance = Terminal::new(backend)?;
            term_instance.clear()?;
            terminal = Some(term_instance);

            let (tx, rx) = std_mpsc::channel();
            log_tx_for_logger = Some(tx);
            log_rx = Some(rx);
        } else {
            log_tx_for_logger = None;
        }

        let (control_tx, _) = broadcast::channel(128);
        let (target_stats_tx, target_stats_rx) = mpsc::channel(256);
        let logger = Logger::new(log_tx_for_logger, cli_mode);

        let stats = Arc::new(TokioMutex::new(Stats {
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
                    is_dying: false,
                    error_rate: 0.0,
                })
                .collect(),
            threads: Vec::new(), // This might be unused now or could represent something else
            success: 0,
            failure: 0,
            total: 0,
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
            // App::new's Ok block starts here
            config,
            stats,
            logger,
            terminal,
            control_tx,
            data_pool_tx: None,
            data_pool_rx: None,
            target_stats_tx,
            target_stats_rx,
            log_rx,
            worker_handles: Vec::new(), // Correctly initialize worker_handles
            data_generator_handles: Vec::new(),
            data_generator_stop_signal: Arc::new(AtomicBool::new(false)),
            log_receiver_handle: None,
            layout_rects: LayoutRects::default(),
            stats_updater: StatsUpdater::new(),
            cli_mode,
        }) // App::new's Ok block ends here
    } // App::new method ends here

    // spawn_data_generators is defined *after* App::new
    pub fn spawn_data_generators(&mut self) {
        let generator_threads = self.config.generator_threads;
        self.logger.info(&format!(
            "Spawning {} data generator tasks...",
            generator_threads
        ));

        let pool_size = self.config.threads * 50;
        let (data_pool_tx, data_pool_rx) = mpsc::channel(pool_size);
        self.data_pool_tx = Some(data_pool_tx);
        self.data_pool_rx = Some(Arc::new(TokioMutex::new(data_pool_rx))); // Use TokioMutex

        self.data_generator_stop_signal
            .store(false, Ordering::SeqCst);

        // 均分targets
        let mut target_chunks: Vec<Vec<usize>> = vec![Vec::new(); generator_threads];
        for (i, t) in self.config.targets.iter().enumerate() {
            target_chunks[i % generator_threads].push(t.id);
        }

        for i in 0..generator_threads {
            let cfg = self.config.clone();
            let pool_tx_clone = self
                .data_pool_tx
                .as_ref()
                .expect("Data pool sender should be initialized")
                .clone();
            let logger_clone = self.logger.clone();
            let stop_signal_clone = self.data_generator_stop_signal.clone();
            let my_target_ids = target_chunks[i].clone();
            let stats_arc = Arc::clone(&self.stats_arc());

            let handle = tokio::spawn(async move {
                data_generator::data_generator_loop(
                    i,
                    cfg,
                    my_target_ids,
                    pool_tx_clone,
                    logger_clone,
                    stop_signal_clone,
                    stats_arc,
                )
                .await;
            });
            self.data_generator_handles.push(handle);
        }
    }

    pub fn spawn_workers(&mut self) {
        self.logger.info(&format!(
            "Spawning {} worker threads...",
            self.config.threads
        ));
        for _i in 0..self.config.threads {
            let control_rx = self.control_tx.subscribe();
            let data_pool_rx_clone = self
                .data_pool_rx
                .clone()
                .expect("Data pool receiver should be initialized before spawning workers");
            let cfg = self.config.clone();
            let worker_logger = self.logger.clone();
            let stats_tx = self.target_stats_tx.clone();
            let handle = tokio::spawn(async move {
                worker_loop(
                    control_rx,
                    data_pool_rx_clone,
                    cfg,
                    std::thread::current().id(),
                    worker_logger.clone(),
                    stats_tx,
                )
                .await;
            });
            self.worker_handles.push(handle);
        }
    }

    pub fn spawn_log_receiver(&mut self) {
        if self.cli_mode {
            return;
        }
        self.logger
            .info("Spawning log receiver thread (TUI mode)...");
        let log_rx_taken = self
            .log_rx
            .take()
            .expect("Log receiver already taken or not initialized for TUI");
        let debug_logs_tx = self.target_stats_tx.clone();
        let logger_clone = self.logger.clone();

        let handle = thread::spawn(move || {
            logger_clone.info("日志接收线程已启动");
            loop {
                match log_rx_taken.try_recv() {
                    Ok(log_entry) => {
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
                            eprintln!(
                                "Log receiver: UI channel (debug_logs_tx) send failed, exiting log receiver loop."
                            );
                            break;
                        }
                    }
                    Err(std_mpsc::TryRecvError::Empty) => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(std_mpsc::TryRecvError::Disconnected) => {
                        logger_clone
                            .info("Log receiver: Logger sender disconnected, exiting loop.");
                        break;
                    }
                }
            }
            logger_clone.info("Log receiver thread loop finished.");
        });
        self.log_receiver_handle = Some(handle);
    }

    pub fn update_layout_rects(&mut self, new_rects: LayoutRects) {
        if !self.cli_mode {
            self.layout_rects = new_rects;
        }
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.spawn_workers();
        if self.cli_mode {
            crate::ui::cli::run_cli(self).await?;
        } else {
            crate::ui::run_tui(self).await?;
        }
        self.shutdown_components().await;
        Ok(())
    }

    async fn shutdown_components(&mut self) {
        Handle::current().block_on(self.stats.lock()).running_state = RunningState::Stopping;
        self.logger.info("Shutting down application components...");

        self.logger.info("Sending global stop signal to workers...");
        let _ = self.control_tx.send(WorkerMessage::Stop);

        self.logger.info("Signaling data generators to stop...");
        self.data_generator_stop_signal
            .store(true, Ordering::SeqCst);
        if self.data_pool_tx.is_some() {
            self.data_pool_tx.take();
            self.logger
                .info("Data pool sender dropped during shutdown.");
        }

        self.logger
            .info("Waiting for data generator tasks to finish...");
        for (i, handle) in self.data_generator_handles.drain(..).enumerate() {
            self.logger
                .info(&format!("Waiting for data generator task {}...", i));
            if let Err(e) = handle.await {
                self.logger
                    .error(&format!("Data generator task {} panicked: {:?}", i, e));
            } else {
                self.logger
                    .info(&format!("Data generator task {} finished.", i));
            }
        }
        self.logger.info("All data generator tasks finished.");

        self.logger.info("Waiting for worker tasks to finish...");
        for (i, handle) in self.worker_handles.drain(..).enumerate() {
            self.logger
                .info(&format!("Waiting for worker task {}...", i));
            if let Err(e) = handle.await {
                self.logger
                    .error(&format!("Worker task {} panicked: {:?}", i, e));
            } else {
                self.logger.info(&format!("Worker task {} finished.", i));
            }
        }
        self.logger.info("All worker tasks finished.");

        if !self.cli_mode {
            self.logger
                .info("Closing logger's TUI sender to allow log_receiver to stop...");
            self.logger.close_sender();

            self.logger
                .info("Logger's TUI sender closed. Waiting for log receiver thread...");
            if let Some(handle) = self.log_receiver_handle.take() {
                if let Err(e) = handle.join() {
                    self.logger
                        .error(&format!("Log receiver thread panicked: {:?}", e));
                } else {
                    self.logger.info("Log receiver thread finished.");
                }
            }
        }

        if !self.cli_mode {
            self.logger
                .info("Cleaning up TUI resources (restoring terminal)...");
            if let Some(terminal) = self.terminal.as_mut() {
                let _ = execute!(
                    terminal.backend_mut(),
                    LeaveAlternateScreen,
                    DisableMouseCapture
                );
                let _ = terminal.show_cursor();
            }
            let _ = disable_raw_mode();
            self.terminal.take();
            self.logger.info("TUI resources cleaned up.");
        }
        self.logger.info("All components shut down.");
    }

    pub fn cleanup(&mut self) -> Result<(), Box<dyn Error>> {
        if !self.cli_mode {
            self.logger.info("Restoring terminal state (TUI mode)...");
            if let Some(terminal) = self.terminal.as_mut() {
                execute!(
                    terminal.backend_mut(),
                    LeaveAlternateScreen,
                    DisableMouseCapture
                )?;
                terminal.show_cursor()?;
            }
            disable_raw_mode()?;
            self.logger.info("Terminal state restored.");
        } else {
            self.logger.info("CLI mode: No terminal state to restore.");
        }
        Ok(())
    }

    pub fn stats_arc(&self) -> Arc<TokioMutex<Stats>> {
        self.stats.clone()
    }

    pub async fn manage_data_generator(&mut self) {
        let running_state = {
            let stats = self.stats.lock().await;
            stats.running_state
        };
        let currently_stopped = self.data_generator_stop_signal.load(Ordering::SeqCst);
        if running_state == RunningState::Running {
            if currently_stopped {
                self.logger
                    .info("Data generators were stopped, signaling them to resume.");
                self.data_generator_stop_signal
                    .store(false, Ordering::SeqCst);
                if self.data_generator_handles.is_empty() && !self.config.start_paused {
                    self.logger.info("No active data generator handles found while trying to resume. Spawning new generators.");
                    self.spawn_data_generators();
                }
            } else if self.data_generator_handles.is_empty() && !self.config.start_paused {
                self.logger.info("No active data generator handles found and not explicitly stopped. Spawning new generators.");
                self.spawn_data_generators();
            }
        } else {
            if !currently_stopped {
                self.logger.info("Application state changed (Paused/Stopping), signaling data generators to stop.");
                self.data_generator_stop_signal
                    .store(true, Ordering::SeqCst);
            }
        }
    }
}

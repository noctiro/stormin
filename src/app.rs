use crate::config::loader::{self, load_config_and_compile};
use crate::data_generator::DataGenerator;
use crate::ui::event_handler::{AppAction, handle_event};
use crate::logger::Logger;
use crate::ui::stats_updater::StatsUpdater;
use crate::ui::{DebugInfo, LayoutRects};
use crate::ui::{RunningState, Stats, TargetStats, draw_ui};
use crate::worker::{PreGeneratedRequest, TargetUpdate, WorkerMessage, worker_loop};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    collections::VecDeque,
    error::Error,
    io::{self, Stdout},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use std::sync::mpsc as std_mpsc;
use sysinfo::System;
use tokio::sync::{broadcast, mpsc};
use tokio::{task::JoinHandle, time::sleep};

pub struct App {
    config: loader::AttackConfig,
    pub stats: Stats,
    pub logger: Logger,
    terminal: Option<Terminal<CrosstermBackend<Stdout>>>, // Optional for CLI mode
    pub control_tx: broadcast::Sender<WorkerMessage>,
    data_tx: broadcast::Sender<PreGeneratedRequest>,
    target_stats_tx: mpsc::Sender<TargetUpdate>,
    target_stats_rx: mpsc::Receiver<TargetUpdate>,
    log_rx: Option<std_mpsc::Receiver<DebugInfo>>, // Only used in TUI mode
    worker_handles: Vec<JoinHandle<()>>,
    pub data_generator: DataGenerator,
    log_receiver_handle: Option<thread::JoinHandle<()>>, // Only used in TUI mode
    pub layout_rects: LayoutRects, // Only used in TUI mode
    stats_updater: StatsUpdater,
    cli_mode: bool, // To indicate if running in CLI mode
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
        let (data_tx, _) = broadcast::channel(1024);
        let (target_stats_tx, target_stats_rx) = mpsc::channel(256);

        let logger = Logger::new(log_tx_for_logger, cli_mode); // Ensure this line is exactly as expected
        let data_generator = DataGenerator::new(config.clone(), data_tx.clone(), logger.clone());

        let stats = Stats {
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
                })
                .collect(),
            threads: Vec::new(),
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
            debug_logs: VecDeque::with_capacity(if cli_mode { 0 } else { 1000 }), // No debug logs stored in CLI
            rps_history: VecDeque::with_capacity(120),
            successful_requests_per_second_history: VecDeque::with_capacity(120),
            success_rate_history: VecDeque::with_capacity(120),
        };

        Ok(App {
            config,
            stats,
            logger,
            terminal,
            control_tx,
            data_tx,
            target_stats_tx,
            target_stats_rx,
            log_rx,
            worker_handles: Vec::new(),
            data_generator,
            log_receiver_handle: None,
            layout_rects: LayoutRects::default(),
            stats_updater: StatsUpdater::new(),
            cli_mode,
        })
    }

    pub fn spawn_workers(&mut self) {
        self.logger.info(&format!(
            "Spawning {} worker threads...",
            self.config.threads
        ));
        for _i in 0..self.config.threads {
            let control_rx = self.control_tx.subscribe();
            let data_rx = self.data_tx.subscribe();
            let cfg = self.config.clone();
            let worker_logger = self.logger.clone();
            let stats_tx = self.target_stats_tx.clone();
            let handle = tokio::spawn(async move {
                worker_loop(
                    control_rx,
                    data_rx,
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
            return; // Log receiver is not used in CLI mode
        }
        self.logger.info("Spawning log receiver thread (TUI mode)...");
        let log_rx_taken = self.log_rx.take().expect("Log receiver already taken or not initialized for TUI");
        let debug_logs_tx = self.target_stats_tx.clone(); // This sends to stats updater
        let logger_clone = self.logger.clone();

        let handle = thread::spawn(move || {
            logger_clone.info("日志接收线程已启动");
            loop {
                match log_rx_taken.try_recv() {
                    Ok(log_entry) => {
                        // In TUI mode, DebugInfo is sent to Stats for display
                        let update = TargetUpdate {
                            id: 0, // Special ID for app-level logs
                            url: String::new(),
                            success: false,
                            timestamp: log_entry.timestamp, // Assuming DebugInfo has Instant
                            debug: Some(log_entry.message),
                            network_error: None,
                            thread_id: std::thread::current().id(), // Not strictly necessary here
                        };
                        if debug_logs_tx.blocking_send(update).is_err() {
                            // If sending to the UI update channel fails,
                            // it likely means the UI/main loop is shutting down.
                            eprintln!("Log receiver: UI channel (debug_logs_tx) send failed, exiting log receiver loop.");
                            break; // Exit the loop
                        }
                    }
                    Err(std_mpsc::TryRecvError::Empty) => {
                        // The channel is empty. Sleep for a short duration to avoid busy-waiting.
                        // The loop will continue to check for new messages.
                        // We need to ensure this thread can be joined during shutdown.
                        // If control_tx is used to signal shutdown, we might need to check it here too,
                        // or rely on the sender disconnecting.
                        // For now, simple sleep.
                        thread::sleep(Duration::from_millis(50)); // Adjust sleep duration as needed
                    }
                    Err(std_mpsc::TryRecvError::Disconnected) => {
                        // The sender (Logger's sender) has been disconnected.
                        // This is the primary signal for this thread to terminate.
                        logger_clone.info("Log receiver: Logger sender disconnected, exiting loop.");
                        break; // Exit the loop
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

    async fn run_tui(&mut self) -> Result<(), Box<dyn Error>> {
        self.logger.info("Starting TUI application loop.");
        let mut last_draw_time = Instant::now();
        let redraw_interval = Duration::from_millis(100);
        let mut needs_redraw = true;

        let running = std::sync::Arc::new(AtomicBool::new(true));
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
            // Optionally, send a quit event or directly signal the app to stop
        })?;
        
        let terminal = self.terminal.as_mut().ok_or("Terminal not initialized for TUI mode")?;

        match draw_ui(terminal, &self.stats) {
            Ok(all_rects) => self.update_layout_rects(all_rects),
            Err(e) => {
                self.logger.error(&format!("Initial TUI draw failed: {}", e));
                return Err(e.into());
            }
        }

        while running.load(Ordering::SeqCst) {
            let mut received_input_or_event = false;
            if event::poll(Duration::from_millis(50))? {
                received_input_or_event = true;
                let event_read = event::read()?;
                let (redraw_from_event, app_action) = handle_event(self, event_read);
                needs_redraw = redraw_from_event || needs_redraw;

                match app_action {
                    AppAction::Quit => {
                        self.logger.info("Quit action received. Signaling workers to stop and exiting immediately.");
                        // Attempt to signal workers to stop. This is a "best effort" before immediate exit.
                        let _ = self.control_tx.send(WorkerMessage::Stop);

                        // Minimal terminal restoration before exiting.
                        // This is to prevent messing up the user's terminal on abrupt exit.
                        if let Some(terminal) = self.terminal.as_mut() {
                            let _ = execute!(
                                terminal.backend_mut(),
                                LeaveAlternateScreen,
                                DisableMouseCapture
                            );
                            let _ = terminal.show_cursor();
                        }
                        let _ = disable_raw_mode(); // Attempt to disable raw mode.

                        self.logger.info("Exiting application now via std::process::exit(0).");
                        std::process::exit(0); // Exit the entire process immediately.
                    }
                    _ => {} // Other actions are handled by modifying app.stats or sending messages
                }
            }

            if !received_input_or_event {
                let stats_updated = self.stats_updater.update_stats(
                    &mut self.stats,
                    &mut self.target_stats_rx,
                    &self.logger,
                );
                if stats_updated {
                    needs_redraw = true;
                }
            }
            
            // Manage data generator (common logic, could be a helper)
            self.manage_data_generator();


            if needs_redraw || last_draw_time.elapsed() >= redraw_interval {
                 let terminal_mut = self.terminal.as_mut().ok_or("Terminal not available for TUI draw")?;
                match draw_ui(terminal_mut, &self.stats) {
                    Ok(all_rects) => {
                        self.update_layout_rects(all_rects);
                        last_draw_time = Instant::now();
                        needs_redraw = false;
                    }
                    Err(e) => {
                        self.logger.error(&format!("Failed to draw TUI: {}", e));
                        running.store(false, Ordering::SeqCst);
                    }
                }
            }

            if !received_input_or_event && !needs_redraw {
                sleep(Duration::from_millis(10)).await;
            }
        }
        Ok(())
    }

    fn print_cli_stats(&self) {
        // Simple CLI output, can be expanded
        println!(
            "{} ----- Stats ----- {}",
            chrono::Utc::now().to_rfc3339(),
            self.config.run_duration.map_or_else(
                || "".to_string(),
                |d| format!("(remaining: {:?})", d.saturating_sub(self.stats.start_time.elapsed()))
            )
        );
        println!(
            "Total: {}, Success: {}, Failure: {}, RPS: {}", // Changed {:.2} to {}
            self.stats.total,
            self.stats.success,
            self.stats.failure,
            // Pass the u64 value directly, remove unnecessary dereference
            self.stats.rps_history.back().copied().unwrap_or(0u64)
        );
        for target_stat in &self.stats.targets {
            println!(
                "  Target {}: Success: {}, Failure: {}",
                target_stat.id, target_stat.success, target_stat.failure
            );
        }
        println!("--------------------");
    }
    
    fn manage_data_generator(&mut self) {
        if self.stats.running_state == RunningState::Running {
            if !self.data_generator.is_running() && self.data_generator.is_finished() {
                self.logger.info("Data generator seems to have stopped, attempting to restart.");
                self.data_generator.spawn();
            } else if !self.data_generator.is_running() && !self.data_generator.is_finished() {
                 // If it's not running but not finished, it might be paused by user or config
                self.data_generator.set_running_flag(true);
                self.logger.info("Data generator was paused, attempting to resume it.");
            }
        }
    }

    async fn run_cli(&mut self) -> Result<(), Box<dyn Error>> {
        self.logger.info("Starting CLI application loop.");
        let running = std::sync::Arc::new(AtomicBool::new(true));
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
            println!("\nCtrl-C received, initiating shutdown...");
        })?;

        let print_interval = Duration::from_secs(self.config.cli_update_interval_secs.unwrap_or(2));
        let mut last_print_time = Instant::now();

        // Start data generator if not already running (e.g. if it's configured to start paused)
        if self.config.start_paused.unwrap_or(false) {
            self.logger.info("Application configured to start paused. Data generator will not start automatically.");
        } else {
             self.data_generator.spawn(); // Start data generator
        }


        while running.load(Ordering::SeqCst) {
            // Check for runtime duration limit
            if let Some(duration) = self.config.run_duration {
                if self.stats.start_time.elapsed() >= duration {
                    self.logger.info(&format!(
                        "Configured run duration of {:?} reached. Stopping.",
                        duration
                    ));
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }

            // Update stats from workers
            let _stats_updated = self.stats_updater.update_stats(
                &mut self.stats,
                &mut self.target_stats_rx,
                &self.logger, // Logger is now CLI-aware
            );
            
            // Manage data generator
            self.manage_data_generator();


            // Print stats periodically
            if last_print_time.elapsed() >= print_interval {
                self.print_cli_stats();
                last_print_time = Instant::now();
            }

            // Minimal sleep to prevent 100% CPU usage
            sleep(Duration::from_millis(100)).await;
        }
        self.print_cli_stats(); // Print final stats before exiting
        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        if self.cli_mode {
            self.run_cli().await?;
        } else {
            self.run_tui().await?;
        }
        // Common shutdown logic after TUI or CLI loop finishes
        self.shutdown_components().await;
        Ok(())
    }
    
    async fn shutdown_components(&mut self) {
        self.stats.running_state = RunningState::Stopping;
        self.logger.info("Shutting down application components...");

        // 1. 发送全局停止信号给 worker
        self.logger.info("Sending global stop signal to workers...");
        let _ = self.control_tx.send(WorkerMessage::Stop); // Workers will observe this
        
        // 2. 停止数据生成器
        // Data generator also observes WorkerMessage::Stop if its control_rx is subscribed to control_tx
        // or needs its own stop mechanism if independent. Assuming data_generator.stop() handles its shutdown.
        self.logger.info("Stopping data generator...");
        self.data_generator.stop().await; // Ensure this properly waits for the generator to finish
        
        // 3. Wait for worker tasks to finish
        // Workers should exit their loops upon receiving WorkerMessage::Stop
        self.logger.info("Waiting for worker tasks to finish...");
        // Iterate over handles and await them. drain() consumes the vector.
        for (i, handle) in self.worker_handles.drain(..).enumerate() {
            self.logger.info(&format!("Waiting for worker task {}...", i));
            if let Err(e) = handle.await {
                // Log error if a worker task panicked
                self.logger.error(&format!("Worker task {} panicked: {:?}", i, e));
            } else {
                self.logger.info(&format!("Worker task {} finished.", i));
            }
        }
        self.logger.info("All worker tasks finished.");

        // 4. Close the logger's TUI sender and wait for the log receiver thread
        if !self.cli_mode {
            self.logger.info("Closing logger's TUI sender to allow log_receiver to stop...");
            self.logger.close_sender(); // This makes the sender None and signals the log_receiver
            // self.log_rx = None; // This was already taken by spawn_log_receiver

            self.logger.info("Logger's TUI sender closed. Waiting for log receiver thread...");
            if let Some(handle) = self.log_receiver_handle.take() {
                // The log_receiver loop should exit when the sender is disconnected.
                // handle.thread().unpark(); // Unparking might not be necessary if try_recv + sleep is used
                if let Err(e) = handle.join() {
                    self.logger.error(&format!("Log receiver thread panicked: {:?}", e));
                } else {
                    self.logger.info("Log receiver thread finished.");
                }
            }
        }

        // 5. 清理TUI资源 (terminal restoration)
        // This should happen after all threads that might interact with the TUI are stopped.
        if !self.cli_mode {
            self.logger.info("Cleaning up TUI resources (restoring terminal)...");
            // 恢复终端设置
            // It's safer to call disable_raw_mode and LeaveAlternateScreen
            // after all other operations that might depend on the raw mode or alternate screen.
            // The cleanup() method in main.rs also does this, consider centralizing.
            // For now, ensure it's done here as part of shutdown.
            if let Some(terminal) = self.terminal.as_mut() {
                 let _ = execute!(
                    terminal.backend_mut(),
                    LeaveAlternateScreen,
                    DisableMouseCapture
                );
                let _ = terminal.show_cursor();
            }
            let _ = disable_raw_mode(); // Always try to disable raw mode if it was enabled.
            
            self.terminal.take(); // Drop the terminal instance.
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
            // disable_raw_mode should be called regardless of terminal instance existence
            // if enable_raw_mode was called.
            disable_raw_mode()?;
            self.logger.info("Terminal state restored.");
        } else {
            self.logger.info("CLI mode: No terminal state to restore.");
        }
        Ok(())
    }
}

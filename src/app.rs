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
    terminal: Terminal<CrosstermBackend<Stdout>>,
    pub control_tx: broadcast::Sender<WorkerMessage>,
    data_tx: broadcast::Sender<PreGeneratedRequest>, // data_tx can remain private
    target_stats_tx: mpsc::Sender<TargetUpdate>,     // target_stats_tx can remain private (tokio mpsc)
    target_stats_rx: mpsc::Receiver<TargetUpdate>,   // target_stats_rx can remain private (tokio mpsc)
    log_rx: Option<std_mpsc::Receiver<DebugInfo>>,   // Changed to std_mpsc for log channel
    worker_handles: Vec<JoinHandle<()>>,             // worker_handles can remain private
    pub data_generator: DataGenerator,
    log_receiver_handle: Option<thread::JoinHandle<()>>, // log_receiver_handle can remain private
    pub layout_rects: LayoutRects,
    stats_updater: StatsUpdater,
    // log_receiver_should_stop: Arc<AtomicBool>, // Removed
}

impl App {
    pub fn new(config_path: &str) -> Result<Self, Box<dyn Error>> {
        let config = load_config_and_compile(config_path)?;

        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let (control_tx, _) = broadcast::channel(128); // Control channel for workers (Pause, Resume, Stop)
        let (data_tx, _) = broadcast::channel(1024); // Data channel for pre-generated requests, larger buffer
        let (target_stats_tx, target_stats_rx) = mpsc::channel(256); // Stats channel (tokio mpsc), increased buffer
        let (log_tx, log_rx) = std_mpsc::channel(); // Log channel (std_mpsc), default buffer

        let logger = Logger::new(log_tx.clone()); // logger for App and other components, will now take std_mpsc::Sender
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
            debug_logs: VecDeque::with_capacity(1000),
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
            log_rx: Some(log_rx),
            worker_handles: Vec::new(),
            data_generator, // Initialize DataGenerator
            log_receiver_handle: None,
            layout_rects: LayoutRects::default(),
            stats_updater: StatsUpdater::new(), // Initialize StatsUpdater
        })
    }

    pub fn spawn_workers(&mut self) {
        self.logger.info(&format!(
            "Spawning {} worker threads...",
            self.config.threads
        ));
        for _i in 0..self.config.threads {
            // Changed i to _i as it's not used in the loop body currently
            let control_rx = self.control_tx.subscribe();
            let data_rx = self.data_tx.subscribe(); // Workers subscribe to pre-generated data
            let cfg = self.config.clone();
            let worker_logger = self.logger.clone();
            let stats_tx = self.target_stats_tx.clone();
            let handle = tokio::spawn(async move {
                worker_loop(
                    control_rx,
                    data_rx,
                    cfg,
                    std::thread::current().id(), // This is the ThreadId of the underlying OS thread executing the task
                    worker_logger.clone(),
                    stats_tx,
                )
                .await;
            });
            self.worker_handles.push(handle);
        }
    }

    pub fn spawn_log_receiver(&mut self) {
        self.logger.info("Spawning log receiver thread...");
        let log_rx = self.log_rx.take().expect("Log receiver already taken");
        let debug_logs_tx = self.target_stats_tx.clone();
        let logger_clone = self.logger.clone();
        // let should_stop_clone = self.log_receiver_should_stop.clone(); // Removed

        let handle = thread::spawn(move || {
            logger_clone.info("Log receiver thread started.");
            // Changed loop to use blocking_recv, which returns None when channel is closed
            // Changed loop to use recv(), which returns Result<T, RecvError>
            // The loop will terminate if recv() returns an Err (e.g., channel closed)
            while let Ok(log_entry) = log_rx.recv() {
                let update = TargetUpdate {
                    id: 0, // id 0 can be used for general app logs or a specific "app" target
                    url: String::new(), // No specific URL for a general log entry
                    success: false, // Not applicable for a log entry
                    timestamp: log_entry.timestamp,
                    debug: Some(log_entry.message),
                    network_error: None, // Not applicable
                    thread_id: std::thread::current().id(), // Log which thread originated this log
                };
                // Using blocking_send here is okay as this is a std::thread
                if debug_logs_tx.blocking_send(update).is_err() {
                    // If the UI channel is closed, there's no point in continuing
                    eprintln!("Log receiver: UI channel send failed, exiting loop.");
                    break;
                }
            }
            // When loop exits, it means log_rx was closed or send failed.
            logger_clone.info("Log receiver thread loop finished (channel closed or send error).");
        });
        self.log_receiver_handle = Some(handle);
    }

    // Helper to update layout rects after drawing (or before mouse event processing)
    pub fn update_layout_rects(&mut self, new_rects: LayoutRects) {
        self.layout_rects = new_rects;
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.logger.info("Starting main application loop.");
        let mut last_draw_time = Instant::now();
        let redraw_interval = Duration::from_millis(100);
        let mut needs_redraw = true;

        let running = std::sync::Arc::new(AtomicBool::new(true));
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        })?;

        // Initial draw to get layout rects
        // This is a bit of a chicken-and-egg, draw_ui needs to return the rects
        // For now, we'll assume draw_ui is modified to do so or we use estimates.
        // A better way is to have draw_ui return the rects or store them in App from draw_ui.
        match draw_ui(&mut self.terminal, &self.stats) {
            Ok(all_rects) => {
                self.update_layout_rects(all_rects);
            }
            Err(e) => {
                self.logger.error(&format!("Initial draw failed: {}", e));
                return Err(e.into());
            }
        }

        while running.load(Ordering::SeqCst) {
            let mut received_input_or_event = false;
            if event::poll(Duration::from_millis(50))? {
                received_input_or_event = true;
                let event_read = event::read()?;
                let (redraw_from_event, app_action) = handle_event(self, event_read);
                needs_redraw = redraw_from_event || needs_redraw; // Combine redraw flags

                match app_action {
                    AppAction::Quit => {
                        running.store(false, Ordering::SeqCst);
                    }
                    AppAction::Pause | AppAction::Resume | AppAction::NoAction => {
                        // Pause, Resume, and NoAction are handled by handle_event or require no specific action here.
                    }
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

                // Check if data generator needs to be managed (restarted/resumed)
                // This logic might also fit better within a dedicated manager or App method
                if self.stats.running_state == RunningState::Running {
                    if !self.data_generator.is_running() && self.data_generator.is_finished() {
                        self.logger.info("Data generator seems to have stopped unexpectedly, attempting to restart.");
                        self.data_generator.spawn();
                    } else if !self.data_generator.is_running() {
                        self.data_generator.set_running_flag(true);
                        self.logger
                            .info("Data generator was paused, attempting to resume it.");
                    }
                }
            }

            if needs_redraw || last_draw_time.elapsed() >= redraw_interval {
                match draw_ui(&mut self.terminal, &self.stats) {
                    Ok(all_rects) => {
                        self.update_layout_rects(all_rects);
                        last_draw_time = Instant::now();
                        needs_redraw = false;
                    }
                    Err(e) => {
                        self.logger.error(&format!("Failed to draw UI: {}", e));
                        running.store(false, Ordering::SeqCst); // Stop on draw error
                    }
                }
            }

            // Minimal sleep if nothing else happened, to prevent 100% CPU usage
            if !received_input_or_event && !needs_redraw {
                sleep(Duration::from_millis(10)).await;
            }
        }
        // The while loop closed above. The run() method continues.

        self.stats.running_state = RunningState::Stopping;
        self.logger
            .info("Shutting down data generator and workers...");

        // Stop data generator first
        self.data_generator.stop().await; // Call the async stop method

        // Then stop workers
        if let Err(e) = self.control_tx.send(WorkerMessage::Stop) {
            self.logger.warning(&format!(
                "Failed to broadcast Stop message to workers: {}",
                e
            ));
        }

        // Wait for worker handles
        self.logger.info("Waiting for worker threads to finish...");
        for (i, handle) in self.worker_handles.drain(..).enumerate() {
            if let Err(e) = handle.await {
                self.logger
                    .error(&format!("Worker task {} panicked: {:?}", i, e));
            }
        }
    self.logger.info("All worker threads finished.");

    // Signal and wait for log receiver thread if it's running
    if let Some(handle) = self.log_receiver_handle.take() {
        self.logger // Assuming self.logger is still directly accessible for now
            .info("Waiting for log receiver thread to finish (it will exit when all log_tx are dropped)...");
        if let Err(e) = handle.join() {
            self.logger
                .error(&format!("Log receiver thread panicked: {:?}", e));
        } else {
            self.logger.info("Log receiver thread finished.");
        }
    }

    Ok(())
}
    pub fn cleanup(&mut self) -> Result<(), Box<dyn Error>> {
        self.logger.info("Restoring terminal state...");
        // It's good practice to ensure raw mode is disabled and screen restored
        // even if `run` returned an error.
        let terminal_backend_mut = self.terminal.backend_mut();
        execute!(
            terminal_backend_mut,
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        self.terminal.show_cursor()?;
        disable_raw_mode()?;
        Ok(())
    }
}

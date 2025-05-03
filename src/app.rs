use crate::config::{self, load_config_and_compile};
use crate::logger::Logger;
use crate::ui::DebugInfo;
use crate::ui::{self, draw_ui, RunningState, Stats, TargetStats, ThreadStats};
use crate::worker::{worker_loop, RequestResult, TargetUpdate, WorkerMessage};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseButton, MouseEventKind,
    },
    execute, // Explicitly import execute macro
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::{broadcast, mpsc};
use std::{
    collections::VecDeque,
    error::Error,
    io::{self, Stdout},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use sysinfo::System;
use tokio::{task::JoinHandle, time::sleep}; // Removed timeout and TokioDuration

pub struct App {
    config: config::AttackConfig,
    stats: Stats,
    logger: Logger,
    terminal: Terminal<CrosstermBackend<Stdout>>,
    // Channels
    // Use broadcast::Sender for task control
    task_tx: broadcast::Sender<WorkerMessage>,
    // task_rx removed, workers subscribe directly
    stat_tx: mpsc::Sender<RequestResult>,
    stat_rx: mpsc::Receiver<RequestResult>,
    thread_stats_tx: mpsc::Sender<ThreadStats>,
    thread_stats_rx: mpsc::Receiver<ThreadStats>,
    target_stats_tx: mpsc::Sender<TargetUpdate>,
    target_stats_rx: mpsc::Receiver<TargetUpdate>,
    // log_tx: Sender<DebugInfo>, // Removed unused field (held by logger)
    // Receiver is Option because it's taken by spawn_log_receiver
    log_rx: Option<mpsc::Receiver<DebugInfo>>,
    // Removed pause_tx, pause_rx
    // Threads
    worker_handles: Vec<JoinHandle<()>>, // Tokio JoinHandle
    // Removed task_sender_handle
    log_receiver_handle: Option<thread::JoinHandle<()>>, // Keep std JoinHandle for sync log thread
}

impl App {
    pub fn new(config_path: &str) -> Result<Self, Box<dyn Error>> {
        // Load Config
        let config = load_config_and_compile(config_path)?;

        // Setup TUI
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?; // Use imported macro directly
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        // Create Channels
        let (task_tx, _) = broadcast::channel(32);
        let (stat_tx, stat_rx) = mpsc::channel(32);
        let (thread_stats_tx, thread_stats_rx) = mpsc::channel(32);
        let (target_stats_tx, target_stats_rx) = mpsc::channel(32);
        let (log_tx, log_rx) = mpsc::channel(32);
        // Removed pause_tx, pause_rx creation

        // Create Logger
        let logger = Logger::new(log_tx.clone());

        // Initialize Stats
        let stats = Stats {
            targets: config
                .targets
                .iter()
                .map(|t| TargetStats {
                    url: t.url.clone(),
                    success: 0,
                    failure: 0,
                    last_success_time: None,
                    last_failure_time: None,
                })
                .collect(),
            threads: Vec::new(),
            success: 0,
            failure: 0,
            total: 0,
            start_time: Instant::now(),
            last_success_time: None,
            last_failure_time: None,
            sys: System::new_all(), // Use new_all for initial full refresh
            cpu_usage: 0.0,
            memory_usage: 0,
            proxy_count: config.proxies.len(), // Initialize proxy count
            running_state: RunningState::Running,
            debug_logs: VecDeque::with_capacity(1000),
        };

        Ok(App {
            config,
            stats,
            logger,
            terminal,
            task_tx,
            // task_rx removed
            stat_tx,
            stat_rx,
            thread_stats_tx,
            thread_stats_rx,
            target_stats_tx,
            target_stats_rx,
            // log_tx, // Removed from initialization (held by logger)
            log_rx: Some(log_rx), // Wrap receiver in Option
            // Removed pause_tx, pause_rx
            worker_handles: Vec::new(),
            // Removed task_sender_handle
            log_receiver_handle: None,
        })
    }

    pub fn spawn_workers(&mut self) {
        self.logger.info(&format!(
            "Spawning {} worker threads...",
            self.config.threads
        ));
        for _ in 0..self.config.threads {
            // Subscribe to the broadcast channel for task messages
            let rx = self.task_tx.subscribe();
            let tx = self.stat_tx.clone();
            let cfg = self.config.clone();
            let thread_stats_tx = self.thread_stats_tx.clone();
            let target_stats_tx = self.target_stats_tx.clone();
            let worker_logger = self.logger.clone();
            // Use tokio::spawn for the async worker_loop
            let handle = tokio::spawn(async move {
                let thread_id = std::thread::current().id(); // Keep for now
                worker_loop(
                    rx,
                    cfg,
                    tx,
                    thread_id,
                    thread_stats_tx,
                    target_stats_tx,
                    worker_logger,
                )
                .await // await the async worker_loop
            });
            self.worker_handles.push(handle); // handle is now tokio::task::JoinHandle
        }
    }

    // Remove the spawn_task_sender method entirely
    // pub fn spawn_task_sender(&mut self) { ... }

    pub fn spawn_log_receiver(&mut self) {
        self.logger.info("Spawning log receiver thread...");
        // Take the receiver from the Option, panicking if it's already taken
        let mut log_rx = self.log_rx.take().expect("Log receiver already taken or not initialized");
        let debug_logs_tx = self.target_stats_tx.clone(); // Use target channel for UI updates
        let logger_clone = self.logger.clone(); // Clone logger for use in the thread

        let handle = thread::spawn(move || {
             logger_clone.info("Log receiver thread started.");
            // Use blocking_recv on the tokio mpsc channel inside the std::thread
            while let Some(log_entry) = log_rx.blocking_recv() {
                // Convert log entry to a TargetUpdate for the UI
                let update = TargetUpdate {
                    url: String::new(), // Use empty URL to signify a log entry
                    success: false,     // Not applicable
                    timestamp: log_entry.timestamp,
                    debug: Some(log_entry.message),
                };
                 // Send to the target stats channel (which handles UI debug log updates)
                // Use blocking send here as this thread is dedicated to logging
                if debug_logs_tx.blocking_send(update).is_err() {
                    // Use eprintln as logger might be involved in shutdown
                    eprintln!("Log receiver failed to send to UI channel, exiting.");
                    break; // Exit if the UI channel is closed
                }
            }
            eprintln!("Log receiver thread loop finished."); // Added log for exit
        });
        self.log_receiver_handle = Some(handle);
    }

    // Make run async
    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.logger.info("Starting main application loop.");
        let mut sysinfo_tick = 0u32;
        let mut last_draw_time = Instant::now(); // Track last draw time
        let redraw_interval = Duration::from_millis(250); // Minimum redraw interval
        let mut needs_redraw = true; // Force initial draw

        // Set Ctrl+C handler
        let running = std::sync::Arc::new(AtomicBool::new(true));
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst); // Use imported Ordering
        })?;

        while running.load(Ordering::SeqCst) {
            // Use imported Ordering
            // --- Input Handling ---
            let mut received_input = false; // Track if input was received this iteration
            if event::poll(Duration::from_millis(50))? {
                // Slightly shorter poll duration
                received_input = true; // Assume input if poll returns true
                match event::read()? {
                    Event::Key(key) => {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('s') => {
                                running.store(false, Ordering::SeqCst); // Use imported Ordering
                            }
                            KeyCode::Char('p')
                                if self.stats.running_state == RunningState::Running =>
                            {
                                self.stats.running_state = RunningState::Paused;
                                self.logger.info("Pausing workers...");
                                // Send Pause message directly to workers via task_tx
                                for i in 0..self.worker_handles.len() {
                                    // Use index for logging
                                    // Send Pause message, log warning on error
                                    if let Err(e) = self.task_tx.send(WorkerMessage::Pause) {
                                        self.logger.warning(&format!(
                                            "Failed to send Pause message to worker {}: {}",
                                            i, e
                                        ));
                                    }
                                }
                            }
                            KeyCode::Char('r')
                                if self.stats.running_state == RunningState::Paused =>
                            {
                                self.stats.running_state = RunningState::Running;
                                self.logger.info("Resuming workers...");
                                // Send Resume message directly to workers via task_tx
                                for i in 0..self.worker_handles.len() {
                                    // Use index for logging
                                    // Send Resume message, log warning on error
                                    if let Err(e) = self.task_tx.send(WorkerMessage::Resume) {
                                        self.logger.warning(&format!(
                                            "Failed to send Resume message to worker {}: {}",
                                            i, e
                                        ));
                                    }
                                } // Add semicolon after the loop
                            }
                            _ => {} // Ignore other keys
                        } // End match key.code
                    } // End Event::Key arm
                    Event::Mouse(event) => {
                        if event.kind == MouseEventKind::Down(MouseButton::Left) {
                            // Try copy log entry, ignore result
                            let _ = ui::try_copy_log_entry(&self.stats, event.row);
                        }
                    }
                    // Add catch-all for other events (Resize, Focus, etc.)
                    _ => {}
                } // End match event::read()?
            }
            if received_input {
                needs_redraw = true; // Redraw if input was handled
            }

            // --- Statistics Update ---
            let mut stats_updated = false; // Track if stats were updated this iteration

            // System Info (throttled) - Revert to refresh_all
            sysinfo_tick = sysinfo_tick.wrapping_add(1);
            if sysinfo_tick % 10 == 0 {
                self.stats.sys.refresh_all(); // Use refresh_all as in original main.rs
                                              // Recalculate CPU based on refresh_all
                self.stats.cpu_usage = self
                    .stats
                    .sys
                    .cpus()
                    .iter()
                    .map(|cpu| cpu.cpu_usage())
                    .sum::<f32>()
                    / self.stats.sys.cpus().len() as f32;
                self.stats.memory_usage = self.stats.sys.used_memory();
            }

            // Thread Stats
            while let Ok(thread_stat) = self.thread_stats_rx.try_recv() {
                stats_updated = true;
                if let Some(existing) = self
                    .stats
                    .threads
                    .iter_mut()
                    .find(|t| t.id == thread_stat.id)
                {
                    *existing = thread_stat;
                } else {
                    self.stats.threads.push(thread_stat);
                }
                // Removed sorting by ThreadId as it doesn't implement Ord
            }

            // Target/Log Updates
            while let Ok(update) = self.target_stats_rx.try_recv() {
                stats_updated = true;
                if update.url.is_empty() {
                    // This is a log message
                    if let Some(debug_msg) = update.debug {
                        self.stats.debug_logs.push_back(DebugInfo {
                            timestamp: update.timestamp,
                            message: debug_msg,
                        });
                        // Keep log buffer size limited
                        while self.stats.debug_logs.len() > 1000 {
                            self.stats.debug_logs.pop_front();
                        }
                    }
                } else {
                    // This is a target update
                    if let Some(target) =
                        self.stats.targets.iter_mut().find(|t| t.url == update.url)
                    {
                        if update.success {
                            target.success += 1;
                            target.last_success_time = Some(update.timestamp);
                        } else {
                            target.failure += 1;
                            target.last_failure_time = Some(update.timestamp);
                        }
                    }
                    // Also record debug info associated with the target update if present
                    if let Some(debug_msg) = update.debug {
                        self.stats.debug_logs.push_back(DebugInfo {
                            timestamp: update.timestamp, // Use update's timestamp
                            message: debug_msg,
                        });
                        while self.stats.debug_logs.len() > 1000 {
                            self.stats.debug_logs.pop_front();
                        }
                    }
                }
            }

            // Request Results (Success/Failure)
            while let Ok(res) = self.stat_rx.try_recv() {
                stats_updated = true;
                self.stats.total += 1;
                match res {
                    RequestResult::Success => {
                        self.stats.success += 1;
                        self.stats.last_success_time = Some(Instant::now());
                    }
                    RequestResult::Failure => {
                        self.stats.failure += 1;
                        self.stats.last_failure_time = Some(Instant::now());
                    }
                }
            }
            if stats_updated {
                needs_redraw = true; // Redraw if stats were updated
            }

            // --- Send Tasks to Workers ---
            if self.stats.running_state == RunningState::Running {
                // Send Task via broadcast. Ignore result (number of receivers).
                // Error occurs only if there are no receivers.
                if let Err(e) = self.task_tx.send(WorkerMessage::Task) {
                     self.logger.warning(&format!("Failed to broadcast Task message: {}", e));
                }
            }

            // --- Draw UI ---
            let should_draw = needs_redraw || last_draw_time.elapsed() >= redraw_interval;

            if should_draw {
                // Check terminal size before drawing
                if self.terminal.size().is_err() {
                    self.logger
                        .error("Failed to get terminal size, skipping draw.");
                    // Consider stopping if terminal is unusable
                    running.store(false, Ordering::SeqCst); // Use imported Ordering
                    continue;
                }
                if let Err(e) = draw_ui(&mut self.terminal, &mut self.stats) {
                    self.logger.error(&format!("Failed to draw UI: {}", e));
                    // Consider stopping if drawing fails repeatedly
                    running.store(false, Ordering::SeqCst); // Use imported Ordering
                } else {
                    last_draw_time = Instant::now(); // Update last draw time on success
                    needs_redraw = false; // Reset redraw flag
                }
            } else {
                // If not drawing, sleep briefly to avoid busy-waiting 100% CPU
                // This sleep duration should be less than the poll duration
                // to ensure responsiveness.
                // Use tokio's async sleep instead of blocking thread::sleep
                sleep(Duration::from_millis(20)).await;
            }
        } // End of main loop (while running)

        self.stats.running_state = RunningState::Stopping;
        self.logger
            .info("Stop signal received (Ctrl+C or key press), shutting down...");
        // Send stop signal to workers (This is done again in shutdown, but might be needed here too if run() errors)
        self.logger.info(&format!(
            "Sending Stop signal to {} workers (from run)...",
            self.worker_handles.len()
        )); // Added log
        for i in 0..self.worker_handles.len() {
            // Use index for logging
            // Log errors, channel might be closed if workers finished early
            // Use send for broadcast, ignore Ok result (receiver count)
            if let Err(e) = self.task_tx.send(WorkerMessage::Stop) {
                self.logger.warning(&format!(
                    "Failed to broadcast Stop message to worker {} from run: {}",
                    i, e
                )); // Log warning on failure
            }
        }
        // Removed redundant Stop signal sending loop here. Drop impl handles it.
        self.logger.info("Main loop exited.");
        Ok(())
    }

    // Removed the entire shutdown() method as it's no longer called from main.rs

    // Made public for explicit call before drop if needed, but Drop handles it too.
    pub fn cleanup(&mut self) -> Result<(), Box<dyn Error>> {
        // Check if raw mode is enabled before attempting to disable
        // This requires checking terminal state, which is complex.
        // Instead, just attempt cleanup and ignore potential errors if already cleaned up.
        self.logger.info("Restoring terminal state...");
        let backend = self.terminal.backend_mut();
        let cleanup_result = execute!(backend, LeaveAlternateScreen, DisableMouseCapture); // Use imported macro
        let cursor_result = self.terminal.show_cursor();
        let raw_mode_result = disable_raw_mode(); // Disable raw mode last

        // Log errors if they occur, but don't propagate them as errors from cleanup
        if let Err(e) = cleanup_result {
            self.logger
                .error(&format!("Error executing terminal cleanup commands: {}", e));
        }
        if let Err(e) = cursor_result {
            self.logger.error(&format!("Error showing cursor: {}", e));
        }
        if let Err(e) = raw_mode_result {
            self.logger
                .error(&format!("Error disabling raw mode: {}", e));
        }
        Ok(()) // Return Ok even if cleanup had minor issues
    }
} // <-- Add missing closing brace for impl App

// Implement Drop to ensure cleanup happens even on panic or early exit
impl Drop for App {
    fn drop(&mut self) {
        // Ensure shutdown sequence is attempted if not already stopped
        if self.stats.running_state != RunningState::Stopping {
            self.logger
                .warning("App dropped unexpectedly, attempting shutdown and cleanup.");
            // Send stop signals if workers might still be running
            // Send stop signals if workers might still be running
            // Use send for broadcast, ignore result/error in drop
            let _ = self.task_tx.send(WorkerMessage::Stop);
            // self.shutdown(); // shutdown() is removed
            self.logger
                .warning("App dropped unexpectedly. Cleanup attempted."); // Simplified warning
        }
        // Attempt cleanup, ignore errors during drop
        if let Err(e) = self.cleanup() {
            // Use eprintln! here as logger might be gone or involved in the panic
            eprintln!("Error during terminal cleanup in App::drop: {}", e);
        }
    }
}

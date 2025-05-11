use crate::config::loader::{self, load_config_and_compile};
use crate::logger::Logger;
use crate::ui::{DebugInfo, LayoutRects, ThreadStats};
use crate::ui::{RunningState, Stats, TargetStats, draw_ui};
use crate::worker::{TargetUpdate, WorkerMessage, worker_loop};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseEventKind},
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
use sysinfo::System;
use tokio::sync::{broadcast, mpsc};
use tokio::{task::JoinHandle, time::sleep};

// Enum to represent the currently focused scrollable widget
#[derive(PartialEq, Debug, Clone, Copy)]
pub enum FocusedWidget { // Made public
    ThreadTable,
    TargetTable,
    Console,
}

pub struct App {
    config: loader::AttackConfig,
    stats: Stats,
    logger: Logger,
    terminal: Terminal<CrosstermBackend<Stdout>>,
    task_tx: broadcast::Sender<WorkerMessage>,
    target_stats_tx: mpsc::Sender<TargetUpdate>,
    target_stats_rx: mpsc::Receiver<TargetUpdate>,
    log_rx: Option<mpsc::Receiver<DebugInfo>>,
    worker_handles: Vec<JoinHandle<()>>,
    log_receiver_handle: Option<thread::JoinHandle<()>>,
    focused_widget: FocusedWidget, // Add field for focused widget
    layout_rects: LayoutRects, // Store all layout rects
}

impl App {
    pub fn new(config_path: &str) -> Result<Self, Box<dyn Error>> {
        let config = load_config_and_compile(config_path)?;

        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?; // Clear screen on startup

        let (task_tx, _) = broadcast::channel(32);
        let (target_stats_tx, target_stats_rx) = mpsc::channel(32);
        let (log_tx, log_rx) = mpsc::channel(32);

        let logger = Logger::new(log_tx.clone());

        let stats = Stats {
            targets: config.targets.iter()
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
            console_scroll_offset: 0,
            console_auto_scroll: true, // Initialize console_auto_scroll
            // thread_table_offset field was here, removed.
            // thread_table_offset_right field was here, removed.
            target_table_offset: 0,
        };

        Ok(App {
            config,
            stats,
            logger,
            terminal,
            task_tx,
            target_stats_tx,
            target_stats_rx,
            log_rx: Some(log_rx),
            worker_handles: Vec::new(),
            log_receiver_handle: None,
            focused_widget: FocusedWidget::ThreadTable,
            layout_rects: LayoutRects::default(), // Initialize with default
        })
    }

    pub fn spawn_workers(&mut self) {
        self.logger.info(&format!("Spawning {} worker threads...", self.config.threads));
        for _ in 0..self.config.threads {
            let rx = self.task_tx.subscribe();
            let cfg = self.config.clone();
            let worker_logger = self.logger.clone();

            let stats_tx = self.target_stats_tx.clone();
            let handle = tokio::spawn(async move {
                 worker_loop(rx, cfg, std::thread::current().id(), worker_logger.clone(), stats_tx).await
            });
            self.worker_handles.push(handle);
        }
    }

    pub fn spawn_log_receiver(&mut self) {
        self.logger.info("Spawning log receiver thread...");
        let mut log_rx = self.log_rx.take().expect("Log receiver already taken");
        let debug_logs_tx = self.target_stats_tx.clone();
        let logger_clone = self.logger.clone();

        let handle = thread::spawn(move || {
            logger_clone.info("Log receiver thread started.");
            while let Some(log_entry) = log_rx.blocking_recv() {
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
                    eprintln!("Log receiver failed to send to UI channel, exiting.");
                    break;
                }
            }
            eprintln!("Log receiver thread loop finished.");
        });
        self.log_receiver_handle = Some(handle);
    }

    // Helper to update layout rects after drawing (or before mouse event processing)
    pub fn update_layout_rects(&mut self, new_rects: LayoutRects) {
        self.layout_rects = new_rects;
    }


    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.logger.info("Starting main application loop.");
        let mut sysinfo_tick = 0u32;
        let mut last_draw_time = Instant::now();
        let redraw_interval = Duration::from_millis(100); // Reduced for smoother mouse scroll feel
        let mut needs_redraw = true;

        let mut last_stats_update_time = Instant::now();
        let stats_update_interval = Duration::from_secs(1);
        let mut requests_in_last_second = 0u64;
        let mut successes_in_last_second = 0u64;

        let running = std::sync::Arc::new(AtomicBool::new(true));
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        })?;

        // Initial draw to get layout rects
        // This is a bit of a chicken-and-egg, draw_ui needs to return the rects
        // For now, we'll assume draw_ui is modified to do so or we use estimates.
        // A better way is to have draw_ui return the rects or store them in App from draw_ui.
        match draw_ui(&mut self.terminal, &self.stats, self.focused_widget) {
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
                match event::read()? {
                    Event::Key(key) => {
                        needs_redraw = true; // Assume any key press might change state
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('s') => {
                                running.store(false, Ordering::SeqCst);
                            }
                            KeyCode::Char('p') if self.stats.running_state == RunningState::Running => {
                                self.stats.running_state = RunningState::Paused;
                                self.logger.info("Pausing workers...");
                                for _i in 0..self.worker_handles.len() {
                                    if let Err(e) = self.task_tx.send(WorkerMessage::Pause) {
                                        self.logger.warning(&format!("Failed to send Pause message: {}",e));
                                    }
                                }
                            }
                            KeyCode::Char('r') if self.stats.running_state == RunningState::Paused => {
                                self.stats.running_state = RunningState::Running;
                                self.logger.info("Resuming workers...");
                                for _i in 0..self.worker_handles.len() {
                                    if let Err(e) = self.task_tx.send(WorkerMessage::Resume) {
                                        self.logger.warning(&format!("Failed to send Resume message: {}",e));
                                    }
                                }
                            }
                            KeyCode::Tab => {
                                self.focused_widget = match self.focused_widget {
                                    FocusedWidget::ThreadTable => FocusedWidget::TargetTable,
                                    FocusedWidget::TargetTable => FocusedWidget::Console,
                                    FocusedWidget::Console => FocusedWidget::ThreadTable,
                                };
                                self.logger.info(&format!("Focus changed to: {:?}", self.focused_widget));
                            }
                            KeyCode::Char('a') => { // Toggle auto-scroll for console
                                if self.focused_widget == FocusedWidget::Console {
                                    self.stats.console_auto_scroll = !self.stats.console_auto_scroll;
                                    self.logger.info(&format!("Console auto-scroll toggled to: {}", self.stats.console_auto_scroll));
                                }
                            }
                            KeyCode::Down => {
                                match self.focused_widget {
                                    FocusedWidget::Console => {
                                        self.stats.console_scroll_offset = self.stats.console_scroll_offset.saturating_add(1);
                                        self.stats.console_auto_scroll = false;
                                    }
                                    FocusedWidget::ThreadTable => {
                                        // Scrolling for thread activity is removed as it's now a Paragraph.
                                        // No action needed for arrow down.
                                    }
                                    FocusedWidget::TargetTable => {
                                        let table_len = self.stats.targets.len();
                                        if table_len > 0 {
                                            let current_offset = self.stats.target_table_offset;
                                            if current_offset < table_len.saturating_sub(1) {
                                                self.stats.target_table_offset = current_offset.saturating_add(1);
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Up => {
                                match self.focused_widget {
                                    FocusedWidget::Console => {
                                        self.stats.console_scroll_offset = self.stats.console_scroll_offset.saturating_sub(1);
                                        self.stats.console_auto_scroll = false;
                                    }
                                    FocusedWidget::ThreadTable => {
                                        // Scrolling for thread activity is removed.
                                        // No action needed for arrow up.
                                    }
                                    FocusedWidget::TargetTable => {
                                        self.stats.target_table_offset = self.stats.target_table_offset.saturating_sub(1);
                                    }
                                }
                            }
                            KeyCode::PageDown => {
                                let scroll_amount_table: usize = 10;
                                let scroll_amount_console: u16 = 10;
                                match self.focused_widget {
                                    FocusedWidget::Console => {
                                        self.stats.console_scroll_offset = self.stats.console_scroll_offset.saturating_add(scroll_amount_console);
                                        self.stats.console_auto_scroll = false;
                                    }
                                    FocusedWidget::ThreadTable => {
                                        // Scrolling for thread activity is removed.
                                        // No action needed for PageDown.
                                    }
                                    FocusedWidget::TargetTable => {
                                        let table_len = self.stats.targets.len();
                                        if table_len > 0 {
                                            let current_offset = self.stats.target_table_offset;
                                            let max_offset = table_len.saturating_sub(1);
                                            self.stats.target_table_offset = (current_offset + scroll_amount_table).min(max_offset);
                                        }
                                    }
                                }
                            }
                            KeyCode::PageUp => {
                                let scroll_amount_table: usize = 10;
                                let scroll_amount_console: u16 = 10;
                                match self.focused_widget {
                                    FocusedWidget::Console => {
                                        self.stats.console_scroll_offset = self.stats.console_scroll_offset.saturating_sub(scroll_amount_console);
                                        self.stats.console_auto_scroll = false;
                                    }
                                    FocusedWidget::ThreadTable => {
                                        // Scrolling for thread activity is removed.
                                        // No action needed for PageUp.
                                    }
                                    FocusedWidget::TargetTable => {
                                        self.stats.target_table_offset = self.stats.target_table_offset.saturating_sub(scroll_amount_table);
                                    }
                                }
                            }
                            _ => { needs_redraw = false; } // Unhandled key, no redraw needed
                        }
                    }
                    Event::Mouse(mouse_event) => {
                        needs_redraw = true; // Assume mouse event might change something
                        match mouse_event.kind {
                            MouseEventKind::Down(button) => { // Handle left-click for buttons
                                if button == event::MouseButton::Left {
                                    let (col, row) = (mouse_event.column, mouse_event.row);
                                    let pause_rect = self.layout_rects.pause_btn;
                                    let resume_rect = self.layout_rects.resume_btn;
                                    let quit_rect = self.layout_rects.quit_btn;

                                    if pause_rect.x <= col && col < pause_rect.x + pause_rect.width && pause_rect.y <= row && row < pause_rect.y + pause_rect.height {
                                        if self.stats.running_state == RunningState::Running {
                                            self.stats.running_state = RunningState::Paused;
                                            self.logger.info("Pausing workers (clicked)...");
                                            for _i in 0..self.worker_handles.len() {
                                                if let Err(e) = self.task_tx.send(WorkerMessage::Pause) {
                                                    self.logger.warning(&format!("Failed to send Pause message: {}",e));
                                                }
                                            }
                                        }
                                    } else if resume_rect.x <= col && col < resume_rect.x + resume_rect.width && resume_rect.y <= row && row < resume_rect.y + resume_rect.height {
                                        if self.stats.running_state == RunningState::Paused {
                                            self.stats.running_state = RunningState::Running;
                                            self.logger.info("Resuming workers (clicked)...");
                                            for _i in 0..self.worker_handles.len() {
                                                if let Err(e) = self.task_tx.send(WorkerMessage::Resume) {
                                                    self.logger.warning(&format!("Failed to send Resume message: {}",e));
                                                }
                                            }
                                        }
                                    } else if quit_rect.x <= col && col < quit_rect.x + quit_rect.width && quit_rect.y <= row && row < quit_rect.y + quit_rect.height {
                                        running.store(false, Ordering::SeqCst);
                                        self.logger.info("Quitting application (clicked)...");
                                    } else {
                                        needs_redraw = false; // Click was not on a button
                                    }
                                } else {
                                    needs_redraw = false; // Not a left click
                                }
                            }
                            MouseEventKind::ScrollDown => {
                                let (col, row) = (mouse_event.column, mouse_event.row);
                                let console_rect = self.layout_rects.console;
                                let threads_rect = self.layout_rects.threads; // Use unified threads rect
                                let targets_rect = self.layout_rects.targets;

                                if console_rect.x <= col && col < console_rect.x + console_rect.width && console_rect.y <= row && row < console_rect.y + console_rect.height {
                                    self.stats.console_scroll_offset = self.stats.console_scroll_offset.saturating_add(3);
                                    self.stats.console_auto_scroll = false;
                                } else if threads_rect.x <= col && col < threads_rect.x + threads_rect.width && threads_rect.y <= row && row < threads_rect.y + threads_rect.height {
                                    // Scrolling for thread activity (Paragraph) is not handled by mouse wheel here.
                                } else if targets_rect.x <= col && col < targets_rect.x + targets_rect.width && targets_rect.y <= row && row < targets_rect.y + targets_rect.height {
                                     let table_len = self.stats.targets.len();
                                    if table_len > 0 {
                                        let current_offset = self.stats.target_table_offset;
                                        let max_offset = table_len.saturating_sub(1);
                                        self.stats.target_table_offset = (current_offset + 3).min(max_offset);
                                    }
                                } else {
                                    needs_redraw = false;
                                }
                            }
                            MouseEventKind::ScrollUp => {
                                let (col, row) = (mouse_event.column, mouse_event.row);
                                let console_rect = self.layout_rects.console;
                                let threads_rect = self.layout_rects.threads; // Use unified threads rect
                                let targets_rect = self.layout_rects.targets;

                                if console_rect.x <= col && col < console_rect.x + console_rect.width && console_rect.y <= row && row < console_rect.y + console_rect.height {
                                    self.stats.console_scroll_offset = self.stats.console_scroll_offset.saturating_sub(3);
                                    self.stats.console_auto_scroll = false;
                                } else if threads_rect.x <= col && col < threads_rect.x + threads_rect.width && threads_rect.y <= row && row < threads_rect.y + threads_rect.height {
                                    // Scrolling for thread activity (Paragraph) is not handled by mouse wheel here.
                                } else if targets_rect.x <= col && col < targets_rect.x + targets_rect.width && targets_rect.y <= row && row < targets_rect.y + targets_rect.height {
                                    self.stats.target_table_offset = self.stats.target_table_offset.saturating_sub(3);
                                } else {
                                    needs_redraw = false;
                                }
                            }
                            _ => { needs_redraw = false; } // Other mouse events like Move, Drag, etc.
                        }
                    }
                    _ => { received_input_or_event = false; } // Unhandled terminal event
                 }
            }

            if !received_input_or_event { // Only process stats if no input happened, to prioritize responsiveness
                // Update system info less frequently or if no input
                sysinfo_tick = sysinfo_tick.wrapping_add(1);
                if sysinfo_tick % 20 == 0 { // e.g., every 1 second if poll is 50ms
                    self.stats.sys.refresh_all();
                    self.stats.cpu_usage = self.stats.sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / self.stats.sys.cpus().len() as f32;
                    self.stats.memory_usage = self.stats.sys.used_memory();
                    needs_redraw = true;
                }

                while let Ok(update) = self.target_stats_rx.try_recv() {
                    needs_redraw = true; // Stats update always needs redraw
                    if update.url.is_empty() {
                        if let Some(debug_msg) = update.debug {
                            self.stats.debug_logs.push_back(DebugInfo {
                                timestamp: update.timestamp,
                                message: debug_msg,
                            });
                            if self.stats.debug_logs.len() > 250 { // Keep a max of 250 logs (MAX_CONSOLE_LOGS)
                                self.stats.debug_logs.pop_front();
                            }
                            if self.stats.console_auto_scroll {
                                self.stats.console_scroll_offset = u16::MAX; // Scroll to bottom
                            }
                        }
                        continue;
                    }

                    self.stats.total += 1;
                    requests_in_last_second += 1;
                    if update.success {
                        self.stats.success += 1;
                        successes_in_last_second += 1;
                        self.stats.last_success_time = Some(update.timestamp);
                    } else {
                        self.stats.failure += 1;
                        self.stats.last_failure_time = Some(update.timestamp);
                    }

                    if !update.url.is_empty() {
                        if let Some(target) = self.stats.targets.iter_mut().find(|t| t.id == update.id) {
                            if update.success {
                                target.success += 1;
                                target.last_success_time = Some(update.timestamp);
                            } else {
                                target.failure += 1;
                                target.last_failure_time = Some(update.timestamp);
                            }
                            if let Some(network_err) = update.network_error {
                                target.last_network_error = Some(network_err);
                            } else if !update.success {
                                target.last_network_error = None;
                            }
                        } else {
                            self.logger.warning(&format!("Received update for unknown target ID: {} for URL: {}", update.id, update.url));
                        }
                    }

                    let now = update.timestamp;
                    match self.stats.threads.iter_mut().find(|ts| ts.id == update.thread_id) {
                        Some(thread_stat) => {
                            thread_stat.requests += 1;
                            thread_stat.last_active = now;
                        }
                        None => {
                            self.logger.info(&format!("First update received from new thread ID: {:?}", update.thread_id));
                            self.stats.threads.push(ThreadStats {
                                id: update.thread_id,
                                requests: 1,
                                last_active: now,
                            });
                        }
                    }
                }

                if self.stats.running_state == RunningState::Running {
                    if let Err(e) = self.task_tx.send(WorkerMessage::Task) {
                        self.logger.warning(&format!("Failed to broadcast Task message: {}", e));
                    }
                }

                if last_stats_update_time.elapsed() >= stats_update_interval {
                    self.stats.rps_history.push_back(requests_in_last_second);
                    if self.stats.rps_history.len() > 120 {
                        self.stats.rps_history.pop_front();
                    }
                    
                    let successful_rps_this_second = successes_in_last_second;
                    self.stats.successful_requests_per_second_history.push_back(successful_rps_this_second);
                    if self.stats.successful_requests_per_second_history.len() > 120 {
                        self.stats.successful_requests_per_second_history.pop_front();
                    }

                    let current_success_rate = if requests_in_last_second > 0 {
                        (successful_rps_this_second * 100 / requests_in_last_second).min(100) // Ensure rate is <= 100
                    } else if self.stats.total > 0 { // If no requests in last sec, use overall
                        (self.stats.success * 100 / self.stats.total).min(100)
                    } else {
                        100 // Default to 100 if no requests at all
                    };
                    self.stats.success_rate_history.push_back(current_success_rate);
                    if self.stats.success_rate_history.len() > 120 {
                        self.stats.success_rate_history.pop_front();
                    }
                    
                    requests_in_last_second = 0;
                    successes_in_last_second = 0;
                    last_stats_update_time = Instant::now();
                    needs_redraw = true;
                }
            }


            if needs_redraw || last_draw_time.elapsed() >= redraw_interval {
                match draw_ui(&mut self.terminal, &self.stats, self.focused_widget) {
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
        self.logger.info("Shutting down...");
        for _i in 0..self.worker_handles.len() {
            if let Err(e) = self.task_tx.send(WorkerMessage::Stop) {
                self.logger.warning(&format!("Failed to send Stop message: {}",e));
            }
        }
        // Give workers a moment to process the stop message
        sleep(Duration::from_millis(100)).await;
        Ok(())
    } // This closes run method

    pub fn cleanup(&mut self) -> Result<(), Box<dyn Error>> {
        self.logger.info("Restoring terminal state...");
        let backend = self.terminal.backend_mut();
        execute!(backend, LeaveAlternateScreen, DisableMouseCapture)?; // Ensure mouse capture is disabled
        self.terminal.show_cursor()?; // Ensure cursor is shown
        disable_raw_mode()?; // Ensure raw mode is disabled
        Ok(())
    }
}

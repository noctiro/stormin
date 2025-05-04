use crate::config::loader::{self, load_config_and_compile};
use crate::logger::Logger;
use crate::ui::{DebugInfo, ThreadStats};
use crate::ui::{RunningState, Stats, TargetStats, draw_ui};
use crate::worker::{TargetUpdate, WorkerMessage, worker_loop};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
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
}

impl App {
    pub fn new(config_path: &str) -> Result<Self, Box<dyn Error>> {
        let config = load_config_and_compile(config_path)?;

        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        let (task_tx, _) = broadcast::channel(32);
        let (target_stats_tx, target_stats_rx) = mpsc::channel(32);
        let (log_tx, log_rx) = mpsc::channel(32);

        let logger = Logger::new(log_tx.clone());

        let stats = Stats {
            targets: config.targets.iter()
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
            sys: System::new_all(),
            cpu_usage: 0.0,
            memory_usage: 0,
            proxy_count: config.proxies.len(),
            running_state: RunningState::Running,
            debug_logs: VecDeque::with_capacity(1000),
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
        })
    }

    pub fn spawn_workers(&mut self) {
        self.logger.info(&format!("Spawning {} worker threads...", self.config.threads));
        for _ in 0..self.config.threads {
            let rx = self.task_tx.subscribe();
            let cfg = self.config.clone();
            let worker_logger = self.logger.clone();

            let stats_tx = self.target_stats_tx.clone();
            let thread_id = std::thread::current().id();
            // 初始化线程统计
            self.stats.threads.push(ThreadStats {
                id: thread_id,
                requests: 0,
                last_active: Instant::now(),
            });
            let handle = tokio::spawn(async move {
                worker_loop(rx, cfg, thread_id, worker_logger.clone(), stats_tx).await
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
                    url: String::new(),
                    success: false,
                    timestamp: log_entry.timestamp,
                    debug: Some(log_entry.message),
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

    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.logger.info("Starting main application loop.");
        let mut sysinfo_tick = 0u32;
        let mut last_draw_time = Instant::now();
        let redraw_interval = Duration::from_millis(250);
        let mut needs_redraw = true;

        let running = std::sync::Arc::new(AtomicBool::new(true));
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
        })?;

        while running.load(Ordering::SeqCst) {
            let mut received_input = false;
            if event::poll(Duration::from_millis(50))? {
                received_input = true;
                match event::read()? {
                    Event::Key(key) => {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('s') => {
                                running.store(false, Ordering::SeqCst);
                            }
                            KeyCode::Char('p') if self.stats.running_state == RunningState::Running => {
                                self.stats.running_state = RunningState::Paused;
                                self.logger.info("Pausing workers...");
                                for i in 0..self.worker_handles.len() {
                                    if let Err(e) = self.task_tx.send(WorkerMessage::Pause) {
                                        self.logger.warning(&format!(
                                            "Failed to send Pause message to worker {}: {}",
                                            i, e
                                        ));
                                    }
                                }
                            }
                            KeyCode::Char('r') if self.stats.running_state == RunningState::Paused => {
                                self.stats.running_state = RunningState::Running;
                                self.logger.info("Resuming workers...");
                                for i in 0..self.worker_handles.len() {
                                    if let Err(e) = self.task_tx.send(WorkerMessage::Resume) {
                                        self.logger.warning(&format!(
                                            "Failed to send Resume message to worker {}: {}",
                                            i, e
                                        ));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            if received_input {
                needs_redraw = true;
            }

            // 更新系统信息
            sysinfo_tick = sysinfo_tick.wrapping_add(1);
            if sysinfo_tick % 10 == 0 {
                self.stats.sys.refresh_all();
                self.stats.cpu_usage = self.stats.sys.cpus()
                    .iter()
                    .map(|cpu| cpu.cpu_usage())
                    .sum::<f32>()
                    / self.stats.sys.cpus().len() as f32;
                self.stats.memory_usage = self.stats.sys.used_memory();
            }

            while let Ok(update) = self.target_stats_rx.try_recv() {
                if update.url.is_empty() {
                    if let Some(debug_msg) = update.debug {
                        self.stats.debug_logs.push_back(DebugInfo {
                            timestamp: update.timestamp,
                            message: debug_msg,
                        });
                        while self.stats.debug_logs.len() > 1000 {
                            self.stats.debug_logs.pop_front();
                        }
                    }
                } else {
                    // 更新全局统计
                    self.stats.total += 1;
                    if update.success {
                        self.stats.success += 1;
                        self.stats.last_success_time = Some(update.timestamp);
                    } else {
                        self.stats.failure += 1;
                        self.stats.last_failure_time = Some(update.timestamp);
                    }
                    
                    // 更新目标统计
                    if let Some(target) = self.stats.targets.iter_mut().find(|t| t.url == update.url) {
                        if update.success {
                            target.success += 1;
                            target.last_success_time = Some(update.timestamp);
                        } else {
                            target.failure += 1;
                            target.last_failure_time = Some(update.timestamp);
                        }
                    }
                }
            }

            if self.stats.running_state == RunningState::Running {
                if let Err(e) = self.task_tx.send(WorkerMessage::Task) {
                    self.logger.warning(&format!("Failed to broadcast Task message: {}", e));
                }
            }

            let should_draw = needs_redraw || last_draw_time.elapsed() >= redraw_interval;
            if should_draw {
                if let Err(e) = draw_ui(&mut self.terminal, &mut self.stats) {
                    self.logger.error(&format!("Failed to draw UI: {}", e));
                    running.store(false, Ordering::SeqCst);
                } else {
                    last_draw_time = Instant::now();
                    needs_redraw = false;
                }
            } else {
                sleep(Duration::from_millis(20)).await;
            }
        }

        self.stats.running_state = RunningState::Stopping;
        self.logger.info("Shutting down...");
        for i in 0..self.worker_handles.len() {
            if let Err(e) = self.task_tx.send(WorkerMessage::Stop) {
                self.logger.warning(&format!(
                    "Failed to send Stop message to worker {}: {}",
                    i, e
                ));
            }
        }
        Ok(())
    }

    pub fn cleanup(&mut self) -> Result<(), Box<dyn Error>> {
        self.logger.info("Restoring terminal state...");
        let backend = self.terminal.backend_mut();
        let _ = execute!(backend, LeaveAlternateScreen, DisableMouseCapture);
        let _ = self.terminal.show_cursor();
        let _ = disable_raw_mode();
        Ok(())
    }
}

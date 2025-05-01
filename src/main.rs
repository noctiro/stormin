mod config;
mod generator;
mod ui;

use crate::ui::{Stats, ThreadStats, TargetStats, RunningState, draw_ui, DebugInfo};
use config::{load_config_and_compile, UrlPart};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseEventKind, MouseButton},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use reqwest::blocking::Client;
use std::{error::Error, io, thread::{self, ThreadId}, time::{Duration, Instant}};
use crossbeam_channel::{unbounded, Receiver};
use rand::{rng, seq::IndexedRandom, Rng};
use ratatui::{backend::CrosstermBackend, Terminal};
use sysinfo::System;

use generator::{
    password::PasswordGenerator, ChineseSocialPasswordGenerator, QQIDGenerator, RandomPasswordGenerator, UsernameGenerator
};
use base64::{engine::general_purpose::STANDARD, Engine as _};

/// 在模板中遇到函数调用时，执行对应操作
fn apply_function(name: &str, arg: &str) -> String {
    match name {
        "base64" => STANDARD.encode(arg),
        _ => arg.to_string(),
    }
}

/// 渲染一个 CompiledUrl，将所有 UrlPart 转为最终字符串pp
fn render_compiled_url(
    template: &config::CompiledUrl,
    username: Option<&str>,
    password: Option<&str>,
    qqid: Option<&str>,
) -> String {
    let mut out = String::with_capacity(128);
    for part in &template.parts {
        match part {
            UrlPart::Static(s) => out.push_str(s),
            UrlPart::User => {
                if let Some(u) = username {
                    out.push_str(u);
                }
            }
            UrlPart::Password => {
                if let Some(p) = password {
                    out.push_str(p);
                }
            }
            UrlPart::Qqid => {
                if let Some(q) = qqid {
                    out.push_str(q);
                }
            }
            UrlPart::FunctionCall { name, args } => {
                let mut arg_str = String::new();
                for a in args {
                    match a {
                        UrlPart::Static(s) => arg_str.push_str(s),
                        UrlPart::User => {
                            if let Some(u) = username {
                                arg_str.push_str(u);
                            }
                        }
                        UrlPart::Password => {
                            if let Some(p) = password {
                                arg_str.push_str(p);
                            }
                        }
                        UrlPart::Qqid => {
                            if let Some(q) = qqid {
                                arg_str.push_str(q);
                            }
                        }
                        UrlPart::FunctionCall { name: inner_name, args: inner_args } => {
                            // 递归处理嵌套的函数调用
                            let inner_template = config::CompiledUrl {
                                parts: vec![UrlPart::FunctionCall {
                                    name: inner_name.clone(),
                                    args: inner_args.clone(),
                                }],
                                needs_user: false,
                                needs_password: false,
                                needs_qqid: false,
                            };
                            arg_str.push_str(&render_compiled_url(&inner_template, username, password, qqid));
                        }
                    }
                }
                out.push_str(&apply_function(name, &arg_str));
            }
        }
    }
    out
}

enum RequestResult {
    Success,
    Failure,
}

enum WorkerMessage {
    Task,
    Pause,
    Resume,
    Stop,
}

// 添加目标统计通道
struct TargetUpdate {
    url: String,
    success: bool,
    timestamp: std::time::Instant,
    debug: Option<String>, // 添加调试信息字段
}

fn worker_loop(
    rx: Receiver<WorkerMessage>,
    client: Client,
    config: config::AttackConfig,
    tx: crossbeam_channel::Sender<RequestResult>,
    thread_id: ThreadId,
    thread_stats_tx: crossbeam_channel::Sender<ThreadStats>,
    target_stats_tx: crossbeam_channel::Sender<TargetUpdate>,
) {
    let mut rng = rng();
    let mut username_gen = UsernameGenerator::new();
    let mut pwd_gen_social = ChineseSocialPasswordGenerator::new();
    let mut pwd_gen_rand = RandomPasswordGenerator::new();
    let mut qqid_gen = QQIDGenerator::new();
    let mut requests = 0u64;

    while let Ok(msg) = rx.recv() {
        match msg {
            WorkerMessage::Stop => break,
            WorkerMessage::Pause => {
                while let Ok(msg) = rx.recv() {
                    match msg {
                        WorkerMessage::Resume => break,
                        WorkerMessage::Stop => return,
                        _ => continue,
                    }
                }
            }
            WorkerMessage::Task => {
                let target = config.targets.choose(&mut rng).unwrap();
                let needs_user = target.params.iter().any(|(_, c)| c.needs_user);
                let needs_pwd = target.params.iter().any(|(_, c)| c.needs_password);
                let needs_qq = target.params.iter().any(|(_, c)| c.needs_qqid);

                let username = needs_user.then(|| username_gen.generate_random());
                let password = needs_pwd.then(|| {
                    if rng.random_bool(0.6) {
                        pwd_gen_social.generate()
                    } else {
                        pwd_gen_rand.generate()
                    }
                });
                let qqid = needs_qq.then(|| qqid_gen.generate_qq_id());

                let mut req = client.request(
                    target.method.parse().unwrap(),
                    &target.url,
                );

                // Add headers
                for (key, value) in &target.headers {
                    req = req.header(key, value);
                }

                // Add parameters
                for (key, template) in &target.params {
                    let value = render_compiled_url(template, username.as_deref(), password.as_deref(), qqid.as_deref());
                    req = req.query(&[(key, value)]);
                }

                // 构建请求前记录完整的调试信息
                let params_debug = target.params.iter()
                    .map(|(key, template)| {
                        let value = render_compiled_url(template, username.as_deref(), password.as_deref(), qqid.as_deref());
                        format!("{}={}", key, value)
                    })
                    .collect::<Vec<_>>()
                    .join("\n  ");

                let debug_info = format!(
                    "[Request]\nURL: {}\nMethod: {}\nParams:\n  {}",
                    target.url,
                    target.method,
                    params_debug
                );
                
                // 发送请求调试信息
                target_stats_tx.send(TargetUpdate {
                    url: target.url.clone(),
                    success: false, // 在请求发送前，不确定成功与否
                    timestamp: Instant::now(),
                    debug: Some(debug_info),
                }).unwrap();

                // 发送请求...
                let request_start = Instant::now();
                let res = req.send();
                let request_duration = request_start.elapsed();
                requests += 1;

                // 确定请求结果
                let success = res.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

                // 记录响应调试信息
                let response_info = match &res {
                    Ok(r) => format!(
                        "[Response]\nStatus: {} {}\nTime: {:.2}ms",
                        r.status().as_u16(),
                        r.status().to_string(),
                        request_duration.as_secs_f64() * 1000.0
                    ),
                    Err(e) => format!("[Error]\n{}", e),
                };
                
                // 发送响应调试信息和结果更新
                target_stats_tx.send(TargetUpdate {
                    url: target.url.clone(),
                    success,
                    timestamp: Instant::now(),
                    debug: Some(response_info),
                }).unwrap();

                // 发送统计更新
                if success {
                    tx.send(RequestResult::Success).unwrap();
                } else {
                    tx.send(RequestResult::Failure).unwrap();
                }

                // 发送线程统计更新
                thread_stats_tx.send(ThreadStats {
                    id: thread_id,
                    requests,
                    last_active: Instant::now(),
                }).unwrap();
            }
            WorkerMessage::Resume => {}
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // 初始化 TUI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (task_tx, task_rx) = unbounded();
    let (stat_tx, stat_rx) = unbounded();
    let (thread_stats_tx, thread_stats_rx) = unbounded();
    let (target_stats_tx, target_stats_rx) = unbounded();
    let config = load_config_and_compile("config.toml")?;
    let client = Client::new();

    // 启动 worker 线程
    let mut thread_handles = Vec::new();
    for _ in 0..config.threads {
        let rx = task_rx.clone();
        let tx = stat_tx.clone();
        let cfg = config.clone();
        let cl = client.clone();
        let thread_stats_tx = thread_stats_tx.clone();
        let target_stats_tx = target_stats_tx.clone();
        let handle = thread::spawn(move || {
            let thread_id = thread::current().id();
            worker_loop(rx, cl, cfg, tx, thread_id, thread_stats_tx, target_stats_tx)
        });
        thread_handles.push(handle);
    }

    // 初始化统计
    let mut stats = Stats {
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
        sys: System::new(),
        cpu_usage: 0.0,
        memory_usage: 0,
        running_state: RunningState::Running,
        debug_logs: Vec::with_capacity(1000),  // 修正字段名
    };

    // 设置 Ctrl+C 处理
    ctrlc::set_handler(move || {
        disable_raw_mode().unwrap();
        crossterm::execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).unwrap();
        std::process::exit(0);
    })?;

    // 启动任务发送线程
    let task_sender = task_tx.clone();
    let task_interval = 10;
    let (pause_tx, pause_rx) = unbounded();
    thread::spawn(move || {
        let mut paused = false;
        loop {
            // 检查暂停信号
            if let Ok(should_pause) = pause_rx.try_recv() {
                paused = should_pause;
            }
            
            if !paused {
                if task_sender.send(WorkerMessage::Task).is_err() {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(task_interval));
        }
    });

    // 主 UI 循环
    loop {
        // 处理输入
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    match key.code {
                        // ...existing key handlers...
                        KeyCode::Char('q') => {
                            stats.running_state = RunningState::Stopping;
                            // 发送停止信号给所有工作线程
                            for _ in 0..thread_handles.len() {
                                task_tx.send(WorkerMessage::Stop).unwrap();
                            }
                            break;
                        }
                        KeyCode::Char('p') if stats.running_state == RunningState::Running => {
                            stats.running_state = RunningState::Paused;
                            pause_tx.send(true).unwrap(); // 发送暂停信号
                            for _ in 0..thread_handles.len() {
                                task_tx.send(WorkerMessage::Pause).unwrap();
                            }
                        }
                        KeyCode::Char('r') if stats.running_state == RunningState::Paused => {
                            stats.running_state = RunningState::Running;
                            pause_tx.send(false).unwrap(); // 发送继续信号
                            for _ in 0..thread_handles.len() {
                                task_tx.send(WorkerMessage::Resume).unwrap();
                            }
                        }
                        KeyCode::Char('s') => {
                            stats.running_state = RunningState::Stopping;
                            for _ in 0..thread_handles.len() {
                                task_tx.send(WorkerMessage::Stop).unwrap();
                            }
                            break;
                        }
                        _ => {}
                    }
                }
                Event::Mouse(event) => {
                    if event.kind == MouseEventKind::Down(MouseButton::Left) {
                        // 尝试复制点击的日志条目
                        if ui::try_copy_log_entry(&stats, event.row)? {
                            // 可以在这里添加复制成功的视觉反馈
                        }
                    }
                }
                _ => {}
            }
        }

        // 更新系统信息
        stats.sys.refresh_all();
        stats.cpu_usage = stats
            .sys
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage())
            .sum::<f32>()
            / stats.sys.cpus().len() as f32;
        stats.memory_usage = stats.sys.used_memory();

        // 更新线程状态
        while let Ok(thread_stat) = thread_stats_rx.try_recv() {
            if let Some(existing) = stats.threads.iter_mut().find(|t| t.id == thread_stat.id) {
                *existing = thread_stat;
            } else {
                stats.threads.push(thread_stat);
            }
        }

        // 更新目标统计
        while let Ok(update) = target_stats_rx.try_recv() {
            if let Some(target) = stats.targets.iter_mut().find(|t| t.url == update.url) {
                if update.success {
                    target.success += 1;
                    target.last_success_time = Some(update.timestamp);
                } else {
                    target.failure += 1;
                    target.last_failure_time = Some(update.timestamp);
                }
                // 记录调试日志
                if let Some(debug) = update.debug {
                    stats.debug_logs.push(DebugInfo {
                        timestamp: Instant::now(),
                        message: debug,
                    });
                    // 保持日志数量在合理范围内
                    if stats.debug_logs.len() > 1000 {
                        stats.debug_logs.remove(0);
                    }
                }
            }
        }

        // 收集请求统计
        while let Ok(res) = stat_rx.try_recv() {
            stats.total += 1;
            match res {
                RequestResult::Success => {
                    stats.success += 1;
                    stats.last_success_time = Some(Instant::now());
                }
                RequestResult::Failure => {
                    stats.failure += 1;
                    stats.last_failure_time = Some(Instant::now());
                }
            }
        }

        draw_ui(&mut terminal, &mut stats)?;

        if stats.running_state == RunningState::Stopping {
            // 等待所有工作线程结束
            for handle in thread_handles {
                let _ = handle.join();
            }
            break;
        }
    }

    // 恢复 Terminal
    disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

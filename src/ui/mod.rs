pub mod cli;
pub mod event_handler;
pub mod stats_updater;
use crate::app::App;
use crossterm::{
    event::{self, DisableMouseCapture},
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode},
};
use ratatui::{
    prelude::*,
    symbols,
    widgets::{
        Block, BorderType, Borders, Cell, Gauge, LineGauge, Paragraph, Row, Sparkline, Table,
        TableState, Wrap,
    },
};
use std::collections::VecDeque;
use std::error::Error;
use std::{thread, time::Instant};
use sysinfo::System;
use tokio::time::sleep;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunningState {
    Running,
    Paused,
    Stopping,
}

pub struct ThreadStats {
    pub id: thread::ThreadId,
    pub requests: u64,
    pub last_active: Instant,
}

pub struct TargetStats {
    pub id: usize, // Unique ID for the target
    pub url: String,
    pub success: u64,
    pub failure: u64,
    pub last_success_time: Option<Instant>,
    pub last_failure_time: Option<Instant>,
    pub last_network_error: Option<String>, // 存储最后的网络错误信息
    pub error_rate: f64,                    // 动态错误率
}

#[derive(Clone, Debug)]
pub struct DebugInfo {
    // Make struct public
    pub timestamp: Instant,
    pub message: String,
}

pub struct Stats {
    pub targets: Vec<TargetStats>,
    pub threads: Vec<ThreadStats>,
    pub success: std::sync::atomic::AtomicU64,
    pub failure: std::sync::atomic::AtomicU64,
    pub total: std::sync::atomic::AtomicU64,
    pub start_time: Instant,
    pub last_success_time: Option<Instant>,
    pub last_failure_time: Option<Instant>,
    pub sys: System,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub proxy_count: usize, // Add field for proxy count
    pub running_state: RunningState,
    // Store recent debug logs. Should be capped at MAX_CONSOLE_LOGS when adding new logs.
    pub debug_logs: VecDeque<DebugInfo>,
    pub rps_history: VecDeque<u64>, // History of requests per second for sparkline
    pub successful_requests_per_second_history: VecDeque<u64>, // History of successful requests per second
    pub success_rate_history: VecDeque<u64>, // History of success rate for sparkline
}

impl Stats {
    pub fn get_success(&self) -> u64 {
        self.success.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn get_failure(&self) -> u64 {
        self.failure.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn get_total(&self) -> u64 {
        self.total.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// Structure to hold all relevant layout rectangles
#[derive(Default, Clone, Copy)]
pub struct LayoutRects {
    pub console: Rect,
    pub threads: Rect,
    pub targets: Rect,
    pub pause_btn: Rect,
    pub resume_btn: Rect,
    pub quit_btn: Rect,
    pub title_bar: Rect,
}

pub fn draw_ui<B: Backend>(
    terminal: &mut Terminal<B>,
    stats: &Stats,
) -> std::io::Result<LayoutRects> {
    let mut layout_rects = LayoutRects::default();

    terminal.draw(|f| {
        let size = f.size();

        // 将屏幕分为左右两个部分
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70), // 主界面
                Constraint::Percentage(30), // 调试窗口
            ])
            .split(size);

        // 主布局
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),      // 标题和控制按钮 (chunks[0])
                Constraint::Length(3),      // 系统状态 (chunks[1])
                Constraint::Length(3),      // 计数器 (chunks[2])
                Constraint::Length(3),      // Sparklines (chunks[3])
                Constraint::Length(7),      // 请求统计图表 (chunks[4])
                Constraint::Length(3),      // 进度条 (chunks[5])
                Constraint::Length(5),      // 线程状态 (chunks[6]) - Reduced height
                Constraint::Percentage(50), // Target状态 (chunks[7])
            ])
            .split(main_chunks[0]);

        layout_rects.title_bar = chunks[0]; // Store the entire title bar rect

        // 调试窗口
        layout_rects.console = main_chunks[1]; // Store the rect for the console
        let debug_area = layout_rects.console;
        let debug_messages: Vec<Line> = stats
            .debug_logs
            .iter()
            .map(|log| {
                // 将调试信息按行分割，确保每行都有正确的缩进和格式
                let message_lines = log.message.lines().collect::<Vec<_>>();
                let mut formatted_lines = Vec::with_capacity(message_lines.len());

                // 添加时间戳行
                formatted_lines.push(Line::from(vec![
                    Span::styled(
                        format!("[{:.1}s] ", log.timestamp.elapsed().as_secs_f64()),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(message_lines[0]),
                ]));

                // 添加后续行（如果有的话）
                for line in message_lines.iter().skip(1) {
                    formatted_lines.push(Line::from(vec![
                        Span::styled("       ", Style::default().fg(Color::DarkGray)), // 对齐缩进
                        Span::raw(*line),
                    ]));
                }

                formatted_lines
            })
            .flatten()
            .collect();

        let num_logs = debug_messages.len() as u16;
        let visible_height = debug_area.height.saturating_sub(2); // Subtract 2 for borders

        let current_scroll_offset = if num_logs > visible_height {
            num_logs - visible_height
        } else {
            0 // No scroll needed if content fits
        };

        let debug_widget = Paragraph::new(debug_messages)
            .block(
                Block::default()
                    .title(Span::styled(
                        "Console",
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: false })
            .scroll((current_scroll_offset, 0)); // Apply the controlled scroll offset
        f.render_widget(debug_widget, debug_area);

        // 标题和状态
        let title_area = chunks[0];
        let title_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(10),    // Main title info (flexible)
                Constraint::Length(27), // Buttons area (fixed width for 3 buttons * 9 width each)
            ])
            .split(title_area);

        let status_color = match stats.running_state {
            RunningState::Running => Color::Rgb(0, 200, 0), // Brighter Green
            RunningState::Paused => Color::Rgb(255, 165, 0), // Orange
            RunningState::Stopping => Color::Rgb(200, 0, 0), // Brighter Red
        };

        let version = env!("CARGO_PKG_VERSION");
        let elapsed_time_secs = stats.start_time.elapsed().as_secs();
        let elapsed_str = format!(
            "{:02}:{:02}:{:02}",
            elapsed_time_secs / 3600,
            (elapsed_time_secs % 3600) / 60,
            elapsed_time_secs % 60
        );
        let main_title_str = format!(
            "Stormin Dashboard v{} {} | Elapsed: {} | Proxies: {}",
            version,
            match stats.running_state {
                RunningState::Running => "[Running]",
                RunningState::Paused => "[Paused]",
                RunningState::Stopping => "[Stopping]",
            },
            elapsed_str,
            stats.proxy_count
        );

        let title_paragraph = Paragraph::new(Text::styled(
            main_title_str,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ))
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::TOP | Borders::BOTTOM) // Adjusted borders
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(100, 100, 100))), // Modern gray
        );
        f.render_widget(title_paragraph, title_chunks[0]);

        // Buttons Area
        let button_area = title_chunks[1];
        let button_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(9), // Pause button
                Constraint::Length(9), // Resume button
                Constraint::Length(9), // Quit button
            ])
            .margin(0) // No margin for buttons within their area
            .split(button_area);

        layout_rects.pause_btn = button_chunks[0];
        layout_rects.resume_btn = button_chunks[1];
        layout_rects.quit_btn = button_chunks[2];

        // Base block (no background color)
        let base_button_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded);

        // Pause Button
        let pause_text = if stats.running_state == RunningState::Running {
            "[P]ause"
        } else {
            "Pause"
        };
        let pause_text_color = if stats.running_state == RunningState::Running {
            Color::Rgb(0, 180, 255) // Light blue when running
        } else {
            Color::White
        };
        let pause_border_color = if stats.running_state == RunningState::Running {
            Color::Rgb(0, 180, 255)
        } else {
            Color::Gray
        };
        let pause_button = Paragraph::new(pause_text)
            .style(Style::default().fg(pause_text_color)) // Only set text color
            .block(
                base_button_block
                    .clone()
                    .border_style(Style::default().fg(pause_border_color)),
            )
            .alignment(Alignment::Center);
        f.render_widget(pause_button, layout_rects.pause_btn);

        // Resume Button
        let resume_text = if stats.running_state == RunningState::Paused {
            "[R]esume"
        } else {
            "Resume"
        };
        let resume_text_color = if stats.running_state == RunningState::Paused {
            Color::Yellow
        } else {
            Color::White
        };
        let resume_border_color = if stats.running_state == RunningState::Paused {
            Color::Yellow
        } else {
            Color::Gray
        };
        let resume_button = Paragraph::new(resume_text)
            .style(Style::default().fg(resume_text_color))
            .block(
                base_button_block
                    .clone()
                    .border_style(Style::default().fg(resume_border_color)),
            )
            .alignment(Alignment::Center);
        f.render_widget(resume_button, layout_rects.resume_btn);

        // Quit Button (always red)
        let quit_text_color = Color::Red;
        let quit_border_color = Color::Red;
        let quit_button = Paragraph::new("[Q]uit")
            .style(Style::default().fg(quit_text_color))
            .block(
                base_button_block
                    .clone()
                    .border_style(Style::default().fg(quit_border_color)),
            )
            .alignment(Alignment::Center);
        f.render_widget(quit_button, layout_rects.quit_btn);

        // 系统状态 - 添加CPU和内存使用率图表
        let sys_info_block = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]); // Restored index to 1

        // CPU使用率图表
        let cpu_usage_f64 = stats.cpu_usage as f64;
        let cpu_ratio = if cpu_usage_f64.is_nan() {
            0.0 // Default to 0.0 if cpu_usage is NaN
        } else {
            (cpu_usage_f64 / 100.0).clamp(0.0, 1.0)
        };
        let cpu_gauge = LineGauge::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        "CPU Usage",
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .gauge_style(Style::default().fg(Color::Cyan))
            .line_set(symbols::line::THICK)
            .ratio(cpu_ratio);
        f.render_widget(cpu_gauge, sys_info_block[0]);

        // 内存使用率图表
        let total_mem = stats.sys.total_memory() as f64;
        let used_mem = stats.memory_usage as f64;
        let memory_ratio = match (total_mem, used_mem) {
            (t, _) if t <= 0.0 => 0.0,                 // 防止除零
            (t, u) if u.is_nan() || t.is_nan() => 0.0, // 处理NaN
            (t, u) => (u / t).clamp(0.0, 1.0),
        };
        let memory_gauge = LineGauge::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        "Memory Usage",
                        Style::default()
                            .fg(Color::LightMagenta)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .gauge_style(Style::default().fg(Color::Magenta))
            .line_set(symbols::line::THICK)
            .ratio(memory_ratio);
        f.render_widget(memory_gauge, sys_info_block[1]);

        // 计数器区域
        let counters = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ])
            .split(chunks[2]); // Restored index to 2

        // 添加请求速率统计
        let elapsed_secs = stats.start_time.elapsed().as_secs() as f64;
        let req_per_sec = if elapsed_secs > 0.0 && stats.get_total() > 0 {
            (stats.get_total() as f64 / elapsed_secs).max(0.0) // 确保非负
        } else {
            0.0
        };

        let total = Paragraph::new(vec![Line::from(vec![
            Span::styled("Total: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{} ({:.1}/s)", stats.get_total(), req_per_sec),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ])])
        .block(
            Block::default()
                .title(Span::styled(
                    "Total",
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        let success = Paragraph::new(vec![Line::from(vec![
            Span::styled("Count: ", Style::default().fg(Color::Gray)),
            Span::styled(
                stats.get_success().to_string(),
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " (Last: {:.1}s ago)",
                    stats
                        .last_success_time
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0)
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ])])
        .block(
            Block::default()
                .title(Span::styled(
                    "Success",
                    Style::default()
                        .fg(Color::LightGreen)
                        .add_modifier(Modifier::BOLD),
                )) // Consistent title
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        let failure = Paragraph::new(vec![Line::from(vec![
            Span::styled("Count: ", Style::default().fg(Color::Gray)),
            Span::styled(
                stats.get_failure().to_string(),
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " (Last: {:.1}s ago)",
                    stats
                        .last_failure_time
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0)
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ])])
        .block(
            Block::default()
                .title(Span::styled(
                    "Failure",
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                )) // Consistent title
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        f.render_widget(total, counters[0]);
        f.render_widget(success, counters[1]);
        f.render_widget(failure, counters[2]);

        // Sparkline 图表区域
        let sparkline_area = chunks[3]; // Restored index to 3
        let sparkline_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33), // Total RPS
                Constraint::Percentage(33), // Successful RPS
                Constraint::Percentage(34), // Success Rate
            ])
            .split(sparkline_area);

        // Total RPS Sparkline
        let rps_data_for_sparkline: Vec<u64> = stats.rps_history.iter().cloned().collect();
        let rps_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        "Total RPS Trend", // Clarified title
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .data(&rps_data_for_sparkline)
            .style(Style::default().fg(Color::LightYellow));
        f.render_widget(rps_sparkline, sparkline_chunks[0]);

        // Successful RPS Sparkline
        let successful_rps_data: Vec<u64> = stats
            .successful_requests_per_second_history
            .iter()
            .cloned()
            .collect();
        let successful_rps_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        "Success RPS Trend",
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .data(&successful_rps_data)
            .style(Style::default().fg(Color::LightGreen)); // Different color
        f.render_widget(successful_rps_sparkline, sparkline_chunks[1]);

        // Success Rate Sparkline
        let success_rate_data_for_sparkline: Vec<u64> =
            stats.success_rate_history.iter().cloned().collect();
        let success_rate_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        "Success Rate Trend (%)",
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .data(&success_rate_data_for_sparkline)
            .style(Style::default().fg(Color::Cyan)); // Different color
        f.render_widget(success_rate_sparkline, sparkline_chunks[2]);

        // 请求统计图表 - Replaced BarChart with custom horizontal stacked bars
        let chart_area = chunks[4];
        let mut lines: Vec<Line> = Vec::new();
        let max_bar_width = chart_area.width.saturating_sub(30); // Reserve space for name and counts

        if !stats.targets.is_empty() && max_bar_width > 10 {
            // Find the max total requests for scaling, or use a reasonable default if all are 0
            let max_total_req: u64 = stats
                .targets
                .iter()
                .map(|t| t.success + t.failure)
                .max()
                .unwrap_or(1) // Avoid division by zero, ensure at least 1 for scaling
                .max(1); // Ensure it's at least 1

            for target_stat in &stats.targets {
                let target_name = target_stat
                    .url
                    .split('/')
                    .last()
                    .unwrap_or(&target_stat.url);
                // let total_req = target_stat.success + target_stat.failure;

                let success_bar_len = if max_total_req > 0 {
                    (target_stat.success as f64 / max_total_req as f64 * max_bar_width as f64)
                        as u16
                } else {
                    0
                };
                let failure_bar_len = if max_total_req > 0 {
                    (target_stat.failure as f64 / max_total_req as f64 * max_bar_width as f64)
                        as u16
                } else {
                    0
                };

                // Ensure total bar length doesn't exceed max_bar_width due to rounding
                let current_total_bar = success_bar_len + failure_bar_len;
                let (s_len, f_len) = if current_total_bar > max_bar_width {
                    // Proportional scaling if exceeds (should be rare with f64 math but good to have)
                    let scale_factor = max_bar_width as f64 / current_total_bar as f64;
                    (
                        (success_bar_len as f64 * scale_factor) as u16,
                        (failure_bar_len as f64 * scale_factor) as u16,
                    )
                } else {
                    (success_bar_len, failure_bar_len)
                };

                let line_spans = vec![
                    Span::styled(
                        format!("{:<15.15}: ", target_name), // Truncate/pad name
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        "▒".repeat(s_len as usize),
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled("▒".repeat(f_len as usize), Style::default().fg(Color::Red)),
                ];
                lines.push(Line::from(line_spans));
            }
        } else if stats.targets.is_empty() {
            lines.push(Line::from("No targets to display stats for."));
        } else {
            lines.push(Line::from("Not enough space for target stats bars."));
        }

        let requests_by_target_widget = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(Span::styled(
                        "Requests by Target (Success/Failure)",
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(requests_by_target_widget, chart_area);

        // 成功率进度条
        let success_rate = if stats.get_total() > 0 {
            let rate =
                (stats.get_success() as f64 / stats.get_total() as f64 * 100.0).clamp(0.0, 100.0);
            rate as u16
        } else {
            0
        };
        let rate_color = if success_rate > 80 {
            Color::LightGreen
        } else if success_rate > 50 {
            Color::LightYellow
        } else {
            Color::LightRed
        };
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("Success Rate: {}%", success_rate),
                        Style::default().fg(rate_color).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .gauge_style(Style::default().fg(rate_color))
            .percent(success_rate);
        f.render_widget(gauge, chunks[5]); // Restored index to 5

        // 线程状态 - Table (Single, unified view)
        let thread_area_chunk = chunks[6]; // Restored index to 6
        layout_rects.threads = thread_area_chunk; // Assign to the unified 'threads' rect

        let available_width = layout_rects.threads.width.saturating_sub(2); // Subtract borders

        // Define fixed widths for each part of the thread info for alignment
        const TID_PREFIX_WIDTH: usize = 4; // "TID:"
        const TID_VALUE_WIDTH: usize = 15; // Max width for ThreadId display (e.g., "ThreadId(XX)")
        const REQ_PREFIX_WIDTH: usize = 2; // "R:"
        const REQ_VALUE_WIDTH: usize = 6; // Max width for requests (e.g., "999999")
        const ACT_PREFIX_WIDTH: usize = 2; // "A:"
        const ACT_VALUE_WIDTH: usize = 6; // Max width for active time (e.g., "123.4s")
        const ITEM_SPACING: u16 = 2; // Number of spaces between items

        // Calculate total width for one formatted item
        const ITEM_FIXED_WIDTH: u16 = (TID_PREFIX_WIDTH
            + TID_VALUE_WIDTH
            + REQ_PREFIX_WIDTH
            + REQ_VALUE_WIDTH
            + ACT_PREFIX_WIDTH
            + ACT_VALUE_WIDTH) as u16;

        let items_per_line = if available_width > ITEM_FIXED_WIDTH {
            (available_width + ITEM_SPACING) / (ITEM_FIXED_WIDTH + ITEM_SPACING)
        } else {
            1
        }
        .max(1) as usize;

        let mut lines: Vec<Line> = Vec::new();
        if !stats.threads.is_empty() {
            for thread_chunk in stats.threads.chunks(items_per_line) {
                let mut line_spans: Vec<Span> = Vec::new();
                for (idx, thread_stat) in thread_chunk.iter().enumerate() {
                    let last_active_secs = thread_stat.last_active.elapsed().as_secs_f64();
                    let activity_color = if last_active_secs < 1.0 {
                        Color::LightGreen
                    } else if last_active_secs < 5.0 {
                        Color::LightYellow
                    } else {
                        Color::LightRed
                    };

                    // Format ThreadId: Left-align, truncate if necessary
                    let tid_str = format!("{:?}", thread_stat.id);
                    let tid_display = if tid_str.len() > TID_VALUE_WIDTH {
                        &tid_str[..TID_VALUE_WIDTH]
                    } else {
                        &tid_str
                    };

                    let label_style = Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD);

                    line_spans.push(Span::styled(
                        format!("{:<TID_PREFIX_WIDTH$}", "TID:"),
                        label_style,
                    ));
                    line_spans.push(Span::raw(format!("{:<TID_VALUE_WIDTH$}", tid_display))); // Keep value style default or specific

                    line_spans.push(Span::styled(
                        format!("{:<REQ_PREFIX_WIDTH$}", "R:"),
                        label_style,
                    ));
                    line_spans.push(Span::styled(
                        format!("{:<REQ_VALUE_WIDTH$}", thread_stat.requests),
                        Style::default().fg(Color::Cyan), // Keep original value style
                    ));

                    line_spans.push(Span::styled(
                        format!("{:<ACT_PREFIX_WIDTH$}", "A:"),
                        label_style,
                    ));
                    line_spans.push(Span::styled(
                        format!("{:<ACT_VALUE_WIDTH$.1}s", last_active_secs),
                        Style::default().fg(activity_color), // Keep original value style
                    ));

                    if idx < thread_chunk.len() - 1 {
                        line_spans.push(Span::raw(" ".repeat(ITEM_SPACING as usize)));
                    }
                }
                lines.push(Line::from(line_spans));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "No active threads.",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let thread_paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("Thread Activity ({})", stats.threads.len()),
                        Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: true }); // Trim lines if they exceed paragraph width

        f.render_widget(thread_paragraph, layout_rects.threads);

        // Target状态 - Table
        let target_header_cells = ["URL", "S/F", "Rate", "RPS", "Last OK", "Last Fail", "Error"]
            .iter()
            .map(|h| {
                Cell::from(*h).style(
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
            });
        let target_header = Row::new(target_header_cells)
            .style(Style::default().bg(Color::DarkGray))
            .height(1);

        let target_rows: Vec<Row> = stats
            .targets
            .iter()
            .map(|t| {
                let success_rate = if t.success + t.failure > 0 {
                    t.success as f64 / (t.success + t.failure) as f64 * 100.0
                } else {
                    0.0
                };
                let target_rate_color = if success_rate > 80.0 {
                    Color::LightGreen
                } else if success_rate > 50.0 {
                    Color::LightYellow
                } else {
                    Color::LightRed
                };
                let last_success_str = t
                    .last_success_time
                    .map(|time| format!("{:.1}s ago", time.elapsed().as_secs_f64()))
                    .unwrap_or_else(|| "N/A".to_string());
                let last_failure_str = t
                    .last_failure_time
                    .map(|time| format!("{:.1}s ago", time.elapsed().as_secs_f64()))
                    .unwrap_or_else(|| "N/A".to_string());
                let rps_val = if let Some(_last_success_time) = t.last_success_time {
                    let elapsed_secs = stats.start_time.elapsed().as_secs_f64();
                    if elapsed_secs > 0.0 {
                        (t.success as f64 / elapsed_secs).round()
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
                let error_msg_str = t.last_network_error.as_deref().unwrap_or("-").to_string();

                Row::new(vec![
                    Cell::from(t.url.clone()).style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ), // Bold URL
                    Cell::from(format!("{}/{}", t.success, t.failure)).style(Style::default().fg(
                        if t.success > t.failure {
                            Color::LightGreen
                        } else {
                            Color::LightRed
                        },
                    )),
                    Cell::from(format!("{:.1}%", success_rate))
                        .style(Style::default().fg(target_rate_color)),
                    Cell::from(format!("{:.0}", rps_val))
                        .style(Style::default().fg(Color::LightYellow)),
                    Cell::from(last_success_str).style(Style::default().fg(Color::Green)), // Ensured DarkGreen is replaced
                    Cell::from(last_failure_str).style(Style::default().fg(Color::Red)), // Ensured DarkRed is replaced
                    Cell::from(error_msg_str).style(Style::default().fg(Color::Red)),
                ])
            })
            .collect();

        let target_rows_iter: Vec<Row> = target_rows; // Ensure type is explicit for Table::new
        let target_table_widget = Table::new(target_rows_iter)
            .widths(&[
                Constraint::Percentage(28), // URL (Min width, can expand)
                Constraint::Length(8),      // S/F
                Constraint::Length(8),      // Rate
                Constraint::Length(7),      // RPS
                Constraint::Length(12),     // Last OK
                Constraint::Length(12),     // Last Fail
                Constraint::Percentage(15), // Error (Takes remaining flexible space)
            ])
            .header(target_header)
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("Target Details ({})", stats.targets.len()),
                        Style::default()
                            .fg(Color::LightMagenta)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            );

        let mut target_table_state = TableState::default();
        layout_rects.targets = chunks[7]; // Restored index to 7
        f.render_stateful_widget(
            target_table_widget,
            layout_rects.targets,
            &mut target_table_state,
        );
    })?;

    Ok(layout_rects)
}

pub async fn run_tui(app: &mut App) -> Result<(), Box<dyn Error>> {
    app.logger.info("Starting TUI application loop.");
    let mut last_draw_time = Instant::now();
    let redraw_interval = std::time::Duration::from_millis(100);
    let mut needs_redraw = true;

    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    })?;

    let terminal = app
        .terminal
        .as_mut()
        .ok_or("Terminal not initialized for TUI mode")?;

    // 首次绘制
    {
        let stats_guard = app.stats.lock().await;
        let all_rects = draw_ui(terminal, &*stats_guard)?;
        drop(stats_guard);
        app.update_layout_rects(all_rects);
    }

    if !app.config.start_paused {
        app.spawn_data_generators();
    }

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        let mut received_input_or_event = false;
        if event::poll(std::time::Duration::from_millis(50))? {
            received_input_or_event = true;
            let event_read = event::read()?;
            // 获取当前 running_state 传递给 handle_event
            let running_state = {
                let stats_guard = app.stats.lock().await;
                stats_guard.running_state
            };
            let (redraw_from_event, app_action) =
                crate::ui::event_handler::handle_event(app, event_read, running_state);
            needs_redraw = redraw_from_event || needs_redraw;

            match app_action {
                crate::ui::event_handler::AppAction::Quit => {
                    app.logger.info(
                        "Quit action received. Signaling workers to stop and exiting immediately.",
                    );
                    let _ = app.control_tx.send(crate::worker::WorkerMessage::Stop);
                    if let Some(terminal) = app.terminal.as_mut() {
                        let _ = execute!(
                            terminal.backend_mut(),
                            LeaveAlternateScreen,
                            DisableMouseCapture
                        );
                        let _ = terminal.show_cursor();
                    }
                    let _ = disable_raw_mode();
                    app.logger
                        .info("Exiting application now via std::process::exit(0).");
                    std::process::exit(0);
                }
                _ => {}
            }
        }

        if !received_input_or_event {
            let mut stats_guard = app.stats.lock().await;
            let stats_updated = app.stats_updater.update_stats(
                &mut *stats_guard,
                &mut app.target_stats_rx,
                &app.logger,
            );
            drop(stats_guard);
            if stats_updated {
                needs_redraw = true;
            }
        }

        // 只在主循环外部调用可变self方法，避免借用冲突
        if needs_redraw || last_draw_time.elapsed() >= redraw_interval {
            let terminal_mut = app
                .terminal
                .as_mut()
                .ok_or("Terminal not available for TUI draw")?;
            let stats_guard = app.stats.lock().await;
            let all_rects = draw_ui(terminal_mut, &*stats_guard)?;
            drop(stats_guard);
            app.update_layout_rects(all_rects);
            last_draw_time = Instant::now();
            needs_redraw = false;
        }

        app.manage_data_generator().await;

        if !received_input_or_event && !needs_redraw {
            sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    Ok(())
}

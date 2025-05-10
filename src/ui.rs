use ratatui::{
    prelude::*,
    symbols,
    widgets::{BarChart, Block, Borders, Gauge, LineGauge, Paragraph, Wrap},
};
use std::collections::VecDeque;
use std::{thread, time::Instant};
use sysinfo::System;

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
    pub url: String,
    pub success: u64,
    pub failure: u64,
    pub last_success_time: Option<Instant>,
    pub last_failure_time: Option<Instant>,
    pub last_network_error: Option<String>, // 存储最后的网络错误信息
}

#[derive(Clone, Debug)] // Add Clone and Debug derives
pub struct DebugInfo {
    // Make struct public
    pub timestamp: Instant,
    pub message: String,
}

pub struct Stats {
    pub targets: Vec<TargetStats>,
    pub threads: Vec<ThreadStats>,
    pub success: u64,
    pub failure: u64,
    pub total: u64,
    pub start_time: Instant,
    pub last_success_time: Option<Instant>,
    pub last_failure_time: Option<Instant>,
    pub sys: System,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub proxy_count: usize, // Add field for proxy count
    pub running_state: RunningState,
    pub debug_logs: VecDeque<DebugInfo>, // Store recent debug logs
}

pub fn draw_ui<B: Backend>(terminal: &mut Terminal<B>, stats: &Stats) -> std::io::Result<()> {
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
                Constraint::Length(3), // 标题和控制按钮
                Constraint::Length(3), // 系统状态
                Constraint::Length(3), // 计数器
                Constraint::Length(6), // 请求统计图表
                Constraint::Length(3), // 进度条
                Constraint::Length(8), // 线程状态
                Constraint::Min(0),    // Target状态
            ])
            .split(main_chunks[0]);

        // 调试窗口
        let debug_area = main_chunks[1];
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
        let visible_height = debug_area.height.saturating_sub(2); // 减去边框高度
        let scroll = if num_logs > visible_height {
            (num_logs - visible_height, 0)
        } else {
            (0, 0)
        };

        let debug_widget = Paragraph::new(debug_messages)
            .block(Block::default().title("Console").borders(Borders::ALL))
            .wrap(Wrap { trim: false }) // 禁用自动换行，使用我们自己的换行逻辑
            .scroll(scroll);
        f.render_widget(debug_widget, debug_area);

        // 标题和状态
        let status_color = match stats.running_state {
            RunningState::Running => Color::Green,
            RunningState::Paused => Color::Yellow,
            RunningState::Stopping => Color::Red,
        };

        let version = env!("CARGO_PKG_VERSION");
        let title = format!(
            "Stormin Attack Dashboard v{} {} | Proxies: {} [Press P:Pause R:Resume Q:Quit]",
            version,
            match stats.running_state {
                RunningState::Running => "[Running]",
                RunningState::Paused => "[Paused]",
                RunningState::Stopping => "[Stopping]",
            },
            stats.proxy_count // Add proxy count here
        );

        let title = Paragraph::new(Text::styled(
            title,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ))
        .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // 系统状态 - 添加CPU和内存使用率图表
        let sys_info_block = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        // CPU使用率图表
        let cpu_ratio = (stats.cpu_usage as f64 / 100.0).clamp(0.0, 1.0);
        let cpu_gauge = LineGauge::default()
            .block(Block::default().borders(Borders::ALL).title("CPU"))
            .gauge_style(Style::default().fg(Color::Cyan))
            .line_set(symbols::line::THICK)
            .ratio(cpu_ratio);
        f.render_widget(cpu_gauge, sys_info_block[0]);

        // 内存使用率图表
        let total_mem = stats.sys.total_memory() as f64;
        let used_mem = stats.memory_usage as f64;
        let memory_ratio = if total_mem > 0.0 {
            (used_mem / total_mem).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let memory_gauge = LineGauge::default()
            .block(Block::default().borders(Borders::ALL).title("Memory"))
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
            .split(chunks[2]);

        // 添加请求速率统计
        let req_per_sec = if stats.start_time.elapsed().as_secs() > 0 {
            stats.total as f64 / stats.start_time.elapsed().as_secs() as f64
        } else {
            0.0
        };

        let total = Paragraph::new(vec![Line::from(vec![
            Span::styled("Total: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{} ({:.1}/s)", stats.total, req_per_sec),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ])])
        .block(Block::default().borders(Borders::ALL));

        let success = Paragraph::new(vec![Line::from(vec![
            Span::styled("Success: ", Style::default().fg(Color::Gray)),
            Span::styled(
                stats.success.to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " ({:.1}s)",
                    stats
                        .last_success_time
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0)
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ])])
        .block(Block::default().borders(Borders::ALL));

        let failure = Paragraph::new(vec![Line::from(vec![
            Span::styled("Failure: ", Style::default().fg(Color::Gray)),
            Span::styled(
                stats.failure.to_string(),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " ({:.1}s)",
                    stats
                        .last_failure_time
                        .map(|t| t.elapsed().as_secs_f64())
                        .unwrap_or(0.0)
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ])])
        .block(Block::default().borders(Borders::ALL));

        f.render_widget(total, counters[0]);
        f.render_widget(success, counters[1]);
        f.render_widget(failure, counters[2]);

        // 请求统计图表 - Total Requests per Target
        // 1. Collect target names into owned Strings to manage lifetime
        let target_names: Vec<String> = stats
            .targets
            .iter()
            .map(|t| {
                // Extract the last part of the URL or use the full URL
                t.url.split('/').last().unwrap_or(&t.url).to_string()
            })
            .collect();

        // 2. Create the data tuples using references to the owned Strings
        let chart_data_tuples: Vec<(&str, u64)> = target_names
            .iter()
            .zip(stats.targets.iter())
            .map(|(name, t)| (name.as_str(), t.success + t.failure)) // Use name.as_str()
            .collect();

        // 3. Modify the BarChart call
        let barchart = BarChart::default()
            .block(
                Block::default()
                    // Update title to reflect the new data
                    .title("Total Requests by Target")
                    .borders(Borders::ALL),
            )
            // Pass the data as a slice of tuples
            .data(chart_data_tuples.as_slice())
            .bar_width(5) // Can adjust width now, maybe wider bars
            .bar_gap(1)
            // Set a single style for all bars in this single group
            .bar_style(Style::default().fg(Color::Blue))
            .value_style(
                Style::default()
                    .fg(Color::White) // Style for values shown under bars
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(barchart, chunks[3]);

        // 成功率进度条
        let success_rate = if stats.total > 0 {
            (stats.success as f64 / stats.total as f64 * 100.0) as u16
        } else {
            0
        };
        let success_color = if success_rate > 80 {
            Color::Green
        } else if success_rate > 50 {
            Color::Yellow
        } else {
            Color::Red
        };
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .title(format!("Success Rate: {}%", success_rate))
                    .borders(Borders::ALL),
            )
            .gauge_style(Style::default().fg(success_color))
            .percent(success_rate);
        f.render_widget(gauge, chunks[4]);

        // 线程状态
        let thread_info: Vec<Line> = stats
            .threads
            .iter()
            .map(|t| {
                let last_active_secs = t.last_active.elapsed().as_secs_f64();
                let status_color = if last_active_secs < 1.0 {
                    Color::Green
                } else if last_active_secs < 5.0 {
                    Color::Yellow
                } else {
                    Color::Red
                };

                Line::from(vec![
                    Span::raw(format!("Thread {:?}: ", t.id)),
                    Span::styled(
                        format!("{} req", t.requests),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(" | "),
                    Span::styled(
                        format!("{:.1}s ago", last_active_secs),
                        Style::default().fg(status_color),
                    ),
                ])
            })
            .collect();

        let thread_status = Paragraph::new(thread_info)
            .block(
                Block::default()
                    .title(format!("Thread Status ({})", stats.threads.len()))
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(thread_status, chunks[5]);

        // Target状态
        let targets_info: Vec<Line> = stats
            .targets
            .iter()
            .flat_map(|t| {
                let success_rate = if t.success + t.failure > 0 {
                    t.success as f64 / (t.success + t.failure) as f64 * 100.0
                } else {
                    0.0
                };

                let status_color = if success_rate > 80.0 {
                    Color::Green
                } else if success_rate > 50.0 {
                    Color::Yellow
                } else {
                    Color::Red
                };

                let last_success = t
                    .last_success_time
                    .map(|t| t.elapsed().as_secs_f64())
                    .map(|s| format!("{:.1}s ago", s))
                    .unwrap_or_else(|| "N/A".to_string());

                let last_failure = t
                    .last_failure_time
                    .map(|t| t.elapsed().as_secs_f64())
                    .map(|s| format!("{:.1}s ago", s))
                    .unwrap_or_else(|| "N/A".to_string());

                let rps = if let Some(time) = t.last_success_time {
                    (t.success as f64 / time.elapsed().as_secs_f64()).round()
                } else {
                    0.0
                };

                let mut lines = vec![
                    Line::from(vec![
                        Span::styled("URL: ", Style::default().fg(Color::Gray)),
                        Span::styled(&t.url, Style::default().fg(Color::Cyan)),
                    ]),
                    Line::from(vec![
                        Span::styled("Success/Failure: ", Style::default().fg(Color::Gray)),
                        Span::styled(t.success.to_string(), Style::default().fg(Color::Green)),
                        Span::raw("/"),
                        Span::styled(t.failure.to_string(), Style::default().fg(Color::Red)),
                        Span::raw("  Rate: "),
                        Span::styled(
                            format!("{:.1}%", success_rate),
                            Style::default().fg(status_color),
                        ),
                        Span::raw("  RPS: "),
                        Span::styled(format!("{:.0}", rps), Style::default().fg(Color::Yellow)),
                    ]),
                    Line::from(vec![
                        Span::styled("Last Success: ", Style::default().fg(Color::Gray)),
                        Span::styled(last_success, Style::default().fg(Color::Green)),
                        Span::raw("  Last Failure: "),
                        Span::styled(last_failure.clone(), Style::default().fg(Color::Red)), // Use clone here if needed later
                    ]),
                ];

                // 如果存在最后的网络错误，并且最后一次失败就是这次网络错误（或没有失败记录），则显示它
                if let Some(error_msg) = &t.last_network_error {
                     // 只在最近一次失败是网络错误时显示，避免显示过时的网络错误
                    if t.last_failure_time.is_some() && t.last_network_error.is_some() {
                        // 显示最新的网络错误
                        lines.push(Line::from(vec![
                           Span::styled("  └─ Last Error: ", Style::default().fg(Color::DarkGray)),
                           Span::styled(error_msg, Style::default().fg(Color::LightRed)),
                        ]));
                    } else if t.last_network_error.is_some() { // 如果没有失败记录但有网络错误
                         lines.push(Line::from(vec![
                           Span::styled("  └─ Last Error: ", Style::default().fg(Color::DarkGray)),
                           Span::styled(error_msg, Style::default().fg(Color::LightRed)),
                        ]));
                    }

                }


                lines.push(Line::default()); // Add the empty line separator
                lines // Return the vector of lines
            })
            .collect();

        let targets_status = Paragraph::new(targets_info)
            .block(
                Block::default()
                    .title(format!("Targets Status ({})", stats.targets.len()))
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(targets_status, chunks[6]);
    })?;
    Ok(())
}

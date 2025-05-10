use ratatui::{
    prelude::*,
    symbols,
    widgets::{
        BarChart, Block, BorderType, Borders, Cell, Gauge, LineGauge, Paragraph, Row, Table, Wrap, // Added Bar
    },
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
    pub id: usize, // Unique ID for the target
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
                Constraint::Length(7), // 请求统计图表 (Increased height for labels)
                Constraint::Length(3), // 进度条
                Constraint::Min(5),    // 线程状态 (Min height, can expand)
                Constraint::Percentage(60), // Target状态 (Takes a good portion of remaining space)
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
                    .border_style(Style::default().fg(Color::DarkGray)), // Added consistent border style
            )
            .wrap(Wrap { trim: false })
            .scroll(scroll);
        f.render_widget(debug_widget, debug_area);

        // 标题和状态
        let status_color = match stats.running_state {
            RunningState::Running => Color::LightGreen,
            RunningState::Paused => Color::LightYellow,
            RunningState::Stopping => Color::LightRed,
        };

        let version = env!("CARGO_PKG_VERSION");
        let title_str = format!(
            "Stormin Attack Dashboard v{} {} | Proxies: {} [P:Pause R:Resume Q:Quit]",
            version,
            match stats.running_state {
                RunningState::Running => "[Running]",
                RunningState::Paused => "[Paused]",
                RunningState::Stopping => "[Stopping]",
            },
            stats.proxy_count
        );

        let title_widget = Paragraph::new(Text::styled(
            title_str,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(title_widget, chunks[0]);

        // 系统状态 - 添加CPU和内存使用率图表
        let sys_info_block = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        // CPU使用率图表
        let cpu_ratio = (stats.cpu_usage as f64 / 100.0).clamp(0.0, 1.0);
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
        let memory_ratio = if total_mem > 0.0 {
            (used_mem / total_mem).clamp(0.0, 1.0)
        } else {
            0.0
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
        .block(
            Block::default()
                .title(Span::styled("Total", Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD))) // Consistent title
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        let success = Paragraph::new(vec![Line::from(vec![
            Span::styled("Count: ", Style::default().fg(Color::Gray)),
            Span::styled(
                stats.success.to_string(),
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
                .title(Span::styled("Success", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD))) // Consistent title
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        let failure = Paragraph::new(vec![Line::from(vec![
            Span::styled("Count: ", Style::default().fg(Color::Gray)),
            Span::styled(
                stats.failure.to_string(),
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
                .title(Span::styled("Failure", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD))) // Consistent title
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        f.render_widget(total, counters[0]);
        f.render_widget(success, counters[1]);
        f.render_widget(failure, counters[2]);

        // 请求统计图表 - Corrected data structure for BarChart
        let target_names_for_chart: Vec<String> = stats
            .targets
            .iter()
            .map(|t| t.url.split('/').last().unwrap_or(&t.url).to_string())
            .collect();

        // Create the data tuples using references to the owned Strings
        let chart_data_tuples: Vec<(&str, u64)> = target_names_for_chart
            .iter()
            .zip(stats.targets.iter())
            .map(|(name, t)| (name.as_str(), t.success + t.failure))
            .collect();

        let barchart = BarChart::default()
            .block(
                Block::default()
                    .title(Span::styled(
                        "Requests by Target",
                        Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .data(chart_data_tuples.as_slice()) // Correctly use Vec<(&str, u64)>
            .bar_width(6)
            .bar_gap(1)
            .bar_style(Style::default().fg(Color::LightYellow)) // Single color for all bars
            .value_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .label_style(Style::default().fg(Color::DarkGray));
        f.render_widget(barchart, chunks[3]);

        // 成功率进度条
        let success_rate = if stats.total > 0 {
            (stats.success as f64 / stats.total as f64 * 100.0) as u16
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
        f.render_widget(gauge, chunks[4]);

        // 线程状态 - Table
        let thread_header_cells = ["Thread ID", "Requests", "Last Active"].iter().map(|h| {
            Cell::from(*h).style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
        });
        let thread_header = Row::new(thread_header_cells)
            .style(Style::default().bg(Color::DarkGray)) // Header background
            .height(1);

        let thread_rows: Vec<Row> = stats
            .threads
            .iter()
            .map(|t| {
                let last_active_secs = t.last_active.elapsed().as_secs_f64();
                let activity_color = if last_active_secs < 1.0 {
                    Color::LightGreen
                } else if last_active_secs < 5.0 {
                    Color::LightYellow
                } else {
                    Color::LightRed
                };
                Row::new(vec![
                    Cell::from(format!("{:?}", t.id)),
                    Cell::from(t.requests.to_string()).style(Style::default().fg(Color::Cyan)),
                    Cell::from(format!("{:.1}s ago", last_active_secs))
                        .style(Style::default().fg(activity_color)),
                ])
            })
            .collect();

        let thread_rows_iter: Vec<Row> = thread_rows; // Ensure type is explicit for Table::new
        let thread_table = Table::new(thread_rows_iter)
            .widths(&[
                Constraint::Percentage(40), // Thread ID
                Constraint::Percentage(30), // Requests
                Constraint::Percentage(30), // Last Active
            ])
            .header(thread_header)
            .block(
                Block::default()
                    .title(Span::styled(
                        format!("Thread Activity ({})", stats.threads.len()),
                        Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD), // Consistent title
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .highlight_style(Style::default().fg(Color::Black).bg(Color::LightCyan)) // More visible highlight
            .highlight_symbol("▶ "); // Different highlight symbol
        f.render_widget(thread_table, chunks[5]);

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
                    Cell::from(t.url.clone()).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)), // Bold URL
                    Cell::from(format!("{}/{}", t.success, t.failure)).style(Style::default().fg(
                        if t.success > t.failure { Color::LightGreen } else { Color::LightRed }
                    )),
                    Cell::from(format!("{:.1}%", success_rate)).style(Style::default().fg(target_rate_color)),
                    Cell::from(format!("{:.0}", rps_val)).style(Style::default().fg(Color::LightYellow)),
                    Cell::from(last_success_str).style(Style::default().fg(Color::Green)), // Ensured DarkGreen is replaced
                    Cell::from(last_failure_str).style(Style::default().fg(Color::Red)),     // Ensured DarkRed is replaced
                    Cell::from(error_msg_str).style(Style::default().fg(Color::Red)),
                ])
            })
            .collect();

        let target_rows_iter: Vec<Row> = target_rows; // Ensure type is explicit for Table::new
        let target_table = Table::new(target_rows_iter)
            .widths(&[
                Constraint::Min(20),        // URL (Min width, can expand)
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
                        Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD), // Consistent title
                    ))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .highlight_style(Style::default().fg(Color::Black).bg(Color::LightCyan)) // Consistent highlight
            .highlight_symbol("▶ ");
        f.render_widget(target_table, chunks[6]);
    })?;
    Ok(())
}

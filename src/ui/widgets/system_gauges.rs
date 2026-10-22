use crate::types::Stats;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, LineGauge},
};

pub fn render(f: &mut Frame, stats: &Stats, area: Rect) {
    let sys_info_block = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let cpu_usage_f64 = stats.cpu_usage as f64;
    let cpu_ratio = if cpu_usage_f64.is_nan() {
        0.0
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
        .filled_style(Style::default().fg(Color::Cyan))
        .ratio(cpu_ratio);
    f.render_widget(cpu_gauge, sys_info_block[0]);

    let total_mem = stats.sys.total_memory() as f64;
    let used_mem = stats.memory_usage as f64;
    let memory_ratio = match (total_mem, used_mem) {
        (t, _) if t <= 0.0 => 0.0,
        (t, u) if u.is_nan() || t.is_nan() => 0.0,
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
        .filled_style(Style::default().fg(Color::Magenta))
        .ratio(memory_ratio);
    f.render_widget(memory_gauge, sys_info_block[1]);
}

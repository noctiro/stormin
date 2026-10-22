use crate::types::Stats;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

pub fn render(f: &mut Frame, stats: &Stats, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    let max_bar_width = area.width.saturating_sub(30);

    if !stats.targets.is_empty() && max_bar_width > 10 {
        let max_total_req: u64 = stats
            .targets
            .iter()
            .map(|t| t.success + t.failure)
            .max()
            .unwrap_or(1)
            .max(1);

        for target_stat in &stats.targets {
            let target_name = target_stat
                .url
                .split('/')
                .last()
                .unwrap_or(&target_stat.url);

            let success_bar_len = if max_total_req > 0 {
                (target_stat.success as f64 / max_total_req as f64 * max_bar_width as f64) as u16
            } else {
                0
            };
            let failure_bar_len = if max_total_req > 0 {
                (target_stat.failure as f64 / max_total_req as f64 * max_bar_width as f64) as u16
            } else {
                0
            };

            let current_total_bar = success_bar_len + failure_bar_len;
            let (s_len, f_len) = if current_total_bar > max_bar_width {
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
                    format!("{:<15.15}: ", target_name),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    "\u{2592}".repeat(s_len as usize),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    "\u{2592}".repeat(f_len as usize),
                    Style::default().fg(Color::Red),
                ),
            ];
            lines.push(Line::from(line_spans));
        }
    } else if stats.targets.is_empty() {
        lines.push(Line::from("No targets to display stats for."));
    } else {
        lines.push(Line::from("Not enough space for target stats bars."));
    }

    let widget = Paragraph::new(lines)
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
    f.render_widget(widget, area);
}

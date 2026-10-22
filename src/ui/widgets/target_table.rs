use crate::types::Stats;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Cell, Row, Table, TableState},
};

fn format_elapsed(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.1}s ago", secs)
    } else if secs < 3600.0 {
        format!("{:.1}m ago", secs / 60.0)
    } else if secs < 86400.0 {
        format!("{:.1}h ago", secs / 3600.0)
    } else {
        format!("{:.1}d ago", secs / 86400.0)
    }
}

pub fn render(f: &mut Frame, stats: &Stats, area: Rect) {
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
                .map(|time| format_elapsed(time.elapsed().as_secs_f64()))
                .unwrap_or_else(|| "N/A".to_string());
            let last_failure_str = t
                .last_failure_time
                .map(|time| format_elapsed(time.elapsed().as_secs_f64()))
                .unwrap_or_else(|| "N/A".to_string());
            let rps_val = if t.last_success_time.is_some() {
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
                ),
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
                Cell::from(last_success_str).style(Style::default().fg(Color::Green)),
                Cell::from(last_failure_str).style(Style::default().fg(Color::Red)),
                Cell::from(error_msg_str).style(Style::default().fg(Color::Red)),
            ])
        })
        .collect();

    let visible_target_rows = {
        let avail = area.height.saturating_sub(3) as usize;
        if target_rows.len() > avail {
            let offset =
                (stats.start_time.elapsed().as_secs() as usize) % (target_rows.len() - avail + 1);
            target_rows
                .into_iter()
                .skip(offset)
                .take(avail)
                .collect::<Vec<Row>>()
        } else {
            target_rows
        }
    };

    let target_table_widget = Table::new(
        visible_target_rows,
        [
            Constraint::Percentage(34),
            Constraint::Percentage(16),
            Constraint::Percentage(6),
            Constraint::Percentage(5),
            Constraint::Percentage(8),
            Constraint::Percentage(8),
            Constraint::Percentage(23),
        ],
    )
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
    f.render_stateful_widget(target_table_widget, area, &mut target_table_state);
}

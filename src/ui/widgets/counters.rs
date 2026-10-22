use crate::types::Stats;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph},
};

pub fn render(f: &mut Frame, stats: &Stats, area: Rect) {
    let counters = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);

    let elapsed_secs = stats.start_time.elapsed().as_secs() as f64;
    let req_per_sec = if elapsed_secs > 0.0 && stats.get_total() > 0 {
        (stats.get_total() as f64 / elapsed_secs).max(0.0)
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
            ))
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
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(total, counters[0]);
    f.render_widget(success, counters[1]);
    f.render_widget(failure, counters[2]);
}

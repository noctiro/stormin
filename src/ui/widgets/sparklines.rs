use crate::types::Stats;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Sparkline},
};

pub fn render(f: &mut Frame, stats: &Stats, area: Rect) {
    let sparkline_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(area);

    let rps_data: Vec<u64> = stats.rps_history.iter().cloned().collect();
    let rps_sparkline = Sparkline::default()
        .block(
            Block::default()
                .title(Span::styled(
                    "Total RPS Trend",
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .data(&rps_data)
        .style(Style::default().fg(Color::LightYellow));
    f.render_widget(rps_sparkline, sparkline_chunks[0]);

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
        .style(Style::default().fg(Color::LightGreen));
    f.render_widget(successful_rps_sparkline, sparkline_chunks[1]);

    let success_rate_data: Vec<u64> = stats.success_rate_history.iter().cloned().collect();
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
        .data(&success_rate_data)
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(success_rate_sparkline, sparkline_chunks[2]);
}

use crate::types::Stats;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Gauge},
};

pub fn render(f: &mut Frame, stats: &Stats, area: Rect) {
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
    f.render_widget(gauge, area);
}

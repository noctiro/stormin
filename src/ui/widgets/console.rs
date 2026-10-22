use crate::types::Stats;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

pub fn render(f: &mut Frame, stats: &Stats, area: Rect) {
    let debug_messages: Vec<Line> = stats
        .debug_logs
        .iter()
        .flat_map(|log| {
            let message_lines = log.message.lines().collect::<Vec<_>>();
            let mut formatted_lines = Vec::with_capacity(message_lines.len());

            formatted_lines.push(Line::from(vec![
                Span::styled(
                    format!("[{:.1}s] ", log.timestamp.elapsed().as_secs_f64()),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(message_lines[0]),
            ]));

            for line in message_lines.iter().skip(1) {
                formatted_lines.push(Line::from(vec![
                    Span::styled("       ", Style::default().fg(Color::DarkGray)),
                    Span::raw(*line),
                ]));
            }

            formatted_lines
        })
        .collect();

    let num_logs = debug_messages.len() as u16;
    let visible_height = area.height.saturating_sub(2);

    let current_scroll_offset = if num_logs > visible_height {
        num_logs - visible_height
    } else {
        0
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
        .scroll((current_scroll_offset, 0));
    f.render_widget(debug_widget, area);
}

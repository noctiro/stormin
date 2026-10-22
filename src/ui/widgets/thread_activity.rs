use crate::types::Stats;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

pub fn render(f: &mut Frame, stats: &Stats, area: Rect) {
    let available_width = area.width.saturating_sub(2);

    const TID_PREFIX_WIDTH: usize = 4;
    const TID_VALUE_WIDTH: usize = 15;
    const REQ_PREFIX_WIDTH: usize = 2;
    const REQ_VALUE_WIDTH: usize = 6;
    const ACT_PREFIX_WIDTH: usize = 2;
    const ACT_VALUE_WIDTH: usize = 6;
    const ITEM_SPACING: u16 = 2;

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
                line_spans.push(Span::raw(format!("{:<TID_VALUE_WIDTH$}", tid_display)));

                line_spans.push(Span::styled(
                    format!("{:<REQ_PREFIX_WIDTH$}", "R:"),
                    label_style,
                ));
                line_spans.push(Span::styled(
                    format!("{:<REQ_VALUE_WIDTH$}", thread_stat.requests),
                    Style::default().fg(Color::Cyan),
                ));

                line_spans.push(Span::styled(
                    format!("{:<ACT_PREFIX_WIDTH$}", "A:"),
                    label_style,
                ));
                line_spans.push(Span::styled(
                    format!("{:<ACT_VALUE_WIDTH$.1}s", last_active_secs),
                    Style::default().fg(activity_color),
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

    let visible_thread_lines = area.height.saturating_sub(2);
    let total_thread_lines = lines.len() as u16;
    let thread_scroll = if total_thread_lines > visible_thread_lines {
        total_thread_lines - visible_thread_lines
    } else {
        0
    };
    let thread_offset = if thread_scroll > 0 {
        (stats.start_time.elapsed().as_secs() as u16) % (thread_scroll + 1)
    } else {
        0
    };

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
        .wrap(Wrap { trim: true })
        .scroll((thread_offset, 0));
    f.render_widget(thread_paragraph, area);
}

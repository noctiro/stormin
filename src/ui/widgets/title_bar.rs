use crate::types::{RunningState, Stats};
use crate::ui::LayoutRects;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph},
};

pub fn render(f: &mut Frame, stats: &Stats, area: Rect, layout_rects: &mut LayoutRects) {
    let title_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(27)])
        .split(area);

    let status_color = match stats.running_state {
        RunningState::Running => Color::Rgb(0, 200, 0),
        RunningState::Paused => Color::Rgb(255, 165, 0),
        RunningState::Stopping => Color::Rgb(200, 0, 0),
    };

    let version = env!("CARGO_PKG_VERSION");
    let elapsed_time_secs = stats.start_time.elapsed().as_secs();
    let elapsed_str = format!(
        "{:02}:{:02}:{:02}",
        elapsed_time_secs / 3600,
        (elapsed_time_secs % 3600) / 60,
        elapsed_time_secs % 60
    );
    let main_title_str = format!(
        "Stormin Dashboard v{} {} | Elapsed: {} | Proxies: {}",
        version,
        match stats.running_state {
            RunningState::Running => "[Running]",
            RunningState::Paused => "[Paused]",
            RunningState::Stopping => "[Stopping]",
        },
        elapsed_str,
        stats.proxy_count
    );

    let title_paragraph = Paragraph::new(Text::styled(
        main_title_str,
        Style::default()
            .fg(status_color)
            .add_modifier(Modifier::BOLD),
    ))
    .block(
        Block::default()
            .borders(Borders::LEFT | Borders::TOP | Borders::BOTTOM)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(100, 100, 100))),
    );
    f.render_widget(title_paragraph, title_chunks[0]);

    // Buttons
    let button_area = title_chunks[1];
    let button_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
        ])
        .split(button_area);

    layout_rects.pause_btn = button_chunks[0];
    layout_rects.resume_btn = button_chunks[1];
    layout_rects.quit_btn = button_chunks[2];

    let base_button_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    // Pause Button
    let pause_text = if stats.running_state == RunningState::Running {
        "[P]ause"
    } else {
        "Pause"
    };
    let pause_text_color = if stats.running_state == RunningState::Running {
        Color::Rgb(0, 180, 255)
    } else {
        Color::White
    };
    let pause_border_color = if stats.running_state == RunningState::Running {
        Color::Rgb(0, 180, 255)
    } else {
        Color::Gray
    };
    let pause_button = Paragraph::new(pause_text)
        .style(Style::default().fg(pause_text_color))
        .block(
            base_button_block
                .clone()
                .border_style(Style::default().fg(pause_border_color)),
        )
        .alignment(Alignment::Center);
    f.render_widget(pause_button, layout_rects.pause_btn);

    // Resume Button
    let resume_text = if stats.running_state == RunningState::Paused {
        "[R]esume"
    } else {
        "Resume"
    };
    let resume_text_color = if stats.running_state == RunningState::Paused {
        Color::Yellow
    } else {
        Color::White
    };
    let resume_border_color = if stats.running_state == RunningState::Paused {
        Color::Yellow
    } else {
        Color::Gray
    };
    let resume_button = Paragraph::new(resume_text)
        .style(Style::default().fg(resume_text_color))
        .block(
            base_button_block
                .clone()
                .border_style(Style::default().fg(resume_border_color)),
        )
        .alignment(Alignment::Center);
    f.render_widget(resume_button, layout_rects.resume_btn);

    // Quit Button
    let quit_button = Paragraph::new("[Q]uit")
        .style(Style::default().fg(Color::Red))
        .block(
            base_button_block
                .clone()
                .border_style(Style::default().fg(Color::Red)),
        )
        .alignment(Alignment::Center);
    f.render_widget(quit_button, layout_rects.quit_btn);
}

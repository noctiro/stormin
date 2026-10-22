pub mod console;
pub mod counters;
pub mod progress_bar;
pub mod sparklines;
pub mod system_gauges;
pub mod target_chart;
pub mod target_table;
pub mod thread_activity;
pub mod title_bar;

use crate::types::Stats;
use crate::ui::LayoutRects;
use ratatui::prelude::*;

pub fn render_all(f: &mut Frame, stats: &Stats, layout_rects: &mut LayoutRects) {
    let size = f.area();

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(size);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Percentage(50),
        ])
        .split(main_chunks[0]);

    layout_rects.title_bar = chunks[0];
    layout_rects.console = main_chunks[1];

    console::render(f, stats, layout_rects.console);
    title_bar::render(f, stats, chunks[0], layout_rects);
    system_gauges::render(f, stats, chunks[1]);
    counters::render(f, stats, chunks[2]);
    sparklines::render(f, stats, chunks[3]);
    target_chart::render(f, stats, chunks[4]);
    progress_bar::render(f, stats, chunks[5]);

    layout_rects.threads = chunks[6];
    thread_activity::render(f, stats, layout_rects.threads);

    layout_rects.targets = chunks[7];
    target_table::render(f, stats, chunks[7]);
}

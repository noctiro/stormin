pub mod cli;
pub mod event_handler;
pub mod stats_updater;
pub mod widgets;

use crate::app::App;
use crate::types::{Stats, WorkerMessage};
use crossterm::event;
use ratatui::prelude::*;
use std::error::Error;
use std::time::Instant;
use tokio::time::sleep;

#[derive(Default, Clone, Copy)]
pub struct LayoutRects {
    pub console: Rect,
    pub threads: Rect,
    pub targets: Rect,
    pub pause_btn: Rect,
    pub resume_btn: Rect,
    pub quit_btn: Rect,
    pub title_bar: Rect,
}

pub fn draw_ui<B: Backend>(
    terminal: &mut Terminal<B>,
    stats: &Stats,
) -> std::io::Result<LayoutRects>
where
    std::io::Error: From<B::Error>,
{
    let mut layout_rects = LayoutRects::default();

    terminal.draw(|f| {
        widgets::render_all(f, stats, &mut layout_rects);
    })?;

    Ok(layout_rects)
}

pub async fn run_tui(app: &mut App) -> Result<(), Box<dyn Error>> {
    app.logger.info("Starting TUI application loop.");
    let mut last_draw_time = Instant::now();
    let redraw_interval = std::time::Duration::from_millis(100);
    let mut needs_redraw = true;

    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    })?;

    let terminal = app
        .terminal
        .as_mut()
        .ok_or("Terminal not initialized for TUI mode")?;

    {
        let stats_guard = app.stats.lock().await;
        let all_rects = draw_ui(terminal, &*stats_guard)?;
        drop(stats_guard);
        app.update_layout_rects(all_rects);
    }

    if !app.config.start_paused {
        app.spawn_data_generators();
    }

    while running.load(std::sync::atomic::Ordering::SeqCst) {
        let mut received_input_or_event = false;
        if event::poll(std::time::Duration::from_millis(50))? {
            received_input_or_event = true;
            let event_read = event::read()?;
            let running_state = {
                let stats_guard = app.stats.lock().await;
                stats_guard.running_state
            };
            let (redraw_from_event, app_action) =
                crate::ui::event_handler::handle_event(app, event_read, running_state);
            needs_redraw = redraw_from_event || needs_redraw;

            match app_action {
                crate::ui::event_handler::AppAction::Quit => {
                    app.logger.info(
                        "Quit action received. Signaling workers to stop and preparing to exit.",
                    );
                    let _ = app.control_tx.send(WorkerMessage::Stop);
                    app.logger.info("Exiting TUI loop.");
                    break;
                }
                _ => {}
            }
        }

        if !received_input_or_event {
            let mut stats_guard = app.stats.lock().await;
            let stats_updated = app.stats_updater.update_stats(
                &mut *stats_guard,
                &mut app.target_stats_rx,
                &app.logger,
            );
            drop(stats_guard);
            if stats_updated {
                needs_redraw = true;
            }
        }

        if needs_redraw || last_draw_time.elapsed() >= redraw_interval {
            let terminal_mut = app
                .terminal
                .as_mut()
                .ok_or("Terminal not available for TUI draw")?;
            let stats_guard = app.stats.lock().await;
            let all_rects = draw_ui(terminal_mut, &*stats_guard)?;
            drop(stats_guard);
            app.update_layout_rects(all_rects);
            last_draw_time = Instant::now();
            needs_redraw = false;
        }

        app.manage_data_generator().await;

        if !received_input_or_event && !needs_redraw {
            sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    Ok(())
}

use std::sync::atomic::Ordering;

use crate::app::App;
use crate::ui::RunningState;
use crate::worker::WorkerMessage;
use crossterm::event::{Event, KeyCode, MouseButton, MouseEventKind};
use tokio::runtime::Handle;

// Define actions that can result from event handling
#[derive(Debug, PartialEq, Eq)]
pub enum AppAction {
    Quit,
    Pause,
    Resume,
    NoAction,
}

// Returns (needs_redraw, AppAction)
pub fn handle_event(app: &mut App, event: Event) -> (bool, AppAction) {
    let mut needs_redraw = false;
    let mut app_action = AppAction::NoAction;

    // 预先获取当前状态，避免多次锁定
    let running_state = {
        let stats = Handle::current().block_on(app.stats.lock());
        stats.running_state
    };

    match event {
        Event::Key(key) => {
            needs_redraw = true; // Assume any key press might change state initially
            match key.code {
                KeyCode::Char('q') => {
                    app_action = AppAction::Quit;
                }
                KeyCode::Char('p') if running_state == RunningState::Running => {
                    app_action = AppAction::Pause;
                }
                KeyCode::Char('r') if running_state == RunningState::Paused => {
                    app_action = AppAction::Resume;
                }
                _ => {
                    needs_redraw = false; // Unhandled key, no redraw needed
                }
            }
        }
        Event::Mouse(mouse_event) => {
            needs_redraw = true; // Assume mouse event might change something initially
            match mouse_event.kind {
                MouseEventKind::Down(button) => {
                    if button == MouseButton::Left {
                        let (col, row) = (mouse_event.column, mouse_event.row);
                        let pause_rect = app.layout_rects.pause_btn;
                        let resume_rect = app.layout_rects.resume_btn;
                        let quit_rect = app.layout_rects.quit_btn;

                        if pause_rect.contains(col, row) {
                            if running_state == RunningState::Running {
                                app_action = AppAction::Pause;
                            } else {
                                needs_redraw = false; // Clicked pause when already paused
                            }
                        } else if resume_rect.contains(col, row) {
                            if running_state == RunningState::Paused {
                                app_action = AppAction::Resume;
                            } else {
                                needs_redraw = false; // Clicked resume when already running
                            }
                        } else if quit_rect.contains(col, row) {
                            app_action = AppAction::Quit;
                        } else {
                            needs_redraw = false; // Click was not on a known button
                        }
                    } else {
                        needs_redraw = false; // Not a left click
                    }
                }
                _ => {
                    needs_redraw = false; // Other mouse events like Move, Drag, etc.
                }
            }
        }
        _ => { /* Unhandled terminal event */ }
    }

    // Apply actions based on AppAction
    // This part is crucial and needs access to app's state and methods
    match app_action {
        AppAction::Pause => {
            let mut stats = Handle::current().block_on(app.stats.lock());
            if stats.running_state == RunningState::Running {
                stats.running_state = RunningState::Paused;
                app.logger
                    .info("Pausing workers and data generator (event)...");
                app.data_generator_stop_signal
                    .store(true, Ordering::Relaxed);
                if let Err(e) = app.control_tx.send(WorkerMessage::Pause) {
                    app.logger
                        .warning(&format!("Failed to broadcast Pause message: {}", e));
                }
            }
        }
        AppAction::Resume => {
            let mut stats = Handle::current().block_on(app.stats.lock());
            let need_spawn = stats.running_state == RunningState::Paused
                && app.data_generator_handles.is_empty();
            if stats.running_state == RunningState::Paused {
                stats.running_state = RunningState::Running;
                app.logger
                    .info("Resuming workers and data generator (event)...");
                app.data_generator_stop_signal
                    .store(false, Ordering::Relaxed);
            }
            drop(stats);
            if need_spawn {
                app.logger
                    .info("Data generators were stopped, respawning...");
                app.spawn_data_generators();
            }
            if let Err(e) = app.control_tx.send(WorkerMessage::Resume) {
                app.logger
                    .warning(&format!("Failed to broadcast Resume message: {}", e));
            }
        }
        AppAction::Quit => {
            app.logger.info("Quitting application (event)...");
            // The main loop will handle the actual exit based on the running flag
        }
        AppAction::NoAction => {
            // If no specific action, but an event was handled that didn't lead to Pause/Resume/Quit,
            // `needs_redraw` would have been set accordingly by the event matching logic.
        }
    }

    (needs_redraw, app_action)
}

// Extension trait for ratatui::layout::Rect to add a contains method
pub trait RectContainsPoint {
    fn contains(&self, x: u16, y: u16) -> bool;
}

impl RectContainsPoint for ratatui::layout::Rect {
    fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

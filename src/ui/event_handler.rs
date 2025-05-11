use crate::app::App; // Assuming App will be accessible, might need to adjust path or make App fields public
use crate::ui::RunningState;
use crate::worker::WorkerMessage;
use crossterm::event::{Event, KeyCode, MouseButton, MouseEventKind};

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

    match event {
        Event::Key(key) => {
            needs_redraw = true; // Assume any key press might change state initially
            match key.code {
                KeyCode::Char('q') => {
                    app_action = AppAction::Quit;
                }
                KeyCode::Char('p') if app.stats.running_state == RunningState::Running => {
                    app_action = AppAction::Pause;
                }
                KeyCode::Char('r') if app.stats.running_state == RunningState::Paused => {
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
                            if app.stats.running_state == RunningState::Running {
                                app_action = AppAction::Pause;
                            } else {
                                needs_redraw = false; // Clicked pause when already paused
                            }
                        } else if resume_rect.contains(col, row) {
                            if app.stats.running_state == RunningState::Paused {
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
            if app.stats.running_state == RunningState::Running {
                app.stats.running_state = RunningState::Paused;
                app.logger
                    .info("Pausing workers and data generator (event)...");
                app.data_generator.set_running_flag(false);
                if let Err(e) = app.control_tx.send(WorkerMessage::Pause) {
                    app.logger
                        .warning(&format!("Failed to broadcast Pause message: {}", e));
                }
            }
        }
        AppAction::Resume => {
            if app.stats.running_state == RunningState::Paused {
                app.stats.running_state = RunningState::Running;
                app.logger
                    .info("Resuming workers and data generator (event)...");
                if !app.data_generator.is_running() {
                    if app.data_generator.is_finished() {
                        app.logger
                            .info("Data generator was stopped or finished, respawning (event)...");
                        app.data_generator.spawn();
                    } else {
                        app.data_generator.set_running_flag(true);
                        app.logger
                            .info("Data generator was paused, resuming (event)...");
                    }
                }
                if let Err(e) = app.control_tx.send(WorkerMessage::Resume) {
                    app.logger
                        .warning(&format!("Failed to broadcast Resume message: {}", e));
                }
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

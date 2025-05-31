use std::sync::atomic::Ordering;

use crate::app::App;
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
// 推荐：将 running_state 作为参数传入，避免同步锁
pub fn handle_event(app: &mut App, event: Event, running_state: RunningState) -> (bool, AppAction) {
    let mut needs_redraw = false;
    let mut app_action = AppAction::NoAction;

    match event {
        Event::Key(key) => {
            needs_redraw = true;
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
                    needs_redraw = false;
                }
            }
        }
        Event::Mouse(mouse_event) => {
            needs_redraw = true;
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
                                needs_redraw = false;
                            }
                        } else if resume_rect.contains(col, row) {
                            if running_state == RunningState::Paused {
                                app_action = AppAction::Resume;
                            } else {
                                needs_redraw = false;
                            }
                        } else if quit_rect.contains(col, row) {
                            app_action = AppAction::Quit;
                        } else {
                            needs_redraw = false;
                        }
                    } else {
                        needs_redraw = false;
                    }
                }
                _ => {
                    needs_redraw = false;
                }
            }
        }
        _ => {}
    }

    // Apply actions based on AppAction
    match app_action {
        AppAction::Pause => {
            let mut stats = app.stats.blocking_lock();
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
            let mut stats = app.stats.blocking_lock();
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
        }
        AppAction::NoAction => {}
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

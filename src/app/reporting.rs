use super::App;
use crate::types::RunningState;
use std::sync::atomic::Ordering;

impl App {
    pub async fn manage_data_generator(&mut self) {
        let running_state = {
            let stats = self.stats.lock().await;
            stats.running_state
        };
        let currently_stopped = self.data_generator_stop_signal.load(Ordering::SeqCst);
        if running_state == RunningState::Running {
            if currently_stopped {
                self.logger
                    .info("Data generators were stopped, signaling them to resume.");
                self.data_generator_stop_signal
                    .store(false, Ordering::SeqCst);
                if self.data_generator_handles.is_empty() && !self.config.start_paused {
                    self.logger.info("No active data generator handles found while trying to resume. Spawning new generators.");
                    self.spawn_data_generators();
                }
            } else if self.data_generator_handles.is_empty() && !self.config.start_paused {
                self.logger.info("No active data generator handles found and not explicitly stopped. Spawning new generators.");
                self.spawn_data_generators();
            }
        } else if !currently_stopped {
            self.logger.info(
                "Application state changed (Paused/Stopping), signaling data generators to stop.",
            );
            self.data_generator_stop_signal
                .store(true, Ordering::SeqCst);
        }
    }

    pub async fn print_final_stats(&mut self) {
        let stats_guard = self.stats.lock().await;
        let total = stats_guard.total.load(Ordering::Relaxed);
        let success = stats_guard.success.load(Ordering::Relaxed);
        let failure = stats_guard.failure.load(Ordering::Relaxed);
        let success_rate = if total > 0 {
            (success as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        let mut output = String::new();
        output.push_str("\nAttack Statistics Report:\n");
        output.push_str("----------------------\n");
        output.push_str(&format!("Total Requests: {}\n", total));
        output.push_str(&format!("Successful: {}\n", success));
        output.push_str(&format!("Failed: {}\n", failure));
        output.push_str(&format!("Success Rate: {:.2}%\n", success_rate));

        output.push_str("\nDetailed Target Statistics:\n");
        output.push_str("-------------------------\n");
        for target in &stats_guard.targets {
            let target_success_rate = if target.success + target.failure > 0 {
                (target.success as f64 / (target.success + target.failure) as f64) * 100.0
            } else {
                0.0
            };
            output.push_str(&format!("Target [{}]:\n", target.url));
            output.push_str(&format!("  Successful: {}\n", target.success));
            output.push_str(&format!("  Failed: {}\n", target.failure));
            output.push_str(&format!("  Success Rate: {:.2}%\n", target_success_rate));
            if let Some(err) = &target.last_network_error {
                output.push_str(&format!("  Last Error: {}\n", err));
            }
            output.push('\n');
        }

        println!("{}", output);
    }
}

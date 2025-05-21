use crate::logger::Logger;
use crate::ui::{DebugInfo, Stats, ThreadStats}; // Assuming Stats and related structs are accessible
use crate::worker::TargetUpdate;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const MAX_CONSOLE_LOGS: usize = 250;
const HISTORY_CAPACITY: usize = 120; // For RPS and success rate history

pub struct StatsUpdater {
    last_stats_update_time: Instant,
    stats_update_interval: Duration,
    requests_in_last_second: u64,
    successes_in_last_second: u64,
    sysinfo_tick: u32,
}

impl StatsUpdater {
    pub fn new() -> Self {
        StatsUpdater {
            last_stats_update_time: Instant::now(),
            stats_update_interval: Duration::from_secs(1),
            requests_in_last_second: 0,
            successes_in_last_second: 0,
            sysinfo_tick: 0,
        }
    }

    // Returns true if stats were updated and might need a redraw
    pub fn update_stats(
        &mut self,
        stats: &mut Stats,
        target_stats_rx: &mut mpsc::Receiver<TargetUpdate>,
        logger: &Logger,
    ) -> bool {
        let mut needs_redraw_due_to_stats = false;

        // Update system info less frequently
        self.sysinfo_tick = self.sysinfo_tick.wrapping_add(1);
        if self.sysinfo_tick % 20 == 0 {
            // e.g., every 1 second if poll is 50ms (adjust as needed)
            stats.sys.refresh_all();
            stats.cpu_usage = stats
                .sys
                .cpus()
                .iter()
                .map(|cpu| cpu.cpu_usage())
                .sum::<f32>()
                / stats.sys.cpus().len() as f32;
            stats.memory_usage = stats.sys.used_memory();
            needs_redraw_due_to_stats = true;
        }

        while let Ok(update) = target_stats_rx.try_recv() {
            needs_redraw_due_to_stats = true; // Any stats update likely needs redraw

            // Always process debug message if present
            if let Some(debug_msg) = update.debug {
                stats.debug_logs.push_back(DebugInfo {
                    timestamp: update.timestamp,
                    message: debug_msg,
                });
                if stats.debug_logs.len() > MAX_CONSOLE_LOGS {
                    stats.debug_logs.pop_front();
                }
            }

            if update.url.is_empty() {
                // This signifies a PURE debug log message, no other stats
                continue; // Skip further processing for pure debug messages
            }

            stats.total += 1;
            self.requests_in_last_second += 1;
            if update.success {
                stats.success += 1;
                self.successes_in_last_second += 1;
                stats.last_success_time = Some(update.timestamp);
            } else {
                stats.failure += 1;
                stats.last_failure_time = Some(update.timestamp);
            }

            // Update per-target stats
            if let Some(target_stat) = stats.targets.iter_mut().find(|t| t.id == update.id) {
                if update.success {
                    target_stat.success += 1;
                    target_stat.last_success_time = Some(update.timestamp);
                } else {
                    target_stat.failure += 1;
                    target_stat.last_failure_time = Some(update.timestamp);
                }
                if let Some(network_err) = update.network_error {
                    target_stat.last_network_error = Some(network_err);
                } else if !update.success {
                    // Clear error if it was a non-network failure (e.g. HTTP 4xx/5xx)
                    target_stat.last_network_error = None;
                }

                let total = target_stat.success + target_stat.failure;
                if total > 0 {
                    target_stat.error_rate = target_stat.failure as f64 / total as f64;
                } else {
                    target_stat.error_rate = 0.0;
                }
                // 濒死判定：失败数大于20且成功率低于10%
                if total > 20 && (target_stat.success as f64 / total as f64) < 0.1 {
                    target_stat.is_dying = true;
                } else {
                    target_stat.is_dying = false;
                }
            } else {
                logger.warning(&format!(
                    "StatsUpdater: Received update for unknown target ID: {} for URL: {}",
                    update.id, update.url
                ));
            }

            // Update per-thread stats
            let now = update.timestamp;
            match stats
                .threads
                .iter_mut()
                .find(|ts| ts.id == update.thread_id)
            {
                Some(thread_stat) => {
                    thread_stat.requests += 1;
                    thread_stat.last_active = now;
                }
                None => {
                    // logger.info(&format!( // This can be very verbose
                    //     "StatsUpdater: First update received from new thread ID: {:?}",
                    //     update.thread_id
                    // ));
                    stats.threads.push(ThreadStats {
                        id: update.thread_id,
                        requests: 1,
                        last_active: now,
                    });
                }
            }
        }

        if self.last_stats_update_time.elapsed() >= self.stats_update_interval {
            stats.rps_history.push_back(self.requests_in_last_second);
            if stats.rps_history.len() > HISTORY_CAPACITY {
                stats.rps_history.pop_front();
            }

            stats
                .successful_requests_per_second_history
                .push_back(self.successes_in_last_second);
            if stats.successful_requests_per_second_history.len() > HISTORY_CAPACITY {
                stats.successful_requests_per_second_history.pop_front();
            }

            let current_success_rate = if self.requests_in_last_second > 0 {
                (self.successes_in_last_second * 100 / self.requests_in_last_second).min(100)
            } else if stats.total > 0 {
                // If no requests in last sec, use overall
                (stats.success * 100 / stats.total).min(100)
            } else {
                100 // Default to 100 if no requests at all
            };
            stats.success_rate_history.push_back(current_success_rate);
            if stats.success_rate_history.len() > HISTORY_CAPACITY {
                stats.success_rate_history.pop_front();
            }

            self.requests_in_last_second = 0;
            self.successes_in_last_second = 0;
            self.last_stats_update_time = Instant::now();
            needs_redraw_due_to_stats = true;
        }
        needs_redraw_due_to_stats
    }
}

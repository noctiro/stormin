use super::App;
use crate::data_generator;
use crate::types::{RunningState, TargetUpdate, WorkerMessage};
use crate::worker::worker_loop;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};
use tokio::time::timeout;

impl App {
    pub fn spawn_data_generators(&mut self) {
        let generator_task_count = self.config.generator_threads;
        self.logger.info(&format!(
            "Spawning {} data generator tasks...",
            generator_task_count
        ));

        let pool_size = self.config.threads * 50;
        let (data_pool_tx, data_pool_rx) = mpsc::channel(pool_size);
        self.data_pool_tx = Some(data_pool_tx);
        self.data_pool_rx = Some(Arc::new(Mutex::new(data_pool_rx)));

        self.data_generator_stop_signal
            .store(false, Ordering::SeqCst);

        let mut target_chunks: Vec<Vec<usize>> = vec![Vec::new(); generator_task_count];
        for (i, t) in self.config.targets.iter().enumerate() {
            target_chunks[i % generator_task_count].push(t.id);
        }

        for i in 0..generator_task_count {
            let cfg = self.config.clone();
            let pool_tx_clone = self
                .data_pool_tx
                .as_ref()
                .expect("Data pool sender should be initialized")
                .clone();
            let logger_clone = self.logger.clone();
            let stop_signal_clone = self.data_generator_stop_signal.clone();
            let my_target_ids = target_chunks[i].clone();
            let stats_arc = self.stats_arc();

            let handle = tokio::spawn(async move {
                data_generator::data_generator_loop(
                    i,
                    cfg,
                    my_target_ids,
                    pool_tx_clone,
                    logger_clone,
                    stop_signal_clone,
                    stats_arc,
                )
                .await;
            });
            self.data_generator_handles.push(handle);
        }
    }

    pub fn spawn_workers(&mut self) {
        self.logger
            .info(&format!("Spawning {} worker tasks...", self.config.threads));
        for _i in 0..self.config.threads {
            let control_rx = self.control_tx.subscribe();
            let data_pool_rx_clone = self
                .data_pool_rx
                .clone()
                .expect("Data pool receiver should be initialized before spawning workers");
            let cfg = self.config.clone();
            let worker_logger = self.logger.clone();
            let stats_tx = self.target_stats_tx.clone();
            let handle = tokio::spawn(async move {
                worker_loop(
                    control_rx,
                    data_pool_rx_clone,
                    cfg,
                    std::thread::current().id(),
                    worker_logger.clone(),
                    stats_tx,
                )
                .await;
            });
            self.worker_handles.push(handle);
        }
    }

    pub fn spawn_log_receiver(&mut self) {
        if self.cli_mode {
            return;
        }

        let log_rx = match self.log_rx.take() {
            Some(rx) => rx,
            None => {
                self.logger
                    .error("Log receiver channel not initialized for TUI");
                return;
            }
        };

        let debug_logs_tx = self.target_stats_tx.clone();
        let logger_clone = self.logger.clone();

        self.logger.info("Starting log receiver thread for TUI...");

        let handle = thread::spawn(move || {
            logger_clone.info("Log receiver thread started");
            for log_entry in log_rx {
                let update = TargetUpdate {
                    id: 0,
                    url: String::new(),
                    success: false,
                    timestamp: log_entry.timestamp,
                    debug: Some(log_entry.message),
                    network_error: None,
                    thread_id: std::thread::current().id(),
                };
                if debug_logs_tx.blocking_send(update).is_err() {
                    break;
                }
            }
            logger_clone.info("Log receiver thread finished");
        });
        self.log_receiver_handle = Some(handle);
    }

    pub(super) async fn shutdown_components(&mut self) {
        self.logger.info("Shutdown initiated...");
        self.stats.lock().await.running_state = RunningState::Stopping;
        let rx = std::mem::replace(&mut self.target_stats_rx, mpsc::channel(1).1);
        drop(rx);

        self.data_generator_stop_signal
            .store(true, Ordering::SeqCst);
        self.data_pool_tx.take();

        self.logger.info("Sending stop signal to all workers...");
        let _ = self.control_tx.send(WorkerMessage::Stop);

        self.logger
            .info("Waiting for data generators to stop (max 1s)...");
        let gen_handles = std::mem::take(&mut self.data_generator_handles);
        let _ = timeout(
            Duration::from_secs(1),
            futures::future::join_all(gen_handles),
        )
        .await;

        self.logger.info("Waiting for workers to stop (max 1s)...");
        let worker_handles = std::mem::take(&mut self.worker_handles);
        let _ = timeout(
            Duration::from_secs(1),
            futures::future::join_all(worker_handles),
        )
        .await;

        self.logger.info("Fast shutdown completed.");

        if !self.cli_mode {
            self.logger
                .info("Closing logger's TUI sender to allow log_receiver to stop...");
            self.logger.close_sender();

            if let Some(handle) = self.log_receiver_handle.take() {
                if let Err(e) = handle.join() {
                    self.logger
                        .error(&format!("Log receiver thread panicked: {:?}", e));
                }
            }

            self.restore_terminal();
        }
        self.logger.info("All components shut down.");
    }
}

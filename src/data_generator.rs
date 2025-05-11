use crate::config::loader;
use crate::logger::Logger;
use crate::template::render_ast_node;
use crate::worker::PreGeneratedRequest;

use rand::SeedableRng;
use rand::prelude::IndexedRandom;
use rand::rngs::StdRng;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::broadcast;
use tokio::{task::JoinHandle, time::sleep};

pub struct DataGenerator {
    config: loader::AttackConfig,
    data_tx: broadcast::Sender<PreGeneratedRequest>,
    logger: Logger,
    running_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl DataGenerator {
    pub fn new(
        config: loader::AttackConfig,
        data_tx: broadcast::Sender<PreGeneratedRequest>,
        logger: Logger,
    ) -> Self {
        DataGenerator {
            config,
            data_tx,
            logger,
            running_flag: Arc::new(AtomicBool::new(false)), // Start as not running
            handle: None,
        }
    }

    pub fn spawn(&mut self) {
        if self.handle.as_ref().map_or(false, |h| !h.is_finished()) {
            self.logger
                .info("Data generator task is already running or not yet joined.");
            return;
        }

        self.logger.info("Spawning data generator task...");
        let cfg = self.config.clone();
        let data_tx_clone = self.data_tx.clone();
        let logger_clone = self.logger.clone();
        let running_flag_clone = self.running_flag.clone();

        // Ensure the flag is set to true before spawning
        running_flag_clone.store(true, Ordering::SeqCst);

        let task_handle = tokio::spawn(async move {
            // Pass running_flag_clone by value as it's an Arc
            data_generator_loop(cfg, data_tx_clone, logger_clone, running_flag_clone).await;
        });
        self.handle = Some(task_handle);
    }

    pub async fn stop(&mut self) {
        self.logger.info("Attempting to stop data generator...");
        self.running_flag.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            self.logger
                .info("Waiting for data generator task to finish...");
            if let Err(e) = handle.await {
                self.logger
                    .error(&format!("Data generator task panicked: {:?}", e));
            } else {
                self.logger.info("Data generator task finished.");
            }
        } else {
            self.logger
                .info("Data generator task was not running or already stopped.");
        }
    }

    pub fn is_running(&self) -> bool {
        self.running_flag.load(Ordering::SeqCst)
    }

    pub fn set_running_flag(&self, running: bool) {
        self.running_flag.store(running, Ordering::SeqCst);
    }

    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().map_or(true, |h| h.is_finished())
    }
}

async fn data_generator_loop(
    config: loader::AttackConfig,
    data_tx: broadcast::Sender<PreGeneratedRequest>,
    logger: Logger,
    running_flag: Arc<AtomicBool>,
) {
    logger.info("Data generator loop started.");
    let mut rng = StdRng::from_os_rng();
    // context_map will now be created per target_config
    let pre_generate_target_count = 2500; // Increased pre-generation count
    let mut generated_count = 0;

    // Adaptive rate limiting parameters (original, adjusted for better default performance)
    const MIN_DELAY_MICROS: u64 = 200;    // Minimum delay: 0.2 milliseconds
    const MAX_DELAY_MICROS: u64 = 50_000;   // Maximum delay: 50 milliseconds
    const INITIAL_DELAY_MICROS: u64 = 1000; // Initial delay for main loop: 1 millisecond
    
    // Factors for when target_rps is NOT set (using f64 for more granular adjustments)
    const DEFAULT_INCREASE_FACTOR_NO_RPS: f64 = 1.5;
    const DEFAULT_DECREASE_FACTOR_NO_RPS: f64 = 0.66; // Multiplicative factor for decrease

    // New dynamic rate calculator constants (remain the same)
    const SUCCESS_RATE_WINDOW_SIZE: usize = 100;
    const DEFAULT_RPS_ADJUST_FACTOR: f64 = 0.2;
    const DEFAULT_SUCCESS_RATE_PENALTY_FACTOR: f64 = 1.5;

    // Initial delay for pre-generation phase, can be different from the main loop's initial delay
    let mut current_pre_gen_delay_micros = 500; // Start pre-generation with a 0.5ms delay

    // Pre-generate initial batch (uses simpler delay logic)
    while running_flag.load(Ordering::SeqCst) && generated_count < pre_generate_target_count {
        if data_tx.receiver_count() == 0 {
            sleep(Duration::from_millis(100)).await;
            continue;
        }

        let target_config = match config.targets.as_slice().choose(&mut rng) {
            Some(t) => t,
            None => {
                logger.error("Data Generator: No targets configured. Stopping pre-generation.");
                // Do not set running_flag to false here, let the outer control manage it.
                // The loop condition `running_flag.load` will handle termination.
                break;
            }
        };

        // Create a new context map for each target to ensure variable independence
        let mut target_context_map = HashMap::new();

        let mut rendered_headers = Vec::with_capacity(target_config.headers.len());
        for (key, template_node) in &target_config.headers {
            // Do NOT clear context_map here; it's shared for this target_config
            match render_ast_node(template_node, &mut target_context_map, logger.clone()) {
                Ok(value_string) => rendered_headers.push((key.clone(), value_string)),
                Err(e) => logger.warning(&format!(
                    "Data Generator: Failed to render header '{}' for target '{}': {}",
                    key, target_config.url, e
                )),
            }
        }

        let mut rendered_params = Vec::with_capacity(target_config.params.len());
        for (key, template_node) in &target_config.params {
            // Do NOT clear context_map here; it's shared for this target_config
            match render_ast_node(template_node, &mut target_context_map, logger.clone()) {
                Ok(value_string) => rendered_params.push((key.clone(), value_string)),
                Err(e) => logger.warning(&format!(
                    "Data Generator: Failed to render param '{}' for target '{}': {}",
                    key, target_config.url, e
                )),
            }
        }

        let pre_gen_req = PreGeneratedRequest {
            target_id: target_config.id,
            target_url: target_config.url.clone(),
            method: target_config.method.clone(),
            rendered_headers,
            rendered_params,
        };

        // During pre-generation, we might not need aggressive rate limiting,
        // or use a simpler fixed delay if send fails.
        match data_tx.send(pre_gen_req) {
            Ok(_) => {
                // Optionally, slightly decrease delay if pre-generation is very fast and successful
                // current_delay_micros = (current_delay_micros / DECREASE_DIVISOR).max(MIN_DELAY_MICROS);
            }
            Err(_) => {
                logger.warning("Data Generator: Failed to send pre-generated request during pre-generation. Channel likely closed/full. Increasing delay.");
                // Increase delay during pre-generation if send fails
                current_pre_gen_delay_micros = (current_pre_gen_delay_micros as f64 * DEFAULT_INCREASE_FACTOR_NO_RPS)
                                                .min(MAX_DELAY_MICROS as f64) as u64;
                current_pre_gen_delay_micros = current_pre_gen_delay_micros.max(MIN_DELAY_MICROS); // Ensure it doesn't go below min
                sleep(Duration::from_micros(current_pre_gen_delay_micros)).await;
                continue; // Retry sending after delay
            }
        }
        generated_count += 1;
        // Decrease delay slightly on successful pre-generation send, or use a very small fixed sleep
        current_pre_gen_delay_micros = (current_pre_gen_delay_micros as f64 * DEFAULT_DECREASE_FACTOR_NO_RPS)
                                            .max(100.0) as u64; // Min 0.1ms for pre-gen success
        sleep(Duration::from_micros(current_pre_gen_delay_micros.max(100))).await; // Ensure at least 0.1ms sleep
    }
    
    let mut current_delay_micros = INITIAL_DELAY_MICROS; // Initialize for the main loop

    logger.info(&format!(
        "Data generator pre-generated {} items. Initial delay for continuous generation set to: {} µs",
        generated_count, current_delay_micros
    ));
    
    // Initialize dynamic rate parameters for the main loop
    // These are assumed to be Option<f64> in config. If not, this will need adjustment.
    let target_rps_opt = config.target_rps;
    let min_success_rate_opt = config.min_success_rate;
    let rps_adjust_factor = config.rps_adjust_factor.unwrap_or(DEFAULT_RPS_ADJUST_FACTOR);
    let success_rate_penalty_factor = config.success_rate_penalty_factor.unwrap_or(DEFAULT_SUCCESS_RATE_PENALTY_FACTOR);

    let mut target_delay_micros_opt: Option<u64> = target_rps_opt.and_then(|rps| {
        if rps > 0.0 {
            Some(((1.0 / rps) * 1_000_000.0) as u64)
        } else {
            None
        }
    });

    if let Some(ref mut t_delay) = target_delay_micros_opt {
        *t_delay = (*t_delay).max(MIN_DELAY_MICROS).min(MAX_DELAY_MICROS); // Clamp target delay
        logger.info(&format!(
            "Data Generator: Target RPS configured: {:.2}, Target delay: {} µs",
            target_rps_opt.unwrap_or(0.0), *t_delay
        ));
    } else {
        logger.info("Data Generator: No target RPS configured. Using basic adaptive delay.");
    }

    if let Some(rate) = min_success_rate_opt {
        logger.info(&format!(
            "Data Generator: Minimum success rate configured: {:.2}% (Penalty Factor: {})",
            rate * 100.0, success_rate_penalty_factor
        ));
    }
    
    let mut send_success_history: std::collections::VecDeque<bool> = std::collections::VecDeque::with_capacity(SUCCESS_RATE_WINDOW_SIZE);

    // Continue generating if workers are consuming with adaptive rate limiting
    while running_flag.load(Ordering::SeqCst) {
        if data_tx.receiver_count() == 0 {
            logger.info(
                "Data Generator: No active workers. Pausing generation until workers appear.",
            );
            // Use DEFAULT_INCREASE_FACTOR_NO_RPS and ensure proper casting and clamping
            current_delay_micros = ((current_delay_micros as f64 * DEFAULT_INCREASE_FACTOR_NO_RPS) as u64)
                                        .min(MAX_DELAY_MICROS)
                                        .max(MIN_DELAY_MICROS); // Ensure it's clamped
            sleep(Duration::from_micros(current_delay_micros.max(500_000))).await; // Wait at least 500ms
            continue;
        }

        let target_config = match config.targets.as_slice().choose(&mut rng) {
            Some(t) => t,
            None => {
                logger.error(
                    "Data Generator: No targets available post pre-generation. Pausing generation.",
                );
                sleep(Duration::from_secs(1)).await; // Wait for config update or shutdown
                continue;
            }
        };

        // Create a new context map for each target to ensure variable independence
        let mut target_context_map = HashMap::new();

        // Simplified rendering logic (same as pre-generation)
        let mut rendered_headers = Vec::with_capacity(target_config.headers.len());
        for (key, template_node) in &target_config.headers {
            // Do NOT clear context_map here; it's shared for this target_config
            match render_ast_node(template_node, &mut target_context_map, logger.clone()) {
                Ok(value_string) => rendered_headers.push((key.clone(), value_string)),
                Err(e) => logger.warning(&format!(
                    "Data Generator: Failed to render header '{}' for target '{}': {}",
                    key, target_config.url, e
                )),
            }
        }

        let mut rendered_params = Vec::with_capacity(target_config.params.len());
        for (key, template_node) in &target_config.params {
            // Do NOT clear context_map here; it's shared for this target_config
            match render_ast_node(template_node, &mut target_context_map, logger.clone()) {
                Ok(value_string) => rendered_params.push((key.clone(), value_string)),
                Err(e) => logger.warning(&format!(
                    "Data Generator: Failed to render param '{}' for target '{}': {}",
                    key, target_config.url, e
                )),
            }
        }

        let pre_gen_req = PreGeneratedRequest {
            target_id: target_config.id,
            target_url: target_config.url.clone(),
            method: target_config.method.clone(),
            rendered_headers,
            rendered_params,
        };

        let send_successful = match data_tx.send(pre_gen_req) {
            Ok(_) => {
                if let Some(target_delay) = target_delay_micros_opt {
                    // Adjust current_delay_micros towards target_delay
                    current_delay_micros = (
                        (current_delay_micros as f64 * (1.0 - rps_adjust_factor)) +
                        (target_delay as f64 * rps_adjust_factor)
                    ) as u64;
                } else {
                    // No target RPS, use adjusted default factors
                    current_delay_micros = (current_delay_micros as f64 * DEFAULT_DECREASE_FACTOR_NO_RPS) as u64;
                }
                current_delay_micros = current_delay_micros.max(MIN_DELAY_MICROS).min(MAX_DELAY_MICROS); // Clamp
                
                true
            }
            Err(_) => {
                let prev_delay = current_delay_micros;
                if target_delay_micros_opt.is_some() {
                    // If target RPS is set, failure might mean we are too fast, so increase delay towards MAX,
                    // but less aggressively than the default no-RPS increase factor.
                    // Or, it could be a sign that target_rps is too ambitious for the current channel capacity.
                    // For now, let's use a slightly moderated increase or rely on success_rate_penalty_factor.
                    // We'll use a simple increase for now, the success_rate_penalty will handle persistent issues.
                     current_delay_micros = (current_delay_micros as f64 * 1.2).min(MAX_DELAY_MICROS as f64) as u64; // Gentle increase
                } else {
                    current_delay_micros = (current_delay_micros as f64 * DEFAULT_INCREASE_FACTOR_NO_RPS) as u64;
                }
                current_delay_micros = current_delay_micros.min(MAX_DELAY_MICROS).max(MIN_DELAY_MICROS); // Clamp

                if current_delay_micros != prev_delay {
                    logger.warning(&format!(
                        "Data Generator: Send failed. Delay increased from {} to {} µs. Receiver count: {}",
                        prev_delay, current_delay_micros, data_tx.receiver_count()
                    ));
                } else { // Delay was already at MAX_DELAY_MICROS or didn't change
                     logger.warning(&format!(
                        "Data Generator: Send failed. Delay remains at {} µs (likely maxed out). Receiver count: {}",
                        current_delay_micros, data_tx.receiver_count()
                    ));
                }
                false
            }
        };

        // Update success history and apply penalty if needed
        if min_success_rate_opt.is_some() {
            if send_success_history.len() == SUCCESS_RATE_WINDOW_SIZE {
                send_success_history.pop_front();
            }
            send_success_history.push_back(send_successful);

            if send_success_history.len() == SUCCESS_RATE_WINDOW_SIZE { // Only check if window is full
                if let Some(min_rate_threshold) = min_success_rate_opt {
                    let successes = send_success_history.iter().filter(|&&s| s).count();
                    let current_success_rate = successes as f64 / SUCCESS_RATE_WINDOW_SIZE as f64;

                    if current_success_rate < min_rate_threshold {
                        let old_delay = current_delay_micros;
                        current_delay_micros = ((current_delay_micros as f64 * success_rate_penalty_factor) as u64)
                                                 .min(MAX_DELAY_MICROS); // Clamp with MAX_DELAY_MICROS
                        
                        if current_delay_micros > old_delay {
                            logger.info(&format!(
                                "Data Generator: Low success rate ({:.2}%) < threshold ({:.2}%). Delay penalized: {} -> {} µs.",
                                current_success_rate * 100.0,
                                min_rate_threshold * 100.0,
                                old_delay,
                                current_delay_micros
                            ));
                        }
                    }
                }
            }
        }
        sleep(Duration::from_micros(current_delay_micros)).await;
    }
    logger.info("Data generator loop finished.");
}

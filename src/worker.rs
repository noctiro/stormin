use crate::config::loader::AttackConfig;
use crate::logger::Logger;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use reqwest::{Client, Method};
use std::thread::ThreadId;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as TokioMutex, broadcast, mpsc};
use tokio::time::sleep;

// Structure for pre-generated request data
#[derive(Debug, Clone)]
pub struct PreGeneratedRequest {
    pub target_id: usize,
    pub target_url: String,
    pub method: Method,
    pub rendered_headers: Vec<(String, String)>,
    pub rendered_params: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
pub enum WorkerMessage {
    Pause,
    Resume,
    Stop,
}

#[derive(Debug)]
pub struct TargetUpdate {
    pub id: usize, // Unique ID of the target
    pub url: String,
    pub success: bool,
    pub timestamp: Instant,
    pub debug: Option<String>,         // Full debug message for logging
    pub network_error: Option<String>, // Specific error for UI display when request fails early 响应前失败
    pub thread_id: ThreadId,           // Add ThreadId
}

pub async fn worker_loop(
    mut control_rx: broadcast::Receiver<WorkerMessage>, // Control channel remains broadcast
    data_pool_rx: Arc<TokioMutex<mpsc::Receiver<PreGeneratedRequest>>>, // Use TokioMutex
    config: AttackConfig,
    thread_id: ThreadId,
    logger: Logger,
    stats_tx: mpsc::Sender<TargetUpdate>, // Corrected type from previous thought
) {
    // buffer for dynamic generation is removed from worker
    let mut rng = StdRng::from_os_rng(); // Initialize Send-compatible rng for proxy selection
    let mut paused = false;
    let loop_sleep_duration = Duration::from_millis(10);
    let proxy_config = if !config.proxies.is_empty() {
        config.proxies.choose(&mut rng).cloned()
    } else {
        None
    };

    let client_builder = Client::builder()
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .timeout(Duration::from_secs(config.timeout));

    let client = match proxy_config {
        Some(proxy) => {
            let proxy_url = if let (Some(user), Some(pass)) = (&proxy.username, &proxy.password) {
                format!(
                    "{}://{}:{}@{}:{}",
                    proxy.scheme, user, pass, proxy.host, proxy.port
                )
            } else {
                format!("{}://{}:{}", proxy.scheme, proxy.host, proxy.port)
            };
            match reqwest::Proxy::all(&proxy_url) {
                Ok(reqwest_proxy) => client_builder.proxy(reqwest_proxy).build(),
                Err(e) => {
                    logger.error(&format!(
                        "Worker {:?}: Failed to create proxy object from {}, falling back: {}",
                        thread_id, proxy.raw, e
                    ));
                    Client::builder()
                        .pool_max_idle_per_host(10)
                        .tcp_keepalive(Some(Duration::from_secs(30)))
                        .build()
                }
            }
        }
        None => client_builder.build(),
    }
    .unwrap_or_else(|e| {
        logger.error(&format!(
            "Worker {:?}: Failed to build client, falling back to default: {}",
            thread_id, e
        ));
        Client::new()
    });

    // This is the correct start of the main loop.
    // The duplicated block above this line in the original file will be removed.
    'main_loop: loop {
        while paused {
            tokio::select! {
                biased;
                control_msg_result = control_rx.recv() => {
                    match control_msg_result {
                        Ok(WorkerMessage::Resume) => {
                            logger.info(&format!("Worker {:?} resuming...", thread_id));
                            paused = false;
                        }
                        Ok(WorkerMessage::Stop) => {
                            logger.info(&format!("Worker {:?} stopping while paused...", thread_id));
                            return;
                        }
                        Ok(_) => {} // Ignore Task while paused
                        Err(broadcast::error::RecvError::Closed) => {
                            logger.warning(&format!("Worker {:?}: Control channel closed while paused. Exiting.", thread_id));
                            return;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            logger.warning(&format!("Worker {:?}: Control channel lagged by {} messages while paused.", thread_id, n));
                        }
                    }
                }
            }
            if paused {
                // Re-check in case Resume was processed before sleeping
                sleep(loop_sleep_duration).await;
            }
        }

        // Main operational loop: select between control messages and data
        tokio::select! {
            biased; // Prioritize control messages

            control_msg_result = control_rx.recv() => {
                match control_msg_result {
                    Ok(WorkerMessage::Stop) => {
                        logger.info(&format!("Worker {:?} received Stop signal.", thread_id));
                        break 'main_loop;
                    }
                    Ok(WorkerMessage::Pause) => {
                        logger.info(&format!("Worker {:?} pausing...", thread_id));
                        paused = true;
                        continue 'main_loop; // Re-evaluate 'while paused'
                    }
                    Ok(WorkerMessage::Resume) => {
                        // Already not paused if we are here, or handled by 'while paused'
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        logger.warning(&format!("Worker {:?}: Control channel closed. Exiting.", thread_id));
                        break 'main_loop;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        logger.warning(&format!("Worker {:?}: Control channel lagged by {} messages.", thread_id, n));
                    }
                }
            }, // Comma separates select arms

            // Receive from the shared mpsc channel, requires locking the mutex
            data_msg_result = async {
                let mut rx_guard = data_pool_rx.lock().await; // Lock the TokioMutex asynchronously
                rx_guard.recv().await // Receive from the mpsc channel
            } => {
                 match data_msg_result {
                    Some(pre_gen_req) => { // mpsc::Receiver::recv returns Option<T>
                         let PreGeneratedRequest {
                             target_id,
                            target_url,
                            method,
                            rendered_headers,
                            rendered_params,
                        } = pre_gen_req;

                        let mut req_builder = client.request(method.clone(), &target_url);

                        // Apply rendered headers
                        for (key, value_string) in &rendered_headers {
                            match reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                                Ok(header_name) => {
                                    match reqwest::header::HeaderValue::from_str(value_string) {
                                        Ok(header_value) => {
                                            req_builder = req_builder.header(header_name, header_value);
                                        }
                                        Err(e) => logger.warning(&format!("Worker {:?}: Invalid pre-rendered header value for '{}' in target '{}': {} (Value: '{}'). Skipping header.", thread_id, key, target_url, e, value_string)),
                                    }
                                }
                                Err(e) => logger.warning(&format!("Worker {:?}: Invalid pre-rendered header name '{}' in target '{}': {}. Skipping header.", thread_id, key, target_url, e)),
                            }
                        }

                        // Apply rendered params
                        match method {
                            Method::GET | Method::DELETE | Method::OPTIONS => {
                                req_builder = req_builder.query(&rendered_params);
                            }
                            Method::POST | Method::PUT | Method::PATCH => {
                                req_builder = req_builder.form(&rendered_params);
                            }
                            _ => {
                                logger.warning(&format!(
                                    "Worker {:?}: Unsupported method {} for params/body with pre-generated data for target '{}'",
                                    thread_id, method, target_url
                                ));
                                // Skip this request if method is not supported for params
                                continue 'main_loop;
                            }
                        }

                        let start_time = Instant::now();
                        let res = req_builder.send().await;
                        let timestamp = Instant::now();
                        let duration = timestamp.duration_since(start_time);

                        let (success, status_code, error_details) = match res {
                            Ok(response) => {
                                let success_status = response.status().is_success();
                                let status = response.status();
                                (success_status, Some(status), None)
                            }
                            Err(e) => (false, None, Some(e.to_string())),
                        };

                        let attack_message = format!(
                            "[Request]\nURL: {}\nMethod: {}\nDuration: {:?}\nStatus: {}{}{}{}",
                            target_url,
                            method,
                            duration,
                            status_code.map_or_else(|| "N/A".to_string(), |s| s.to_string()),
                            if !rendered_headers.is_empty() {
                                format!(
                                    "\nHeaders:\n{}",
                                    rendered_headers
                                        .iter()
                                        .map(|(k, v)| format!("  {}: {}", k, v))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                )
                            } else { String::new() },
                            if !rendered_params.is_empty() {
                                format!(
                                    "\nParams: {}",
                                    rendered_params
                                        .iter()
                                        .map(|(k, v)| format!("{}={}", k, v))
                                        .collect::<Vec<_>>()
                                        .join("&")
                                )
                            } else { String::new() },
                            error_details.as_ref().map(|err| format!("\nError: {}", err)).unwrap_or_default()
                        );

                        let update = TargetUpdate {
                            id: target_id,
                            url: target_url, // target_url is already a String
                            success,
                            timestamp,
                            debug: Some(attack_message),
                            network_error: error_details,
                            thread_id,
                        };
                        if stats_tx.send(update).await.is_err() {
                            logger.warning(&format!("Worker {:?}: Failed to send stats update for target {}. UI channel likely closed.", thread_id, target_id));
                            // If UI channel is closed, worker might as well stop.
                            break 'main_loop;
                        }
                    }
                    None => { // Channel closed
                        logger.info(&format!("Worker {:?}: Data pool channel closed. Exiting.", thread_id));
                        break 'main_loop;
                    }
                    // Note: mpsc::Receiver doesn't have a Lagged error like broadcast::Receiver
                }
            }
        }
    } // This is the correct closing brace for the outer 'main_loop
    logger.info(&format!("Worker thread {:?} finished", thread_id));
}

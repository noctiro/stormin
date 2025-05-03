use crate::config::AttackConfig;
use crate::logger::Logger;
use crate::template::render_ast_node;
use crate::ui::ThreadStats;
use rand::rngs::{SmallRng, ThreadRng};
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use reqwest::{Client, Method};
use std::thread::ThreadId;
use std::{
    mem, // Import mem
    time::{Duration, Instant},
};
// Use broadcast receiver for control messages
use tokio::sync::{broadcast, mpsc::Sender};
use tokio::time::sleep;

#[derive(Debug, Clone, Copy)]
pub enum RequestResult {
    Success,
    Failure,
}

// WorkerMessage remains the same, but will be sent over broadcast channel
#[derive(Debug, Clone, Copy)]
pub enum WorkerMessage {
    Task,
    Pause,
    Resume,
    Stop,
}

// TargetUpdate remains the same
#[derive(Debug)]
pub struct TargetUpdate {
    pub url: String,
    pub success: bool,
    pub timestamp: Instant,
    pub debug: Option<String>,
}

// RequestBuffer remains the same
struct RequestBuffer {
    params_vec: Vec<(String, String)>,
    debug_info: String,
}

impl RequestBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            params_vec: Vec::with_capacity(capacity),
            debug_info: String::with_capacity(1024),
        }
    }

    fn clear(&mut self) {
        self.params_vec.clear();
        self.debug_info.clear();
    }
}

pub async fn worker_loop(
    // Change receiver type to broadcast::Receiver
    mut rx: broadcast::Receiver<WorkerMessage>,
    config: AttackConfig,
    // stat_tx remains mpsc::Sender
    stat_tx: Sender<RequestResult>,
    thread_id: ThreadId,
    thread_stats_tx: Sender<ThreadStats>,
    target_stats_tx: Sender<TargetUpdate>,
    logger: Logger,
) {
    let mut buffer = RequestBuffer::new(16);

    let mut rng = SmallRng::seed_from_u64(ThreadRng::default().random());
    let mut paused = false;
    let loop_sleep_duration = Duration::from_millis(10);
    let proxy_config = config.proxies.choose(&mut rng).cloned();

    let client_builder = Client::builder()
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .timeout(Duration::from_secs(config.timeout));

    let stats_update_interval = Duration::from_millis(500);
    let mut last_stats_update = Instant::now();
    let mut requests = 0u64;

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

    let method_get = Method::GET;
    let method_post = Method::POST;
    let method_put = Method::PUT;
    let method_delete = Method::DELETE;
    let method_patch = Method::PATCH;

    'main_loop: loop {
        while paused {
            // Use broadcast receiver's recv
            match rx.recv().await {
                Ok(WorkerMessage::Resume) => {
                    logger.info(&format!("Worker {:?} resuming...", thread_id));
                    paused = false;
                }
                Ok(WorkerMessage::Stop) => {
                    logger.info(&format!("Worker {:?} stopping while paused...", thread_id));
                    return;
                }
                Ok(_) => { /* Ignore other messages like Task while paused */ }
                // Handle channel closed
                Err(broadcast::error::RecvError::Closed) => {
                    logger.warning(&format!(
                        "Worker {:?} channel disconnected while paused, exiting.",
                        thread_id
                    ));
                    return;
                }
                // Handle lagged receiver (missed messages)
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    logger.warning(&format!(
                        "Worker {:?} lagged by {} messages while paused.",
                        thread_id, n
                    ));
                    // Decide how to handle lag, e.g., continue or exit
                    // For now, just log and continue waiting for Resume/Stop
                }
            }
            // Add a small sleep to prevent busy-waiting in the paused loop
            sleep(loop_sleep_duration).await;
        }

        // Use broadcast receiver's recv
        match rx.recv().await {
            Ok(msg) => {
                match msg {
                    WorkerMessage::Stop => {
                        logger.info(&format!("Worker {:?} received Stop signal.", thread_id));
                        break 'main_loop;
                    }
                    WorkerMessage::Pause => {
                        logger.info(&format!("Worker {:?} pausing...", thread_id));
                        paused = true;
                        continue; // Go back to the start of 'main_loop to enter the paused state
                    }
                    WorkerMessage::Task => {
                        requests += 1;
                        let target = match config.targets.choose(&mut rng) {
                            Some(t) => t,
                            None => {
                                logger.error(&format!(
                                    "Worker {:?}: No targets available, skipping task.",
                                    thread_id
                                ));
                                continue;
                            }
                        };

                        let method_str = target.method.to_uppercase(); // Do uppercase once
                        let method = match method_str.as_str() {
                            // Match on &str
                            "GET" => &method_get,
                            "POST" => &method_post,
                            "PUT" => &method_put,
                            "DELETE" => &method_delete,
                            "PATCH" => &method_patch,
                            _ => {
                                logger.error(&format!("Worker {:?}: Invalid HTTP method '{}' for target {}, skipping.",
                                    thread_id, target.method, target.url));
                                continue;
                            }
                        };

                        let mut req = client.request(method.clone(), &target.url);
                        for (key, value) in &target.headers {
                            req = req.header(key, value);
                        }

                        buffer.clear(); // Clears params_vec and debug_info
                        for (key, template) in &target.params {
                            let value_string = render_ast_node(template, logger.clone());
                            buffer.params_vec.push((key.clone(), value_string));
                        }

                        match *method {
                            Method::GET | Method::DELETE => {
                                req = req.query(&buffer.params_vec);
                            }
                            Method::POST | Method::PUT | Method::PATCH => {
                                req = req.form(&buffer.params_vec);
                            }
                            _ => {
                                // This case should technically not be reachable due to the match above
                                logger.warning(&format!(
                                    "Worker {:?}: Unsupported method {} for params/body, sending without.",
                                    thread_id, target.method));
                            }
                        }

                        let request_start = Instant::now();
                        let res = req.send().await;
                        let request_duration = request_start.elapsed();

                        let success = res
                            .as_ref()
                            .map(|r| r.status().is_success())
                            .unwrap_or(false);

                        // Build combined debug info string directly into buffer.debug_info
                        // buffer.debug_info is already cleared by buffer.clear()
                        buffer.debug_info.push_str("[Request]\nURL: ");
                        buffer.debug_info.push_str(&target.url);
                        buffer.debug_info.push_str("\nMethod: ");
                        buffer.debug_info.push_str(&target.method); // Use original case method for logging
                        buffer.debug_info.push_str("\nParams: ");
                        for (i, (key, value)) in buffer.params_vec.iter().enumerate() {
                            if i > 0 {
                                buffer.debug_info.push_str("&");
                            }
                            buffer.debug_info.push_str(key);
                            buffer.debug_info.push_str("=");
                            buffer.debug_info.push_str(value);
                        }
                        buffer.debug_info.push_str("\n"); // Separator

                        // Append response info
                        match &res {
                            Ok(r) => {
                                let status = r.status();
                                // Use format_args! with write! macro for potentially better performance if needed,
                                // but format! into a temporary string is usually fine here.
                                let response_str = format!(
                                    "[Response]\nStatus: {} {}\nTime: {:.2}ms",
                                    status.as_u16(),
                                    status.canonical_reason().unwrap_or("Unknown"),
                                    request_duration.as_secs_f64() * 1000.0
                                );
                                buffer.debug_info.push_str(&response_str);
                            }
                            Err(e) => {
                                // Similar optimization potential for formatting if needed
                                let error_str = format!("[Error]\n{}", e);
                                buffer.debug_info.push_str(&error_str);
                            }
                        }

                        // Send ONE TargetUpdate with final result and combined debug info
                        // Use mem::take to avoid cloning the debug_info string
                        let _ = target_stats_tx.try_send(TargetUpdate {
                            url: target.url.clone(), // URL clone is likely still necessary
                            success,
                            timestamp: Instant::now(), // Use timestamp after request completion
                            // Take ownership of the string from the buffer
                            debug: Some(mem::take(&mut buffer.debug_info)),
                        });
                        // buffer.debug_info is now empty

                        // Send overall success/failure via stat_tx (remains the same)
                        let _ = stat_tx.try_send(if success {
                            // Use try_send
                            RequestResult::Success
                        } else {
                            RequestResult::Failure
                        });

                        let now = Instant::now();
                        if now.duration_since(last_stats_update) >= stats_update_interval {
                            let _ = thread_stats_tx.try_send(ThreadStats {
                                // Use try_send
                                id: thread_id,
                                requests,
                                last_active: now,
                            });
                            last_stats_update = now;
                        }
                    }
                    WorkerMessage::Resume => { /* Should be handled by the paused loop */ }
                }
            }
            // Handle channel closed
            Err(broadcast::error::RecvError::Closed) => {
                logger.warning(&format!(
                    "Worker {:?} channel disconnected, exiting.",
                    thread_id
                ));
                break 'main_loop;
            }
            // Handle lagged receiver
            Err(broadcast::error::RecvError::Lagged(n)) => {
                logger.warning(&format!("Worker {:?} lagged by {} messages.", thread_id, n));
                // If lagging, we might miss Stop/Pause signals.
                // Continue processing tasks, but log the warning.
            }
        }
    }

    // Ensure final stats update is sent (use try_send)
    let _ = thread_stats_tx.try_send(ThreadStats {
        id: thread_id,
        requests,
        last_active: Instant::now(),
    });

    logger.info(&format!(
        "Worker thread {:?} finished after {} requests.",
        thread_id, requests
    ));
}

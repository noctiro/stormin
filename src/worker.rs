use crate::config::loader::AttackConfig;
use crate::logger::Logger;
use crate::template::render_ast_node;
use rand::rngs::{SmallRng, ThreadRng};
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use reqwest::{Client, Method};
use std::collections::HashMap;
use std::thread::ThreadId;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio::time::sleep;

#[derive(Debug, Clone, Copy)]
pub enum WorkerMessage {
    Task,
    Pause,
    Resume,
    Stop,
}

#[derive(Debug)]
pub struct TargetUpdate {
    pub url: String,
    pub success: bool,
    pub timestamp: Instant,
    pub debug: Option<String>, // Full debug message for logging
    pub network_error: Option<String>, // Specific error for UI display when request fails early 响应前失败
    pub thread_id: ThreadId, // Add ThreadId
}

struct RequestBuffer {
    params_vec: Vec<(String, String)>,
}

impl RequestBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            params_vec: Vec::with_capacity(capacity),
        }
    }

    fn clear(&mut self) {
        self.params_vec.clear();
    }
}

pub async fn worker_loop(
    mut rx: broadcast::Receiver<WorkerMessage>,
    config: AttackConfig,
    thread_id: ThreadId,
    logger: Logger,
    stats_tx: tokio::sync::mpsc::Sender<TargetUpdate>,
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

    'main_loop: loop {
        while paused {
            match rx.recv().await {
                Ok(WorkerMessage::Resume) => {
                    logger.info(&format!("Worker {:?} resuming...", thread_id));
                    paused = false;
                }
                Ok(WorkerMessage::Stop) => {
                    logger.info(&format!("Worker {:?} stopping while paused...", thread_id));
                    return;
                }
                Ok(_) => { /* Ignore other messages */ }
                Err(broadcast::error::RecvError::Closed) => {
                    logger.warning(&format!(
                        "Worker {:?} channel disconnected while paused, exiting.",
                        thread_id
                    ));
                    return;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    logger.warning(&format!(
                        "Worker {:?} lagged by {} messages while paused.",
                        thread_id, n
                    ));
                }
            }
            sleep(loop_sleep_duration).await;
        }

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
                        continue;
                    }
                    WorkerMessage::Task => {
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

                        let mut req = client.request(target.method.clone(), &target.url);

                        // 添加headers
                        for (key, value) in &target.headers {
                            req = req.header(key, value);
                        }

                        // 构建请求参数
                        buffer.clear();
                        // Create a fresh context for each request task
                        let mut context = HashMap::new();
                        for (key, template) in &target.params {
                            // Render the value, passing the mutable context
                            let value_result =
                                render_ast_node(template, &mut context, logger.clone());
                            match value_result {
                                Ok(value_string) => {
                                    buffer.params_vec.push((key.clone(), value_string));
                                }
                                Err(e) => {
                                    logger.warning(&format!(
                                        "Worker {:?}: Failed to render template for param '{}' in target '{}': {}. Skipping param.",
                                        thread_id, key, target.url, e
                                    ));
                                    // Optionally push an empty string or skip entirely
                                    // buffer.params_vec.push((key.clone(), String::new()));
                                }
                            }
                        }

                        // 根据方法添加参数
                        match target.method {
                            Method::GET | Method::DELETE => {
                                req = req.query(&buffer.params_vec);
                            }
                            Method::POST | Method::PUT | Method::PATCH => {
                                req = req.form(&buffer.params_vec);
                            }
                            _ => {
                                logger.warning(&format!(
                                    "Worker {:?}: Unsupported method {} for params/body",
                                    thread_id, target.method
                                ));
                            }
                        }

                        // 发送请求并处理结果
                        let start_time = Instant::now();
                        let res = req.send().await;
                        let timestamp = Instant::now();
                        let duration = timestamp.duration_since(start_time);

                        let (success, status_code, error_details) = match res {
                            Ok(response) => {
                                let success = response.status().is_success();
                                let status = response.status();
                                // Optionally read body for more details, but be careful with large responses
                                // let body_text = response.text().await.unwrap_or_else(|e| format!("Error reading body: {}", e));
                                (success, Some(status), None) // Store status code
                            }
                            Err(e) => {
                                // Request failed before getting a response
                                (false, None, Some(e.to_string())) // Store error string
                            }
                        };

                        let debug_message = format!(
                            "[Request Debug]\nURL: {}\nMethod: {}\nDuration: {:?}\nStatus: {}\nParams: {}\nError: {}",
                            target.url,
                            target.method,
                            duration,
                            status_code.map_or_else(|| "N/A".to_string(), |s| s.to_string()),
                            buffer
                                .params_vec
                                .iter()
                                .map(|(k, v)| format!("{}={}", k, v))
                                .collect::<Vec<_>>()
                                .join("&"),
                            error_details.as_deref().unwrap_or("None")
                        );
                        logger.debug(&debug_message); // Log detailed debug info regardless of success

                        // 发送统计信息到UI
                        let update = TargetUpdate {
                            url: target.url.clone(),
                            success,
                            timestamp,
                            // Pass the detailed debug message to the stats channel
                            debug: Some(debug_message),
                            // Populate network_error only if the request failed before getting a status
                            network_error: error_details, // error_details is Some(String) only on Err(e)
                            thread_id, // Include thread_id
                        };
                        if let Err(e) = stats_tx.send(update).await {
                            logger.warning(&format!("Failed to send stats update: {}", e));
                        }
                    }
                    WorkerMessage::Resume => {}
                }
            }
            Err(broadcast::error::RecvError::Closed) => {
                logger.warning(&format!(
                    "Worker {:?} channel disconnected, exiting.",
                    thread_id
                ));
                break 'main_loop;
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                logger.warning(&format!("Worker {:?} lagged by {} messages.", thread_id, n));
            }
        }
    }

    logger.info(&format!("Worker thread {:?} finished", thread_id));
}

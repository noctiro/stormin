use crate::config::loader::AttackConfig;
use crate::logger::Logger;
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
    let mut paused = false;
    let loop_sleep_duration = Duration::from_millis(10);

    // 智能代理选择
    let proxy_config = if !config.proxies.is_empty() {
        // 使用线程ID来确定代理，确保同一线程始终使用相同代理
        let thread_id_hash = format!("{:?}", thread_id)
            .as_bytes()
            .iter()
            .fold(0u64, |acc, &x| acc.wrapping_add(x as u64));
        let proxy_index = (thread_id_hash as usize) % config.proxies.len();
        Some(config.proxies[proxy_index].clone())
    } else {
        None
    };

    // 更优的客户端配置
    let client_builder = Client::builder()
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .timeout(config.timeout)
        .pool_idle_timeout(Some(Duration::from_secs(90))); // 增加连接池空闲超时

    let client = match proxy_config {
        Some(proxy) => match reqwest::Proxy::all(proxy.to_url_string()) {
            Ok(reqwest_proxy) => client_builder.proxy(reqwest_proxy).build(),
            Err(e) => {
                logger.error(&format!(
                    "Worker {:?}: Failed to create proxy object from {}, falling back: {}",
                    thread_id, proxy.raw, e
                ));
                client_builder.build()
            }
        },
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
                         // 使用预分配缓冲区来构建请求消息
                         let PreGeneratedRequest {
                             target_id,
                             target_url,
                             method,
                             rendered_headers,
                             rendered_params,
                         } = pre_gen_req;

                        let mut req_builder = client.request(method.clone(), &target_url);

                        // 优化头部应用逻辑
                        for (key, value_string) in &rendered_headers {
                            match reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                                Ok(header_name) => {
                                    match reqwest::header::HeaderValue::from_str(value_string) {
                                        Ok(header_value) => {
                                            req_builder = req_builder.header(header_name, header_value);
                                        }
                                        Err(e) => {
                                            if cfg!(debug_assertions) {
                                                logger.warning(&format!("Worker {:?}: Invalid header value for '{}': {} (Value: '{}')",
                                                    thread_id, key, e, value_string));
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    if cfg!(debug_assertions) {
                                        logger.warning(&format!("Worker {:?}: Invalid header name '{}': {}",
                                            thread_id, key, e));
                                    }
                                }
                            }
                        }

                        // 优化参数应用逻辑
                        match method {
                            Method::GET | Method::DELETE | Method::OPTIONS => {
                                req_builder = req_builder.query(&rendered_params);
                            }
                            Method::POST | Method::PUT | Method::PATCH => {
                                req_builder = req_builder.form(&rendered_params);
                            }
                            _ => {
                                logger.warning(&format!(
                                    "Worker {:?}: Unsupported method {} for params",
                                    thread_id, method
                                ));
                                continue 'main_loop;
                            }
                        }

                        // 执行请求并测量时间
                        let start_time = Instant::now();
                        let res = req_builder.send().await;
                        let timestamp = Instant::now();
                        let duration = timestamp.duration_since(start_time);

                        let (success, status_code, error_details) = match res {
                            Ok(response) => {
                                let success_status = response.status().is_success();
                                let status = response.status();
                                if !success_status {
                                    let err_msg = format!(
                                        "HTTP {} {}",
                                        status.as_u16(),
                                        status.canonical_reason().unwrap_or("Unknown")
                                    );
                                    (false, Some(status), Some(err_msg))
                                } else {
                                    (true, Some(status), None)
                                }
                            }
                            Err(e) => {
                                let err_msg = if e.is_timeout() {
                                    "Timeout".to_string()
                                } else if e.is_connect() {
                                    "Connection Error".to_string()
                                } else if e.is_redirect() {
                                    format!("Redirect Error: {}", e)
                                } else if e.is_status() {
                                    format!("HTTP Error: {}", e)
                                } else if e.is_body() {
                                    format!("Body Error: {}", e)
                                } else if e.is_request() {
                                    format!("Request Error: {}", e)
                                } else if e.is_decode() {
                                    format!("Decode Error: {}", e)
                                } else if e.is_builder() {
                                    format!("Builder Error: {}", e)
                                } else {
                                    format!("Other Error: {}", e)
                                };
                                (false, None, Some(err_msg))
                            }
                        };

                        // 使用预分配容量构建消息，减少内存分配
                        let mut attack_message = String::with_capacity(512);

                        attack_message.push_str("[Request]\n");
                        attack_message.push_str(&format!("URL: {}\n", target_url));
                        attack_message.push_str(&format!("Method: {}\n", method));
                        attack_message.push_str(&format!("Duration: {:?}\n", duration));
                        attack_message.push_str(&format!("Status: {}",
                            status_code.map_or_else(|| "N/A".to_string(), |s| s.to_string())));

                        if !rendered_headers.is_empty() {
                            attack_message.push_str("\nHeaders:\n");
                            for (i, (k, v)) in rendered_headers.iter().enumerate() {
                                if i > 0 {
                                    attack_message.push('\n');
                                }
                                attack_message.push_str(&format!("  {}: {}", k, v));
                            }
                        }

                        if !rendered_params.is_empty() {
                            attack_message.push_str("\nParams: ");
                            for (i, (k, v)) in rendered_params.iter().enumerate() {
                                if i > 0 {
                                    attack_message.push('&');
                                }
                                attack_message.push_str(&format!("{}={}", k, v));
                            }
                        }

                        if let Some(err) = &error_details {
                            attack_message.push_str(&format!("\nError: {}", err));
                        }

                        let update = TargetUpdate {
                            id: target_id,
                            url: target_url,
                            success,
                            timestamp,
                            debug: Some(attack_message),
                            network_error: error_details.clone(),
                            thread_id,
                        };

                        // 发送状态更新
                        if stats_tx.send(update).await.is_err() {
                            logger.info(&format!("Worker {:?}: Stats channel closed, exiting.", thread_id));
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
    }
}

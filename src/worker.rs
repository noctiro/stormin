use crate::config::{AttackConfig, CompiledTarget};
use crate::generator::{
    password::PasswordGenerator, ChineseSocialPasswordGenerator, QQIDGenerator,
    RandomPasswordGenerator, UsernameGenerator,
};
use crate::logger::Logger;
use crate::template::render_compiled_url;
use crate::ui::ThreadStats;
use crossbeam_channel::{Receiver, Sender};
use rand::rngs::{SmallRng, ThreadRng};
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng}; // Keep Rng trait for .gen() and .random_bool()
use reqwest::Client;
use std::{
    thread::ThreadId,
    time::{Duration, Instant},
};
use tokio::time::sleep;

#[derive(Debug, Clone)]
pub enum RequestResult {
    Success,
    Failure,
}

#[derive(Debug, Clone)]
pub enum WorkerMessage {
    Task,
    Pause,
    Resume,
    Stop,
}

// 目标统计通道
#[derive(Debug, Clone)]
pub struct TargetUpdate {
    pub url: String,
    pub success: bool,
    pub timestamp: std::time::Instant,
    pub debug: Option<String>,
}

// Change to async fn
pub async fn worker_loop(
    rx: Receiver<WorkerMessage>,
    config: AttackConfig, // Use the imported type
    tx: Sender<RequestResult>,
    thread_id: ThreadId,
    thread_stats_tx: Sender<ThreadStats>,
    target_stats_tx: Sender<TargetUpdate>,
    logger: Logger,
) {
    // Use SmallRng seeded from thread-local entropy
    let mut rng = SmallRng::seed_from_u64(ThreadRng::default().random());
    let mut username_gen = UsernameGenerator::new();
    let mut pwd_gen_social = ChineseSocialPasswordGenerator::new();
    let mut pwd_gen_rand = RandomPasswordGenerator::new();
    let mut qqid_gen = QQIDGenerator::new();
    let mut requests = 0u64;
    let mut paused = false; // Track pause state locally

    // --- Create Async HTTP Client Once ---
    let client_timeout = Duration::from_secs(10); // Keep using config or defaults
    let connect_timeout = Duration::from_secs(5);
    let proxy_config = config.proxies.choose(&mut rng).cloned();

    let client_builder = Client::builder() // Start with the async builder
        .timeout(client_timeout)
        .connect_timeout(connect_timeout);

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
                    // Build without proxy on error
                    Client::builder()
                        .timeout(client_timeout)
                        .connect_timeout(connect_timeout)
                        .build()
                }
            }
        }
        None => {
            // Build without proxy if none configured
            client_builder.build()
        }
    }
    .unwrap_or_else(|e| {
        // Fallback client if builder fails
        logger.error(&format!(
            "Worker {:?}: Failed to build client, falling back to default: {}",
            thread_id, e
        ));
        Client::new()
    });
    // --- End Client Creation ---

    let loop_sleep_duration = Duration::from_millis(10); // Short sleep when idle

    loop {
        // Handle pause state first
        while paused {
            match rx.try_recv() {
                Ok(WorkerMessage::Resume) => {
                    logger.info(&format!("Worker {:?} resuming...", thread_id));
                    paused = false;
                }
                Ok(WorkerMessage::Stop) => {
                    logger.info(&format!("Worker {:?} stopping while paused...", thread_id));
                    return;
                }
                Ok(_) => {} // Ignore other messages like Task while paused
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    // Sleep briefly while paused and waiting for Resume/Stop
                    sleep(loop_sleep_duration).await;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    logger.warning(&format!(
                        "Worker {:?} channel disconnected while paused, exiting.",
                        thread_id
                    ));
                    return;
                }
            }
        }

        // Process messages if not paused
        match rx.try_recv() {
            Ok(msg) => {
                match msg {
                    WorkerMessage::Stop => {
                        logger.info(&format!("Worker {:?} received Stop signal.", thread_id));
                        break; // Exit the main loop
                    }
                    WorkerMessage::Pause => {
                        logger.info(&format!("Worker {:?} pausing...", thread_id));
                        paused = true;
                        continue; // Go back to the start of the loop to enter the paused state
                    }
                    WorkerMessage::Task => {
                        // Select target
                        let target: &CompiledTarget = match config.targets.choose(&mut rng) {
                            Some(t) => t,
                            None => {
                                logger.error(&format!(
                                    "Worker {:?}: No targets available, skipping task.",
                                    thread_id
                                ));
                                continue; // Skip to next loop iteration
                            }
                        };

                        // Generate credentials
                        let needs_user = target.params.iter().any(|(_, c)| c.needs_user);
                        let needs_pwd = target.params.iter().any(|(_, c)| c.needs_password);
                        let needs_qq = target.params.iter().any(|(_, c)| c.needs_qqid);

                        let username = needs_user.then(|| username_gen.generate_random());
                        let password = needs_pwd.then(|| {
                            if rng.random_bool(0.6) {
                                pwd_gen_social.generate()
                            } else {
                                pwd_gen_rand.generate()
                            }
                        });
                        let qqid = needs_qq.then(|| qqid_gen.generate_qq_id());

                        let method = match target.method.parse() {
                            Ok(m) => m,
                            Err(_) => {
                                logger.error(&format!("Worker {:?}: Invalid HTTP method '{}' for target {}, skipping.", thread_id, target.method, target.url));
                                continue;
                            }
                        };

                        // Use target.url directly as it's a String, not CompiledUrl
                        let mut req = client.request(method, &target.url);

                        // Use header values directly (assuming they are not templates)
                        for (key, value) in &target.headers {
                            req = req.header(key, value); // Use the value directly
                        }

                        // Render param values (which are CompiledUrl) into Strings
                        let mut params_vec: Vec<(String, String)> = Vec::new(); // Explicit type
                        for (key, template) in &target.params {
                            let value_string = render_compiled_url(
                                // Render CompiledUrl to String
                                template,
                                username.as_deref(),
                                password.as_deref(),
                                qqid.as_deref(),
                            );
                            params_vec.push((key.clone(), value_string)); // Store the rendered String
                        }

                        // Apply params_vec (now Vec<(String, String)>) based on method
                        match target.method.to_uppercase().as_str() {
                            "GET" | "DELETE" => {
                                req = req.query(&params_vec); // reqwest can serialize Vec<(String, String)>
                            }
                            "POST" | "PUT" | "PATCH" => {
                                req = req.form(&params_vec); // reqwest can serialize Vec<(String, String)>
                            }
                            _ => {
                                logger.warning(&format!("Worker {:?}: Unsupported method {} for params/body, sending without.", thread_id, target.method));
                            }
                        }

                        // Build debug string using the rendered params_vec
                        let params_debug = params_vec
                            .iter()
                            .map(|(key, value)| format!("{}={}", key, value)) // value is now String, Display is implemented
                            .collect::<Vec<_>>()
                            .join("&");

                        // Use target.url in debug info
                        let debug_info = format!(
                            "[Request]\nURL: {}\nMethod: {}\nParams: {}",
                            target.url, target.method, params_debug
                        );

                        let _ = target_stats_tx.send(TargetUpdate {
                            url: target.url.clone(),
                            success: false,
                            timestamp: Instant::now(),
                            debug: Some(debug_info.clone()), // Clone debug info for logging
                        });

                        // --- Send Request Asynchronously ---
                        let request_start = Instant::now();
                        let res = req.send().await; // Use .await here
                        let request_duration = request_start.elapsed();
                        requests += 1;
                        // --- End Send Request ---

                        // Process the result AFTER awaiting
                        let success = res
                            .as_ref()
                            .map(|r| r.status().is_success())
                            .unwrap_or(false);

                        let response_info = match &res {
                            // Match on the Result directly
                            Ok(r) => format!(
                                "[Response]\nStatus: {} {}\nTime: {:.2}ms",
                                r.status().as_u16(),
                                r.status().canonical_reason().unwrap_or("Unknown"),
                                request_duration.as_secs_f64() * 1000.0
                            ),
                            Err(e) => format!("[Error]\n{}", e),
                        };

                        let _ = target_stats_tx.send(TargetUpdate {
                            url: target.url.clone(),
                            success,
                            timestamp: Instant::now(),
                            debug: Some(response_info.clone()), // Clone response info for logging
                        });

                        let result_msg = if success {
                            RequestResult::Success
                        } else {
                            RequestResult::Failure
                        };
                        if tx.send(result_msg).is_err() {
                            logger.error(&format!("Worker {:?}: Failed to send request result to stats channel, exiting.", thread_id));
                            break;
                        }

                        let _ = thread_stats_tx.send(ThreadStats {
                            id: thread_id,
                            requests,
                            last_active: Instant::now(),
                        });
                    }
                    WorkerMessage::Resume => { /* Should be handled by the paused loop */ }
                }
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {
                // No message received, sleep briefly to avoid busy-waiting
                sleep(loop_sleep_duration).await;
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                logger.warning(&format!(
                    "Worker {:?} channel disconnected, exiting.",
                    thread_id
                ));
                break; // Exit the main loop
            }
        }
    }
    logger.info(&format!(
        "Worker thread {:?} finished after {} requests.",
        thread_id, requests
    ));
}

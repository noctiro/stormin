use crate::config::AttackConfig;
use crate::logger::Logger;
use crate::template::render_ast_node;
use crate::ui::ThreadStats;
use crossbeam_channel::{Receiver, Sender};
use rand::rngs::{SmallRng, ThreadRng};
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use reqwest::{Client, Method};
use std::{
    thread::ThreadId,
    time::{Duration, Instant},
};
use tokio::time::sleep;

#[derive(Debug, Clone, Copy)]
pub enum RequestResult {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy)]
pub enum WorkerMessage {
    Task,
    Pause,
    Resume,
    Stop,
}

// 目标统计通道
#[derive(Debug)]
pub struct TargetUpdate {
    pub url: String,
    pub success: bool,
    pub timestamp: Instant,
    pub debug: Option<String>,
}

// 创建一个预分配缓冲区结构体以重用内存
struct RequestBuffer {
    params_vec: Vec<(String, String)>,
    debug_info: String,
    response_info: String,
}

impl RequestBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            params_vec: Vec::with_capacity(capacity),
            debug_info: String::with_capacity(1024),
            response_info: String::with_capacity(1024),
        }
    }

    fn clear(&mut self) {
        self.params_vec.clear();
        self.debug_info.clear();
        self.response_info.clear();
    }
}

pub async fn worker_loop(
    rx: Receiver<WorkerMessage>,
    config: AttackConfig,
    tx: Sender<RequestResult>,
    thread_id: ThreadId,
    thread_stats_tx: Sender<ThreadStats>,
    target_stats_tx: Sender<TargetUpdate>,
    logger: Logger,
) {
    // 使用固定大小预分配的缓冲区
    let mut buffer = RequestBuffer::new(16);

    // 使用SmallRng提高性能
    let mut rng = SmallRng::seed_from_u64(ThreadRng::default().random());
    let mut requests = 0u64;
    let mut paused = false;

    // 预先计算睡眠时间以避免重复创建Duration对象
    let loop_sleep_duration = Duration::from_millis(10);

    // 预先选择代理，避免每次请求时重新选择
    let proxy_config = config.proxies.choose(&mut rng).cloned();

    // 构建HTTP客户端，增加连接池配置
    let client_builder = Client::builder()
        .pool_max_idle_per_host(10) // 增加连接池大小
        .tcp_keepalive(Some(Duration::from_secs(30))); // 保持TCP连接活跃

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
                    // 出错时不使用代理构建
                    Client::builder()
                        .pool_max_idle_per_host(10)
                        .tcp_keepalive(Some(Duration::from_secs(30)))
                        .build()
                }
            }
        }
        None => {
            // 不需要代理时的构建
            client_builder.build()
        }
    }
    .unwrap_or_else(|e| {
        // 构建失败时的后备客户端
        logger.error(&format!(
            "Worker {:?}: Failed to build client, falling back to default: {}",
            thread_id, e
        ));
        Client::new()
    });

    // 缓存HTTP方法字符串到Method枚举的映射，避免重复解析
    let method_get = Method::GET;
    let method_post = Method::POST;
    let method_put = Method::PUT;
    let method_delete = Method::DELETE;
    let method_patch = Method::PATCH;

    let mut last_stats_update = Instant::now();
    let stats_update_interval = Duration::from_millis(500); // 限制状态更新频率

    // 主循环
    'main_loop: loop {
        // 先处理暂停状态
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
                Ok(_) => {} // 忽略暂停时的其他消息
                Err(crossbeam_channel::TryRecvError::Empty) => {
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

        // 非暂停状态下处理消息
        match rx.try_recv() {
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
                        // 执行请求任务
                        requests += 1;

                        // 选择目标
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

                        // 使用缓存的Method枚举减少解析
                        let method = match target.method.to_uppercase().as_str() {
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

                        // 构建请求
                        let mut req = client.request(method.clone(), &target.url);

                        // 添加头信息
                        for (key, value) in &target.headers {
                            req = req.header(key, value);
                        }

                        // 清除并重用上一次的缓冲区
                        buffer.clear();

                        // 渲染参数值
                        for (key, template) in &target.params {
                            let value_string = render_ast_node(template, logger.clone());
                            buffer.params_vec.push((key.clone(), value_string));
                        }

                        // 根据方法应用参数
                        match *method {
                            Method::GET | Method::DELETE => {
                                req = req.query(&buffer.params_vec);
                            }
                            Method::POST | Method::PUT | Method::PATCH => {
                                req = req.form(&buffer.params_vec);
                            }
                            _ => {
                                logger.warning(&format!(
                                    "Worker {:?}: Unsupported method {} for params/body, sending without.", 
                                    thread_id, target.method));
                            }
                        }

                        // 构建调试信息字符串
                        for (i, (key, value)) in buffer.params_vec.iter().enumerate() {
                            if i > 0 {
                                buffer.debug_info.push_str("&");
                            }
                            buffer.debug_info.push_str(key);
                            buffer.debug_info.push_str("=");
                            buffer.debug_info.push_str(value);
                        }

                        // 添加URL和方法信息
                        let url_str = &target.url;
                        let method_str = &target.method;

                        // 直接写入预分配的缓冲区而不是创建新字符串
                        buffer.debug_info = format!(
                            "[Request]\nURL: {}\nMethod: {}\nParams: {}",
                            url_str, method_str, buffer.debug_info
                        );

                        // 发送目标更新
                        let _ = target_stats_tx.send(TargetUpdate {
                            url: url_str.clone(),
                            success: false,
                            timestamp: Instant::now(),
                            debug: Some(buffer.debug_info.clone()),
                        });

                        // 异步发送请求
                        let request_start = Instant::now();
                        let res = req.send().await;
                        let request_duration = request_start.elapsed();

                        // 处理结果
                        let success = res
                            .as_ref()
                            .map(|r| r.status().is_success())
                            .unwrap_or(false);

                        // 构建响应信息
                        match &res {
                            Ok(r) => {
                                let status = r.status();
                                buffer.response_info = format!(
                                    "[Response]\nStatus: {} {}\nTime: {:.2}ms",
                                    status.as_u16(),
                                    status.canonical_reason().unwrap_or("Unknown"),
                                    request_duration.as_secs_f64() * 1000.0
                                );
                            }
                            Err(e) => {
                                buffer.response_info = format!("[Error]\n{}", e);
                            }
                        }

                        // 发送目标更新
                        let _ = target_stats_tx.send(TargetUpdate {
                            url: url_str.clone(),
                            success,
                            timestamp: Instant::now(),
                            debug: Some(buffer.response_info.clone()),
                        });

                        // 发送结果统计
                        let result_msg = if success {
                            RequestResult::Success
                        } else {
                            RequestResult::Failure
                        };

                        if tx.send(result_msg).is_err() {
                            logger.error(&format!(
                                "Worker {:?}: Failed to send request result to stats channel, exiting.", 
                                thread_id));
                            break 'main_loop;
                        }

                        // 限制状态更新频率以减少通道负载
                        let now = Instant::now();
                        if now.duration_since(last_stats_update) >= stats_update_interval {
                            let _ = thread_stats_tx.send(ThreadStats {
                                id: thread_id,
                                requests,
                                last_active: now,
                            });
                            last_stats_update = now;
                        }
                    }
                    WorkerMessage::Resume => { /* 由暂停循环处理 */ }
                }
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {
                // 没有消息，短暂休眠避免忙等
                sleep(loop_sleep_duration).await;
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                logger.warning(&format!(
                    "Worker {:?} channel disconnected, exiting.",
                    thread_id
                ));
                break 'main_loop;
            }
        }
    }

    // 确保最后发送一次线程状态更新
    let _ = thread_stats_tx.send(ThreadStats {
        id: thread_id,
        requests,
        last_active: Instant::now(),
    });

    logger.info(&format!(
        "Worker thread {:?} finished after {} requests.",
        thread_id, requests
    ));
}

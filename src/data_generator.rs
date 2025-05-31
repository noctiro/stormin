use crate::config::loader;
use crate::logger::Logger;
use crate::template::render_ast_node;
use crate::ui::Stats;
use crate::worker::PreGeneratedRequest;

use dashmap::DashMap;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::time::sleep;

pub async fn data_generator_loop(
    generator_id: usize,
    config: loader::AttackConfig,
    target_ids: Vec<usize>,
    data_pool_tx: mpsc::Sender<PreGeneratedRequest>,
    logger: Logger,
    stop_signal: Arc<AtomicBool>,
    stats: Arc<Mutex<Stats>>,
) {
    logger.info(&format!("Data generator loop {} started.", generator_id));
    // 使用更高效的随机数生成器
    let mut rng = StdRng::from_os_rng();
    let mut current_delay_micros = config.initial_delay_micros;

    // 筛选出此生成器负责的目标配置
    let my_target_configs: Vec<&loader::CompiledTarget> = config
        .targets
        .iter()
        .filter(|t| target_ids.contains(&t.id))
        .collect();

    if my_target_configs.is_empty() {
        logger.error(&format!(
            "Data generator {}: No targets configured. Exiting.",
            generator_id
        ));
        return;
    }

    logger.info(&format!(
        "Data generator {}: Starting continuous generation with {} targets. Initial delay: {} µs",
        generator_id,
        my_target_configs.len(),
        current_delay_micros
    ));

    // 使用 DashMap 缓存目标状态，减少锁操作
    let target_stats_cache = DashMap::new();
    let mut last_stats_refresh = Instant::now();
    let stats_cache_refresh_interval = Duration::from_millis(500);

    // 创建请求生成池 - 批量生成请求
    let mut request_batch = Vec::with_capacity(10);

    while !stop_signal.load(Ordering::Relaxed) {
        // 周期性刷新目标状态缓存
        let refresh_cache = last_stats_refresh.elapsed() >= stats_cache_refresh_interval;
        if refresh_cache {
            // 更新状态缓存
            let stats_guard = stats.lock().await;
            for target in &my_target_configs {
                if let Some(stat) = stats_guard.targets.iter().find(|s| s.id == target.id) {
                    target_stats_cache.insert(target.id, stat.error_rate);
                } else {
                    target_stats_cache.insert(target.id, 0.0);
                }
            }
            last_stats_refresh = Instant::now();
            drop(stats_guard);
        }

        // 使用缓存计算权重和选择目标
        let mut targets_with_weights = Vec::with_capacity(my_target_configs.len());
        {
            // 获取 stats_guard 以便读取最新的目标统计
            let stats_guard = stats.lock().await;
            for target in &my_target_configs {
                // 查找目标统计
                let stat = stats_guard.targets.iter().find(|s| s.id == target.id);
                let (failure, success, error_rate, last_network_error) = if let Some(stat) = stat {
                    (
                        stat.failure,
                        stat.success,
                        stat.error_rate,
                        stat.last_network_error.as_ref(),
                    )
                } else {
                    (0, 0, 0.0, None)
                };

                // weight 计算逻辑：
                // 1. 错误率高，weight 低
                // 2. 失败数远大于成功数，weight 低
                // 3. 有网络错误，weight 进一步降低
                let mut weight = 1.0;
                if error_rate > 0.8 {
                    weight = 0.05;
                } else if error_rate > 0.5 {
                    weight = 0.15;
                } else if error_rate > 0.2 {
                    weight = 0.5;
                } else if error_rate > 0.05 {
                    weight = 0.8;
                }
                if failure > success * 2 && failure > 20 {
                    weight *= 0.5;
                }
                if let Some(err) = last_network_error {
                    if !err.is_empty() {
                        weight *= 0.3;
                    }
                }
                // 保证 weight 不为负
                if weight < 0.01 {
                    weight = 0.01;
                }
                targets_with_weights.push((target, weight));
            }
        }
        // 使用加权随机选择
        let target_config = if targets_with_weights.is_empty() {
            logger.error(&format!(
                "Data generator {}: No targets available for generation.",
                generator_id
            ));
            sleep(Duration::from_secs(1)).await;
            continue;
        } else {
            let total_weight: f64 = targets_with_weights.iter().map(|(_, w)| w).sum();

            if total_weight <= 0.0 {
                // 所有目标权重为0，随机选择一个
                let random_idx = (rand::random::<f64>() * my_target_configs.len() as f64) as usize;
                my_target_configs[random_idx % my_target_configs.len()]
            } else {
                let pick = rand::random::<f64>() * total_weight;
                let mut acc = 0.0;
                let mut selected = targets_with_weights[0].0; // 默认值

                for (target, weight) in &targets_with_weights {
                    acc += weight;
                    if pick <= acc {
                        selected = target;
                        break;
                    }
                }
                selected
            }
        };

        let mut target_context_map = HashMap::new();

        let mut rendered_headers = Vec::with_capacity(target_config.headers.len());
        for (key, template_node) in &target_config.headers {
            match render_ast_node(
                template_node,
                &mut target_context_map,
                logger.clone(),
                &mut rng,
            ) {
                Ok(value_string) => rendered_headers.push((key.clone(), value_string)),
                Err(e) => logger.warning(&format!(
                    "Data generator {}: Failed to render header '{}' for target '{}': {}",
                    generator_id, key, target_config.url, e
                )),
            }
        }

        let mut rendered_params = Vec::with_capacity(target_config.params.len());
        for (key, template_node) in &target_config.params {
            match render_ast_node(
                template_node,
                &mut target_context_map,
                logger.clone(),
                &mut rng,
            ) {
                Ok(value_string) => rendered_params.push((key.clone(), value_string)),
                Err(e) => logger.warning(&format!(
                    "Data generator {}: Failed to render param '{}' for target '{}': {}",
                    generator_id, key, target_config.url, e
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

        // 添加到批处理请求
        request_batch.push(pre_gen_req);

        // 当批次满或其他条件满足时，尝试发送请求
        if request_batch.len() >= 10 || refresh_cache {
            let mut backoff_count = 0;
            const MAX_BACKOFF_COUNT: usize = 10;

            'send_loop: while !request_batch.is_empty() && !stop_signal.load(Ordering::Relaxed) {
                let req = request_batch[0].clone();

                match data_pool_tx.try_send(req) {
                    Ok(_) => {
                        // 成功发送，移除已发送的请求
                        request_batch.remove(0);

                        // 根据当前延迟调整
                        current_delay_micros =
                            ((current_delay_micros as f64 * config.decrease_factor) as u64)
                                .max(config.min_delay_micros);

                        // 重置退避计数
                        backoff_count = 0;
                    }
                    Err(TrySendError::Full(_req)) => {
                        backoff_count += 1;

                        // 指数退避策略
                        if backoff_count > MAX_BACKOFF_COUNT {
                            current_delay_micros = config.max_delay_micros;
                        } else {
                            let old_delay = current_delay_micros;
                            current_delay_micros =
                                ((current_delay_micros as f64 * config.increase_factor) as u64)
                                    .min(config.max_delay_micros);

                            if current_delay_micros > old_delay && backoff_count % 3 == 0 {
                                logger.warning(&format!(
                                    "Data generator {}: Pool full. Delay increased: {} -> {} µs",
                                    generator_id, old_delay, current_delay_micros
                                ));
                            }
                        }

                        // 暂停一段时间后重试
                        sleep(Duration::from_micros(current_delay_micros / 2)).await;
                        continue 'send_loop;
                    }
                    Err(TrySendError::Closed(_)) => {
                        logger.error(&format!(
                            "Data generator {}: Data pool channel closed. Stopping generation.",
                            generator_id
                        ));
                        return;
                    }
                }
            }

            if stop_signal.load(Ordering::Relaxed) {
                logger.info(&format!(
                    "Data generator {}: Stop signal received, exiting.",
                    generator_id
                ));
                return;
            }
        }

        sleep(Duration::from_micros(current_delay_micros)).await;
    }
    logger.info(&format!("Data generator loop {} finished.", generator_id));
}

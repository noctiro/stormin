use crate::config::loader;
use crate::logger::Logger;
use crate::template::render_ast_node;
use crate::ui::Stats;
use crate::worker::PreGeneratedRequest;

use rand::SeedableRng;
use rand::rngs::StdRng;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
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
    let mut rng = StdRng::from_os_rng();
    let mut current_delay_micros = config.initial_delay_micros;

    logger.info(&format!(
        "Data generator {}: Starting continuous generation. Initial delay: {} µs",
        generator_id, current_delay_micros
    ));

    while !stop_signal.load(Ordering::Relaxed) {
        // 动态读取最新TargetStats
        let stats_guard = stats.lock().await;
        let mut my_targets = Vec::new();
        let mut weights = Vec::new();
        for t in &config.targets {
            if !target_ids.contains(&t.id) {
                continue;
            }
            // 查找最新TargetStats
            if let Some(stat) = stats_guard.targets.iter().find(|s| s.id == t.id) {
                let weight = if stat.is_dying {
                    0.01
                } else if stat.error_rate > 0.5 {
                    0.1
                } else if stat.error_rate > 0.2 {
                    0.3
                } else {
                    1.0
                };
                my_targets.push(t);
                weights.push(weight);
            } else {
                my_targets.push(t);
                weights.push(1.0);
            }
        }
        drop(stats_guard);
        // 按权重随机选择target
        let total_weight: f64 = weights.iter().sum();
        let pick = rand::random::<f64>() * total_weight;
        let mut acc = 0.0;
        let mut picked = None;
        for (i, w) in weights.iter().enumerate() {
            acc += w;
            if pick <= acc {
                picked = Some(i);
                break;
            }
        }
        let target_config = match picked {
            Some(idx) => my_targets[idx],
            None => {
                logger.error(&format!(
                    "Data generator {}: No targets available for generation.",
                    generator_id
                ));
                sleep(Duration::from_secs(1)).await;
                continue;
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
        let mut pre_gen_req = PreGeneratedRequest {
            target_id: target_config.id,
            target_url: target_config.url.clone(),
            method: target_config.method.clone(),
            rendered_headers,
            rendered_params,
        };

        loop {
            if stop_signal.load(Ordering::Relaxed) {
                logger.info(&format!(
                    "Data generator {}: Stop signal received, exiting.",
                    generator_id
                ));
                return;
            }
            match data_pool_tx.try_send(pre_gen_req.clone()) {
                Ok(_) => {
                    current_delay_micros = ((current_delay_micros as f64 * config.decrease_factor)
                        as u64)
                        .max(config.min_delay_micros);
                    break;
                }
                Err(TrySendError::Full(req)) => {
                    let old_delay = current_delay_micros;
                    current_delay_micros = ((current_delay_micros as f64 * config.increase_factor)
                        as u64)
                        .min(config.max_delay_micros);
                    if current_delay_micros > old_delay {
                        logger.warning(&format!(
                            "Data generator {}: Pool full. Delay increased: {} -> {} µs",
                            generator_id, old_delay, current_delay_micros
                        ));
                    }
                    sleep(Duration::from_micros(current_delay_micros / 2)).await;
                    pre_gen_req = req;
                    continue;
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

        sleep(Duration::from_micros(current_delay_micros)).await;
    }
    logger.info(&format!("Data generator loop {} finished.", generator_id));
}

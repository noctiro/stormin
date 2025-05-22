use crate::logger::Logger;
use crate::ui::{DebugInfo, Stats, ThreadStats}; // Assuming Stats and related structs are accessible
use crate::worker::TargetUpdate;
use dashmap::DashMap;
use std::{
    collections::HashMap,
    thread::ThreadId,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

// 控制台日志和历史数据的容量限制
const MAX_CONSOLE_LOGS: usize = 250;
const HISTORY_CAPACITY: usize = 120; // For RPS and success rate history
const STATS_BATCH_SIZE: usize = 100; // 统计批处理大小

pub struct StatsUpdater {
    last_stats_update_time: Instant,
    stats_update_interval: Duration,
    requests_in_last_second: u64,
    successes_in_last_second: u64,
    sysinfo_tick: u32,
    // 使用 DashMap 替代 HashMap，避免显式锁定
    target_id_index_map: DashMap<usize, usize>, // 缓存 target_id 到 stats.targets 索引的映射
    thread_id_index_map: DashMap<ThreadId, usize>, // 缓存线程 ID 到索引的映射
    // 批处理相关
    batch_buffer: Vec<TargetUpdate>,
}

impl StatsUpdater {
    pub fn new() -> Self {
        StatsUpdater {
            last_stats_update_time: Instant::now(),
            stats_update_interval: Duration::from_secs(1),
            requests_in_last_second: 0,
            successes_in_last_second: 0,
            sysinfo_tick: 0,
            target_id_index_map: DashMap::new(),
            thread_id_index_map: DashMap::new(),
            batch_buffer: Vec::with_capacity(STATS_BATCH_SIZE),
        }
    }
    
    // 添加辅助方法来更新缓存
    fn rebuild_target_cache(&mut self, stats: &Stats) {
        self.target_id_index_map.clear();
        for (i, target) in stats.targets.iter().enumerate() {
            self.target_id_index_map.insert(target.id, i);
        }
    }

    // Returns true if stats were updated and might need a redraw
    pub fn update_stats(
        &mut self,
        stats: &mut Stats,
        target_stats_rx: &mut mpsc::Receiver<TargetUpdate>,
        logger: &Logger,
    ) -> bool {
        let mut needs_redraw_due_to_stats = false;

        // 更新系统信息（较低频率）
        self.sysinfo_tick = self.sysinfo_tick.wrapping_add(1);
        if self.sysinfo_tick % 20 == 0 {
            stats.sys.refresh_all();
            
            // 优化CPU使用率计算
            let cpus = stats.sys.cpus();
            if !cpus.is_empty() {
                let total_usage: f32 = cpus.iter().map(|cpu| cpu.cpu_usage()).sum();
                stats.cpu_usage = total_usage / cpus.len() as f32;
            }
            
            stats.memory_usage = stats.sys.used_memory();
            needs_redraw_due_to_stats = true;
        }

        // 批量收集更新
        let mut total_requests = 0u64;
        let mut total_successes = 0u64;
        
        // 如果缓存为空，重建缓存
        if self.target_id_index_map.is_empty() && !stats.targets.is_empty() {
            self.rebuild_target_cache(stats);
        }
        
        // 收集批量更新
        while let Ok(update) = target_stats_rx.try_recv() {
            self.batch_buffer.push(update);
            
            if self.batch_buffer.len() >= STATS_BATCH_SIZE {
                break; // 避免处理太多更新导致UI不响应
            }
        }
        
        if !self.batch_buffer.is_empty() {
            needs_redraw_due_to_stats = true;
            
            // 使用临时HashMap收集目标和线程更新
            let mut target_updates: HashMap<usize, (u64, u64, String, Option<Instant>, Option<Instant>, Option<String>)> = HashMap::new();
            let mut thread_updates: HashMap<ThreadId, u64> = HashMap::new();
            
            // 第一步：处理批量更新，收集统计信息
            for update in self.batch_buffer.drain(..) {
                // 处理调试消息
                if let Some(debug_msg) = update.debug {
                    stats.debug_logs.push_back(DebugInfo {
                        timestamp: update.timestamp,
                        message: debug_msg,
                    });
                    while stats.debug_logs.len() > MAX_CONSOLE_LOGS {
                        stats.debug_logs.pop_front();
                    }
                }
                
                if update.url.is_empty() {
                    // 纯调试消息，跳过统计更新
                    continue;
                }
                
                // 更新总计数
                total_requests += 1;
                if update.success {
                    total_successes += 1;
                    stats.last_success_time = Some(update.timestamp);
                } else {
                    stats.last_failure_time = Some(update.timestamp);
                }
                
                // 合并目标更新
                // 获取和更新值
                let update_data = (
                    update.success,
                    update.timestamp,
                    update.network_error.clone(),
                    update.url.clone()
                );
                
                target_updates
                    .entry(update.id)
                    .and_modify(|e| {
                        if update_data.0 {
                            e.0 += 1; // 成功计数
                            e.3 = Some(update_data.1); // 最后成功时间
                        } else {
                            e.1 += 1; // 失败计数
                            e.4 = Some(update_data.1); // 最后失败时间
                            if update_data.2.is_some() {
                                e.5 = update_data.2.clone(); // 网络错误
                            }
                        }
                    })
                    .or_insert_with(|| {
                        let mut success = 0;
                        let mut failure = 0;
                        let mut success_time = None;
                        let mut failure_time = None;
                        
                        if update_data.0 {
                            success = 1;
                            success_time = Some(update_data.1);
                        } else {
                            failure = 1;
                            failure_time = Some(update_data.1);
                        }
                        
                        (success, failure, update_data.3, success_time, failure_time, update_data.2)
                    });
                
                // 合并线程更新
                *thread_updates.entry(update.thread_id).or_insert(0) += 1;
            }
            
            // 更新原子计数器
            if total_requests > 0 {
                stats.total.fetch_add(total_requests, std::sync::atomic::Ordering::Relaxed);
                self.requests_in_last_second += total_requests;
            }
            
            if total_successes > 0 {
                stats.success.fetch_add(total_successes, std::sync::atomic::Ordering::Relaxed);
                self.successes_in_last_second += total_successes;
            }
            
            if total_requests > total_successes {
                stats.failure.fetch_add(
                    total_requests - total_successes, 
                    std::sync::atomic::Ordering::Relaxed
                );
            }
            
            // 为更新目标准备缓存
            let mut target_indices = Vec::new();
            let mut missing_targets = Vec::new();
            
            // 首先收集所有目标索引或确定哪些是缺失的
            for (id, _) in &target_updates {
                if let Some(idx) = self.target_id_index_map.get(id) {
                    let idx_value = *idx.value();
                    if idx_value < stats.targets.len() {
                        target_indices.push((*id, idx_value));
                    } else {
                        missing_targets.push(*id);
                    }
                } else {
                    missing_targets.push(*id);
                }
            }
            
            // 如果有缺失的目标，重建缓存
            if !missing_targets.is_empty() {
                self.rebuild_target_cache(stats);
                
                // 再次尝试获取索引
                for id in missing_targets {
                    if let Some(idx) = self.target_id_index_map.get(&id) {
                        let idx_value = *idx.value();
                        if idx_value < stats.targets.len() {
                            target_indices.push((id, idx_value));
                        } else if let Some((success, failure, url, _, _, _)) = target_updates.get(&id) {
                            let warning_msg = format!(
                                "StatsUpdater: Received update for unknown target ID: {} for URL: {}",
                                id, url
                            );
                            logger.warning(&warning_msg);
                        }
                    } else if let Some((success, failure, url, _, _, _)) = target_updates.get(&id) {
                        let warning_msg = format!(
                            "StatsUpdater: Received update for unknown target ID: {} for URL: {}",
                            id, url
                        );
                        logger.warning(&warning_msg);
                    }
                }
            }
            
            // 应用目标更新
            for (id, idx_value) in target_indices {
                if let Some((success, failure, _, success_time, failure_time, network_error)) = target_updates.get(&id) {
                    let target_stat = &mut stats.targets[idx_value];
                    
                    target_stat.success += success;
                    target_stat.failure += failure;
                    
                    if let Some(time) = success_time {
                        target_stat.last_success_time = Some(*time);
                    }
                    
                    if let Some(time) = failure_time {
                        target_stat.last_failure_time = Some(*time);
                    }
                    
                    if let Some(err) = network_error {
                        target_stat.last_network_error = Some(err.clone());
                    } else if *failure > 0 && target_stat.last_network_error.is_some() {
                        // 只在非网络错误时清除
                        target_stat.last_network_error = None;
                    }
                    
                    // 更新错误率和濒死状态
                    let total = target_stat.success + target_stat.failure;
                    if total > 0 {
                        target_stat.error_rate = target_stat.failure as f64 / total as f64;
                        target_stat.is_dying = total > 20 && (target_stat.success as f64 / total as f64) < 0.1;
                    }
                }
            }
            
            // 应用线程更新
            let now = Instant::now();
            for (thread_id, request_count) in thread_updates {
                if let Some(idx) = self.thread_id_index_map.get(&thread_id) {
                    let idx_value = *idx.value();
                    if idx_value < stats.threads.len() {
                        let thread_stat = &mut stats.threads[idx_value];
                        thread_stat.requests += request_count;
                        thread_stat.last_active = now;
                    } else {
                        // 索引无效，添加新线程
                        let new_idx = stats.threads.len();
                        stats.threads.push(ThreadStats {
                            id: thread_id,
                            requests: request_count,
                            last_active: now,
                        });
                        self.thread_id_index_map.insert(thread_id, new_idx);
                    }
                } else {
                    // 添加新线程
                    let new_idx = stats.threads.len();
                    stats.threads.push(ThreadStats {
                        id: thread_id,
                        requests: request_count,
                        last_active: now,
                    });
                    self.thread_id_index_map.insert(thread_id, new_idx);
                }
            }
        }

        // 定期更新历史数据
        if self.last_stats_update_time.elapsed() >= self.stats_update_interval {
            // 更新RPS历史
            stats.rps_history.push_back(self.requests_in_last_second);
            while stats.rps_history.len() > HISTORY_CAPACITY {
                stats.rps_history.pop_front();
            }
            
            // 更新成功RPS历史
            stats.successful_requests_per_second_history.push_back(self.successes_in_last_second);
            while stats.successful_requests_per_second_history.len() > HISTORY_CAPACITY {
                stats.successful_requests_per_second_history.pop_front();
            }
            
            // 计算并更新成功率历史，使用整数计算避免浮点运算
            let current_success_rate = if self.requests_in_last_second > 0 {
                ((self.successes_in_last_second * 100) / self.requests_in_last_second).min(100)
            } else if stats.get_total() > 0 {
                ((stats.get_success() * 100) / stats.get_total()).min(100)
            } else {
                100 // 默认值为 100（如果没有请求）
            };
            
            stats.success_rate_history.push_back(current_success_rate);
            while stats.success_rate_history.len() > HISTORY_CAPACITY {
                stats.success_rate_history.pop_front();
            }
            
            // 重置计数器
            self.requests_in_last_second = 0;
            self.successes_in_last_second = 0;
            self.last_stats_update_time = Instant::now();
            needs_redraw_due_to_stats = true;
        }
        
        needs_redraw_due_to_stats
    }
}

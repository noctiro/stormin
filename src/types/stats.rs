use std::collections::VecDeque;
use std::thread;
use std::time::Instant;
use sysinfo::System;

use super::debug::DebugInfo;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunningState {
    Running,
    Paused,
    Stopping,
}

pub struct ThreadStats {
    pub id: thread::ThreadId,
    pub requests: u64,
    pub last_active: Instant,
}

pub struct TargetStats {
    pub id: usize,
    pub url: String,
    pub success: u64,
    pub failure: u64,
    pub last_success_time: Option<Instant>,
    pub last_failure_time: Option<Instant>,
    pub last_network_error: Option<String>,
    pub error_details: Vec<String>,
    pub error_rate: f64,
}

pub struct Stats {
    pub targets: Vec<TargetStats>,
    pub threads: Vec<ThreadStats>,
    pub success: std::sync::atomic::AtomicU64,
    pub failure: std::sync::atomic::AtomicU64,
    pub total: std::sync::atomic::AtomicU64,
    pub start_time: Instant,
    pub last_success_time: Option<Instant>,
    pub last_failure_time: Option<Instant>,
    pub sys: System,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub proxy_count: usize,
    pub running_state: RunningState,
    pub debug_logs: VecDeque<DebugInfo>,
    pub rps_history: VecDeque<u64>,
    pub successful_requests_per_second_history: VecDeque<u64>,
    pub success_rate_history: VecDeque<u64>,
}

impl Stats {
    pub fn get_success(&self) -> u64 {
        self.success.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn get_failure(&self) -> u64 {
        self.failure.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn get_total(&self) -> u64 {
        self.total.load(std::sync::atomic::Ordering::Relaxed)
    }
}

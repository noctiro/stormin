use reqwest::Method;
use std::thread::ThreadId;
use std::time::Instant;

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
    pub id: usize,
    pub url: String,
    pub success: bool,
    pub timestamp: Instant,
    pub debug: Option<String>,
    pub network_error: Option<String>,
    pub thread_id: ThreadId,
}

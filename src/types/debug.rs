use std::time::Instant;

#[derive(Clone, Debug)]
pub struct DebugInfo {
    pub timestamp: Instant,
    pub message: String,
}

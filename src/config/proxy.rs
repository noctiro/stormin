use reqwest::Proxy;
use serde::Deserialize;
use tokio::time::{Duration as TokioDuration, timeout};

use super::validator::ConfigError;

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub scheme: String,
    pub raw: String,        // 原始代理字符串，便于 fallback
    pub url_string: String, // 标准化代理URL字符串
}

impl ProxyConfig {
    pub fn parse(line: &str) -> Result<Self, ConfigError> {
        let line = line.trim();
        if line.is_empty() {
            return Err(ConfigError::ProxyParseError("Empty proxy line".into()));
        }

        // First try to parse as full URL format
        if let Ok(url) = url::Url::parse(line) {
            let scheme = url.scheme().to_string();
            let host = url
                .host_str()
                .ok_or_else(|| {
                    ConfigError::ProxyParseError(format!("Missing host in proxy URL: {}", line))
                })?
                .to_string();
            let port = url.port_or_known_default().ok_or_else(|| {
                ConfigError::ProxyParseError(format!("Missing port in proxy URL: {}", line))
            })?;
            let username = if !url.username().is_empty() {
                Some(url.username().to_string())
            } else {
                None
            };
            let password = url.password().map(|s| s.to_string());
            let url_string = match (&username, &password) {
                (Some(user), Some(pass)) => {
                    format!("{}://{}:{}@{}:{}", scheme, user, pass, host, port)
                }
                _ => format!("{}://{}:{}", scheme, host, port),
            };
            return Ok(ProxyConfig {
                scheme,
                raw: line.to_string(),
                url_string,
            });
        }

        // 处理简单格式: [user:pass@]host:port
        let (auth, host_port) = if let Some(at_pos) = line.find('@') {
            let auth_part = &line[..at_pos];
            let host_port_part = &line[at_pos + 1..];
            let auth_parts: Vec<&str> = auth_part.split(':').collect();
            if auth_parts.len() != 2 {
                (None, line)
            } else {
                (Some((auth_parts[0], auth_parts[1])), host_port_part)
            }
        } else {
            (None, line)
        };

        let host_port_parts: Vec<&str> = host_port.split(':').collect();
        if host_port_parts.len() != 2 {
            return Err(ConfigError::ProxyParseError(format!(
                "Invalid host:port format in proxy: {}",
                host_port
            )));
        }

        let host = host_port_parts[0].to_string();
        let port: u16 = host_port_parts[1].parse().map_err(|e| {
            ConfigError::ProxyParseError(format!(
                "Invalid port '{}' in proxy: {}",
                host_port_parts[1], e
            ))
        })?;
        let (username, password) = match auth {
            Some((user, pass)) => (Some(user.to_string()), Some(pass.to_string())),
            None => (None, None),
        };
        let scheme = "http".to_string(); // 默认使用HTTP协议
        let url_string = match (&username, &password) {
            (Some(user), Some(pass)) => format!("{}://{}:{}@{}:{}", scheme, user, pass, host, port),
            _ => format!("{}://{}:{}", scheme, host, port),
        };
        Ok(ProxyConfig {
            scheme,
            raw: line.to_string(),
            url_string,
        })
    }

    /// 返回标准代理URL字符串，parse时已生成，直接返回
    pub fn to_url_string(&self) -> &str {
        &self.url_string
    }

    /// 测试该代理的延迟，返回毫秒和详细错误。超时或失败返回Err(String)。
    pub async fn test_latency(&self, max_latency_ms: u64) -> Result<u128, String> {
        let timeout_duration = TokioDuration::from_millis(max_latency_ms);
        let proxy_url = self.to_url_string().to_string();
        let proxy = Proxy::all(proxy_url).map_err(|e| format!("Proxy::all error: {e}"))?;
        let client = reqwest::Client::builder()
            .proxy(proxy)
            .timeout(timeout_duration)
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|e| format!("Client build error: {e}"))?;

        let test_url = match self.scheme.as_str() {
            "https" => "https://cp.cloudflare.com/generate_204",
            _ => "http://cp.cloudflare.com/generate_204",
        };
        let start = std::time::Instant::now();
        // 优先用 HEAD，失败再用 GET
        let fut = client.head(test_url).header("User-Agent", "stormin").send();
        let head_result = match timeout(timeout_duration, fut).await {
            Ok(Ok(resp)) if resp.status().is_success() || resp.status().is_redirection() => {
                return Ok(start.elapsed().as_millis());
            }
            Ok(Ok(resp)) => Err(format!("HTTP status error: {}", resp.status())),
            Ok(Err(_)) => Err("HEAD request failed".to_string()),
            Err(_) => Err("Timeout on HEAD request".to_string()),
        };
        // HEAD 失败或超时，降级为 GET
        let fut = client.get(test_url).header("User-Agent", "stormin").send();
        match timeout(timeout_duration, fut).await {
            Ok(Ok(resp)) if resp.status().is_success() || resp.status().is_redirection() => {
                Ok(start.elapsed().as_millis())
            }
            Ok(Ok(resp)) => Err(format!("HTTP status error: {}", resp.status())),
            Ok(Err(_)) => Err("GET request failed".to_string()),
            Err(_) => head_result, // 返回HEAD的错误信息
        }
    }
}

// --- ProxyFileSource ---
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ProxyFileSource {
    Single(String),
    Multiple(Vec<String>),
}

impl ProxyFileSource {
    pub fn iter(&self) -> Vec<&str> {
        match self {
            ProxyFileSource::Single(s) => vec![s.as_str()],
            ProxyFileSource::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

use serde::Deserialize;

use super::validator::ConfigError;

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub raw: String, // 原始代理字符串，便于 fallback
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
            return Ok(ProxyConfig {
                scheme,
                host,
                port,
                username,
                password,
                raw: line.to_string(),
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

        Ok(ProxyConfig {
            scheme: "http".to_string(), // 默认使用HTTP协议
            host,
            port,
            username,
            password,
            raw: line.to_string(),
        })
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

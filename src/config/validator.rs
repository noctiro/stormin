use std::fmt;

use super::loader::Rule;

/// Configuration validation error type
#[derive(Debug)]
pub enum ConfigError {
    InvalidUrl(String),
    InvalidMethod(String),
    InvalidThreadCount,
    InvalidTimeoutValue,
    ProxyParseError(String),
    TemplateParseError(String),
    NoTargets,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidUrl(e) => write!(f, "Invalid URL: {}", e),
            ConfigError::InvalidMethod(m) => write!(f, "Invalid HTTP method: {}", m),
            ConfigError::InvalidThreadCount => write!(f, "Thread count must be at least 1"),
            ConfigError::InvalidTimeoutValue => write!(f, "Timeout must be a positive number"),
            ConfigError::ProxyParseError(e) => write!(f, "Invalid proxy configuration: {}", e),
            ConfigError::TemplateParseError(e) => write!(f, "Template parsing error: {}", e),
            ConfigError::NoTargets => write!(f, "No targets specified in configuration"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<pest::error::Error<Rule>> for ConfigError {
    fn from(e: pest::error::Error<Rule>) -> Self {
        ConfigError::TemplateParseError(e.to_string())
    }
}

/// 校验HTTP方法是否有效
pub fn is_valid_http_method(method: &str) -> bool {
    matches!(
        method.to_uppercase().as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS" | "PATCH" | "TRACE"
    )
}

/// 验证目标配置的合法性
pub fn validate_target(target: &crate::config::loader::RawTarget) -> Result<(), ConfigError> {
    // URL格式及协议校验
    let parsed_url = url::Url::parse(&target.url)
        .map_err(|e| ConfigError::InvalidUrl(format!("Invalid URL format: {}", e)))?;

    // 仅允许http/https协议
    // 增强协议校验
    let scheme = parsed_url.scheme().to_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(ConfigError::InvalidUrl(format!(
            "Unsupported protocol type: {}",
            parsed_url.scheme()
        )));
    }

    // 支持带方括号的域名/IP验证
    if let Some(domain) = parsed_url.host_str() {
        // 正则规则说明：
        let is_valid = regex::Regex::new(r"^\[*(?:(?:[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?|xn--[A-Za-z0-9-]+|[\p{L}\p{N}](?:[\p{L}\p{N}-]*[\p{L}\p{N}])?)(?:\.(?:[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?|xn--[A-Za-z0-9-]+|[\p{L}\p{N}](?:[\p{L}\p{N}-]*[\p{L}\p{N}])?))+|(?:\d{1,3}\.){3}\d{1,3}|(?:[0-9A-Fa-f]{1,4}:){7}[0-9A-Fa-f]{1,4}|(?:[0-9A-Fa-f]{1,4}:){1,7}:|:(?:[0-9A-Fa-f]{1,4}:){1,7}|::)\]*$").unwrap().is_match(domain);

        if !is_valid {
            return Err(ConfigError::InvalidUrl(format!(
                "Invalid domain or IP address: {}",
                domain
            )));
        }
    } else {
        return Err(ConfigError::InvalidUrl(
            "Missing a valid domain name".to_string(),
        ));
    }

    // 方法校验（可选字段，空时使用默认值GET）
    if let Some(method) = &target.method {
        if !is_valid_http_method(method) {
            return Err(ConfigError::InvalidMethod(method.clone()));
        }
    }

    Ok(())
}

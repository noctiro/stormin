use pest::Parser;
use pest_derive::Parser;
use serde::Deserialize;
use std::{error::Error, fs, num::NonZeroUsize, time::Duration};

use super::validator::ConfigError;

// --- Pest Parser Setup ---

#[derive(Parser)]
#[grammar = "template.pest"] // Path relative to src
struct TemplateParser;

// --- AST Definition ---

// Represents the parsed structure of a template string
#[derive(Clone, Debug, PartialEq)]
pub enum TemplateAstNode {
    Static(String),
    FunctionCall {
        def_name: Option<String>, // Optional name for variable definition
        name: String,
        args: Vec<TemplateAstNode>,
    },
    // Represents the top-level sequence of nodes in a template
    Root(Vec<TemplateAstNode>),
    TemplateString(Vec<TemplateAstNode>),
}

// --- Configuration Structs ---

#[derive(Debug, Clone, Deserialize)]
pub struct RawConfig {
    pub threads: Option<usize>,           // 攻击线程数
    pub generator_threads: Option<usize>, // 数据生成器线程数
    pub timeout: Option<u64>,
    pub proxy_file: Option<String>,
    // 新增的动态速率配置项
    pub target_rps: Option<f64>,
    pub min_success_rate: Option<f64>,            // 0.0 to 1.0
    pub rps_adjust_factor: Option<f64>,           // e.g., 0.1 for 10% adjustment per step
    pub success_rate_penalty_factor: Option<f64>, // e.g., 1.5 to multiply delay by 1.5
    // 生成器延迟控制配置项
    pub min_delay_micros: Option<u64>,     // 最小延迟 (微秒)
    pub max_delay_micros: Option<u64>,     // 最大延迟 (微秒)
    pub initial_delay_micros: Option<u64>, // 初始延迟 (微秒)
    pub increase_factor: Option<f64>,      // 延迟增加因子
    pub decrease_factor: Option<f64>,      // 延迟减少因子
    // Fields for CLI mode and general control
    pub cli_update_interval_secs: Option<u64>, // Interval for CLI stats printing
    pub start_paused: Option<bool>,            // Start in paused state
    pub run_duration: Option<String>,          // e.g., "10m", "1h30m", "30s"
    #[serde(rename = "Target")]
    pub targets: Vec<RawTarget>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawTarget {
    pub url: String,
    pub method: Option<String>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub params: Option<std::collections::HashMap<String, String>>,
}

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

#[derive(Clone, Debug)]
pub struct AttackConfig {
    pub threads: usize,
    pub generator_threads: usize,
    pub timeout: Duration,
    pub targets: Vec<CompiledTarget>,
    pub proxies: Vec<ProxyConfig>,
    // 数据生成器默认配置
    pub min_delay_micros: u64,     // 最小延迟 (微秒)
    pub max_delay_micros: u64,     // 最大延迟 (微秒)
    pub initial_delay_micros: u64, // 初始延迟 (微秒)
    pub increase_factor: f64,      // 延迟增加因子
    pub decrease_factor: f64,      // 延迟减少因子
    // 运行控制配置
    pub cli_update_interval_secs: u64,
    pub start_paused: bool,
    pub run_duration: Duration, // Changed from Option<Duration> to Duration with a default value
}

#[derive(Clone, Debug)]
pub struct CompiledTarget {
    pub id: usize, // Unique ID for the target
    pub url: String,
    pub method: reqwest::Method,
    pub headers: Vec<(String, TemplateAstNode)>, // Changed to support template AST
    pub params: Vec<(String, TemplateAstNode)>,
}

// --- Parsing Logic ---

// Parses a template string into an AST using Pest
fn parse_template_string(input: &str) -> Result<TemplateAstNode, ConfigError> {
    let pairs = TemplateParser::parse(Rule::template, input)
        .map_err(|e| ConfigError::TemplateParseError(e.to_string()))?;
    let top_pair = pairs
        .peek()
        .ok_or_else(|| ConfigError::TemplateParseError("Empty parse result".into()))?;
    build_ast_from_pair(top_pair)
        .map_err(|e| ConfigError::TemplateParseError(format!("Failed to build AST: {}", e)))
}

// Recursively builds the AST from Pest parse pairs
fn build_ast_from_pair(
    pair: pest::iterators::Pair<Rule>,
) -> Result<TemplateAstNode, pest::error::Error<Rule>> {
    match pair.as_rule() {
        Rule::template => Ok(TemplateAstNode::Root(
            pair.into_inner()
                .filter(|p| p.as_rule() != Rule::EOI)
                .map(build_ast_from_pair)
                .collect::<Result<Vec<TemplateAstNode>, pest::error::Error<Rule>>>()?,
        )),

        Rule::expression => {
            let mut inner_rules = pair.into_inner();
            let identifier_pair = inner_rules
                .next()
                .expect("Expression must have an identifier");
            let name = identifier_pair.as_str().to_string();

            let mut def_name: Option<String> = None;
            let mut args: Vec<TemplateAstNode> = Vec::new();

            if let Some(next_pair) = inner_rules.peek() {
                if next_pair.as_rule() == Rule::definition {
                    let def_pair = inner_rules.next().unwrap();
                    def_name = Some(def_pair.into_inner().next().unwrap().as_str().to_string());
                }
            }

            if let Some(args_pair) = inner_rules.next() {
                if args_pair.as_rule() == Rule::arguments {
                    args = args_pair
                        .into_inner()
                        .map(build_ast_from_pair)
                        .collect::<Result<_, _>>()?;
                }
            }

            Ok(TemplateAstNode::FunctionCall {
                def_name,
                name,
                args,
            })
        }

        Rule::argument => build_ast_from_pair(pair.into_inner().next().unwrap()),

        Rule::string_literal => {
            let literal_str = pair.as_str();
            let content = if literal_str.len() >= 2 {
                &literal_str[1..literal_str.len() - 1]
            } else {
                ""
            };
            let unescaped = content.replace("\\\"", "\"").replace("\\\\", "\\");
            Ok(TemplateAstNode::Static(unescaped))
        }

        Rule::static_text => Ok(TemplateAstNode::Static(pair.as_str().to_string())),
        Rule::number => Ok(TemplateAstNode::Static(pair.as_str().to_string())),
        Rule::identifier => Ok(TemplateAstNode::Static(pair.as_str().to_string())),

        Rule::template_string => {
            let children: Vec<TemplateAstNode> =
                pair.into_inner()
                    .map(build_ast_from_pair)
                    .collect::<Result<Vec<TemplateAstNode>, pest::error::Error<Rule>>>()?;
            Ok(TemplateAstNode::TemplateString(children))
        }

        Rule::template_string_literal => Ok(TemplateAstNode::Static(pair.as_str().to_string())),

        _ => unreachable!(
            "Unexpected rule: {:?} in build_ast_from_pair",
            pair.as_rule()
        ),
    }
}

// Helper function to parse duration string (e.g., "10s", "5m", "1h")
fn parse_duration_str(duration_str: &str) -> Result<Duration, ConfigError> {
    let duration_str = duration_str.trim();
    if duration_str.is_empty() {
        return Err(ConfigError::InvalidDurationFormat(
            "Duration string is empty".to_string(),
        ));
    }

    let mut total_secs = 0u64;
    let mut current_num_str = String::new();

    for ch in duration_str.chars() {
        if ch.is_ascii_digit() {
            current_num_str.push(ch);
        } else {
            if current_num_str.is_empty() && !"smh".contains(ch) {
                return Err(ConfigError::InvalidDurationFormat(format!(
                    "Invalid character in duration string: {}",
                    ch
                )));
            }
            let num = if current_num_str.is_empty() {
                1
            } else {
                current_num_str.parse::<u64>().map_err(|_| {
                    ConfigError::InvalidDurationFormat(format!(
                        "Invalid number in duration string: {}",
                        current_num_str
                    ))
                })?
            };
            current_num_str.clear();

            match ch {
                's' => total_secs += num,
                'm' => total_secs += num * 60,
                'h' => total_secs += num * 60 * 60,
                _ => {
                    return Err(ConfigError::InvalidDurationFormat(format!(
                        "Invalid unit in duration string: {}",
                        ch
                    )));
                }
            }
        }
    }
    if !current_num_str.is_empty() {
        let num = current_num_str.parse::<u64>().map_err(|_| {
            ConfigError::InvalidDurationFormat(format!(
                "Invalid trailing number in duration string: {}",
                current_num_str
            ))
        })?;
        total_secs += num;
    }

    if total_secs == 0 && !duration_str.contains('0') {
        return Err(ConfigError::InvalidDurationFormat(
            "Duration cannot be zero unless explicitly stated as '0s', '0m', etc.".to_string(),
        ));
    }

    Ok(Duration::from_secs(total_secs))
}

/// Loads configuration and compiles all targets
pub fn load_config_and_compile(path: &str) -> Result<AttackConfig, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let raw: RawConfig = toml::from_str(&content)?;

    super::validator::validate_rate_control_config(&raw)
        .map_err(|e| Box::new(e) as Box<dyn Error>)?;

    let builtin_functions = crate::template::get_builtin_function_names();

    if raw.targets.is_empty() {
        return Err(ConfigError::NoTargets.into());
    }

    let mut proxies = Vec::new();
    if let Some(proxy_path_str) = &raw.proxy_file {
        if !proxy_path_str.trim().is_empty() {
            let proxy_path = std::path::Path::new(proxy_path_str);
            if proxy_path.exists() {
                let proxy_content = fs::read_to_string(proxy_path)?;
                for line in proxy_content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    match ProxyConfig::parse(line) {
                        Ok(proxy) => proxies.push(proxy),
                        Err(e) => eprintln!("Invalid proxy configuration '{}': {}", line, e),
                    }
                }
            } else {
                eprintln!(
                    "Warning: proxy_file '{}' not found, ignoring.",
                    proxy_path_str
                );
            }
        }
    }

    // 计算线程数，默认是系统可用线程数 * 16
    let threads = if let Some(t) = raw.threads {
        if t < 1 {
            return Err(ConfigError::InvalidThreadCount.into());
        }
        t
    } else {
        std::thread::available_parallelism()
            .unwrap_or(NonZeroUsize::new(1).unwrap())
            .get()
            * 16
    };

    // 计算数据生成器线程数，默认是 threads/512，最小为 1
    let generator_threads = match raw.generator_threads {
        Some(g) => {
            if g < 1 {
                return Err(ConfigError::InvalidGeneratorThreadCount.into());
            }
            g
        }
        None => (threads / 512).max(1),
    };

    let timeout = if let Some(t) = raw.timeout {
        if t <= 0 {
            return Err(ConfigError::InvalidTimeoutValue.into());
        }
        t
    } else {
        5 // Default timeout 5 seconds
    };

    let mut compiled: Vec<CompiledTarget> = Vec::new();
    let mut target_id_counter = 0;
    'target_loop: for raw_t in raw.targets {
        if let Err(e) = super::validator::validate_target(&raw_t) {
            eprintln!(
                "[Configuration verification failed] Target '{}' was removed: {}",
                raw_t.url, e
            );
            continue;
        }

        let mut parsed_params = Vec::new();
        let mut parsed_headers = Vec::new();
        let mut all_parsed_templates: Vec<(String, TemplateAstNode)> = Vec::new();
        let mut target_has_error = false;

        if let Some(map) = raw_t.params {
            for (k, v) in map {
                match parse_template_string(&v) {
                    Ok(ast_node) => {
                        parsed_params.push((k.clone(), ast_node.clone()));
                        all_parsed_templates.push((k.clone(), ast_node));
                    }
                    Err(e) => {
                        eprintln!(
                            "[Configuration verification failed] Target '{}', Param '{}': Failed to parse template: {}",
                            raw_t.url, k, e
                        );
                        target_has_error = true;
                        break;
                    }
                }
            }
        }
        if target_has_error {
            eprintln!(
                "[Configuration verification failed] Skipping Target '{}' due to param parsing errors.",
                raw_t.url
            );
            continue 'target_loop;
        }

        if let Some(map) = raw_t.headers {
            for (k, v) in map {
                match parse_template_string(&v) {
                    Ok(ast_node) => {
                        parsed_headers.push((k.clone(), ast_node.clone()));
                        all_parsed_templates.push((k.clone(), ast_node));
                    }
                    Err(e) => {
                        eprintln!(
                            "[Configuration verification failed] Target '{}', Header '{}': Failed to parse template: {}",
                            raw_t.url, k, e
                        );
                        target_has_error = true;
                        break;
                    }
                }
            }
        }
        if target_has_error {
            eprintln!(
                "[Configuration verification failed] Skipping Target '{}' due to header parsing errors.",
                raw_t.url
            );
            continue 'target_loop;
        }

        all_parsed_templates.sort_by_key(|(_, node)| match node {
            TemplateAstNode::FunctionCall { def_name, .. } if def_name.is_some() => 0,
            _ => 1,
        });

        if let Err(e) =
            super::validator::validate_target_templates(&all_parsed_templates, &builtin_functions)
        {
            eprintln!(
                "[Configuration verification failed] Target '{}': {}",
                raw_t.url, e
            );
            target_has_error = true;
        }

        if target_has_error {
            eprintln!(
                "[Configuration verification failed] Skipping Target '{}' due to template validation errors.",
                raw_t.url
            );
            continue 'target_loop;
        }

        let method = match raw_t
            .method
            .as_deref()
            .unwrap_or("GET")
            .to_uppercase()
            .as_str()
        {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            "PATCH" => reqwest::Method::PATCH,
            "TRACE" => reqwest::Method::TRACE,
            m => {
                eprintln!(
                    "Skipping invalid target '{}': Invalid HTTP method {}",
                    raw_t.url, m
                );
                continue;
            }
        };

        compiled.push(CompiledTarget {
            id: target_id_counter,
            url: raw_t.url,
            method,
            headers: parsed_headers,
            params: parsed_params,
        });
        target_id_counter += 1;
    }

    if compiled.is_empty() {
        return Err(ConfigError::NoTargets.into());
    }

    let run_duration = match raw.run_duration {
        Some(duration_str) => match parse_duration_str(&duration_str) {
            Ok(d) => d,
            Err(e) => return Err(Box::new(e) as Box<dyn Error>),
        },
        None => Duration::from_secs(0),
    };

    Ok(AttackConfig {
        threads,
        timeout: Duration::from_secs(timeout),
        targets: compiled,
        proxies,
        generator_threads,
        min_delay_micros: raw.min_delay_micros.unwrap_or(1000), // 默认1ms最小延迟
        max_delay_micros: raw.max_delay_micros.unwrap_or(100_000), // 默认100ms最大延迟
        initial_delay_micros: raw.initial_delay_micros.unwrap_or(5000), // 默认5ms初始延迟
        increase_factor: raw.increase_factor.unwrap_or(1.2),    // 默认20%增长
        decrease_factor: raw.decrease_factor.unwrap_or(0.85),   // 默认15%减少
        cli_update_interval_secs: raw.cli_update_interval_secs.unwrap_or(2),
        start_paused: raw.start_paused.unwrap_or(false),
        run_duration,
    })
}

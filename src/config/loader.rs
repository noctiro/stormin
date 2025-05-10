use pest::Parser;
use pest_derive::Parser;
use serde::Deserialize;
use std::{error::Error, fs, num::NonZeroUsize};

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
        def_name: Option<String>, // Added: Optional name for variable definition
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
    pub proxy_file: Option<String>,
    pub threads: Option<usize>,
    pub timeout: Option<u64>,
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
            let host = url.host_str().ok_or_else(|| {
                ConfigError::ProxyParseError(format!("Missing host in proxy URL: {}", line))
            })?.to_string();
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
            return Err(ConfigError::ProxyParseError(
                format!("Invalid host:port format in proxy: {}", host_port)
            ));
        }

        let host = host_port_parts[0].to_string();
        let port: u16 = host_port_parts[1].parse().map_err(|e| {
            ConfigError::ProxyParseError(
                format!("Invalid port '{}' in proxy: {}", host_port_parts[1], e)
            )
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
    pub timeout: u64,
    pub targets: Vec<CompiledTarget>,
    pub proxies: Vec<ProxyConfig>, // 类型改为 ProxyConfig
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
    // Need to specify the Rule type for the parser
    let pairs = TemplateParser::parse(Rule::template, input)
        .map_err(|e| ConfigError::TemplateParseError(e.to_string()))?;
    // We expect a single 'template' rule match at the top level
    let top_pair = pairs.peek().ok_or_else(|| {
        ConfigError::TemplateParseError("Empty parse result".into())
    })?;
    build_ast_from_pair(top_pair).map_err(|e| {
        ConfigError::TemplateParseError(format!("Failed to build AST: {}", e))
    })
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
            let identifier_pair = inner_rules.next().expect("Expression must have an identifier");
            let name = identifier_pair.as_str().to_string();

            let mut def_name: Option<String> = None;
            let mut args: Vec<TemplateAstNode> = Vec::new();

            // Check for optional definition first
            if let Some(next_pair) = inner_rules.peek() {
                if next_pair.as_rule() == Rule::definition {
                    let def_pair = inner_rules.next().unwrap();
                    // The definition rule contains the identifier for the name
                    def_name = Some(def_pair.into_inner().next().unwrap().as_str().to_string());
                }
            }

            // Check for optional arguments
            if let Some(args_pair) = inner_rules.next() {
                 if args_pair.as_rule() == Rule::arguments {
                     args = args_pair.into_inner().map(build_ast_from_pair).collect::<Result<_,_>>()?;
                 }
            }

            // Create the node, including the optional definition name
                 Ok(TemplateAstNode::FunctionCall {
                     def_name,
                     name,
                     args, // Use parsed args
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
            // Process backtick template string with potential nested expressions
            let children: Vec<TemplateAstNode> =
                pair.into_inner()
                    .map(build_ast_from_pair)
                    .collect::<Result<Vec<TemplateAstNode>, pest::error::Error<Rule>>>()?;

            Ok(TemplateAstNode::TemplateString(children))
        }

        Rule::template_string_literal => {
            // Handle literal text parts in template strings
            Ok(TemplateAstNode::Static(pair.as_str().to_string()))
        }

        _ => unreachable!(
            "Unexpected rule: {:?} in build_ast_from_pair",
            pair.as_rule()
        ),
    }
}

/// Loads configuration and compiles all targets
pub fn load_config_and_compile(path: &str) -> Result<AttackConfig, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let raw: RawConfig = toml::from_str(&content)?;

    // Get the set of built-in function names from the template module
    let builtin_functions = crate::template::get_builtin_function_names();

    // 检查是否存在目标配置
    if raw.targets.is_empty() {
        return Err(ConfigError::NoTargets.into());
    }

    // 读取代理文件并筛选
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

    // 处理线程数，默认为 CPU 核数 * 4
    let threads = if let Some(t) = raw.threads {
        if t < 1 {
            return Err(ConfigError::InvalidThreadCount.into());
        }
        t
    } else {
        std::thread::available_parallelism()
            .unwrap_or(NonZeroUsize::new(1).unwrap())
            .get()
            * 4
    };

    let timeout = if let Some(t) = raw.timeout {
        if t <= 0 {
            return Err(ConfigError::InvalidTimeoutValue.into());
        }
        t
    } else {
        5
    };

    let mut compiled: Vec<CompiledTarget> = Vec::new();
    let mut target_id_counter = 0; // Counter for generating unique IDs
    'target_loop: for raw_t in raw.targets { // Add a label to the outer loop
        // 使用更严格的验证逻辑
        if let Err(e) = super::validator::validate_target(&raw_t) {
            eprintln!("[Configuration verification failed] Target '{}' was removed: {}", raw_t.url, e);
            continue;
        }

        let mut parsed_params = Vec::new(); // Temporary storage for parsed params
        let mut parsed_headers = Vec::new(); // Temporary storage for parsed headers
        let mut target_has_error = false; // Flag for validation errors

        // Parse and validate params
        if let Some(map) = raw_t.params {
            for (k, v) in map {
                match parse_template_string(&v) {
                    Ok(ast_node) => {
                        parsed_params.push((k.clone(), ast_node));
                    }
                    Err(e) => {
                        eprintln!("[Configuration verification failed] Target '{}', Param '{}': Failed to parse template: {}", raw_t.url, k, e);
                        target_has_error = true;
                        break;
                    }
                };
            }
        }
        if !target_has_error {
            parsed_params.sort_by_key(|(_, node)| {
                match node {
                    TemplateAstNode::FunctionCall { def_name, .. } if def_name.is_some() => 0,
                    _ => 1,
                }
            });
            if let Err(e) = super::validator::validate_target_templates(&parsed_params, &builtin_functions) {
                eprintln!("[Configuration verification failed] Target '{}' (Params): {}", raw_t.url, e);
                target_has_error = true;
            }
        }

        // Parse and validate headers
        if !target_has_error { // Only proceed if params were okay
            if let Some(map) = raw_t.headers {
                for (k, v) in map {
                    match parse_template_string(&v) {
                        Ok(ast_node) => {
                            parsed_headers.push((k.clone(), ast_node));
                        }
                        Err(e) => {
                            eprintln!("[Configuration verification failed] Target '{}', Header '{}': Failed to parse template: {}", raw_t.url, k, e);
                            target_has_error = true;
                            break;
                        }
                    };
                }
            }
            if !target_has_error { // Only sort and validate if parsing was okay
                // Headers generally don't have the same definition-use dependency as params,
                // but sorting can be kept for consistency if complex header templates arise.
                // For now, simple sort by key might be sufficient or no sort.
                // parsed_headers.sort_by_key(|(k, _)| k.clone()); // Example: sort by key

                // Validate header templates
                if let Err(e) = super::validator::validate_target_templates(&parsed_headers, &builtin_functions) {
                    eprintln!("[Configuration verification failed] Target '{}' (Headers): {}", raw_t.url, e);
                    target_has_error = true;
                }
            }
        }

        // If any parsing or validation error occurred, skip this target
        if target_has_error {
            eprintln!("[Configuration verification failed] Skipping Target '{}' due to errors.", raw_t.url);
            continue 'target_loop; // Continue to the next target
        }

        // If everything is valid, proceed
        let method = match raw_t.method.as_deref().unwrap_or("GET").to_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            "PATCH" => reqwest::Method::PATCH,
            "TRACE" => reqwest::Method::TRACE,
            m => {
                eprintln!("Skipping invalid target '{}': Invalid HTTP method {}", raw_t.url, m);
                continue;
            }
        };

        compiled.push(CompiledTarget {
            id: target_id_counter,
            url: raw_t.url,
            method,
            headers: parsed_headers, // Use the validated parsed_headers
            params: parsed_params,
        });
        target_id_counter += 1;
    }

    // Check again after validation filtering
    if compiled.is_empty() {
        return Err(ConfigError::NoTargets.into());
    }

    Ok(AttackConfig {
        threads,
        timeout,
        targets: compiled,
        proxies,
    })
}



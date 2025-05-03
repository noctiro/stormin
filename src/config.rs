use pest::Parser;
use pest_derive::Parser;
use serde::Deserialize;
use std::{error::Error, fs, num::NonZeroUsize};

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
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        // 首先尝试解析为完整URL格式
        if let Ok(url) = url::Url::parse(line) {
            let scheme = url.scheme().to_string();
            let host = url.host_str()?.to_string();
            let port = url.port_or_known_default()?;
            let username = if !url.username().is_empty() {
                Some(url.username().to_string())
            } else {
                None
            };
            let password = url.password().map(|s| s.to_string());
            return Some(ProxyConfig {
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
            return None;
        }

        let host = host_port_parts[0].to_string();
        let port: u16 = host_port_parts[1].parse().ok()?;
        let (username, password) = match auth {
            Some((user, pass)) => (Some(user.to_string()), Some(pass.to_string())),
            None => (None, None),
        };

        Some(ProxyConfig {
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
    pub url: String,
    pub method: String,
    pub headers: std::collections::HashMap<String, String>,
    pub params: Vec<(String, TemplateAstNode)>,
}

// --- Parsing Logic ---

// Parses a template string into an AST using Pest
fn parse_template_string(input: &str) -> Result<TemplateAstNode, pest::error::Error<Rule>> {
    // Need to specify the Rule type for the parser
    let pairs = TemplateParser::parse(Rule::template, input)?;
    // We expect a single 'template' rule match at the top level
    let top_pair = pairs.peek().ok_or_else(|| {
        // Create a custom error if parsing returns no pairs (shouldn't happen on success)
        pest::error::Error::new_from_span(
            pest::error::ErrorVariant::CustomError {
                message: "Empty parse result".to_string(),
            },
            pest::Span::new(input, 0, 0).unwrap(), // Span covering the beginning
        )
    })?;
    Ok(build_ast_from_pair(top_pair)?)
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
            let identifier_pair = inner_rules.next().unwrap(); // function or variable name
            let name = identifier_pair.as_str().to_string();

            if let Some(args_pair) = inner_rules.next() {
                let args = args_pair
                    .into_inner()
                    .map(build_ast_from_pair)
                    .collect::<Result<Vec<TemplateAstNode>, pest::error::Error<Rule>>>()?;
                Ok(TemplateAstNode::FunctionCall { name, args })
            } else {
                // Support identifier without arguments: treat as function call with empty arguments
                Ok(TemplateAstNode::FunctionCall {
                    name,
                    args: Vec::new(),
                })
            }
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
                    if let Some(proxy) = ProxyConfig::parse(line) {
                        proxies.push(proxy);
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
            return Err("Config error: threads must be at least 1".into());
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
            return Err("Config error: timeout must be at least 0".into());
        }
        t
    } else {
        5
    };

    let mut compiled: Vec<CompiledTarget> = Vec::new();
    for raw_t in raw.targets {
        let mut params = Vec::new();
        if let Some(map) = raw_t.params {
            for (k, v) in map {
                params.push((k, parse_template_string(&v)?));
            }
        }
        compiled.push(CompiledTarget {
            url: raw_t.url,
            method: raw_t.method.unwrap_or_else(|| "GET".into()),
            headers: raw_t.headers.unwrap_or_default(),
            params,
        });
    }

    Ok(AttackConfig {
        threads,
        timeout,
        targets: compiled,
        proxies,
    })
}

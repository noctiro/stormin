use pest::Parser;
use pest_derive::Parser;
use serde::Deserialize;
use std::{error::Error, fs};

// --- Pest Parser Setup ---

#[derive(Parser)]
#[grammar = "template.pest"] // Path relative to src
struct TemplateParser;

// --- AST Definition ---

// Represents the parsed structure of a template string
#[derive(Clone, Debug, PartialEq)]
pub enum TemplateAstNode {
    Static(String),
    Variable(String), // e.g., "user", "password", "qqid"
    FunctionCall {
        name: String,
        args: Vec<TemplateAstNode>,
    },
    // Represents the top-level sequence of nodes in a template
    Root(Vec<TemplateAstNode>),
}

// --- Configuration Structs ---

#[derive(Debug, Clone, Deserialize)]
pub struct RawConfig {
    pub proxy_file: String,
    pub threads: Option<usize>,
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
    pub targets: Vec<CompiledTarget>,
    pub proxies: Vec<ProxyConfig>, // 类型改为 ProxyConfig
}

#[derive(Clone, Debug)]
pub struct CompiledTarget {
    pub url: String,
    pub method: String,
    pub headers: std::collections::HashMap<String, String>,
    pub params: Vec<(String, CompiledUrl)>,
}

// Updated CompiledUrl to store the AST
#[derive(Clone, Debug)]
pub struct CompiledUrl {
    pub ast: TemplateAstNode, // Changed from Vec<UrlPart>
    pub needs_user: bool,
    pub needs_password: bool,
    pub needs_qqid: bool,
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
    Ok(build_ast_from_pair(top_pair))
}

// Recursively builds the AST from Pest parse pairs
fn build_ast_from_pair(pair: pest::iterators::Pair<Rule>) -> TemplateAstNode {
    match pair.as_rule() {
        Rule::template => {
            // The children of 'template' are the sequence of static text and expressions, plus EOI.
            // Filter out the EOI rule before mapping.
            TemplateAstNode::Root(
                pair.into_inner()
                    .filter(|p| p.as_rule() != Rule::EOI) // Ignore EOI
                    .map(build_ast_from_pair)
                    .collect(),
            )
        }
        Rule::expression => {
            let mut inner_rules = pair.into_inner();
            let identifier_pair = inner_rules.next().unwrap(); // identifier is guaranteed
            let name = identifier_pair.as_str().to_string();

            if let Some(args_pair) = inner_rules.next() {
                // Optional arguments part
                // It's a function call
                let args = args_pair
                    .into_inner() // Go into 'arguments' rule -> 'argument'*
                    .map(build_ast_from_pair) // Process each 'argument'
                    .collect();
                TemplateAstNode::FunctionCall { name, args }
            } else {
                // No arguments provided after identifier
                // Treat user, password, qqid as parameter-less functions
                match name.as_str() {
                    "user" | "password" | "qqid" => TemplateAstNode::FunctionCall {
                        name,
                        args: Vec::new(), // Empty argument list
                    },
                    // Treat other identifiers without args as variables (or potentially error?)
                    _ => TemplateAstNode::Variable(name),
                }
            }
        }
        Rule::argument => {
            // An argument is either a string literal or a nested expression
            // There will always be exactly one inner element for 'argument'
            build_ast_from_pair(pair.into_inner().next().unwrap())
        }
        Rule::string_literal => {
            // Atomic rule: content is in pair.as_str() including quotes.
            let literal_str = pair.as_str();
            // Remove surrounding quotes. Check length to prevent panic on empty/invalid literal.
            let content = if literal_str.len() >= 2 {
                &literal_str[1..literal_str.len() - 1]
            } else {
                "" // Or handle error appropriately
            };
            // Handle escapes
            let unescaped = content.replace("\\\"", "\"").replace("\\\\", "\\");
            TemplateAstNode::Static(unescaped)
        }
        Rule::static_text => TemplateAstNode::Static(pair.as_str().to_string()),
        // Rules like WHITESPACE, identifier, arguments, inner_string, escape_sequence, EOI, etc.
        // are structural or atomic and don't directly map to AST nodes in this structure.
        // We only handle the rules that define the structure we want in the AST.
        _ => unreachable!(
            "Encountered unexpected rule: {:?} in build_ast_from_pair",
            pair.as_rule()
        ),
    }
}

// Helper to determine required variables from the AST
fn analyze_ast_needs(node: &TemplateAstNode) -> (bool, bool, bool) {
    match node {
        TemplateAstNode::Static(_) => (false, false, false),
        TemplateAstNode::Variable(_) => (false, false, false), // Basic variables don't trigger flags anymore
        TemplateAstNode::FunctionCall { name, args, .. } => {
            // Check the function name itself
            let self_needs = match name.as_str() {
                "user" => (true, false, false),
                "password" => (false, true, false),
                "qqid" => (false, false, true),
                _ => (false, false, false),
            };
            // Combine with needs from arguments
            args.iter()
                .map(analyze_ast_needs)
                .fold(self_needs, |acc, needs| {
                    (acc.0 || needs.0, acc.1 || needs.1, acc.2 || needs.2)
                })
        }
        TemplateAstNode::Root(nodes) => nodes
            .iter()
            .map(analyze_ast_needs)
            .fold((false, false, false), |acc, needs| {
                (acc.0 || needs.0, acc.1 || needs.1, acc.2 || needs.2)
            }),
    }
}

// Updated function to use the new parser
// Note the change in the Error type in the Result
fn compile_url_template(template: String) -> Result<CompiledUrl, pest::error::Error<Rule>> {
    let ast = parse_template_string(&template)?;
    let (needs_user, needs_password, needs_qqid) = analyze_ast_needs(&ast);
    Ok(CompiledUrl {
        ast, // Store the AST directly
        needs_user,
        needs_password,
        needs_qqid,
    })
}

/// Loads configuration and compiles all targets
pub fn load_config_and_compile(path: &str) -> Result<AttackConfig, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let raw: RawConfig = toml::from_str(&content)?;

    // 读取代理文件并筛选
    let mut proxies = Vec::new();
    if !raw.proxy_file.is_empty() && std::path::Path::new(&raw.proxy_file).exists() {
        let proxy_content = fs::read_to_string(&raw.proxy_file)?;
        for line in proxy_content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(proxy) = ProxyConfig::parse(line) {
                proxies.push(proxy);
            }
        }
    }

    let mut compiled = Vec::new();
    for raw_t in raw.targets {
        let mut params = Vec::new();
        if let Some(map) = raw_t.params {
            for (k, v) in map {
                params.push((k, compile_url_template(v)?));
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
        threads: raw.threads.unwrap_or(1),
        targets: compiled,
        proxies,
    })
}

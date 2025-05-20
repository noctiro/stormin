use std::collections::HashSet;
use std::fmt; // Import HashSet

use super::loader::{Rule, TemplateAstNode}; // Import TemplateAstNode

/// Configuration validation error type
#[derive(Debug)]
pub enum ConfigError {
    InvalidUrl(String),
    InvalidMethod(String),
    InvalidThreadCount,
    InvalidTimeoutValue,
    InvalidGeneratorThreadCount,
    ProxyParseError(String),
    TemplateParseError(String),
    NoTargets,
    // variable
    DuplicateVariableDefinition(String), // Duplicate variable name
    CircularVariableDependency(String),  // Circular dependency detected
    UndefinedVariableReference(String),  // Variable reference not defined
    // New errors for rate control parameters
    InvalidTargetRps(String),
    InvalidMinSuccessRate(String),
    InvalidRpsAdjustFactor(String),
    InvalidSuccessRatePenaltyFactor(String),
    InvalidDurationFormat(String), // Added for run_duration parsing
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidUrl(e) => write!(f, "Invalid URL: {}", e),
            ConfigError::InvalidMethod(m) => write!(f, "Invalid HTTP method: {}", m),
            ConfigError::InvalidThreadCount => write!(f, "Thread count must be at least 1"),
            ConfigError::InvalidTimeoutValue => write!(f, "Timeout must be a positive number"),
            ConfigError::InvalidGeneratorThreadCount => {
                write!(f, "Generator thread count must be at least 1")
            }
            ConfigError::ProxyParseError(e) => write!(f, "Invalid proxy configuration: {}", e),
            ConfigError::TemplateParseError(e) => write!(f, "Template parsing error: {}", e),
            ConfigError::NoTargets => write!(f, "No targets specified in configuration"),
            ConfigError::DuplicateVariableDefinition(name) => {
                write!(f, "Duplicate variable definition: '{}'", name)
            }
            ConfigError::CircularVariableDependency(path) => {
                write!(f, "Circular variable dependency detected: {}", path)
            }
            ConfigError::UndefinedVariableReference(name) => {
                write!(f, "Undefined variable reference: '{}'", name)
            }
            ConfigError::InvalidTargetRps(value) => {
                write!(
                    f,
                    "Invalid target_rps value: '{}'. Must be a positive number.",
                    value
                )
            }
            ConfigError::InvalidMinSuccessRate(value) => {
                write!(
                    f,
                    "Invalid min_success_rate value: '{}'. Must be between 0.0 and 1.0.",
                    value
                )
            }
            ConfigError::InvalidRpsAdjustFactor(value) => {
                write!(
                    f,
                    "Invalid rps_adjust_factor value: '{}'. Must be a positive number.",
                    value
                )
            }
            ConfigError::InvalidSuccessRatePenaltyFactor(value) => {
                write!(
                    f,
                    "Invalid success_rate_penalty_factor value: '{}'. Must be >= 1.0.",
                    value
                )
            }
            ConfigError::InvalidDurationFormat(e) => {
                write!(
                    f,
                    "Invalid duration format: {}. Expected format like '10s', '5m', '1h30m'.",
                    e
                )
            }
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

/// 验证目标配置的合法性 (基础验证)
pub fn validate_target(target: &crate::config::loader::RawTarget) -> Result<(), ConfigError> {
    // URL格式及协议校验
    let parsed_url = url::Url::parse(&target.url)
        .map_err(|e| ConfigError::InvalidUrl(format!("Invalid URL format: {}", e)))?;

    // 仅允许http/https协议
    let scheme = parsed_url.scheme().to_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(ConfigError::InvalidUrl(format!(
            "Unsupported protocol type: {}",
            parsed_url.scheme()
        )));
    }

    // 支持带方括号的域名/IP验证
    if let Some(domain) = parsed_url.host_str() {
        // Regex for domain/IP validation (simplified example)
        // A more robust validation might be needed depending on requirements
        if !domain.contains('.') && !domain.contains(':') && domain != "localhost" {
            // Basic check, might need refinement
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

// --- AST Validation Logic ---

#[derive(Default)] // Add Default derive
struct ValidationContext {
    defined_vars: HashSet<String>,
    visiting_vars: HashSet<String>, // For cycle detection
    current_path: Vec<String>,      // For cycle detection path reporting
}

/// Validates all template ASTs within a single target for consistency.
pub fn validate_target_templates(
    templates: &[(String, TemplateAstNode)], // Combined list of templates (params and headers) for the target
    builtin_functions: &HashSet<String>,
) -> Result<(), ConfigError> {
    let mut context = ValidationContext::default();

    // First pass: Collect all definitions across all templates in this target
    for (_, ast_node) in templates {
        collect_definitions(ast_node, &mut context.defined_vars)?;
    }

    // Second pass: Validate references and cycles for each template AST using the collected context
    for (_, ast_node) in templates {
        // Reset cycle detection state for each independent AST validation run
        context.visiting_vars.clear();
        context.current_path.clear();
        validate_references_and_cycles(ast_node, &mut context, builtin_functions)?;
    }

    Ok(())
}

// --- Internal Helper Functions ---

// Collects all variable definitions from a single AST node recursively.
// Detects duplicate definitions within the scope of this collection run.
fn collect_definitions(
    node: &TemplateAstNode,
    defined_vars: &mut HashSet<String>,
) -> Result<(), ConfigError> {
    match node {
        TemplateAstNode::FunctionCall { def_name, args, .. } => {
            if let Some(d_name) = def_name {
                if !defined_vars.insert(d_name.clone()) {
                    return Err(ConfigError::DuplicateVariableDefinition(d_name.clone()));
                }
            }
            for arg in args {
                collect_definitions(arg, defined_vars)?;
            }
        }
        TemplateAstNode::Root(nodes) | TemplateAstNode::TemplateString(nodes) => {
            for n in nodes {
                collect_definitions(n, defined_vars)?;
            }
        }
        TemplateAstNode::Static(_) => {} // Static text doesn't have definitions
    }
    Ok(())
}

// Validates variable references and detects circular dependencies within a single AST node recursively.
fn validate_references_and_cycles(
    node: &TemplateAstNode,
    context: &mut ValidationContext,
    builtin_functions: &HashSet<String>,
) -> Result<(), ConfigError> {
    match node {
        TemplateAstNode::FunctionCall {
            def_name,
            name,
            args,
        } => {
            // Check if it's a known built-in function first
            let is_builtin = builtin_functions.contains(name);
            let is_variable_reference = args.is_empty() && def_name.is_none();

            if is_variable_reference {
                // Now check if the reference is NOT a built-in
                if !is_builtin {
                    // If not built-in, treat as variable reference
                    if !context.defined_vars.contains(name) {
                        return Err(ConfigError::UndefinedVariableReference(name.clone()));
                    }
                    // Cycle detection check
                    if context.visiting_vars.contains(name) {
                        let path = context.current_path.join(" -> ") + " -> " + name;
                        return Err(ConfigError::CircularVariableDependency(path));
                    }
                }
            }

            // Add current node to path for cycle detection if it's a definition or reference being resolved
            let added_to_path = if let Some(d_name) = def_name {
                context.current_path.push(d_name.clone());
                context.visiting_vars.insert(d_name.clone());
                true
            } else if is_variable_reference && !is_builtin {
                // is_builtin is now in scope here
                context.current_path.push(name.clone());
                context.visiting_vars.insert(name.clone());
                true
            } else {
                false
            };

            // Recursively validate arguments
            for arg in args {
                validate_references_and_cycles(arg, context, builtin_functions)?;
            }

            // Remove from path after visiting children
            if added_to_path {
                let finished_var = context.current_path.pop().unwrap();
                context.visiting_vars.remove(&finished_var);
            }
        }
        TemplateAstNode::Root(nodes) | TemplateAstNode::TemplateString(nodes) => {
            for n in nodes {
                validate_references_and_cycles(n, context, builtin_functions)?;
            }
        }
        TemplateAstNode::Static(_) => {} // Static text doesn't need reference validation
    }
    Ok(())
}

/// Validates the dynamic rate control parameters from RawConfig.
pub fn validate_rate_control_config(
    raw_config: &crate::config::loader::RawConfig,
) -> Result<(), ConfigError> {
    if let Some(rps) = raw_config.target_rps {
        if rps <= 0.0 {
            return Err(ConfigError::InvalidTargetRps(rps.to_string()));
        }
    }

    if let Some(rate) = raw_config.min_success_rate {
        if !(0.0..=1.0).contains(&rate) {
            return Err(ConfigError::InvalidMinSuccessRate(rate.to_string()));
        }
    }

    if let Some(factor) = raw_config.rps_adjust_factor {
        if factor <= 0.0 {
            return Err(ConfigError::InvalidRpsAdjustFactor(factor.to_string()));
        }
    }

    if let Some(factor) = raw_config.success_rate_penalty_factor {
        if factor < 1.0 {
            return Err(ConfigError::InvalidSuccessRatePenaltyFactor(
                factor.to_string(),
            ));
        }
    }

    // 验证生成器延迟控制参数
    if let Some(min_delay) = raw_config.min_delay_micros {
        if min_delay == 0 {
            return Err(ConfigError::InvalidTimeoutValue);
        }
    }

    if let Some(max_delay) = raw_config.max_delay_micros {
        if max_delay == 0 {
            return Err(ConfigError::InvalidTimeoutValue);
        }
        // 如果最小延迟也设置了，确保最大延迟大于最小延迟
        if let Some(min_delay) = raw_config.min_delay_micros {
            if max_delay < min_delay {
                return Err(ConfigError::InvalidTimeoutValue);
            }
        }
    }

    if let Some(initial_delay) = raw_config.initial_delay_micros {
        if initial_delay == 0 {
            return Err(ConfigError::InvalidTimeoutValue);
        }
    }

    if let Some(factor) = raw_config.increase_factor {
        if factor <= 1.0 {
            return Err(ConfigError::InvalidRpsAdjustFactor(factor.to_string()));
        }
    }

    if let Some(factor) = raw_config.decrease_factor {
        if factor >= 1.0 || factor <= 0.0 {
            return Err(ConfigError::InvalidRpsAdjustFactor(factor.to_string()));
        }
    }

    Ok(())
}

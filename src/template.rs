use std::collections::{HashMap, HashSet};

use crate::{
    config::loader::TemplateAstNode,
    generator::{
        chinese_bank_card::generate_chinese_bank_card,
        chinese_id::generate_chinese_id,
        chinese_name::generate_chinese_name,
        cn_mobile::generate_cn_mobile,
        email::generate_email,
        ip::{generate_ipv4, generate_ipv6},
        password::generate_password,
        qqid::generate_qq_id,
        user_agent::generate_user_agent,
        username::generate_username,
    },
    logger::Logger,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::Rng;

/// Applies built-in template functions.
/// Takes the function name, rendered arguments, and the current rendering context.
/// Returns a Result, potentially containing an error message string.
pub fn apply_function(
    name: &str,
    args: Vec<String>,
    context: &mut HashMap<String, String>, // Use context now
    logger: Logger,
    rng: &mut impl Rng,
) -> Result<String, String> {
    // Note: Context is mainly used by render_ast_node for variable lookup.
    // apply_function generally calculates/generates values.
    // Consistency for functions like password() is handled by render_ast_node storing
    // the result under the defined name (e.g., "pwd" for ${password(:pwd)})
    // and subsequent lookups hitting the context.
    match name {
        // Handle context functions first - always generate fresh value here
        "username" => {
            if !args.is_empty() {
                logger.warning(&format!(
                    "Warning: username function does not take arguments."
                ));
            }
            Ok(generate_username(rng))
        }
        "password" => {
            if !args.is_empty() {
                logger.warning(&format!(
                    "Warning: password function does not take arguments."
                ));
            }
            Ok(generate_password(rng))
        }
        "qqid" => {
            if !args.is_empty() {
                logger.warning(&format!("Warning: qqid function does not take arguments."));
            }
            Ok(generate_qq_id(rng))
        }
        "email" => {
            if !args.is_empty() {
                logger.warning(&format!("Warning: email function does not take arguments."));
            }
            Ok(generate_email(rng))
        }
        "cn_mobile" => {
            if !args.is_empty() {
                logger.warning(&format!(
                    "Warning: cn_mobile function does not take arguments."
                ));
            }
            Ok(generate_cn_mobile(rng))
        }
        "chinese_name" => {
            if !args.is_empty() {
                logger.warning(&format!(
                    "Warning: chinese_name function does not take arguments."
                ));
            }
            Ok(generate_chinese_name(rng))
        }
        "chinese_id" => {
            if !args.is_empty() {
                logger.warning(&format!(
                    "Warning: chinese_id function does not take arguments."
                ));
            }
            Ok(generate_chinese_id(rng))
        }
        "chinese_bank_card" => {
            if !args.is_empty() {
                logger.warning(&format!(
                    "Warning: chinese_bank_card function does not take arguments."
                ));
            }
            Ok(generate_chinese_bank_card(rng))
        }
        "ipv4" => {
            if !args.is_empty() {
                logger.warning(&format!("Warning: ipv4 function does not take arguments."));
            }
            Ok(generate_ipv4(rng))
        }
        "ipv6" => {
            if !args.is_empty() {
                logger.warning(&format!("Warning: ipv6 function does not take arguments."));
            }
            Ok(generate_ipv6(rng))
        }
        "user_agent" => {
            if !args.is_empty() {
                logger.warning(&format!(
                    "Warning: user_agent function does not take arguments."
                ));
            }
            Ok(generate_user_agent(rng))
        }
        "base64" => match args.first() {
            Some(arg) => Ok(STANDARD.encode(arg)),
            None => {
                logger.warning("Warning: base64 function called with no arguments.");
                Ok(String::new())
            }
        }, // Add comma here
        "upper" => Ok(args
            .first()
            .map_or_else(String::new, |arg| arg.to_uppercase())), // Add comma here
        "lower" => Ok(args
            .first()
            .map_or_else(String::new, |arg| arg.to_lowercase())), // Add comma here
        "replace" => {
            if args.len() == 3 {
                Ok(args[0].replace(&args[1], &args[2]))
            } else {
                logger.warning(&format!(
                    "Warning: replace function expects 3 arguments (target, old, new). Got {}.",
                    args.len()
                ));
                Ok(args.first().cloned().unwrap_or_default())
            }
        } // Add comma here
        "substr" => {
            if args.len() < 2 {
                logger.warning(&format!(
                    "Warning: substr function expects at least 2 arguments (target, start). Got {}.",
                    args.len()
                ));
                return Ok(args.first().cloned().unwrap_or_default());
            }

            let target = &args[0];
            let start = match args[1].parse::<usize>() {
                Ok(s) => s,
                Err(_) => {
                    logger.warning("Warning: substr start index must be a number.");
                    return Ok(target.clone());
                }
            };

            if args.len() >= 3 {
                if let Ok(len) = args[2].parse::<usize>() {
                    return Ok(target.chars().skip(start).take(len).collect());
                }
            }
            Ok(target.chars().skip(start).collect())
        } // Add comma here
        "random" => {
            if args.is_empty() {
                logger.warning(&format!(
                    "Warning: random function requires at least one argument (type)."
                ));
                return Ok(String::new());
            }

            // let mut rng = rng(); // Removed: rng is now passed as a parameter
            let random_type = &args[0];

            match random_type.as_str() {
                "chars" => {
                    if args.len() < 2 {
                        logger.warning(&format!(
                            "Warning: random chars requires at least length argument."
                        ));
                        return Ok(String::new());
                    }
                    if let Ok(len) = args[1].parse::<usize>() {
                        let charset = if args.len() >= 3 {
                            args[2].as_bytes()
                        } else {
                            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
                        };

                        if charset.is_empty() {
                            logger.warning(&format!(
                                "Warning: random chars charset cannot be empty."
                            ));
                            return Ok(String::new());
                        }

                        Ok((0..len)
                            .map(|_| {
                                let idx = rng.random_range(0..charset.len()); // Use random_range from passed rng
                                charset[idx] as char
                            })
                            .collect())
                    } else {
                        logger.warning(&format!("Warning: random chars length must be a number."));
                        Ok(String::new())
                    }
                }
                "number" => {
                    if args.len() == 2 {
                        if let Ok(max) = args[1].parse::<i64>() {
                            if max < 0 {
                                logger.warning(&format!(
                                    "Warning: random number max cannot be negative when min is 0."
                                ));
                                Ok(String::new())
                            } else {
                                Ok(rng.random_range(0..=max).to_string()) // Use random_range from passed rng
                            }
                        } else {
                            logger
                                .warning(&format!("Warning: random number max must be a number."));
                            Ok(String::new())
                        }
                    } else if args.len() == 3 {
                        if let (Ok(min), Ok(max)) = (args[1].parse::<i64>(), args[2].parse::<i64>())
                        {
                            if min > max {
                                logger.warning(&format!(
                                    "Warning: random number min cannot be greater than max."
                                ));
                                Ok(String::new())
                            } else {
                                Ok(rng.random_range(min..=max).to_string()) // Use random_range from passed rng
                            }
                        } else {
                            logger.warning(&format!(
                                "Warning: random number min and max must be numbers."
                            ));
                            Ok(String::new())
                        }
                    } else {
                        logger.warning(&format!("Warning: random number expects 1 or 2 numeric arguments (max or min, max). Got {}.", args.len() - 1));
                        Ok(String::new())
                    }
                }
                _ => {
                    logger.warning(&format!(
                        "Warning: unknown random type '{}'. Use 'chars' or 'number'.",
                        random_type
                    ));
                    Ok(String::new())
                }
            }
        } // Add comma here
        "choose_random" => {
            if args.is_empty() {
                logger.warning(&format!(
                    "Warning: choose_random function requires at least one argument."
                ));
                return Ok(String::new());
            }
            let index = rng.random_range(0..args.len()); // Use random_range from passed rng
            Ok(args[index].clone())
        } // Add comma here
        // Default: if function is not known
        _ => {
            // Check context first in case it's a defined variable
            if let Some(value) = context.get(name) {
                return Ok(value.clone());
            }
            // If not in context and not a known function, it's an error (handled by validator, but log here too)
            logger.warning(&format!(
                "Warning: Unknown function or undefined variable '{}' called.",
                name
            ));
            Ok(String::new()) // Or return Err(...)
        }
    }
}

// Recursive helper function to render an AST node
// Takes a mutable context HashMap to store/retrieve defined variables.
// Returns a Result with the rendered string or an error message.
pub fn render_ast_node(
    node: &TemplateAstNode,
    context: &mut HashMap<String, String>,
    logger: Logger,
    rng: &mut impl Rng,
) -> Result<String, String> {
    match node {
        TemplateAstNode::Static(s) => Ok(s.to_string()),
        TemplateAstNode::FunctionCall {
            def_name,
            name,
            args,
        } => {
            // 1. Check if it's a variable reference (no args, no def_name)
            if args.is_empty() && def_name.is_none() {
                // Try context first
                // Try context first (for defined variables like 'pass')
                if let Some(value) = context.get(name) {
                    return Ok(value.clone());
                }
            }

            // 2. Render arguments recursively
            let mut rendered_args = Vec::with_capacity(args.len());
            for arg_node in args {
                rendered_args.push(render_ast_node(arg_node, context, logger.clone(), rng)?);
            }

            // 3. Apply the function
            let result = apply_function(name, rendered_args, context, logger.clone(), rng)?;

            // 4. Store result if it's a definition
            if let Some(d_name) = def_name {
                // Validator already checked for duplicates, but maybe check again? Or rely on validator.
                // For simplicity here, we overwrite if re-defined (though validator prevents this).
                context.insert(d_name.clone(), result.clone());
            }

            Ok(result)
        }
        TemplateAstNode::Root(nodes) => {
            // Render each node in the root sequence and concatenate
            nodes
                .iter()
                .map(|n| render_ast_node(n, context, logger.clone(), rng))
                .collect::<Result<String, _>>()
        }
        TemplateAstNode::TemplateString(nodes) => {
            // Render each node within the template string and concatenate
            nodes
                .iter()
                .map(|n| render_ast_node(n, context, logger.clone(), rng))
                .collect::<Result<String, _>>()
        }
    }
}

/// Returns a HashSet containing the names of all built-in template functions.
pub fn get_builtin_function_names() -> HashSet<String> {
    [
        "base64",
        "upper",
        "lower",
        "replace",
        "substr",
        "random",
        "choose_random",
        "username",
        "password",
        "qqid",
        "email",
        "cn_mobile",
        "chinese_name",
        "chinese_id",
        "chinese_bank_card",
        "ipv4",
        "ipv6",
        "user_agent",
    ]
    .iter()
    .map(|&s| s.to_string())
    .collect()
}

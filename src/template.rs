use crate::{
    config::TemplateAstNode,
    generator::{password::generate_password, qqid::generate_qq_id, username::generate_username},
    logger::Logger,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand::{rng, Rng};

/// Applies built-in template functions.
/// Takes the function name, rendered arguments, and the current rendering context.
pub fn apply_function(name: &str, args: Vec<String>, logger: Logger) -> String {
    match name {
        // Handle context functions first
        "username" => {
            if !args.is_empty() {
                logger.warning(&format!(
                    "Warning: username function does not take arguments."
                ));
            }
            generate_username(&mut rng())
        }
        "password" => {
            if !args.is_empty() {
                logger.warning(&format!(
                    "Warning: password function does not take arguments."
                ));
            }
            generate_password(&mut rng())
        }
        "qqid" => {
            if !args.is_empty() {
                logger.warning(&format!("Warning: qqid function does not take arguments."));
            }
            generate_qq_id(&mut rng())
        }
        // Other functions
        "base64" => {
            // base64 takes one argument
            if let Some(arg) = args.first() {
                STANDARD.encode(arg)
            } else {
                logger.warning(&format!(
                    "Warning: base64 function called with no arguments."
                ));
                String::new()
            }
        }
        "upper" => {
            if let Some(arg) = args.first() {
                arg.to_uppercase()
            } else {
                String::new()
            }
        }
        "lower" => {
            if let Some(arg) = args.first() {
                arg.to_lowercase()
            } else {
                String::new()
            }
        }
        "replace" => {
            // replace:target,old,new
            if args.len() == 3 {
                args[0].replace(&args[1], &args[2])
            } else {
                logger.warning(&format!(
                    "Warning: replace function expects 3 arguments (target, old, new). Got {}.",
                    args.len()
                ));
                args.first().cloned().unwrap_or_default() // Return original string on error
            }
        }
        "substr" => {
            // substr:target,start,length (optional)
            if args.len() >= 2 {
                let target = &args[0];
                if let Ok(start) = args[1].parse::<usize>() {
                    let len = if args.len() >= 3 {
                        args[2].parse::<usize>().ok()
                    } else {
                        None // Take rest of string if no length specified
                    };

                    if let Some(l) = len {
                        target.chars().skip(start).take(l).collect()
                    } else {
                        target.chars().skip(start).collect()
                    }
                } else {
                    logger.warning(&format!("Warning: substr start index must be a number."));
                    target.clone()
                }
            } else {
                logger.warning(&format!("Warning: substr function expects at least 2 arguments (target, start). Got {}.", args.len()));
                args.first().cloned().unwrap_or_default()
            }
        }
        "random" => {
            // random:type,...args
            if args.is_empty() {
                logger.warning(&format!(
                    "Warning: random function requires at least one argument (type)."
                ));
                return String::new();
            }

            let mut rng = rng(); // Use thread_rng
            let random_type = &args[0];

            match random_type.as_str() {
                "chars" => {
                    // random:chars,length[,charset]
                    if args.len() < 2 {
                        logger.warning(&format!(
                            "Warning: random chars requires at least length argument."
                        ));
                        return String::new();
                    }
                    if let Ok(len) = args[1].parse::<usize>() {
                        let charset = if args.len() >= 3 {
                            args[2].as_bytes()
                        } else {
                            // Default charset
                            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
                        };

                        if charset.is_empty() {
                            logger.warning(&format!(
                                "Warning: random chars charset cannot be empty."
                            ));
                            return String::new();
                        }

                        (0..len)
                            .map(|_| {
                                let idx = rng.random_range(0..charset.len());
                                charset[idx] as char
                            })
                            .collect()
                    } else {
                        logger.warning(&format!("Warning: random chars length must be a number."));
                        String::new()
                    }
                }
                "number" => {
                    // random:number,max OR random:number,min,max
                    if args.len() == 2 {
                        // random:number,max (assume min=0)
                        if let Ok(max) = args[1].parse::<i64>() {
                            if max < 0 {
                                logger.warning(&format!(
                                    "Warning: random number max cannot be negative when min is 0."
                                ));
                                String::new()
                            } else {
                                rng.random_range(0..=max).to_string()
                            }
                        } else {
                            logger
                                .warning(&format!("Warning: random number max must be a number."));
                            String::new()
                        }
                    } else if args.len() == 3 {
                        // random:number,min,max
                        if let (Ok(min), Ok(max)) = (args[1].parse::<i64>(), args[2].parse::<i64>())
                        {
                            if min > max {
                                logger.warning(&format!(
                                    "Warning: random number min cannot be greater than max."
                                ));
                                String::new()
                            } else {
                                rng.random_range(min..=max).to_string()
                            }
                        } else {
                            logger.warning(&format!(
                                "Warning: random number min and max must be numbers."
                            ));
                            String::new()
                        }
                    } else {
                        logger.warning(&format!("Warning: random number expects 1 or 2 numeric arguments (max or min, max). Got {}.", args.len() - 1));
                        String::new()
                    }
                }
                _ => {
                    logger.warning(&format!(
                        "Warning: unknown random type '{}'. Use 'chars' or 'number'.",
                        random_type
                    ));
                    String::new()
                }
            }
        }
        // Default: if function is not known
        _ => {
            logger.warning(&format!("Warning: Unknown function '{}' called.", name));
            // Maybe return a placeholder or the raw call? For now, empty string.
            // format!("${{{}:{}}}", name, args.join(","))
            String::new()
        }
    }
}

// Recursive helper function to render an AST node
pub fn render_ast_node(node: &TemplateAstNode, logger: Logger) -> String {
    match node {
        TemplateAstNode::Static(s) => s.clone(),
        TemplateAstNode::FunctionCall { name, args } => {
            let rendered_args: Vec<String> = args.iter().map(|arg_node| {
                match arg_node {
                    TemplateAstNode::Root(nodes) => {
                        nodes.iter().map(|n| render_ast_node(n, logger.clone())).collect::<String>()
                    }
                    _ => render_ast_node(arg_node, logger.clone()),
                }
            }).collect();
        
            apply_function(name, rendered_args, logger.clone())
        }
        TemplateAstNode::Root(nodes) => {
            // Render each node in the root sequence and concatenate
            nodes
                .iter()
                .map(|n| render_ast_node(n, logger.clone()))
                .collect::<String>()
        }
        TemplateAstNode::TemplateString(nodes) => {
            nodes.iter()
                .map(|n| render_ast_node(n, logger.clone()))
                .collect::<String>()
        }
    }
}

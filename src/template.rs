use crate::config::{CompiledUrl, TemplateAstNode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand::{rng, Rng}; // Use thread_rng instead of deprecated rng()

/// Applies built-in template functions.
/// Takes the function name, rendered arguments, and the current rendering context.
pub fn apply_function(
    name: &str,
    args: Vec<String>,
    username: Option<&str>,
    password: Option<&str>,
    qqid: Option<&str>,
) -> String {
    match name {
        // Handle context functions first
        "username" => {
            if !args.is_empty() {
                eprintln!("Warning: username function does not take arguments.");
            }
            username.unwrap_or("").to_string()
        }
        "password" => {
            if !args.is_empty() {
                eprintln!("Warning: password function does not take arguments.");
            }
            password.unwrap_or("").to_string()
        }
        "qqid" => {
            if !args.is_empty() {
                eprintln!("Warning: qqid function does not take arguments.");
            }
            qqid.unwrap_or("").to_string()
        }
        // Other functions
        "base64" => {
            // base64 takes one argument
            if let Some(arg) = args.first() {
                STANDARD.encode(arg)
            } else {
                eprintln!("Warning: base64 function called with no arguments.");
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
                eprintln!(
                    "Warning: replace function expects 3 arguments (target, old, new). Got {}.",
                    args.len()
                );
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
                    eprintln!("Warning: substr start index must be a number.");
                    target.clone()
                }
            } else {
                eprintln!("Warning: substr function expects at least 2 arguments (target, start). Got {}.", args.len());
                args.first().cloned().unwrap_or_default()
            }
        }
        "random" => {
            // random:type,...args
            if args.is_empty() {
                eprintln!("Warning: random function requires at least one argument (type).");
                return String::new();
            }

            let mut rng = rng(); // Use thread_rng
            let random_type = &args[0];

            match random_type.as_str() {
                "chars" => {
                    // random:chars,length[,charset]
                    if args.len() < 2 {
                        eprintln!("Warning: random chars requires at least length argument.");
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
                             eprintln!("Warning: random chars charset cannot be empty.");
                             return String::new();
                        }

                        (0..len)
                            .map(|_| {
                                let idx = rng.random_range(0..charset.len());
                                charset[idx] as char
                            })
                            .collect()
                    } else {
                        eprintln!("Warning: random chars length must be a number.");
                        String::new()
                    }
                }
                "number" => {
                    // random:number,max OR random:number,min,max
                    if args.len() == 2 {
                        // random:number,max (assume min=0)
                        if let Ok(max) = args[1].parse::<i64>() {
                             if max < 0 {
                                eprintln!("Warning: random number max cannot be negative when min is 0.");
                                String::new()
                             } else {
                                rng.random_range(0..=max).to_string()
                             }
                        } else {
                            eprintln!("Warning: random number max must be a number.");
                            String::new()
                        }
                    } else if args.len() == 3 {
                         // random:number,min,max
                         if let (Ok(min), Ok(max)) = (args[1].parse::<i64>(), args[2].parse::<i64>()) {
                             if min > max {
                                eprintln!("Warning: random number min cannot be greater than max.");
                                String::new()
                             } else {
                                rng.random_range(min..=max).to_string()
                             }
                         } else {
                             eprintln!("Warning: random number min and max must be numbers.");
                             String::new()
                         }
                    } else {
                        eprintln!("Warning: random number expects 1 or 2 numeric arguments (max or min, max). Got {}.", args.len() - 1);
                        String::new()
                    }
                }
                _ => {
                    eprintln!(
                        "Warning: unknown random type '{}'. Use 'chars' or 'number'.",
                        random_type
                    );
                    String::new()
                }
            }
        }
        // Default: if function is not known
        _ => {
            eprintln!("Warning: Unknown function '{}' called.", name);
            // Maybe return a placeholder or the raw call? For now, empty string.
            // format!("${{{}:{}}}", name, args.join(","))
            String::new()
        }
    }
}

// Recursive helper function to render an AST node
fn render_ast_node(
    node: &TemplateAstNode,
    username: Option<&str>,
    password: Option<&str>,
    qqid: Option<&str>,
) -> String {
    match node {
        TemplateAstNode::Static(s) => s.clone(),
        // Handle Variables (identifiers used without arguments, other than user/pass/qqid)
        TemplateAstNode::Variable(name) => {
             eprintln!("Warning: Encountered unknown variable identifier without arguments: ${{{}}}. Returning empty string.", name);
             "".to_string()
        }
        TemplateAstNode::FunctionCall { name, args } => {
            // Render all arguments first
            let rendered_args: Vec<String> = args
                .iter()
                .map(|arg_node| render_ast_node(arg_node, username, password, qqid))
                .collect();

            // Pass rendered args AND context to apply_function
            apply_function(name, rendered_args, username, password, qqid)
        }
        TemplateAstNode::Root(nodes) => {
            // Render each node in the root sequence and concatenate
            nodes
                .iter()
                .map(|n| render_ast_node(n, username, password, qqid))
                .collect::<String>()
        }
    }
}

/// Renders a CompiledUrl by processing its AST.
pub fn render_compiled_url(
    template: &CompiledUrl, // Use the updated CompiledUrl struct
    username: Option<&str>,
    password: Option<&str>,
    qqid: Option<&str>,
) -> String {
    // Start rendering from the root of the AST stored in the template
    render_ast_node(&template.ast, username, password, qqid)
}

mod ast;
mod parser;
mod renderer;

pub use ast::TemplateAstNode;
pub use parser::parse_template_string;
pub use renderer::{get_builtin_function_names, render_ast_node};

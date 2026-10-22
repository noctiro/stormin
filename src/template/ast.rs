#[derive(Clone, Debug, PartialEq)]
pub enum TemplateAstNode {
    Static(String),
    FunctionCall {
        def_name: Option<String>,
        name: String,
        args: Vec<TemplateAstNode>,
    },
    Root(Vec<TemplateAstNode>),
    TemplateString(Vec<TemplateAstNode>),
}

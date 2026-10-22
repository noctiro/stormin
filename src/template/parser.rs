use super::TemplateAstNode;
use crate::config::validator::ConfigError;
use pest::Parser;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "template/template.pest"]
struct TemplateParser;

pub fn parse_template_string(input: &str) -> Result<TemplateAstNode, ConfigError> {
    let pairs = TemplateParser::parse(Rule::template, input)
        .map_err(|e| ConfigError::TemplateParseError(e.to_string()))?;
    let top_pair = pairs
        .peek()
        .ok_or_else(|| ConfigError::TemplateParseError("Empty parse result".into()))?;
    build_ast_from_pair(top_pair)
        .map_err(|e| ConfigError::TemplateParseError(format!("Failed to build AST: {}", e)))
}

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

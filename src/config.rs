use serde::Deserialize;
use std::{error::Error, fmt, fs};

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
pub struct AttackConfig {
    pub proxy_file: String,
    pub threads: usize,
    pub targets: Vec<CompiledTarget>,
}

#[derive(Clone, Debug)]
pub struct CompiledTarget {
    pub url: String,
    pub method: String,
    pub headers: std::collections::HashMap<String, String>,
    pub params: Vec<(String, CompiledUrl)>,
}

#[derive(Clone, Debug)]
pub struct CompiledUrl {
    pub parts: Vec<UrlPart>,
    pub needs_user: bool,
    pub needs_password: bool,
    pub needs_qqid: bool,
}

// --- Parsing Logic & Types ---

#[derive(Debug)]
pub enum ParseError {
    UnmatchedQuote,
    InvalidPlaceholder(String),
    UnexpectedEndOfInput,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnmatchedQuote => write!(f, "Unmatched '\"' found during parsing"),
            ParseError::InvalidPlaceholder(s) => write!(f, "Invalid placeholder content: {}", s),
            ParseError::UnexpectedEndOfInput => write!(f, "Unexpected end of input during parsing"),
        }
    }
}

impl Error for ParseError {}

#[derive(Clone, Debug, PartialEq)]
pub enum UrlPart {
    Static(String),
    User,
    Password,
    Qqid,
    FunctionCall { name: String, args: Vec<UrlPart> },
}

struct ParserState<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> ParserState<'a> {
    fn new(input: &'a str) -> Self { ParserState { input, pos: 0 } }
    fn peek(&self) -> Option<char> { self.input.get(self.pos..).and_then(|s| s.chars().next()) }
    fn advance(&mut self) { if let Some(c) = self.peek() { self.pos += c.len_utf8(); } }
    fn consume_str(&mut self, expected: &str) -> bool {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len(); true
        } else { false }
    }
    fn slice(&self, start: usize, end: usize) -> &'a str { &self.input[start..end] }
    fn is_eof(&self) -> bool { self.pos >= self.input.len() }
}

fn parse_recursive(
    state: &mut ParserState,
    terminator: Option<char>,
) -> Result<(Vec<UrlPart>, bool, bool, bool), ParseError> {
    let mut parts = Vec::new();
    let mut start = state.pos;
    let mut flags = (false, false, false);

    while !state.is_eof() {
        if let Some(term) = terminator {
            if state.peek() == Some(term) {
                if state.pos > start {
                    parts.push(UrlPart::Static(state.slice(start, state.pos).to_string()));
                }
                state.advance();
                return Ok((parts, flags.0, flags.1, flags.2));
            }
        }
        if state.peek() == Some('$') && state.input[state.pos..].starts_with("${") {
            if state.pos > start {
                parts.push(UrlPart::Static(state.slice(start, state.pos).to_string()));
            }
            state.consume_str("${");
            let (part, u, p, q) = parse_placeholder_content(state)?;
            parts.push(part);
            flags.0 |= u;
            flags.1 |= p;
            flags.2 |= q;
            start = state.pos;
        } else if state.peek() == Some('{') {
            // brace-function
            if state.pos > start {
                parts.push(UrlPart::Static(state.slice(start, state.pos).to_string()));
            }
            state.advance();
            let start_pos = state.pos;
            let (_ignored, _u, _p, _q) = parse_recursive(state, Some('}'))?;
            let content = state.slice(start_pos, state.pos - 1);
            if let Some(colon) = find_top_level_colon(content) {
                let name = content[..colon].trim();
                let args_str = &content[colon + 1..];
                let mut arg_state = ParserState::new(args_str);
                let (arg_parts, au, ap, aq) = parse_recursive(&mut arg_state, None)?;
                parts.push(UrlPart::FunctionCall { name: name.to_string(), args: arg_parts });
                flags.0 |= au;
                flags.1 |= ap;
                flags.2 |= aq;
            } else {
                parts.push(UrlPart::Static(format!("{{{}}}", content)));
            }
            start = state.pos;
        } else if state.peek() == Some('"') {
            if state.pos > start {
                parts.push(UrlPart::Static(state.slice(start, state.pos).to_string()));
            }
            state.advance();
            let lit_start = state.pos;
            while !state.is_eof() {
                if state.peek() == Some('\\') {
                    state.advance(); // Skip the backslash
                    state.advance(); // Skip the next character (escaped character)
                } else if state.peek() == Some('"') {
                    break;
                } else {
                    state.advance();
                }
            }
            if state.consume_str("\"") {
                parts.push(UrlPart::Static(state.slice(lit_start, state.pos - 1).to_string()));
                start = state.pos;
            } else {
                return Err(ParseError::UnmatchedQuote);
            }
        } else {
            state.advance();
        }
    }
    if terminator.is_some() { return Err(ParseError::UnexpectedEndOfInput); }
    if state.pos > start {
        parts.push(UrlPart::Static(state.slice(start, state.pos).to_string()));
    }
    Ok((parts, flags.0, flags.1, flags.2))
}

fn parse_placeholder_content(
    state: &mut ParserState,
) -> Result<(UrlPart, bool, bool, bool), ParseError> {
    let start_pos = state.pos;
    let (_ignored, _u, _p, _q) = parse_recursive(state, Some('}'))?;
    let content = state.slice(start_pos, state.pos - 1);

    if let Some(colon) = find_top_level_colon(content) {
        let name = content[..colon].trim();
        let args_str = content[colon + 1..].trim();
        let mut arg_state = ParserState::new(args_str);
        let (arg_parts, au, ap, aq) = parse_recursive(&mut arg_state, None)?;
        Ok((UrlPart::FunctionCall { name: name.to_string(), args: arg_parts }, au, ap, aq))
    } else {
        match content {
            "user" => Ok((UrlPart::User, true, false, false)),
            "password" => Ok((UrlPart::Password, false, true, false)),
            "qqid" => Ok((UrlPart::Qqid, false, false, true)),
            _ => Err(ParseError::InvalidPlaceholder(content.to_string())),
        }
    }
}

fn find_top_level_colon(text: &str) -> Option<usize> {
    let mut level = 0;
    for (i, ch) in text.char_indices() {
        match ch {
            ':' if level == 0 => return Some(i),
            '$' if text[i..].starts_with("${") => level += 1,
            '}' if level > 0 => level -= 1,
            _ => {}
        }
    }
    None
}

fn compile_url_template(template: String) -> Result<CompiledUrl, ParseError> {
    let mut state = ParserState::new(&template);
    let (parts, u, p, q) = parse_recursive(&mut state, None)?;
    if !state.is_eof() {
        eprintln!("Warning: unfinished parse at pos {}", state.pos);
    }
    Ok(CompiledUrl { parts, needs_user: u, needs_password: p, needs_qqid: q })
}

/// Loads configuration and compiles all targets
pub fn load_config_and_compile(path: &str) -> Result<AttackConfig, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let raw: RawConfig = toml::from_str(&content)?;

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
    Ok(AttackConfig { proxy_file: raw.proxy_file, threads: raw.threads.unwrap_or(1), targets: compiled })
}

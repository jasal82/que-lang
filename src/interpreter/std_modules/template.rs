//! std.template module — Simple template engine with {{ expr }}, conditionals, and loops.

use crate::error::*;
use crate::value::Value;
use crate::interpreter::helpers::arg_path_str;
use super::super::Interpreter;
use super::StdModule;

use std::collections::BTreeMap;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "template",
        functions: &["render", "render_file"],
    }
}

impl Interpreter {
    pub(crate) fn call_template(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "render" => {
                let tmpl = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(sig_type("template.render", "String for template")),
                };
                let ctx = match args.get(1) {
                    Some(Value::Map(m)) => m.clone(),
                    _ => return Err(sig_type("template.render", "Map for context")),
                };
                let result = render_template(&tmpl, &ctx)
                    .map_err(|e| sig_err(e))?;
                Ok(Value::String(result))
            }
            "render_file" => {
                let path = arg_path_str(args, 0, "template.render_file")?;
                let ctx = match args.get(1) {
                    Some(Value::Map(m)) => m.clone(),
                    _ => return Err(sig_type("template.render_file", "Map for context")),
                };
                let tmpl = std::fs::read_to_string(&path)
                    .map_err(|e| sig_err(format!("failed to read template file '{}': {}", path, e)))?;
                let result = render_template(&tmpl, &ctx)
                    .map_err(|e| sig_err(e))?;
                Ok(Value::String(result))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'template.{}'", func),
            ))),
        }
    }
}

// ── Private helpers ────────────────────────────────────────────────────────

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

fn sig_type(name: &str, expected: &str) -> Signal {
    Signal::Error(QueError::new(
        ErrorKind::TypeMismatch,
        format!("{}: expected {}", name, expected),
    ))
}

// ── Template engine ────────────────────────────────────────────────────────

fn render_template(
    template: &str,
    context: &BTreeMap<String, Value>,
) -> Result<String, String> {
    let tokens = parse_template(template)?;
    render_tokens(&tokens, context)
}

#[derive(Debug)]
enum TemplateToken {
    Literal(String),
    Expr(String),
    If {
        condition: String,
        body: Vec<TemplateToken>,
        else_body: Vec<TemplateToken>,
    },
    For {
        var: String,
        iterable: String,
        body: Vec<TemplateToken>,
    },
}

fn parse_template(template: &str) -> Result<Vec<TemplateToken>, String> {
    let mut tokens = Vec::new();
    let mut rest = template;

    while !rest.is_empty() {
        if let Some(pos) = rest.find("{{") {
            if pos > 0 {
                tokens.push(TemplateToken::Literal(rest[..pos].to_string()));
            }
            rest = &rest[pos + 2..];

            if rest.starts_with('-') {
                rest = &rest[1..];
                if let Some(TemplateToken::Literal(ref mut s)) = tokens.last_mut() {
                    *s = s.trim_end().to_string();
                }
            }

            let rest_trimmed = rest.trim_start();
            if rest_trimmed.starts_with('#') {
                let directive_rest = rest_trimmed[1..].trim_start();
                if directive_rest.starts_with("if ") || directive_rest.starts_with("if\t") {
                    let (if_token, consumed) = parse_if_directive(rest)?;
                    tokens.push(if_token);
                    rest = consumed;
                } else if directive_rest.starts_with("for ") || directive_rest.starts_with("for\t") {
                    let (for_token, consumed) = parse_for_directive(rest)?;
                    tokens.push(for_token);
                    rest = consumed;
                } else {
                    return Err(format!("unknown template directive: {{{{# {}}}}}", &directive_rest[..directive_rest.find("}}").unwrap_or(20).min(20)]));
                }
            } else if rest_trimmed.starts_with('/') {
                return Err("unexpected closing tag outside of block directive".to_string());
            } else {
                let close = find_closing_braces(rest)?;
                let expr_str = rest[..close].trim();
                let expr_str = expr_str.strip_suffix('-').unwrap_or(expr_str).trim();
                tokens.push(TemplateToken::Expr(expr_str.to_string()));
                rest = &rest[close + 2..];
            }
        } else {
            tokens.push(TemplateToken::Literal(rest.to_string()));
            break;
        }
    }

    Ok(tokens)
}

fn find_closing_braces(s: &str) -> Result<usize, String> {
    s.find("}}").ok_or_else(|| "unclosed template tag: missing `}}`".to_string())
}

fn parse_if_directive(rest: &str) -> Result<(TemplateToken, &str), String> {
    let close = find_closing_braces(rest)?;
    let tag_content = rest[..close].trim();
    let tag_content = tag_content.strip_suffix('-').unwrap_or(tag_content).trim();
    let condition = tag_content
        .strip_prefix('#')
        .unwrap_or(tag_content)
        .trim()
        .strip_prefix("if")
        .ok_or("expected 'if' in template directive")?
        .trim()
        .to_string();

    let mut after_tag = &rest[close + 2..];
    if after_tag.starts_with('\n') {
        after_tag = &after_tag[1..];
    } else if after_tag.starts_with("\r\n") {
        after_tag = &after_tag[2..];
    }

    let (body, else_body, remaining) = parse_block_body(after_tag, "if")?;
    Ok((TemplateToken::If { condition, body, else_body }, remaining))
}

fn parse_for_directive(rest: &str) -> Result<(TemplateToken, &str), String> {
    let close = find_closing_braces(rest)?;
    let tag_content = rest[..close].trim();
    let tag_content = tag_content.strip_suffix('-').unwrap_or(tag_content).trim();
    let inner = tag_content
        .strip_prefix('#')
        .unwrap_or(tag_content)
        .trim()
        .strip_prefix("for")
        .ok_or("expected 'for' in template directive")?
        .trim();

    let in_pos = inner.find(" in ")
        .ok_or("expected ' in ' in for directive")?;
    let var = inner[..in_pos].trim().to_string();
    let iterable = inner[in_pos + 4..].trim().to_string();

    let mut after_tag = &rest[close + 2..];
    if after_tag.starts_with('\n') {
        after_tag = &after_tag[1..];
    } else if after_tag.starts_with("\r\n") {
        after_tag = &after_tag[2..];
    }

    let (body, remaining) = parse_for_body(after_tag)?;
    Ok((TemplateToken::For { var, iterable, body }, remaining))
}

fn parse_block_body<'a>(
    input: &'a str,
    tag_name: &str,
) -> Result<(Vec<TemplateToken>, Vec<TemplateToken>, &'a str), String> {
    let mut body = Vec::new();
    let mut else_body = Vec::new();
    let mut in_else = false;
    let mut rest = input;

    while !rest.is_empty() {
        if let Some(pos) = rest.find("{{") {
            if pos > 0 {
                let lit = rest[..pos].to_string();
                if in_else { else_body.push(TemplateToken::Literal(lit)); }
                else { body.push(TemplateToken::Literal(lit)); }
            }
            rest = &rest[pos + 2..];

            let rest_trimmed = rest.trim_start();

            if rest_trimmed.starts_with('/') {
                let after_slash = rest_trimmed[1..].trim_start();
                let closing_name = after_slash.split(|c: char| c.is_whitespace() || c == '}' || c == '-')
                    .next().unwrap_or("");
                if closing_name == tag_name {
                    let close = find_closing_braces(rest)?;
                    let mut after = &rest[close + 2..];
                    if after.starts_with('\n') { after = &after[1..]; }
                    else if after.starts_with("\r\n") { after = &after[2..]; }
                    return Ok((body, else_body, after));
                }
            }

            if rest_trimmed.starts_with('#') {
                let directive_rest = rest_trimmed[1..].trim_start();
                if directive_rest.starts_with("else") {
                    let after_else = &directive_rest[4..];
                    if after_else.is_empty() || after_else.starts_with(|c: char| c.is_whitespace() || c == '}' || c == '-') {
                        let close = find_closing_braces(rest)?;
                        rest = &rest[close + 2..];
                        if rest.starts_with('\n') { rest = &rest[1..]; }
                        else if rest.starts_with("\r\n") { rest = &rest[2..]; }
                        in_else = true;
                        continue;
                    }
                }

                if directive_rest.starts_with("if ") || directive_rest.starts_with("if\t") {
                    let (token, remaining) = parse_if_directive(rest)?;
                    if in_else { else_body.push(token); } else { body.push(token); }
                    rest = remaining;
                    continue;
                }
                if directive_rest.starts_with("for ") || directive_rest.starts_with("for\t") {
                    let (token, remaining) = parse_for_directive(rest)?;
                    if in_else { else_body.push(token); } else { body.push(token); }
                    rest = remaining;
                    continue;
                }
            }

            let close = find_closing_braces(rest)?;
            let expr_str = rest[..close].trim();
            let expr_str = expr_str.strip_suffix('-').unwrap_or(expr_str).trim();
            let token = TemplateToken::Expr(expr_str.to_string());
            if in_else { else_body.push(token); } else { body.push(token); }
            rest = &rest[close + 2..];
        } else {
            let lit = rest.to_string();
            if in_else { else_body.push(TemplateToken::Literal(lit)); }
            else { body.push(TemplateToken::Literal(lit)); }
            return Err(format!("unclosed {{{{# {} }}}} block", tag_name));
        }
    }
    Err(format!("unclosed {{{{# {} }}}} block", tag_name))
}

fn parse_for_body(input: &str) -> Result<(Vec<TemplateToken>, &str), String> {
    let (body, _else, remaining) = parse_block_body(input, "for")?;
    Ok((body, remaining))
}

fn render_tokens(
    tokens: &[TemplateToken],
    context: &BTreeMap<String, Value>,
) -> Result<String, String> {
    let mut output = String::new();
    for token in tokens {
        match token {
            TemplateToken::Literal(s) => output.push_str(s),
            TemplateToken::Expr(expr) => {
                let value = resolve_expr(expr, context);
                output.push_str(&value.display_string());
            }
            TemplateToken::If { condition, body, else_body } => {
                let val = resolve_expr(condition, context);
                if is_truthy(&val) {
                    output.push_str(&render_tokens(body, context)?);
                } else {
                    output.push_str(&render_tokens(else_body, context)?);
                }
            }
            TemplateToken::For { var, iterable, body } => {
                let collection = resolve_expr(iterable, context);
                let items = match &collection {
                    Value::List(l) => l.clone(),
                    Value::Tuple(t) => t.clone(),
                    _ => return Err(format!(
                        "template for-loop: '{}' is not iterable (got {})",
                        iterable, collection.type_name()
                    )),
                };
                for item in &items {
                    let mut inner_ctx = context.clone();
                    inner_ctx.insert(var.clone(), item.clone());
                    output.push_str(&render_tokens(body, &inner_ctx)?);
                }
            }
        }
    }
    Ok(output)
}

fn resolve_expr(expr: &str, context: &BTreeMap<String, Value>) -> Value {
    let expr = expr.trim();

    if (expr.starts_with('"') && expr.ends_with('"'))
        || (expr.starts_with('\'') && expr.ends_with('\''))
    {
        return Value::String(expr[1..expr.len()-1].to_string());
    }

    if let Ok(n) = expr.parse::<i64>() {
        return Value::Int(n);
    }
    if let Ok(f) = expr.parse::<f64>() {
        return Value::Float(f);
    }
    if expr == "true" { return Value::Bool(true); }
    if expr == "false" { return Value::Bool(false); }
    if expr == "null" { return Value::Null; }

    let parts: Vec<&str> = expr.split('.').collect();
    let mut current = match context.get(parts[0]) {
        Some(v) => v.clone(),
        None => return Value::Null,
    };
    for part in &parts[1..] {
        current = match &current {
            Value::Map(m) => m.get(*part).cloned().unwrap_or(Value::Null),
            _ => return Value::Null,
        };
    }
    current
}

fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(f) => *f != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::List(l) => !l.is_empty(),
        Value::Map(m) => !m.is_empty(),
        _ => true,
    }
}

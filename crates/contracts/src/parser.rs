//! Condition expression parser and evaluator.
//!
//! Supports a simple DSL for matching tool arguments and response content:
//!
//! ```text
//! args.command CONTAINS "rm -rf"
//! args.url MATCHES "^https?://10\\."
//! args.path NOT STARTS_WITH "/workspace"
//! args.amount > 50
//! content CONTAINS "password"
//! args.command CONTAINS "rm" AND args.command NOT CONTAINS "workspace"
//! ```
//!
//! Grammar (informal):
//! ```text
//! expr     = clause (("AND" | "OR") clause)*
//! clause   = ["NOT"] atom
//! atom     = field OP value
//! field    = "args." IDENT | "content" | "tool_name"
//! OP       = "CONTAINS" | "MATCHES" | "STARTS_WITH" | "ENDS_WITH"
//!          | "==" | "!=" | ">" | "<" | ">=" | "<="
//! value    = QUOTED_STRING | NUMBER
//! ```

use regex_lite::Regex;

/// A parsed condition tree.
#[derive(Debug, Clone)]
pub enum Condition {
    /// A single comparison.
    Atom(Atom),
    /// Logical AND of two sub-conditions.
    And(Box<Condition>, Box<Condition>),
    /// Logical OR of two sub-conditions.
    Or(Box<Condition>, Box<Condition>),
    /// Negation.
    Not(Box<Condition>),
    /// Always true (empty condition).
    Always,
}

#[derive(Debug, Clone)]
pub struct Atom {
    pub field: Field,
    pub op: Op,
    pub value: Value,
}

/// A field reference in a condition.
#[derive(Debug, Clone)]
pub enum Field {
    /// `args.<name>` — looks up a key in the tool call's arguments JSON.
    Arg(String),
    /// `content` — the full text content (for response contracts).
    Content,
    /// `tool_name` — the name of the tool being invoked.
    ToolName,
}

/// Comparison operators.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Contains,
    NotContains,
    Matches,
    NotMatches,
    StartsWith,
    NotStartsWith,
    EndsWith,
    NotEndsWith,
    Eq,
    NotEq,
    Gt,
    Lt,
    Gte,
    Lte,
}

/// A literal value in a condition.
#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Num(f64),
}

impl Condition {
    /// Evaluate this condition against a context.
    pub fn evaluate(&self, ctx: &EvalContext<'_>) -> bool {
        match self {
            Condition::Always => true,
            Condition::Atom(atom) => atom.evaluate(ctx),
            Condition::And(a, b) => a.evaluate(ctx) && b.evaluate(ctx),
            Condition::Or(a, b) => a.evaluate(ctx) || b.evaluate(ctx),
            Condition::Not(inner) => !inner.evaluate(ctx),
        }
    }
}

/// Context provided for condition evaluation.
pub struct EvalContext<'a> {
    /// The tool call arguments (JSON object), if applicable.
    pub args: Option<&'a serde_json::Value>,
    /// The text content (for response conditions).
    pub content: Option<&'a str>,
    /// The tool name being invoked.
    pub tool_name: Option<&'a str>,
}

impl Atom {
    fn evaluate(&self, ctx: &EvalContext<'_>) -> bool {
        let field_value = self.resolve_field(ctx);
        match &self.op {
            Op::Contains => field_value
                .as_deref()
                .is_some_and(|fv| fv.contains(self.value.as_str())),
            Op::NotContains => field_value
                .as_deref()
                .is_none_or(|fv| !fv.contains(self.value.as_str())),
            Op::Matches => {
                let pattern = self.value.as_str();
                field_value
                    .as_deref()
                    .is_some_and(|fv| Regex::new(pattern).is_ok_and(|re| re.is_match(fv)))
            }
            Op::NotMatches => {
                let pattern = self.value.as_str();
                field_value
                    .as_deref()
                    .is_none_or(|fv| Regex::new(pattern).is_ok_and(|re| !re.is_match(fv)))
            }
            Op::StartsWith => field_value
                .as_deref()
                .is_some_and(|fv| fv.starts_with(self.value.as_str())),
            Op::NotStartsWith => field_value
                .as_deref()
                .is_none_or(|fv| !fv.starts_with(self.value.as_str())),
            Op::EndsWith => field_value
                .as_deref()
                .is_some_and(|fv| fv.ends_with(self.value.as_str())),
            Op::NotEndsWith => field_value
                .as_deref()
                .is_none_or(|fv| !fv.ends_with(self.value.as_str())),
            Op::Eq => match (&field_value, &self.value) {
                (Some(fv), Value::Str(s)) => fv == s,
                (Some(fv), Value::Num(n)) => fv
                    .parse::<f64>()
                    .is_ok_and(|x| (x - n).abs() < f64::EPSILON),
                (None, _) => false,
            },
            Op::NotEq => match (&field_value, &self.value) {
                (Some(fv), Value::Str(s)) => fv != s,
                (Some(fv), Value::Num(n)) => fv
                    .parse::<f64>()
                    .is_ok_and(|x| (x - n).abs() >= f64::EPSILON),
                (None, _) => true,
            },
            Op::Gt => self.compare_num(&field_value, |a, b| a > b),
            Op::Lt => self.compare_num(&field_value, |a, b| a < b),
            Op::Gte => self.compare_num(&field_value, |a, b| a >= b),
            Op::Lte => self.compare_num(&field_value, |a, b| a <= b),
        }
    }

    fn resolve_field(&self, ctx: &EvalContext<'_>) -> Option<String> {
        match &self.field {
            Field::Content => ctx.content.map(|s| s.to_string()),
            Field::ToolName => ctx.tool_name.map(|s| s.to_string()),
            Field::Arg(key) => {
                let args = ctx.args?;
                // Support dotted paths: args.nested.key
                let mut current = args;
                for part in key.split('.') {
                    current = current.get(part)?;
                }
                match current {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    serde_json::Value::Bool(b) => Some(b.to_string()),
                    other => Some(other.to_string()),
                }
            }
        }
    }

    fn compare_num(&self, field_value: &Option<String>, cmp: impl Fn(f64, f64) -> bool) -> bool {
        match (&field_value, &self.value) {
            (Some(fv), Value::Num(n)) => fv.parse::<f64>().is_ok_and(|x| cmp(x, *n)),
            _ => false,
        }
    }
}

impl Value {
    fn as_str(&self) -> &str {
        match self {
            Value::Str(s) => s,
            Value::Num(n) => {
                // This is a workaround — numeric values rarely appear in
                // string comparisons, but if they do, we convert.
                // We use a leaked &str for simplicity — only happens in edge cases.
                Box::leak(n.to_string().into_boxed_str())
            }
        }
    }
}

// ─── Parser ──────────────────────────────────────────────────────────

/// Parse a condition expression string into a [`Condition`] tree.
///
/// Returns `Ok(Condition::Always)` for empty input.
pub fn parse_condition(input: &str) -> Result<Condition, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Condition::Always);
    }
    let tokens = tokenize(input)?;
    let (cond, rest) = parse_or(&tokens)?;
    if !rest.is_empty() {
        return Err(format!("unexpected tokens after expression: {rest:?}"));
    }
    Ok(cond)
}

/// Token types for the condition DSL.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Num(f64),
    And,
    Or,
    Not,
    // Operators
    Contains,
    Matches,
    StartsWith,
    EndsWith,
    Eq,
    NotEq,
    Gt,
    Lt,
    Gte,
    Lte,
    LParen,
    RParen,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            '"' | '\'' => {
                let quote = c;
                chars.next();
                let mut s = String::new();
                loop {
                    match chars.next() {
                        Some('\\') => {
                            if let Some(escaped) = chars.next() {
                                s.push(escaped);
                            }
                        }
                        Some(ch) if ch == quote => break,
                        Some(ch) => s.push(ch),
                        None => return Err("unterminated string literal".into()),
                    }
                }
                tokens.push(Token::Str(s));
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Gte);
                } else {
                    tokens.push(Token::Gt);
                }
            }
            '<' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::Lte);
                } else {
                    tokens.push(Token::Lt);
                }
            }
            '=' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                }
                tokens.push(Token::Eq);
            }
            '!' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::NotEq);
                } else {
                    tokens.push(Token::Not);
                }
            }
            _ if c.is_ascii_digit() || c == '-' => {
                let mut num_str = String::new();
                num_str.push(c);
                chars.next();
                while let Some(&nc) = chars.peek() {
                    if nc.is_ascii_digit() || nc == '.' {
                        num_str.push(nc);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match num_str.parse::<f64>() {
                    Ok(n) => tokens.push(Token::Num(n)),
                    Err(_) => return Err(format!("invalid number: {num_str}")),
                }
            }
            _ if c.is_alphanumeric() || c == '_' || c == '.' => {
                let mut word = String::new();
                while let Some(&wc) = chars.peek() {
                    if wc.is_alphanumeric() || wc == '_' || wc == '.' {
                        word.push(wc);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let token = match word.as_str() {
                    "AND" | "and" => Token::And,
                    "OR" | "or" => Token::Or,
                    "NOT" | "not" => Token::Not,
                    "CONTAINS" | "contains" => Token::Contains,
                    "MATCHES" | "matches" => Token::Matches,
                    "STARTS_WITH" | "starts_with" => Token::StartsWith,
                    "ENDS_WITH" | "ends_with" => Token::EndsWith,
                    _ => Token::Ident(word),
                };
                tokens.push(token);
            }
            _ => return Err(format!("unexpected character: {c}")),
        }
    }

    Ok(tokens)
}

fn parse_or(tokens: &[Token]) -> Result<(Condition, &[Token]), String> {
    let (mut left, mut rest) = parse_and(tokens)?;
    while rest.first() == Some(&Token::Or) {
        let (right, remaining) = parse_and(&rest[1..])?;
        left = Condition::Or(Box::new(left), Box::new(right));
        rest = remaining;
    }
    Ok((left, rest))
}

fn parse_and(tokens: &[Token]) -> Result<(Condition, &[Token]), String> {
    let (mut left, mut rest) = parse_unary(tokens)?;
    while rest.first() == Some(&Token::And) {
        let (right, remaining) = parse_unary(&rest[1..])?;
        left = Condition::And(Box::new(left), Box::new(right));
        rest = remaining;
    }
    Ok((left, rest))
}

fn parse_unary(tokens: &[Token]) -> Result<(Condition, &[Token]), String> {
    if tokens.first() == Some(&Token::Not) {
        // Check if NOT is part of an operator: NOT CONTAINS, NOT MATCHES, etc.
        // If the token after NOT is a field (ident), it's a logical NOT.
        // If it's an operator keyword, defer to atom parsing (handled inside parse_atom).
        // Actually, we handle NOT <operator> in parse_atom via negated ops.
        // Here we handle NOT as logical negation: NOT (expr) or NOT atom.
        if tokens.len() > 1 {
            match &tokens[1] {
                Token::LParen => {
                    let (inner, rest) = parse_primary(&tokens[1..])?;
                    return Ok((Condition::Not(Box::new(inner)), rest));
                }
                Token::Ident(_) => {
                    // Could be `NOT args.x CONTAINS "y"` — negate the whole atom.
                    let (inner, rest) = parse_atom(&tokens[1..])?;
                    return Ok((Condition::Not(Box::new(inner)), rest));
                }
                _ => {}
            }
        }
        let (inner, rest) = parse_primary(&tokens[1..])?;
        return Ok((Condition::Not(Box::new(inner)), rest));
    }
    parse_primary(tokens)
}

fn parse_primary(tokens: &[Token]) -> Result<(Condition, &[Token]), String> {
    if tokens.first() == Some(&Token::LParen) {
        let (inner, rest) = parse_or(&tokens[1..])?;
        if rest.first() != Some(&Token::RParen) {
            return Err("expected closing parenthesis".into());
        }
        return Ok((inner, &rest[1..]));
    }
    parse_atom(tokens)
}

fn parse_atom(tokens: &[Token]) -> Result<(Condition, &[Token]), String> {
    // field OP value
    // field NOT OP value  (negated operator)
    let (field, rest) = parse_field(tokens)?;

    let (op, rest) = parse_op(rest)?;

    let (value, rest) = parse_value(rest)?;

    Ok((Condition::Atom(Atom { field, op, value }), rest))
}

fn parse_field(tokens: &[Token]) -> Result<(Field, &[Token]), String> {
    match tokens.first() {
        Some(Token::Ident(name)) => {
            let field = if let Some(arg_name) = name.strip_prefix("args.") {
                Field::Arg(arg_name.to_string())
            } else {
                match name.as_str() {
                    "content" => Field::Content,
                    "tool_name" => Field::ToolName,
                    other => Field::Arg(other.to_string()),
                }
            };
            Ok((field, &tokens[1..]))
        }
        _ => Err(format!("expected field name, got {:?}", tokens.first())),
    }
}

fn parse_op(tokens: &[Token]) -> Result<(Op, &[Token]), String> {
    // Check for NOT <op> pattern.
    if tokens.first() == Some(&Token::Not) && tokens.len() > 1 {
        let (base_op, rest) = parse_base_op(&tokens[1..])?;
        let negated = match base_op {
            Op::Contains => Op::NotContains,
            Op::Matches => Op::NotMatches,
            Op::StartsWith => Op::NotStartsWith,
            Op::EndsWith => Op::NotEndsWith,
            other => {
                return Err(format!("cannot negate operator: {other:?}"));
            }
        };
        return Ok((negated, rest));
    }
    parse_base_op(tokens)
}

fn parse_base_op(tokens: &[Token]) -> Result<(Op, &[Token]), String> {
    match tokens.first() {
        Some(Token::Contains) => Ok((Op::Contains, &tokens[1..])),
        Some(Token::Matches) => Ok((Op::Matches, &tokens[1..])),
        Some(Token::StartsWith) => Ok((Op::StartsWith, &tokens[1..])),
        Some(Token::EndsWith) => Ok((Op::EndsWith, &tokens[1..])),
        Some(Token::Eq) => Ok((Op::Eq, &tokens[1..])),
        Some(Token::NotEq) => Ok((Op::NotEq, &tokens[1..])),
        Some(Token::Gt) => Ok((Op::Gt, &tokens[1..])),
        Some(Token::Lt) => Ok((Op::Lt, &tokens[1..])),
        Some(Token::Gte) => Ok((Op::Gte, &tokens[1..])),
        Some(Token::Lte) => Ok((Op::Lte, &tokens[1..])),
        _ => Err(format!("expected operator, got {:?}", tokens.first())),
    }
}

fn parse_value(tokens: &[Token]) -> Result<(Value, &[Token]), String> {
    match tokens.first() {
        Some(Token::Str(s)) => Ok((Value::Str(s.clone()), &tokens[1..])),
        Some(Token::Num(n)) => Ok((Value::Num(*n), &tokens[1..])),
        Some(Token::Ident(s)) => {
            // Bare identifier as a string value.
            Ok((Value::Str(s.clone()), &tokens[1..]))
        }
        _ => Err(format!(
            "expected value (string or number), got {:?}",
            tokens.first()
        )),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_args(args: serde_json::Value) -> EvalContext<'static> {
        // Leak for test convenience.
        let args = Box::leak(Box::new(args));
        EvalContext {
            args: Some(args),
            content: None,
            tool_name: Some("shell"),
        }
    }

    #[test]
    fn parse_simple_contains() {
        let cond = parse_condition(r#"args.command CONTAINS "rm -rf""#).unwrap();
        let ctx = ctx_with_args(serde_json::json!({"command": "rm -rf /"}));
        assert!(cond.evaluate(&ctx));

        let ctx2 = ctx_with_args(serde_json::json!({"command": "ls -la"}));
        assert!(!cond.evaluate(&ctx2));
    }

    #[test]
    fn parse_not_contains() {
        let cond = parse_condition(r#"args.command NOT CONTAINS "rm""#).unwrap();
        let ctx = ctx_with_args(serde_json::json!({"command": "ls -la"}));
        assert!(cond.evaluate(&ctx));

        let ctx2 = ctx_with_args(serde_json::json!({"command": "rm file.txt"}));
        assert!(!cond.evaluate(&ctx2));
    }

    #[test]
    fn parse_and_expression() {
        let cond = parse_condition(
            r#"args.command CONTAINS "rm" AND args.command NOT CONTAINS "workspace""#,
        )
        .unwrap();
        // rm outside workspace → true
        let ctx = ctx_with_args(serde_json::json!({"command": "rm /etc/passwd"}));
        assert!(cond.evaluate(&ctx));

        // rm inside workspace → false
        let ctx2 = ctx_with_args(serde_json::json!({"command": "rm workspace/tmp.txt"}));
        assert!(!cond.evaluate(&ctx2));

        // no rm → false
        let ctx3 = ctx_with_args(serde_json::json!({"command": "ls"}));
        assert!(!cond.evaluate(&ctx3));
    }

    #[test]
    fn parse_or_expression() {
        let cond = parse_condition(r#"args.command CONTAINS "rm" OR args.command CONTAINS "del""#)
            .unwrap();
        let ctx = ctx_with_args(serde_json::json!({"command": "del file.txt"}));
        assert!(cond.evaluate(&ctx));

        let ctx2 = ctx_with_args(serde_json::json!({"command": "rm file.txt"}));
        assert!(cond.evaluate(&ctx2));

        let ctx3 = ctx_with_args(serde_json::json!({"command": "ls"}));
        assert!(!cond.evaluate(&ctx3));
    }

    #[test]
    fn parse_regex_matches() {
        let cond = parse_condition(r#"args.url MATCHES "^https?://10\\.""#).unwrap();
        let ctx = ctx_with_args(serde_json::json!({"url": "http://10.0.0.1/admin"}));
        assert!(cond.evaluate(&ctx));

        let ctx2 = ctx_with_args(serde_json::json!({"url": "https://example.com"}));
        assert!(!cond.evaluate(&ctx2));
    }

    #[test]
    fn parse_numeric_comparison() {
        let cond = parse_condition("args.amount > 50").unwrap();
        let ctx = ctx_with_args(serde_json::json!({"amount": 100}));
        assert!(cond.evaluate(&ctx));

        let ctx2 = ctx_with_args(serde_json::json!({"amount": 30}));
        assert!(!cond.evaluate(&ctx2));
    }

    #[test]
    fn parse_starts_with() {
        let cond = parse_condition(r#"args.path STARTS_WITH "/etc""#).unwrap();
        let ctx = ctx_with_args(serde_json::json!({"path": "/etc/passwd"}));
        assert!(cond.evaluate(&ctx));

        let ctx2 = ctx_with_args(serde_json::json!({"path": "/home/user"}));
        assert!(!cond.evaluate(&ctx2));
    }

    #[test]
    fn parse_tool_name() {
        let cond = parse_condition(r#"tool_name == "shell""#).unwrap();
        let ctx = EvalContext {
            args: None,
            content: None,
            tool_name: Some("shell"),
        };
        assert!(cond.evaluate(&ctx));

        let ctx2 = EvalContext {
            args: None,
            content: None,
            tool_name: Some("file_read"),
        };
        assert!(!cond.evaluate(&ctx2));
    }

    #[test]
    fn parse_content_field() {
        let cond = parse_condition(r#"content CONTAINS "password""#).unwrap();
        let ctx = EvalContext {
            args: None,
            content: Some("Please enter your password here"),
            tool_name: None,
        };
        assert!(cond.evaluate(&ctx));
    }

    #[test]
    fn empty_condition_is_always() {
        let cond = parse_condition("").unwrap();
        let ctx = EvalContext {
            args: None,
            content: None,
            tool_name: None,
        };
        assert!(cond.evaluate(&ctx));
    }

    #[test]
    fn nested_json_args() {
        let cond = parse_condition(r#"args.config.mode == "dangerous""#).unwrap();
        let ctx = ctx_with_args(serde_json::json!({
            "config": {"mode": "dangerous"}
        }));
        assert!(cond.evaluate(&ctx));
    }

    #[test]
    fn invalid_condition_rejects() {
        assert!(parse_condition("CONTAINS").is_err());
        assert!(parse_condition(r#"args.x BADOP "y""#).is_err());
    }
}

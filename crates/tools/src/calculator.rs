//! Calculator tool — evaluates mathematical expressions.
//!
//! Supports basic arithmetic: `+`, `-`, `*`, `/`, parentheses, and
//! unary negation. Uses a recursive-descent parser for correctness.
//! No dependencies beyond std.

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};

pub struct CalculatorTool;

#[async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Evaluate a mathematical expression. Supports +, -, *, /, parentheses, and decimal numbers."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "The mathematical expression to evaluate, e.g. '(2 + 3) * 4'"
                }
            },
            "required": ["expression"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult, ToolError> {
        let expr = arguments["expression"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'expression' argument".into()))?;

        match evaluate(expr) {
            Ok(value) => {
                // Format nicely: remove trailing .0 for integers.
                let formatted = if value.fract() == 0.0 && value.abs() < 1e15 {
                    format!("{}", value as i64)
                } else {
                    format!("{}", value)
                };
                Ok(ToolResult {
                    call_id: String::new(),
                    success: true,
                    output: formatted,
                    data: Some(serde_json::json!({"result": value})),
                })
            }
            Err(e) => Ok(ToolResult {
                call_id: String::new(),
                success: false,
                output: format!("Error: {}", e),
                data: None,
            }),
        }
    }
}

// ── Recursive-descent expression evaluator ────────────────────────────────

/// Evaluate a mathematical expression string.
pub fn evaluate(expr: &str) -> Result<f64, String> {
    let tokens = tokenize(expr)?;
    let mut parser = Parser::new(&tokens);
    let result = parser.parse_expr()?;
    if parser.pos < parser.tokens.len() {
        return Err(format!(
            "Unexpected token at position {}: {:?}",
            parser.pos, parser.tokens[parser.pos]
        ));
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => { tokens.push(Token::Plus); i += 1; }
            '-' => { tokens.push(Token::Minus); i += 1; }
            '*' => { tokens.push(Token::Star); i += 1; }
            '/' => { tokens.push(Token::Slash); i += 1; }
            '(' => { tokens.push(Token::LParen); i += 1; }
            ')' => { tokens.push(Token::RParen); i += 1; }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let num_str: String = chars[start..i].iter().collect();
                let num: f64 = num_str
                    .parse()
                    .map_err(|_| format!("Invalid number: {}", num_str))?;
                tokens.push(Token::Number(num));
            }
            c => return Err(format!("Unexpected character: '{}'", c)),
        }
    }

    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn consume(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    // expr = term (('+' | '-') term)*
    fn parse_expr(&mut self) -> Result<f64, String> {
        let mut left = self.parse_term()?;
        while let Some(op) = self.peek() {
            match op {
                Token::Plus => {
                    self.consume();
                    left += self.parse_term()?;
                }
                Token::Minus => {
                    self.consume();
                    left -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    // term = unary (('*' | '/') unary)*
    fn parse_term(&mut self) -> Result<f64, String> {
        let mut left = self.parse_unary()?;
        while let Some(op) = self.peek() {
            match op {
                Token::Star => {
                    self.consume();
                    left *= self.parse_unary()?;
                }
                Token::Slash => {
                    self.consume();
                    let right = self.parse_unary()?;
                    if right == 0.0 {
                        return Err("Division by zero".into());
                    }
                    left /= right;
                }
                _ => break,
            }
        }
        Ok(left)
    }

    // unary = '-' unary | primary
    fn parse_unary(&mut self) -> Result<f64, String> {
        if let Some(Token::Minus) = self.peek() {
            self.consume();
            let val = self.parse_unary()?;
            return Ok(-val);
        }
        self.parse_primary()
    }

    // primary = NUMBER | '(' expr ')'
    fn parse_primary(&mut self) -> Result<f64, String> {
        match self.consume() {
            Some(Token::Number(n)) => Ok(*n),
            Some(Token::LParen) => {
                let val = self.parse_expr()?;
                match self.consume() {
                    Some(Token::RParen) => Ok(val),
                    _ => Err("Expected closing parenthesis".into()),
                }
            }
            Some(tok) => Err(format!("Unexpected token: {:?}", tok)),
            None => Err("Unexpected end of expression".into()),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_addition() {
        assert_eq!(evaluate("2 + 3").unwrap(), 5.0);
    }

    #[test]
    fn operator_precedence() {
        assert_eq!(evaluate("2 + 3 * 4").unwrap(), 14.0);
    }

    #[test]
    fn parentheses() {
        assert_eq!(evaluate("(2 + 3) * 4").unwrap(), 20.0);
    }

    #[test]
    fn nested_parentheses() {
        assert_eq!(evaluate("((1 + 2) * (3 + 4))").unwrap(), 21.0);
    }

    #[test]
    fn division() {
        assert_eq!(evaluate("10 / 4").unwrap(), 2.5);
    }

    #[test]
    fn division_by_zero() {
        assert!(evaluate("1 / 0").is_err());
    }

    #[test]
    fn unary_negation() {
        assert_eq!(evaluate("-5 + 3").unwrap(), -2.0);
    }

    #[test]
    fn decimals() {
        assert_eq!(evaluate("3.14 * 2").unwrap(), 6.28);
    }

    #[test]
    fn complex_expression() {
        let result = evaluate("(10 + 5) / 3 - 2 * (1 + 1)").unwrap();
        assert!((result - 1.0).abs() < 1e-10);
    }

    #[test]
    fn invalid_expression() {
        assert!(evaluate("2 +").is_err());
    }

    #[test]
    fn empty_expression() {
        assert!(evaluate("").is_err());
    }

    #[tokio::test]
    async fn tool_execute() {
        let tool = CalculatorTool;
        let result = tool
            .execute(serde_json::json!({"expression": "2 + 3"}))
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.output, "5");
    }

    #[tokio::test]
    async fn tool_formats_integers() {
        let tool = CalculatorTool;
        let result = tool
            .execute(serde_json::json!({"expression": "10 / 2"}))
            .await
            .unwrap();

        assert_eq!(result.output, "5");
    }

    #[tokio::test]
    async fn tool_formats_decimals() {
        let tool = CalculatorTool;
        let result = tool
            .execute(serde_json::json!({"expression": "10 / 3"}))
            .await
            .unwrap();

        assert!(result.output.starts_with("3.333"));
    }

    #[tokio::test]
    async fn tool_missing_expression() {
        let tool = CalculatorTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn tool_definition() {
        let tool = CalculatorTool;
        let def = tool.to_definition();
        assert_eq!(def.name, "calculator");
    }
}

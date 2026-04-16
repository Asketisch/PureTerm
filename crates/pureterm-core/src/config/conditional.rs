//! Conditional configuration parsing
//!
//! Supports if/else blocks based on theme, OS, and other conditions.

use std::collections::HashMap;

/// A conditional configuration block
#[derive(Debug, Clone)]
pub enum ConditionalBlock {
    If {
        condition: Condition,
        then_body: Vec<ConditionalBlock>,
        else_body: Option<Vec<ConditionalBlock>>,
    },
    KeyValue(ConfigEntry),
}

/// A condition for conditional config
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    ThemeEqual(String),
    ThemeNotEqual(String),
    OsEqual(String),
    OsNotEqual(String),
    EnvVar {
        name: String,
        value: Option<String>,
    },
    Always,
}

/// A key-value config entry
#[derive(Debug, Clone)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

/// Parsed conditional configuration
#[derive(Debug, Clone, Default)]
pub struct ConditionalConfig {
    pub blocks: Vec<ConditionalBlock>,
}

/// Parse a conditional config string
///
/// Example:
/// ```ini
/// if theme == light {
///   background = "#ffffff"
/// } else {
///   background = "#000000"
/// }
/// ```
pub fn parse_conditional(content: &str) -> Result<ConditionalConfig, ParseError> {
    let mut parser = Parser::new(content);
    parser.parse()
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.line, self.message)
    }
}

struct Parser<'a> {
    content: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Parser<'a> {
    fn new(content: &'a str) -> Self {
        Self {
            content,
            pos: 0,
            line: 1,
        }
    }

    fn parse(&mut self) -> Result<ConditionalConfig, ParseError> {
        let mut blocks = Vec::new();
        self.skip_whitespace_and_comments();

        while self.pos < self.content.len() {
            if self.peek_is("if") {
                let block = self.parse_if_block()?;
                blocks.push(block);
            } else {
                let entry = self.parse_key_value()?;
                blocks.push(ConditionalBlock::KeyValue(entry));
            }
            self.skip_whitespace_and_comments();
        }

        Ok(ConditionalConfig { blocks })
    }

    fn parse_if_block(&mut self) -> Result<ConditionalBlock, ParseError> {
        // Parse "if"
        self.expect("if")?;

        // Parse condition
        let condition = self.parse_condition()?;

        // Parse "{"
        self.skip_whitespace();
        self.expect("{")?;

        // Parse then body
        let then_body = self.parse_block_body()?;

        // Parse "}"
        self.skip_whitespace();
        self.expect("}")?;

        // Check for else
        self.skip_whitespace();
        if self.peek_is("else") {
            self.expect("else")?;
            self.skip_whitespace();
            if self.peek_is("{") {
                self.expect("{")?;
                let else_blocks = self.parse_block_body()?;
                self.skip_whitespace();
                self.expect("}")?;
                Ok(ConditionalBlock::If {
                    condition,
                    then_body,
                    else_body: Some(else_blocks),
                })
            } else {
                // else if - parse recursively
                let nested = self.parse_if_block()?;
                Ok(ConditionalBlock::If {
                    condition,
                    then_body,
                    else_body: Some(vec![nested]),
                })
            }
        } else {
            Ok(ConditionalBlock::If {
                condition,
                then_body,
                else_body: None,
            })
        }
    }

    fn parse_condition(&mut self) -> Result<Condition, ParseError> {
        self.skip_whitespace();

        // Check for theme condition
        if self.peek_is("theme") {
            self.expect("theme")?;
            self.skip_whitespace();
            let negated = self.peek_is("!=");
            if negated {
                self.expect("!=")?;
            } else {
                self.expect("==")?;
            }
            self.skip_whitespace();
            let value = self.parse_ident_or_string()?;
            if negated {
                Ok(Condition::ThemeNotEqual(value))
            } else {
                Ok(Condition::ThemeEqual(value))
            }
        }
        // Check for os condition
        else if self.peek_is("os") {
            self.expect("os")?;
            self.skip_whitespace();
            let negated = self.peek_is("!=");
            if negated {
                self.expect("!=")?;
            } else {
                self.expect("==")?;
            }
            self.skip_whitespace();
            let value = self.parse_ident_or_string()?;
            if negated {
                Ok(Condition::OsNotEqual(value))
            } else {
                Ok(Condition::OsEqual(value))
            }
        }
        // Check for env condition
        else if self.peek_is("env") {
            self.expect("env")?;
            self.skip_whitespace();
            let name = self.parse_ident()?;
            self.skip_whitespace();
            let value = if self.peek_is("==") {
                self.expect("==")?;
                self.skip_whitespace();
                Some(self.parse_ident_or_string()?)
            } else {
                None
            };
            Ok(Condition::EnvVar { name, value })
        } else {
            Err(ParseError {
                message: "Expected condition (theme, os, or env)".to_string(),
                line: self.line,
            })
        }
    }

    fn parse_block_body(&mut self) -> Result<Vec<ConditionalBlock>, ParseError> {
        let mut entries = Vec::new();
        self.skip_whitespace_and_comments();

        while !self.at_end() && !self.peek_is("}") {
            if self.peek_is("if") {
                let block = self.parse_if_block()?;
                entries.push(block);
            } else {
                let entry = self.parse_key_value()?;
                entries.push(ConditionalBlock::KeyValue(entry));
            }
            self.skip_whitespace_and_comments();
        }

        Ok(entries)
    }

    fn parse_key_value(&mut self) -> Result<ConfigEntry, ParseError> {
        self.skip_whitespace();
        let key = self.parse_ident_or_string()?;
        self.skip_whitespace();
        self.expect("=")?;
        self.skip_whitespace();
        let value = self.parse_value()?;
        self.skip_whitespace_and_comments();

        Ok(ConfigEntry { key, value })
    }

    fn parse_ident_or_string(&mut self) -> Result<String, ParseError> {
        self.skip_whitespace();

        if self.peek_is("\"") {
            self.parse_string()
        } else {
            self.parse_ident()
        }
    }

    fn parse_ident(&mut self) -> Result<String, ParseError> {
        self.skip_whitespace();
        let start = self.pos;

        while !self.at_end() && !self.content[self.pos..].starts_with(" if ")
            && !self.content[self.pos..].starts_with(" else ")
            && !self.content[self.pos..].starts_with('}')
            && !self.content[self.pos..].starts_with(' ')
            && !self.content[self.pos..].starts_with('\t')
            && !self.content[self.pos..].starts_with('\n')
            && !self.content[self.pos..].starts_with("==")
            && !self.content[self.pos..].starts_with("!=")
        {
            self.pos += 1;
        }

        if start == self.pos {
            Err(ParseError {
                message: "Expected identifier".to_string(),
                line: self.line,
            })
        } else {
            Ok(self.content[start..self.pos].to_string())
        }
    }

    fn parse_string(&mut self) -> Result<String, ParseError> {
        self.skip_whitespace();
        self.expect("\"")?;
        let start = self.pos;
        while !self.at_end() && self.content[self.pos..].chars().next() != Some('"') {
            if self.content[self.pos..].starts_with("\\\"") {
                self.pos += 2;
            } else {
                self.pos += 1;
            }
        }
        let value = self.content[start..self.pos].to_string();
        self.expect("\"")?;
        Ok(value)
    }

    fn parse_value(&mut self) -> Result<String, ParseError> {
        self.skip_whitespace();
        let start = self.pos;

        // Check if value starts with a quote
        let quoted = self.content[self.pos..].starts_with('"');

        // Simple value parsing - read until newline or comment
        while !self.at_end() {
            let ch = self.content[self.pos..].chars().next().unwrap();
            if ch == '\n' {
                break;
            }
            if !quoted && ch == '#' && (start == self.pos || self.content[self.pos - 1..].starts_with(' ')
                || self.content[self.pos - 1..].starts_with('\t'))
            {
                break;
            }
            self.pos += 1;
        }

        let mut value = self.content[start..self.pos]
            .trim_end()
            .trim()
            .to_string();

        // Strip surrounding quotes
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value = value[1..value.len()-1].to_string();
        }

        if value.is_empty() {
            Err(ParseError {
                message: "Expected value".to_string(),
                line: self.line,
            })
        } else {
            Ok(value)
        }
    }

    fn skip_whitespace(&mut self) {
        while !self.at_end() {
            let ch = self.content[self.pos..].chars().next().unwrap();
            match ch {
                ' ' | '\t' => self.pos += 1,
                '\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                _ => break,
            }
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            self.skip_whitespace();
            if self.content[self.pos..].starts_with('#') {
                while !self.at_end() && self.content[self.pos..].chars().next() != Some('\n') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn peek_is(&self, s: &str) -> bool {
        self.content[self.pos..].starts_with(s)
    }

    fn expect(&mut self, s: &str) -> Result<(), ParseError> {
        if self.content[self.pos..].starts_with(s) {
            self.pos += s.len();
            Ok(())
        } else {
            Err(ParseError {
                message: format!("Expected '{}'", s),
                line: if s == "\n" { self.line + 1 } else { self.line },
            })
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.content.len()
    }
}

/// Evaluate conditional blocks based on context
pub struct ConditionalEvaluator {
    theme: String,
    os: String,
    env: HashMap<String, String>,
}

impl ConditionalEvaluator {
    pub fn new(theme: &str) -> Self {
        Self {
            theme: theme.to_string(),
            os: std::env::consts::OS.to_string(),
            env: std::env::vars().collect(),
        }
    }

    pub fn evaluate(&self, config: &ConditionalConfig) -> HashMap<String, String> {
        let mut result = HashMap::new();
        for block in &config.blocks {
            self.evaluate_block(block, &mut result);
        }
        result
    }

    fn evaluate_block(&self, block: &ConditionalBlock, result: &mut HashMap<String, String>) {
        match block {
            ConditionalBlock::If {
                condition,
                then_body,
                else_body,
            } => {
                if self.check_condition(condition) {
                    for entry in then_body {
                        if let ConditionalBlock::KeyValue(e) = entry {
                            if !e.key.is_empty() {
                                result.insert(e.key.clone(), e.value.clone());
                            }
                        }
                    }
                } else if let Some(else_blocks) = else_body {
                    for nested in else_blocks {
                        self.evaluate_block(nested, result);
                    }
                }
            }
            ConditionalBlock::KeyValue(entry) => {
                if !entry.key.is_empty() {
                    result.insert(entry.key.clone(), entry.value.clone());
                }
            }
        }
    }

    fn check_condition(&self, condition: &Condition) -> bool {
        match condition {
            Condition::ThemeEqual(s) => self.theme == *s,
            Condition::ThemeNotEqual(s) => self.theme != *s,
            Condition::OsEqual(s) => self.os == *s,
            Condition::OsNotEqual(s) => self.os != *s,
            Condition::EnvVar { name, value } => {
                if let Some(v) = self.env.get(name) {
                    if let Some(expected) = value {
                        v == expected
                    } else {
                        true
                    }
                } else {
                    false
                }
            }
            Condition::Always => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let content = r#"
            font-size = 14
            title = "My Terminal"
        "#;
        let config = parse_conditional(content).unwrap();
        assert_eq!(config.blocks.len(), 2);
    }

    #[test]
    fn test_parse_conditional() {
        let content = r##"
            if theme == light {
                background = "#ffffff"
            } else {
                background = "#000000"
            }
        "##;
        let config = parse_conditional(content).unwrap();
        assert_eq!(config.blocks.len(), 1);
    }

    #[test]
    fn test_evaluate_theme() {
        let content = r##"
            if theme == light {
                background = "#ffffff"
            } else {
                background = "#000000"
            }
        "##;
        let config = parse_conditional(content).unwrap();
        let evaluator = ConditionalEvaluator::new("dark");
        let result = evaluator.evaluate(&config);
        assert_eq!(result.get("background"), Some(&"#000000".to_string()));
    }
}

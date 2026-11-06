//! The workflow script language.
//!
//! One lexer and expression grammar serve two modes: statement programs for
//! `action`, and boolean expressions for transition `on`. The compiler resolves
//! variables, types, roles and workflow references; runtime evaluation is a
//! pure function of the task snapshot and the current stage variables.

use crate::ChildState;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const MAX_SOURCE_BYTES: usize = 16 * 1024;
pub const MAX_DEPTH: usize = 32;
pub const MAX_STATEMENTS: usize = 1024;

/// The value type of a stage variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VarType {
    Bool,
    String,
    Int,
    ActorSet,
}

impl VarType {
    pub fn default_value(self) -> VariableValue {
        match self {
            Self::Bool => VariableValue::Bool(false),
            Self::String => VariableValue::String(String::new()),
            Self::Int => VariableValue::Int(0),
            Self::ActorSet => VariableValue::ActorSet(BTreeSet::new()),
        }
    }
}

/// A persisted stage variable value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariableValue {
    Bool(bool),
    String(String),
    Int(i64),
    ActorSet(BTreeSet<Uuid>),
}

impl VariableValue {
    pub fn var_type(&self) -> VarType {
        match self {
            Self::Bool(_) => VarType::Bool,
            Self::String(_) => VarType::String,
            Self::Int(_) => VarType::Int,
            Self::ActorSet(_) => VarType::ActorSet,
        }
    }
}

impl Serialize for VariableValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::String(value) => serializer.serialize_str(value),
            Self::Int(value) => serializer.serialize_i64(*value),
            Self::ActorSet(values) => values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for VariableValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Bool(value) => Ok(Self::Bool(value)),
            serde_json::Value::String(value) => Ok(Self::String(value)),
            serde_json::Value::Number(number) => number
                .as_i64()
                .map(Self::Int)
                .ok_or_else(|| serde::de::Error::custom("variable integer is out of range")),
            serde_json::Value::Array(values) => values
                .into_iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| serde::de::Error::custom("actor set must contain strings"))?
                        .parse::<Uuid>()
                        .map_err(|error| serde::de::Error::custom(error.to_string()))
                })
                .collect::<Result<BTreeSet<_>, _>>()
                .map(Self::ActorSet),
            _ => Err(serde::de::Error::custom(
                "variable must be a boolean, integer, string or actor set",
            )),
        }
    }
}

/// Which trigger's event fields a script may read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventAccess {
    /// `on`: no event data at all.
    Condition,
    /// An operation action: only `event.actor`.
    Operation,
    /// An event action: the fields published by that event.
    Event(EventFieldSet),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventFieldSet {
    None,
    TaskStateChanged,
    ChildCreated,
    ChildStateChanged,
}

/// A compile-time view of the stage in which a script runs.
pub struct ScriptScope<'a> {
    pub variables: &'a BTreeMap<String, VarType>,
    pub roles: &'a [String],
    pub active_roles: &'a [String],
    pub known_workflow: &'a dyn Fn(&str) -> bool,
    pub event: EventAccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptIssue {
    pub code: &'static str,
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl ScriptIssue {
    fn new(code: &'static str, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            code,
            message: format!("line {line}, column {column}: {}", message.into()),
            line,
            column,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompiledAction {
    statements: Vec<Statement>,
    pub reads: BTreeSet<String>,
    pub writes: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct CompiledExpression {
    expression: Expression,
    pub reads: BTreeSet<String>,
}

/// The reserved words of the language. Variables may not use these names.
pub const RESERVED: &[&str] = &[
    "if",
    "else",
    "add",
    "to",
    "and",
    "or",
    "not",
    "true",
    "false",
    "null",
    "task",
    "event",
    "active_actors",
    "count_children",
    "result",
];

pub fn is_reserved(name: &str) -> bool {
    RESERVED.contains(&name)
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    Int(i64),
    Str(String),
    True,
    False,
    Null,
    If,
    Else,
    Add,
    To,
    And,
    Or,
    Not,
    Task,
    Event,
    ActiveActors,
    CountChildren,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Assign,
    Plus,
    Minus,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Superset,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    line: usize,
    column: usize,
}

fn lex(source: &str) -> Result<Vec<Token>, ScriptIssue> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;
    let mut column = 1;

    let advance = |ch: char, line: &mut usize, column: &mut usize| {
        if ch == '\n' {
            *line += 1;
            *column = 1;
        } else {
            *column += 1;
        }
    };

    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            advance(ch, &mut line, &mut column);
            index += 1;
            continue;
        }
        if ch == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && chars[index] != '\n' {
                advance(chars[index], &mut line, &mut column);
                index += 1;
            }
            continue;
        }

        let token_line = line;
        let token_column = column;
        let single = |kind: TokenKind| Token {
            kind,
            line: token_line,
            column: token_column,
        };

        if ch.is_ascii_lowercase() {
            let mut text = String::new();
            while index < chars.len() {
                let current = chars[index];
                if current.is_ascii_lowercase() || current.is_ascii_digit() || current == '_' {
                    text.push(current);
                    advance(current, &mut line, &mut column);
                    index += 1;
                } else {
                    break;
                }
            }
            let kind = match text.as_str() {
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "null" => TokenKind::Null,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "add" => TokenKind::Add,
                "to" => TokenKind::To,
                "and" => TokenKind::And,
                "or" => TokenKind::Or,
                "not" => TokenKind::Not,
                "task" => TokenKind::Task,
                "event" => TokenKind::Event,
                "active_actors" => TokenKind::ActiveActors,
                "count_children" => TokenKind::CountChildren,
                _ => TokenKind::Ident(text),
            };
            tokens.push(single(kind));
            continue;
        }

        if ch.is_ascii_digit() {
            let mut text = String::new();
            while index < chars.len() && chars[index].is_ascii_digit() {
                text.push(chars[index]);
                advance(chars[index], &mut line, &mut column);
                index += 1;
            }
            if text.len() > 1 && text.starts_with('0') {
                return Err(ScriptIssue::new(
                    "script_syntax_error",
                    token_line,
                    token_column,
                    "integer literal must not have leading zeroes",
                ));
            }
            let value = text.parse::<i64>().map_err(|_| {
                ScriptIssue::new(
                    "script_syntax_error",
                    token_line,
                    token_column,
                    "integer literal is out of range",
                )
            })?;
            tokens.push(single(TokenKind::Int(value)));
            continue;
        }

        if ch == '\'' {
            advance(ch, &mut line, &mut column);
            index += 1;
            let mut text = String::new();
            let mut closed = false;
            while index < chars.len() {
                let current = chars[index];
                if current == '\'' {
                    advance(current, &mut line, &mut column);
                    index += 1;
                    closed = true;
                    break;
                }
                if current == '\n' {
                    return Err(ScriptIssue::new(
                        "script_syntax_error",
                        token_line,
                        token_column,
                        "string literal must not contain a newline",
                    ));
                }
                if current == '\\' {
                    let Some(&escaped) = chars.get(index + 1) else {
                        break;
                    };
                    let value = match escaped {
                        '\\' => '\\',
                        '\'' => '\'',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        other => {
                            return Err(ScriptIssue::new(
                                "script_syntax_error",
                                line,
                                column,
                                format!("unknown string escape \\{other}"),
                            ));
                        }
                    };
                    text.push(value);
                    advance(current, &mut line, &mut column);
                    advance(escaped, &mut line, &mut column);
                    index += 2;
                    continue;
                }
                text.push(current);
                advance(current, &mut line, &mut column);
                index += 1;
            }
            if !closed {
                return Err(ScriptIssue::new(
                    "script_syntax_error",
                    token_line,
                    token_column,
                    "unterminated string literal",
                ));
            }
            tokens.push(single(TokenKind::Str(text)));
            continue;
        }

        let next = chars.get(index + 1).copied();
        let (kind, width) = match ch {
            '(' => (TokenKind::LParen, 1),
            ')' => (TokenKind::RParen, 1),
            '{' => (TokenKind::LBrace, 1),
            '}' => (TokenKind::RBrace, 1),
            ',' => (TokenKind::Comma, 1),
            '.' => (TokenKind::Dot, 1),
            '+' => (TokenKind::Plus, 1),
            '-' => (TokenKind::Minus, 1),
            '⊇' => (TokenKind::Superset, 1),
            '=' if next == Some('=') => (TokenKind::EqEq, 2),
            '=' => (TokenKind::Assign, 1),
            '!' if next == Some('=') => (TokenKind::NotEq, 2),
            '<' if next == Some('=') => (TokenKind::Le, 2),
            '<' => (TokenKind::Lt, 1),
            '>' if next == Some('=') => (TokenKind::Ge, 2),
            '>' => (TokenKind::Gt, 1),
            other => {
                return Err(ScriptIssue::new(
                    "script_syntax_error",
                    token_line,
                    token_column,
                    format!("unexpected character {other:?}"),
                ));
            }
        };
        for _ in 0..width {
            advance(chars[index], &mut line, &mut column);
            index += 1;
        }
        tokens.push(single(kind));
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        line,
        column,
    });
    Ok(tokens)
}

// ---------------------------------------------------------------------------
// Syntax tree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Expression {
    kind: ExpressionKind,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone)]
enum ExpressionKind {
    Bool(bool),
    Int(i64),
    Str(String),
    Null,
    Variable(String),
    Path {
        root: PathRoot,
        segments: Vec<String>,
    },
    ActiveActors(String),
    CountChildren(String),
    Not(Box<Expression>),
    And(Box<Expression>, Box<Expression>),
    Or(Box<Expression>, Box<Expression>),
    Compare(CompareOp, Box<Expression>, Box<Expression>),
    Add(Box<Expression>, Box<Expression>),
    Subtract(Box<Expression>, Box<Expression>),
    Negate(Box<Expression>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathRoot {
    Task,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Superset,
}

#[derive(Debug, Clone)]
enum Statement {
    Assign {
        name: String,
        expression: Expression,
        line: usize,
        column: usize,
    },
    Add {
        expression: Expression,
        name: String,
        line: usize,
        column: usize,
    },
    If {
        condition: Expression,
        then_branch: Vec<Statement>,
        else_branch: Option<Vec<Statement>>,
        line: usize,
        column: usize,
    },
    Block(Vec<Statement>),
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    depth: usize,
    statements: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            index: 0,
            depth: 0,
            statements: 0,
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn next(&mut self) -> Token {
        let token = self.tokens[self.index].clone();
        if !matches!(token.kind, TokenKind::Eof) {
            self.index += 1;
        }
        token
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind) {
            self.next();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Token, ScriptIssue> {
        let token = self.peek().clone();
        if std::mem::discriminant(&token.kind) == std::mem::discriminant(kind) {
            Ok(self.next())
        } else {
            Err(ScriptIssue::new(
                "script_syntax_error",
                token.line,
                token.column,
                format!("expected {what}"),
            ))
        }
    }

    fn enter_depth(&mut self, line: usize, column: usize) -> Result<(), ScriptIssue> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(ScriptIssue::new(
                "script_too_deep",
                line,
                column,
                format!("script nesting exceeds {MAX_DEPTH}"),
            ));
        }
        Ok(())
    }

    fn leave_depth(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn count_statement(&mut self, line: usize, column: usize) -> Result<(), ScriptIssue> {
        self.statements += 1;
        if self.statements > MAX_STATEMENTS {
            return Err(ScriptIssue::new(
                "script_too_many_statements",
                line,
                column,
                format!("a program may contain at most {MAX_STATEMENTS} statements"),
            ));
        }
        Ok(())
    }

    fn program(&mut self) -> Result<Vec<Statement>, ScriptIssue> {
        let mut statements = Vec::new();
        while !matches!(self.peek().kind, TokenKind::Eof) {
            statements.push(self.statement()?);
        }
        Ok(statements)
    }

    fn statement(&mut self) -> Result<Statement, ScriptIssue> {
        let token = self.peek().clone();
        self.count_statement(token.line, token.column)?;
        match token.kind {
            TokenKind::LBrace => self.block(),
            TokenKind::If => self.if_statement(),
            TokenKind::Add => self.add_statement(),
            TokenKind::Ident(ref name) => {
                let name = name.clone();
                self.next();
                if !self.eat(&TokenKind::Assign) {
                    return Err(ScriptIssue::new(
                        "script_statement_not_allowed",
                        token.line,
                        token.column,
                        "a bare expression is not a statement",
                    ));
                }
                let expression = self.expression()?;
                Ok(Statement::Assign {
                    name,
                    expression,
                    line: token.line,
                    column: token.column,
                })
            }
            _ => Err(ScriptIssue::new(
                "script_syntax_error",
                token.line,
                token.column,
                "expected a statement",
            )),
        }
    }

    fn block(&mut self) -> Result<Statement, ScriptIssue> {
        let open = self.expect(&TokenKind::LBrace, "{")?;
        self.enter_depth(open.line, open.column)?;
        let mut statements = Vec::new();
        while !matches!(self.peek().kind, TokenKind::RBrace | TokenKind::Eof) {
            statements.push(self.statement()?);
        }
        if matches!(self.peek().kind, TokenKind::Eof) {
            return Err(ScriptIssue::new(
                "script_syntax_error",
                open.line,
                open.column,
                "unclosed block",
            ));
        }
        self.next();
        self.leave_depth();
        Ok(Statement::Block(statements))
    }

    fn if_statement(&mut self) -> Result<Statement, ScriptIssue> {
        let keyword = self.expect(&TokenKind::If, "if")?;
        let condition = self.expression()?;
        let then_branch = self.block_statements()?;
        let else_branch = if self.eat(&TokenKind::Else) {
            if matches!(self.peek().kind, TokenKind::If) {
                let nested = self.if_statement()?;
                Some(vec![nested])
            } else {
                Some(self.block_statements()?)
            }
        } else {
            None
        };
        Ok(Statement::If {
            condition,
            then_branch,
            else_branch,
            line: keyword.line,
            column: keyword.column,
        })
    }

    fn block_statements(&mut self) -> Result<Vec<Statement>, ScriptIssue> {
        match self.block()? {
            Statement::Block(statements) => Ok(statements),
            _ => unreachable!(),
        }
    }

    fn add_statement(&mut self) -> Result<Statement, ScriptIssue> {
        let keyword = self.expect(&TokenKind::Add, "add")?;
        let expression = self.expression()?;
        self.expect(&TokenKind::To, "to")?;
        let target = self.peek().clone();
        let TokenKind::Ident(name) = target.kind else {
            return Err(ScriptIssue::new(
                "script_syntax_error",
                target.line,
                target.column,
                "expected a variable after to",
            ));
        };
        self.next();
        Ok(Statement::Add {
            expression,
            name,
            line: keyword.line,
            column: keyword.column,
        })
    }

    fn expression(&mut self) -> Result<Expression, ScriptIssue> {
        let token = self.peek().clone();
        self.enter_depth(token.line, token.column)?;
        let result = self.or_expression();
        self.leave_depth();
        result
    }

    fn or_expression(&mut self) -> Result<Expression, ScriptIssue> {
        let mut left = self.and_expression()?;
        while matches!(self.peek().kind, TokenKind::Or) {
            let token = self.next();
            let right = self.and_expression()?;
            left = Expression {
                kind: ExpressionKind::Or(Box::new(left), Box::new(right)),
                line: token.line,
                column: token.column,
            };
        }
        Ok(left)
    }

    fn and_expression(&mut self) -> Result<Expression, ScriptIssue> {
        let mut left = self.not_expression()?;
        while matches!(self.peek().kind, TokenKind::And) {
            let token = self.next();
            let right = self.not_expression()?;
            left = Expression {
                kind: ExpressionKind::And(Box::new(left), Box::new(right)),
                line: token.line,
                column: token.column,
            };
        }
        Ok(left)
    }

    fn not_expression(&mut self) -> Result<Expression, ScriptIssue> {
        if matches!(self.peek().kind, TokenKind::Not) {
            let token = self.next();
            let operand = self.not_expression()?;
            return Ok(Expression {
                kind: ExpressionKind::Not(Box::new(operand)),
                line: token.line,
                column: token.column,
            });
        }
        let left = self.additive()?;
        let Some(operator) = compare_operator(&self.peek().kind) else {
            return Ok(left);
        };
        let token = self.next();
        let right = self.additive()?;
        if compare_operator(&self.peek().kind).is_some() {
            let extra = self.peek();
            return Err(ScriptIssue::new(
                "script_syntax_error",
                extra.line,
                extra.column,
                "comparison operators are not associative; use parentheses",
            ));
        }
        Ok(Expression {
            kind: ExpressionKind::Compare(operator, Box::new(left), Box::new(right)),
            line: token.line,
            column: token.column,
        })
    }

    fn additive(&mut self) -> Result<Expression, ScriptIssue> {
        let mut left = self.unary()?;
        loop {
            let kind = match self.peek().kind {
                TokenKind::Plus => true,
                TokenKind::Minus => false,
                _ => break,
            };
            let token = self.next();
            let right = self.unary()?;
            left = Expression {
                kind: if kind {
                    ExpressionKind::Add(Box::new(left), Box::new(right))
                } else {
                    ExpressionKind::Subtract(Box::new(left), Box::new(right))
                },
                line: token.line,
                column: token.column,
            };
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expression, ScriptIssue> {
        if matches!(self.peek().kind, TokenKind::Minus) {
            let token = self.next();
            let operand = self.unary()?;
            return Ok(Expression {
                kind: ExpressionKind::Negate(Box::new(operand)),
                line: token.line,
                column: token.column,
            });
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expression, ScriptIssue> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::True => {
                self.next();
                Ok(Expression {
                    kind: ExpressionKind::Bool(true),
                    line: token.line,
                    column: token.column,
                })
            }
            TokenKind::False => {
                self.next();
                Ok(Expression {
                    kind: ExpressionKind::Bool(false),
                    line: token.line,
                    column: token.column,
                })
            }
            TokenKind::Null => {
                self.next();
                Ok(Expression {
                    kind: ExpressionKind::Null,
                    line: token.line,
                    column: token.column,
                })
            }
            TokenKind::Int(value) => {
                self.next();
                Ok(Expression {
                    kind: ExpressionKind::Int(value),
                    line: token.line,
                    column: token.column,
                })
            }
            TokenKind::Str(value) => {
                self.next();
                Ok(Expression {
                    kind: ExpressionKind::Str(value),
                    line: token.line,
                    column: token.column,
                })
            }
            TokenKind::Ident(name) => {
                self.next();
                Ok(Expression {
                    kind: ExpressionKind::Variable(name),
                    line: token.line,
                    column: token.column,
                })
            }
            TokenKind::Task | TokenKind::Event => {
                let root = if matches!(token.kind, TokenKind::Task) {
                    PathRoot::Task
                } else {
                    PathRoot::Event
                };
                self.next();
                let mut segments = Vec::new();
                while self.eat(&TokenKind::Dot) {
                    let segment = self.peek().clone();
                    let TokenKind::Ident(name) = segment.kind else {
                        return Err(ScriptIssue::new(
                            "script_syntax_error",
                            segment.line,
                            segment.column,
                            "expected a field name after .",
                        ));
                    };
                    self.next();
                    segments.push(name);
                }
                if segments.is_empty() {
                    return Err(ScriptIssue::new(
                        "script_syntax_error",
                        token.line,
                        token.column,
                        "expected a field path",
                    ));
                }
                Ok(Expression {
                    kind: ExpressionKind::Path { root, segments },
                    line: token.line,
                    column: token.column,
                })
            }
            TokenKind::ActiveActors | TokenKind::CountChildren => {
                let is_active = matches!(token.kind, TokenKind::ActiveActors);
                self.next();
                self.expect(&TokenKind::LParen, "(")?;
                let argument = self.peek().clone();
                let TokenKind::Str(argument_value) = argument.kind else {
                    return Err(ScriptIssue::new(
                        "script_syntax_error",
                        argument.line,
                        argument.column,
                        "function argument must be a string literal",
                    ));
                };
                self.next();
                self.expect(&TokenKind::RParen, ")")?;
                Ok(Expression {
                    kind: if is_active {
                        ExpressionKind::ActiveActors(argument_value)
                    } else {
                        ExpressionKind::CountChildren(argument_value)
                    },
                    line: token.line,
                    column: token.column,
                })
            }
            TokenKind::LParen => {
                self.next();
                let inner = self.expression()?;
                self.expect(&TokenKind::RParen, ")")?;
                Ok(inner)
            }
            _ => Err(ScriptIssue::new(
                "script_syntax_error",
                token.line,
                token.column,
                "expected an expression",
            )),
        }
    }
}

fn compare_operator(kind: &TokenKind) -> Option<CompareOp> {
    Some(match kind {
        TokenKind::EqEq => CompareOp::Eq,
        TokenKind::NotEq => CompareOp::Ne,
        TokenKind::Lt => CompareOp::Lt,
        TokenKind::Le => CompareOp::Le,
        TokenKind::Gt => CompareOp::Gt,
        TokenKind::Ge => CompareOp::Ge,
        TokenKind::Superset => CompareOp::Superset,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

pub fn compile_action(
    source: &str,
    scope: &ScriptScope<'_>,
) -> Result<CompiledAction, Vec<ScriptIssue>> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(vec![ScriptIssue::new(
            "script_source_too_large",
            1,
            1,
            format!("script exceeds {MAX_SOURCE_BYTES} bytes"),
        )]);
    }
    let tokens = lex(source).map_err(|issue| vec![issue])?;
    let mut parser = Parser::new(tokens);
    let statements = parser.program().map_err(|issue| vec![issue])?;
    let mut checker = Checker::new(scope);
    checker.check_statements(&statements);
    if !checker.issues.is_empty() {
        return Err(checker.issues);
    }
    Ok(CompiledAction {
        statements,
        reads: checker.reads,
        writes: checker.writes,
    })
}

pub fn compile_condition(
    source: &str,
    scope: &ScriptScope<'_>,
) -> Result<CompiledExpression, Vec<ScriptIssue>> {
    if source.trim().is_empty() {
        return Err(vec![ScriptIssue::new(
            "script_syntax_error",
            1,
            1,
            "on must not be empty",
        )]);
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(vec![ScriptIssue::new(
            "script_source_too_large",
            1,
            1,
            format!("script exceeds {MAX_SOURCE_BYTES} bytes"),
        )]);
    }
    let tokens = lex(source).map_err(|issue| vec![issue])?;
    let mut parser = Parser::new(tokens);
    if matches!(
        parser.peek().kind,
        TokenKind::If | TokenKind::Add | TokenKind::LBrace
    ) {
        return Err(vec![ScriptIssue::new(
            "script_statement_not_allowed",
            parser.peek().line,
            parser.peek().column,
            "on accepts an expression, not a statement",
        )]);
    }
    let expression = parser.expression().map_err(|issue| vec![issue])?;
    if !matches!(parser.peek().kind, TokenKind::Eof) {
        let token = parser.peek();
        let code = if matches!(token.kind, TokenKind::Assign | TokenKind::LBrace) {
            "script_statement_not_allowed"
        } else {
            "script_syntax_error"
        };
        return Err(vec![ScriptIssue::new(
            code,
            token.line,
            token.column,
            "unexpected text after the expression",
        )]);
    }

    let mut checker = Checker::new(scope);
    let ty = checker.check_expression(&expression);
    if ty != Some(Type::Bool) && ty.is_some() {
        checker.issues.push(ScriptIssue::new(
            "script_condition_not_boolean",
            expression.line,
            expression.column,
            "on must evaluate to bool",
        ));
    }
    if !checker.issues.is_empty() {
        return Err(checker.issues);
    }
    Ok(CompiledExpression {
        expression,
        reads: checker.reads,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Type {
    Bool,
    Int,
    String,
    StringOptional,
    Null,
    Actor,
    ActorSet,
}

struct Checker<'a> {
    scope: &'a ScriptScope<'a>,
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    issues: Vec<ScriptIssue>,
}

impl<'a> Checker<'a> {
    fn new(scope: &'a ScriptScope<'a>) -> Self {
        Self {
            scope,
            reads: BTreeSet::new(),
            writes: BTreeSet::new(),
            issues: Vec::new(),
        }
    }

    fn check_statements(&mut self, statements: &[Statement]) {
        for statement in statements {
            self.check_statement(statement);
        }
    }

    fn check_statement(&mut self, statement: &Statement) {
        match statement {
            Statement::Assign {
                name,
                expression,
                line,
                column,
            } => {
                let value_type = self.check_expression(expression);
                let Some(expected) = self.scope.variables.get(name).copied() else {
                    self.issues.push(ScriptIssue::new(
                        "script_unknown_variable",
                        *line,
                        *column,
                        format!("unknown variable {name:?}"),
                    ));
                    return;
                };
                self.writes.insert(name.clone());
                if let Some(actual) = value_type
                    && actual != type_of(expected)
                {
                    self.issues.push(ScriptIssue::new(
                        "script_type_mismatch",
                        expression.line,
                        expression.column,
                        format!(
                            "cannot assign {} to {name} of type {}",
                            type_name(actual),
                            type_name(type_of(expected))
                        ),
                    ));
                }
            }
            Statement::Add {
                expression,
                name,
                line,
                column,
            } => {
                let value_type = self.check_expression(expression);
                let Some(expected) = self.scope.variables.get(name).copied() else {
                    self.issues.push(ScriptIssue::new(
                        "script_unknown_variable",
                        *line,
                        *column,
                        format!("unknown variable {name:?}"),
                    ));
                    return;
                };
                self.writes.insert(name.clone());
                if expected != VarType::ActorSet {
                    self.issues.push(ScriptIssue::new(
                        "script_type_mismatch",
                        *line,
                        *column,
                        format!("add target {name:?} must be actor_set"),
                    ));
                }
                if let Some(actual) = value_type
                    && actual != Type::Actor
                {
                    self.issues.push(ScriptIssue::new(
                        "script_type_mismatch",
                        expression.line,
                        expression.column,
                        format!("add operand must be actor, got {}", type_name(actual)),
                    ));
                }
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                line,
                column,
            } => {
                let condition_type = self.check_expression(condition);
                if condition_type != Some(Type::Bool) && condition_type.is_some() {
                    self.issues.push(ScriptIssue::new(
                        "script_condition_not_boolean",
                        *line,
                        *column,
                        "if condition must be bool",
                    ));
                }
                self.check_statements(then_branch);
                if let Some(else_branch) = else_branch {
                    self.check_statements(else_branch);
                }
            }
            Statement::Block(statements) => self.check_statements(statements),
        }
    }

    fn check_expression(&mut self, expression: &Expression) -> Option<Type> {
        match &expression.kind {
            ExpressionKind::Bool(_) => Some(Type::Bool),
            ExpressionKind::Int(_) => Some(Type::Int),
            ExpressionKind::Str(_) => Some(Type::String),
            ExpressionKind::Null => Some(Type::Null),
            ExpressionKind::Variable(name) => match self.scope.variables.get(name) {
                Some(var_type) => {
                    self.reads.insert(name.clone());
                    Some(type_of(*var_type))
                }
                None => {
                    self.issues.push(ScriptIssue::new(
                        "script_unknown_variable",
                        expression.line,
                        expression.column,
                        format!("unknown variable {name:?}"),
                    ));
                    None
                }
            },
            ExpressionKind::Path { root, segments } => {
                self.check_path(*root, segments, expression.line, expression.column)
            }
            ExpressionKind::ActiveActors(role) => {
                if !self.scope.roles.iter().any(|declared| declared == role) {
                    self.issues.push(ScriptIssue::new(
                        "script_unknown_role",
                        expression.line,
                        expression.column,
                        format!("unknown role {role:?}"),
                    ));
                } else if !self.scope.active_roles.iter().any(|active| active == role) {
                    self.issues.push(ScriptIssue::new(
                        "script_role_not_active",
                        expression.line,
                        expression.column,
                        format!("role {role:?} is not active in this stage"),
                    ));
                }
                Some(Type::ActorSet)
            }
            ExpressionKind::CountChildren(workflow) => {
                if !(self.scope.known_workflow)(workflow) {
                    self.issues.push(ScriptIssue::new(
                        "script_unknown_workflow",
                        expression.line,
                        expression.column,
                        format!("unknown workflow {workflow:?}"),
                    ));
                }
                Some(Type::Int)
            }
            ExpressionKind::Not(operand) => {
                let operand_type = self.check_expression(operand);
                if operand_type != Some(Type::Bool) && operand_type.is_some() {
                    self.type_mismatch(expression, "not operand must be bool");
                }
                Some(Type::Bool)
            }
            ExpressionKind::And(left, right) | ExpressionKind::Or(left, right) => {
                let left_type = self.check_expression(left);
                let right_type = self.check_expression(right);
                if (left_type != Some(Type::Bool) && left_type.is_some())
                    || (right_type != Some(Type::Bool) && right_type.is_some())
                {
                    self.type_mismatch(expression, "boolean operator operands must be bool");
                }
                Some(Type::Bool)
            }
            ExpressionKind::Compare(operator, left, right) => {
                let left_type = self.check_expression(left);
                let right_type = self.check_expression(right);
                if let (Some(left_type), Some(right_type)) = (left_type, right_type) {
                    let ok = match operator {
                        CompareOp::Eq | CompareOp::Ne => equality_compatible(left_type, right_type),
                        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => {
                            left_type == Type::Int && right_type == Type::Int
                        }
                        CompareOp::Superset => {
                            left_type == Type::ActorSet && right_type == Type::ActorSet
                        }
                    };
                    if !ok {
                        self.type_mismatch(
                            expression,
                            format!(
                                "cannot compare {} and {} with {}",
                                type_name(left_type),
                                type_name(right_type),
                                compare_name(*operator)
                            ),
                        );
                    }
                }
                Some(Type::Bool)
            }
            ExpressionKind::Add(left, right) | ExpressionKind::Subtract(left, right) => {
                let left_type = self.check_expression(left);
                let right_type = self.check_expression(right);
                if (left_type != Some(Type::Int) && left_type.is_some())
                    || (right_type != Some(Type::Int) && right_type.is_some())
                {
                    self.type_mismatch(expression, "arithmetic operands must be int");
                }
                Some(Type::Int)
            }
            ExpressionKind::Negate(operand) => {
                let operand_type = self.check_expression(operand);
                if operand_type != Some(Type::Int) && operand_type.is_some() {
                    self.type_mismatch(expression, "unary - operand must be int");
                }
                Some(Type::Int)
            }
        }
    }

    fn check_path(
        &mut self,
        root: PathRoot,
        segments: &[String],
        line: usize,
        column: usize,
    ) -> Option<Type> {
        match root {
            PathRoot::Task => match segments {
                [field] if field == "state" => Some(Type::String),
                [field] if field == "title" => Some(Type::String),
                _ => {
                    self.issues.push(ScriptIssue::new(
                        "script_unknown_field",
                        line,
                        column,
                        format!("unknown task field task.{}", segments.join(".")),
                    ));
                    None
                }
            },
            PathRoot::Event => {
                let fields = segments.join(".");
                if self.scope.event == EventAccess::Condition {
                    self.issues.push(ScriptIssue::new(
                        "script_event_field_not_available",
                        line,
                        column,
                        "on cannot read event data",
                    ));
                    return None;
                }
                let available = match (&self.scope.event, field_type(&fields)) {
                    (EventAccess::Operation, Some(Type::Actor)) if fields == "actor" => {
                        Some(Type::Actor)
                    }
                    (EventAccess::Event(EventFieldSet::None), _) => None,
                    (EventAccess::Event(EventFieldSet::TaskStateChanged), Some(Type::String))
                        if fields == "state" =>
                    {
                        Some(Type::String)
                    }
                    (EventAccess::Event(EventFieldSet::ChildCreated), Some(ty))
                        if matches!(
                            fields.as_str(),
                            "child.id" | "child.workflow" | "child.title"
                        ) =>
                    {
                        Some(ty)
                    }
                    (EventAccess::Event(EventFieldSet::ChildStateChanged), Some(ty))
                        if matches!(
                            fields.as_str(),
                            "child.id" | "child.workflow" | "child.state" | "child.result"
                        ) =>
                    {
                        Some(ty)
                    }
                    _ => None,
                };
                if let Some(ty) = available {
                    return Some(ty);
                }
                let code = if field_type(&fields).is_some() {
                    "script_event_field_not_available"
                } else {
                    "script_unknown_field"
                };
                self.issues.push(ScriptIssue::new(
                    code,
                    line,
                    column,
                    format!("event field event.{fields} is not available here"),
                ));
                None
            }
        }
    }

    fn type_mismatch(&mut self, expression: &Expression, message: impl Into<String>) {
        self.issues.push(ScriptIssue::new(
            "script_type_mismatch",
            expression.line,
            expression.column,
            message,
        ));
    }
}

fn type_of(var_type: VarType) -> Type {
    match var_type {
        VarType::Bool => Type::Bool,
        VarType::String => Type::String,
        VarType::Int => Type::Int,
        VarType::ActorSet => Type::ActorSet,
    }
}

fn field_type(path: &str) -> Option<Type> {
    Some(match path {
        "actor" => Type::Actor,
        "state" => Type::String,
        "child.id" | "child.workflow" | "child.title" | "child.state" => Type::String,
        "child.result" => Type::StringOptional,
        _ => return None,
    })
}

fn equality_compatible(left: Type, right: Type) -> bool {
    let optional = |ty: Type| matches!(ty, Type::StringOptional | Type::Null);
    if optional(left) || optional(right) {
        matches!(
            (left, right),
            (
                Type::String | Type::StringOptional | Type::Null,
                Type::String | Type::StringOptional | Type::Null
            )
        )
    } else {
        matches!(
            (left, right),
            (Type::Bool, Type::Bool) | (Type::Int, Type::Int) | (Type::String, Type::String)
        )
    }
}

fn type_name(ty: Type) -> &'static str {
    match ty {
        Type::Bool => "bool",
        Type::Int => "int",
        Type::String => "string",
        Type::StringOptional => "string?",
        Type::Null => "null",
        Type::Actor => "actor",
        Type::ActorSet => "actor_set",
    }
}

fn compare_name(operator: CompareOp) -> &'static str {
    match operator {
        CompareOp::Eq => "==",
        CompareOp::Ne => "!=",
        CompareOp::Lt => "<",
        CompareOp::Le => "<=",
        CompareOp::Gt => ">",
        CompareOp::Ge => ">=",
        CompareOp::Superset => "⊇",
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum EventData<'a> {
    Operation {
        actor: Uuid,
    },
    TaskCreated,
    TaskStateChanged {
        state: &'a str,
    },
    ChildCreated {
        id: &'a str,
        workflow: &'a str,
        title: &'a str,
    },
    ChildStateChanged {
        id: &'a str,
        workflow: &'a str,
        state: &'a str,
        result: Option<&'a str>,
    },
}

pub struct RuntimeContext<'a> {
    pub task_state: &'a str,
    pub task_title: &'a str,
    pub active_actors: &'a dyn Fn(&str) -> BTreeSet<Uuid>,
    pub children: &'a [ChildState],
    pub event: Option<EventData<'a>>,
}

#[derive(Debug, Clone)]
enum Value {
    Bool(bool),
    Int(i64),
    String(String),
    StringOptional(Option<String>),
    Null,
    Actor(Uuid),
    ActorSet(BTreeSet<Uuid>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeError {
    #[error("script_integer_overflow")]
    IntegerOverflow,
    #[error("workflow error: {0}")]
    Internal(String),
}

pub fn run_action(
    compiled: &CompiledAction,
    variables: &mut BTreeMap<String, VariableValue>,
    context: &RuntimeContext<'_>,
) -> Result<(), RuntimeError> {
    for statement in &compiled.statements {
        run_statement(statement, variables, context)?;
    }
    Ok(())
}

pub fn eval_condition(
    compiled: &CompiledExpression,
    variables: &BTreeMap<String, VariableValue>,
    context: &RuntimeContext<'_>,
) -> Result<bool, RuntimeError> {
    match evaluate(&compiled.expression, variables, context)? {
        Value::Bool(value) => Ok(value),
        _ => Err(RuntimeError::Internal(
            "condition did not evaluate to bool".to_string(),
        )),
    }
}

pub fn default_variables(declared: &BTreeMap<String, VarType>) -> BTreeMap<String, VariableValue> {
    declared
        .iter()
        .map(|(name, var_type)| (name.clone(), var_type.default_value()))
        .collect()
}

fn run_statement(
    statement: &Statement,
    variables: &mut BTreeMap<String, VariableValue>,
    context: &RuntimeContext<'_>,
) -> Result<(), RuntimeError> {
    match statement {
        Statement::Assign {
            name, expression, ..
        } => {
            let value = evaluate(expression, variables, context)?;
            let stored = to_variable(value)?;
            variables.insert(name.clone(), stored);
        }
        Statement::Add {
            expression, name, ..
        } => {
            let value = evaluate(expression, variables, context)?;
            let Value::Actor(actor) = value else {
                return Err(RuntimeError::Internal(
                    "add operand is not an actor".to_string(),
                ));
            };
            match variables.get_mut(name) {
                Some(VariableValue::ActorSet(values)) => {
                    values.insert(actor);
                }
                _ => {
                    return Err(RuntimeError::Internal(format!(
                        "{name} is not an actor_set variable"
                    )));
                }
            }
        }
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            if matches!(evaluate(condition, variables, context)?, Value::Bool(true)) {
                for statement in then_branch {
                    run_statement(statement, variables, context)?;
                }
            } else if let Some(else_branch) = else_branch {
                for statement in else_branch {
                    run_statement(statement, variables, context)?;
                }
            }
        }
        Statement::Block(statements) => {
            for statement in statements {
                run_statement(statement, variables, context)?;
            }
        }
    }
    Ok(())
}

fn to_variable(value: Value) -> Result<VariableValue, RuntimeError> {
    Ok(match value {
        Value::Bool(value) => VariableValue::Bool(value),
        Value::Int(value) => VariableValue::Int(value),
        Value::String(value) => VariableValue::String(value),
        Value::ActorSet(values) => VariableValue::ActorSet(values),
        other => {
            return Err(RuntimeError::Internal(format!(
                "value {other:?} cannot be stored in a variable"
            )));
        }
    })
}

fn evaluate(
    expression: &Expression,
    variables: &BTreeMap<String, VariableValue>,
    context: &RuntimeContext<'_>,
) -> Result<Value, RuntimeError> {
    Ok(match &expression.kind {
        ExpressionKind::Bool(value) => Value::Bool(*value),
        ExpressionKind::Int(value) => Value::Int(*value),
        ExpressionKind::Str(value) => Value::String(value.clone()),
        ExpressionKind::Null => Value::Null,
        ExpressionKind::Variable(name) => match variables.get(name) {
            Some(VariableValue::Bool(value)) => Value::Bool(*value),
            Some(VariableValue::String(value)) => Value::String(value.clone()),
            Some(VariableValue::Int(value)) => Value::Int(*value),
            Some(VariableValue::ActorSet(value)) => Value::ActorSet(value.clone()),
            None => {
                return Err(RuntimeError::Internal(format!(
                    "variable {name:?} is not initialized"
                )));
            }
        },
        ExpressionKind::Path { root, segments } => match root {
            PathRoot::Task => match segments.join(".").as_str() {
                "state" => Value::String(context.task_state.to_string()),
                "title" => Value::String(context.task_title.to_string()),
                other => {
                    return Err(RuntimeError::Internal(format!(
                        "unknown task field {other}"
                    )));
                }
            },
            PathRoot::Event => event_value(segments.join(".").as_str(), context)?,
        },
        ExpressionKind::ActiveActors(role) => Value::ActorSet((context.active_actors)(role)),
        ExpressionKind::CountChildren(workflow) => Value::Int(
            context
                .children
                .iter()
                .filter(|child| child.workflow.as_str() == workflow)
                .count() as i64,
        ),
        ExpressionKind::Not(operand) => match evaluate(operand, variables, context)? {
            Value::Bool(value) => Value::Bool(!value),
            _ => {
                return Err(RuntimeError::Internal(
                    "not operand is not bool".to_string(),
                ));
            }
        },
        ExpressionKind::And(left, right) => {
            let left = match evaluate(left, variables, context)? {
                Value::Bool(value) => value,
                _ => {
                    return Err(RuntimeError::Internal(
                        "and operand is not bool".to_string(),
                    ));
                }
            };
            if !left {
                Value::Bool(false)
            } else {
                match evaluate(right, variables, context)? {
                    Value::Bool(value) => Value::Bool(value),
                    _ => {
                        return Err(RuntimeError::Internal(
                            "and operand is not bool".to_string(),
                        ));
                    }
                }
            }
        }
        ExpressionKind::Or(left, right) => {
            let left = match evaluate(left, variables, context)? {
                Value::Bool(value) => value,
                _ => return Err(RuntimeError::Internal("or operand is not bool".to_string())),
            };
            if left {
                Value::Bool(true)
            } else {
                match evaluate(right, variables, context)? {
                    Value::Bool(value) => Value::Bool(value),
                    _ => {
                        return Err(RuntimeError::Internal("or operand is not bool".to_string()));
                    }
                }
            }
        }
        ExpressionKind::Compare(operator, left, right) => {
            let left = evaluate(left, variables, context)?;
            let right = evaluate(right, variables, context)?;
            Value::Bool(compare(*operator, left, right)?)
        }
        ExpressionKind::Add(left, right) => {
            let left = int_value(evaluate(left, variables, context)?)?;
            let right = int_value(evaluate(right, variables, context)?)?;
            Value::Int(
                left.checked_add(right)
                    .ok_or(RuntimeError::IntegerOverflow)?,
            )
        }
        ExpressionKind::Subtract(left, right) => {
            let left = int_value(evaluate(left, variables, context)?)?;
            let right = int_value(evaluate(right, variables, context)?)?;
            Value::Int(
                left.checked_sub(right)
                    .ok_or(RuntimeError::IntegerOverflow)?,
            )
        }
        ExpressionKind::Negate(operand) => {
            let value = int_value(evaluate(operand, variables, context)?)?;
            Value::Int(value.checked_neg().ok_or(RuntimeError::IntegerOverflow)?)
        }
    })
}

fn event_value(path: &str, context: &RuntimeContext<'_>) -> Result<Value, RuntimeError> {
    let Some(event) = &context.event else {
        return Err(RuntimeError::Internal(
            "event data is not available".to_string(),
        ));
    };
    Ok(match (event, path) {
        (EventData::Operation { actor }, "actor") => Value::Actor(*actor),
        (EventData::TaskStateChanged { state }, "state") => Value::String((*state).to_string()),
        (EventData::ChildCreated { id, .. }, "child.id") => Value::String((*id).to_string()),
        (EventData::ChildCreated { workflow, .. }, "child.workflow") => {
            Value::String((*workflow).to_string())
        }
        (EventData::ChildCreated { title, .. }, "child.title") => {
            Value::String((*title).to_string())
        }
        (EventData::ChildStateChanged { id, .. }, "child.id") => Value::String((*id).to_string()),
        (EventData::ChildStateChanged { workflow, .. }, "child.workflow") => {
            Value::String((*workflow).to_string())
        }
        (EventData::ChildStateChanged { state, .. }, "child.state") => {
            Value::String((*state).to_string())
        }
        (EventData::ChildStateChanged { result, .. }, "child.result") => {
            Value::StringOptional(result.map(str::to_string))
        }
        _ => {
            return Err(RuntimeError::Internal(format!(
                "event field event.{path} is not available"
            )));
        }
    })
}

fn int_value(value: Value) -> Result<i64, RuntimeError> {
    match value {
        Value::Int(value) => Ok(value),
        _ => Err(RuntimeError::Internal("operand is not int".to_string())),
    }
}

fn compare(operator: CompareOp, left: Value, right: Value) -> Result<bool, RuntimeError> {
    if operator == CompareOp::Superset {
        let (Value::ActorSet(left), Value::ActorSet(right)) = (left, right) else {
            return Err(RuntimeError::Internal(
                "⊇ operands are not actor sets".to_string(),
            ));
        };
        return Ok(right.is_subset(&left));
    }
    Ok(match operator {
        CompareOp::Eq | CompareOp::Ne => {
            let equal = values_equal(&left, &right)?;
            if operator == CompareOp::Eq {
                equal
            } else {
                !equal
            }
        }
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => {
            let (Value::Int(left), Value::Int(right)) = (left, right) else {
                return Err(RuntimeError::Internal(
                    "comparison operands are not int".to_string(),
                ));
            };
            match operator {
                CompareOp::Lt => left < right,
                CompareOp::Le => left <= right,
                CompareOp::Gt => left > right,
                CompareOp::Ge => left >= right,
                _ => unreachable!(),
            }
        }
        CompareOp::Superset => unreachable!(),
    })
}

fn values_equal(left: &Value, right: &Value) -> Result<bool, RuntimeError> {
    Ok(match (left, right) {
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Int(left), Value::Int(right)) => left == right,
        (Value::String(left), Value::String(right)) => left == right,
        (Value::StringOptional(left), Value::StringOptional(right)) => left == right,
        (Value::StringOptional(left), Value::Null) | (Value::Null, Value::StringOptional(left)) => {
            left.is_none()
        }
        (Value::String(_), Value::Null) | (Value::Null, Value::String(_)) => false,
        (Value::String(left), Value::StringOptional(right))
        | (Value::StringOptional(right), Value::String(left)) => right.as_ref() == Some(left),
        (Value::Null, Value::Null) => true,
        _ => {
            return Err(RuntimeError::Internal(
                "values are not equality-comparable".to_string(),
            ));
        }
    })
}

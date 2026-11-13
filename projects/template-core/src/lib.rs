use data_core::{Value, ValueError};

/// Root variable lookup for templates.
///
/// Implementations resolve only the first identifier of an expression. Member
/// access (`a.b`, `a[0]`) is evaluated by the template system.
pub trait VariableSet {
    fn get(&self, name: &str) -> Option<Value>;
}

impl VariableSet for Value {
    fn get(&self, name: &str) -> Option<Value> {
        self.as_object()?.get(name).cloned()
    }
}

/// A template with `{{ expression }}` substitutions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    source: String,
    parts: Vec<TemplatePart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TemplatePart {
    Literal(String),
    Expression(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    Literal(Value),
    Variable(String),
    Field {
        base: Box<Expr>,
        name: String,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Ternary {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnaryOp {
    Not,
    Neg,
    Pos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl Template {
    pub fn parse(input: impl Into<String>) -> Result<Self, TemplateError> {
        let source = input.into();
        let mut parts = Vec::new();
        let mut rest = source.as_str();

        while let Some(start) = rest.find("{{") {
            let literal = &rest[..start];
            if !literal.is_empty() {
                parts.push(TemplatePart::Literal(literal.to_owned()));
            }

            let after = &rest[start + 2..];
            let end = after
                .find("}}")
                .ok_or(TemplateError::UnclosedVariable(start))?;
            let raw = after[..end].trim();
            if raw.is_empty() {
                return Err(TemplateError::EmptyVariable);
            }

            parts.push(TemplatePart::Expression(ExprParser::new(raw).parse()?));
            rest = &after[end + 2..];
        }

        if !rest.is_empty() {
            parts.push(TemplatePart::Literal(rest.to_owned()));
        }

        Ok(Self { source, parts })
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub fn render(&self, variables: &dyn VariableSet) -> Result<String, TemplateError> {
        let mut output = String::with_capacity(self.source.len());

        for part in &self.parts {
            match part {
                TemplatePart::Literal(text) => output.push_str(text),
                TemplatePart::Expression(expr) => {
                    let value = eval(expr, variables)?;
                    output.push_str(&render_value(&value)?);
                }
            }
        }

        Ok(output)
    }
}

fn eval(expr: &Expr, variables: &dyn VariableSet) -> Result<Value, TemplateError> {
    match expr {
        Expr::Literal(value) => Ok(value.clone()),
        Expr::Variable(name) => variables
            .get(name)
            .ok_or_else(|| TemplateError::MissingVariable(name.clone())),
        Expr::Field { base, name } => {
            let base = eval(base, variables)?;
            base.as_object()
                .and_then(|object| object.get(name))
                .cloned()
                .ok_or_else(|| TemplateError::MissingField(name.clone()))
        }
        Expr::Index { base, index } => {
            let base = eval(base, variables)?;
            let index_value = eval(index, variables)?;
            let index = index_value
                .as_i64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| TemplateError::Type("array index must be an integer".into()))?;

            base.as_array()
                .and_then(|array| array.get(index))
                .cloned()
                .ok_or(TemplateError::IndexOutOfBounds(index))
        }
        Expr::Unary { op, expr } => {
            let value = eval(expr, variables)?;
            eval_unary(*op, &value)
        }
        Expr::Binary { op, left, right } => {
            if *op == BinaryOp::And {
                let left = eval(left, variables)?;
                if !truthy(&left) {
                    return Ok(Value::bool(false));
                }
                return Ok(Value::bool(truthy(&eval(right, variables)?)));
            }
            if *op == BinaryOp::Or {
                let left = eval(left, variables)?;
                if truthy(&left) {
                    return Ok(Value::bool(true));
                }
                return Ok(Value::bool(truthy(&eval(right, variables)?)));
            }

            let left = eval(left, variables)?;
            let right = eval(right, variables)?;
            eval_binary(*op, &left, &right)
        }
        Expr::Ternary {
            condition,
            then_branch,
            else_branch,
        } => {
            if truthy(&eval(condition, variables)?) {
                eval(then_branch, variables)
            } else {
                eval(else_branch, variables)
            }
        }
    }
}

fn eval_unary(op: UnaryOp, value: &Value) -> Result<Value, TemplateError> {
    match op {
        UnaryOp::Not => Ok(Value::bool(!truthy(value))),
        UnaryOp::Pos => {
            if value.as_i64().is_some() || value.as_f64().is_some() {
                Ok(value.clone())
            } else {
                Err(TemplateError::Type("unary + expects a number".into()))
            }
        }
        UnaryOp::Neg => {
            if let Some(integer) = value.as_i64()
                && let Some(negated) = integer.checked_neg()
            {
                return Ok(Value::integer(negated));
            }

            let number = number_f64(value)
                .ok_or_else(|| TemplateError::Type("unary - expects a number".into()))?;
            Ok(Value::number(-number))
        }
    }
}

fn eval_binary(op: BinaryOp, left: &Value, right: &Value) -> Result<Value, TemplateError> {
    match op {
        BinaryOp::Add if left.as_str().is_some() || right.as_str().is_some() => {
            let left = render_value(left)?;
            let right = render_value(right)?;
            Ok(Value::string(format!("{left}{right}")))
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            numeric_binary(op, left, right)
        }
        BinaryOp::Eq => Ok(Value::bool(values_equal(left, right))),
        BinaryOp::Ne => Ok(Value::bool(!values_equal(left, right))),
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            Ok(Value::bool(compare(op, left, right)?))
        }
        BinaryOp::And | BinaryOp::Or => unreachable!("short-circuited above"),
    }
}

fn numeric_binary(op: BinaryOp, left: &Value, right: &Value) -> Result<Value, TemplateError> {
    if op != BinaryOp::Div
        && let (Some(left), Some(right)) = (left.as_i64(), right.as_i64())
    {
        let result = match op {
            BinaryOp::Add => left.checked_add(right),
            BinaryOp::Sub => left.checked_sub(right),
            BinaryOp::Mul => left.checked_mul(right),
            BinaryOp::Rem => left.checked_rem(right),
            _ => None,
        };
        if let Some(result) = result {
            return Ok(Value::integer(result));
        }
    }

    let left =
        number_f64(left).ok_or_else(|| TemplateError::Type("arithmetic expects numbers".into()))?;
    let right = number_f64(right)
        .ok_or_else(|| TemplateError::Type("arithmetic expects numbers".into()))?;

    let result = match op {
        BinaryOp::Add => left + right,
        BinaryOp::Sub => left - right,
        BinaryOp::Mul => left * right,
        BinaryOp::Div => left / right,
        BinaryOp::Rem => left % right,
        _ => unreachable!("not a numeric operator"),
    };
    Ok(Value::number(result))
}

fn compare(op: BinaryOp, left: &Value, right: &Value) -> Result<bool, TemplateError> {
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        return Ok(match op {
            BinaryOp::Lt => left < right,
            BinaryOp::Le => left <= right,
            BinaryOp::Gt => left > right,
            BinaryOp::Ge => left >= right,
            _ => unreachable!("not a comparison"),
        });
    }

    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return Ok(match op {
            BinaryOp::Lt => left < right,
            BinaryOp::Le => left <= right,
            BinaryOp::Gt => left > right,
            BinaryOp::Ge => left >= right,
            _ => unreachable!("not a comparison"),
        });
    }

    let left = number_f64(left)
        .ok_or_else(|| TemplateError::Type("comparison expects numbers or strings".into()))?;
    let right = number_f64(right)
        .ok_or_else(|| TemplateError::Type("comparison expects numbers or strings".into()))?;

    Ok(match op {
        BinaryOp::Lt => left < right,
        BinaryOp::Le => left <= right,
        BinaryOp::Gt => left > right,
        BinaryOp::Ge => left >= right,
        _ => unreachable!("not a comparison"),
    })
}

fn values_equal(left: &Value, right: &Value) -> bool {
    if left.is_null() || right.is_null() {
        return left.is_null() && right.is_null();
    }
    if let (Some(left), Some(right)) = (left.as_bool(), right.as_bool()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (number_f64(left), number_f64(right)) {
        return left == right;
    }

    left == right
}

fn truthy(value: &Value) -> bool {
    if value.is_null() {
        return false;
    }
    if let Some(value) = value.as_bool() {
        return value;
    }
    if let Some(value) = value.as_i64() {
        return value != 0;
    }
    if let Some(value) = value.as_f64() {
        return value != 0.0;
    }
    if let Some(value) = value.as_str() {
        return !value.is_empty();
    }

    true
}

fn number_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
}

fn render_value(value: &Value) -> Result<String, TemplateError> {
    if value.is_null() {
        return Ok(String::new());
    }
    if let Some(value) = value.as_bool() {
        return Ok(value.to_string());
    }
    if let Some(value) = value.as_i64() {
        return Ok(value.to_string());
    }
    if let Some(value) = value.as_f64() {
        return Ok(value.to_string());
    }
    if let Some(value) = value.as_str() {
        return Ok(value.to_owned());
    }

    Ok(value.to_json()?.to_string())
}

struct ExprParser {
    chars: Vec<char>,
    pos: usize,
}

impl ExprParser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn parse(mut self) -> Result<Expr, TemplateError> {
        let expr = self.parse_ternary()?;
        self.skip_whitespace();
        if self.peek().is_some() {
            return Err(self.error("unexpected trailing input"));
        }
        Ok(expr)
    }

    fn parse_ternary(&mut self) -> Result<Expr, TemplateError> {
        let condition = self.parse_or()?;
        self.skip_whitespace();
        if !self.eat('?') {
            return Ok(condition);
        }

        let then_branch = self.parse_ternary()?;
        self.skip_whitespace();
        self.expect(':')?;
        let else_branch = self.parse_ternary()?;

        Ok(Expr::Ternary {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        })
    }

    fn parse_or(&mut self) -> Result<Expr, TemplateError> {
        let mut expr = self.parse_and()?;
        loop {
            self.skip_whitespace();
            if !self.eat_str("||") {
                return Ok(expr);
            }
            let right = self.parse_and()?;
            expr = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
    }

    fn parse_and(&mut self) -> Result<Expr, TemplateError> {
        let mut expr = self.parse_equality()?;
        loop {
            self.skip_whitespace();
            if !self.eat_str("&&") {
                return Ok(expr);
            }
            let right = self.parse_equality()?;
            expr = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
    }

    fn parse_equality(&mut self) -> Result<Expr, TemplateError> {
        let mut expr = self.parse_comparison()?;
        loop {
            self.skip_whitespace();
            let op = if self.eat_str("==") {
                BinaryOp::Eq
            } else if self.eat_str("!=") {
                BinaryOp::Ne
            } else {
                return Ok(expr);
            };

            let right = self.parse_comparison()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr, TemplateError> {
        let mut expr = self.parse_additive()?;
        loop {
            self.skip_whitespace();
            let op = if self.eat_str("<=") {
                BinaryOp::Le
            } else if self.eat_str(">=") {
                BinaryOp::Ge
            } else if self.eat('<') {
                BinaryOp::Lt
            } else if self.eat('>') {
                BinaryOp::Gt
            } else {
                return Ok(expr);
            };

            let right = self.parse_additive()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
    }

    fn parse_additive(&mut self) -> Result<Expr, TemplateError> {
        let mut expr = self.parse_multiplicative()?;
        loop {
            self.skip_whitespace();
            let op = if self.eat('+') {
                BinaryOp::Add
            } else if self.eat('-') {
                BinaryOp::Sub
            } else {
                return Ok(expr);
            };

            let right = self.parse_multiplicative()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, TemplateError> {
        let mut expr = self.parse_unary()?;
        loop {
            self.skip_whitespace();
            let op = if self.eat('*') {
                BinaryOp::Mul
            } else if self.eat('/') {
                BinaryOp::Div
            } else if self.eat('%') {
                BinaryOp::Rem
            } else {
                return Ok(expr);
            };

            let right = self.parse_unary()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(right),
            };
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, TemplateError> {
        self.skip_whitespace();

        if self.eat('!') {
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.parse_unary()?),
            });
        }
        if self.eat('-') {
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(self.parse_unary()?),
            });
        }
        if self.eat('+') {
            return Ok(Expr::Unary {
                op: UnaryOp::Pos,
                expr: Box::new(self.parse_unary()?),
            });
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, TemplateError> {
        let mut expr = self.parse_primary()?;

        loop {
            self.skip_whitespace();
            if self.eat('.') {
                self.skip_whitespace();
                if self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                    let index = self.parse_usize()?;
                    let index =
                        i64::try_from(index).map_err(|_| self.error("array index is too large"))?;
                    expr = Expr::Index {
                        base: Box::new(expr),
                        index: Box::new(Expr::Literal(Value::integer(index))),
                    };
                } else {
                    let name = self.parse_identifier()?;
                    expr = Expr::Field {
                        base: Box::new(expr),
                        name,
                    };
                }
            } else if self.eat('[') {
                let index = self.parse_ternary()?;
                self.skip_whitespace();
                self.expect(']')?;
                expr = Expr::Index {
                    base: Box::new(expr),
                    index: Box::new(index),
                };
            } else {
                return Ok(expr);
            }
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, TemplateError> {
        self.skip_whitespace();
        match self.peek() {
            Some('"') | Some('\'') => Ok(Expr::Literal(self.parse_string()?)),
            Some(ch) if ch.is_ascii_digit() => Ok(Expr::Literal(self.parse_number()?)),
            Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {
                let name = self.parse_identifier()?;
                match name.as_str() {
                    "true" => Ok(Expr::Literal(Value::bool(true))),
                    "false" => Ok(Expr::Literal(Value::bool(false))),
                    "null" => Ok(Expr::Literal(Value::null())),
                    _ => Ok(Expr::Variable(name)),
                }
            }
            Some('(') => {
                self.pos += 1;
                let expr = self.parse_ternary()?;
                self.skip_whitespace();
                self.expect(')')?;
                Ok(expr)
            }
            _ => Err(self.error("expected expression")),
        }
    }

    fn parse_identifier(&mut self) -> Result<String, TemplateError> {
        let start = self.pos;
        match self.peek() {
            Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {
                self.pos += 1;
            }
            _ => return Err(self.error("expected identifier")),
        }

        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            self.pos += 1;
        }

        Ok(self.chars[start..self.pos].iter().collect())
    }

    fn parse_usize(&mut self) -> Result<usize, TemplateError> {
        let start = self.pos;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.pos += 1;
        }

        if start == self.pos {
            return Err(self.error("expected array index"));
        }

        let text: String = self.chars[start..self.pos].iter().collect();
        text.parse()
            .map_err(|_| self.error("array index is too large"))
    }

    fn parse_number(&mut self) -> Result<Value, TemplateError> {
        let start = self.pos;

        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.pos += 1;
        }

        if self.peek() == Some('.') {
            self.pos += 1;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.pos += 1;
            }
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            let exponent_start = self.pos;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.pos += 1;
            }
            if exponent_start == self.pos {
                return Err(self.error("expected exponent digits"));
            }
        }

        let text: String = self.chars[start..self.pos].iter().collect();
        let value = text
            .parse::<f64>()
            .map_err(|_| self.error("invalid number"))?;
        Ok(Value::number(value))
    }

    fn parse_string(&mut self) -> Result<Value, TemplateError> {
        let quote = self.bump().expect("quote");
        let mut output = String::new();

        loop {
            match self.bump() {
                Some(ch) if ch == quote => return Ok(Value::string(output)),
                Some('\\') => match self.bump() {
                    Some('\\') => output.push('\\'),
                    Some('\'') => output.push('\''),
                    Some('"') => output.push('"'),
                    Some('n') => output.push('\n'),
                    Some('r') => output.push('\r'),
                    Some('t') => output.push('\t'),
                    _ => return Err(self.error("invalid string escape")),
                },
                Some(ch) => output.push(ch),
                None => return Err(self.error("unclosed string literal")),
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_str(&mut self, expected: &str) -> bool {
        let mut pos = self.pos;
        for ch in expected.chars() {
            if self.chars.get(pos) != Some(&ch) {
                return false;
            }
            pos += 1;
        }
        self.pos = pos;
        true
    }

    fn expect(&mut self, expected: char) -> Result<(), TemplateError> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{expected}'")))
        }
    }

    fn error(&self, message: &str) -> TemplateError {
        TemplateError::Parse {
            position: self.pos,
            message: message.to_owned(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("unclosed variable starting at byte {0}")]
    UnclosedVariable(usize),

    #[error("empty variable")]
    EmptyVariable,

    #[error("parse error at character {position}: {message}")]
    Parse { position: usize, message: String },

    #[error("missing variable: {0}")]
    MissingVariable(String),

    #[error("missing field: {0}")]
    MissingField(String),

    #[error("array index out of bounds: {0}")]
    IndexOutOfBounds(usize),

    #[error("type error: {0}")]
    Type(String),

    #[error(transparent)]
    Data(#[from] ValueError),
}

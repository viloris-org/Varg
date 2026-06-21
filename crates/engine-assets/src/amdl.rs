//! Aster Model Language (`.amdl`) parser and validation primitives.
//!
//! `.amdl` is an AI-facing declaration language for reusable model asset
//! composition. It describes what an object is: mesh source, material defaults,
//! collider defaults, rigidbody defaults, sockets, LODs, and metadata. It does
//! not describe scene placement or runtime behavior.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Parsed `.amdl` document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlDocument {
    /// Declared models in source order.
    pub models: Vec<AmdlModel>,
}

/// A single `model Name { ... }` declaration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlModel {
    /// Model identifier.
    pub name: String,
    /// Statements inside the model block.
    pub statements: Vec<AmdlStatement>,
}

/// Statement inside an `.amdl` block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AmdlStatement {
    /// `name = expr`
    Assignment {
        /// Assigned field name.
        name: String,
        /// Assigned expression.
        value: AmdlExpr,
    },
    /// `block OptionalName { ... }`
    Block(AmdlNamedBlock),
}

/// Named block statement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlNamedBlock {
    /// Block kind, for example `material`, `rigidbody`, `socket`, or `lod`.
    pub kind: String,
    /// Optional block label, for example `socket Top`.
    pub name: Option<String>,
    /// Nested statements.
    pub statements: Vec<AmdlStatement>,
}

/// `.amdl` expression.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AmdlExpr {
    /// Literal scalar.
    Value(AmdlValue),
    /// Array literal.
    Array(Vec<AmdlExpr>),
    /// Function-like constructor call, for example `asset("foo.glb")`.
    Call(AmdlCall),
    /// Constructor-like block expression, for example `material { ... }`.
    Object(AmdlObject),
}

/// Literal value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AmdlValue {
    /// String literal.
    String(String),
    /// Number literal with optional unit suffix, for example `12kg` or `25m`.
    Number {
        /// Numeric value.
        value: f64,
        /// Optional unit suffix.
        unit: Option<String>,
    },
    /// Boolean literal.
    Bool(bool),
    /// Symbol literal used for enum-like values, for example `dynamic`.
    Ident(String),
}

/// Function-like constructor call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlCall {
    /// Dotted call path, for example `primitive.box` or `collider.capsule`.
    pub path: Vec<String>,
    /// Positional or named arguments.
    pub arguments: Vec<AmdlArgument>,
}

/// Constructor-like block expression.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlObject {
    /// Dotted constructor path, for example `material`, `collider.box`, or `dynamic`.
    pub path: Vec<String>,
    /// Nested assignments or blocks.
    pub statements: Vec<AmdlStatement>,
}

/// Call argument.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AmdlArgument {
    /// Positional argument.
    Positional {
        /// Argument value.
        value: AmdlExpr,
    },
    /// `name: expr`
    Named {
        /// Argument name.
        name: String,
        /// Argument value.
        value: AmdlExpr,
    },
}

/// Parses an `.amdl` document.
pub fn parse_amdl(source: &str) -> Result<AmdlDocument, AmdlParserError> {
    Parser::new(source).parse_document()
}

/// `.amdl` parser error with byte position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmdlParserError {
    /// Byte offset in the source.
    pub offset: usize,
    /// Human-readable parser message.
    pub message: String,
}

impl AmdlParserError {
    fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for AmdlParserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for AmdlParserError {}

/// `.amdl` semantic validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmdlValidationError {
    /// Human-readable validation message.
    pub message: String,
}

impl AmdlValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AmdlValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AmdlValidationError {}

/// Semantic validator for `.amdl`.
#[derive(Clone, Debug, Default)]
pub struct AmdlValidator;

impl AmdlValidator {
    /// Validates the parsed document.
    pub fn validate(document: &AmdlDocument) -> Result<(), Vec<AmdlValidationError>> {
        let mut errors = Vec::new();
        if document.models.is_empty() {
            errors.push(AmdlValidationError::new(
                "document must contain at least one model declaration",
            ));
        }
        for model in &document.models {
            validate_model(model, &mut errors);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn validate_model(model: &AmdlModel, errors: &mut Vec<AmdlValidationError>) {
    if model.name.trim().is_empty() {
        errors.push(AmdlValidationError::new("model name cannot be empty"));
    }

    let mut mesh_count = 0;
    for statement in &model.statements {
        match statement {
            AmdlStatement::Assignment { name, value } => {
                match name.as_str() {
                    "mesh" => {
                        mesh_count += 1;
                        validate_call_like(value, &["asset", "primitive"], "mesh", errors);
                    }
                    "material" => {
                        validate_constructor_like(
                            value,
                            &["ref", "material"],
                            "material",
                            errors,
                        );
                    }
                    "collider" => {
                        validate_constructor_like(value, &["collider"], "collider", errors);
                    }
                    "rigidbody" => validate_rigidbody_value(value, errors),
                    "metadata" => {}
                    other => errors.push(AmdlValidationError::new(format!(
                        "unsupported assignment `{other}` in model `{}`",
                        model.name
                    ))),
                }
            }
            AmdlStatement::Block(block) => validate_block(block, errors),
        }
    }

    if mesh_count == 0 {
        errors.push(AmdlValidationError::new(format!(
            "model `{}` must declare a mesh",
            model.name
        )));
    } else if mesh_count > 1 {
        errors.push(AmdlValidationError::new(format!(
            "model `{}` must not declare more than one top-level mesh",
            model.name
        )));
    }
}

fn validate_block(block: &AmdlNamedBlock, errors: &mut Vec<AmdlValidationError>) {
    match block.kind.as_str() {
        "material" => {
            if block.name.is_some() {
                errors.push(AmdlValidationError::new(
                    "`material` blocks do not take a name in MVP syntax",
                ));
            }
        }
        "rigidbody" => {
            if block
                .name
                .as_deref()
                .map_or(true, |name| !matches!(name, "static" | "dynamic" | "kinematic"))
            {
                errors.push(AmdlValidationError::new(
                    "`rigidbody` block must be named static, dynamic, or kinematic",
                ));
            }
        }
        "collider" => {
            if block.name.as_deref().map_or(true, |name| {
                !matches!(name, "box" | "sphere" | "capsule" | "cylinder" | "mesh")
            }) {
                errors.push(AmdlValidationError::new(
                    "`collider` block must be named box, sphere, capsule, cylinder, or mesh",
                ));
            }
        }
        "socket" => {
            if block.name.is_none() {
                errors.push(AmdlValidationError::new("`socket` block requires a name"));
            }
        }
        "lod" => {
            if block.name.is_none() {
                errors.push(AmdlValidationError::new("`lod` block requires an index"));
            }
            if !block.statements.iter().any(|statement| {
                matches!(statement, AmdlStatement::Assignment { name, .. } if name == "mesh")
            }) {
                errors.push(AmdlValidationError::new("`lod` block requires `mesh = ...`"));
            }
        }
        "metadata" => {}
        other => errors.push(AmdlValidationError::new(format!(
            "unsupported block `{other}` in model declaration"
        ))),
    }
}

fn validate_call_like(
    value: &AmdlExpr,
    allowed_roots: &[&str],
    field: &str,
    errors: &mut Vec<AmdlValidationError>,
) {
    match value {
        AmdlExpr::Call(call) => {
            let root = call.path.first().map(String::as_str);
            if !root.is_some_and(|root| allowed_roots.contains(&root)) {
                errors.push(AmdlValidationError::new(format!(
                    "`{field}` must use one of: {}",
                    allowed_roots.join(", ")
                )));
            }
        }
        _ => errors.push(AmdlValidationError::new(format!(
            "`{field}` must be a constructor call"
        ))),
    }
}

fn validate_constructor_like(
    value: &AmdlExpr,
    allowed_roots: &[&str],
    field: &str,
    errors: &mut Vec<AmdlValidationError>,
) {
    match value {
        AmdlExpr::Call(call) => {
            let root = call.path.first().map(String::as_str);
            if !root.is_some_and(|root| allowed_roots.contains(&root)) {
                errors.push(AmdlValidationError::new(format!(
                    "`{field}` must use one of: {}",
                    allowed_roots.join(", ")
                )));
            }
        }
        AmdlExpr::Object(object) => {
            let root = object.path.first().map(String::as_str);
            if !root.is_some_and(|root| allowed_roots.contains(&root)) {
                errors.push(AmdlValidationError::new(format!(
                    "`{field}` must use one of: {}",
                    allowed_roots.join(", ")
                )));
            }
        }
        _ => errors.push(AmdlValidationError::new(format!(
            "`{field}` must be a constructor call or constructor block"
        ))),
    }
}

fn validate_rigidbody_value(value: &AmdlExpr, errors: &mut Vec<AmdlValidationError>) {
    match value {
        AmdlExpr::Value(AmdlValue::Ident(value))
            if matches!(value.as_str(), "static" | "dynamic" | "kinematic") => {}
        AmdlExpr::Object(object)
            if object.path.len() == 1
                && matches!(
                    object.path.first().map(String::as_str),
                    Some("static" | "dynamic" | "kinematic")
                ) => {}
        _ => errors.push(AmdlValidationError::new(
            "`rigidbody` must be static, dynamic, kinematic, or a constructor block",
        )),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Ident(String),
    String(String),
    Number { value: f64, unit: Option<String> },
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Equal,
    Colon,
    Comma,
    Dot,
}

#[derive(Clone, Debug, PartialEq)]
struct Token {
    kind: TokenKind,
    offset: usize,
}

struct Parser<'a> {
    tokens: Vec<Token>,
    cursor: usize,
    source: &'a str,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            tokens: lex(source),
            cursor: 0,
            source,
        }
    }

    fn parse_document(&mut self) -> Result<AmdlDocument, AmdlParserError> {
        let mut models = Vec::new();
        while !self.is_eof() {
            self.expect_ident_value("model")?;
            let name = self.expect_ident()?;
            self.expect(TokenKindDiscriminant::LBrace)?;
            let statements = self.parse_statements()?;
            models.push(AmdlModel { name, statements });
        }
        Ok(AmdlDocument { models })
    }

    fn parse_statements(&mut self) -> Result<Vec<AmdlStatement>, AmdlParserError> {
        let mut statements = Vec::new();
        while !self.check(TokenKindDiscriminant::RBrace) {
            if self.is_eof() {
                return Err(self.error_here("expected `}`"));
            }
            let first = self.expect_ident_or_number_name()?;
            if self.check(TokenKindDiscriminant::Equal) {
                self.advance();
                let value = self.parse_expr()?;
                statements.push(AmdlStatement::Assignment { name: first, value });
            } else if self.check(TokenKindDiscriminant::LBrace) {
                self.advance();
                let nested = self.parse_statements()?;
                statements.push(AmdlStatement::Block(AmdlNamedBlock {
                    kind: first,
                    name: None,
                    statements: nested,
                }));
            } else {
                let name = self.expect_ident_or_number_name()?;
                self.expect(TokenKindDiscriminant::LBrace)?;
                let nested = self.parse_statements()?;
                statements.push(AmdlStatement::Block(AmdlNamedBlock {
                    kind: first,
                    name: Some(name),
                    statements: nested,
                }));
            }
        }
        self.expect(TokenKindDiscriminant::RBrace)?;
        Ok(statements)
    }

    fn parse_expr(&mut self) -> Result<AmdlExpr, AmdlParserError> {
        if self.check(TokenKindDiscriminant::LBracket) {
            return self.parse_array();
        }

        let token = self
            .peek()
            .ok_or_else(|| self.error_here("expected expression"))?
            .clone();
        match token.kind {
            TokenKind::String(value) => {
                self.advance();
                Ok(AmdlExpr::Value(AmdlValue::String(value)))
            }
            TokenKind::Number { value, unit } => {
                self.advance();
                Ok(AmdlExpr::Value(AmdlValue::Number { value, unit }))
            }
            TokenKind::Ident(_) => {
                let path = self.parse_path()?;
                if self.check(TokenKindDiscriminant::LParen) {
                    let arguments = self.parse_call_arguments()?;
                    Ok(AmdlExpr::Call(AmdlCall { path, arguments }))
                } else if self.check(TokenKindDiscriminant::LBrace) {
                    self.advance();
                    let statements = self.parse_statements()?;
                    Ok(AmdlExpr::Object(AmdlObject { path, statements }))
                } else if path.len() == 1 {
                    let value = path.into_iter().next().unwrap();
                    match value.as_str() {
                        "true" => Ok(AmdlExpr::Value(AmdlValue::Bool(true))),
                        "false" => Ok(AmdlExpr::Value(AmdlValue::Bool(false))),
                        _ => Ok(AmdlExpr::Value(AmdlValue::Ident(value))),
                    }
                } else {
                    Err(AmdlParserError::new(
                        token.offset,
                        "dotted identifiers are only valid as constructor calls",
                    ))
                }
            }
            _ => Err(AmdlParserError::new(token.offset, "expected expression")),
        }
    }

    fn parse_array(&mut self) -> Result<AmdlExpr, AmdlParserError> {
        self.expect(TokenKindDiscriminant::LBracket)?;
        let mut values = Vec::new();
        while !self.check(TokenKindDiscriminant::RBracket) {
            values.push(self.parse_expr()?);
            if self.check(TokenKindDiscriminant::Comma) {
                self.advance();
            } else if !self.check(TokenKindDiscriminant::RBracket) {
                return Err(self.error_here("expected `,` or `]`"));
            }
        }
        self.expect(TokenKindDiscriminant::RBracket)?;
        Ok(AmdlExpr::Array(values))
    }

    fn parse_path(&mut self) -> Result<Vec<String>, AmdlParserError> {
        let mut path = vec![self.expect_ident()?];
        while self.check(TokenKindDiscriminant::Dot) {
            self.advance();
            path.push(self.expect_ident()?);
        }
        Ok(path)
    }

    fn parse_call_arguments(&mut self) -> Result<Vec<AmdlArgument>, AmdlParserError> {
        self.expect(TokenKindDiscriminant::LParen)?;
        let mut arguments = Vec::new();
        while !self.check(TokenKindDiscriminant::RParen) {
            if let Some(Token {
                kind: TokenKind::Ident(name),
                ..
            }) = self.peek()
            {
                if self.peek_n_is(1, TokenKindDiscriminant::Colon) {
                    let name = name.clone();
                    self.advance();
                    self.advance();
                    let value = self.parse_expr()?;
                    arguments.push(AmdlArgument::Named { name, value });
                } else {
                    arguments.push(AmdlArgument::Positional {
                        value: self.parse_expr()?,
                    });
                }
            } else {
                arguments.push(AmdlArgument::Positional {
                    value: self.parse_expr()?,
                });
            }

            if self.check(TokenKindDiscriminant::Comma) {
                self.advance();
            } else if !self.check(TokenKindDiscriminant::RParen) {
                return Err(self.error_here("expected `,` or `)`"));
            }
        }
        self.expect(TokenKindDiscriminant::RParen)?;
        Ok(arguments)
    }

    fn expect_ident_value(&mut self, expected: &str) -> Result<(), AmdlParserError> {
        let token = self
            .peek()
            .ok_or_else(|| self.error_here(format!("expected `{expected}`")))?;
        match &token.kind {
            TokenKind::Ident(value) if value == expected => {
                self.advance();
                Ok(())
            }
            _ => Err(AmdlParserError::new(
                token.offset,
                format!("expected `{expected}`"),
            )),
        }
    }

    fn expect_ident(&mut self) -> Result<String, AmdlParserError> {
        let token = self
            .peek()
            .ok_or_else(|| self.error_here("expected identifier"))?
            .clone();
        match token.kind {
            TokenKind::Ident(value) => {
                self.advance();
                Ok(value)
            }
            _ => Err(AmdlParserError::new(token.offset, "expected identifier")),
        }
    }

    fn expect_ident_or_number_name(&mut self) -> Result<String, AmdlParserError> {
        let token = self
            .peek()
            .ok_or_else(|| self.error_here("expected identifier"))?
            .clone();
        match token.kind {
            TokenKind::Ident(value) => {
                self.advance();
                Ok(value)
            }
            TokenKind::Number { value, unit: None } if value.fract() == 0.0 => {
                self.advance();
                Ok(format!("{value:.0}"))
            }
            _ => Err(AmdlParserError::new(token.offset, "expected identifier")),
        }
    }

    fn expect(&mut self, expected: TokenKindDiscriminant) -> Result<(), AmdlParserError> {
        if self.check(expected) {
            self.advance();
            Ok(())
        } else {
            Err(self.error_here(format!("expected `{}`", expected.display())))
        }
    }

    fn check(&self, expected: TokenKindDiscriminant) -> bool {
        self.peek()
            .is_some_and(|token| expected.matches(&token.kind))
    }

    fn peek_n_is(&self, offset: usize, expected: TokenKindDiscriminant) -> bool {
        self.tokens
            .get(self.cursor + offset)
            .is_some_and(|token| expected.matches(&token.kind))
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    fn advance(&mut self) {
        self.cursor += 1;
    }

    fn is_eof(&self) -> bool {
        self.cursor >= self.tokens.len()
    }

    fn error_here(&self, message: impl Into<String>) -> AmdlParserError {
        let offset = self
            .peek()
            .map(|token| token.offset)
            .unwrap_or(self.source.len());
        AmdlParserError::new(offset, message)
    }
}

#[derive(Clone, Copy)]
enum TokenKindDiscriminant {
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Equal,
    Colon,
    Comma,
    Dot,
}

impl TokenKindDiscriminant {
    fn matches(self, kind: &TokenKind) -> bool {
        matches!(
            (self, kind),
            (Self::LBrace, TokenKind::LBrace)
                | (Self::RBrace, TokenKind::RBrace)
                | (Self::LParen, TokenKind::LParen)
                | (Self::RParen, TokenKind::RParen)
                | (Self::LBracket, TokenKind::LBracket)
                | (Self::RBracket, TokenKind::RBracket)
                | (Self::Equal, TokenKind::Equal)
                | (Self::Colon, TokenKind::Colon)
                | (Self::Comma, TokenKind::Comma)
                | (Self::Dot, TokenKind::Dot)
        )
    }

    fn display(self) -> &'static str {
        match self {
            Self::LBrace => "{",
            Self::RBrace => "}",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::LBracket => "[",
            Self::RBracket => "]",
            Self::Equal => "=",
            Self::Colon => ":",
            Self::Comma => ",",
            Self::Dot => ".",
        }
    }
}

fn lex(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    let bytes = source.as_bytes();

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        match byte {
            b' ' | b'\t' | b'\r' | b'\n' => cursor += 1,
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor += 2;
                while cursor < bytes.len() && bytes[cursor] != b'\n' {
                    cursor += 1;
                }
            }
            b'#' => {
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor] != b'\n' {
                    cursor += 1;
                }
            }
            b'{' => push_simple(&mut tokens, TokenKind::LBrace, &mut cursor),
            b'}' => push_simple(&mut tokens, TokenKind::RBrace, &mut cursor),
            b'(' => push_simple(&mut tokens, TokenKind::LParen, &mut cursor),
            b')' => push_simple(&mut tokens, TokenKind::RParen, &mut cursor),
            b'[' => push_simple(&mut tokens, TokenKind::LBracket, &mut cursor),
            b']' => push_simple(&mut tokens, TokenKind::RBracket, &mut cursor),
            b'=' => push_simple(&mut tokens, TokenKind::Equal, &mut cursor),
            b':' => push_simple(&mut tokens, TokenKind::Colon, &mut cursor),
            b',' => push_simple(&mut tokens, TokenKind::Comma, &mut cursor),
            b'.' => push_simple(&mut tokens, TokenKind::Dot, &mut cursor),
            b'"' => {
                let offset = cursor;
                cursor += 1;
                let mut value = String::new();
                while cursor < bytes.len() {
                    match bytes[cursor] {
                        b'"' => {
                            cursor += 1;
                            break;
                        }
                        b'\\' if cursor + 1 < bytes.len() => {
                            cursor += 1;
                            let escaped = match bytes[cursor] {
                                b'"' => '"',
                                b'\\' => '\\',
                                b'n' => '\n',
                                b'r' => '\r',
                                b't' => '\t',
                                other => other as char,
                            };
                            value.push(escaped);
                            cursor += 1;
                        }
                        other => {
                            value.push(other as char);
                            cursor += 1;
                        }
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::String(value),
                    offset,
                });
            }
            b'-' | b'0'..=b'9' => {
                let offset = cursor;
                cursor += 1;
                while cursor < bytes.len()
                    && matches!(bytes[cursor], b'0'..=b'9' | b'.')
                {
                    cursor += 1;
                }
                let number_end = cursor;
                while cursor < bytes.len()
                    && matches!(bytes[cursor], b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'%')
                {
                    cursor += 1;
                }
                let number_text = &source[offset..number_end];
                let value = number_text.parse::<f64>().unwrap_or(0.0);
                let unit = (cursor > number_end).then(|| source[number_end..cursor].to_string());
                tokens.push(Token {
                    kind: TokenKind::Number { value, unit },
                    offset,
                });
            }
            _ if is_ident_start(byte) => {
                let offset = cursor;
                cursor += 1;
                while cursor < bytes.len() && is_ident_continue(bytes[cursor]) {
                    cursor += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Ident(source[offset..cursor].to_string()),
                    offset,
                });
            }
            _ => cursor += 1,
        }
    }
    tokens
}

fn push_simple(tokens: &mut Vec<Token>, kind: TokenKind, cursor: &mut usize) {
    tokens.push(Token {
        kind,
        offset: *cursor,
    });
    *cursor += 1;
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_asset_model() {
        let source = r##"
model Crate {
  mesh = primitive.box(size: [1, 1, 1])
  material = material(base_color: "#8a5a2b", roughness: 0.75)
  collider = collider.box(size: [1, 1, 1])
  rigidbody = dynamic
}
"##;

        let document = parse_amdl(source).unwrap();
        AmdlValidator::validate(&document).unwrap();

        assert_eq!(document.models[0].name, "Crate");
        assert_eq!(document.models[0].statements.len(), 4);
    }

    #[test]
    fn parses_blocks_and_units() {
        let source = r#"
model Barrel {
  mesh = asset("models/barrel.glb")

  collider capsule {
    radius = 0.45m
    height = 1.2m
  }

  rigidbody dynamic {
    mass = 18kg
    friction = 0.8
  }

  socket Top {
    position = [0, 1.2, 0]
    rotation = [0, 0, 0]
  }

  lod 1 {
    mesh = asset("models/barrel_low.glb")
    distance = 25m
  }
}
"#;

        let document = parse_amdl(source).unwrap();
        AmdlValidator::validate(&document).unwrap();

        assert!(matches!(
            &document.models[0].statements[1],
            AmdlStatement::Block(AmdlNamedBlock {
                kind,
                name: Some(name),
                ..
            }) if kind == "collider" && name == "capsule"
        ));
    }

    #[test]
    fn parses_constructor_block_expressions() {
        let source = r##"
model Lantern {
  mesh = asset("models/lantern.glb")

  material = material {
    base_color = "#f5d67b"
    emissive = [1.0, 0.75, 0.25]
    roughness = 0.4
  }

  collider = collider.box {
    size = [0.4, 0.8, 0.4]
  }

  rigidbody = dynamic {
    mass = 2kg
    friction = 0.6
  }
}
"##;

        let document = parse_amdl(source).unwrap();
        AmdlValidator::validate(&document).unwrap();

        let material = document.models[0]
            .statements
            .iter()
            .find_map(|statement| match statement {
                AmdlStatement::Assignment { name, value } if name == "material" => Some(value),
                _ => None,
            })
            .unwrap();

        assert!(matches!(
            material,
            AmdlExpr::Object(AmdlObject { path, statements })
                if path == &vec!["material".to_string()] && statements.len() == 3
        ));
    }

    #[test]
    fn validation_rejects_model_without_mesh() {
        let document = parse_amdl("model Empty { rigidbody = static }").unwrap();
        let errors = AmdlValidator::validate(&document).unwrap_err();

        assert!(errors
            .iter()
            .any(|error| error.message.contains("must declare a mesh")));
    }
}

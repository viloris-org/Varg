//! Aster Model Language (`.amdl`) parser and diagnostics.
//!
//! AMDL is an AI-facing, system-parseable declaration language for reusable
//! model assets. It intentionally has one canonical shape: explicit declaration
//! blocks with discriminator fields such as `kind`, `shape`, and `mode`. It
//! does not support alternate call-style syntax, loops, conditions, scene
//! placement, or gameplay logic.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

/// Required AMDL file header.
pub const AMDL_HEADER: &str = "amdl";

/// Parsed and validated AMDL document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlDocument {
    /// Declared models in source order.
    pub models: Vec<AmdlModelDecl>,
}

/// Canonical model declaration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlModelDecl {
    /// Model identifier.
    pub name: String,
    /// Required mesh declaration.
    pub mesh: AmdlMeshDecl,
    /// Optional material declaration.
    pub material: Option<AmdlMaterialDecl>,
    /// Optional collider declaration.
    pub collider: Option<AmdlColliderDecl>,
    /// Optional rigidbody declaration.
    pub rigidbody: Option<AmdlRigidbodyDecl>,
    /// Attachment sockets.
    pub sockets: Vec<AmdlSocketDecl>,
    /// Level-of-detail declarations.
    pub lods: Vec<AmdlLodDecl>,
    /// Free-form metadata values.
    pub metadata: BTreeMap<String, AmdlValue>,
}

/// Canonical mesh declaration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlMeshDecl {
    /// Mesh source.
    pub source: AmdlMeshSource,
}

/// Canonical mesh source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AmdlMeshSource {
    /// External mesh asset path.
    Asset {
        /// Asset-root-relative path.
        path: String,
    },
    /// Engine primitive mesh.
    Primitive {
        /// Primitive kind, for example `box` or `sphere`.
        primitive: AmdlPrimitiveKind,
        /// Primitive parameters.
        parameters: BTreeMap<String, AmdlValue>,
    },
}

/// Supported primitive mesh kinds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmdlPrimitiveKind {
    /// Box/cuboid primitive.
    Box,
    /// Sphere primitive.
    Sphere,
    /// Capsule primitive.
    Capsule,
    /// Cylinder primitive.
    Cylinder,
    /// Plane primitive.
    Plane,
}

/// Canonical material declaration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AmdlMaterialDecl {
    /// Inline simple material parameters.
    Inline {
        /// Material parameters.
        parameters: BTreeMap<String, AmdlValue>,
    },
    /// External material reference.
    Ref {
        /// Asset-root-relative material path or stable asset id.
        path: String,
    },
}

/// Canonical collider declaration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlColliderDecl {
    /// Collider shape.
    pub shape: AmdlColliderShape,
    /// Collider parameters.
    pub parameters: BTreeMap<String, AmdlValue>,
}

/// Supported collider shapes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmdlColliderShape {
    /// Box collider.
    Box,
    /// Sphere collider.
    Sphere,
    /// Capsule collider.
    Capsule,
    /// Cylinder collider.
    Cylinder,
    /// Mesh collider.
    Mesh,
}

/// Canonical rigidbody declaration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlRigidbodyDecl {
    /// Rigidbody mode.
    pub mode: AmdlRigidbodyMode,
    /// Rigidbody parameters.
    pub parameters: BTreeMap<String, AmdlValue>,
}

/// Supported rigidbody modes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmdlRigidbodyMode {
    /// Static body.
    Static,
    /// Dynamic body.
    Dynamic,
    /// Kinematic body.
    Kinematic,
}

/// Canonical socket declaration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlSocketDecl {
    /// Socket name.
    pub name: String,
    /// Socket parameters.
    pub parameters: BTreeMap<String, AmdlValue>,
}

/// Canonical LOD declaration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmdlLodDecl {
    /// LOD index.
    pub index: u32,
    /// Required LOD mesh.
    pub mesh: AmdlMeshDecl,
    /// Optional switch distance.
    pub distance: Option<AmdlValue>,
    /// Additional LOD parameters.
    pub parameters: BTreeMap<String, AmdlValue>,
}

/// AMDL value.
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
    /// Symbol literal for controlled enum-like extension values.
    Symbol(String),
    /// List value.
    List(Vec<AmdlValue>),
}

/// Structured AMDL diagnostic shared by editor and AI tooling.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AmdlDiagnostic {
    /// Stable diagnostic code.
    pub code: String,
    /// Diagnostic severity.
    pub severity: String,
    /// One-based line number when known.
    pub line: Option<usize>,
    /// One-based column number when known.
    pub column: Option<usize>,
    /// Diagnostic message.
    pub message: String,
    /// Concrete fix suggestion.
    pub suggestion: String,
    /// Source line when known.
    pub source_line: Option<String>,
}

/// Parses and validates an `.amdl` document.
pub fn parse_amdl(source: &str) -> Result<AmdlDocument, AmdlParserError> {
    Parser::new(source).parse_document()
}

/// Parses, validates, normalizes, and compiles an `.amdl` source document.
pub fn compile_amdl(source: &str) -> Result<AmdlDocument, Vec<AmdlDiagnostic>> {
    parse_amdl(source).map_err(|error| vec![diagnostic_from_parse_error(source, error)])
}

/// Returns editor/AI diagnostics for an `.amdl` source document.
pub fn diagnose_amdl(source: &str) -> Vec<AmdlDiagnostic> {
    compile_amdl(source).err().unwrap_or_default()
}

/// `.amdl` parser error with byte position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmdlParserError {
    /// Byte offset in the source.
    pub offset: usize,
    /// Stable diagnostic code.
    pub code: &'static str,
    /// Human-readable parser message.
    pub message: String,
    /// Concrete fix suggestion.
    pub suggestion: String,
}

impl AmdlParserError {
    fn new(
        offset: usize,
        code: &'static str,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            offset,
            code,
            message: message.into(),
            suggestion: suggestion.into(),
        }
    }
}

impl fmt::Display for AmdlParserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for AmdlParserError {}

/// Semantic validator for `.amdl`.
#[derive(Clone, Debug, Default)]
pub struct AmdlValidator;

impl AmdlValidator {
    /// Validates an already parsed typed AMDL document.
    pub fn validate(document: &AmdlDocument) -> Result<(), Vec<AmdlDiagnostic>> {
        let mut errors = Vec::new();
        if document.models.is_empty() {
            errors.push(AmdlDiagnostic {
                code: "AMDL_MODEL_REQUIRED".into(),
                severity: "error".into(),
                line: None,
                column: None,
                message: "document must contain at least one model declaration".into(),
                suggestion: "Add `model Name { mesh { kind = primitive.box size = [1, 1, 1] } }`."
                    .into(),
                source_line: None,
            });
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Ident(String),
    String(String),
    Number { value: f64, unit: Option<String> },
    Unknown(char),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Equal,
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
        self.expect_ident_value(AMDL_HEADER)?;
        if let Some(Token {
            kind: TokenKind::Number { .. },
            offset,
        }) = self.peek()
        {
            return Err(AmdlParserError::new(
                *offset,
                "AMDL_HEADER",
                "AMDL no longer uses a numeric version header",
                "Start the file with `amdl`, then declare one or more `model` blocks.",
            ));
        }

        let mut models = Vec::new();
        while !self.is_eof() {
            models.push(self.parse_model()?);
        }

        if models.is_empty() {
            return Err(self.error_here(
                "AMDL_MODEL_REQUIRED",
                "expected at least one `model` declaration",
                "Add `model Name { mesh { kind = primitive.box size = [1, 1, 1] } }`.",
            ));
        }

        Ok(AmdlDocument { models })
    }

    fn parse_model(&mut self) -> Result<AmdlModelDecl, AmdlParserError> {
        self.expect_ident_value("model")?;
        let name = self.expect_ident()?;
        self.expect(TokenKindDiscriminant::LBrace)?;

        let mut mesh = None;
        let mut material = None;
        let mut collider = None;
        let mut rigidbody = None;
        let mut sockets = Vec::new();
        let mut lods = Vec::new();
        let mut metadata = BTreeMap::new();

        while !self.check(TokenKindDiscriminant::RBrace) {
            if self.is_eof() {
                return Err(self.error_here(
                    "AMDL_PARSE",
                    "expected `}`",
                    "Close the model block.",
                ));
            }
            let field = self.expect_ident()?;
            if self.check(TokenKindDiscriminant::Equal) {
                return Err(self.error_at_previous(
                    "AMDL_LEGACY_SYNTAX",
                    format!("legacy assignment syntax is not supported for `{field}`"),
                    format!(
                        "Use canonical AMDL block syntax, for example `{field} {{ kind = ... }}`."
                    ),
                ));
            }
            match field.as_str() {
                "mesh" => {
                    if mesh.is_some() {
                        return Err(self.error_at_previous(
                            "AMDL_DUPLICATE_FIELD",
                            "model must not declare more than one top-level mesh",
                            "Remove the duplicate `mesh` declaration.",
                        ));
                    }
                    mesh = Some(self.parse_mesh_decl()?);
                }
                "material" => {
                    if material.is_some() {
                        return Err(self.error_at_previous(
                            "AMDL_DUPLICATE_FIELD",
                            "model must not declare more than one material",
                            "Remove the duplicate `material` declaration.",
                        ));
                    }
                    material = Some(self.parse_material_decl()?);
                }
                "collider" => {
                    if collider.is_some() {
                        return Err(self.error_at_previous(
                            "AMDL_DUPLICATE_FIELD",
                            "model must not declare more than one collider",
                            "Remove the duplicate `collider` declaration.",
                        ));
                    }
                    collider = Some(self.parse_collider_decl()?);
                }
                "rigidbody" => {
                    if rigidbody.is_some() {
                        return Err(self.error_at_previous(
                            "AMDL_DUPLICATE_FIELD",
                            "model must not declare more than one rigidbody",
                            "Remove the duplicate `rigidbody` declaration.",
                        ));
                    }
                    rigidbody = Some(self.parse_rigidbody_decl()?);
                }
                "socket" => sockets.push(self.parse_socket_decl()?),
                "lod" => lods.push(self.parse_lod_decl()?),
                "metadata" => {
                    let values = self.parse_record()?;
                    metadata.extend(values);
                }
                other => {
                    return Err(self.error_at_previous(
                        "AMDL_UNKNOWN_FIELD",
                        format!("unsupported model field `{other}`"),
                        "Use only `mesh`, `material`, `collider`, `rigidbody`, `socket`, `lod`, or `metadata`.",
                    ));
                }
            }
        }
        self.expect(TokenKindDiscriminant::RBrace)?;

        let mesh = mesh.ok_or_else(|| {
            self.error_at_previous(
                "AMDL_MESH_REQUIRED",
                format!("model `{name}` must declare a mesh"),
                "Add `mesh { kind = primitive.box size = [1, 1, 1] }` or `mesh { kind = asset path = \"models/name.glb\" }`.",
            )
        })?;

        Ok(AmdlModelDecl {
            name,
            mesh,
            material,
            collider,
            rigidbody,
            sockets,
            lods,
            metadata,
        })
    }

    fn parse_mesh_decl(&mut self) -> Result<AmdlMeshDecl, AmdlParserError> {
        if !self.check(TokenKindDiscriminant::LBrace) {
            let offset = self
                .peek()
                .map(|token| token.offset)
                .unwrap_or(self.previous_offset());
            return Err(AmdlParserError::new(
                offset,
                "AMDL_CANONICAL_BLOCK",
                "mesh must be declared as a block with a `kind` field",
                "Write `mesh { kind = primitive.box size = [1, 1, 1] }` or `mesh { kind = asset path = \"models/foo.glb\" }`.",
            ));
        }
        let mut parameters = self.parse_record()?;
        let kind = take_required_symbol_path(&mut parameters, "kind", self.previous_offset())?;
        if kind.len() == 1 && kind[0] == "asset" {
            let path = take_required_string(&mut parameters, "path", self.previous_offset())?;
            reject_unknown_fields(
                &parameters,
                self.previous_offset(),
                "mesh asset",
                &["kind", "path"],
            )?;
            Ok(AmdlMeshDecl {
                source: AmdlMeshSource::Asset { path },
            })
        } else if kind.len() == 2 && kind[0] == "primitive" {
            let kind = parse_primitive_kind(&kind[1], self.previous_offset())?;
            Ok(AmdlMeshDecl {
                source: AmdlMeshSource::Primitive {
                    primitive: kind,
                    parameters,
                },
            })
        } else {
            Err(self.error_at_previous(
                "AMDL_MESH_KIND",
                format!("unsupported mesh kind `{}`", kind.join(".")),
                "Use `mesh { kind = asset path = \"models/foo.glb\" }` or `mesh { kind = primitive.box size = [1, 1, 1] }`.",
            ))
        }
    }

    fn parse_material_decl(&mut self) -> Result<AmdlMaterialDecl, AmdlParserError> {
        if !self.check(TokenKindDiscriminant::LBrace) {
            let offset = self
                .peek()
                .map(|token| token.offset)
                .unwrap_or(self.previous_offset());
            return Err(AmdlParserError::new(
                offset,
                "AMDL_CANONICAL_BLOCK",
                "material must be declared as a block with a `kind` field",
                "Write `material { kind = inline ... }` or `material { kind = ref path = \"materials/foo.amat\" }`.",
            ));
        }
        let mut parameters = self.parse_record()?;
        let kind = take_required_symbol_path(&mut parameters, "kind", self.previous_offset())?;
        if kind.len() != 1 {
            return Err(self.error_at_previous(
                "AMDL_MATERIAL_KIND",
                format!("unsupported material kind `{}`", kind.join(".")),
                "Use `material { kind = inline ... }` or `material { kind = ref path = \"materials/foo.amat\" }`.",
            ));
        }
        match kind[0].as_str() {
            "inline" => Ok(AmdlMaterialDecl::Inline {
                parameters,
            }),
            "ref" => {
                let path = take_required_string(&mut parameters, "path", self.previous_offset())?;
                reject_unknown_fields(
                    &parameters,
                    self.previous_offset(),
                    "material ref",
                    &["kind", "path"],
                )?;
                Ok(AmdlMaterialDecl::Ref { path })
            }
            _ => Err(self.error_at_previous(
                "AMDL_MATERIAL_KIND",
                format!("unsupported material kind `{}`", kind.join(".")),
                "Use `material { kind = inline ... }` or `material { kind = ref path = \"materials/foo.amat\" }`.",
            )),
        }
    }

    fn parse_collider_decl(&mut self) -> Result<AmdlColliderDecl, AmdlParserError> {
        if !self.check(TokenKindDiscriminant::LBrace) {
            let offset = self
                .peek()
                .map(|token| token.offset)
                .unwrap_or(self.previous_offset());
            return Err(AmdlParserError::new(
                offset,
                "AMDL_CANONICAL_BLOCK",
                "collider must be declared as a block with a `shape` field",
                "Write `collider { shape = box size = [1, 1, 1] }`.",
            ));
        }
        let mut parameters = self.parse_record()?;
        let shape = take_required_symbol(&mut parameters, "shape", self.previous_offset())?;
        let shape = parse_collider_shape(&shape, self.previous_offset())?;
        Ok(AmdlColliderDecl { shape, parameters })
    }

    fn parse_rigidbody_decl(&mut self) -> Result<AmdlRigidbodyDecl, AmdlParserError> {
        if !self.check(TokenKindDiscriminant::LBrace) {
            let offset = self
                .peek()
                .map(|token| token.offset)
                .unwrap_or(self.previous_offset());
            return Err(AmdlParserError::new(
                offset,
                "AMDL_CANONICAL_BLOCK",
                "rigidbody must be declared as a block with a `mode` field",
                "Write `rigidbody { mode = dynamic mass = 12kg }`.",
            ));
        }
        let mut parameters = self.parse_record()?;
        let mode = take_required_symbol(&mut parameters, "mode", self.previous_offset())?;
        let mode = parse_rigidbody_mode(&mode, self.previous_offset())?;
        Ok(AmdlRigidbodyDecl { mode, parameters })
    }

    fn parse_socket_decl(&mut self) -> Result<AmdlSocketDecl, AmdlParserError> {
        let name = self.expect_ident_or_number_name()?;
        Ok(AmdlSocketDecl {
            name,
            parameters: self.parse_record()?,
        })
    }

    fn parse_lod_decl(&mut self) -> Result<AmdlLodDecl, AmdlParserError> {
        let index = self.expect_u32_name()?;
        self.expect(TokenKindDiscriminant::LBrace)?;
        let mut mesh = None;
        let mut distance = None;
        let mut parameters = BTreeMap::new();
        while !self.check(TokenKindDiscriminant::RBrace) {
            if self.is_eof() {
                return Err(self.error_here("AMDL_PARSE", "expected `}`", "Close the LOD block."));
            }
            let key = self.expect_ident()?;
            if key == "mesh" {
                mesh = Some(self.parse_mesh_decl()?);
            } else {
                self.expect(TokenKindDiscriminant::Equal)?;
                let value = self.parse_value()?;
                if key == "distance" {
                    distance = Some(value.clone());
                } else {
                    parameters.insert(key, value);
                }
            }
        }
        self.expect(TokenKindDiscriminant::RBrace)?;
        let mesh = mesh.ok_or_else(|| {
            self.error_at_previous(
                "AMDL_LOD_MESH_REQUIRED",
                "`lod` block requires a mesh declaration",
                "Add `mesh { kind = asset path = \"models/foo_lod.glb\" }` inside the LOD block.",
            )
        })?;
        Ok(AmdlLodDecl {
            index,
            mesh,
            distance,
            parameters,
        })
    }

    fn parse_record(&mut self) -> Result<BTreeMap<String, AmdlValue>, AmdlParserError> {
        self.expect(TokenKindDiscriminant::LBrace)?;
        let mut values = BTreeMap::new();
        while !self.check(TokenKindDiscriminant::RBrace) {
            if self.is_eof() {
                return Err(self.error_here("AMDL_PARSE", "expected `}`", "Close the block."));
            }
            let key = self.expect_ident()?;
            self.expect(TokenKindDiscriminant::Equal)?;
            let value = self.parse_value()?;
            if values.insert(key.clone(), value).is_some() {
                return Err(self.error_at_previous(
                    "AMDL_DUPLICATE_FIELD",
                    format!("duplicate field `{key}`"),
                    "Remove the duplicate field or merge the values.",
                ));
            }
        }
        self.expect(TokenKindDiscriminant::RBrace)?;
        Ok(values)
    }

    fn parse_value(&mut self) -> Result<AmdlValue, AmdlParserError> {
        if self.check(TokenKindDiscriminant::LBracket) {
            return self.parse_list();
        }
        let token = self
            .peek()
            .ok_or_else(|| {
                self.error_here(
                    "AMDL_VALUE",
                    "expected value",
                    "Write a string, number, boolean, symbol, or list.",
                )
            })?
            .clone();
        match token.kind {
            TokenKind::String(value) => {
                self.advance();
                Ok(AmdlValue::String(value))
            }
            TokenKind::Number { value, unit } => {
                self.advance();
                Ok(AmdlValue::Number { value, unit })
            }
            TokenKind::Ident(value) => {
                self.advance();
                let mut symbol = value;
                let mut had_dot = false;
                while self.check(TokenKindDiscriminant::Dot) {
                    self.advance();
                    had_dot = true;
                    symbol.push('.');
                    symbol.push_str(&self.expect_ident()?);
                }
                match (symbol.as_str(), had_dot) {
                    ("true", false) => Ok(AmdlValue::Bool(true)),
                    ("false", false) => Ok(AmdlValue::Bool(false)),
                    _ => Ok(AmdlValue::Symbol(symbol)),
                }
            }
            TokenKind::Unknown(ch) => Err(AmdlParserError::new(
                token.offset,
                "AMDL_INVALID_TOKEN",
                format!("unsupported token `{ch}`"),
                "Use only strict AMDL block syntax. Do not use function-call syntax such as `asset(...)` or `primitive.box(...)`.",
            )),
            _ => Err(AmdlParserError::new(
                token.offset,
                "AMDL_VALUE",
                "expected value",
                "Write a string, number, boolean, symbol, or list.",
            )),
        }
    }

    fn parse_list(&mut self) -> Result<AmdlValue, AmdlParserError> {
        self.expect(TokenKindDiscriminant::LBracket)?;
        let mut values = Vec::new();
        while !self.check(TokenKindDiscriminant::RBracket) {
            values.push(self.parse_value()?);
            if self.check(TokenKindDiscriminant::Comma) {
                self.advance();
            } else if !self.check(TokenKindDiscriminant::RBracket) {
                return Err(self.error_here(
                    "AMDL_LIST_SEPARATOR",
                    "expected `,` or `]`",
                    "Separate list values with commas, for example `[1, 1, 1]`.",
                ));
            }
        }
        self.expect(TokenKindDiscriminant::RBracket)?;
        Ok(AmdlValue::List(values))
    }

    fn expect_ident_value(&mut self, expected: &str) -> Result<(), AmdlParserError> {
        let token = self.peek().ok_or_else(|| {
            self.error_here(
                "AMDL_PARSE",
                format!("expected `{expected}`"),
                format!("Start with `{expected}`."),
            )
        })?;
        match &token.kind {
            TokenKind::Ident(value) if value == expected => {
                self.advance();
                Ok(())
            }
            _ => Err(AmdlParserError::new(
                token.offset,
                "AMDL_PARSE",
                format!("expected `{expected}`"),
                format!("Write `{expected}` here."),
            )),
        }
    }

    fn expect_ident(&mut self) -> Result<String, AmdlParserError> {
        let token = self
            .peek()
            .ok_or_else(|| {
                self.error_here(
                    "AMDL_IDENT",
                    "expected identifier",
                    "Write an identifier here.",
                )
            })?
            .clone();
        match token.kind {
            TokenKind::Ident(value) => {
                self.advance();
                Ok(value)
            }
            _ => Err(AmdlParserError::new(
                token.offset,
                "AMDL_IDENT",
                "expected identifier",
                "Write an identifier here.",
            )),
        }
    }

    fn expect_ident_or_number_name(&mut self) -> Result<String, AmdlParserError> {
        let token = self
            .peek()
            .ok_or_else(|| {
                self.error_here(
                    "AMDL_IDENT",
                    "expected identifier",
                    "Write an identifier here.",
                )
            })?
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
            _ => Err(AmdlParserError::new(
                token.offset,
                "AMDL_IDENT",
                "expected identifier",
                "Write an identifier here.",
            )),
        }
    }

    fn expect_u32_name(&mut self) -> Result<u32, AmdlParserError> {
        let token = self
            .peek()
            .ok_or_else(|| {
                self.error_here(
                    "AMDL_LOD_INDEX",
                    "expected LOD index",
                    "Write an integer after `lod`, for example `lod 1 { ... }`.",
                )
            })?
            .clone();
        match token.kind {
            TokenKind::Number { value, unit: None } if value.fract() == 0.0 && value >= 0.0 => {
                self.advance();
                Ok(value as u32)
            }
            _ => Err(AmdlParserError::new(
                token.offset,
                "AMDL_LOD_INDEX",
                "expected LOD index",
                "Write an integer after `lod`, for example `lod 1 { ... }`.",
            )),
        }
    }

    fn expect(&mut self, expected: TokenKindDiscriminant) -> Result<(), AmdlParserError> {
        if self.check(expected) {
            self.advance();
            Ok(())
        } else {
            Err(self.error_here(
                "AMDL_PARSE",
                format!("expected `{}`", expected.display()),
                format!("Write `{}` here.", expected.display()),
            ))
        }
    }

    fn check(&self, expected: TokenKindDiscriminant) -> bool {
        self.peek()
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

    fn previous_offset(&self) -> usize {
        self.cursor
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map(|token| token.offset)
            .unwrap_or(0)
    }

    fn error_here(
        &self,
        code: &'static str,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> AmdlParserError {
        let offset = self
            .peek()
            .map(|token| token.offset)
            .unwrap_or(self.source.len());
        AmdlParserError::new(offset, code, message, suggestion)
    }

    fn error_at_previous(
        &self,
        code: &'static str,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> AmdlParserError {
        AmdlParserError::new(self.previous_offset(), code, message, suggestion)
    }
}

#[derive(Clone, Copy)]
enum TokenKindDiscriminant {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Equal,
    Comma,
    Dot,
}

impl TokenKindDiscriminant {
    fn matches(self, kind: &TokenKind) -> bool {
        matches!(
            (self, kind),
            (Self::LBrace, TokenKind::LBrace)
                | (Self::RBrace, TokenKind::RBrace)
                | (Self::LBracket, TokenKind::LBracket)
                | (Self::RBracket, TokenKind::RBracket)
                | (Self::Equal, TokenKind::Equal)
                | (Self::Comma, TokenKind::Comma)
                | (Self::Dot, TokenKind::Dot)
        )
    }

    fn display(self) -> &'static str {
        match self {
            Self::LBrace => "{",
            Self::RBrace => "}",
            Self::LBracket => "[",
            Self::RBracket => "]",
            Self::Equal => "=",
            Self::Comma => ",",
            Self::Dot => ".",
        }
    }
}

fn parse_primitive_kind(kind: &str, offset: usize) -> Result<AmdlPrimitiveKind, AmdlParserError> {
    match kind {
        "box" => Ok(AmdlPrimitiveKind::Box),
        "sphere" => Ok(AmdlPrimitiveKind::Sphere),
        "capsule" => Ok(AmdlPrimitiveKind::Capsule),
        "cylinder" => Ok(AmdlPrimitiveKind::Cylinder),
        "plane" => Ok(AmdlPrimitiveKind::Plane),
        _ => Err(AmdlParserError::new(
            offset,
            "AMDL_PRIMITIVE_KIND",
            format!("unsupported primitive kind `{kind}`"),
            "Use one of `primitive.box`, `primitive.sphere`, `primitive.capsule`, `primitive.cylinder`, or `primitive.plane`.",
        )),
    }
}

fn parse_collider_shape(shape: &str, offset: usize) -> Result<AmdlColliderShape, AmdlParserError> {
    match shape {
        "box" => Ok(AmdlColliderShape::Box),
        "sphere" => Ok(AmdlColliderShape::Sphere),
        "capsule" => Ok(AmdlColliderShape::Capsule),
        "cylinder" => Ok(AmdlColliderShape::Cylinder),
        "mesh" => Ok(AmdlColliderShape::Mesh),
        _ => Err(AmdlParserError::new(
            offset,
            "AMDL_COLLIDER_SHAPE",
            format!("unsupported collider shape `{shape}`"),
            "Use `collider { shape = box }`, `collider { shape = sphere }`, `collider { shape = capsule }`, `collider { shape = cylinder }`, or `collider { shape = mesh }`.",
        )),
    }
}

fn parse_rigidbody_mode(mode: &str, offset: usize) -> Result<AmdlRigidbodyMode, AmdlParserError> {
    match mode {
        "static" => Ok(AmdlRigidbodyMode::Static),
        "dynamic" => Ok(AmdlRigidbodyMode::Dynamic),
        "kinematic" => Ok(AmdlRigidbodyMode::Kinematic),
        _ => Err(AmdlParserError::new(
            offset,
            "AMDL_RIGIDBODY_MODE",
            format!("unsupported rigidbody mode `{mode}`"),
            "Use `rigidbody { mode = static }`, `rigidbody { mode = dynamic }`, or `rigidbody { mode = kinematic }`.",
        )),
    }
}

fn take_required_string(
    parameters: &mut BTreeMap<String, AmdlValue>,
    field: &str,
    offset: usize,
) -> Result<String, AmdlParserError> {
    match parameters.remove(field) {
        Some(AmdlValue::String(value)) => Ok(value),
        Some(_) => Err(AmdlParserError::new(
            offset,
            "AMDL_FIELD_TYPE",
            format!("`{field}` must be a string"),
            format!("Write `{field} = \"path/or/id\"`."),
        )),
        None => Err(AmdlParserError::new(
            offset,
            "AMDL_FIELD_REQUIRED",
            format!("missing required field `{field}`"),
            format!("Add `{field} = \"path/or/id\"`."),
        )),
    }
}

fn take_required_symbol(
    parameters: &mut BTreeMap<String, AmdlValue>,
    field: &str,
    offset: usize,
) -> Result<String, AmdlParserError> {
    match parameters.remove(field) {
        Some(AmdlValue::Symbol(value)) => Ok(value),
        Some(_) => Err(AmdlParserError::new(
            offset,
            "AMDL_FIELD_TYPE",
            format!("`{field}` must be a symbol"),
            format!("Write `{field} = name`."),
        )),
        None => Err(AmdlParserError::new(
            offset,
            "AMDL_FIELD_REQUIRED",
            format!("missing required field `{field}`"),
            format!("Add `{field} = name`."),
        )),
    }
}

fn take_required_symbol_path(
    parameters: &mut BTreeMap<String, AmdlValue>,
    field: &str,
    offset: usize,
) -> Result<Vec<String>, AmdlParserError> {
    let value = take_required_symbol(parameters, field, offset)?;
    Ok(value.split('.').map(str::to_string).collect())
}

fn reject_unknown_fields(
    parameters: &BTreeMap<String, AmdlValue>,
    offset: usize,
    block: &str,
    allowed_fields: &[&str],
) -> Result<(), AmdlParserError> {
    if let Some(field) = parameters.keys().next() {
        return Err(AmdlParserError::new(
            offset,
            "AMDL_UNKNOWN_FIELD",
            format!("unsupported field `{field}` in `{block}`"),
            format!("Use only `{}` in `{block}`.", allowed_fields.join("`, `")),
        ));
    }
    Ok(())
}

fn diagnostic_from_parse_error(source: &str, error: AmdlParserError) -> AmdlDiagnostic {
    let (line, column, source_line) = line_column_for_offset(source, error.offset);
    AmdlDiagnostic {
        code: error.code.into(),
        severity: "error".into(),
        line,
        column,
        message: error.message,
        suggestion: error.suggestion,
        source_line,
    }
}

fn line_column_for_offset(
    source: &str,
    offset: usize,
) -> (Option<usize>, Option<usize>, Option<String>) {
    let mut line = 1usize;
    let mut line_start = 0usize;
    for (index, ch) in source.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = index + ch.len_utf8();
        }
    }
    let line_end = source[line_start..]
        .find('\n')
        .map(|relative| line_start + relative)
        .unwrap_or(source.len());
    let column = source[line_start..offset.min(source.len())].chars().count() + 1;
    (
        Some(line),
        Some(column),
        Some(source[line_start..line_end].to_string()),
    )
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
            b'[' => push_simple(&mut tokens, TokenKind::LBracket, &mut cursor),
            b']' => push_simple(&mut tokens, TokenKind::RBracket, &mut cursor),
            b'=' => push_simple(&mut tokens, TokenKind::Equal, &mut cursor),
            b',' => push_simple(&mut tokens, TokenKind::Comma, &mut cursor),
            b'.' => push_simple(&mut tokens, TokenKind::Dot, &mut cursor),
            b'(' | b')' | b':' => {
                tokens.push(Token {
                    kind: TokenKind::Unknown(byte as char),
                    offset: cursor,
                });
                cursor += 1;
            }
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
                while cursor < bytes.len() && matches!(bytes[cursor], b'0'..=b'9' | b'.') {
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
            _ => {
                tokens.push(Token {
                    kind: TokenKind::Unknown(byte as char),
                    offset: cursor,
                });
                cursor += 1;
            }
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
    fn parses_canonical_model() {
        let source = r##"
amdl

model Crate {
  mesh {
    kind = primitive.box
    size = [1, 1, 1]
  }

  material {
    kind = inline
    base_color = "#8a5a2b"
    roughness = 0.75
    metallic = 0.0
  }

  collider {
    shape = box
    size = [1, 1, 1]
  }

  rigidbody {
    mode = dynamic
    mass = 12kg
  }

  socket Top {
    position = [0, 1.0, 0]
    rotation = [0, 0, 0]
  }

  lod 1 {
    mesh {
      kind = asset
      path = "models/crate_low.glb"
    }
    distance = 25m
  }
}
"##;

        let document = parse_amdl(source).unwrap();
        AmdlValidator::validate(&document).unwrap();

        assert_eq!(document.models[0].name, "Crate");
        assert_eq!(document.models[0].sockets[0].name, "Top");
        assert_eq!(document.models[0].lods[0].index, 1);
    }

    #[test]
    fn rejects_legacy_call_syntax() {
        let source = r#"
amdl
model Crate {
  mesh = primitive.box(size: [1, 1, 1])
}
"#;

        let error = parse_amdl(source).unwrap_err();
        assert_eq!(error.code, "AMDL_LEGACY_SYNTAX");
    }

    #[test]
    fn diagnostics_include_location_and_source_line() {
        let source = r#"
amdl
model Empty {
  rigidbody {
    mode = dynamic
    mass = 12kg
  }
}
"#;

        let diagnostics = diagnose_amdl(source);
        assert_eq!(diagnostics[0].code, "AMDL_MESH_REQUIRED");
        assert!(diagnostics[0].line.is_some());
        assert!(diagnostics[0].source_line.is_some());
    }

    #[test]
    fn repository_examples_parse() {
        let examples = [
            (
                "crate.amdl",
                include_str!("../../../examples/project/models/crate.amdl"),
            ),
            (
                "tree.amdl",
                include_str!("../../../examples/project/models/tree.amdl"),
            ),
            (
                "barrel.amdl",
                include_str!("../../../examples/project/models/barrel.amdl"),
            ),
        ];

        for (name, source) in examples {
            let diagnostics = diagnose_amdl(source);
            assert!(
                diagnostics.is_empty(),
                "{name} produced AMDL diagnostics: {diagnostics:?}"
            );
        }
    }
}

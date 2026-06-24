#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Varg language parser and diagnostics.
//!
//! This crate owns the public Varg authoring surface and the MVP runtime
//! interpreter used by the engine while the full compiler is built out.

use std::collections::HashMap;
use std::path::Path;

use engine_core::math::{Transform, Vec3};

/// Varg source role inferred from extension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VargFileRole {
    /// Logic files: scripts, modules, and declarative behaviors.
    Logic,
    /// World files: scenes, prefabs, and network declarations.
    World,
    /// Asset files: models, materials, and audio declarations.
    Asset,
}

impl VargFileRole {
    /// Infers a Varg file role from a path extension.
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        match path
            .as_ref()
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("varg") => Some(Self::Logic),
            Some("vscene") => Some(Self::World),
            Some("vasset") => Some(Self::Asset),
            _ => None,
        }
    }
}

/// Diagnostic severity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum VargDiagnosticSeverity {
    /// The source cannot compile.
    Error,
    /// The source is accepted but likely unintended.
    Warning,
}

/// Structured Varg diagnostic suitable for editor and AI tools.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct VargDiagnostic {
    /// Stable diagnostic code.
    pub code: String,
    /// Diagnostic severity.
    pub severity: VargDiagnosticSeverity,
    /// One-based line, when available.
    pub line: Option<usize>,
    /// One-based column, when available.
    pub column: Option<usize>,
    /// Human-readable message.
    pub message: String,
    /// Expected syntax or semantic shape.
    pub expected: String,
    /// Concrete suggested fix.
    pub suggestion: String,
    /// Whether the diagnostic blocks compilation.
    pub blocking: bool,
    /// Source line containing the issue, when available.
    pub source_line: Option<String>,
}

/// Parsed Varg file summary.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct VargFileAst {
    /// Top-level imports.
    pub imports: Vec<VargImport>,
    /// Top-level declarations.
    pub declarations: Vec<VargDeclaration>,
}

/// Parsed import declaration.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct VargImport {
    /// Imported module path.
    pub path: String,
    /// One-based line.
    pub line: usize,
}

/// Parsed top-level declaration.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct VargDeclaration {
    /// Declaration kind.
    pub kind: String,
    /// Declaration name, when present.
    pub name: Option<String>,
    /// One-based line.
    pub line: usize,
    /// Exported properties declared inside this declaration.
    pub exports: Vec<VargExport>,
}

/// Exported script property.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct VargExport {
    /// Property name.
    pub name: String,
    /// Varg type annotation.
    pub type_name: String,
    /// Optional default literal.
    pub default_value: Option<String>,
    /// One-based line.
    pub line: usize,
}

/// Compiled Varg script summary used by the MVP runtime.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VargScript {
    /// Script declaration name.
    pub name: String,
    /// Editor-exposed properties.
    pub exports: Vec<VargExport>,
    /// Mutable script state variable defaults.
    pub state_defaults: HashMap<String, serde_json::Value>,
    /// Lifecycle hook bodies keyed by reserved hook name.
    hooks: HashMap<String, Vec<RuntimeStatement>>,
}

/// Per-invocation context passed to the Varg script runtime.
#[derive(Clone, Debug)]
pub struct VargRuntimeContext {
    /// Local transform for the entity this script is attached to.
    pub transform: Transform,
    /// Frame input state.
    pub input: engine_platform::InputState,
    /// Delta time for the lifecycle call.
    pub delta_time: f32,
    /// Total elapsed runtime time in seconds.
    pub total_time: f32,
    /// Monotonic runtime frame index.
    pub frame_index: u64,
    /// Editor-exposed overrides keyed by exported property name.
    pub exported_values: HashMap<String, serde_json::Value>,
    /// Persistent script state keyed by state variable name.
    pub state: HashMap<String, serde_json::Value>,
}

/// Result of executing one lifecycle hook.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VargRuntimeOutput {
    /// Updated local transform.
    pub transform: Transform,
    /// Updated persistent state.
    pub state: HashMap<String, serde_json::Value>,
    /// Log entries emitted by `log(...)`.
    pub logs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum RuntimeStatement {
    Log(String),
    Translate(Expression),
    SetPosition(Expression),
    SetPositionAxis {
        axis: Axis,
        value: Expression,
    },
    AddToPosition {
        axis: Axis,
        value: Expression,
    },
    DeclareLocal {
        name: String,
        value: Expression,
    },
    AssignBinding {
        name: String,
        value: Expression,
    },
    AddToBinding {
        name: String,
        value: Expression,
    },
    SubFromBinding {
        name: String,
        value: Expression,
    },
    AssignState {
        name: String,
        value: Expression,
    },
    AddToState {
        name: String,
        value: Expression,
    },
    SubFromState {
        name: String,
        value: Expression,
    },
    If {
        condition: ConditionExpression,
        statements: Vec<RuntimeStatement>,
        else_statements: Vec<RuntimeStatement>,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum ConditionExpression {
    InputDown(String),
    InputJustPressed(String),
    InputJustReleased(String),
    ActionDown(String),
    ActionJustPressed(String),
    ActionJustReleased(String),
    ActionUp(String),
    Not(Box<ConditionExpression>),
    And(Box<ConditionExpression>, Box<ConditionExpression>),
    Or(Box<ConditionExpression>, Box<ConditionExpression>),
    Compare {
        lhs: Expression,
        op: CompareOp,
        rhs: Expression,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum Expression {
    Number(f32),
    String(String),
    Bool(bool),
    Variable(String),
    Member(String, String),
    Call {
        function: String,
        args: Vec<Expression>,
    },
    Vec3(Box<Expression>, Box<Expression>, Box<Expression>),
    Binary {
        op: BinaryOp,
        lhs: Box<Expression>,
        rhs: Box<Expression>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompareOp {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Axis {
    X,
    Y,
    Z,
}

/// Parses and validates Varg source for the path role.
pub fn diagnose_source(path: impl AsRef<Path>, source: &str) -> Vec<VargDiagnostic> {
    let role = VargFileRole::from_path(path);
    let mut parser = Parser::new(source, role);
    parser.parse();
    parser.diagnostics
}

/// Parses Varg source and returns an AST summary plus diagnostics.
pub fn parse_source(
    path: impl AsRef<Path>,
    source: &str,
) -> (Option<VargFileAst>, Vec<VargDiagnostic>) {
    let (ast, diagnostics) = parse_source_lossy(path, source);
    if diagnostics.iter().any(|diagnostic| diagnostic.blocking) {
        (None, diagnostics)
    } else {
        (Some(ast), diagnostics)
    }
}

/// Parses Varg source and always returns the best-effort AST summary plus diagnostics.
pub fn parse_source_lossy(
    path: impl AsRef<Path>,
    source: &str,
) -> (VargFileAst, Vec<VargDiagnostic>) {
    let role = VargFileRole::from_path(path);
    let mut parser = Parser::new(source, role);
    let ast = parser.parse();
    let diagnostics = parser.diagnostics;
    (ast, diagnostics)
}

/// Compiles a `.varg` script into the MVP executable runtime summary.
pub fn compile_script_source(
    path: impl AsRef<Path>,
    source: &str,
) -> (Option<VargScript>, Vec<VargDiagnostic>) {
    let (ast, mut diagnostics) = parse_source(path, source);
    let Some(ast) = ast else {
        return (None, diagnostics);
    };
    let Some(declaration) = ast
        .declarations
        .iter()
        .find(|declaration| declaration.kind == "script")
    else {
        diagnostics.push(VargDiagnostic {
            code: "VARG3003".to_string(),
            severity: VargDiagnosticSeverity::Error,
            line: Some(1),
            column: Some(1),
            message: "logic file does not contain a script declaration".to_string(),
            expected: "`script Name { ... }`".to_string(),
            suggestion: "Add a script declaration or attach a file that contains one.".to_string(),
            blocking: true,
            source_line: source.lines().next().map(str::to_string),
        });
        return (None, diagnostics);
    };
    let mut script = VargScript {
        name: declaration
            .name
            .clone()
            .unwrap_or_else(|| "UnnamedScript".to_string()),
        exports: declaration.exports.clone(),
        state_defaults: HashMap::new(),
        hooks: HashMap::new(),
    };
    compile_runtime_blocks(source, &mut script);
    (Some(script), diagnostics)
}

impl VargScript {
    /// Executes a lifecycle hook if the script defines it.
    pub fn run_hook(&self, hook: &str, mut context: VargRuntimeContext) -> VargRuntimeOutput {
        for (name, value) in &self.state_defaults {
            context
                .state
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }

        let mut output = VargRuntimeOutput {
            transform: context.transform,
            state: context.state,
            logs: Vec::new(),
        };
        let Some(statements) = self.hooks.get(hook) else {
            return output;
        };
        let mut env = RuntimeEnvironment {
            script: self,
            input: &context.input,
            delta_time: context.delta_time,
            total_time: context.total_time,
            frame_index: context.frame_index,
            exported_values: &context.exported_values,
            transform: &mut output.transform,
            state: &mut output.state,
            locals: HashMap::new(),
            logs: &mut output.logs,
        };
        for statement in statements {
            env.execute(statement);
        }
        output
    }
}

struct Parser<'a> {
    source: &'a str,
    role: Option<VargFileRole>,
    diagnostics: Vec<VargDiagnostic>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, role: Option<VargFileRole>) -> Self {
        Self {
            source,
            role,
            diagnostics: Vec::new(),
        }
    }

    fn parse(&mut self) -> VargFileAst {
        let mut ast = VargFileAst::default();
        let mut stack: Vec<Block> = Vec::new();

        if self.role.is_none() {
            self.push(
                "VARG1000",
                1,
                1,
                "unsupported Varg file extension",
                "Use .varg, .vscene, or .vasset.",
                "Rename the file to the Varg role extension that matches its contents.",
            );
        }

        for (line_index, raw_line) in self.source.lines().enumerate() {
            let line_no = line_index + 1;
            let without_comment = strip_line_comment(raw_line);
            let trimmed = without_comment.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(path) = parse_import(trimmed) {
                self.validate_import(&path, line_no, raw_line);
                ast.imports.push(VargImport {
                    path,
                    line: line_no,
                });
            }

            if stack.last().is_some_and(|block| block.declarative) && starts_imperative(trimmed) {
                self.push_line(
                    "VARG4001",
                    line_no,
                    raw_line,
                    "imperative control flow is not allowed in declarative Varg files or behavior blocks",
                    "Scene, asset, and behavior declarations must stay deterministic and declarative.",
                    "Move runtime logic into a `script` declaration in a .varg file, or use a declarative construct such as `scatter`.",
                );
            }

            if let Some(header) = parse_header(trimmed).filter(|_| !trimmed.starts_with("} else")) {
                self.validate_top_level_header(&header, stack.len(), line_no, raw_line);
                self.validate_declaration_name(&header, stack.len(), line_no, raw_line);
                let declarative = header.kind == "behavior"
                    || matches!(self.role, Some(VargFileRole::World | VargFileRole::Asset));
                if stack.is_empty() && is_known_top_level_kind(&header.kind) {
                    ast.declarations.push(VargDeclaration {
                        kind: header.kind.clone(),
                        name: header.name.clone(),
                        line: line_no,
                        exports: Vec::new(),
                    });
                }
                stack.push(Block {
                    line: line_no,
                    declarative,
                });
            } else if stack.is_empty() && looks_like_top_level_declaration(trimmed) {
                self.push_line(
                    "VARG1003",
                    line_no,
                    raw_line,
                    "unknown top-level Varg declaration",
                    "Use a declaration allowed by the file role, followed by a block.",
                    "Use `script`, `module`, or `behavior` in .varg; `scene`, `prefab`, or `network` in .vscene; `model`, `material`, or `audio` in .vasset.",
                );
            }

            if let Some(export) = parse_export(trimmed, line_no) {
                if let Some(declaration) = ast.declarations.last_mut() {
                    declaration.exports.push(export);
                }
            } else if trimmed.starts_with("@export var ") {
                self.push_line(
                    "VARG3002",
                    line_no,
                    raw_line,
                    "exported property is missing a name or explicit type annotation",
                    "`@export var name: Type = value`",
                    "Add a camelCase property name and explicit Varg type annotation.",
                );
            }

            if let Some(signature) = parse_function_signature(trimmed) {
                self.validate_lifecycle_signature(&signature, line_no, raw_line);
            }

            let opens = trimmed.matches('{').count();
            let closes = trimmed.matches('}').count();
            for _ in 0..closes.saturating_sub(opens) {
                stack.pop();
            }
        }

        if let Some(block) = stack.last() {
            self.push(
                "VARG1004",
                block.line,
                1,
                "unclosed Varg block",
                "Every `{` must be paired with a closing `}`.",
                "Add a closing brace for this declaration or nested block.",
            );
        }

        ast
    }

    fn validate_import(&mut self, path: &str, line: usize, raw_line: &str) {
        if self.role != Some(VargFileRole::Logic) {
            self.push_line(
                "VARG1005",
                line,
                raw_line,
                "imports are only allowed in .varg logic files",
                "`import \"path/to/module.varg\"` may only import Varg code modules.",
                "Use typed resource constructors such as `Asset(...)`, `Scene(...)`, or `Prefab(...)` for non-code references.",
            );
        }
        if !path.ends_with(".varg") {
            self.push_line(
                "VARG1006",
                line,
                raw_line,
                "imports may only reference .varg code modules",
                "`import \"scripts/combat.varg\"`",
                "Replace this import with a .varg module import, or use a typed resource constructor for scenes and assets.",
            );
        }
    }

    fn validate_top_level_header(
        &mut self,
        header: &Header,
        depth: usize,
        line: usize,
        raw_line: &str,
    ) {
        if depth != 0 {
            return;
        }

        let Some(role) = self.role else {
            return;
        };
        let allowed = match role {
            VargFileRole::Logic => matches!(header.kind.as_str(), "script" | "module" | "behavior"),
            VargFileRole::World => matches!(header.kind.as_str(), "scene" | "prefab" | "network"),
            VargFileRole::Asset => matches!(header.kind.as_str(), "model" | "material" | "audio"),
        };

        if !allowed {
            let expected = match role {
                VargFileRole::Logic => "`script`, `module`, or `behavior`",
                VargFileRole::World => "`scene`, `prefab`, or `network`",
                VargFileRole::Asset => "`model`, `material`, or `audio`",
            };
            self.push_line(
                "VARG1002",
                line,
                raw_line,
                &format!("`{}` is not a valid top-level declaration for this file role", header.kind),
                expected,
                "Move the declaration to the matching Varg file type or change the declaration kind.",
            );
        }
    }

    fn validate_declaration_name(
        &mut self,
        header: &Header,
        depth: usize,
        line: usize,
        raw_line: &str,
    ) {
        if depth != 0 || !is_known_top_level_kind(&header.kind) || header.name.is_some() {
            return;
        }

        self.push_line(
            "VARG1007",
            line,
            raw_line,
            "top-level Varg declarations must have a PascalCase name",
            "`script PlayerController { ... }`, `scene MainScene { ... }`, or `material WoodCrate { ... }`",
            "Add a declaration name after the declaration kind.",
        );
    }

    fn validate_lifecycle_signature(
        &mut self,
        signature: &FunctionSignature,
        line: usize,
        raw_line: &str,
    ) {
        let expected = match signature.name.as_str() {
            "start" => Some(""),
            "update" | "fixedUpdate" | "lateUpdate" => Some("_ dt: Float"),
            "collisionEnter" | "collisionExit" => Some("_ other: Entity"),
            "event" => Some("_ name: String, _ data: EventData"),
            _ => None,
        };

        if let Some(expected_params) = expected {
            let actual = normalize_params(&signature.params);
            if actual != normalize_params(expected_params) {
                self.push_line(
                    "VARG3001",
                    line,
                    raw_line,
                    &format!("{} hook has invalid parameters", signature.name),
                    &format!("`func {}({expected_params})`", signature.name),
                    "Update the hook signature to the reserved Varg lifecycle shape.",
                );
            }
        }
    }

    fn push(
        &mut self,
        code: &str,
        line: usize,
        column: usize,
        message: &str,
        expected: &str,
        suggestion: &str,
    ) {
        self.diagnostics.push(VargDiagnostic {
            code: code.to_string(),
            severity: VargDiagnosticSeverity::Error,
            line: Some(line),
            column: Some(column),
            message: message.to_string(),
            expected: expected.to_string(),
            suggestion: suggestion.to_string(),
            blocking: true,
            source_line: self
                .source
                .lines()
                .nth(line.saturating_sub(1))
                .map(str::to_string),
        });
    }

    fn push_line(
        &mut self,
        code: &str,
        line: usize,
        raw_line: &str,
        message: &str,
        expected: &str,
        suggestion: &str,
    ) {
        let column = raw_line
            .chars()
            .position(|ch| !ch.is_whitespace())
            .map(|index| index + 1)
            .unwrap_or(1);
        self.push(code, line, column, message, expected, suggestion);
    }
}

struct Block {
    line: usize,
    declarative: bool,
}

struct Header {
    kind: String,
    name: Option<String>,
}

struct FunctionSignature {
    name: String,
    params: String,
}

fn strip_line_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(before, _)| before)
}

fn parse_import(line: &str) -> Option<String> {
    let rest = line.strip_prefix("import ")?.trim();
    parse_quoted(rest)
}

fn parse_header(line: &str) -> Option<Header> {
    if !line.ends_with('{') {
        return None;
    }
    let before_brace = line.trim_end_matches('{').trim();
    let mut parts = before_brace.split_whitespace();
    let kind = parts.next()?.to_string();
    let name = parts.next().map(|part| part.trim_matches('"').to_string());
    Some(Header { kind, name })
}

fn parse_export(line: &str, line_no: usize) -> Option<VargExport> {
    let rest = line.strip_prefix("@export var ")?.trim();
    let (name, after_name) = rest.split_once(':')?;
    let (type_name, default_value) = match after_name.split_once('=') {
        Some((type_name, value)) => (type_name.trim(), Some(value.trim().to_string())),
        None => (after_name.trim(), None),
    };
    Some(VargExport {
        name: name.trim().to_string(),
        type_name: type_name.to_string(),
        default_value,
        line: line_no,
    })
}

fn parse_function_signature(line: &str) -> Option<FunctionSignature> {
    let rest = line.strip_prefix("func ")?.trim();
    let open = rest.find('(')?;
    let close = rest[open + 1..].find(')')? + open + 1;
    Some(FunctionSignature {
        name: rest[..open].trim().to_string(),
        params: rest[open + 1..close].trim().to_string(),
    })
}

fn parse_quoted(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix('"')?;
    let end = value.find('"')?;
    Some(value[..end].to_string())
}

fn starts_imperative(line: &str) -> bool {
    matches!(
        line.split_whitespace().next(),
        Some("if" | "for" | "while" | "func" | "return" | "break" | "continue" | "var" | "let")
    )
}

fn is_known_top_level_kind(kind: &str) -> bool {
    matches!(
        kind,
        "script"
            | "module"
            | "behavior"
            | "scene"
            | "prefab"
            | "network"
            | "model"
            | "material"
            | "audio"
    )
}

fn looks_like_top_level_declaration(line: &str) -> bool {
    let Some(first) = line.split_whitespace().next() else {
        return false;
    };
    first.chars().next().is_some_and(char::is_alphabetic)
        && !matches!(
            first,
            "import" | "let" | "var" | "func" | "if" | "else" | "for" | "while" | "return"
        )
}

fn normalize_params(params: &str) -> String {
    params.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct RuntimeEnvironment<'a> {
    script: &'a VargScript,
    input: &'a engine_platform::InputState,
    delta_time: f32,
    total_time: f32,
    frame_index: u64,
    exported_values: &'a HashMap<String, serde_json::Value>,
    transform: &'a mut Transform,
    state: &'a mut HashMap<String, serde_json::Value>,
    locals: HashMap<String, serde_json::Value>,
    logs: &'a mut Vec<String>,
}

impl RuntimeEnvironment<'_> {
    fn execute(&mut self, statement: &RuntimeStatement) {
        match statement {
            RuntimeStatement::Log(message) => self.logs.push(message.clone()),
            RuntimeStatement::Translate(expression) => {
                let delta = self.eval_vec3(expression);
                self.transform.translation += delta;
            }
            RuntimeStatement::SetPosition(expression) => {
                self.transform.translation = self.eval_vec3(expression);
            }
            RuntimeStatement::SetPositionAxis { axis, value } => {
                let value = self.eval_number(value);
                match axis {
                    Axis::X => self.transform.translation.x = value,
                    Axis::Y => self.transform.translation.y = value,
                    Axis::Z => self.transform.translation.z = value,
                }
            }
            RuntimeStatement::AddToPosition { axis, value } => {
                let value = self.eval_number(value);
                match axis {
                    Axis::X => self.transform.translation.x += value,
                    Axis::Y => self.transform.translation.y += value,
                    Axis::Z => self.transform.translation.z += value,
                }
            }
            RuntimeStatement::AssignState { name, value } => {
                let value = self.eval_json(value);
                self.state.insert(name.clone(), value);
            }
            RuntimeStatement::AddToState { name, value } => {
                let current = self.state_number(name);
                let next = current + self.eval_number(value);
                self.state
                    .insert(name.clone(), serde_json::Value::from(next as f64));
            }
            RuntimeStatement::SubFromState { name, value } => {
                let current = self.state_number(name);
                let next = current - self.eval_number(value);
                self.state
                    .insert(name.clone(), serde_json::Value::from(next as f64));
            }
            RuntimeStatement::DeclareLocal { name, value } => {
                let value = self.eval_json(value);
                self.locals.insert(name.clone(), value);
            }
            RuntimeStatement::AssignBinding { name, value } => {
                let value = self.eval_json(value);
                if self.locals.contains_key(name) {
                    self.locals.insert(name.clone(), value);
                } else {
                    self.state.insert(name.clone(), value);
                }
            }
            RuntimeStatement::AddToBinding { name, value } => {
                let current = self.binding_number(name);
                let next = current + self.eval_number(value);
                self.assign_number_binding(name, next);
            }
            RuntimeStatement::SubFromBinding { name, value } => {
                let current = self.binding_number(name);
                let next = current - self.eval_number(value);
                self.assign_number_binding(name, next);
            }
            RuntimeStatement::If {
                condition,
                statements,
                else_statements,
            } => {
                let branch = if self.eval_condition(condition) {
                    statements
                } else {
                    else_statements
                };
                for statement in branch {
                    self.execute(statement);
                }
            }
        }
    }

    fn eval_condition(&self, condition: &ConditionExpression) -> bool {
        match condition {
            ConditionExpression::InputDown(action) | ConditionExpression::ActionDown(action) => {
                input_action_down(self.input, action)
            }
            ConditionExpression::InputJustPressed(action) => {
                input_action_pressed(self.input, action)
            }
            ConditionExpression::InputJustReleased(action) => {
                input_action_released(self.input, action)
            }
            ConditionExpression::ActionUp(action) => !input_action_down(self.input, action),
            ConditionExpression::ActionJustPressed(action) => {
                input_action_pressed(self.input, action)
            }
            ConditionExpression::ActionJustReleased(action) => {
                input_action_released(self.input, action)
            }
            ConditionExpression::Not(condition) => !self.eval_condition(condition),
            ConditionExpression::And(lhs, rhs) => {
                self.eval_condition(lhs) && self.eval_condition(rhs)
            }
            ConditionExpression::Or(lhs, rhs) => {
                self.eval_condition(lhs) || self.eval_condition(rhs)
            }
            ConditionExpression::Compare { lhs, op, rhs } => {
                let lhs = self.eval_number(lhs);
                let rhs = self.eval_number(rhs);
                match op {
                    CompareOp::Equal => (lhs - rhs).abs() <= f32::EPSILON,
                    CompareOp::NotEqual => (lhs - rhs).abs() > f32::EPSILON,
                    CompareOp::GreaterThan => lhs > rhs,
                    CompareOp::GreaterThanOrEqual => lhs >= rhs,
                    CompareOp::LessThan => lhs < rhs,
                    CompareOp::LessThanOrEqual => lhs <= rhs,
                }
            }
        }
    }

    fn eval_vec3(&self, expression: &Expression) -> Vec3 {
        match expression {
            Expression::Vec3(x, y, z) => Vec3::new(
                self.eval_number(x),
                self.eval_number(y),
                self.eval_number(z),
            ),
            _ => Vec3::new(self.eval_number(expression), 0.0, 0.0),
        }
    }

    fn eval_json(&self, expression: &Expression) -> serde_json::Value {
        match expression {
            Expression::String(value) => serde_json::Value::String(value.clone()),
            Expression::Bool(value) => serde_json::Value::Bool(*value),
            _ => serde_json::Value::from(self.eval_number(expression) as f64),
        }
    }

    fn eval_number(&self, expression: &Expression) -> f32 {
        match expression {
            Expression::Number(value) => *value,
            Expression::String(_) => 0.0,
            Expression::Bool(value) => {
                if *value {
                    1.0
                } else {
                    0.0
                }
            }
            Expression::Variable(name) => self.variable_number(name),
            Expression::Member(owner, field) => self.member_number(owner, field),
            Expression::Call { function, args } => self.call_number(function, args),
            Expression::Vec3(_, _, _) => 0.0,
            Expression::Binary { op, lhs, rhs } => {
                let lhs = self.eval_number(lhs);
                let rhs = self.eval_number(rhs);
                match op {
                    BinaryOp::Add => lhs + rhs,
                    BinaryOp::Sub => lhs - rhs,
                    BinaryOp::Mul => lhs * rhs,
                    BinaryOp::Div => {
                        if rhs.abs() <= f32::EPSILON {
                            0.0
                        } else {
                            lhs / rhs
                        }
                    }
                }
            }
        }
    }

    fn variable_number(&self, name: &str) -> f32 {
        if name == "dt" {
            return self.delta_time;
        }
        if name == "time" {
            return self.total_time;
        }
        if let Some(value) = self
            .exported_values
            .get(name)
            .or_else(|| self.locals.get(name))
            .or_else(|| self.state.get(name))
            .and_then(json_number)
        {
            return value;
        }
        self.script
            .exports
            .iter()
            .find(|export| export.name == name)
            .and_then(|export| export.default_value.as_ref())
            .and_then(|value| parse_default_literal(value))
            .and_then(|value| json_number(&value))
            .unwrap_or(0.0)
    }

    fn member_number(&self, owner: &str, field: &str) -> f32 {
        match (owner, field) {
            ("entity.position", "x") | ("position", "x") => self.transform.translation.x,
            ("entity.position", "y") | ("position", "y") => self.transform.translation.y,
            ("entity.position", "z") | ("position", "z") => self.transform.translation.z,
            ("Input", "moveX") => self.input.action_value("MoveX"),
            ("Input", "moveY") => self.input.action_value("MoveY"),
            ("InputAction", action) => self.input.action_value(action),
            ("Time", "time") | ("Time", "elapsed") => self.total_time,
            ("Time", "delta") | ("Time", "dt") => self.delta_time,
            ("Time", "frame") => self.frame_index as f32,
            ("state", name) => self.state.get(name).and_then(json_number).unwrap_or(0.0),
            _ => self
                .state
                .get(owner)
                .and_then(|value| value.get(field))
                .and_then(json_number)
                .unwrap_or(0.0),
        }
    }

    fn state_number(&self, name: &str) -> f32 {
        self.state.get(name).and_then(json_number).unwrap_or(0.0)
    }

    fn binding_number(&self, name: &str) -> f32 {
        self.locals
            .get(name)
            .or_else(|| self.state.get(name))
            .and_then(json_number)
            .unwrap_or(0.0)
    }

    fn assign_number_binding(&mut self, name: &str, value: f32) {
        let value = serde_json::Value::from(value as f64);
        if self.locals.contains_key(name) {
            self.locals.insert(name.to_string(), value);
        } else {
            self.state.insert(name.to_string(), value);
        }
    }

    fn call_number(&self, function: &str, args: &[Expression]) -> f32 {
        match function {
            "sin" | "Math.sin" => self.unary_math(args, f32::sin),
            "cos" | "Math.cos" => self.unary_math(args, f32::cos),
            "tan" | "Math.tan" => self.unary_math(args, f32::tan),
            "abs" | "Math.abs" => self.unary_math(args, f32::abs),
            "sqrt" | "Math.sqrt" => self.unary_math(args, |value| value.max(0.0).sqrt()),
            "floor" | "Math.floor" => self.unary_math(args, f32::floor),
            "ceil" | "Math.ceil" => self.unary_math(args, f32::ceil),
            "round" | "Math.round" => self.unary_math(args, f32::round),
            "min" | "Math.min" => args
                .iter()
                .map(|arg| self.eval_number(arg))
                .reduce(f32::min)
                .unwrap_or(0.0),
            "max" | "Math.max" => args
                .iter()
                .map(|arg| self.eval_number(arg))
                .reduce(f32::max)
                .unwrap_or(0.0),
            "clamp" | "Math.clamp" => {
                if args.len() != 3 {
                    return 0.0;
                }
                self.eval_number(&args[0])
                    .clamp(self.eval_number(&args[1]), self.eval_number(&args[2]))
            }
            "lerp" | "Math.lerp" => {
                if args.len() != 3 {
                    return 0.0;
                }
                let from = self.eval_number(&args[0]);
                let to = self.eval_number(&args[1]);
                let t = self.eval_number(&args[2]);
                from + (to - from) * t
            }
            _ => 0.0,
        }
    }

    fn unary_math(&self, args: &[Expression], op: impl FnOnce(f32) -> f32) -> f32 {
        args.first()
            .map(|arg| op(self.eval_number(arg)))
            .unwrap_or(0.0)
    }
}

fn compile_runtime_blocks(source: &str, script: &mut VargScript) {
    let lines = source.lines().collect::<Vec<_>>();
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = strip_line_comment(lines[index]).trim();
        if trimmed.starts_with("var ") {
            if let Some((name, value)) = parse_state_default(trimmed) {
                script.state_defaults.insert(name, value);
            }
        }
        if let Some(signature) = parse_function_signature(trimmed) {
            let (body, next) = collect_block(&lines, index);
            let mut body_index = 0usize;
            let statements = parse_runtime_statements(&body, &mut body_index);
            script.hooks.insert(signature.name, statements);
            index = next;
            continue;
        }
        index += 1;
    }
}

fn parse_runtime_statements(lines: &[String], index: &mut usize) -> Vec<RuntimeStatement> {
    let mut statements = Vec::new();
    while *index < lines.len() {
        let trimmed = strip_line_comment(&lines[*index]).trim();
        *index += 1;
        if trimmed.is_empty() || trimmed == "}" {
            continue;
        }
        if let Some(condition) = parse_if_condition(trimmed) {
            let nested = collect_inline_or_block(lines, index);
            let mut nested_index = 0usize;
            let else_nested = collect_else_block(lines, index);
            let mut else_index = 0usize;
            statements.push(RuntimeStatement::If {
                condition,
                statements: parse_runtime_statements(&nested, &mut nested_index),
                else_statements: parse_runtime_statements(&else_nested, &mut else_index),
            });
            continue;
        }
        if let Some(statement) = parse_runtime_statement(trimmed) {
            statements.push(statement);
        }
    }
    statements
}

fn parse_runtime_statement(line: &str) -> Option<RuntimeStatement> {
    if let Some(content) = function_args(line, "log") {
        return parse_string_literal(content).map(RuntimeStatement::Log);
    }
    if let Some(content) = method_args(line, "entity.translate") {
        return parse_expression(content).map(RuntimeStatement::Translate);
    }
    if let Some((name, value)) = parse_local_declaration(line) {
        return Some(RuntimeStatement::DeclareLocal {
            name: name.to_string(),
            value: parse_expression(value)?,
        });
    }
    if let Some(value) = parse_position_assignment(line) {
        return Some(RuntimeStatement::SetPosition(parse_expression(value)?));
    }
    if let Some((axis, value)) = parse_position_axis_assignment(line) {
        return Some(RuntimeStatement::SetPositionAxis {
            axis,
            value: parse_expression(value)?,
        });
    }
    if let Some((axis, value)) = parse_position_add(line) {
        return Some(RuntimeStatement::AddToPosition {
            axis,
            value: parse_expression(value)?,
        });
    }
    if let Some((name, value)) = parse_state_add(line) {
        return Some(RuntimeStatement::AddToState {
            name: name.to_string(),
            value: parse_expression(value)?,
        });
    }
    if let Some((name, value)) = parse_binding_add(line) {
        return Some(RuntimeStatement::AddToBinding {
            name: name.to_string(),
            value: parse_expression(value)?,
        });
    }
    if let Some((name, value)) = parse_state_sub(line) {
        return Some(RuntimeStatement::SubFromState {
            name: name.to_string(),
            value: parse_expression(value)?,
        });
    }
    if let Some((name, value)) = parse_binding_sub(line) {
        return Some(RuntimeStatement::SubFromBinding {
            name: name.to_string(),
            value: parse_expression(value)?,
        });
    }
    if let Some((name, value)) = parse_state_assignment(line) {
        return Some(RuntimeStatement::AssignState {
            name: name.to_string(),
            value: parse_expression(value)?,
        });
    }
    if let Some((name, value)) = parse_binding_assignment(line) {
        return Some(RuntimeStatement::AssignBinding {
            name: name.to_string(),
            value: parse_expression(value)?,
        });
    }
    None
}

fn parse_if_condition(line: &str) -> Option<ConditionExpression> {
    let rest = line.strip_prefix("if ")?.trim();
    let condition = rest.strip_suffix('{').unwrap_or(rest).trim();
    parse_condition_expression(condition)
}

fn parse_condition_expression(condition: &str) -> Option<ConditionExpression> {
    let condition = strip_wrapping_parens(condition.trim());
    if let Some((lhs, rhs)) = split_logical(condition, "||") {
        return Some(ConditionExpression::Or(
            Box::new(parse_condition_expression(lhs)?),
            Box::new(parse_condition_expression(rhs)?),
        ));
    }
    if let Some((lhs, rhs)) = split_logical(condition, "&&") {
        return Some(ConditionExpression::And(
            Box::new(parse_condition_expression(lhs)?),
            Box::new(parse_condition_expression(rhs)?),
        ));
    }
    if let Some(inner) = condition.strip_prefix('!') {
        return Some(ConditionExpression::Not(Box::new(
            parse_condition_expression(inner.trim())?,
        )));
    }
    if let Some(action) = function_args(condition, "Input.pressed") {
        return parse_string_literal(action).map(ConditionExpression::InputDown);
    }
    if let Some(action) = function_args(condition, "Input.down") {
        return parse_string_literal(action).map(ConditionExpression::InputDown);
    }
    if let Some(action) = function_args(condition, "Input.justPressed") {
        return parse_string_literal(action).map(ConditionExpression::InputJustPressed);
    }
    if let Some(action) = function_args(condition, "Input.justReleased") {
        return parse_string_literal(action).map(ConditionExpression::InputJustReleased);
    }
    if let Some(action) = function_args(condition, "Input.actionDown") {
        return parse_string_literal(action).map(ConditionExpression::ActionDown);
    }
    if let Some(action) = function_args(condition, "Input.actionPressed") {
        return parse_string_literal(action).map(ConditionExpression::ActionJustPressed);
    }
    if let Some(action) = function_args(condition, "Input.actionReleased") {
        return parse_string_literal(action).map(ConditionExpression::ActionJustReleased);
    }
    if let Some(action) = function_args(condition, "Input.actionUp") {
        return parse_string_literal(action).map(ConditionExpression::ActionUp);
    }
    if let Some((lhs, op, rhs)) = split_comparison(condition) {
        return Some(ConditionExpression::Compare {
            lhs: parse_expression(lhs)?,
            op,
            rhs: parse_expression(rhs)?,
        });
    }
    None
}

fn parse_expression(source: &str) -> Option<Expression> {
    let source = source.trim().trim_end_matches(';').trim();
    parse_binary_expression(source, &[('+', BinaryOp::Add), ('-', BinaryOp::Sub)])
        .or_else(|| parse_binary_expression(source, &[('*', BinaryOp::Mul), ('/', BinaryOp::Div)]))
        .or_else(|| parse_atom(source))
}

fn parse_binary_expression(source: &str, ops: &[(char, BinaryOp)]) -> Option<Expression> {
    let mut depth = 0usize;
    let mut in_string = false;
    let chars = source.char_indices().collect::<Vec<_>>();
    for (index, ch) in chars.into_iter().rev() {
        match ch {
            '"' => in_string = !in_string,
            ')' if !in_string => depth += 1,
            '(' if !in_string => depth = depth.saturating_sub(1),
            _ => {}
        }
        if in_string || depth != 0 {
            continue;
        }
        if let Some((_, op)) = ops.iter().find(|(candidate, _)| *candidate == ch) {
            if index == 0 {
                continue;
            }
            let lhs = parse_expression(&source[..index])?;
            let rhs = parse_expression(&source[index + ch.len_utf8()..])?;
            return Some(Expression::Binary {
                op: *op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            });
        }
    }
    None
}

fn parse_atom(source: &str) -> Option<Expression> {
    if let Ok(number) = source.parse::<f32>() {
        return Some(Expression::Number(number));
    }
    if source == "true" {
        return Some(Expression::Bool(true));
    }
    if source == "false" {
        return Some(Expression::Bool(false));
    }
    if let Some(value) = parse_string_literal(source) {
        return Some(Expression::String(value));
    }
    if let Some(content) = function_args(source, "Vec3") {
        let parts = split_top_level_commas(content);
        if parts.len() == 3 {
            return Some(Expression::Vec3(
                Box::new(parse_expression(parts[0])?),
                Box::new(parse_expression(parts[1])?),
                Box::new(parse_expression(parts[2])?),
            ));
        }
    }
    if let Some(action) = function_args(source, "Input.axis") {
        let action = parse_string_literal(action)?;
        return Some(match action.as_str() {
            "move" => Expression::Member("Input".to_string(), "moveY".to_string()),
            "moveX" => Expression::Member("Input".to_string(), "moveX".to_string()),
            "moveY" => Expression::Member("Input".to_string(), "moveY".to_string()),
            _ => Expression::Variable(action),
        });
    }
    if let Some(action) = function_args(source, "Input.actionValue") {
        return parse_string_literal(action)
            .map(|action| Expression::Member("InputAction".to_string(), action));
    }
    if let Some((function, args)) = parse_expression_call(source) {
        return Some(Expression::Call {
            function: function.to_string(),
            args: split_top_level_commas(args)
                .into_iter()
                .map(parse_expression)
                .collect::<Option<Vec<_>>>()?,
        });
    }
    if let Some((owner, field)) = source.rsplit_once('.') {
        return Some(Expression::Member(
            owner.trim().to_string(),
            field.trim().to_string(),
        ));
    }
    Some(Expression::Variable(source.to_string()))
}

fn parse_state_default(line: &str) -> Option<(String, serde_json::Value)> {
    let rest = line.strip_prefix("var ")?.trim();
    let (name, after_name) = rest.split_once(':')?;
    let (_, value) = after_name.split_once('=')?;
    Some((
        name.trim().to_string(),
        parse_default_literal(value.trim())?,
    ))
}

fn parse_default_literal(value: &str) -> Option<serde_json::Value> {
    let value = value.trim().trim_end_matches(';').trim();
    if let Ok(number) = value.parse::<f64>() {
        return Some(serde_json::Value::from(number));
    }
    if value == "true" {
        return Some(serde_json::Value::Bool(true));
    }
    if value == "false" {
        return Some(serde_json::Value::Bool(false));
    }
    parse_string_literal(value).map(serde_json::Value::String)
}

fn parse_local_declaration(line: &str) -> Option<(&str, &str)> {
    let rest = line
        .strip_prefix("let ")
        .or_else(|| line.strip_prefix("var "))?
        .trim();
    let (name_part, value) = rest.split_once('=')?;
    let name = name_part
        .split_once(':')
        .map_or(name_part, |(name, _)| name)
        .trim();
    if is_valid_runtime_binding_name(name) {
        Some((name, value.trim()))
    } else {
        None
    }
}

fn parse_position_assignment(line: &str) -> Option<&str> {
    let (lhs, rhs) = line.split_once('=')?;
    if lhs.contains('+') || lhs.contains('-') || lhs.contains('*') || lhs.contains('/') {
        return None;
    }
    match lhs.trim() {
        "entity.position" | "position" => Some(rhs.trim()),
        _ => None,
    }
}

fn parse_position_axis_assignment(line: &str) -> Option<(Axis, &str)> {
    let (lhs, rhs) = line.split_once('=')?;
    if lhs.contains('+') || lhs.contains('-') || lhs.contains('*') || lhs.contains('/') {
        return None;
    }
    let axis = parse_position_axis(lhs.trim())?;
    Some((axis, rhs.trim()))
}

fn parse_position_add(line: &str) -> Option<(Axis, &str)> {
    let (lhs, rhs) = line.split_once("+=")?;
    let axis = parse_position_axis(lhs.trim())?;
    Some((axis, rhs.trim()))
}

fn parse_position_axis(lhs: &str) -> Option<Axis> {
    match lhs {
        "entity.position.x" | "position.x" => Some(Axis::X),
        "entity.position.y" | "position.y" => Some(Axis::Y),
        "entity.position.z" | "position.z" => Some(Axis::Z),
        _ => None,
    }
}

fn parse_state_add(line: &str) -> Option<(&str, &str)> {
    let (lhs, rhs) = line.split_once("+=")?;
    lhs.trim()
        .strip_prefix("state.")
        .map(|name| (name, rhs.trim()))
        .or_else(|| Some((lhs.trim(), rhs.trim())))
}

fn parse_state_sub(line: &str) -> Option<(&str, &str)> {
    let (lhs, rhs) = line.split_once("-=")?;
    lhs.trim()
        .strip_prefix("state.")
        .map(|name| (name, rhs.trim()))
        .or_else(|| Some((lhs.trim(), rhs.trim())))
}

fn parse_state_assignment(line: &str) -> Option<(&str, &str)> {
    let (lhs, rhs) = line.split_once('=')?;
    if lhs.contains('+') || lhs.contains('-') || lhs.contains('*') || lhs.contains('/') {
        return None;
    }
    lhs.trim()
        .strip_prefix("state.")
        .map(|name| (name, rhs.trim()))
}

fn parse_binding_add(line: &str) -> Option<(&str, &str)> {
    let (lhs, rhs) = line.split_once("+=")?;
    let name = lhs.trim();
    is_valid_runtime_binding_name(name).then_some((name, rhs.trim()))
}

fn parse_binding_sub(line: &str) -> Option<(&str, &str)> {
    let (lhs, rhs) = line.split_once("-=")?;
    let name = lhs.trim();
    is_valid_runtime_binding_name(name).then_some((name, rhs.trim()))
}

fn parse_binding_assignment(line: &str) -> Option<(&str, &str)> {
    let (lhs, rhs) = line.split_once('=')?;
    if lhs.contains('+') || lhs.contains('-') || lhs.contains('*') || lhs.contains('/') {
        return None;
    }
    let name = lhs.trim();
    is_valid_runtime_binding_name(name).then_some((name, rhs.trim()))
}

fn is_valid_runtime_binding_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && !matches!(
            name,
            "if" | "else" | "for" | "while" | "return" | "true" | "false"
        )
}

fn collect_inline_or_block(lines: &[String], index: &mut usize) -> Vec<String> {
    let mut collected = Vec::new();
    let mut depth = 1isize;
    while *index < lines.len() {
        let line = lines[*index].clone();
        *index += 1;
        let trimmed = strip_line_comment(&line).trim();
        if depth == 1 && trimmed.starts_with("} else") {
            *index = (*index).saturating_sub(1);
            break;
        }
        depth += trimmed.matches('{').count() as isize;
        depth -= trimmed.matches('}').count() as isize;
        if depth <= 0 {
            break;
        }
        collected.push(line);
    }
    collected
}

fn collect_else_block(lines: &[String], index: &mut usize) -> Vec<String> {
    if *index >= lines.len() {
        return Vec::new();
    }
    let trimmed = strip_line_comment(&lines[*index]).trim();
    if trimmed == "else {" || trimmed == "} else {" {
        *index += 1;
        return collect_inline_or_block(lines, index);
    }
    Vec::new()
}

fn collect_block(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    let mut body = Vec::new();
    let mut depth =
        lines[start].matches('{').count() as isize - lines[start].matches('}').count() as isize;
    let mut index = start + 1;
    while index < lines.len() {
        let line = lines[index];
        depth += line.matches('{').count() as isize;
        depth -= line.matches('}').count() as isize;
        if depth <= 0 {
            return (body, index + 1);
        }
        body.push(line.to_string());
        index += 1;
    }
    (body, index)
}

fn function_args<'a>(line: &'a str, function: &str) -> Option<&'a str> {
    let rest = line.trim().strip_prefix(function)?.trim();
    Some(rest.strip_prefix('(')?.strip_suffix(')')?.trim())
}

fn method_args<'a>(line: &'a str, method: &str) -> Option<&'a str> {
    function_args(line.trim_end_matches(';'), method)
}

fn parse_string_literal(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix('"')?;
    let end = value.rfind('"')?;
    Some(value[..end].to_string())
}

fn parse_expression_call(source: &str) -> Option<(&str, &str)> {
    let open = source.find('(')?;
    let function = source[..open].trim();
    if function.is_empty()
        || !function
            .chars()
            .all(|ch| ch == '_' || ch == '.' || ch.is_ascii_alphanumeric())
    {
        return None;
    }
    let args = source[open + 1..].strip_suffix(')')?;
    spans_whole_call(source).then_some((function, args))
}

fn spans_whole_call(source: &str) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    for (index, ch) in source.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => {
                depth = depth.saturating_sub(1);
                if depth == 0 && index + ch.len_utf8() < source.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn split_top_level_commas(source: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut in_string = false;
    for (index, ch) in source.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => depth = depth.saturating_sub(1),
            ',' if !in_string && depth == 0 => {
                parts.push(source[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(source[start..].trim());
    parts
}

fn strip_wrapping_parens(source: &str) -> &str {
    let mut current = source.trim();
    loop {
        let Some(inner) = current
            .strip_prefix('(')
            .and_then(|value| value.strip_suffix(')'))
        else {
            return current;
        };
        if spans_whole_expression(current) {
            current = inner.trim();
        } else {
            return current;
        }
    }
}

fn spans_whole_expression(source: &str) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    for (index, ch) in source.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => {
                depth = depth.saturating_sub(1);
                if depth == 0 && index + ch.len_utf8() < source.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn split_logical<'a>(source: &'a str, operator: &str) -> Option<(&'a str, &'a str)> {
    let mut depth = 0usize;
    let mut in_string = false;
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let ch = source[index..].chars().next()?;
        match ch {
            '"' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => depth = depth.saturating_sub(1),
            _ => {}
        }
        if !in_string && depth == 0 && source[index..].starts_with(operator) {
            let lhs = source[..index].trim();
            let rhs = source[index + operator.len()..].trim();
            if !lhs.is_empty() && !rhs.is_empty() {
                return Some((lhs, rhs));
            }
        }
        index += ch.len_utf8();
    }
    None
}

fn split_comparison(source: &str) -> Option<(&str, CompareOp, &str)> {
    let mut depth = 0usize;
    let mut in_string = false;
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let ch = source[index..].chars().next()?;
        match ch {
            '"' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => depth = depth.saturating_sub(1),
            _ => {}
        }
        if !in_string && depth == 0 {
            for (symbol, op) in [
                ("==", CompareOp::Equal),
                ("!=", CompareOp::NotEqual),
                (">=", CompareOp::GreaterThanOrEqual),
                ("<=", CompareOp::LessThanOrEqual),
                (">", CompareOp::GreaterThan),
                ("<", CompareOp::LessThan),
            ] {
                if source[index..].starts_with(symbol) {
                    let lhs = source[..index].trim();
                    let rhs = source[index + symbol.len()..].trim();
                    if !lhs.is_empty() && !rhs.is_empty() {
                        return Some((lhs, op, rhs));
                    }
                }
            }
        }
        index += ch.len_utf8();
    }
    None
}

fn json_number(value: &serde_json::Value) -> Option<f32> {
    value.as_f64().map(|number| number as f32)
}

fn input_action_down(input: &engine_platform::InputState, action: &str) -> bool {
    if let Some(keys) = default_action_keys(action) {
        return keys.iter().any(|key| input.key_down(*key));
    }
    input.action_down(action)
}

fn input_action_pressed(input: &engine_platform::InputState, action: &str) -> bool {
    if let Some(keys) = default_action_keys(action) {
        return keys.iter().any(|key| input.key_pressed(*key));
    }
    false
}

fn input_action_released(input: &engine_platform::InputState, action: &str) -> bool {
    if let Some(keys) = default_action_keys(action) {
        return keys.iter().any(|key| input.key_released(*key));
    }
    false
}

fn default_action_keys(action: &str) -> Option<&'static [engine_platform::KeyCode]> {
    use engine_platform::KeyCode;

    match action {
        "jump" | "Jump" | "Space" => Some(&[KeyCode::Space]),
        "fire" | "Fire" => Some(&[KeyCode::Character('f'), KeyCode::Character('e')]),
        "interact" | "Interact" => Some(&[KeyCode::Character('e')]),
        "pause" | "Pause" | "Escape" => Some(&[KeyCode::Escape]),
        "moveForward" | "MoveForward" => Some(&[KeyCode::Character('w'), KeyCode::ArrowUp]),
        "moveBackward" | "MoveBackward" | "MoveBack" => {
            Some(&[KeyCode::Character('s'), KeyCode::ArrowDown])
        }
        "moveLeft" | "MoveLeft" => Some(&[KeyCode::Character('a'), KeyCode::ArrowLeft]),
        "moveRight" | "MoveRight" => Some(&[KeyCode::Character('d'), KeyCode::ArrowRight]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_script_lifecycle() {
        let diagnostics = diagnose_source(
            "scripts/player.varg",
            r#"script PlayerController {
    @export var speed: Float = 6.0

    func update(_ dt: Float) {
        log("tick")
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn rejects_invalid_update_signature() {
        let diagnostics = diagnose_source(
            "scripts/player.varg",
            r#"script PlayerController {
    func update() {
    }
}
"#,
        );

        assert_eq!(diagnostics[0].code, "VARG3001");
    }

    #[test]
    fn rejects_scene_loops() {
        let diagnostics = diagnose_source(
            "scenes/main.vscene",
            r#"scene MainScene {
    for i in 0..<100 {
        spawnTree(i)
    }
}
"#,
        );

        assert_eq!(diagnostics[0].code, "VARG4001");
    }

    #[test]
    fn extracts_exported_properties() {
        let (ast, diagnostics) = parse_source(
            "scripts/player.varg",
            r#"script PlayerController {
    @export var jumpForce: Float = 8.0
}
"#,
        );

        assert!(diagnostics.is_empty());
        let ast = ast.unwrap();
        assert_eq!(ast.declarations[0].exports[0].name, "jumpForce");
    }

    #[test]
    fn rejects_scene_imports() {
        let diagnostics = diagnose_source("scenes/main.vscene", "import \"scripts/combat.varg\"\n");

        assert_eq!(diagnostics[0].code, "VARG1005");
    }

    #[test]
    fn rejects_non_varg_import_targets() {
        let diagnostics = diagnose_source("scripts/player.varg", "import \"scenes/main.vscene\"\n");

        assert_eq!(diagnostics[0].code, "VARG1006");
    }

    #[test]
    fn rejects_unclosed_blocks() {
        let diagnostics = diagnose_source("scripts/player.varg", "script Player {\n");

        assert_eq!(diagnostics[0].code, "VARG1004");
    }

    #[test]
    fn rejects_missing_declaration_name() {
        let diagnostics = diagnose_source("scripts/player.varg", "script {\n}\n");

        assert_eq!(diagnostics[0].code, "VARG1007");
    }

    #[test]
    fn rejects_malformed_export() {
        let diagnostics = diagnose_source(
            "scripts/player.varg",
            r#"script Player {
    @export var speed = 6.0
}
"#,
        );

        assert_eq!(diagnostics[0].code, "VARG3002");
    }

    #[test]
    fn runtime_supports_else_and_comparisons() {
        let (script, diagnostics) = compile_script_source(
            "scripts/health.varg",
            r#"script Health {
    var hp: Int = 2

    func update(_ dt: Float) {
        if state.hp <= 0 {
            state.dead = 1
        } else {
            state.dead = 0
            state.hp -= 2
        }
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let script = script.unwrap();
        let context = VargRuntimeContext {
            transform: Transform::default(),
            input: engine_platform::InputState::default(),
            delta_time: 0.016,
            total_time: 0.016,
            frame_index: 1,
            exported_values: HashMap::new(),
            state: HashMap::new(),
        };
        let output = script.run_hook("update", context);
        assert_eq!(
            output.state.get("dead").and_then(|value| value.as_f64()),
            Some(0.0)
        );
        assert_eq!(
            output.state.get("hp").and_then(|value| value.as_f64()),
            Some(0.0)
        );

        let context = VargRuntimeContext {
            transform: Transform::default(),
            input: engine_platform::InputState::default(),
            delta_time: 0.016,
            total_time: 0.032,
            frame_index: 2,
            exported_values: HashMap::new(),
            state: output.state,
        };
        let output = script.run_hook("update", context);
        assert_eq!(
            output.state.get("dead").and_then(|value| value.as_f64()),
            Some(1.0)
        );
    }

    #[test]
    fn runtime_supports_locals_boolean_conditions_and_position_assignment() {
        let (script, diagnostics) = compile_script_source(
            "scripts/movement.varg",
            r#"script Movement {
    @export var speed: Float = 3.0
    var ticks: Int = 0

    func update(_ dt: Float) {
        let moveX: Float = Input.actionValue("MoveX")
        let distance: Float = moveX * speed

        if Input.down("moveRight") && !Input.down("jump") {
            position.x = distance
        }

        if state.ticks == 0 || position.x >= 3.0 {
            state.ready = 1
        }

        ticks += 1
        position = Vec3(position.x, 2.0, 0.0)
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let script = script.unwrap();
        let mut input = engine_platform::InputState::default();
        input.bind_default_player_actions();
        input.apply_event(engine_platform::InputEvent::KeyDown(
            engine_platform::KeyCode::Character('d'),
        ));
        let output = script.run_hook(
            "update",
            VargRuntimeContext {
                transform: Transform::default(),
                input,
                delta_time: 0.016,
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
            },
        );

        assert_eq!(output.transform.translation.x, 3.0);
        assert_eq!(output.transform.translation.y, 2.0);
        assert_eq!(
            output.state.get("ready").and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert_eq!(
            output.state.get("ticks").and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert!(!output.state.contains_key("moveX"));
        assert!(!output.state.contains_key("distance"));
    }

    #[test]
    fn runtime_supports_action_pressed_aliases() {
        let (script, diagnostics) = compile_script_source(
            "scripts/input.varg",
            r#"script InputProbe {
    func update(_ dt: Float) {
        if Input.actionPressed("Fire") {
            state.fired = 1
        }

        if Input.actionReleased("Fire") {
            state.released = 1
        }
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let script = script.unwrap();
        let mut input = engine_platform::InputState::default();
        input.apply_event(engine_platform::InputEvent::KeyDown(
            engine_platform::KeyCode::Character('f'),
        ));
        let output = script.run_hook(
            "update",
            VargRuntimeContext {
                transform: Transform::default(),
                input,
                delta_time: 0.016,
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
            },
        );
        assert_eq!(
            output.state.get("fired").and_then(|value| value.as_f64()),
            Some(1.0)
        );

        let mut input = engine_platform::InputState::default();
        input.apply_event(engine_platform::InputEvent::KeyDown(
            engine_platform::KeyCode::Character('f'),
        ));
        input.end_frame();
        input.apply_event(engine_platform::InputEvent::KeyUp(
            engine_platform::KeyCode::Character('f'),
        ));
        let output = script.run_hook(
            "update",
            VargRuntimeContext {
                transform: Transform::default(),
                input,
                delta_time: 0.016,
                total_time: 0.032,
                frame_index: 2,
                exported_values: HashMap::new(),
                state: output.state,
            },
        );
        assert_eq!(
            output
                .state
                .get("released")
                .and_then(|value| value.as_f64()),
            Some(1.0)
        );
    }

    #[test]
    fn runtime_supports_time_and_math_for_wave_motion() {
        let (script, diagnostics) = compile_script_source(
            "scripts/buoy.varg",
            r#"script Buoy {
    @export var amplitude: Float = 2.0
    @export var frequency: Float = 3.1415927

    func update(_ dt: Float) {
        let wave: Float = sin(Time.time * frequency) * amplitude
        let lift: Float = clamp(wave, -1.0, 1.0)
        position.y = lerp(position.y, lift, 1.0)
        state.frame = Time.frame
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let script = script.unwrap();
        let output = script.run_hook(
            "update",
            VargRuntimeContext {
                transform: Transform::default(),
                input: engine_platform::InputState::default(),
                delta_time: 0.016,
                total_time: 0.5,
                frame_index: 7,
                exported_values: HashMap::new(),
                state: HashMap::new(),
            },
        );

        assert!((output.transform.translation.y - 1.0).abs() < 0.0001);
        assert_eq!(
            output.state.get("frame").and_then(|value| value.as_f64()),
            Some(7.0)
        );
    }
}

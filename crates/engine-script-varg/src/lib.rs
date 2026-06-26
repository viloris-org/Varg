#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Varg language parser and diagnostics.
//!
//! This crate owns the public Varg authoring surface and the MVP runtime
//! interpreter used by the engine while the full compiler is built out.

use std::collections::HashMap;
use std::path::Path;

use engine_core::math::{Quat, Transform, Vec3};
use engine_ecs::{
    CameraComponentData, CameraRole, ColliderComponentData, ComponentData, GameObject,
    LightComponentData, MaterialRef, MeshRendererComponentData, RigidbodyComponentData,
    SCENE_FILE_VERSION, Scene, SceneFile, ScriptComponent, SerializedGameObject,
};

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

/// Compiled declarative behavior tree summary from a `.varg` behavior declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct VargBehavior {
    /// Behavior declaration name.
    pub name: String,
    /// Root behavior node.
    pub root: VargBehaviorNode,
}

/// Declarative behavior tree node.
#[derive(Clone, Debug, PartialEq)]
pub enum VargBehaviorNode {
    /// Execute children in order until one fails.
    Sequence {
        /// Optional author-facing node name.
        name: Option<String>,
        /// Child nodes.
        children: Vec<VargBehaviorNode>,
    },
    /// Execute children in order until one succeeds.
    Selector {
        /// Optional author-facing node name.
        name: Option<String>,
        /// Child nodes.
        children: Vec<VargBehaviorNode>,
    },
    /// Execute child nodes in parallel.
    Parallel {
        /// Optional author-facing node name.
        name: Option<String>,
        /// Child nodes.
        children: Vec<VargBehaviorNode>,
    },
    /// Pure condition expression.
    Condition {
        /// Varg condition expression source after `when`.
        expression: String,
    },
    /// Declarative action call.
    Action {
        /// Varg action expression source after `action`.
        expression: String,
    },
    /// Invert child result.
    Invert {
        /// Child node.
        child: Box<VargBehaviorNode>,
    },
    /// Force child result to success.
    Succeed {
        /// Child node.
        child: Box<VargBehaviorNode>,
    },
    /// Repeat a child node.
    Repeat {
        /// Optional repeat count. `None` means unbounded.
        count: Option<u32>,
        /// Child node.
        child: Box<VargBehaviorNode>,
    },
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
    /// Read-only scene facts exposed to migrated declarative gameplay APIs.
    pub scene: VargSceneContext,
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
    /// UI draw commands emitted by `ui.*(...)` calls during this hook.
    pub ui_commands: Vec<VargUiCommand>,
    /// Whether the script requested deferred destruction of its owning entity.
    pub destroy_self: bool,
}

/// A retained UI draw request emitted by Varg scripts.
#[derive(Clone, Debug, PartialEq)]
pub enum VargUiCommand {
    /// Draws text at a screen-space position.
    Label {
        /// Stable script-provided widget id.
        id: String,
        /// Text to draw.
        text: String,
        /// Screen-space x position in pixels.
        x: f32,
        /// Screen-space y position in pixels.
        y: f32,
    },
    /// Draws a flat colored rectangle in screen space.
    Rect {
        /// Stable script-provided widget id.
        id: String,
        /// Screen-space x position in pixels.
        x: f32,
        /// Screen-space y position in pixels.
        y: f32,
        /// Width in pixels.
        width: f32,
        /// Height in pixels.
        height: f32,
        /// RGBA color in linear float channels.
        color: [f32; 4],
    },
}

/// Read-only scene facts available to one script invocation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VargSceneContext {
    /// User-visible name of the entity this script is attached to.
    pub entity_name: String,
    /// User-visible tag of the entity this script is attached to.
    pub entity_tag: String,
    /// Local positions keyed by user-visible object name.
    pub positions_by_name: HashMap<String, Vec3>,
    /// Local positions grouped by tag.
    pub positions_by_tag: HashMap<String, Vec<Vec3>>,
}

impl VargSceneContext {
    /// Returns true when the owning entity has the requested tag.
    pub fn entity_has_tag(&self, tag: &str) -> bool {
        self.entity_tag == tag
    }

    /// Returns the distance from the owning entity position to the first object
    /// with the given name.
    pub fn distance_to_name(&self, origin: Vec3, name: &str) -> Option<f32> {
        self.positions_by_name
            .get(name)
            .map(|target| (*target - origin).length())
    }

    /// Returns the nearest distance from the owning entity position to objects
    /// with the given tag.
    pub fn distance_to_tag(&self, origin: Vec3, tag: &str) -> Option<f32> {
        self.positions_by_tag
            .get(tag)?
            .iter()
            .map(|target| (*target - origin).length())
            .reduce(f32::min)
    }
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
    ForLoop {
        variable: String,
        range: RangeExpression,
        body: Vec<RuntimeStatement>,
    },
    WhileLoop {
        condition: ConditionExpression,
        body: Vec<RuntimeStatement>,
    },
    Return(Expression),
    Break,
    Continue,
    Wait(Expression),
    DestroySelf,
    UiLabel {
        id: Expression,
        text: Expression,
        x: Expression,
        y: Expression,
    },
    UiRect {
        id: Expression,
        x: Expression,
        y: Expression,
        width: Expression,
        height: Expression,
        color: [Expression; 4],
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

#[derive(Clone, Debug, PartialEq)]
enum RangeExpression {
    Range(Expression, Expression),          // i in 0..10
    RangeInclusive(Expression, Expression), // i in 0..=10
    Count(Expression),                      // i in count(10)
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
    let mut diagnostics = parser.diagnostics;
    if matches!(role, Some(VargFileRole::Logic))
        && !diagnostics.iter().any(|diagnostic| diagnostic.blocking)
    {
        diagnostics.extend(diagnose_runtime_blocks(source));
    }
    diagnostics
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
    let mut diagnostics = parser.diagnostics;
    if matches!(role, Some(VargFileRole::Logic))
        && !diagnostics.iter().any(|diagnostic| diagnostic.blocking)
    {
        diagnostics.extend(diagnose_runtime_blocks(source));
    }
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

/// Compiles a `.varg` behavior declaration into a declarative behavior tree IR.
pub fn compile_behavior_source(
    path: impl AsRef<Path>,
    source: &str,
) -> (Option<VargBehavior>, Vec<VargDiagnostic>) {
    let (ast, mut diagnostics) = parse_source(path, source);
    let Some(ast) = ast else {
        return (None, diagnostics);
    };
    let Some(declaration) = ast
        .declarations
        .iter()
        .find(|declaration| declaration.kind == "behavior")
    else {
        diagnostics.push(VargDiagnostic {
            code: "VARG5000".to_string(),
            severity: VargDiagnosticSeverity::Error,
            line: Some(1),
            column: Some(1),
            message: "logic file does not contain a behavior declaration".to_string(),
            expected: "`behavior Name { ... }`".to_string(),
            suggestion: "Add a behavior declaration or compile a file that contains one."
                .to_string(),
            blocking: true,
            source_line: source.lines().next().map(str::to_string),
        });
        return (None, diagnostics);
    };

    match parse_behavior_declaration(
        source,
        declaration
            .name
            .clone()
            .unwrap_or_else(|| "UnnamedBehavior".to_string()),
        declaration.line,
    ) {
        Ok(behavior) => (Some(behavior), diagnostics),
        Err(error) => {
            diagnostics.push(error);
            (None, diagnostics)
        }
    }
}

/// Parses a `.vscene` world file into the native scene file structure.
///
/// This is the preferred load path for Varg scenes. It parses the authoring
/// source directly into the engine's typed ECS scene model.
pub fn compile_vscene_source_to_scene_file(
    path: impl AsRef<Path>,
    source: &str,
) -> (Option<SceneFile>, Vec<VargDiagnostic>) {
    let path = path.as_ref();
    let (ast, mut diagnostics) = parse_source(path, source);
    if ast.is_none() {
        return (None, diagnostics);
    }
    let document = match parse_vscene_document(source) {
        Ok(document) => document,
        Err(error) => {
            diagnostics.push(error);
            return (None, diagnostics);
        }
    };
    let Some(scene_block) = document.children.iter().find(|block| block.kind == "scene") else {
        diagnostics.push(vscene_error(
            source,
            1,
            1,
            "VSCENE1000",
            ".vscene file does not contain a scene declaration",
            "`scene Name { ... }`",
            "Add a top-level scene declaration.",
        ));
        return (None, diagnostics);
    };

    match compile_vscene_scene(scene_block) {
        Ok(file) => (Some(file), diagnostics),
        Err(mut error) => {
            error.source_line = source
                .lines()
                .nth(error.line.unwrap_or(1).saturating_sub(1))
                .map(str::to_string);
            diagnostics.push(error);
            (None, diagnostics)
        }
    }
}

/// Parses a `.vscene` world file directly into an executable ECS [`Scene`].
pub fn compile_vscene_source_to_scene(
    path: impl AsRef<Path>,
    source: &str,
) -> (Option<Scene>, Vec<VargDiagnostic>) {
    let (file, mut diagnostics) = compile_vscene_source_to_scene_file(path, source);
    let Some(file) = file else {
        return (None, diagnostics);
    };
    match Scene::from_scene_file(file) {
        Ok(scene) => (Some(scene), diagnostics),
        Err(error) => {
            diagnostics.push(VargDiagnostic {
                code: "VSCENE9001".to_string(),
                severity: VargDiagnosticSeverity::Error,
                line: Some(1),
                column: Some(1),
                message: format!("scene construction failed: {error}"),
                expected: "valid ECS scene".to_string(),
                suggestion: "Check generated object IDs, hierarchy, and component data."
                    .to_string(),
                blocking: true,
                source_line: source.lines().next().map(str::to_string),
            });
            (None, diagnostics)
        }
    }
}

/// Serializes an ECS [`Scene`] as native `.vscene` source.
pub fn serialize_scene_to_vscene(
    scene: &Scene,
    name: impl AsRef<str>,
) -> engine_core::EngineResult<String> {
    serialize_scene_file_to_vscene(&scene.to_scene_file(name.as_ref())?)
}

/// Serializes a typed scene file as native `.vscene` source.
pub fn serialize_scene_file_to_vscene(file: &SceneFile) -> engine_core::EngineResult<String> {
    let mut output = String::new();
    output.push_str("scene ");
    output.push_str(&vscene_block_name(&file.name));
    output.push_str(" {\n");

    for record in &file.objects {
        write_vscene_object(&mut output, record, 1)?;
    }

    output.push_str("}\n");
    Ok(output)
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
            ui_commands: Vec::new(),
            destroy_self: false,
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
            scene: &context.scene,
            transform: &mut output.transform,
            state: &mut output.state,
            locals: HashMap::new(),
            logs: &mut output.logs,
            ui_commands: &mut output.ui_commands,
            destroy_self: &mut output.destroy_self,
            should_return: false,
            should_break: false,
            should_continue: false,
        };
        for statement in statements {
            env.execute(statement);
            if env.should_return {
                break;
            }
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

#[derive(Clone, Debug, PartialEq)]
struct BehaviorBlock {
    kind: String,
    name: Option<String>,
    repeat_count: Option<u32>,
    line: usize,
    children: Vec<VargBehaviorNode>,
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

fn parse_behavior_declaration(
    source: &str,
    name: String,
    declaration_line: usize,
) -> Result<VargBehavior, VargDiagnostic> {
    let mut stack: Vec<BehaviorBlock> = Vec::new();
    let mut root_children = Vec::new();
    let mut inside_behavior = false;
    let mut behavior_depth = 0isize;

    for (line_index, raw_line) in source.lines().enumerate() {
        let line = line_index + 1;
        let without_comment = strip_line_comment(raw_line);
        let trimmed = without_comment.trim();
        if trimmed.is_empty() {
            continue;
        }

        if !inside_behavior {
            if line == declaration_line {
                inside_behavior = true;
                behavior_depth =
                    trimmed.matches('{').count() as isize - trimmed.matches('}').count() as isize;
            }
            continue;
        }

        if line == declaration_line {
            continue;
        }

        if trimmed == "}" {
            if let Some(block) = stack.pop() {
                let node = behavior_block_to_node(block)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root_children.push(node);
                }
            } else {
                behavior_depth -= 1;
                if behavior_depth <= 0 {
                    break;
                }
            }
            continue;
        }

        if let Some(block) = parse_behavior_block_header(trimmed, line, source)? {
            stack.push(block);
            behavior_depth += 1;
            continue;
        }

        if let Some(expression) = trimmed.strip_prefix("when ") {
            let node = VargBehaviorNode::Condition {
                expression: expression.trim().to_string(),
            };
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            } else {
                root_children.push(node);
            }
            continue;
        }

        if let Some(expression) = trimmed.strip_prefix("action ") {
            let node = VargBehaviorNode::Action {
                expression: expression.trim().to_string(),
            };
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            } else {
                root_children.push(node);
            }
            continue;
        }

        return Err(behavior_error(
            source,
            line,
            1,
            "VARG5001",
            "unsupported behavior statement",
            "`selector`, `sequence`, `parallel`, `repeat`, `invert`, `succeed`, `when`, or `action`",
            "Rewrite this line using declarative behavior tree syntax.",
        ));
    }

    if let Some(block) = stack.last() {
        return Err(behavior_error(
            source,
            block.line,
            1,
            "VARG5002",
            "unclosed behavior block",
            "Every behavior node block must be closed with `}`.",
            "Add a closing brace for this behavior node.",
        ));
    }

    let root = match root_children.len() {
        0 => {
            return Err(behavior_error(
                source,
                declaration_line,
                1,
                "VARG5003",
                "behavior declaration has no nodes",
                "At least one `when`, `action`, `selector`, `sequence`, or `parallel` node.",
                "Add a root behavior node.",
            ));
        }
        1 => root_children.remove(0),
        _ => VargBehaviorNode::Parallel {
            name: Some("root".to_string()),
            children: root_children,
        },
    };

    Ok(VargBehavior { name, root })
}

fn parse_behavior_block_header(
    trimmed: &str,
    line: usize,
    source: &str,
) -> Result<Option<BehaviorBlock>, VargDiagnostic> {
    if !trimmed.ends_with('{') {
        return Ok(None);
    }
    let before_brace = trimmed.trim_end_matches('{').trim();
    let mut parts = before_brace.split_whitespace();
    let Some(kind) = parts.next() else {
        return Ok(None);
    };
    if !matches!(
        kind,
        "selector" | "sequence" | "parallel" | "repeat" | "invert" | "succeed"
    ) {
        return Ok(None);
    }

    let mut repeat_count = None;
    let name = match kind {
        "repeat" => match parts.next() {
            Some("forever") | None => None,
            Some(value) => {
                repeat_count = Some(value.parse::<u32>().map_err(|_| {
                    behavior_error(
                        source,
                        line,
                        1,
                        "VARG5004",
                        "repeat count must be a positive integer or `forever`",
                        "`repeat 3 { ... }` or `repeat forever { ... }`",
                        "Use an integer repeat count, or omit it for an unbounded repeat.",
                    )
                })?);
                None
            }
        },
        _ => {
            let rest = before_brace[kind.len()..].trim();
            if rest.is_empty() {
                None
            } else {
                parse_quoted(rest).or_else(|| Some(rest.to_string()))
            }
        }
    };

    Ok(Some(BehaviorBlock {
        kind: kind.to_string(),
        name,
        repeat_count,
        line,
        children: Vec::new(),
    }))
}

fn behavior_block_to_node(block: BehaviorBlock) -> Result<VargBehaviorNode, VargDiagnostic> {
    match block.kind.as_str() {
        "sequence" => Ok(VargBehaviorNode::Sequence {
            name: block.name,
            children: block.children,
        }),
        "selector" => Ok(VargBehaviorNode::Selector {
            name: block.name,
            children: block.children,
        }),
        "parallel" => Ok(VargBehaviorNode::Parallel {
            name: block.name,
            children: block.children,
        }),
        "invert" => {
            let child = single_behavior_child(&block)?;
            Ok(VargBehaviorNode::Invert {
                child: Box::new(child),
            })
        }
        "succeed" => {
            let child = single_behavior_child(&block)?;
            Ok(VargBehaviorNode::Succeed {
                child: Box::new(child),
            })
        }
        "repeat" => {
            let child = single_behavior_child(&block)?;
            Ok(VargBehaviorNode::Repeat {
                count: block.repeat_count,
                child: Box::new(child),
            })
        }
        _ => unreachable!("behavior block kind validated before push"),
    }
}

fn single_behavior_child(block: &BehaviorBlock) -> Result<VargBehaviorNode, VargDiagnostic> {
    if block.children.len() == 1 {
        return Ok(block.children[0].clone());
    }
    Err(VargDiagnostic {
        code: "VARG5005".to_string(),
        severity: VargDiagnosticSeverity::Error,
        line: Some(block.line),
        column: Some(1),
        message: format!("`{}` behavior node requires exactly one child", block.kind),
        expected: format!("`{} {{ <one child node> }}`", block.kind),
        suggestion: "Wrap multiple children in a `sequence`, `selector`, or `parallel` node."
            .to_string(),
        blocking: true,
        source_line: None,
    })
}

fn behavior_error(
    source: &str,
    line: usize,
    column: usize,
    code: &str,
    message: &str,
    expected: &str,
    suggestion: &str,
) -> VargDiagnostic {
    VargDiagnostic {
        code: code.to_string(),
        severity: VargDiagnosticSeverity::Error,
        line: Some(line),
        column: Some(column),
        message: message.to_string(),
        expected: expected.to_string(),
        suggestion: suggestion.to_string(),
        blocking: true,
        source_line: source
            .lines()
            .nth(line.saturating_sub(1))
            .map(str::to_string),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct VsceneDocument {
    children: Vec<VsceneBlock>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct VsceneBlock {
    kind: String,
    name: Option<String>,
    line: usize,
    properties: HashMap<String, VsceneValue>,
    children: Vec<VsceneBlock>,
}

#[derive(Clone, Debug, PartialEq)]
enum VsceneValue {
    Number(f32),
    Bool(bool),
    String(String),
    Identifier(String),
    Vec3(Vec3),
    Color(Vec3),
    Call {
        function: String,
        args: HashMap<String, VsceneValue>,
    },
}

fn parse_vscene_document(source: &str) -> Result<VsceneDocument, VargDiagnostic> {
    let mut stack: Vec<VsceneBlock> = Vec::new();
    let mut document = VsceneDocument::default();

    for (line_index, raw_line) in source.lines().enumerate() {
        let line = line_index + 1;
        let without_comment = strip_line_comment(raw_line);
        let trimmed = without_comment.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed == "}" {
            let Some(block) = stack.pop() else {
                return Err(vscene_error(
                    source,
                    line,
                    1,
                    "VSCENE1001",
                    "unexpected closing brace",
                    "A closing brace must match an open block.",
                    "Remove this brace or add the missing block header before it.",
                ));
            };
            if let Some(parent) = stack.last_mut() {
                parent.children.push(block);
            } else {
                document.children.push(block);
            }
            continue;
        }

        if let Some(header) = parse_header(trimmed) {
            stack.push(VsceneBlock {
                kind: header.kind,
                name: header.name,
                line,
                properties: HashMap::new(),
                children: Vec::new(),
            });
            continue;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let Some(block) = stack.last_mut() else {
                return Err(vscene_error(
                    source,
                    line,
                    1,
                    "VSCENE1002",
                    "property declared outside a block",
                    "`property: value` inside a block",
                    "Move this property inside a scene, entity, or component block.",
                ));
            };
            let parsed = parse_vscene_value(value.trim()).ok_or_else(|| {
                vscene_error(
                    source,
                    line,
                    raw_line.find(':').map(|index| index + 2).unwrap_or(1),
                    "VSCENE1003",
                    "unsupported .vscene value syntax",
                    "Use numbers, booleans, strings, identifiers, Vec3(...), Color(...), or Constructor(key: value).",
                    "Simplify the value or add compiler support for this construct.",
                )
            })?;
            block.properties.insert(key.trim().to_string(), parsed);
            continue;
        }

        return Err(vscene_error(
            source,
            line,
            1,
            "VSCENE1004",
            "unsupported .vscene statement",
            "Use `block Name { ... }`, `property: value`, or `}`.",
            "Rewrite this line using the declarative .vscene block syntax.",
        ));
    }

    if let Some(block) = stack.last() {
        return Err(vscene_error(
            source,
            block.line,
            1,
            "VSCENE1005",
            "unclosed .vscene block",
            "Every `{` must be paired with a closing `}`.",
            "Add a closing brace for this block.",
        ));
    }

    Ok(document)
}

fn compile_vscene_scene(scene_block: &VsceneBlock) -> Result<SceneFile, VargDiagnostic> {
    let mut objects = Vec::new();
    let mut next_id = 1_u64;

    for child in &scene_block.children {
        match child.kind.as_str() {
            "camera" | "entity" | "light" => {
                objects.push(compile_vscene_object(child, next_id)?);
                next_id += 1;
            }
            "group" => {
                for nested in &child.children {
                    if matches!(nested.kind.as_str(), "camera" | "entity" | "light") {
                        objects.push(compile_vscene_object(nested, next_id)?);
                        next_id += 1;
                    }
                }
            }
            "intent" | "constraints" | "scatter" => {}
            _ => {
                return Err(vscene_compile_error(
                    child,
                    "VSCENE2000",
                    &format!("unsupported scene child block `{}`", child.kind),
                    "`camera`, `entity`, `light`, `group`, or future generator blocks",
                    "Use an entity-like block supported by the compiler.",
                ));
            }
        }
    }

    Ok(SceneFile {
        version: SCENE_FILE_VERSION,
        name: scene_block
            .name
            .clone()
            .unwrap_or_else(|| "Scene".to_string()),
        objects,
    })
}

fn compile_vscene_object(
    block: &VsceneBlock,
    id: u64,
) -> Result<SerializedGameObject, VargDiagnostic> {
    let name = block
        .name
        .clone()
        .unwrap_or_else(|| format!("{} {id}", block.kind));
    let mut tag = string_property(block, "tag").unwrap_or_else(|| "Untagged".to_string());
    let mut camera_role = None;
    let mut components = Vec::new();
    let mut transform = Transform::IDENTITY;
    let mut mesh_renderer_child = None;
    let mut material_child = None;

    if block.kind == "camera" {
        tag = "MainCamera".to_string();
        camera_role = Some(CameraRole::Main);
        components.push(ComponentData::Camera(compile_camera_component(block)));
    }
    if block.kind == "light" {
        tag = "Light".to_string();
        components.push(ComponentData::Light(compile_light_component(block)));
    }
    apply_vscene_transform_properties(block, &mut transform);

    for child in &block.children {
        match child.kind.as_str() {
            "transform" => {
                transform = Transform::IDENTITY;
                apply_vscene_transform_properties(child, &mut transform);
            }
            "perspective" => upsert_component(
                &mut components,
                ComponentData::Camera(compile_camera_component(child)),
            ),
            "mesh" | "geometry" => mesh_renderer_child = Some(child),
            "material" => material_child = Some(child),
            "rigidbody" => {
                components.push(ComponentData::Rigidbody(compile_rigidbody_component(child)))
            }
            "collider" => {
                components.push(ComponentData::Collider(compile_collider_component(child)))
            }
            "script" => components.push(ComponentData::Script(compile_script_component(child))),
            "light" => components.push(ComponentData::Light(compile_light_component(child))),
            _ => {
                return Err(vscene_compile_error(
                    child,
                    "VSCENE2001",
                    &format!("unsupported object child block `{}`", child.kind),
                    "`transform`, `perspective`, `mesh`, `geometry`, `material`, `rigidbody`, `collider`, `script`, or `light`",
                    "Use a supported component block or extend the .vscene compiler.",
                ));
            }
        }
    }

    if block.properties.contains_key("mesh")
        || block.properties.contains_key("geometry")
        || block.properties.contains_key("material")
        || mesh_renderer_child.is_some()
        || material_child.is_some()
    {
        upsert_component(
            &mut components,
            ComponentData::MeshRenderer(compile_mesh_renderer_component(
                block,
                mesh_renderer_child,
                material_child,
            )),
        );
    }

    Ok(SerializedGameObject {
        object: GameObject {
            id: engine_core::EntityId::from_u128(u128::from(id)),
            name,
            tag,
            layer: 0,
            camera_role,
            active: true,
            scripts: Vec::new(),
            components,
        },
        local_transform: transform,
        parent: None,
        sibling_index: (id - 1) as usize,
    })
}

fn apply_vscene_transform_properties(block: &VsceneBlock, transform: &mut Transform) {
    if let Some(position) = vec3_property(block, "position") {
        transform.translation = position;
    }
    if let Some(rotation) = vec3_property(block, "rotation") {
        transform.rotation = Quat::from_euler_deg(rotation.y, rotation.x, rotation.z);
    }
    if let Some(scale) = vec3_property(block, "scale") {
        transform.scale = scale;
    }
}

fn compile_camera_component(block: &VsceneBlock) -> CameraComponentData {
    CameraComponentData {
        vertical_fov_degrees: number_property(block, "fov").unwrap_or(60.0),
        near: number_property(block, "near").unwrap_or(0.01),
        far: number_property(block, "far").unwrap_or(1000.0),
        aspect_ratio: None,
        primary: bool_property(block, "primary").unwrap_or(true),
        clear_color: Vec3::new(0.1, 0.1, 0.1),
    }
}

fn compile_mesh_renderer_component(
    object: &VsceneBlock,
    mesh: Option<&VsceneBlock>,
    material: Option<&VsceneBlock>,
) -> MeshRendererComponentData {
    let builtin_material = material
        .and_then(vscene_material_builtin)
        .or_else(|| string_property(object, "material"))
        .unwrap_or_else(|| "debug/default".to_string());
    MeshRendererComponentData {
        mesh: None,
        builtin_mesh: Some(
            vscene_mesh_builtin(object, mesh).unwrap_or_else(|| "debug/cube".to_string()),
        ),
        material: MaterialRef {
            asset: None,
            builtin: Some(builtin_material),
        },
        casts_shadows: true,
        receive_shadows: true,
    }
}

fn vscene_mesh_builtin(object: &VsceneBlock, mesh: Option<&VsceneBlock>) -> Option<String> {
    mesh.and_then(vscene_mesh_builtin_from_block)
        .or_else(|| string_property(object, "mesh").and_then(|value| normalize_vscene_mesh(&value)))
        .or_else(|| {
            string_property(object, "geometry").and_then(|value| normalize_vscene_mesh(&value))
        })
        .or_else(|| call_property(object, "mesh").and_then(vscene_mesh_builtin_from_call))
        .or_else(|| call_property(object, "geometry").and_then(vscene_mesh_builtin_from_call))
}

fn vscene_mesh_builtin_from_block(block: &VsceneBlock) -> Option<String> {
    string_property(block, "builtin")
        .or_else(|| string_property(block, "type").and_then(|value| normalize_vscene_mesh(&value)))
        .or_else(|| string_property(block, "kind").and_then(|value| normalize_vscene_mesh(&value)))
        .or_else(|| string_property(block, "path").map(|value| format!("model:{value}")))
        .or_else(|| call_property(block, "type").and_then(vscene_mesh_builtin_from_call))
        .or_else(|| call_property(block, "kind").and_then(vscene_mesh_builtin_from_call))
}

fn normalize_vscene_mesh(value: &str) -> Option<String> {
    let normalized = match value {
        "box" | "cube" | "primitive.box" | "debug/cube" => "debug/cube".to_string(),
        "sphere" | "primitive.sphere" | "debug/sphere" => "debug/sphere".to_string(),
        "plane" | "primitive.plane" | "debug/plane" => "debug/plane".to_string(),
        "cylinder" | "primitive.cylinder" | "debug/cylinder" => "debug/cylinder".to_string(),
        other if other.starts_with("debug/") => other.to_string(),
        other if other.starts_with("model:") => other.to_string(),
        other if other.ends_with(".gltf") || other.ends_with(".glb") => format!("model:{other}"),
        _ => return None,
    };
    Some(normalized)
}

fn vscene_mesh_builtin_from_call(value: &VsceneValue) -> Option<String> {
    let VsceneValue::Call { function, args } = value else {
        return None;
    };
    match function.as_str() {
        "Box" | "Cube" | "primitive.box" => Some("debug/cube".to_string()),
        "Sphere" | "primitive.sphere" => Some("debug/sphere".to_string()),
        "Plane" | "primitive.plane" => Some("debug/plane".to_string()),
        "Cylinder" | "primitive.cylinder" => Some("debug/cylinder".to_string()),
        "Model" => args
            .get("path")
            .and_then(vscene_value_string)
            .map(|path| format!("model:{path}")),
        _ => None,
    }
}

fn vscene_material_builtin(block: &VsceneBlock) -> Option<String> {
    if block.properties.contains_key("baseColor")
        || block.properties.contains_key("color")
        || block.properties.contains_key("emissive")
        || block.properties.contains_key("metallic")
        || block.properties.contains_key("roughness")
    {
        return Some(vscene_inline_material_name(block));
    }
    string_property(block, "builtin")
        .or_else(|| string_property(block, "name"))
        .or_else(|| string_property(block, "type"))
        .or_else(|| string_property(block, "kind"))
}

fn vscene_inline_material_name(block: &VsceneBlock) -> String {
    let base_color = vec3_property(block, "baseColor")
        .or_else(|| vec3_property(block, "color"))
        .unwrap_or(Vec3::ONE);
    let alpha = number_property(block, "alpha").unwrap_or(1.0);
    let metallic = number_property(block, "metallic").unwrap_or(0.0);
    let roughness = number_property(block, "roughness").unwrap_or(0.5);
    let emissive = vec3_property(block, "emissive").unwrap_or(Vec3::ZERO);
    format!(
        "@vscene-material:base={},{},{},{};metallic={};roughness={};emissive={},{},{}",
        base_color.x,
        base_color.y,
        base_color.z,
        alpha,
        metallic,
        roughness,
        emissive.x,
        emissive.y,
        emissive.z
    )
}

fn compile_rigidbody_component(block: &VsceneBlock) -> RigidbodyComponentData {
    RigidbodyComponentData {
        body_type: identifier_property(block, "mode").unwrap_or_else(|| "dynamic".to_string()),
        mass: number_property(block, "mass").unwrap_or(1.0),
        use_gravity: bool_property(block, "useGravity").unwrap_or(true),
        linear_damping: 0.0,
        angular_damping: 0.05,
        lock_position: [false, false, false],
        lock_rotation: [false, false, false],
    }
}

fn compile_collider_component(block: &VsceneBlock) -> ColliderComponentData {
    ColliderComponentData {
        shape: identifier_property(block, "shape").unwrap_or_else(|| "box".to_string()),
        size: vec3_property(block, "size").unwrap_or(Vec3::ONE),
        is_trigger: bool_property(block, "isTrigger").unwrap_or(false),
        mask: u32::MAX,
        physics_material: "default".to_string(),
    }
}

fn compile_script_component(block: &VsceneBlock) -> ScriptComponent {
    let mut exported = HashMap::new();
    for (key, value) in &block.properties {
        if key == "source" {
            continue;
        }
        exported.insert(key.clone(), vscene_value_to_json(value));
    }
    ScriptComponent {
        source: string_property(block, "source").unwrap_or_default(),
        exported_values: exported,
        state: HashMap::new(),
    }
}

fn compile_light_component(block: &VsceneBlock) -> LightComponentData {
    LightComponentData {
        color: vec3_property(block, "color").unwrap_or(Vec3::ONE),
        intensity: number_property(block, "intensity").unwrap_or(1.0),
        kind: identifier_property(block, "kind")
            .or_else(|| identifier_property(block, "type"))
            .unwrap_or_else(|| "point".to_string()),
        range: number_property(block, "range").unwrap_or(10.0),
        spot_angle: number_property(block, "spotAngle").unwrap_or(30.0),
    }
}

fn upsert_component(components: &mut Vec<ComponentData>, component: ComponentData) {
    let component_type = component.type_id();
    if let Some(existing) = components
        .iter_mut()
        .find(|candidate| candidate.type_id() == component_type)
    {
        *existing = component;
    } else {
        components.push(component);
    }
}

fn parse_vscene_value(source: &str) -> Option<VsceneValue> {
    let source = source.trim();
    if source == "true" {
        return Some(VsceneValue::Bool(true));
    }
    if source == "false" {
        return Some(VsceneValue::Bool(false));
    }
    if let Ok(number) = source.parse::<f32>() {
        return Some(VsceneValue::Number(number));
    }
    if let Some(value) = parse_string_literal(source) {
        return Some(VsceneValue::String(value));
    }
    if let Some(args) = source
        .strip_prefix("Vec3(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let parts = split_top_level_commas(args);
        if parts.len() == 3 {
            return Some(VsceneValue::Vec3(Vec3::new(
                parts[0].trim().parse().ok()?,
                parts[1].trim().parse().ok()?,
                parts[2].trim().parse().ok()?,
            )));
        }
    }
    if let Some(args) = source
        .strip_prefix("Color(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let raw = parse_string_literal(args.trim())?;
        return parse_hex_color(&raw).map(VsceneValue::Color);
    }
    if let Some((function, args)) = parse_expression_call(source) {
        let mut parsed_args = HashMap::new();
        for arg in split_top_level_commas(args) {
            let (key, value) = arg.split_once(':')?;
            parsed_args.insert(key.trim().to_string(), parse_vscene_value(value.trim())?);
        }
        return Some(VsceneValue::Call {
            function: function.to_string(),
            args: parsed_args,
        });
    }
    is_vscene_identifier(source).then(|| VsceneValue::Identifier(source.to_string()))
}

fn is_vscene_identifier(source: &str) -> bool {
    let mut chars = source.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| {
            ch == '_' || ch == '-' || ch == '/' || ch == '.' || ch.is_ascii_alphanumeric()
        })
}

fn parse_hex_color(source: &str) -> Option<Vec3> {
    let hex = source.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f32 / 255.0;
    Some(Vec3::new(r, g, b))
}

fn number_property(block: &VsceneBlock, key: &str) -> Option<f32> {
    match block.properties.get(key)? {
        VsceneValue::Number(value) => Some(*value),
        _ => None,
    }
}

fn bool_property(block: &VsceneBlock, key: &str) -> Option<bool> {
    match block.properties.get(key)? {
        VsceneValue::Bool(value) => Some(*value),
        _ => None,
    }
}

fn string_property(block: &VsceneBlock, key: &str) -> Option<String> {
    match block.properties.get(key)? {
        VsceneValue::String(value) | VsceneValue::Identifier(value) => Some(value.clone()),
        _ => None,
    }
}

fn identifier_property(block: &VsceneBlock, key: &str) -> Option<String> {
    match block.properties.get(key)? {
        VsceneValue::Identifier(value) | VsceneValue::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn call_property<'a>(block: &'a VsceneBlock, key: &str) -> Option<&'a VsceneValue> {
    match block.properties.get(key)? {
        value @ VsceneValue::Call { .. } => Some(value),
        _ => None,
    }
}

fn vscene_value_string(value: &VsceneValue) -> Option<String> {
    match value {
        VsceneValue::String(value) | VsceneValue::Identifier(value) => Some(value.clone()),
        _ => None,
    }
}

fn vec3_property(block: &VsceneBlock, key: &str) -> Option<Vec3> {
    match block.properties.get(key)? {
        VsceneValue::Vec3(value) | VsceneValue::Color(value) => Some(*value),
        _ => None,
    }
}

fn vscene_value_to_json(value: &VsceneValue) -> serde_json::Value {
    match value {
        VsceneValue::Number(value) => serde_json::json!(value),
        VsceneValue::Bool(value) => serde_json::json!(value),
        VsceneValue::String(value) | VsceneValue::Identifier(value) => serde_json::json!(value),
        VsceneValue::Vec3(value) | VsceneValue::Color(value) => vec3_json(*value),
        VsceneValue::Call { function, args } => {
            let mut object = serde_json::Map::new();
            object.insert("type".to_string(), serde_json::json!(function));
            for (key, value) in args {
                object.insert(key.clone(), vscene_value_to_json(value));
            }
            serde_json::Value::Object(object)
        }
    }
}

fn vec3_json(value: Vec3) -> serde_json::Value {
    serde_json::json!({
        "x": value.x,
        "y": value.y,
        "z": value.z,
    })
}

fn vscene_compile_error(
    block: &VsceneBlock,
    code: &str,
    message: &str,
    expected: &str,
    suggestion: &str,
) -> VargDiagnostic {
    VargDiagnostic {
        code: code.to_string(),
        severity: VargDiagnosticSeverity::Error,
        line: Some(block.line),
        column: Some(1),
        message: message.to_string(),
        expected: expected.to_string(),
        suggestion: suggestion.to_string(),
        blocking: true,
        source_line: None,
    }
}

fn write_vscene_object(
    output: &mut String,
    record: &SerializedGameObject,
    indent: usize,
) -> engine_core::EngineResult<()> {
    let is_camera = record.object.camera_role == Some(CameraRole::Main)
        || record
            .object
            .components
            .iter()
            .any(|component| matches!(component, ComponentData::Camera(_)));
    let standalone_light = (!is_camera)
        .then(|| {
            record
                .object
                .components
                .iter()
                .find_map(|component| match component {
                    ComponentData::Light(light) if record.object.components.len() == 1 => {
                        Some(light)
                    }
                    _ => None,
                })
        })
        .flatten();
    write_indent(output, indent);
    output.push_str(if is_camera {
        "camera "
    } else if standalone_light.is_some() {
        "light "
    } else {
        "entity "
    });
    output.push_str(&vscene_quoted(&record.object.name));
    output.push_str(" {\n");

    if !is_camera && standalone_light.is_none() && record.object.tag != "Untagged" {
        write_property(
            output,
            indent + 1,
            "tag",
            &vscene_quoted(&record.object.tag),
        );
    }

    write_transform_block(output, indent + 1, record.local_transform);
    if let Some(light) = standalone_light {
        write_light_properties(output, indent + 1, light);
    }

    for component in &record.object.components {
        if standalone_light.is_some() && matches!(component, ComponentData::Light(_)) {
            continue;
        }
        match component {
            ComponentData::Camera(camera) => write_camera_block(output, indent + 1, camera),
            ComponentData::MeshRenderer(mesh) => {
                write_mesh_renderer_block(output, indent + 1, mesh)?
            }
            ComponentData::Rigidbody(rigidbody) => {
                write_rigidbody_block(output, indent + 1, rigidbody);
            }
            ComponentData::Collider(collider) => write_collider_block(output, indent + 1, collider),
            ComponentData::Script(script) => write_script_block(output, indent + 1, script),
            ComponentData::Light(light) => write_light_block(output, indent + 1, light),
            other => {
                return Err(engine_core::EngineError::config(format!(
                    "native .vscene writer does not support {} components yet",
                    other.type_id()
                )));
            }
        }
    }

    write_indent(output, indent);
    output.push_str("}\n\n");
    Ok(())
}

fn write_transform_block(output: &mut String, indent: usize, transform: Transform) {
    write_indent(output, indent);
    output.push_str("transform {\n");
    write_property(
        output,
        indent + 1,
        "position",
        &vscene_vec3(transform.translation),
    );
    let (yaw, pitch, roll) = transform.rotation.to_euler_deg();
    write_property(
        output,
        indent + 1,
        "rotation",
        &vscene_vec3(Vec3::new(pitch, yaw, roll)),
    );
    write_property(output, indent + 1, "scale", &vscene_vec3(transform.scale));
    write_indent(output, indent);
    output.push_str("}\n");
}

fn write_camera_block(output: &mut String, indent: usize, camera: &CameraComponentData) {
    write_indent(output, indent);
    output.push_str("perspective {\n");
    write_property(
        output,
        indent + 1,
        "fov",
        &vscene_number(camera.vertical_fov_degrees),
    );
    write_property(output, indent + 1, "near", &vscene_number(camera.near));
    write_property(output, indent + 1, "far", &vscene_number(camera.far));
    write_indent(output, indent);
    output.push_str("}\n");
    write_property(
        output,
        indent,
        "primary",
        if camera.primary { "true" } else { "false" },
    );
}

fn write_mesh_renderer_block(
    output: &mut String,
    indent: usize,
    mesh: &MeshRendererComponentData,
) -> engine_core::EngineResult<()> {
    if mesh.mesh.is_some() {
        return Err(engine_core::EngineError::config(
            "native .vscene writer does not support asset mesh references yet",
        ));
    }
    let builtin_mesh = mesh
        .builtin_mesh
        .as_deref()
        .unwrap_or("debug/cube")
        .to_string();
    write_property(output, indent, "mesh", &builtin_mesh);
    if let Some(builtin_material) = mesh.material.builtin.as_deref() {
        write_indent(output, indent);
        output.push_str("material {\n");
        write_property(
            output,
            indent + 1,
            "builtin",
            &vscene_quoted(builtin_material),
        );
        write_indent(output, indent);
        output.push_str("}\n");
    }
    Ok(())
}

fn write_rigidbody_block(output: &mut String, indent: usize, rigidbody: &RigidbodyComponentData) {
    write_indent(output, indent);
    output.push_str("rigidbody {\n");
    write_property(output, indent + 1, "mode", &rigidbody.body_type);
    write_property(output, indent + 1, "mass", &vscene_number(rigidbody.mass));
    write_property(
        output,
        indent + 1,
        "useGravity",
        if rigidbody.use_gravity {
            "true"
        } else {
            "false"
        },
    );
    write_indent(output, indent);
    output.push_str("}\n");
}

fn write_collider_block(output: &mut String, indent: usize, collider: &ColliderComponentData) {
    write_indent(output, indent);
    output.push_str("collider {\n");
    write_property(output, indent + 1, "shape", &collider.shape);
    write_property(output, indent + 1, "size", &vscene_vec3(collider.size));
    write_property(
        output,
        indent + 1,
        "isTrigger",
        if collider.is_trigger { "true" } else { "false" },
    );
    write_indent(output, indent);
    output.push_str("}\n");
}

fn write_script_block(output: &mut String, indent: usize, script: &ScriptComponent) {
    write_indent(output, indent);
    output.push_str("script ");
    output.push_str(&vscene_block_name(
        script
            .source
            .rsplit('/')
            .next()
            .and_then(|name| name.strip_suffix(".varg"))
            .unwrap_or("Script"),
    ));
    output.push_str(" {\n");
    write_property(output, indent + 1, "source", &vscene_quoted(&script.source));
    let mut exported = script.exported_values.iter().collect::<Vec<_>>();
    exported.sort_by(|left, right| left.0.cmp(right.0));
    for (key, value) in exported {
        write_property(output, indent + 1, key, &json_value_to_vscene(value));
    }
    write_indent(output, indent);
    output.push_str("}\n");
}

fn write_light_block(output: &mut String, indent: usize, light: &LightComponentData) {
    write_indent(output, indent);
    output.push_str("light {\n");
    write_light_properties(output, indent + 1, light);
    write_indent(output, indent);
    output.push_str("}\n");
}

fn write_light_properties(output: &mut String, indent: usize, light: &LightComponentData) {
    write_property(output, indent, "kind", &light.kind);
    write_property(output, indent, "color", &vscene_vec3(light.color));
    write_property(output, indent, "intensity", &vscene_number(light.intensity));
    write_property(output, indent, "range", &vscene_number(light.range));
    write_property(
        output,
        indent,
        "spotAngle",
        &vscene_number(light.spot_angle),
    );
}

fn write_property(output: &mut String, indent: usize, key: &str, value: &str) {
    write_indent(output, indent);
    output.push_str(key);
    output.push_str(": ");
    output.push_str(value);
    output.push('\n');
}

fn write_indent(output: &mut String, indent: usize) {
    for _ in 0..indent {
        output.push_str("    ");
    }
}

fn vscene_block_name(name: &str) -> String {
    if is_vscene_identifier(name) {
        name.to_string()
    } else {
        vscene_quoted(name)
    }
}

fn vscene_quoted(value: &str) -> String {
    format!("{:?}", value)
}

fn vscene_vec3(value: Vec3) -> String {
    format!(
        "Vec3({}, {}, {})",
        vscene_number(value.x),
        vscene_number(value.y),
        vscene_number(value.z)
    )
}

fn vscene_number(value: f32) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

fn json_value_to_vscene(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => vscene_quoted(value),
        serde_json::Value::Object(object) => {
            if let (Some(x), Some(y), Some(z)) = (object.get("x"), object.get("y"), object.get("z"))
            {
                return format!(
                    "Vec3({}, {}, {})",
                    json_value_to_vscene(x),
                    json_value_to_vscene(y),
                    json_value_to_vscene(z)
                );
            }
            vscene_quoted(&value.to_string())
        }
        _ => vscene_quoted(&value.to_string()),
    }
}

fn vscene_error(
    source: &str,
    line: usize,
    column: usize,
    code: &str,
    message: &str,
    expected: &str,
    suggestion: &str,
) -> VargDiagnostic {
    VargDiagnostic {
        code: code.to_string(),
        severity: VargDiagnosticSeverity::Error,
        line: Some(line),
        column: Some(column),
        message: message.to_string(),
        expected: expected.to_string(),
        suggestion: suggestion.to_string(),
        blocking: true,
        source_line: source
            .lines()
            .nth(line.saturating_sub(1))
            .map(str::to_string),
    }
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
    scene: &'a VargSceneContext,
    transform: &'a mut Transform,
    state: &'a mut HashMap<String, serde_json::Value>,
    locals: HashMap<String, serde_json::Value>,
    logs: &'a mut Vec<String>,
    ui_commands: &'a mut Vec<VargUiCommand>,
    destroy_self: &'a mut bool,
    /// When true, the current function should return.
    should_return: bool,
    /// When true, the current loop should break.
    should_break: bool,
    /// When true, skip to the next loop iteration.
    should_continue: bool,
}

impl RuntimeEnvironment<'_> {
    fn execute(&mut self, statement: &RuntimeStatement) {
        if self.should_return {
            return;
        }
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
                    if self.should_return || self.should_break || self.should_continue {
                        break;
                    }
                }
            }
            RuntimeStatement::ForLoop {
                variable,
                range,
                body,
            } => {
                let (start, end, inclusive) = match range {
                    RangeExpression::Range(s, e) => (
                        self.eval_number(s) as i32,
                        self.eval_number(e) as i32,
                        false,
                    ),
                    RangeExpression::RangeInclusive(s, e) => {
                        (self.eval_number(s) as i32, self.eval_number(e) as i32, true)
                    }
                    RangeExpression::Count(n) => (0, self.eval_number(n) as i32, false),
                };

                let limit = if inclusive { end + 1 } else { end };
                for i in start..limit {
                    self.locals
                        .insert(variable.clone(), serde_json::Value::from(i as f64));
                    self.should_continue = false;

                    for statement in body {
                        self.execute(statement);
                        if self.should_return || self.should_break || self.should_continue {
                            break;
                        }
                    }

                    if self.should_return || self.should_break {
                        break;
                    }
                }
                self.should_break = false;
                self.locals.remove(variable);
            }
            RuntimeStatement::WhileLoop { condition, body } => {
                const MAX_ITERATIONS: usize = 10000;
                let mut iterations = 0;

                while self.eval_condition(condition) && iterations < MAX_ITERATIONS {
                    iterations += 1;
                    self.should_continue = false;

                    for statement in body {
                        self.execute(statement);
                        if self.should_return || self.should_break || self.should_continue {
                            break;
                        }
                    }

                    if self.should_return || self.should_break {
                        break;
                    }
                }
                self.should_break = false;
            }
            RuntimeStatement::Return(_) => {
                self.should_return = true;
            }
            RuntimeStatement::Break => {
                self.should_break = true;
            }
            RuntimeStatement::Continue => {
                self.should_continue = true;
            }
            RuntimeStatement::Wait(duration) => {
                let seconds = self.eval_number(duration);
                if seconds > 0.0 {
                    let timer_key = "__wait_timer";

                    // Check if we're already waiting
                    if let Some(remaining) = self.state.get(timer_key).and_then(|v| v.as_f64()) {
                        let remaining = remaining as f32;
                        let new_remaining = remaining - self.delta_time;

                        if new_remaining > 0.0 {
                            // Still waiting
                            self.state.insert(
                                timer_key.to_string(),
                                serde_json::Value::from(new_remaining as f64),
                            );
                            self.should_return = true;
                        } else {
                            // Wait finished, clear timer and continue
                            self.state.remove(timer_key);
                        }
                    } else {
                        // Start new wait
                        self.state.insert(
                            timer_key.to_string(),
                            serde_json::Value::from(seconds as f64),
                        );
                        self.should_return = true;
                    }
                }
            }
            RuntimeStatement::DestroySelf => {
                *self.destroy_self = true;
                self.should_return = true;
            }
            RuntimeStatement::UiLabel { id, text, x, y } => {
                self.ui_commands.push(VargUiCommand::Label {
                    id: self.eval_string(id).unwrap_or_default(),
                    text: self.eval_display_string(text),
                    x: self.eval_number(x),
                    y: self.eval_number(y),
                });
            }
            RuntimeStatement::UiRect {
                id,
                x,
                y,
                width,
                height,
                color,
            } => {
                self.ui_commands.push(VargUiCommand::Rect {
                    id: self.eval_string(id).unwrap_or_default(),
                    x: self.eval_number(x),
                    y: self.eval_number(y),
                    width: self.eval_number(width).max(0.0),
                    height: self.eval_number(height).max(0.0),
                    color: [
                        self.eval_number(&color[0]).clamp(0.0, 1.0),
                        self.eval_number(&color[1]).clamp(0.0, 1.0),
                        self.eval_number(&color[2]).clamp(0.0, 1.0),
                        self.eval_number(&color[3]).clamp(0.0, 1.0),
                    ],
                });
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
            "entity.hasTag" => {
                if args.len() != 1 {
                    return 0.0;
                }
                self.eval_string(&args[0])
                    .is_some_and(|tag| self.scene.entity_has_tag(&tag)) as u8 as f32
            }
            "scene.distanceTo" | "distanceTo" => {
                if args.len() != 1 {
                    return 0.0;
                }
                self.eval_string(&args[0])
                    .and_then(|name| {
                        self.scene
                            .distance_to_name(self.transform.translation, &name)
                    })
                    .unwrap_or(0.0)
            }
            "scene.distanceToTag" | "distanceToTag" => {
                if args.len() != 1 {
                    return 0.0;
                }
                self.eval_string(&args[0])
                    .and_then(|tag| self.scene.distance_to_tag(self.transform.translation, &tag))
                    .unwrap_or(0.0)
            }
            "playerDistance" | "scene.playerDistance" => self
                .scene
                .distance_to_tag(self.transform.translation, "Player")
                .or_else(|| {
                    self.scene
                        .distance_to_name(self.transform.translation, "Player")
                })
                .unwrap_or(0.0),
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

    fn eval_string(&self, expression: &Expression) -> Option<String> {
        match expression {
            Expression::String(value) => Some(value.clone()),
            Expression::Variable(name) => self
                .locals
                .get(name)
                .or_else(|| self.state.get(name))
                .and_then(|value| value.as_str())
                .map(str::to_string),
            Expression::Member(owner, field) => self
                .state
                .get(owner)
                .and_then(|value| value.get(field))
                .and_then(|value| value.as_str())
                .map(str::to_string),
            _ => None,
        }
    }

    fn eval_display_string(&self, expression: &Expression) -> String {
        self.eval_string(expression)
            .unwrap_or_else(|| self.eval_number(expression).to_string())
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

fn diagnose_runtime_blocks(source: &str) -> Vec<VargDiagnostic> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut diagnostics = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = strip_line_comment(lines[index]).trim();
        if let Some(_signature) = parse_function_signature(trimmed) {
            let (body, next) = collect_block(&lines, index);
            let mut body_index = 0usize;
            let _ = parse_runtime_statements_with_diagnostics(
                &body,
                &mut body_index,
                &mut diagnostics,
                source,
            );
            index = next;
            continue;
        }
        index += 1;
    }
    diagnostics
}

fn parse_runtime_statements(lines: &[RuntimeLine], index: &mut usize) -> Vec<RuntimeStatement> {
    let mut diagnostics = Vec::new();
    parse_runtime_statements_with_diagnostics(lines, index, &mut diagnostics, "")
}

fn parse_runtime_statements_with_diagnostics(
    lines: &[RuntimeLine],
    index: &mut usize,
    diagnostics: &mut Vec<VargDiagnostic>,
    source: &str,
) -> Vec<RuntimeStatement> {
    let mut statements = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        let trimmed = strip_line_comment(&line.text).trim();
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
                statements: parse_runtime_statements_with_diagnostics(
                    &nested,
                    &mut nested_index,
                    diagnostics,
                    source,
                ),
                else_statements: parse_runtime_statements_with_diagnostics(
                    &else_nested,
                    &mut else_index,
                    diagnostics,
                    source,
                ),
            });
            continue;
        }
        if let Some((variable, range)) = parse_for_loop(trimmed) {
            let body = collect_inline_or_block(lines, index);
            let mut body_index = 0usize;
            statements.push(RuntimeStatement::ForLoop {
                variable,
                range,
                body: parse_runtime_statements_with_diagnostics(
                    &body,
                    &mut body_index,
                    diagnostics,
                    source,
                ),
            });
            continue;
        }
        if let Some(condition) = parse_while_loop(trimmed) {
            let body = collect_inline_or_block(lines, index);
            let mut body_index = 0usize;
            statements.push(RuntimeStatement::WhileLoop {
                condition,
                body: parse_runtime_statements_with_diagnostics(
                    &body,
                    &mut body_index,
                    diagnostics,
                    source,
                ),
            });
            continue;
        }
        if let Some(statement) = parse_runtime_statement(trimmed) {
            statements.push(statement);
        } else {
            diagnostics.push(unsupported_runtime_statement_diagnostic(
                source,
                line.line_no,
                &line.text,
                trimmed,
            ));
        }
    }
    statements
}

fn parse_runtime_statement(line: &str) -> Option<RuntimeStatement> {
    if line.trim() == "break" {
        return Some(RuntimeStatement::Break);
    }
    if line.trim() == "continue" {
        return Some(RuntimeStatement::Continue);
    }
    if let Some(expr) = line.strip_prefix("return ") {
        return Some(RuntimeStatement::Return(parse_expression(expr.trim())?));
    }
    if line.trim() == "return" {
        return Some(RuntimeStatement::Return(Expression::Number(0.0)));
    }
    if let Some(content) = function_args(line, "wait") {
        return Some(RuntimeStatement::Wait(parse_expression(content)?));
    }
    if line.trim() == "entity.destroy()" || line.trim() == "destroySelf()" {
        return Some(RuntimeStatement::DestroySelf);
    }
    if let Some(content) = function_args(line, "log") {
        return parse_string_literal(content).map(RuntimeStatement::Log);
    }
    if let Some(content) = method_args(line, "entity.translate") {
        return parse_expression(content).map(RuntimeStatement::Translate);
    }
    if let Some(content) = method_args(line, "ui.label") {
        let args = split_top_level_commas(content);
        if args.len() == 4 {
            return Some(RuntimeStatement::UiLabel {
                id: parse_expression(args[0])?,
                text: parse_expression(args[1])?,
                x: parse_expression(args[2])?,
                y: parse_expression(args[3])?,
            });
        }
    }
    if let Some(content) = method_args(line, "ui.rect") {
        let args = split_top_level_commas(content);
        if args.len() == 9 {
            return Some(RuntimeStatement::UiRect {
                id: parse_expression(args[0])?,
                x: parse_expression(args[1])?,
                y: parse_expression(args[2])?,
                width: parse_expression(args[3])?,
                height: parse_expression(args[4])?,
                color: [
                    parse_expression(args[5])?,
                    parse_expression(args[6])?,
                    parse_expression(args[7])?,
                    parse_expression(args[8])?,
                ],
            });
        }
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
        return parse_string_literal(action).map(ConditionExpression::InputJustPressed);
    }
    if let Some(action) = function_args(condition, "Input.down") {
        return parse_string_literal(action).map(ConditionExpression::InputDown);
    }
    if let Some(action) = function_args(condition, "Input.justPressed") {
        return parse_string_literal(action).map(ConditionExpression::InputJustPressed);
    }
    if let Some(action) = function_args(condition, "Input.pressedThisFrame") {
        return parse_string_literal(action).map(ConditionExpression::InputJustPressed);
    }
    if let Some(action) = function_args(condition, "Input.justReleased") {
        return parse_string_literal(action).map(ConditionExpression::InputJustReleased);
    }
    if let Some(action) = function_args(condition, "Input.released") {
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
    if parse_expression_call(condition).is_some_and(|(function, _)| {
        matches!(
            function,
            "entity.hasTag"
                | "scene.distanceTo"
                | "distanceTo"
                | "scene.distanceToTag"
                | "distanceToTag"
                | "playerDistance"
                | "scene.playerDistance"
        )
    }) {
        return Some(ConditionExpression::Compare {
            lhs: parse_expression(condition)?,
            op: CompareOp::NotEqual,
            rhs: Expression::Number(0.0),
        });
    }
    if is_truthy_condition_source(condition) {
        return Some(ConditionExpression::Compare {
            lhs: parse_expression(condition)?,
            op: CompareOp::NotEqual,
            rhs: Expression::Number(0.0),
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
    if let Some(action) = function_args(source, "Input.value") {
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

fn is_truthy_condition_source(source: &str) -> bool {
    let source = source.trim();
    if is_valid_runtime_binding_name(source) {
        return true;
    }
    source
        .strip_prefix("state.")
        .is_some_and(is_valid_runtime_binding_name)
}

fn unsupported_runtime_statement_diagnostic(
    source: &str,
    line_no: usize,
    raw_line: &str,
    trimmed: &str,
) -> VargDiagnostic {
    let column = raw_line
        .find(trimmed)
        .map(|index| index + 1)
        .unwrap_or_else(|| {
            raw_line
                .chars()
                .position(|ch| !ch.is_whitespace())
                .map(|index| index + 1)
                .unwrap_or(1)
        });
    let (message, expected, suggestion) = unsupported_runtime_statement_help(trimmed);
    VargDiagnostic {
        code: "VARG4100".to_string(),
        severity: VargDiagnosticSeverity::Error,
        line: Some(line_no),
        column: Some(column),
        message,
        expected,
        suggestion,
        blocking: true,
        source_line: Some(
            source
                .lines()
                .nth(line_no.saturating_sub(1))
                .unwrap_or(raw_line)
                .to_string(),
        ),
    }
}

fn unsupported_runtime_statement_help(trimmed: &str) -> (String, String, String) {
    if trimmed.starts_with("emit(") || trimmed.starts_with("emit ") {
        return (
            "unsupported runtime API `emit`".to_string(),
            "The MVP runtime supports local script state, transform position changes, Input, Time, Math, `log(...)`, `wait(...)`, and basic `ui.*(...)` draw commands."
                .to_string(),
            "`emit(...)` is in the target language direction but is not wired into this runtime yet. Store a value in `state.*` or use `log(...)` for now."
                .to_string(),
        );
    }

    if trimmed.contains("entity.velocity") {
        return (
            "unsupported entity API `entity.velocity`".to_string(),
            "Use `entity.translate(Vec3(...))`, `position = Vec3(...)`, or `position.x/y/z` assignment in the MVP runtime."
                .to_string(),
            "For transform-only motion, replace velocity mutation with `position.y += jumpForce * dt` or `entity.translate(Vec3(0, jumpForce * dt, 0))`."
                .to_string(),
        );
    }

    if trimmed.starts_with("if ") {
        return (
            "unsupported or malformed `if` condition".to_string(),
            "Supported conditions use Input checks, numeric comparisons, `!`, `&&`, and `||`."
                .to_string(),
            "Rewrite the condition with supported bindings such as `Input.down(\"jump\")`, `state.count > 0`, or `position.y <= 1.0`."
                .to_string(),
        );
    }

    if trimmed.starts_with("for ") {
        return (
            "unsupported or malformed `for` loop".to_string(),
            "Supported loops are `for i in 0..10`, `for i in 0..=10`, and `for i in count(n)`."
                .to_string(),
            "Rewrite the loop range using one of the supported range forms.".to_string(),
        );
    }

    if trimmed.starts_with("while ") {
        return (
            "unsupported or malformed `while` condition".to_string(),
            "Supported conditions use Input checks, numeric comparisons, `!`, `&&`, and `||`."
                .to_string(),
            "Rewrite the condition with supported numeric state, local, Time, Input, or position bindings."
                .to_string(),
        );
    }

    (
        "unsupported runtime statement".to_string(),
        "Supported statements are `let`/`var` locals, state assignment, position assignment, `entity.translate(...)`, `ui.label(...)`, `ui.rect(...)`, `if`, `for`, `while`, `return`, `break`, `continue`, `wait(...)`, and `log(...)`."
            .to_string(),
        "Rewrite this line using the supported MVP script API, or add runtime support before using this language construct."
            .to_string(),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeLine {
    line_no: usize,
    text: String,
}

fn collect_inline_or_block(lines: &[RuntimeLine], index: &mut usize) -> Vec<RuntimeLine> {
    let mut collected = Vec::new();
    let mut depth = 1isize;
    while *index < lines.len() {
        let line = lines[*index].clone();
        *index += 1;
        let trimmed = strip_line_comment(&line.text).trim();
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

fn collect_else_block(lines: &[RuntimeLine], index: &mut usize) -> Vec<RuntimeLine> {
    if *index >= lines.len() {
        return Vec::new();
    }
    let trimmed = strip_line_comment(&lines[*index].text).trim();
    if trimmed == "else {" || trimmed == "} else {" {
        *index += 1;
        return collect_inline_or_block(lines, index);
    }
    Vec::new()
}

fn collect_block(lines: &[&str], start: usize) -> (Vec<RuntimeLine>, usize) {
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
        body.push(RuntimeLine {
            line_no: index + 1,
            text: line.to_string(),
        });
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
    if let Some(number) = value.as_f64() {
        return Some(number as f32);
    }
    value.as_bool().map(|value| if value { 1.0 } else { 0.0 })
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

fn parse_for_loop(line: &str) -> Option<(String, RangeExpression)> {
    let rest = line.strip_prefix("for ")?.trim();
    let (loop_part, _) = rest.split_once('{').unwrap_or((rest, ""));
    let loop_part = loop_part.trim();

    let (variable, range_part) = loop_part.split_once(" in ")?;
    let variable = variable.trim().to_string();
    let range_part = range_part.trim();

    // Parse count(n) syntax
    if let Some(count_expr) = function_args(range_part, "count") {
        let expr = parse_expression(count_expr)?;
        return Some((variable, RangeExpression::Count(expr)));
    }

    // Parse range expressions: a..b or a..=b
    if let Some((start_str, end_str)) = range_part.split_once("..=") {
        let start = parse_expression(start_str.trim())?;
        let end = parse_expression(end_str.trim())?;
        return Some((variable, RangeExpression::RangeInclusive(start, end)));
    }

    if let Some((start_str, end_str)) = range_part.split_once("..") {
        let start = parse_expression(start_str.trim())?;
        let end = parse_expression(end_str.trim())?;
        return Some((variable, RangeExpression::Range(start, end)));
    }

    None
}

fn parse_while_loop(line: &str) -> Option<ConditionExpression> {
    let rest = line.strip_prefix("while ")?.trim();
    let condition = rest.strip_suffix('{').unwrap_or(rest).trim();
    parse_condition_expression(condition)
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
    fn compiles_vscene_to_native_scene_file() {
        let source = r##"scene Example {
    camera "Main Camera" {
        transform {
            position: Vec3(0, 1.5, -6)
        }

        perspective {
            fov: 60
            near: 0.01
            far: 1000
        }

        primary: true
    }

    entity "Player" {
        tag: "Player"

        transform {
            position: Vec3(0, 0, 0)
        }

        mesh: Box(size: Vec3(1, 1, 1))

        material {
            baseColor: Color("#7aa2ff")
            roughness: 0.7
        }

        rigidbody {
            mode: kinematic
        }

        collider {
            shape: box
            size: Vec3(1, 1, 1)
        }

        script PlayerController {
            source: "scripts/player_controller.varg"
            speed: 6.0
            jumpForce: 8.0
        }
    }
}
"##;

        let (file, diagnostics) =
            compile_vscene_source_to_scene_file("scenes/example.vscene", source);

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let file = file.unwrap();
        assert_eq!(file.name, "Example");
        assert_eq!(file.objects.len(), 2);
        assert_eq!(file.objects[1].object.name, "Player");
        let script = file.objects[1]
            .object
            .components
            .iter()
            .find_map(|component| match component {
                ComponentData::Script(script) => Some(script),
                _ => None,
            })
            .expect("player should have script component");
        assert_eq!(script.source, "scripts/player_controller.varg");
    }

    #[test]
    fn compiles_declarative_scene_geometry_concepts_to_vscene_mesh_renderers() {
        let source = r##"scene GeometryMigration {
    entity BoxActor {
        mesh: Box(size: Vec3(1, 2, 3))
        material {
            builtin: "debug/red"
        }
    }

    entity SphereActor {
        geometry {
            type: sphere
        }
        material: "debug/blue"
    }

    entity PlaneActor {
        mesh: plane
    }

    entity ModelActor {
        geometry {
            path: "models/ship.gltf"
        }
    }
}
"##;

        let (file, diagnostics) =
            compile_vscene_source_to_scene_file("scenes/geometry.vscene", source);

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let file = file.unwrap();
        assert_eq!(file.objects.len(), 4);

        let mesh_for = |name: &str| {
            file.objects
                .iter()
                .find(|record| record.object.name == name)
                .and_then(|record| {
                    record
                        .object
                        .components
                        .iter()
                        .find_map(|component| match component {
                            ComponentData::MeshRenderer(mesh) => Some(mesh),
                            _ => None,
                        })
                })
                .expect("object should have mesh renderer")
        };

        let box_mesh = mesh_for("BoxActor");
        assert_eq!(box_mesh.builtin_mesh.as_deref(), Some("debug/cube"));
        assert_eq!(box_mesh.material.builtin.as_deref(), Some("debug/red"));

        let sphere_mesh = mesh_for("SphereActor");
        assert_eq!(sphere_mesh.builtin_mesh.as_deref(), Some("debug/sphere"));
        assert_eq!(sphere_mesh.material.builtin.as_deref(), Some("debug/blue"));

        assert_eq!(
            mesh_for("PlaneActor").builtin_mesh.as_deref(),
            Some("debug/plane")
        );
        assert_eq!(
            mesh_for("ModelActor").builtin_mesh.as_deref(),
            Some("model:models/ship.gltf")
        );
    }

    #[test]
    fn compiles_top_level_light_blocks_to_light_objects() {
        let source = r##"scene Lighting {
    light "Sun" {
        kind: directional
        intensity: 3.5
        color: Vec3(1.0, 0.94, 0.84)
        rotation: Vec3(-50, 35, 0)
    }
}
"##;

        let (file, diagnostics) =
            compile_vscene_source_to_scene_file("scenes/lighting.vscene", source);

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let file = file.unwrap();
        assert_eq!(file.objects.len(), 1);
        assert_eq!(file.objects[0].object.name, "Sun");
        assert_eq!(file.objects[0].object.tag, "Light");
        let light = file.objects[0]
            .object
            .components
            .iter()
            .find_map(|component| match component {
                ComponentData::Light(light) => Some(light),
                _ => None,
            })
            .expect("Sun should have light component");
        assert_eq!(light.kind, "directional");
        assert_eq!(light.intensity, 3.5);

        let serialized = serialize_scene_file_to_vscene(&file).unwrap();
        assert!(serialized.contains("light \"Sun\""));
        assert!(serialized.contains("kind: directional"));
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
    fn rejects_unsupported_runtime_statement_with_source_location() {
        let diagnostics = diagnose_source(
            "scripts/player.varg",
            r#"script Player {
    func update(_ dt: Float) {
        emit("coin_collected")
    }
}
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.code, "VARG4100");
        assert_eq!(diagnostic.line, Some(3));
        assert_eq!(diagnostic.column, Some(9));
        assert!(diagnostic.message.contains("emit"));
        assert!(diagnostic.expected.contains("MVP runtime"));
        assert!(diagnostic.suggestion.contains("not wired"));
        assert_eq!(
            diagnostic.source_line.as_deref(),
            Some(r#"        emit("coin_collected")"#)
        );
    }

    #[test]
    fn rejects_spec_api_that_runtime_does_not_execute() {
        let diagnostics = diagnose_source(
            "scripts/player.varg",
            r#"script Player {
    @export var jumpForce: Float = 8.0

    func update(_ dt: Float) {
        entity.velocity.y = jumpForce
    }
}
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.code, "VARG4100");
        assert_eq!(diagnostic.line, Some(5));
        assert_eq!(diagnostic.column, Some(9));
        assert!(diagnostic.message.contains("entity.velocity"));
        assert!(diagnostic.suggestion.contains("position.y"));
    }

    #[test]
    fn compile_rejects_unsupported_runtime_statement() {
        let (script, diagnostics) = compile_script_source(
            "scripts/player.varg",
            r#"script Player {
    func update(_ dt: Float) {
        spawnEnemy()
    }
}
"#,
        );

        assert!(script.is_none());
        assert_eq!(diagnostics[0].code, "VARG4100");
        assert!(diagnostics[0].blocking);
        assert!(
            diagnostics[0]
                .suggestion
                .contains("supported MVP script API")
        );
    }

    #[test]
    fn rejects_unsupported_condition_calls() {
        let diagnostics = diagnose_source(
            "scripts/player.varg",
            r#"script Player {
    func update(_ dt: Float) {
        if target.has(Health) {
            log("hit")
        }
    }
}
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "VARG4100");
        assert!(diagnostics[0].message.contains("if"));
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
            scene: VargSceneContext::default(),
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
            scene: VargSceneContext::default(),
        };
        let output = script.run_hook("update", context);
        assert_eq!(
            output.state.get("dead").and_then(|value| value.as_f64()),
            Some(1.0)
        );
    }

    #[test]
    fn runtime_emits_ui_draw_commands() {
        let (script, diagnostics) = compile_script_source(
            "scripts/hud.varg",
            r#"script Hud {
    func update(_ dt: Float) {
        ui.rect("health_bg", 12.0, 16.0, 120.0, 10.0, 0.1, 0.1, 0.1, 0.8)
        ui.label("score", "Score: 10", 12.0, 32.0)
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let output = script.unwrap().run_hook(
            "update",
            VargRuntimeContext {
                transform: Transform::default(),
                input: engine_platform::InputState::default(),
                delta_time: 0.016,
                total_time: 1.0,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        assert_eq!(
            output.ui_commands,
            vec![
                VargUiCommand::Rect {
                    id: "health_bg".to_string(),
                    x: 12.0,
                    y: 16.0,
                    width: 120.0,
                    height: 10.0,
                    color: [0.1, 0.1, 0.1, 0.8],
                },
                VargUiCommand::Label {
                    id: "score".to_string(),
                    text: "Score: 10".to_string(),
                    x: 12.0,
                    y: 32.0,
                },
            ]
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
                scene: VargSceneContext::default(),
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
                scene: VargSceneContext::default(),
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
                scene: VargSceneContext::default(),
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
    fn runtime_supports_preferred_explicit_input_and_bool_state() {
        let (script, diagnostics) = compile_script_source(
            "scripts/preferred_input.varg",
            r#"script PreferredInput {
    var canFire: Bool = true
    var fired: Int = 0
    var released: Int = 0

    func update(_ dt: Float) {
        let moveX: Float = Input.value("MoveX")
        position.x = moveX

        if Input.pressed("Fire") && canFire {
            fired += 1
            canFire = false
        }

        if Input.released("Fire") {
            released += 1
            canFire = true
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
                scene: VargSceneContext::default(),
            },
        );
        assert_eq!(
            output.state.get("fired").and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert_eq!(
            output
                .state
                .get("canFire")
                .and_then(|value| value.as_bool()),
            Some(false)
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
                transform: output.transform,
                input,
                delta_time: 0.016,
                total_time: 0.032,
                frame_index: 2,
                exported_values: HashMap::new(),
                state: output.state,
                scene: VargSceneContext::default(),
            },
        );
        assert_eq!(
            output
                .state
                .get("released")
                .and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert_eq!(
            output
                .state
                .get("canFire")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn runtime_supports_migrated_declarative_entity_queries_and_destroy() {
        let (script, diagnostics) = compile_script_source(
            "scripts/hazard.varg",
            r#"script Hazard {
    func update(_ dt: Float) {
        if entity.hasTag("Enemy") && scene.distanceTo("Player") <= 5.0 {
            state.nearPlayer = 1
        }

        if playerDistance() <= 5.0 {
            state.playerDistanceMatched = 1
        }

        if scene.distanceToTag("Treasure") < 3.0 {
            entity.destroy()
        }

        state.afterDestroy = 1
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let script = script.unwrap();
        let mut scene = VargSceneContext {
            entity_name: "EnemyA".to_string(),
            entity_tag: "Enemy".to_string(),
            ..VargSceneContext::default()
        };
        scene
            .positions_by_name
            .insert("Player".to_string(), Vec3::new(3.0, 0.0, 4.0));
        scene
            .positions_by_tag
            .insert("Player".to_string(), vec![Vec3::new(3.0, 0.0, 4.0)]);
        scene
            .positions_by_tag
            .insert("Treasure".to_string(), vec![Vec3::new(1.0, 0.0, 0.0)]);

        let output = script.run_hook(
            "update",
            VargRuntimeContext {
                transform: Transform::default(),
                input: engine_platform::InputState::default(),
                delta_time: 0.016,
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene,
            },
        );

        assert_eq!(
            output
                .state
                .get("nearPlayer")
                .and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert_eq!(
            output
                .state
                .get("playerDistanceMatched")
                .and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert!(output.destroy_self);
        assert!(!output.state.contains_key("afterDestroy"));
    }

    #[test]
    fn compiles_behavior_declaration_to_varg_behavior_ir() {
        let (behavior, diagnostics) = compile_behavior_source(
            "scripts/enemy_ai.varg",
            r#"behavior EnemyAI {
    selector {
        sequence "chase branch" {
            when playerDistance() < 10
            action chase("Player", speed: 4.0)
        }

        repeat 3 {
            action patrol(points: ["A", "B", "C"], speed: 2.0)
        }
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let behavior = behavior.expect("behavior should compile");
        assert_eq!(behavior.name, "EnemyAI");
        let VargBehaviorNode::Selector { children, .. } = behavior.root else {
            panic!("expected selector root");
        };
        assert_eq!(children.len(), 2);
        match &children[0] {
            VargBehaviorNode::Sequence { name, children } => {
                assert_eq!(name.as_deref(), Some("chase branch"));
                assert_eq!(
                    children,
                    &vec![
                        VargBehaviorNode::Condition {
                            expression: "playerDistance() < 10".to_string()
                        },
                        VargBehaviorNode::Action {
                            expression: "chase(\"Player\", speed: 4.0)".to_string()
                        }
                    ]
                );
            }
            other => panic!("expected sequence, got {other:#?}"),
        }
        match &children[1] {
            VargBehaviorNode::Repeat { count, child } => {
                assert_eq!(*count, Some(3));
                assert_eq!(
                    **child,
                    VargBehaviorNode::Action {
                        expression: "patrol(points: [\"A\", \"B\", \"C\"], speed: 2.0)".to_string()
                    }
                );
            }
            other => panic!("expected repeat, got {other:#?}"),
        }
    }

    #[test]
    fn compiles_behavior_decorators() {
        let (behavior, diagnostics) = compile_behavior_source(
            "scripts/decorators.varg",
            r#"behavior Decorators {
    sequence {
        invert {
            when entity.hasTag("Frozen")
        }
        succeed {
            action idle()
        }
        repeat forever {
            action wait(1.0)
        }
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let behavior = behavior.unwrap();
        let VargBehaviorNode::Sequence { children, .. } = behavior.root else {
            panic!("expected sequence root");
        };
        assert!(matches!(children[0], VargBehaviorNode::Invert { .. }));
        assert!(matches!(children[1], VargBehaviorNode::Succeed { .. }));
        assert!(matches!(
            children[2],
            VargBehaviorNode::Repeat { count: None, .. }
        ));
    }

    #[test]
    fn rejects_empty_behavior_declaration() {
        let (behavior, diagnostics) = compile_behavior_source(
            "scripts/empty.varg",
            r#"behavior Empty {
}
"#,
        );

        assert!(behavior.is_none());
        assert_eq!(diagnostics[0].code, "VARG5003");
    }

    #[test]
    fn rejects_decorator_with_multiple_children() {
        let (behavior, diagnostics) = compile_behavior_source(
            "scripts/bad.varg",
            r#"behavior Bad {
    invert {
        when entity.hasTag("Frozen")
        action idle()
    }
}
"#,
        );

        assert!(behavior.is_none());
        assert_eq!(diagnostics[0].code, "VARG5005");
    }

    #[test]
    fn checked_in_examples_compile() {
        for (path, source) in [
            (
                "examples/scripts/loop_demo.varg",
                include_str!("../../../examples/scripts/loop_demo.varg"),
            ),
            (
                "examples/scripts/particle_system.varg",
                include_str!("../../../examples/scripts/particle_system.varg"),
            ),
            (
                "examples/scripts/timed_sequence.varg",
                include_str!("../../../examples/scripts/timed_sequence.varg"),
            ),
            (
                "examples/scripts/wave_spawner.varg",
                include_str!("../../../examples/scripts/wave_spawner.varg"),
            ),
            (
                "examples/scripts/weapon_cooldown.varg",
                include_str!("../../../examples/scripts/weapon_cooldown.varg"),
            ),
            (
                "examples/project/scripts/player_controller.varg",
                include_str!("../../../examples/project/scripts/player_controller.varg"),
            ),
        ] {
            let (script, diagnostics) = compile_script_source(path, source);
            assert!(script.is_some(), "{path} did not compile: {diagnostics:#?}");
            assert!(
                diagnostics.is_empty(),
                "{path} diagnostics: {diagnostics:#?}"
            );
        }
    }

    #[test]
    fn runtime_supports_for_loops_with_range() {
        let (script, diagnostics) = compile_script_source(
            "scripts/counter.varg",
            r#"script Counter {
    var sum: Int = 0

    func update(_ dt: Float) {
        for i in 1..5 {
            state.sum += i
        }
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
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        // 1 + 2 + 3 + 4 = 10
        assert_eq!(
            output.state.get("sum").and_then(|value| value.as_f64()),
            Some(10.0)
        );
    }

    #[test]
    fn runtime_supports_for_loops_with_inclusive_range() {
        let (script, diagnostics) = compile_script_source(
            "scripts/counter.varg",
            r#"script Counter {
    var sum: Int = 0

    func update(_ dt: Float) {
        for i in 1..=5 {
            state.sum += i
        }
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
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        // 1 + 2 + 3 + 4 + 5 = 15
        assert_eq!(
            output.state.get("sum").and_then(|value| value.as_f64()),
            Some(15.0)
        );
    }

    #[test]
    fn runtime_supports_for_loops_with_count() {
        let (script, diagnostics) = compile_script_source(
            "scripts/spawner.varg",
            r#"script Spawner {
    var count: Int = 0

    func update(_ dt: Float) {
        for i in count(3) {
            state.count += 1
        }
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
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        assert_eq!(
            output.state.get("count").and_then(|value| value.as_f64()),
            Some(3.0)
        );
    }

    #[test]
    fn runtime_supports_while_loops() {
        let (script, diagnostics) = compile_script_source(
            "scripts/countdown.varg",
            r#"script Countdown {
    var counter: Int = 5

    func update(_ dt: Float) {
        while state.counter > 0 {
            state.counter -= 1
        }
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
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        assert_eq!(
            output.state.get("counter").and_then(|value| value.as_f64()),
            Some(0.0)
        );
    }

    #[test]
    fn runtime_supports_break_in_loops() {
        let (script, diagnostics) = compile_script_source(
            "scripts/breaker.varg",
            r#"script Breaker {
    var sum: Int = 0

    func update(_ dt: Float) {
        for i in 0..10 {
            if i >= 5 {
                break
            }
            state.sum += i
        }
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
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        // 0 + 1 + 2 + 3 + 4 = 10
        assert_eq!(
            output.state.get("sum").and_then(|value| value.as_f64()),
            Some(10.0)
        );
    }

    #[test]
    fn runtime_supports_continue_in_loops() {
        let (script, diagnostics) = compile_script_source(
            "scripts/skipper.varg",
            r#"script Skipper {
    var sum: Int = 0

    func update(_ dt: Float) {
        for i in 0..10 {
            if i == 2 || i == 5 {
                continue
            }
            state.sum += i
        }
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
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        // 0 + 1 + 3 + 4 + 6 + 7 + 8 + 9 = 38
        assert_eq!(
            output.state.get("sum").and_then(|value| value.as_f64()),
            Some(38.0)
        );
    }

    #[test]
    fn runtime_supports_return_early() {
        let (script, diagnostics) = compile_script_source(
            "scripts/early_exit.varg",
            r#"script EarlyExit {
    var executed: Int = 0

    func update(_ dt: Float) {
        state.executed = 1
        if state.executed == 1 {
            return
        }
        state.executed = 2
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
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        assert_eq!(
            output
                .state
                .get("executed")
                .and_then(|value| value.as_f64()),
            Some(1.0)
        );
    }

    #[test]
    fn runtime_supports_nested_loops() {
        let (script, diagnostics) = compile_script_source(
            "scripts/nested.varg",
            r#"script Nested {
    var sum: Int = 0

    func update(_ dt: Float) {
        for i in 0..3 {
            for j in 0..2 {
                state.sum += i + j
            }
        }
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
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        // (0+0) + (0+1) + (1+0) + (1+1) + (2+0) + (2+1) = 0 + 1 + 1 + 2 + 2 + 3 = 9
        assert_eq!(
            output.state.get("sum").and_then(|value| value.as_f64()),
            Some(9.0)
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
                scene: VargSceneContext::default(),
            },
        );

        assert!((output.transform.translation.y - 1.0).abs() < 0.0001);
        assert_eq!(
            output.state.get("frame").and_then(|value| value.as_f64()),
            Some(7.0)
        );
    }

    #[test]
    fn runtime_supports_wait_for_simple_delays() {
        let (script, diagnostics) = compile_script_source(
            "scripts/delayed.varg",
            r#"script Delayed {
    func update(_ dt: Float) {
        if state.executed == 1 {
            return
        }
        wait(1.0)
        state.executed = 1
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let script = script.unwrap();

        // First frame: wait starts, executed should not be set
        let output = script.run_hook(
            "update",
            VargRuntimeContext {
                transform: Transform::default(),
                input: engine_platform::InputState::default(),
                delta_time: 0.016,
                total_time: 0.016,
                frame_index: 1,
                exported_values: HashMap::new(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        // Wait timer should be created, but executed should not be 1
        assert!(output.state.get("__wait_timer").is_some());
        assert_ne!(
            output.state.get("executed").and_then(|v| v.as_f64()),
            Some(1.0)
        );

        // Simulate frames during wait (0.5 seconds passed)
        let mut state = output.state;
        for _ in 0..30 {
            let output = script.run_hook(
                "update",
                VargRuntimeContext {
                    transform: Transform::default(),
                    input: engine_platform::InputState::default(),
                    delta_time: 0.016,
                    total_time: 0.5,
                    frame_index: 30,
                    exported_values: HashMap::new(),
                    state: state.clone(),
                    scene: VargSceneContext::default(),
                },
            );
            state = output.state;
        }

        // Still waiting
        assert_ne!(
            state.get("executed").and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert!(state.get("__wait_timer").is_some());

        // Simulate more frames (total > 1.0 second)
        for _ in 0..40 {
            let output = script.run_hook(
                "update",
                VargRuntimeContext {
                    transform: Transform::default(),
                    input: engine_platform::InputState::default(),
                    delta_time: 0.016,
                    total_time: 1.2,
                    frame_index: 70,
                    exported_values: HashMap::new(),
                    state: state.clone(),
                    scene: VargSceneContext::default(),
                },
            );
            state = output.state;
        }

        // Wait finished, code after wait executed
        assert_eq!(
            state.get("executed").and_then(|value| value.as_f64()),
            Some(1.0)
        );
        assert!(state.get("__wait_timer").is_none());
    }

    #[test]
    fn runtime_supports_wait_with_expressions() {
        let (script, diagnostics) = compile_script_source(
            "scripts/dynamic_wait.varg",
            r#"script DynamicWait {
    @export var cooldown: Float = 0.5

    func update(_ dt: Float) {
        if state.fired == 1 {
            return
        }
        wait(cooldown)
        state.fired = 1
    }
}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let script = script.unwrap();

        let mut exported = HashMap::new();
        exported.insert("cooldown".to_string(), serde_json::Value::from(0.5));

        // First frame
        let output = script.run_hook(
            "update",
            VargRuntimeContext {
                transform: Transform::default(),
                input: engine_platform::InputState::default(),
                delta_time: 0.016,
                total_time: 0.016,
                frame_index: 1,
                exported_values: exported.clone(),
                state: HashMap::new(),
                scene: VargSceneContext::default(),
            },
        );

        // Count should not be set yet
        assert_ne!(
            output.state.get("fired").and_then(|v| v.as_f64()),
            Some(1.0)
        );

        // Simulate 0.5 seconds of frames
        let mut state = output.state;
        for _ in 0..32 {
            let output = script.run_hook(
                "update",
                VargRuntimeContext {
                    transform: Transform::default(),
                    input: engine_platform::InputState::default(),
                    delta_time: 0.016,
                    total_time: 0.5,
                    frame_index: 32,
                    exported_values: exported.clone(),
                    state: state.clone(),
                    scene: VargSceneContext::default(),
                },
            );
            state = output.state;
        }

        // After 0.5 seconds, fired should be set
        assert_eq!(
            state.get("fired").and_then(|value| value.as_f64()),
            Some(1.0)
        );
    }
}

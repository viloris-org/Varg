//! Intermediate representation for shaders, backend-agnostic.

use std::collections::HashMap;

use engine_core::EngineResult;

/// Shader intermediate representation.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderIR {
    /// Shader name.
    pub name: String,
    /// Uniform bindings.
    pub uniforms: Vec<UniformDesc>,
    /// Varying inputs/outputs.
    pub varyings: Vec<VaryingDesc>,
    /// Functions defined in the shader.
    pub functions: Vec<FunctionIR>,
    /// Entry point function name.
    pub entry_point: String,
}

/// Description of a uniform binding.
#[derive(Clone, Debug, PartialEq)]
pub struct UniformDesc {
    /// Uniform name.
    pub name: String,
    /// Uniform type.
    pub uniform_type: UniformType,
    /// Binding location.
    pub binding: u32,
    /// Descriptor set / group index.
    pub set: u32,
}

/// Uniform types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UniformType {
    /// f32 scalar.
    Float,
    /// vec2<f32>.
    Vec2,
    /// vec3<f32>.
    Vec3,
    /// vec4<f32>.
    Vec4,
    /// mat4x4<f32>.
    Mat4,
    /// i32 scalar.
    Int,
    /// 2D texture.
    Texture2D,
    /// Cube texture.
    TextureCube,
    /// Sampler.
    Sampler,
}

/// Description of a varying input/output.
#[derive(Clone, Debug, PartialEq)]
pub struct VaryingDesc {
    /// Varying name.
    pub name: String,
    /// Varying type.
    pub varying_type: String,
    /// Interpolation mode.
    pub interpolation: Interpolation,
}

/// Interpolation mode for varyings.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Interpolation {
    /// Perspective-correct interpolation.
    #[default]
    Perspective,
    /// Flat (no interpolation).
    Flat,
    /// Linear interpolation.
    Linear,
}

/// A function in the IR.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionIR {
    /// Function name.
    pub name: String,
    /// Return type.
    pub return_type: String,
    /// Parameter names and types.
    pub params: Vec<(String, String)>,
    /// Function body as IR operations.
    pub body: Vec<OpIR>,
}

/// IR operation.
#[derive(Clone, Debug, PartialEq)]
pub enum OpIR {
    /// Load from a uniform.
    LoadUniform {
        /// Destination variable.
        dest: String,
        /// Uniform index.
        uniform: usize,
        /// Optional field name.
        field: Option<String>,
    },
    /// Load from a varying.
    LoadVarying {
        /// Destination variable.
        dest: String,
        /// Varying index.
        varying: usize,
    },
    /// Store to output.
    StoreOutput {
        /// Source variable.
        src: String,
        /// Output name.
        output: String,
    },
    /// Binary operation.
    Binary {
        /// Destination variable.
        dest: String,
        /// Operator.
        op: String,
        /// Left operand variable.
        left: String,
        /// Right operand variable or literal.
        right: String,
    },
    /// Unary operation.
    Unary {
        /// Destination variable.
        dest: String,
        /// Operator.
        op: String,
        /// Source variable.
        src: String,
    },
    /// Texture sample.
    TextureSample {
        /// Destination variable.
        dest: String,
        /// Texture uniform index.
        texture: usize,
        /// Sampler uniform index.
        sampler: usize,
        /// UV coordinate variable.
        uv: String,
    },
    /// Function call.
    Call {
        /// Destination variable.
        dest: Option<String>,
        /// Function name.
        name: String,
        /// Arguments as variable names.
        args: Vec<String>,
    },
    /// Return from function.
    Return(Option<String>),
    /// Set a variable to a literal value.
    SetLiteral {
        /// Destination variable.
        dest: String,
        /// Literal value.
        value: String,
    },
}

/// Validates the IR and checks for common issues.
pub fn validate(ir: &ShaderIR) -> EngineResult<()> {
    let mut binding_set = HashMap::new();
    for uniform in &ir.uniforms {
        let key = (uniform.set, uniform.binding);
        if binding_set.contains_key(&key) {
            return Err(engine_core::EngineError::other(format!(
                "duplicate uniform binding: set={}, binding={}",
                uniform.set, uniform.binding
            )));
        }
        binding_set.insert(key, &uniform.name);
    }

    let has_entry = ir
        .functions
        .iter()
        .any(|f| f.name == ir.entry_point);
    if !has_entry {
        return Err(engine_core::EngineError::other(format!(
            "entry point '{}' not found",
            ir.entry_point
        )));
    }

    Ok(())
}

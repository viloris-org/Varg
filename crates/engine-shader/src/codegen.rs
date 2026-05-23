//! Shader code generation backends.

use engine_core::EngineResult;

use crate::ir::{FunctionIR, OpIR, ShaderIR, UniformType};

/// Trait for shader code generation backends.
pub trait ShaderBackend {
    /// Generates source code from IR.
    fn generate(&self, ir: &ShaderIR) -> EngineResult<String>;
}

/// WGSL code generation backend.
#[derive(Default)]
pub struct WgslCodegen;

impl ShaderBackend for WgslCodegen {
    fn generate(&self, ir: &ShaderIR) -> EngineResult<String> {
        let mut output = String::new();

        for uniform in &ir.uniforms {
            let wgsl_type = uniform_type_wgsl(uniform.uniform_type);
            output.push_str(&format!(
                "@group({}) @binding({}) var<uniform> {}: {};\n",
                uniform.set, uniform.binding, uniform.name, wgsl_type
            ));
        }

        for varying in &ir.varyings {
            output.push_str(&format!(
                "var<private> {}: {};\n",
                varying.name, varying.varying_type
            ));
        }

        for function in &ir.functions {
            output.push_str(&generate_function_wgsl(function));
            output.push('\n');
        }

        Ok(output)
    }
}

fn uniform_type_wgsl(ty: UniformType) -> &'static str {
    match ty {
        UniformType::Float => "f32",
        UniformType::Vec2 => "vec2<f32>",
        UniformType::Vec3 => "vec3<f32>",
        UniformType::Vec4 => "vec4<f32>",
        UniformType::Mat4 => "mat4x4<f32>",
        UniformType::Int => "i32",
        UniformType::Texture2D => "texture_2d<f32>",
        UniformType::TextureCube => "texture_cube<f32>",
        UniformType::Sampler => "sampler",
    }
}

fn generate_function_wgsl(func: &FunctionIR) -> String {
    let params = func
        .params
        .iter()
        .map(|(name, ty)| format!("{}: {}", name, ty))
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = format!(
        "fn {}({}) -> {} {{\n",
        func.name, params, func.return_type
    );

    let mut vars: Vec<String> = Vec::new();
    for op in &func.body {
        match op {
            OpIR::SetLiteral { dest, value } => {
                out.push_str(&format!("    let {} = {};\n", dest, value));
                vars.push(dest.clone());
            }
            OpIR::Binary {
                dest,
                op,
                left,
                right,
            } => {
                out.push_str(&format!(
                    "    let {} = {} {} {};\n",
                    dest, left, op, right
                ));
                vars.push(dest.clone());
            }
            OpIR::Unary { dest, op, src } => {
                out.push_str(&format!("    let {} = {}{};\n", dest, op, src));
                vars.push(dest.clone());
            }
            OpIR::Call { dest, name, args } => {
                let args_str = args.join(", ");
                if let Some(d) = dest {
                    out.push_str(&format!(
                        "    let {} = {}({});\n",
                        d, name, args_str
                    ));
                    vars.push(d.clone());
                } else {
                    out.push_str(&format!("    {}({});\n", name, args_str));
                }
            }
            OpIR::Return(val) => {
                if let Some(v) = val {
                    out.push_str(&format!("    return {};\n", v));
                } else {
                    out.push_str("    return;\n");
                }
            }
            OpIR::LoadUniform {
                dest,
                uniform,
                field,
            } => {
                let uniform_name = format!("u{}", uniform);
                if let Some(f) = field {
                    out.push_str(&format!(
                        "    let {} = {}.{};\n",
                        dest, uniform_name, f
                    ));
                } else {
                    out.push_str(&format!(
                        "    let {} = {};\n",
                        dest, uniform_name
                    ));
                }
                vars.push(dest.clone());
            }
            OpIR::LoadVarying { dest, varying: _ } => {
                out.push_str(&format!("    // varying load: {}\n", dest));
            }
            OpIR::StoreOutput { src, output } => {
                out.push_str(&format!("    // store {} to {}\n", src, output));
            }
            OpIR::TextureSample {
                dest,
                texture: _,
                sampler: _,
                uv,
            } => {
                out.push_str(&format!(
                    "    let {} = textureSample(t, s, {});\n",
                    dest, uv
                ));
            }
        }
    }
    out.push_str("}\n");
    out
}

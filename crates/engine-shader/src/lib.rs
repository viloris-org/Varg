#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Shader compiler and material system.
//!
//! Provides a shader processing pipeline (tokenize → parse → IR → codegen),
//! a StandardMaterial3D with PBR parameters, and a material instance system.

pub mod codegen;
pub mod ir;
pub mod material;
pub mod material_instance;
pub mod parser;
pub mod tokenizer;

pub use codegen::{ShaderBackend, WgslCodegen};
pub use ir::{
    FunctionIR, OpIR, ShaderIR, UniformDesc, UniformType, VaryingDesc,
};
pub use material::{AlphaMode, StandardMaterial3D};
pub use material_instance::{MaterialInstance, MaterialParam};

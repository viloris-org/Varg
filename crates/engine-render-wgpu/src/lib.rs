//! WebGPU rendering backend for Varg.

#![deny(missing_docs)]

mod batches;
mod constructors;
mod device;
mod device_trait;
mod diagnostics;
mod format;
mod ibl;
mod lifecycle;
mod light_preparation;
mod math;
mod meshes;
mod particles;
mod passes;
mod post;
mod render;
mod scene_uniforms;
mod shaders;
mod uniforms;

/// Re-exported wgpu API used by window hosts that need to create surfaces.
pub use wgpu;

pub use constructors::WgpuOffscreenConfig;
pub use device::{SurfaceViewportRect, WgpuOutputCapabilities, WgpuRenderDevice};
pub use meshes::{DebugMesh, MeshBuffers};
pub use uniforms::Vertex;

#[cfg(test)]
mod tests;

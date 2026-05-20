#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Render abstraction only. Concrete backends live outside `runtime-min`.

use engine_core::{math::Transform, EngineError, EngineResult, EntityId, Handle};

pub mod graph;
pub mod pipeline;
pub mod resource;
pub mod target;

#[cfg(feature = "editor")]
pub mod egui_convert;

pub use graph::{PassId, RenderGraph, RenderGraphBuilder, RenderPass};
pub use pipeline::{
    GuiDrawList, GuiTextureId, MaterialHandle, PipelineDesc, ShaderHandle, ShaderStage,
};
pub use resource::{
    BufferDesc, BufferHandle, BufferUsage, ImageDesc, ImageFormat, ImageHandle, ImageUsage,
    SamplerDesc, SamplerHandle, TextureCache,
};
pub use target::{RenderTarget, RenderTargetDesc, ViewKind};

/// Render API selected by a concrete backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderApi {
    /// No rendering backend.
    Headless,
    /// Vulkan backend.
    Vulkan,
    /// Metal backend.
    Metal,
    /// Direct3D 12 backend.
    D3D12,
    /// WebGPU backend.
    WebGpu,
}

/// Render frame context passed to backends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderFrame {
    /// Frame index.
    pub frame_index: u64,
}

/// Camera data extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderCamera {
    /// Source scene object.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Vertical field of view in degrees.
    pub vertical_fov_degrees: f32,
    /// Near clipping plane.
    pub near: f32,
    /// Far clipping plane.
    pub far: f32,
}

/// Mesh draw data extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderObject {
    /// Source scene object.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Mesh identifier, either a built-in name or asset label.
    pub mesh: String,
    /// Material identifier, either a built-in name or asset label.
    pub material: String,
}

/// Light data extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderLight {
    /// Source scene object.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Light kind.
    pub kind: String,
    /// Light intensity.
    pub intensity: f32,
}

/// Minimal render queue shared by runtime, editor Scene View, and Game View.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderWorld {
    /// Active camera.
    pub camera: Option<RenderCamera>,
    /// Queued mesh renderers.
    pub objects: Vec<RenderObject>,
    /// Queued lights.
    pub lights: Vec<RenderLight>,
}

impl RenderWorld {
    /// Returns true when there is visible geometry and a camera.
    pub fn is_visible(&self) -> bool {
        self.camera.is_some() && !self.objects.is_empty()
    }
}

/// Render backend abstraction.
pub trait RenderDevice {
    /// Returns the concrete API exposed by this device.
    fn api(&self) -> RenderApi;

    /// Renders one frame using the compiled graph.
    fn render(&mut self, frame: RenderFrame) -> EngineResult<()>;

    /// Submits a scene extraction to the backend for rendering.
    fn submit_render_world(&mut self, world: &RenderWorld, frame: RenderFrame) -> EngineResult<()> {
        let _ = world;
        self.render(frame)
    }

    /// Executes a compiled render graph.
    fn execute_graph(&mut self, graph: &RenderGraph, frame: RenderFrame) -> EngineResult<()>;

    /// Creates a render target.
    fn create_render_target(&mut self, desc: RenderTargetDesc) -> EngineResult<RenderTarget>;

    /// Destroys a render target, queuing GPU cleanup.
    fn destroy_render_target(&mut self, target: RenderTarget);

    /// Creates a GPU image.
    fn create_image(&mut self, desc: ImageDesc) -> EngineResult<ImageHandle>;

    /// Creates a GPU image and uploads tightly packed pixel data into mip 0.
    fn upload_texture(&mut self, desc: ImageDesc, data: &[u8]) -> EngineResult<ImageHandle> {
        let _ = data;
        self.create_image(desc)
    }

    /// Destroys a GPU image.
    fn destroy_image(&mut self, handle: ImageHandle);

    /// Creates a GPU buffer.
    fn create_buffer(&mut self, desc: BufferDesc) -> EngineResult<BufferHandle>;

    /// Destroys a GPU buffer.
    fn destroy_buffer(&mut self, handle: BufferHandle);

    /// Uploads a GUI texture and returns its backend id.
    fn upload_gui_texture(&mut self, desc: ImageDesc, data: &[u8]) -> EngineResult<GuiTextureId>;

    /// Submits a GUI draw list for rendering.
    fn draw_gui(&mut self, draw_list: &GuiDrawList) -> EngineResult<()>;

    /// Flushes the delayed destruction queue for the given frame.
    fn flush_destroy_queue(&mut self, frame_index: u64);
}

/// Null renderer used by minimal runtime builds.
#[derive(Clone, Debug, Default)]
pub struct HeadlessRenderDevice {
    frame_index: u64,
}

impl RenderDevice for HeadlessRenderDevice {
    fn api(&self) -> RenderApi {
        RenderApi::Headless
    }

    fn render(&mut self, frame: RenderFrame) -> EngineResult<()> {
        self.frame_index = frame.frame_index;
        Ok(())
    }

    fn execute_graph(&mut self, _graph: &RenderGraph, frame: RenderFrame) -> EngineResult<()> {
        self.frame_index = frame.frame_index;
        Ok(())
    }

    fn create_render_target(&mut self, desc: RenderTargetDesc) -> EngineResult<RenderTarget> {
        Ok(RenderTarget {
            handle: Handle::new(0, engine_core::Generation::FIRST),
            desc,
        })
    }

    fn destroy_render_target(&mut self, _target: RenderTarget) {}

    fn create_image(&mut self, _desc: ImageDesc) -> EngineResult<ImageHandle> {
        Ok(ImageHandle(Handle::new(0, engine_core::Generation::FIRST)))
    }

    fn upload_texture(&mut self, desc: ImageDesc, _data: &[u8]) -> EngineResult<ImageHandle> {
        self.create_image(desc)
    }

    fn destroy_image(&mut self, _handle: ImageHandle) {}

    fn create_buffer(&mut self, _desc: BufferDesc) -> EngineResult<BufferHandle> {
        Ok(BufferHandle(Handle::new(0, engine_core::Generation::FIRST)))
    }

    fn destroy_buffer(&mut self, _handle: BufferHandle) {}

    fn upload_gui_texture(&mut self, _desc: ImageDesc, _data: &[u8]) -> EngineResult<GuiTextureId> {
        Ok(GuiTextureId(0))
    }

    fn draw_gui(&mut self, _draw_list: &GuiDrawList) -> EngineResult<()> {
        Ok(())
    }

    fn flush_destroy_queue(&mut self, _frame_index: u64) {}
}

/// Placeholder for profiles that request a concrete backend before one is linked.
#[derive(Clone, Debug, Default)]
pub struct MissingRenderDevice;

impl RenderDevice for MissingRenderDevice {
    fn api(&self) -> RenderApi {
        RenderApi::Headless
    }

    fn render(&mut self, _frame: RenderFrame) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn execute_graph(&mut self, _graph: &RenderGraph, _frame: RenderFrame) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn create_render_target(&mut self, _desc: RenderTargetDesc) -> EngineResult<RenderTarget> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn destroy_render_target(&mut self, _target: RenderTarget) {}

    fn create_image(&mut self, _desc: ImageDesc) -> EngineResult<ImageHandle> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn upload_texture(&mut self, _desc: ImageDesc, _data: &[u8]) -> EngineResult<ImageHandle> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn destroy_image(&mut self, _handle: ImageHandle) {}

    fn create_buffer(&mut self, _desc: BufferDesc) -> EngineResult<BufferHandle> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn destroy_buffer(&mut self, _handle: BufferHandle) {}

    fn upload_gui_texture(&mut self, _desc: ImageDesc, _data: &[u8]) -> EngineResult<GuiTextureId> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn draw_gui(&mut self, _draw_list: &GuiDrawList) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "render-backend",
        })
    }

    fn flush_destroy_queue(&mut self, _frame_index: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_renderer_accepts_frame() {
        let mut renderer = HeadlessRenderDevice::default();
        renderer.render(RenderFrame { frame_index: 0 }).unwrap();
        assert_eq!(renderer.api(), RenderApi::Headless);
    }

    #[test]
    fn headless_executes_empty_graph() {
        let mut renderer = HeadlessRenderDevice::default();
        let graph = RenderGraphBuilder::new().build();
        renderer
            .execute_graph(&graph, RenderFrame { frame_index: 1 })
            .unwrap();
    }
}

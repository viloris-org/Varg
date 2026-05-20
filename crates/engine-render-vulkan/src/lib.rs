//! Vulkan rendering backend for Aster.
//!
//! Enable the `vulkan` feature to compile the real backend.
//! Without it, only the stub types are available so the crate compiles
//! in all profiles without requiring Vulkan headers.

#![deny(missing_docs)]

use engine_core::EngineResult;
use engine_render::{
    BufferDesc, BufferHandle, GuiDrawList, GuiTextureId, ImageDesc, RenderApi, RenderDevice,
    RenderFrame, RenderGraph, RenderTarget, RenderTargetDesc,
};

#[cfg(feature = "vulkan")]
mod vk;

#[cfg(feature = "vulkan")]
pub use vk::VulkanRenderDevice;

/// Stub returned when the `vulkan` feature is disabled.
///
/// All methods return [`engine_core::EngineError::UnsupportedCapability`].
#[derive(Debug, Default)]
pub struct VulkanStub;

impl RenderDevice for VulkanStub {
    fn api(&self) -> RenderApi {
        RenderApi::Vulkan
    }

    fn render(&mut self, _frame: RenderFrame) -> EngineResult<()> {
        Err(engine_core::EngineError::UnsupportedCapability {
            capability: "vulkan",
        })
    }

    fn execute_graph(&mut self, _graph: &RenderGraph, _frame: RenderFrame) -> EngineResult<()> {
        Err(engine_core::EngineError::UnsupportedCapability {
            capability: "vulkan",
        })
    }

    fn create_render_target(&mut self, _desc: RenderTargetDesc) -> EngineResult<RenderTarget> {
        Err(engine_core::EngineError::UnsupportedCapability {
            capability: "vulkan",
        })
    }

    fn destroy_render_target(&mut self, _target: RenderTarget) {}

    fn create_image(&mut self, _desc: ImageDesc) -> EngineResult<engine_render::ImageHandle> {
        Err(engine_core::EngineError::UnsupportedCapability {
            capability: "vulkan",
        })
    }

    fn upload_texture(
        &mut self,
        _desc: ImageDesc,
        _data: &[u8],
    ) -> EngineResult<engine_render::ImageHandle> {
        Err(engine_core::EngineError::UnsupportedCapability {
            capability: "vulkan",
        })
    }

    fn destroy_image(&mut self, _handle: engine_render::ImageHandle) {}

    fn create_buffer(&mut self, _desc: BufferDesc) -> EngineResult<BufferHandle> {
        Err(engine_core::EngineError::UnsupportedCapability {
            capability: "vulkan",
        })
    }

    fn destroy_buffer(&mut self, _handle: BufferHandle) {}

    fn upload_gui_texture(&mut self, _desc: ImageDesc, _data: &[u8]) -> EngineResult<GuiTextureId> {
        Err(engine_core::EngineError::UnsupportedCapability {
            capability: "vulkan",
        })
    }

    fn draw_gui(&mut self, _draw_list: &GuiDrawList) -> EngineResult<()> {
        Err(engine_core::EngineError::UnsupportedCapability {
            capability: "vulkan",
        })
    }

    fn flush_destroy_queue(&mut self, _frame_index: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_reports_vulkan_api() {
        assert_eq!(VulkanStub.api(), RenderApi::Vulkan);
    }

    #[test]
    fn stub_render_returns_unsupported() {
        let mut stub = VulkanStub;
        let err = stub.render(RenderFrame { frame_index: 0 }).unwrap_err();
        assert!(err.to_string().contains("vulkan"));
    }
}

//! Offscreen render targets for Scene View and Game View.

use engine_core::Handle;

use crate::resource::ImageFormat;

/// Identifies the purpose of a render target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewKind {
    /// Editor scene view.
    SceneView,
    /// Game camera view.
    GameView,
    /// Shadow map.
    Shadow,
    /// Post-processing intermediate.
    PostProcess,
    /// Material/mesh preview.
    Preview,
}

/// Render target creation descriptor.
#[derive(Clone, Debug)]
pub struct RenderTargetDesc {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
    /// Internal rendering width in pixels.
    pub internal_width: u32,
    /// Internal rendering height in pixels.
    pub internal_height: u32,
    /// UI composition width in pixels.
    pub ui_width: u32,
    /// UI composition height in pixels.
    pub ui_height: u32,
    /// Color attachment format.
    pub color_format: ImageFormat,
    /// Whether a depth attachment is needed.
    pub with_depth: bool,
    /// MSAA sample count.
    pub samples: u32,
    /// View kind.
    pub kind: ViewKind,
    /// Debug label.
    pub label: Option<&'static str>,
}

impl RenderTargetDesc {
    /// Creates a standard scene/game view descriptor.
    pub fn view(width: u32, height: u32, kind: ViewKind) -> Self {
        Self {
            width,
            height,
            internal_width: width,
            internal_height: height,
            ui_width: width,
            ui_height: height,
            color_format: ImageFormat::Rgba8Srgb,
            with_depth: true,
            samples: 1,
            kind,
            label: None,
        }
    }

    /// Sets internal render dimensions while preserving output and UI dimensions.
    pub fn with_internal_size(mut self, width: u32, height: u32) -> Self {
        self.internal_width = width.max(1);
        self.internal_height = height.max(1);
        self
    }

    /// Sets UI composition dimensions independently from render dimensions.
    pub fn with_ui_size(mut self, width: u32, height: u32) -> Self {
        self.ui_width = width.max(1);
        self.ui_height = height.max(1);
        self
    }

    /// Returns internal rendering dimensions.
    pub fn internal_size(&self) -> (u32, u32) {
        (self.internal_width.max(1), self.internal_height.max(1))
    }

    /// Returns output dimensions.
    pub fn output_size(&self) -> (u32, u32) {
        (self.width.max(1), self.height.max(1))
    }

    /// Returns UI composition dimensions.
    pub fn ui_size(&self) -> (u32, u32) {
        (self.ui_width.max(1), self.ui_height.max(1))
    }
}

/// A live render target backed by GPU resources.
#[derive(Debug)]
pub struct RenderTarget {
    /// Opaque backend handle.
    pub handle: Handle,
    /// Creation descriptor.
    pub desc: RenderTargetDesc,
}

impl RenderTarget {
    /// Returns the view kind.
    pub fn kind(&self) -> ViewKind {
        self.desc.kind
    }

    /// Returns output pixel dimensions.
    pub fn size(&self) -> (u32, u32) {
        self.desc.output_size()
    }

    /// Returns internal rendering dimensions.
    pub fn internal_size(&self) -> (u32, u32) {
        self.desc.internal_size()
    }

    /// Returns UI composition dimensions.
    pub fn ui_size(&self) -> (u32, u32) {
        self.desc.ui_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_tracks_internal_output_and_ui_sizes_independently() {
        let desc = RenderTargetDesc::view(1920, 1080, ViewKind::GameView)
            .with_internal_size(1280, 720)
            .with_ui_size(2560, 1440);
        assert_eq!(desc.internal_size(), (1280, 720));
        assert_eq!(desc.output_size(), (1920, 1080));
        assert_eq!(desc.ui_size(), (2560, 1440));
    }
}

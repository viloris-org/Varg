//! Editor render integration: Scene View and Game View offscreen targets,
//! GUI draw list submission, and preview render requests.

use std::collections::HashMap;

use engine_core::{EngineError, EngineResult};
use engine_render::{
    GuiDrawList, RenderDevice, RenderFrame, RenderGraph, RenderGraphBuilder, RenderTarget,
    RenderTargetDesc, RenderWorld, ViewKind,
};

/// Manages the two editor offscreen render targets and the GUI draw list.
pub struct EditorRenderer<R: RenderDevice> {
    device: R,
    scene_view: Option<RenderTarget>,
    game_view: Option<RenderTarget>,
    render_graph: RenderGraph,
}

impl<R: RenderDevice> EditorRenderer<R> {
    /// Creates an editor renderer wrapping the given device.
    pub fn new(device: R) -> Self {
        let render_graph = build_editor_render_graph();
        Self {
            device,
            scene_view: None,
            game_view: None,
            render_graph,
        }
    }

    /// Allocates or resizes the Scene View render target.
    pub fn resize_scene_view(&mut self, width: u32, height: u32) -> EngineResult<()> {
        if let Some(old) = self.scene_view.take() {
            self.device.destroy_render_target(old);
        }
        let target = self.device.create_render_target(RenderTargetDesc::view(
            width,
            height,
            ViewKind::SceneView,
        ))?;
        self.scene_view = Some(target);
        Ok(())
    }

    /// Allocates or resizes the Game View render target.
    pub fn resize_game_view(&mut self, width: u32, height: u32) -> EngineResult<()> {
        if let Some(old) = self.game_view.take() {
            self.device.destroy_render_target(old);
        }
        let target = self.device.create_render_target(RenderTargetDesc::view(
            width,
            height,
            ViewKind::GameView,
        ))?;
        self.game_view = Some(target);
        Ok(())
    }

    /// Renders one editor frame: executes the graph then submits the GUI draw list.
    pub fn render_frame(&mut self, frame: RenderFrame, gui: &GuiDrawList) -> EngineResult<()> {
        self.device.execute_graph(&self.render_graph, frame)?;
        self.device.draw_gui(gui)?;
        self.device
            .flush_destroy_queue(frame.frame_index.saturating_sub(2));
        Ok(())
    }

    /// Returns the scene view target, if allocated.
    pub fn scene_view(&self) -> Option<&RenderTarget> {
        self.scene_view.as_ref()
    }

    /// Returns the game view target, if allocated.
    pub fn game_view(&self) -> Option<&RenderTarget> {
        self.game_view.as_ref()
    }

    /// Replaces the active render graph.
    pub fn set_render_graph(&mut self, graph: RenderGraph) {
        self.render_graph = graph;
    }

    /// Returns a reference to the underlying device.
    pub fn device(&self) -> &R {
        &self.device
    }

    /// Returns a mutable reference to the underlying device.
    pub fn device_mut(&mut self) -> &mut R {
        &mut self.device
    }
}

/// Builds the default editor render graph (shadow → gbuffer → deferred-lighting → outline → post → gui).
pub fn build_editor_render_graph() -> RenderGraph {
    let mut builder = RenderGraphBuilder::new();
    let shadow = builder.add_pass("shadow");
    let gbuffer = builder.add_pass("gbuffer");
    let deferred = builder.add_pass("deferred-lighting");
    let outline = builder.add_pass("outline");
    let post = builder.add_pass("post");
    let gui = builder.add_pass("gui");
    builder.order_before(shadow, gbuffer);
    builder.order_before(gbuffer, deferred);
    builder.order_before(deferred, outline);
    builder.order_before(outline, post);
    builder.order_before(post, gui);
    builder.build()
}

/// Wraps a [`RenderDevice`] trait object and manages named offscreen render targets
/// for editor viewports (scene_view, game_view, and previews).
pub struct RenderService {
    device: Box<dyn RenderDevice>,
    targets: HashMap<String, RenderTargetDesc>,
}

impl RenderService {
    /// Creates a new render service around the given device with default
    /// render targets for `scene_view` and `game_view`.
    pub fn new(device: Box<dyn RenderDevice>) -> Self {
        let mut targets = HashMap::new();
        targets.insert(
            "scene_view".to_string(),
            RenderTargetDesc::view(1920, 1080, ViewKind::SceneView),
        );
        targets.insert(
            "game_view".to_string(),
            RenderTargetDesc::view(1920, 1080, ViewKind::GameView),
        );
        Self { device, targets }
    }

    /// Returns the descriptor for the named render target, if it exists.
    pub fn render_target(&self, name: &str) -> Option<&RenderTargetDesc> {
        self.targets.get(name)
    }

    /// Updates the dimensions of a named render target.
    /// The underlying render target resources are recreated on the next
    /// [`render_to_target`] call.
    pub fn resize_target(&mut self, name: &str, width: u32, height: u32) -> EngineResult<()> {
        let desc = self
            .targets
            .get_mut(name)
            .ok_or_else(|| EngineError::other(format!("unknown render target: {name}")))?;
        desc.width = width;
        desc.height = height;
        Ok(())
    }

    /// Renders a [`RenderWorld`] to the named offscreen target.
    pub fn render_to_target(&mut self, name: &str, world: &RenderWorld) -> EngineResult<()> {
        let desc = self
            .targets
            .get(name)
            .ok_or_else(|| EngineError::other(format!("unknown render target: {name}")))?
            .clone();
        let target = self.device.create_render_target(desc)?;
        let render_result = self.device.submit_render_world_to_target(
            world,
            &target,
            RenderFrame { frame_index: 0 },
        );
        self.device.destroy_render_target(target);
        render_result
    }

    /// Returns a reference to the underlying device.
    pub fn device(&self) -> &dyn RenderDevice {
        self.device.as_ref()
    }

    /// Returns a mutable reference to the underlying device.
    pub fn device_mut(&mut self) -> &mut dyn RenderDevice {
        self.device.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_render::HeadlessRenderDevice;

    #[test]
    fn editor_render_graph_has_hybrid_deferred_passes_in_order() {
        let graph = build_editor_render_graph();
        assert_eq!(graph.pass_count(), 6);
        let names: Vec<&str> = graph.passes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "shadow",
                "gbuffer",
                "deferred-lighting",
                "outline",
                "post",
                "gui"
            ]
        );
    }

    #[test]
    fn editor_renderer_allocates_scene_and_game_views() {
        let device = HeadlessRenderDevice::default();
        let mut renderer = EditorRenderer::new(device);
        renderer.resize_scene_view(1280, 720).unwrap();
        renderer.resize_game_view(1280, 720).unwrap();
        assert_eq!(renderer.scene_view().unwrap().kind(), ViewKind::SceneView);
        assert_eq!(renderer.game_view().unwrap().kind(), ViewKind::GameView);
    }

    #[test]
    fn editor_renderer_renders_frame_with_empty_gui() {
        let device = HeadlessRenderDevice::default();
        let mut renderer = EditorRenderer::new(device);
        renderer
            .render_frame(RenderFrame { frame_index: 0 }, &GuiDrawList::default())
            .unwrap();
    }

    // ── RenderService tests ──

    #[test]
    fn render_service_has_default_targets() {
        let device = Box::new(HeadlessRenderDevice::default());
        let service = RenderService::new(device);
        assert!(service.render_target("scene_view").is_some());
        assert!(service.render_target("game_view").is_some());
        assert!(service.render_target("unknown").is_none());
    }

    #[test]
    fn render_service_resize_updates_dimensions() {
        let device = Box::new(HeadlessRenderDevice::default());
        let mut service = RenderService::new(device);
        service.resize_target("scene_view", 800, 600).unwrap();
        let desc = service.render_target("scene_view").unwrap();
        assert_eq!(desc.width, 800);
        assert_eq!(desc.height, 600);
    }

    #[test]
    fn render_service_resize_unknown_target_errors() {
        let device = Box::new(HeadlessRenderDevice::default());
        let mut service = RenderService::new(device);
        assert!(service.resize_target("nope", 100, 100).is_err());
    }

    #[test]
    fn render_service_renders_to_target() {
        let device = Box::new(HeadlessRenderDevice::default());
        let mut service = RenderService::new(device);
        let world = RenderWorld::default();
        service.render_to_target("scene_view", &world).unwrap();
    }

    #[test]
    fn render_service_renders_to_unknown_target_errors() {
        let device = Box::new(HeadlessRenderDevice::default());
        let mut service = RenderService::new(device);
        let world = RenderWorld::default();
        assert!(service.render_to_target("nope", &world).is_err());
    }
}

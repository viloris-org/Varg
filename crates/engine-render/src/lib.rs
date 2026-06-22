#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Render abstraction only. Concrete backends live outside `runtime-min`.

use engine_core::{EngineError, EngineResult, EntityId, Handle, math::Transform};

pub mod graph;
pub mod performance;
pub mod pipeline;
pub mod resource;
pub mod scaling;
pub mod target;
pub mod visibility;

pub use graph::{PassId, RenderGraph, RenderGraphBuilder, RenderPass, RenderStage};
pub use performance::{
    DynamicResolutionConfig, DynamicResolutionController, PresentStrategy, RenderPerformanceConfig,
    RenderPerformanceMetrics,
};
pub use pipeline::{
    GuiDrawCmd, GuiDrawList, GuiTextureId, GuiVertex, MaterialHandle, PipelineDesc, ShaderHandle,
    ShaderStage,
};
pub use resource::{
    BufferDesc, BufferHandle, BufferUsage, ImageDesc, ImageFormat, ImageHandle, ImageUsage,
    SamplerDesc, SamplerHandle, TextureCache,
};
pub use scaling::{
    BatteryPolicy, FrameGenerationCapability, FrameGenerationKind, MobileVendorAdapter,
    RenderPlatformClass, RenderQualityMode, RenderScalingCapabilities, RenderScalingContext,
    RenderScalingSelection, RenderScalingSettings, TemporalCameraData, TemporalFrameState,
    ThermalState, UiCompositionPolicy, UpscalerBackend, UpscalerCapability, UpscalerFrameData,
    UpscalerKind, negotiate_render_scaling,
};
pub use target::{RenderTarget, RenderTargetDesc, ViewKind};
pub use visibility::{RenderBounds, RenderLod, VisibilityResult, select_visibility};

/// Render API selected by a concrete backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderApi {
    /// No rendering backend.
    Headless,
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
    /// Projection used to render this camera.
    pub projection: RenderProjection,
    /// Vertical field of view in degrees.
    pub vertical_fov_degrees: f32,
    /// Near clipping plane.
    pub near: f32,
    /// Far clipping plane.
    pub far: f32,
    /// Optional explicit look-at target in world space.
    ///
    /// When `Some`, the renderer uses this point as the `look_at` target instead
    /// of deriving the direction from `transform.rotation`. Used by the editor
    /// orbit camera to correctly apply pan offsets.
    pub look_at_target: Option<engine_core::math::Vec3>,
}

/// Camera projection used by a render view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderProjection {
    /// Perspective projection using [`RenderCamera::vertical_fov_degrees`].
    Perspective,
    /// Orthographic projection with the given vertical world-space size.
    Orthographic {
        /// Vertical visible size in world units.
        vertical_size: f32,
    },
}

impl Default for RenderProjection {
    fn default() -> Self {
        Self::Perspective
    }
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
    /// Whether this object contributes to shadow maps.
    pub casts_shadows: bool,
    /// Whether this object receives real-time shadows.
    pub receive_shadows: bool,
    /// Local-space conservative bounds used by visibility selection.
    pub bounds: RenderBounds,
    /// Optional distance-based mesh levels ordered from nearest to farthest.
    pub lods: Vec<RenderLod>,
}

/// Texture handles for a PBR material, ready for GPU binding.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderMaterialTextures {
    /// Base color (albedo) texture handle.
    pub base_color: Option<ImageHandle>,
    /// Tangent-space normal map texture handle.
    pub normal: Option<ImageHandle>,
    /// Metallic (B), roughness (G), ambient occlusion (R) packed texture handle.
    pub metallic_roughness: Option<ImageHandle>,
    /// Emissive texture handle.
    pub emissive: Option<ImageHandle>,
    /// Ambient occlusion texture handle (single channel).
    pub occlusion: Option<ImageHandle>,
}

/// 2D sprite draw data extracted from a scene.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderSprite {
    /// Source scene object.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Optional texture asset label.
    pub texture: Option<String>,
    /// Sprite tint color.
    pub color: [f32; 4],
    /// Draw order within the sorting layer.
    pub order_in_layer: i32,
    /// Sorting layer name.
    pub layer: String,
    /// Whether the sprite is flipped horizontally.
    pub flip_h: bool,
    /// Whether the sprite is flipped vertically.
    pub flip_v: bool,
}

/// Light data extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderLight {
    /// Source scene object.
    pub object: EntityId,
    /// World transform.
    pub transform: Transform,
    /// Light kind.
    pub kind: RenderLightKind,
    /// RGB light color.
    pub color: engine_core::math::Vec3,
    /// Light intensity.
    pub intensity: f32,
    /// Light range for point and spot lights.
    pub range: f32,
    /// Spot cone angle in degrees for spot lights.
    pub spot_angle: f32,
}

/// Light category used by render backends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderLightKind {
    /// Directional light with no position or range attenuation.
    Directional,
    /// Point light attenuated by range.
    Point,
    /// Spot light attenuated by range and cone angle.
    Spot,
}

impl RenderLightKind {
    /// Converts a serialized ECS light kind into a render light kind.
    pub fn from_component_kind(kind: &str) -> Self {
        match kind {
            "point" => Self::Point,
            "spot" => Self::Spot,
            _ => Self::Directional,
        }
    }
}

/// Lighting path requested by a render world or compiled frame graph.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RenderLightingMode {
    /// Forward shading path.
    #[default]
    Forward,
    /// Hybrid path that writes G-buffer targets and resolves lighting as a separate step.
    HybridDeferred,
}

/// Global illumination strategy requested by a render world.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderGlobalIllumination {
    /// Screen-space GI only.
    ScreenSpace,
    /// Probe volume assisted GI.
    ProbeVolume(RenderProbeVolume),
}

impl Default for RenderGlobalIllumination {
    fn default() -> Self {
        Self::ScreenSpace
    }
}

/// Probe volume settings for diffuse global illumination.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderProbeVolume {
    /// World-space center.
    pub center: engine_core::math::Vec3,
    /// World-space extents.
    pub extent: engine_core::math::Vec3,
    /// Probe counts on x/y/z axes.
    pub counts: [u32; 3],
    /// Indirect lighting multiplier.
    pub intensity: f32,
}

impl Default for RenderProbeVolume {
    fn default() -> Self {
        Self {
            center: engine_core::math::Vec3::ZERO,
            extent: engine_core::math::Vec3::new(20.0, 8.0, 20.0),
            counts: [6, 3, 6],
            intensity: 1.0,
        }
    }
}

/// Shadow map allocation strategy requested by a render world.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RenderShadowVirtualization {
    /// Fixed cascaded shadow maps.
    #[default]
    Cascaded,
    /// Virtual-shadow-map style page allocation boundary.
    VirtualPages {
        /// Page size in texels.
        page_size: u32,
        /// Maximum resident pages.
        max_pages: u32,
    },
}

/// Particle draw data extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderParticle {
    /// Source emitter scene object.
    pub object: EntityId,
    /// Particle world-space transform.
    pub transform: Transform,
    /// Particle RGBA color.
    pub color: [f32; 4],
    /// Particle normalized age from birth to death.
    pub age_fraction: f32,
}

/// GPU-simulatable particle emitter extracted from a scene.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderParticleEmitter {
    /// Source emitter scene object.
    pub object: EntityId,
    /// Emitter world transform.
    pub transform: Transform,
    /// Maximum particles evaluated by the backend.
    pub max_particles: u32,
    /// Particles emitted per second.
    pub emission_rate: f32,
    /// Particle lifetime in seconds.
    pub lifetime: f32,
    /// Initial speed.
    pub start_speed: f32,
    /// Initial and final size.
    pub size_range: [f32; 2],
    /// Initial RGBA color.
    pub start_color: [f32; 4],
    /// Final RGBA color.
    pub end_color: [f32; 4],
    /// World-space acceleration.
    pub gravity: engine_core::math::Vec3,
    /// Direction spread in degrees.
    pub spread_degrees: f32,
    /// Whether emission repeats.
    pub looping: bool,
    /// Deterministic random seed.
    pub seed: u32,
    /// Current simulation time.
    pub elapsed: f32,
}

/// Skybox configuration extracted from a scene for rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderSkybox {
    /// Optional cubemap texture asset label.
    pub cubemap: Option<String>,
    /// Zenith (top) color for procedural gradient fallback.
    pub zenith_color: [f32; 3],
    /// Horizon (bottom) color for procedural gradient fallback.
    pub horizon_color: [f32; 3],
    /// Rotation around the Y axis in degrees.
    pub rotation_degrees: f32,
    /// Intensity multiplier.
    pub intensity: f32,
}

/// Fog configuration forwarded to the render backend.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderFog {
    /// Exponential fog density (higher = thicker).
    pub density: f32,
    /// RGB fog color.
    pub color: [f32; 3],
    /// Whether fog is active.
    pub enabled: bool,
}

impl Default for RenderFog {
    fn default() -> Self {
        Self {
            density: 0.0003,
            color: [0.6, 0.7, 0.85],
            enabled: false,
        }
    }
}

/// Minimal render queue shared by runtime, editor Scene View, and Game View.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderWorld {
    /// Active camera.
    pub camera: Option<RenderCamera>,
    /// Queued mesh renderers.
    pub objects: Vec<RenderObject>,
    /// Queued 2D sprites.
    pub sprites: Vec<RenderSprite>,
    /// Queued lights.
    pub lights: Vec<RenderLight>,
    /// Queued particle instances.
    pub particles: Vec<RenderParticle>,
    /// Particle emitters available for backend GPU simulation.
    pub particle_emitters: Vec<RenderParticleEmitter>,
    /// Optional skybox configuration.
    pub skybox: Option<RenderSkybox>,
    /// Optional fog configuration.
    pub fog: Option<RenderFog>,
    /// Requested lighting path.
    pub lighting_mode: RenderLightingMode,
    /// Requested global illumination strategy.
    pub global_illumination: RenderGlobalIllumination,
    /// Requested shadow allocation strategy.
    pub shadow_virtualization: RenderShadowVirtualization,
}

impl RenderWorld {
    /// Returns true when there is visible geometry and a camera.
    pub fn is_visible(&self) -> bool {
        self.camera.is_some()
            && (!self.objects.is_empty()
                || !self.sprites.is_empty()
                || !self.particles.is_empty()
                || !self.particle_emitters.is_empty()
                || self.skybox.is_some())
    }

    /// Extracts renderable data from a [`Scene`](engine_ecs::Scene).
    ///
    /// Iterates all active game objects. For each object, extracts:
    /// - `Camera` → [`RenderCamera`] (stored as `self.camera`)
    /// - `MeshRenderer` + transform → [`RenderObject`]
    /// - `Sprite2D` + transform → [`RenderSprite`]
    /// - `Light` → [`RenderLight`]
    /// - `Skybox` → [`RenderSkybox`] (stored as `self.skybox`)
    ///
    /// Inactive objects and objects without any renderable component are skipped.
    pub fn extract(scene: &engine_ecs::Scene) -> Self {
        let mut world = RenderWorld::default();
        let mut camera_is_primary = false;

        for (entity, obj) in scene.iter_objects() {
            if !obj.active {
                continue;
            }

            let transform = scene.transforms().world(entity).unwrap_or_default();

            for component in &obj.components {
                match component {
                    engine_ecs::ComponentData::Camera(cam) => {
                        if world.camera.is_none() || (cam.primary && !camera_is_primary) {
                            world.camera = Some(RenderCamera {
                                object: obj.id,
                                transform,
                                projection: RenderProjection::Perspective,
                                vertical_fov_degrees: cam.vertical_fov_degrees,
                                near: cam.near,
                                far: cam.far,
                                look_at_target: None,
                            });
                            camera_is_primary = cam.primary;
                        }
                    }
                    engine_ecs::ComponentData::MeshRenderer(mesh) => {
                        let mesh_name = mesh
                            .builtin_mesh
                            .clone()
                            .or_else(|| mesh.mesh.map(|id| format!("asset:{:032x}", id.as_u128())))
                            .unwrap_or_default();
                        let material_name = mesh
                            .material
                            .builtin
                            .clone()
                            .or_else(|| {
                                mesh.material
                                    .asset
                                    .map(|id| format!("asset:{:032x}", id.as_u128()))
                            })
                            .unwrap_or_default();
                        world.objects.push(RenderObject {
                            object: obj.id,
                            transform,
                            mesh: mesh_name,
                            material: material_name,
                            casts_shadows: mesh.casts_shadows,
                            receive_shadows: mesh.receive_shadows,
                            bounds: RenderBounds::default(),
                            lods: Vec::new(),
                        });
                    }
                    engine_ecs::ComponentData::Camera2D(camera) => {
                        if world.camera.is_none() {
                            let mut camera_transform = transform;
                            camera_transform.translation.z += 10.0;
                            world.camera = Some(RenderCamera {
                                object: obj.id,
                                transform: camera_transform,
                                projection: RenderProjection::Orthographic {
                                    vertical_size: 10.0 / camera.zoom.max(0.01),
                                },
                                vertical_fov_degrees: 60.0,
                                near: 0.01,
                                far: 1000.0,
                                look_at_target: Some(transform.translation),
                            });
                        }
                    }
                    engine_ecs::ComponentData::Sprite2D(sprite) => {
                        world.sprites.push(RenderSprite {
                            object: obj.id,
                            transform,
                            texture: sprite
                                .texture
                                .map(|id| format!("asset:{:032x}", id.as_u128())),
                            color: sprite.color,
                            order_in_layer: sprite.order_in_layer,
                            layer: sprite.layer.clone(),
                            flip_h: sprite.flip_h,
                            flip_v: sprite.flip_v,
                        });
                    }
                    engine_ecs::ComponentData::Light(light) => {
                        world.lights.push(RenderLight {
                            object: obj.id,
                            transform,
                            kind: RenderLightKind::from_component_kind(&light.kind),
                            color: light.color,
                            intensity: light.intensity,
                            range: light.range,
                            spot_angle: light.spot_angle,
                        });
                    }
                    engine_ecs::ComponentData::ParticleEmitter(emitter) => {
                        world.particle_emitters.push(RenderParticleEmitter {
                            object: obj.id,
                            transform,
                            max_particles: emitter.max_particles,
                            emission_rate: emitter.emission_rate,
                            lifetime: emitter.lifetime,
                            start_speed: emitter.start_speed,
                            size_range: [emitter.start_size, emitter.end_size],
                            start_color: [
                                emitter.start_color.x,
                                emitter.start_color.y,
                                emitter.start_color.z,
                                1.0,
                            ],
                            end_color: [
                                emitter.end_color.x,
                                emitter.end_color.y,
                                emitter.end_color.z,
                                0.0,
                            ],
                            gravity: emitter.gravity,
                            spread_degrees: emitter.spread_degrees,
                            looping: emitter.looping,
                            seed: emitter.seed,
                            elapsed: emitter.elapsed,
                        });
                        world.particles.extend(
                            engine_ecs::ParticleSystem::sample(emitter, transform)
                                .into_iter()
                                .map(|particle| {
                                    let mut particle_transform = Transform::IDENTITY;
                                    particle_transform.translation = particle.position;
                                    particle_transform.scale = engine_core::math::Vec3::new(
                                        particle.size,
                                        particle.size,
                                        particle.size,
                                    );
                                    RenderParticle {
                                        object: obj.id,
                                        transform: particle_transform,
                                        color: particle.color,
                                        age_fraction: particle.age_fraction,
                                    }
                                }),
                        );
                    }
                    engine_ecs::ComponentData::Skybox(skybox) => {
                        world.skybox = Some(RenderSkybox {
                            cubemap: skybox
                                .cubemap
                                .map(|id| format!("asset:{:032x}", id.as_u128())),
                            zenith_color: skybox.zenith_color,
                            horizon_color: skybox.horizon_color,
                            rotation_degrees: skybox.rotation_degrees,
                            intensity: skybox.intensity,
                        });
                    }
                    _ => {}
                }
            }

            if obj.camera_role == Some(engine_ecs::CameraRole::Main) && world.camera.is_none() {
                world.camera = Some(RenderCamera {
                    object: obj.id,
                    transform,
                    projection: RenderProjection::Perspective,
                    vertical_fov_degrees: 60.0,
                    near: 0.1,
                    far: 1000.0,
                    look_at_target: None,
                });
            }
        }

        world
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

    /// Submits a Render World through a compiled Frame Pipeline.
    ///
    /// Backends that do not support graph-directed encoding preserve the legacy
    /// behavior by rendering first and then executing the graph.
    fn submit_render_world_with_graph(
        &mut self,
        world: &RenderWorld,
        graph: &RenderGraph,
        frame: RenderFrame,
    ) -> EngineResult<()> {
        self.submit_render_world(world, frame)?;
        self.execute_graph(graph, frame)
    }

    /// Submits a scene extraction to a specific render target.
    ///
    /// Backends without explicit target support fall back to their default target.
    fn submit_render_world_to_target(
        &mut self,
        world: &RenderWorld,
        target: &RenderTarget,
        frame: RenderFrame,
    ) -> EngineResult<()> {
        let _ = target;
        self.submit_render_world(world, frame)
    }

    /// Updates the internal linear render scale.
    fn set_render_scale(&mut self, _scale: f32) {}

    /// Returns scaling and frame generation capabilities exposed by this backend.
    fn render_scaling_capabilities(&self) -> RenderScalingCapabilities {
        RenderScalingCapabilities::built_in()
    }

    /// Negotiates scaling settings and applies the selected initial render scale.
    fn configure_render_scaling(
        &mut self,
        settings: &RenderScalingSettings,
        context: RenderScalingContext,
    ) -> RenderScalingSelection {
        let capabilities = if context.platform.is_mobile() {
            RenderScalingCapabilities::mobile_prototype(context)
        } else {
            self.render_scaling_capabilities()
        };
        let selection = negotiate_render_scaling(settings, &capabilities, context);
        self.set_render_scale(selection.render_scale);
        selection
    }

    /// Returns the latest backend performance measurements.
    fn performance_metrics(&self) -> RenderPerformanceMetrics {
        RenderPerformanceMetrics::default()
    }

    /// Records the complete runtime frame duration for adaptive policies.
    fn record_frame_time(&mut self, _frame_ms: f32) {}

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

    /// Uploads a mesh to the GPU with vertex data and indices.
    ///
    /// The default implementation is a no-op; backends that support mesh
    /// rendering should override this.
    fn upload_mesh_data(
        &mut self,
        _mesh_name: &str,
        _positions: &[[f32; 3]],
        _normals: &[[f32; 3]],
        _texcoords: &[[f32; 2]],
        _indices: &[u32],
    ) -> EngineResult<()> {
        Ok(())
    }

    /// Uploads a mesh with four joint indices and weights per vertex.
    ///
    /// Backends without GPU skinning support return an unsupported capability.
    #[allow(clippy::too_many_arguments)]
    fn upload_skinned_mesh_data(
        &mut self,
        _mesh_name: &str,
        _positions: &[[f32; 3]],
        _normals: &[[f32; 3]],
        _texcoords: &[[f32; 2]],
        _joint_indices: &[[u16; 4]],
        _joint_weights: &[[f32; 4]],
        _indices: &[u32],
    ) -> EngineResult<()> {
        Err(EngineError::UnsupportedCapability {
            capability: "gpu-skinning",
        })
    }

    /// Flushes the delayed destruction queue for the given frame.
    fn flush_destroy_queue(&mut self, frame_index: u64);

    /// Draws a batch of 2D quads.
    fn draw_2d_batch(&mut self, _vertices: &[[f32; 8]], _texture: ImageHandle) -> EngineResult<()> {
        Ok(())
    }

    /// Uploads bone matrices for GPU skinning.
    fn upload_bone_matrices(&mut self, _matrices: &[[f32; 16]]) -> EngineResult<BufferHandle> {
        Ok(BufferHandle(engine_core::Handle::new(
            0,
            engine_core::Generation::FIRST,
        )))
    }

    /// Draws a skinned mesh with bone matrices applied.
    fn draw_skinned_mesh(
        &mut self,
        _mesh_name: &str,
        _material_name: &str,
        _bone_buffer: BufferHandle,
        _bone_count: u32,
    ) -> EngineResult<()> {
        Ok(())
    }

    /// Registers PBR material parameters keyed by material name.
    ///
    /// The name must match the string produced by the scene extraction step for
    /// `RenderObject::material`. Backends that support per-material PBR
    /// parameters override this; others use the default no-op.
    fn register_material_params(
        &mut self,
        _name: &str,
        _base_color: [f32; 4],
        _metallic: f32,
        _roughness: f32,
        _emissive: [f32; 3],
    ) {
    }

    /// Registers GPU texture handles for a named material.
    ///
    /// Backends that support texture sampling override this to create
    /// per-material bind groups. Unset slots fall back to default textures.
    fn register_material_textures(&mut self, _name: &str, _textures: &RenderMaterialTextures) {}
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
    use engine_ecs::{ComponentData, Scene};

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

    #[test]
    fn extract_scene_with_camera_and_meshes() {
        let mut scene = Scene::new();

        // Camera entity
        let cam_entity = scene.create_object("Main Camera").unwrap();
        scene
            .upsert_component(
                cam_entity,
                ComponentData::Camera(engine_ecs::CameraComponentData {
                    vertical_fov_degrees: 60.0,
                    near: 0.1,
                    far: 100.0,
                    aspect_ratio: None,
                    primary: true,
                    clear_color: engine_core::math::Vec3::new(0.1, 0.1, 0.1),
                }),
            )
            .unwrap();

        // First mesh entity
        let mesh1 = scene.create_object("Cube").unwrap();
        scene
            .upsert_component(
                mesh1,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData {
                    mesh: None,
                    builtin_mesh: Some("debug/cube".to_string()),
                    material: engine_ecs::MaterialRef::debug(),
                    casts_shadows: true,
                    receive_shadows: true,
                }),
            )
            .unwrap();

        // Second mesh entity
        let mesh2 = scene.create_object("Sphere").unwrap();
        scene
            .upsert_component(
                mesh2,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData {
                    mesh: None,
                    builtin_mesh: Some("debug/sphere".to_string()),
                    material: engine_ecs::MaterialRef::debug(),
                    casts_shadows: false,
                    receive_shadows: true,
                }),
            )
            .unwrap();

        // Inactive entity — should be skipped
        let inactive = scene.create_object("Inactive").unwrap();
        scene
            .upsert_component(
                inactive,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData::default()),
            )
            .unwrap();
        scene.object_mut(inactive).unwrap().active = false;

        let world = RenderWorld::extract(&scene);

        assert!(world.camera.is_some(), "should have a camera");
        assert_eq!(world.objects.len(), 2, "should have 2 active mesh objects");
        assert_eq!(world.lights.len(), 0, "should have no lights");

        let cam = world.camera.unwrap();
        assert_eq!(cam.projection, RenderProjection::Perspective);
        assert_eq!(cam.vertical_fov_degrees, 60.0);
        assert_eq!(cam.near, 0.1);
        assert_eq!(cam.far, 100.0);

        // Verify mesh names
        let cube = world
            .objects
            .iter()
            .find(|o| o.mesh == "debug/cube")
            .unwrap();
        let sphere = world
            .objects
            .iter()
            .find(|o| o.mesh == "debug/sphere")
            .unwrap();
        assert_eq!(cube.material, "debug/default");
        assert_eq!(sphere.material, "debug/default");
        assert_ne!(sphere.object.as_u128(), 0);
    }

    #[test]
    fn maps_component_light_kind_to_render_kind() {
        assert_eq!(
            RenderLightKind::from_component_kind("directional"),
            RenderLightKind::Directional
        );
        assert_eq!(
            RenderLightKind::from_component_kind("point"),
            RenderLightKind::Point
        );
        assert_eq!(
            RenderLightKind::from_component_kind("spot"),
            RenderLightKind::Spot
        );
        assert_eq!(
            RenderLightKind::from_component_kind("invalid"),
            RenderLightKind::Directional
        );
    }

    #[test]
    fn extracts_particle_emitters() {
        let mut scene = Scene::new();
        let camera = scene.create_object("Camera").unwrap();
        scene
            .upsert_component(
                camera,
                ComponentData::Camera(engine_ecs::CameraComponentData::default()),
            )
            .unwrap();
        let emitter = scene.create_object("Emitter").unwrap();
        scene
            .upsert_component(
                emitter,
                ComponentData::ParticleEmitter(engine_ecs::ParticleEmitterComponentData {
                    elapsed: 0.5,
                    ..engine_ecs::ParticleEmitterComponentData::default()
                }),
            )
            .unwrap();

        let world = RenderWorld::extract(&scene);

        assert!(world.is_visible());
        assert!(!world.particles.is_empty());
    }

    #[test]
    fn extracts_mesh_shadow_flags() {
        let mut scene = Scene::new();
        let mesh = scene.create_object("No Shadows").unwrap();
        scene
            .upsert_component(
                mesh,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData {
                    casts_shadows: false,
                    receive_shadows: false,
                    ..engine_ecs::MeshRendererComponentData::default()
                }),
            )
            .unwrap();

        let world = RenderWorld::extract(&scene);

        assert_eq!(world.objects.len(), 1);
        assert!(!world.objects[0].casts_shadows);
        assert!(!world.objects[0].receive_shadows);
    }

    #[test]
    fn extracts_2d_camera_and_sprites() {
        let mut scene = Scene::new();
        let camera = scene.create_object("2D Camera").unwrap();
        scene
            .upsert_component(
                camera,
                ComponentData::Camera2D(engine_ecs::Camera2DComponentData {
                    zoom: 2.0,
                    ..engine_ecs::Camera2DComponentData::default()
                }),
            )
            .unwrap();

        let sprite = scene.create_object("Sprite").unwrap();
        scene
            .upsert_component(
                sprite,
                ComponentData::Sprite2D(engine_ecs::Sprite2DComponentData {
                    color: [0.25, 0.5, 0.75, 0.5],
                    flip_h: true,
                    order_in_layer: 3,
                    layer: "Foreground".to_string(),
                    ..engine_ecs::Sprite2DComponentData::default()
                }),
            )
            .unwrap();

        let world = RenderWorld::extract(&scene);

        assert!(world.is_visible());
        assert_eq!(world.sprites.len(), 1);
        assert_eq!(world.sprites[0].color, [0.25, 0.5, 0.75, 0.5]);
        assert!(world.sprites[0].flip_h);
        assert_eq!(world.sprites[0].order_in_layer, 3);
        assert_eq!(world.sprites[0].layer, "Foreground");
        assert_eq!(
            world.camera.unwrap().projection,
            RenderProjection::Orthographic { vertical_size: 5.0 }
        );
    }

    #[test]
    fn extraction_uses_world_transforms_primary_camera_and_active_state() {
        let mut scene = Scene::new();
        let parent = scene.create_object("Parent").unwrap();
        let child = scene.create_object("Child").unwrap();
        scene.set_parent(child, Some(parent)).unwrap();
        let mut parent_transform = Transform::IDENTITY;
        parent_transform.translation.x = 5.0;
        scene.transforms_mut().set_local(parent, parent_transform);
        let mut child_transform = Transform::IDENTITY;
        child_transform.translation.x = 2.0;
        scene.transforms_mut().set_local(child, child_transform);
        scene
            .upsert_component(
                child,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData::default()),
            )
            .unwrap();

        let secondary = scene.create_object("Secondary Camera").unwrap();
        scene
            .upsert_component(
                secondary,
                ComponentData::Camera(engine_ecs::CameraComponentData {
                    primary: false,
                    ..Default::default()
                }),
            )
            .unwrap();
        let primary = scene.create_object("Primary Camera").unwrap();
        scene
            .upsert_component(
                primary,
                ComponentData::Camera(engine_ecs::CameraComponentData {
                    primary: true,
                    ..Default::default()
                }),
            )
            .unwrap();

        let hidden = scene.create_object("Hidden").unwrap();
        scene
            .upsert_component(
                hidden,
                ComponentData::Sprite2D(engine_ecs::Sprite2DComponentData::default()),
            )
            .unwrap();
        scene.object_mut(hidden).unwrap().active = false;

        let world = RenderWorld::extract(&scene);

        assert_eq!(world.objects[0].transform.translation.x, 7.0);
        assert_eq!(
            world.camera.unwrap().object,
            scene.object(primary).unwrap().id
        );
        assert!(world.sprites.is_empty());
    }
}

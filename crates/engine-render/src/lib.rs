#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Render abstraction only. Concrete backends live outside `runtime-min`.

use std::collections::HashMap;

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
    AntiAliasingMode, BatteryPolicy, FrameGenerationCapability, FrameGenerationKind,
    MobileVendorAdapter, RenderPlatformClass, RenderQualityMode, RenderScalingCapabilities,
    RenderScalingContext, RenderScalingSelection, RenderScalingSettings, TemporalCameraData,
    TemporalFrameState, ThermalState, UiCompositionPolicy, UpscalerBackend, UpscalerCapability,
    UpscalerFrameData, UpscalerKind, negotiate_render_scaling,
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

/// PBR material parameters extracted directly from scene data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderMaterialParams {
    /// Base color and alpha.
    pub base_color: [f32; 4],
    /// Metallic/specular reflection hint.
    pub metallic: f32,
    /// Microfacet roughness.
    pub roughness: f32,
    /// Emissive color.
    pub emissive: [f32; 3],
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
    /// Additional physically-oriented light controls.
    pub settings: RenderLightSettings,
}

/// Renderer-facing controls for light quality and physically-inspired shading.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderLightSettings {
    /// Whether this light may contribute to real-time shadow maps.
    pub casts_shadow: bool,
    /// Approximate emitter radius in world units for softer local lighting.
    pub source_radius: f32,
    /// Optional correlated color temperature in Kelvin. Values <= 0 use the light color.
    pub temperature_kelvin: f32,
    /// Contact-shadow strength hint for near-surface shadowing.
    pub contact_shadow_strength: f32,
    /// Indirect lighting contribution multiplier.
    pub indirect_energy: f32,
    /// Specular highlight contribution multiplier.
    pub specular: f32,
    /// Distance attenuation exponent for point and spot lights.
    pub attenuation: f32,
    /// Constant shadow depth bias.
    pub shadow_bias: f32,
    /// Normal-dependent shadow bias.
    pub shadow_normal_bias: f32,
    /// Fraction of shadow range where shadows fade out.
    pub shadow_fade_start: f32,
    /// Maximum directional shadow distance.
    pub shadow_max_distance: f32,
    /// Render layers affected by this light.
    pub cull_mask: u32,
    /// Render layers allowed to cast shadows for this light.
    pub shadow_caster_mask: u32,
    /// Light baking behavior.
    pub bake_mode: RenderLightBakeMode,
    /// Directional shadow cascade layout.
    pub directional_shadow_mode: RenderDirectionalShadowMode,
    /// Whether directional cascades blend at split boundaries.
    pub directional_shadow_blend_splits: bool,
    /// Directional shadow split fractions.
    pub directional_shadow_splits: [f32; 3],
    /// Optional projector texture asset label/path.
    pub projector: Option<String>,
}

impl Default for RenderLightSettings {
    fn default() -> Self {
        Self {
            casts_shadow: true,
            source_radius: 0.0,
            temperature_kelvin: 0.0,
            contact_shadow_strength: 0.0,
            indirect_energy: 1.0,
            specular: 1.0,
            attenuation: 2.0,
            shadow_bias: 0.0008,
            shadow_normal_bias: 0.0025,
            shadow_fade_start: 0.8,
            shadow_max_distance: 200.0,
            cull_mask: u32::MAX,
            shadow_caster_mask: u32::MAX,
            bake_mode: RenderLightBakeMode::Dynamic,
            directional_shadow_mode: RenderDirectionalShadowMode::Parallel4Splits,
            directional_shadow_blend_splits: true,
            directional_shadow_splits: [0.1, 0.28, 0.55],
            projector: None,
        }
    }
}

/// Renderer-facing light baking behavior.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RenderLightBakeMode {
    /// Excluded from baked lighting.
    Disabled,
    /// Baked as static lighting.
    Static,
    /// May affect dynamic GI/light probes.
    #[default]
    Dynamic,
}

/// Renderer-facing directional shadow cascade layout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RenderDirectionalShadowMode {
    /// Single orthographic directional shadow.
    Orthogonal,
    /// Two parallel split shadow maps.
    Parallel2Splits,
    /// Four parallel split shadow maps.
    #[default]
    Parallel4Splits,
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
    /// Area light hint approximated by the active backend.
    Area,
}

impl RenderLightKind {
    /// Converts a serialized ECS light kind into a render light kind.
    pub fn from_component_kind(kind: &str) -> Self {
        match kind {
            "point" | "omni" => Self::Point,
            "spot" => Self::Spot,
            "area" => Self::Area,
            _ => Self::Directional,
        }
    }
}

impl From<engine_ecs::LightBakeMode> for RenderLightBakeMode {
    fn from(value: engine_ecs::LightBakeMode) -> Self {
        match value {
            engine_ecs::LightBakeMode::Disabled => Self::Disabled,
            engine_ecs::LightBakeMode::Static => Self::Static,
            engine_ecs::LightBakeMode::Dynamic => Self::Dynamic,
        }
    }
}

impl From<engine_ecs::DirectionalShadowMode> for RenderDirectionalShadowMode {
    fn from(value: engine_ecs::DirectionalShadowMode) -> Self {
        match value {
            engine_ecs::DirectionalShadowMode::Orthogonal => Self::Orthogonal,
            engine_ecs::DirectionalShadowMode::Parallel2Splits => Self::Parallel2Splits,
            engine_ecs::DirectionalShadowMode::Parallel4Splits => Self::Parallel4Splits,
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
    ScreenSpace {
        /// Indirect lighting multiplier.
        intensity: f32,
    },
    /// Probe volume assisted GI.
    ProbeVolume(RenderProbeVolume),
}

impl Default for RenderGlobalIllumination {
    fn default() -> Self {
        Self::ScreenSpace { intensity: 1.0 }
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
            intensity: 0.45,
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
            density: 0.00018,
            color: [0.62, 0.68, 0.76],
            enabled: true,
        }
    }
}

/// Environment controls extracted from scene data.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderEnvironment {
    /// Whether sky rendering is enabled.
    pub sky_enabled: bool,
    /// Optional cubemap texture asset label.
    pub sky_cubemap: Option<String>,
    /// Procedural sky zenith color.
    pub sky_zenith_color: [f32; 3],
    /// Procedural sky horizon color.
    pub sky_horizon_color: [f32; 3],
    /// Sky rotation around the Y axis in degrees.
    pub sky_rotation_degrees: f32,
    /// Sky intensity.
    pub sky_intensity: f32,
    /// Ambient diffuse color.
    pub ambient_color: [f32; 3],
    /// Ambient diffuse multiplier.
    pub ambient_intensity: f32,
    /// Fog settings.
    pub fog: RenderFog,
    /// HDR exposure multiplier.
    pub exposure: f32,
    /// Tonemapper identifier.
    pub tonemap: String,
    /// Whether bloom is composited.
    pub bloom_enabled: bool,
    /// Bloom contribution.
    pub bloom_intensity: f32,
    /// Whether SSAO is evaluated.
    pub ssao_enabled: bool,
    /// SSAO radius.
    pub ssao_radius: f32,
    /// SSAO contribution.
    pub ssao_intensity: f32,
    /// Whether SSGI is evaluated.
    pub ssgi_enabled: bool,
    /// SSGI ray radius.
    pub ssgi_radius: f32,
    /// SSGI contribution.
    pub ssgi_intensity: f32,
    /// Whether SSR is evaluated.
    pub ssr_enabled: bool,
    /// SSR contribution.
    pub ssr_intensity: f32,
}

impl Default for RenderEnvironment {
    fn default() -> Self {
        Self {
            sky_enabled: true,
            sky_cubemap: None,
            sky_zenith_color: [0.13, 0.23, 0.38],
            sky_horizon_color: [0.62, 0.68, 0.74],
            sky_rotation_degrees: 0.0,
            sky_intensity: 0.8,
            ambient_color: [0.07, 0.08, 0.095],
            ambient_intensity: 1.15,
            fog: RenderFog::default(),
            exposure: 1.08,
            tonemap: "aces".to_string(),
            bloom_enabled: true,
            bloom_intensity: 0.12,
            ssao_enabled: true,
            ssao_radius: 0.035,
            ssao_intensity: 0.65,
            ssgi_enabled: true,
            ssgi_radius: 2.25,
            ssgi_intensity: 0.45,
            ssr_enabled: true,
            ssr_intensity: 0.22,
        }
    }
}

/// Minimal render queue shared by runtime, editor Scene View, and Game View.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderWorld {
    /// Active camera.
    pub camera: Option<RenderCamera>,
    /// Queued mesh renderers.
    pub objects: Vec<RenderObject>,
    /// Dynamic material parameters keyed by extracted material name.
    pub material_params: HashMap<String, RenderMaterialParams>,
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
    /// Optional environment configuration.
    pub environment: Option<RenderEnvironment>,
    /// Requested lighting path.
    pub lighting_mode: RenderLightingMode,
    /// Requested global illumination strategy.
    pub global_illumination: RenderGlobalIllumination,
    /// Requested shadow allocation strategy.
    pub shadow_virtualization: RenderShadowVirtualization,
}

impl Default for RenderWorld {
    fn default() -> Self {
        Self {
            camera: None,
            objects: Vec::new(),
            material_params: HashMap::new(),
            sprites: Vec::new(),
            lights: Vec::new(),
            particles: Vec::new(),
            particle_emitters: Vec::new(),
            skybox: None,
            fog: None,
            environment: None,
            lighting_mode: RenderLightingMode::default(),
            global_illumination: RenderGlobalIllumination::ScreenSpace { intensity: 0.35 },
            shadow_virtualization: RenderShadowVirtualization::default(),
        }
    }
}

impl RenderWorld {
    /// Resolves the single renderer-facing environment for this frame.
    ///
    /// `environment` is the canonical input. Standalone `skybox` and `fog`
    /// remain as legacy authoring adapters for older scenes and direct tests.
    pub fn resolved_environment(&self) -> RenderEnvironment {
        if let Some(environment) = &self.environment {
            return environment.clone();
        }

        let mut environment = RenderEnvironment::default();
        if let Some(skybox) = &self.skybox {
            environment.sky_enabled = true;
            environment.sky_cubemap = skybox.cubemap.clone();
            environment.sky_zenith_color = skybox.zenith_color;
            environment.sky_horizon_color = skybox.horizon_color;
            environment.sky_rotation_degrees = skybox.rotation_degrees;
            environment.sky_intensity = skybox.intensity;
        } else {
            environment.sky_enabled = false;
        }
        if let Some(fog) = &self.fog {
            environment.fog = fog.clone();
        }
        environment
    }

    /// Returns the skybox representation derived from the resolved environment.
    pub fn active_skybox(&self) -> Option<RenderSkybox> {
        let environment = self.resolved_environment();
        environment.sky_enabled.then_some(RenderSkybox {
            cubemap: environment.sky_cubemap,
            zenith_color: environment.sky_zenith_color,
            horizon_color: environment.sky_horizon_color,
            rotation_degrees: environment.sky_rotation_degrees,
            intensity: environment.sky_intensity,
        })
    }

    /// Returns the fog representation derived from the resolved environment.
    pub fn active_fog(&self) -> Option<RenderFog> {
        let environment = self.resolved_environment();
        environment.fog.enabled.then_some(environment.fog)
    }

    /// Returns true when there is visible geometry and a camera.
    pub fn is_visible(&self) -> bool {
        self.camera.is_some()
            && (!self.objects.is_empty()
                || !self.sprites.is_empty()
                || !self.particles.is_empty()
                || !self.particle_emitters.is_empty()
                || self
                    .active_skybox()
                    .is_some_and(|skybox| skybox.intensity > 0.0))
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
            let fluid_volume = obj.components.iter().find_map(|component| match component {
                engine_ecs::ComponentData::FluidVolume(fluid) => Some(fluid),
                _ => None,
            });

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
                        if let Some(params) = parse_vscene_material_params(&material_name) {
                            world.material_params.insert(material_name.clone(), params);
                        }
                        let material_name = if let Some(fluid) = fluid_volume {
                            let material_name = format!("@water:{}", obj.id.as_u128());
                            let (base_color, metallic, roughness, emissive) =
                                fluid.render_material_params();
                            world.material_params.insert(
                                material_name.clone(),
                                RenderMaterialParams {
                                    base_color,
                                    metallic,
                                    roughness,
                                    emissive,
                                },
                            );
                            material_name
                        } else {
                            material_name
                        };
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
                            kind: RenderLightKind::from_component_kind(
                                light.light_kind().as_legacy_str(),
                            ),
                            color: light.color,
                            intensity: light.intensity,
                            range: light.range,
                            spot_angle: light.spot_angle,
                            settings: RenderLightSettings {
                                casts_shadow: light.casts_shadow,
                                source_radius: light.source_radius,
                                temperature_kelvin: light.temperature_kelvin,
                                contact_shadow_strength: light.contact_shadow_strength,
                                indirect_energy: light.indirect_energy,
                                specular: light.specular,
                                attenuation: light.attenuation,
                                shadow_bias: light.shadow_bias,
                                shadow_normal_bias: light.shadow_normal_bias,
                                shadow_fade_start: light.shadow_fade_start,
                                shadow_max_distance: light.shadow_max_distance,
                                cull_mask: light.cull_mask,
                                shadow_caster_mask: light.shadow_caster_mask,
                                bake_mode: light.bake_mode.into(),
                                directional_shadow_mode: light.directional_shadow_mode.into(),
                                directional_shadow_blend_splits: light
                                    .directional_shadow_blend_splits,
                                directional_shadow_splits: [
                                    light.directional_shadow_split_1,
                                    light.directional_shadow_split_2,
                                    light.directional_shadow_split_3,
                                ],
                                projector: light.projector.clone(),
                            },
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
                        let cubemap = skybox
                            .cubemap
                            .map(|id| format!("asset:{:032x}", id.as_u128()));
                        world.skybox = Some(RenderSkybox {
                            cubemap: cubemap.clone(),
                            zenith_color: skybox.zenith_color,
                            horizon_color: skybox.horizon_color,
                            rotation_degrees: skybox.rotation_degrees,
                            intensity: skybox.intensity,
                        });
                        if world.environment.is_none() {
                            world.environment = Some(RenderEnvironment {
                                sky_enabled: true,
                                sky_cubemap: cubemap,
                                sky_zenith_color: skybox.zenith_color,
                                sky_horizon_color: skybox.horizon_color,
                                sky_rotation_degrees: skybox.rotation_degrees,
                                sky_intensity: skybox.intensity,
                                ..RenderEnvironment::default()
                            });
                        }
                    }
                    engine_ecs::ComponentData::Environment(environment) => {
                        let sky_cubemap = environment
                            .sky_cubemap
                            .map(|id| format!("asset:{:032x}", id.as_u128()));
                        let fog = RenderFog {
                            density: environment.fog_density,
                            color: environment.fog_color,
                            enabled: environment.fog_enabled,
                        };
                        world.environment = Some(RenderEnvironment {
                            sky_enabled: environment.sky_enabled,
                            sky_cubemap: sky_cubemap.clone(),
                            sky_zenith_color: environment.sky_zenith_color,
                            sky_horizon_color: environment.sky_horizon_color,
                            sky_rotation_degrees: environment.sky_rotation_degrees,
                            sky_intensity: environment.sky_intensity,
                            ambient_color: environment.ambient_color,
                            ambient_intensity: environment.ambient_intensity,
                            fog: fog.clone(),
                            exposure: environment.exposure,
                            tonemap: environment.tonemap.clone(),
                            bloom_enabled: environment.bloom_enabled,
                            bloom_intensity: environment.bloom_intensity,
                            ssao_enabled: environment.ssao_enabled,
                            ssao_radius: environment.ssao_radius,
                            ssao_intensity: environment.ssao_intensity,
                            ssgi_enabled: environment.ssgi_enabled,
                            ssgi_radius: environment.ssgi_radius,
                            ssgi_intensity: environment.ssgi_intensity,
                            ssr_enabled: environment.ssr_enabled,
                            ssr_intensity: environment.ssr_intensity,
                        });
                        world.fog = Some(fog);
                        world.skybox = environment.sky_enabled.then_some(RenderSkybox {
                            cubemap: sky_cubemap,
                            zenith_color: environment.sky_zenith_color,
                            horizon_color: environment.sky_horizon_color,
                            rotation_degrees: environment.sky_rotation_degrees,
                            intensity: environment.sky_intensity,
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

fn parse_vscene_material_params(material: &str) -> Option<RenderMaterialParams> {
    let source = material.strip_prefix("@vscene-material:")?;
    let mut base_color = [1.0, 1.0, 1.0, 1.0];
    let mut metallic = 0.0;
    let mut roughness = 0.5;
    let mut emissive = [0.0, 0.0, 0.0];

    for part in source.split(';') {
        let (key, value) = part.split_once('=')?;
        match key {
            "base" => {
                let values = parse_f32_list(value);
                if values.len() == 4 {
                    base_color = [values[0], values[1], values[2], values[3]];
                }
            }
            "metallic" => metallic = value.parse().ok()?,
            "roughness" => roughness = value.parse().ok()?,
            "emissive" => {
                let values = parse_f32_list(value);
                if values.len() == 3 {
                    emissive = [values[0], values[1], values[2]];
                }
            }
            _ => {}
        }
    }

    Some(RenderMaterialParams {
        base_color,
        metallic,
        roughness,
        emissive,
    })
}

fn parse_f32_list(source: &str) -> Vec<f32> {
    source
        .split(',')
        .filter_map(|part| part.parse::<f32>().ok())
        .collect()
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

    /// Creates a GPU cubemap and uploads six tightly packed square faces into mip 0.
    ///
    /// Face order is +X, -X, +Y, -Y, +Z, -Z. Backends without cubemap support may
    /// fall back to a regular image handle, but GPU backends should create a cube view.
    fn upload_cubemap(&mut self, desc: ImageDesc, data: &[u8]) -> EngineResult<ImageHandle> {
        self.upload_texture(desc, data)
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

    /// Queues a GUI draw list to composite over the next presented surface frame.
    ///
    /// Backends without a surface overlay path may ignore this and rely on
    /// [`RenderDevice::draw_gui`] for offscreen composition.
    fn queue_surface_gui(&mut self, _draw_list: GuiDrawList) {}

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

    /// Registers a cubemap handle under a scene skybox asset label.
    ///
    /// The label must match `RenderSkybox::cubemap` after scene extraction.
    fn register_skybox_cubemap(&mut self, _label: &str, _cubemap: ImageHandle) {}
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

    fn upload_cubemap(&mut self, desc: ImageDesc, _data: &[u8]) -> EngineResult<ImageHandle> {
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

    fn upload_cubemap(&mut self, _desc: ImageDesc, _data: &[u8]) -> EngineResult<ImageHandle> {
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
    fn extracts_fluid_volume_as_dynamic_reflective_water_material() {
        let mut scene = Scene::new();
        let water = scene.create_object("Water").unwrap();
        scene
            .upsert_component(
                water,
                ComponentData::FluidVolume(engine_ecs::FluidVolumeComponentData {
                    water_tint: engine_core::math::Vec3::new(0.02, 0.22, 0.36),
                    water_alpha: 0.62,
                    reflection_strength: 0.9,
                    reflection_roughness: 0.04,
                    ..engine_ecs::FluidVolumeComponentData::default()
                }),
            )
            .unwrap();
        scene
            .upsert_component(
                water,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData {
                    builtin_mesh: Some("debug/plane".to_string()),
                    ..engine_ecs::MeshRendererComponentData::default()
                }),
            )
            .unwrap();

        let world = RenderWorld::extract(&scene);

        assert_eq!(world.objects.len(), 1);
        let object = &world.objects[0];
        assert!(object.material.starts_with("@water:"));
        let params = world.material_params.get(&object.material).unwrap();
        assert_eq!(params.base_color[3], 0.62);
        assert_eq!(params.metallic, 0.9);
        assert_eq!(params.roughness, 0.04);
    }

    #[test]
    fn extracts_vscene_inline_material_params() {
        let mut scene = Scene::new();
        let entity = scene.create_object("Inline Material").unwrap();
        let material =
            "@vscene-material:base=0.25,0.5,0.75,1;metallic=0.1;roughness=0.35;emissive=0.2,0.1,0";
        scene
            .upsert_component(
                entity,
                ComponentData::MeshRenderer(engine_ecs::MeshRendererComponentData {
                    builtin_mesh: Some("debug/cube".to_string()),
                    material: engine_ecs::MaterialRef {
                        asset: None,
                        builtin: Some(material.to_string()),
                    },
                    casts_shadows: true,
                    receive_shadows: true,
                    ..engine_ecs::MeshRendererComponentData::default()
                }),
            )
            .unwrap();

        let world = RenderWorld::extract(&scene);
        let params = world.material_params.get(material).unwrap();

        assert_eq!(world.objects[0].material, material);
        assert_eq!(params.base_color, [0.25, 0.5, 0.75, 1.0]);
        assert_eq!(params.metallic, 0.1);
        assert_eq!(params.roughness, 0.35);
        assert_eq!(params.emissive, [0.2, 0.1, 0.0]);
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
            RenderLightKind::from_component_kind("area"),
            RenderLightKind::Area
        );
        assert_eq!(
            RenderLightKind::from_component_kind("invalid"),
            RenderLightKind::Directional
        );
    }

    #[test]
    fn extracts_extended_light_quality_settings() {
        let mut scene = Scene::new();
        let entity = scene.create_object("Area Light").unwrap();
        scene
            .upsert_component(
                entity,
                ComponentData::Light(engine_ecs::LightComponentData {
                    kind: "area".to_string(),
                    indirect_energy: 0.4,
                    specular: 0.7,
                    attenuation: 1.5,
                    shadow_bias: 0.002,
                    shadow_normal_bias: 0.004,
                    shadow_fade_start: 0.6,
                    shadow_max_distance: 80.0,
                    cull_mask: 0b101,
                    shadow_caster_mask: 0b001,
                    bake_mode: engine_ecs::LightBakeMode::Static,
                    directional_shadow_mode: engine_ecs::DirectionalShadowMode::Parallel2Splits,
                    directional_shadow_blend_splits: false,
                    directional_shadow_split_1: 0.2,
                    directional_shadow_split_2: 0.45,
                    directional_shadow_split_3: 0.75,
                    projector: Some("textures/window.png".to_string()),
                    ..engine_ecs::LightComponentData::default()
                }),
            )
            .unwrap();

        let world = RenderWorld::extract(&scene);
        let light = &world.lights[0];

        assert_eq!(light.kind, RenderLightKind::Area);
        assert_eq!(light.settings.indirect_energy, 0.4);
        assert_eq!(light.settings.specular, 0.7);
        assert_eq!(light.settings.attenuation, 1.5);
        assert_eq!(light.settings.shadow_bias, 0.002);
        assert_eq!(light.settings.shadow_normal_bias, 0.004);
        assert_eq!(light.settings.shadow_fade_start, 0.6);
        assert_eq!(light.settings.shadow_max_distance, 80.0);
        assert_eq!(light.settings.cull_mask, 0b101);
        assert_eq!(light.settings.shadow_caster_mask, 0b001);
        assert_eq!(light.settings.bake_mode, RenderLightBakeMode::Static);
        assert_eq!(
            light.settings.directional_shadow_mode,
            RenderDirectionalShadowMode::Parallel2Splits
        );
        assert!(!light.settings.directional_shadow_blend_splits);
        assert_eq!(light.settings.directional_shadow_splits, [0.2, 0.45, 0.75]);
        assert_eq!(
            light.settings.projector.as_deref(),
            Some("textures/window.png")
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

    #[test]
    fn extracts_environment_as_render_settings() {
        let mut scene = Scene::new();
        let environment = scene.create_object("World Environment").unwrap();
        scene
            .upsert_component(
                environment,
                ComponentData::Environment(engine_ecs::EnvironmentComponentData {
                    sky_enabled: true,
                    sky_zenith_color: [0.2, 0.3, 0.4],
                    sky_horizon_color: [0.5, 0.6, 0.7],
                    sky_rotation_degrees: 15.0,
                    sky_intensity: 1.5,
                    fog_enabled: true,
                    fog_density: 0.002,
                    fog_color: [0.1, 0.2, 0.3],
                    exposure: 1.25,
                    bloom_enabled: false,
                    ssao_radius: 0.08,
                    ssgi_enabled: false,
                    ssr_intensity: 0.5,
                    ..engine_ecs::EnvironmentComponentData::default()
                }),
            )
            .unwrap();

        let world = RenderWorld::extract(&scene);
        let environment = world.environment.as_ref().expect("environment extracted");

        assert_eq!(environment.sky_zenith_color, [0.2, 0.3, 0.4]);
        assert_eq!(environment.sky_horizon_color, [0.5, 0.6, 0.7]);
        assert_eq!(environment.sky_rotation_degrees, 15.0);
        assert_eq!(environment.sky_intensity, 1.5);
        assert!(environment.fog.enabled);
        assert_eq!(environment.fog.density, 0.002);
        assert_eq!(environment.exposure, 1.25);
        assert!(!environment.bloom_enabled);
        assert_eq!(environment.ssao_radius, 0.08);
        assert!(!environment.ssgi_enabled);
        assert_eq!(environment.ssr_intensity, 0.5);
        assert!(world.skybox.is_some());
        assert!(world.fog.is_some());
        let resolved = world.resolved_environment();
        assert_eq!(resolved.sky_zenith_color, [0.2, 0.3, 0.4]);
        assert_eq!(resolved.fog.density, 0.002);
        assert_eq!(world.active_skybox().expect("active skybox").intensity, 1.5);
        assert_eq!(
            world.active_fog().expect("active fog").color,
            [0.1, 0.2, 0.3]
        );
    }

    #[test]
    fn resolves_legacy_skybox_and_fog_as_environment() {
        let world = RenderWorld {
            skybox: Some(RenderSkybox {
                cubemap: Some("asset:sky".to_string()),
                zenith_color: [0.1, 0.2, 0.3],
                horizon_color: [0.4, 0.5, 0.6],
                rotation_degrees: 42.0,
                intensity: 2.0,
            }),
            fog: Some(RenderFog {
                density: 0.01,
                color: [0.7, 0.8, 0.9],
                enabled: true,
            }),
            ..RenderWorld::default()
        };

        let environment = world.resolved_environment();

        assert!(environment.sky_enabled);
        assert_eq!(environment.sky_cubemap.as_deref(), Some("asset:sky"));
        assert_eq!(environment.sky_zenith_color, [0.1, 0.2, 0.3]);
        assert_eq!(environment.sky_horizon_color, [0.4, 0.5, 0.6]);
        assert_eq!(environment.sky_rotation_degrees, 42.0);
        assert_eq!(environment.sky_intensity, 2.0);
        assert_eq!(environment.fog.density, 0.01);
        assert_eq!(environment.fog.color, [0.7, 0.8, 0.9]);
        assert!(world.active_skybox().is_some());
        assert!(world.active_fog().is_some());
    }

    #[test]
    fn environment_takes_priority_over_legacy_skybox_and_fog() {
        let world = RenderWorld {
            environment: Some(RenderEnvironment {
                sky_enabled: true,
                sky_cubemap: Some("asset:environment".to_string()),
                sky_zenith_color: [0.9, 0.8, 0.7],
                sky_horizon_color: [0.6, 0.5, 0.4],
                sky_rotation_degrees: 12.0,
                sky_intensity: 0.75,
                fog: RenderFog {
                    density: 0.02,
                    color: [0.3, 0.2, 0.1],
                    enabled: true,
                },
                ..RenderEnvironment::default()
            }),
            skybox: Some(RenderSkybox {
                cubemap: Some("asset:legacy".to_string()),
                zenith_color: [0.0, 0.0, 0.0],
                horizon_color: [0.0, 0.0, 0.0],
                rotation_degrees: 180.0,
                intensity: 9.0,
            }),
            fog: Some(RenderFog {
                density: 1.0,
                color: [1.0, 1.0, 1.0],
                enabled: true,
            }),
            ..RenderWorld::default()
        };

        let skybox = world.active_skybox().expect("active skybox");
        let fog = world.active_fog().expect("active fog");

        assert_eq!(skybox.cubemap.as_deref(), Some("asset:environment"));
        assert_eq!(skybox.zenith_color, [0.9, 0.8, 0.7]);
        assert_eq!(skybox.rotation_degrees, 12.0);
        assert_eq!(skybox.intensity, 0.75);
        assert_eq!(fog.density, 0.02);
        assert_eq!(fog.color, [0.3, 0.2, 0.1]);
    }
}

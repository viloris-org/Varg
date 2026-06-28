use crate::{ColliderShape, Transform, Vec3};

/// Fluid surface profile used by buoyancy sampling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FluidSurfaceModel {
    /// Flat, still surface.
    Still,
    /// Directional river slope plus optional traveling waves.
    River,
    /// Ocean-style traveling wave pair.
    Ocean,
    /// Tidal offset plus optional traveling waves.
    Tidal,
}

impl Default for FluidSurfaceModel {
    fn default() -> Self {
        Self::Still
    }
}

/// Physics-facing description of a fluid volume.
#[derive(Clone, Debug, PartialEq)]
pub struct FluidVolumeDesc {
    /// Axis-aligned volume size in local space.
    pub size: Vec3,
    /// Fluid density in kg/m³.
    pub density: f32,
    /// Scales upward buoyancy.
    pub buoyancy_scale: f32,
    /// Linear drag applied against relative velocity.
    pub linear_drag: f32,
    /// Constant current velocity.
    pub flow_velocity: Vec3,
    /// Offset from the local top face used as the surface.
    pub surface_offset: f32,
    /// Surface sampling model.
    pub surface_model: FluidSurfaceModel,
    /// Direction used by waves and river slope in local XZ space.
    pub wave_direction: Vec3,
    /// Primary wave amplitude in world units.
    pub wave_amplitude: f32,
    /// Primary wave length in world units.
    pub wave_length: f32,
    /// Primary wave travel speed in world units per second.
    pub wave_speed: f32,
    /// Secondary cross-wave amplitude in world units.
    pub chop_amplitude: f32,
    /// Secondary cross-wave length in world units.
    pub chop_length: f32,
    /// River surface slope in height units per horizontal unit.
    pub river_slope: f32,
    /// Tidal height amplitude in world units.
    pub tide_amplitude: f32,
    /// Tidal cycle duration in seconds.
    pub tide_period_seconds: f32,
    /// Tidal phase offset in seconds.
    pub tide_phase_seconds: f32,
}

impl Default for FluidVolumeDesc {
    fn default() -> Self {
        Self {
            size: Vec3::new(4.0, 2.0, 4.0),
            density: 1000.0,
            buoyancy_scale: 1.0,
            linear_drag: 2.0,
            flow_velocity: Vec3::ZERO,
            surface_offset: 0.0,
            surface_model: FluidSurfaceModel::Still,
            wave_direction: Vec3::new(1.0, 0.0, 0.0),
            wave_amplitude: 0.0,
            wave_length: 12.0,
            wave_speed: 2.0,
            chop_amplitude: 0.0,
            chop_length: 5.0,
            river_slope: 0.0,
            tide_amplitude: 0.0,
            tide_period_seconds: 44_712.0,
            tide_phase_seconds: 0.0,
        }
    }
}

impl FluidVolumeDesc {
    /// Samples the local-space fluid surface height at a point and time.
    pub fn surface_height_at(&self, local_position: Vec3, time_seconds: f32) -> f32 {
        let base_height = self.size.y * 0.5 - self.surface_offset;

        let river_height = if self.surface_model == FluidSurfaceModel::River {
            self.river_slope * horizontal_phase(local_position, self.wave_direction)
        } else {
            0.0
        };

        let tide_height = if self.surface_model == FluidSurfaceModel::Tidal {
            cycle_height(
                self.tide_amplitude,
                self.tide_period_seconds,
                time_seconds + self.tide_phase_seconds,
            )
        } else {
            0.0
        };

        let wave_height = if matches!(
            self.surface_model,
            FluidSurfaceModel::River | FluidSurfaceModel::Ocean | FluidSurfaceModel::Tidal
        ) {
            traveling_wave_height(
                self.wave_amplitude,
                self.wave_length,
                self.wave_speed,
                self.wave_direction,
                local_position,
                time_seconds,
            ) + traveling_wave_height(
                self.chop_amplitude,
                self.chop_length,
                self.wave_speed * 1.7,
                Vec3::new(-self.wave_direction.z, 0.0, self.wave_direction.x),
                local_position,
                time_seconds,
            )
        } else {
            0.0
        };

        base_height + river_height + tide_height + wave_height
    }

    /// Returns signed depth below the sampled local-space fluid surface.
    pub fn depth_at(&self, local_position: Vec3, time_seconds: f32) -> f32 {
        self.surface_height_at(local_position, time_seconds) - local_position.y
    }
}

/// A fluid volume sampled in world space for one physics step.
#[derive(Clone, Debug, PartialEq)]
pub struct FluidVolumeSample {
    /// Fluid description.
    pub desc: FluidVolumeDesc,
    /// World transform of the volume.
    pub transform: Transform,
    /// Inverse world transform of the volume.
    pub inverse_transform: Transform,
    /// World-space minimum AABB bound.
    pub min: Vec3,
    /// World-space maximum AABB bound.
    pub max: Vec3,
}

impl FluidVolumeSample {
    /// Builds a fluid sample from a description and world transform.
    pub fn new(desc: FluidVolumeDesc, transform: Transform) -> Self {
        let half_extents = (desc.size * transform.scale) * 0.5;
        Self {
            desc,
            transform,
            inverse_transform: transform.inverse(),
            min: transform.translation - half_extents,
            max: transform.translation + half_extents,
        }
    }

    /// Samples the world-space fluid surface Y at a world-space position.
    pub fn surface_world_y(&self, world_position: Vec3, time_seconds: f32) -> f32 {
        let local = self.inverse_transform.transform_point(world_position);
        let local_surface = Vec3::new(
            local.x,
            self.desc.surface_height_at(local, time_seconds),
            local.z,
        );
        self.transform
            .transform_point(local_surface)
            .y
            .clamp(self.min.y, self.max.y)
    }
}

/// Rigid body sample used for volume-based buoyancy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuoyancyBodySample {
    /// World-space AABB minimum.
    pub min: Vec3,
    /// World-space AABB maximum.
    pub max: Vec3,
    /// Current linear velocity.
    pub velocity: Vec3,
    /// Collider displacement volume.
    pub collider_volume: f32,
    /// Body mass.
    pub mass: f32,
}

/// Probe set used for wave-aware buoyancy.
#[derive(Clone, Debug, PartialEq)]
pub struct BuoyancyProbeSet {
    /// Local-space probe positions.
    pub probes: Vec<Vec3>,
    /// Upward force scale per submerged probe.
    pub buoyancy: f32,
    /// Drag applied per probe.
    pub damping: f32,
    /// Torque scale for off-center probe forces.
    pub angular_response: f32,
}

impl Default for BuoyancyProbeSet {
    fn default() -> Self {
        Self {
            probes: vec![
                Vec3::new(-0.5, -0.5, -0.5),
                Vec3::new(0.5, -0.5, -0.5),
                Vec3::new(-0.5, -0.5, 0.5),
                Vec3::new(0.5, -0.5, 0.5),
            ],
            buoyancy: 1.0,
            damping: 2.0,
            angular_response: 1.0,
        }
    }
}

/// Net force and torque produced by one fluid interaction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FluidForce {
    /// Linear force to apply to the body.
    pub force: Vec3,
    /// Torque to apply to the body.
    pub torque: Vec3,
}

impl FluidForce {
    /// Returns whether both force and torque are effectively zero.
    pub fn is_zero(self) -> bool {
        self.force.length_squared() <= f32::EPSILON && self.torque.length_squared() <= f32::EPSILON
    }
}

/// Computes volume-based buoyancy and drag for a rigid body in a fluid volume.
pub fn solve_volume_buoyancy(
    fluid: &FluidVolumeSample,
    body: BuoyancyBodySample,
    gravity: f32,
    time_seconds: f32,
) -> FluidForce {
    let sample_position = Vec3::new(
        (body.min.x + body.max.x) * 0.5,
        (body.min.y + body.max.y) * 0.5,
        (body.min.z + body.max.z) * 0.5,
    );
    let surface_y = fluid.surface_world_y(sample_position, time_seconds);
    let overlap_min = Vec3::new(
        body.min.x.max(fluid.min.x),
        body.min.y.max(fluid.min.y),
        body.min.z.max(fluid.min.z),
    );
    let overlap_max = Vec3::new(
        body.max.x.min(fluid.max.x),
        body.max.y.min(surface_y.min(fluid.max.y)),
        body.max.z.min(fluid.max.z),
    );
    let overlap = overlap_max - overlap_min;
    if overlap.x <= 0.0 || overlap.y <= 0.0 || overlap.z <= 0.0 {
        return FluidForce::default();
    }

    let body_aabb_volume =
        ((body.max.x - body.min.x) * (body.max.y - body.min.y) * (body.max.z - body.min.z))
            .max(f32::EPSILON);
    let overlap_volume = overlap.x * overlap.y * overlap.z;
    let submerged_fraction = (overlap_volume / body_aabb_volume).clamp(0.0, 1.0);
    if submerged_fraction <= f32::EPSILON {
        return FluidForce::default();
    }

    let mass = body.mass.max(0.001);
    let displaced_volume = body.collider_volume.max(0.001) * submerged_fraction;
    let buoyancy = Vec3::new(
        0.0,
        fluid.desc.density.max(0.0) * displaced_volume * gravity * fluid.desc.buoyancy_scale / mass,
        0.0,
    );
    let relative_velocity = body.velocity - fluid.desc.flow_velocity;
    let drag = relative_velocity * (-fluid.desc.linear_drag.max(0.0) * mass) * submerged_fraction;
    FluidForce {
        force: buoyancy + drag,
        torque: Vec3::ZERO,
    }
}

/// Computes probe-based buoyancy and torque for a rigid body in a fluid volume.
pub fn solve_probe_buoyancy(
    fluid: &FluidVolumeSample,
    probe_set: &BuoyancyProbeSet,
    transform: Transform,
    previous_transform: Transform,
    mass: f32,
    gravity: f32,
    dt: f32,
    time_seconds: f32,
) -> FluidForce {
    if dt <= f32::EPSILON {
        return FluidForce::default();
    }

    let live_probes = probe_set
        .probes
        .iter()
        .copied()
        .filter(|probe| probe.x.is_finite() && probe.y.is_finite() && probe.z.is_finite())
        .collect::<Vec<_>>();
    if live_probes.is_empty() {
        return FluidForce::default();
    }

    let mass = mass.max(0.001);
    let probe_count = live_probes.len() as f32;
    let mut output = FluidForce::default();
    for local_probe in live_probes {
        let world_probe = transform.transform_point(local_probe);
        let local_to_fluid = fluid.inverse_transform.transform_point(world_probe);
        if local_to_fluid.x < -fluid.desc.size.x * 0.5
            || local_to_fluid.x > fluid.desc.size.x * 0.5
            || local_to_fluid.z < -fluid.desc.size.z * 0.5
            || local_to_fluid.z > fluid.desc.size.z * 0.5
            || local_to_fluid.y < -fluid.desc.size.y * 0.5
        {
            continue;
        }

        let depth = fluid.desc.depth_at(local_to_fluid, time_seconds);
        if depth <= f32::EPSILON {
            continue;
        }

        let depth_fraction = (depth / fluid.desc.size.y.max(f32::EPSILON)).clamp(0.0, 1.0);
        let previous_world_probe = previous_transform.transform_point(local_probe);
        let probe_velocity = (world_probe - previous_world_probe) / dt;
        let relative_velocity = probe_velocity - fluid.desc.flow_velocity;
        let buoyancy = Vec3::new(
            0.0,
            fluid.desc.density.max(0.0)
                * gravity
                * fluid.desc.buoyancy_scale
                * probe_set.buoyancy.max(0.0)
                * depth_fraction
                / (mass * probe_count),
            0.0,
        );
        let drag = relative_velocity
            * (-probe_set.damping.max(0.0) * fluid.desc.linear_drag.max(0.0) * depth_fraction);
        let force = buoyancy + drag;
        if force.length_squared() <= f32::EPSILON {
            continue;
        }

        output.force += force;
        let lever_arm = world_probe - transform.translation;
        output.torque += lever_arm.cross(force) * probe_set.angular_response.max(0.0);
    }

    output
}

/// Returns displacement volume for a collider shape.
pub fn collider_displacement_volume(shape: &ColliderShape) -> f32 {
    match *shape {
        ColliderShape::Sphere { radius } => {
            (4.0 / 3.0) * std::f32::consts::PI * radius.max(0.0).powi(3)
        }
        ColliderShape::Capsule {
            half_height,
            radius,
        } => {
            let radius = radius.max(0.0);
            let cylinder_height = (half_height.max(0.0) * 2.0).max(0.0);
            let cylinder = std::f32::consts::PI * radius.powi(2) * cylinder_height;
            let caps = (4.0 / 3.0) * std::f32::consts::PI * radius.powi(3);
            cylinder + caps
        }
        ColliderShape::Box { half_extents } => {
            let size = half_extents * 2.0;
            (size.x.abs() * size.y.abs() * size.z.abs()).max(0.0)
        }
        ColliderShape::Mesh { .. }
        | ColliderShape::TriMesh { .. }
        | ColliderShape::Heightfield { .. } => 0.0,
    }
}

fn horizontal_phase(local_position: Vec3, direction: Vec3) -> f32 {
    let dir = horizontal_direction(direction);
    local_position.x * dir.x + local_position.z * dir.z
}

fn horizontal_direction(direction: Vec3) -> Vec3 {
    let horizontal = Vec3::new(direction.x, 0.0, direction.z);
    if horizontal.length_squared() <= 1e-8 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        horizontal.normalized()
    }
}

fn cycle_height(amplitude: f32, period_seconds: f32, time_seconds: f32) -> f32 {
    if amplitude == 0.0 || period_seconds <= 1e-5 {
        return 0.0;
    }
    let phase = time_seconds / period_seconds * std::f32::consts::TAU;
    amplitude * phase.sin()
}

fn traveling_wave_height(
    amplitude: f32,
    wave_length: f32,
    wave_speed: f32,
    direction: Vec3,
    local_position: Vec3,
    time_seconds: f32,
) -> f32 {
    if amplitude == 0.0 || wave_length <= 1e-5 {
        return 0.0;
    }
    let phase = (horizontal_phase(local_position, direction) - wave_speed * time_seconds)
        / wave_length
        * std::f32::consts::TAU;
    amplitude * phase.sin()
}

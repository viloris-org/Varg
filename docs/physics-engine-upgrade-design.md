# Physics Engine Commercial-Grade Upgrade Design

## Overview

Upgrade `engine-physics` from "passable" to "excellent" by completing the full feature set expected in a commercial engine (UE5, Unity, Godot Jolt). The Rapier 0.22 backend is the production target; `SimplePhysicsBackend` remains a deterministic dev stub.

---

## Phase 1: Vehicle System

### Motivation
Every commercial engine ships wheeled vehicle support. Rapier 0.22 provides `DynamicRayCastVehicleController` — a full raycast-based vehicle with suspension, tire friction, and Ackermann steering.

### API Surface

```rust
// ===== engine-physics/src/vehicle.rs (new file) =====

/// Wheel definition attached to a vehicle.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct WheelDesc {
    /// Chassis-relative attachment point.
    pub chassis_connection: Vec3,
    /// Wheel center offset (typically downward).
    pub center_offset: Vec3,
    /// Wheel radius.
    pub radius: f32,
    /// Suspension rest length.
    pub suspension_rest: f32,
    /// Suspension maximum travel.
    pub suspension_travel: f32,
    /// Suspension stiffness (N/m).
    pub suspension_stiffness: f32,
    /// Suspension damping (N·s/m).
    pub suspension_damping: f32,
    /// Maximum suspension force.
    pub max_suspension_force: f32,
    /// Lateral friction stiffness (corrective force on side-slip).
    pub lateral_friction_stiffness: f32,
    /// Longitudinal friction stiffness (drive/brake force).
    pub longitudinal_friction_stiffness: f32,
    /// Whether this wheel is steered.
    pub is_steered: bool,
    /// Whether this wheel is powered.
    pub is_powered: bool,
    /// Whether this wheel has braking enabled.
    pub is_braked: bool,
}

/// Vehicle-wide tuning parameters.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct VehicleTuning {
    /// Mass fraction of the chassis used for vehicle inertia (0..1).
    pub chassis_mass_fraction: f32,
    /// Maximum steering angle in radians.
    pub max_steering_angle: f32,
    /// Wheel radius used by the controller.
    pub wheel_radius: f32,
}

/// Vehicle creation descriptor.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct VehicleDesc {
    /// Chassis rigidbody handle.
    pub chassis: BodyHandle,
    /// Per-wheel definitions.
    pub wheels: Vec<WheelDesc>,
    /// Vehicle-wide tuning.
    pub tuning: VehicleTuning,
}

/// Opaque handle to a vehicle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct VehicleHandle(pub u64);

/// Per-step vehicle input (digital, like a gamepad or keyboard).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VehicleInput {
    /// Forward acceleration (0..1).
    pub throttle: f32,
    /// Brake force (0..1).
    pub brake: f32,
    /// Steering angle fraction (-1..1). Scaled by max_steering_angle.
    pub steering: f32,
    /// Handbrake toggle.
    pub handbrake: bool,
}

/// Runtime state returned after each vehicle step.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleState {
    /// Current speed magnitude (m/s).
    pub speed: f32,
    /// Current gear direction: -1 reverse, 0 neutral, 1 forward.
    pub gear: i8,
    /// Per-wheel world-space transforms.
    pub wheel_transforms: Vec<Transform>,
    /// Per-wheel suspension displacement (0=rest, positive=compressed).
    pub suspension_displacements: Vec<f32>,
    /// Whether the vehicle is grounded.
    pub grounded: bool,
}
```

### Backend Trait Additions

```rust
// In PhysicsBackend trait (lib.rs):

/// Creates a wheeled vehicle attached to a chassis body.
fn create_vehicle(&mut self, desc: &VehicleDesc) -> EngineResult<VehicleHandle>;

/// Destroys a vehicle.
fn destroy_vehicle(&mut self, vehicle: VehicleHandle) -> EngineResult<()>;

/// Updates vehicle inputs and returns current state.
/// Call during fixed_update or separately each physics step.
fn update_vehicle(
    &mut self,
    vehicle: VehicleHandle,
    input: VehicleInput,
) -> EngineResult<VehicleState>;
```

### Rapier Implementation

`RapierPhysicsBackend` wraps `DynamicRayCastVehicleController`. In `fixed_update`, after `pipeline.step()`, call `vehicle_controller.update_vehicle()` for each active vehicle. Rapier's controller handles suspension raycasts, tire forces, and updates the chassis velocity internally.

Key detail: Rapier's `DynamicRayCastVehicleController::update_vehicle()` requires `dt` and a filter predicate. We bind each vehicle to a `ColliderHandle` used as the filter group.

### SimplePhysicsBackend Implementation

Stub — returns `UnsupportedCapability` for all vehicle methods.

### ECS Integration

```rust
// In engine-ecs/src/vehicle.rs (new file, gated on feature "physics"):

pub struct VehicleComponent {
    pub desc: VehicleDesc,
    pub input: VehicleInput,
    pub handle: Option<VehicleHandle>,
    pub state: VehicleState,
}
```

### Data Flow

```
Script/Input → VehicleComponent.input
                ↓ (each fixed_update)
PhysicsWorld::update_vehicle(handle, input)
                ↓
Rapier: DynamicRayCastVehicleController::update_vehicle()
                ↓
VehicleComponent.state ← speed, gear, wheel_transforms
                ↓
Renderer reads wheel_transforms for mesh placement
```

---

## Phase 2: Runtime Collision Filtering

### Motivation
Commercial engines allow per-entity or per-pair collision filtering at runtime (Unreal's `NotifyHit`, Unity's collision matrix + layer override). Currently Aster can only decide layering at collider creation time.

### Design: Contact Filter Chain

Instead of a single callback (which can't be serialized), use a **filter chain** — a list of predicates evaluated in order. The first `Ignore` or `Block` wins; if none match, the default layer matrix applies.

```rust
/// Predicate evaluated at narrow-phase before the contact pair is solved.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ContactFilter {
    /// Ignore all contacts involving this body.
    IgnoreBody { body: BodyHandle },
    /// Ignore contacts between this specific pair.
    IgnorePair {
        body_a: BodyHandle,
        body_b: BodyHandle,
    },
    /// Ignore contacts involving any body with this name tag.
    IgnoreTag { tag: String },
    /// Always allow, regardless of layer matrix (overrides Ignore from earlier filters).
    ForcePair {
        body_a: BodyHandle,
        body_b: BodyHandle,
    },
}

#[derive(Clone, Debug, Default)]
pub struct ContactFilterChain {
    filters: Vec<ContactFilter>,
}
```

Both backends consult the filter chain before processing a contact pair:

```
for each contact pair (a, b) in narrow_phase:
    for filter in chain:
        if filter.matches(a, b) and filter == Ignore*:
            skip pair
        if filter.matches(a, b) and filter == ForcePair:
            process pair
    // fall through to layer matrix
    if layer_matrix.collides(layer_a, layer_b):
        process pair
```

### Rapier Integration

Rapier natively supports `QueryFilter` with `exclude_collider`, `exclude_rigid_body`, and custom `filter: Option<&dyn Fn(...)>`. Our filter chain translates to Rapier's filter predicates during each step.

### API

```rust
impl PhysicsWorld {
    /// Adds a contact filter to the chain.
    pub fn add_contact_filter(&mut self, filter: ContactFilter);

    /// Removes all contact filters matching a predicate.
    pub fn remove_contact_filters(&mut self, predicate: impl Fn(&ContactFilter) -> bool);

    /// Clears all contact filters.
    pub fn clear_contact_filters(&mut self);
}
```

---

## Phase 3: Joint System Completion

### Current Defects

| Issue | File:Line | Fix |
|---|---|---|
| `set_joint_motor` hardcoded to `AngX` for all joints | `lib.rs:2820` | Dispatch to correct axis: `AngX` for Hinge, `LinX` for Slider, etc. |
| `joint_state` returns `UnsupportedCapability` in Rapier | `lib.rs:2806` | Read actual position/velocity from `impulse_joints.get()` |
| `joint_forces` returns `(0,0)` in Rapier | `lib.rs:2850` | Rapier supports `ImpulseJoint::contacts_enabled()` for force reporting |
| `ConeTwist` maps to bare `SphericalJoint::new()` | `lib.rs:2743` | Apply `GenericJoint` with swing/twist limits |
| `SpringArm` maps to `FixedJoint::new()` | `lib.rs:2739` | Should apply a spring force each step, not fixed constraint |

### Fix: ConeTwist

Map to `GenericJoint` with:
- `JointAxis::AngX` = twist limits
- `JointAxis::AngY` = swing Y limits
- `JointAxis::AngZ` = swing Z limits
- Lock linear axes (LinX/Y/Z)

```rust
JointType::ConeTwist {
    twist_axis_a,
    swing_limits,
    twist_limits,
    ..
} => {
    let mut j = rp::GenericJoint::default();
    // Lock all linear axes
    for axis in [rp::JointAxis::LinX, rp::JointAxis::LinY, rp::JointAxis::LinZ] {
        j.set_limits(axis, [0.0, 0.0]);
    }
    // Twist limits
    j.set_limits(rp::JointAxis::AngX, [twist_limits.min, twist_limits.max]);
    // Swing limits (cone approximation)
    j.set_limits(rp::JointAxis::AngY, [-swing_limits.max, swing_limits.max]);
    j.set_limits(rp::JointAxis::AngZ, [-swing_limits.max, swing_limits.max]);
    j
}
```

### Fix: SpringArm

Use `GenericJoint` with free linear+angular axes, and apply explicit spring force each step in `fixed_update`. Track per-joint force state.

Alternative: Use Rapier's `SpringJoint` if available in 0.22, or `RopeJoint` / `PrismaticJoint` with a spring limit.

Preferred: `GenericJoint` with motor on `LinX` (distance axis) to simulate spring. In `set_joint_motor`, configure the axis motor:

```rust
JointType::SpringArm { stiffness, damping, rest_length, .. } => {
    let mut j = rp::GenericJoint::default();
    // Motor on linear X to simulate spring toward rest_length
    j.set_motor_model(rp::JointAxis::LinX, rp::MotorModel::ForceBased);
    j.set_motor(rp::JointAxis::LinX, 0.0, stiffness, 0.0, damping);
    j
}
```

### Fix: joint_state

```rust
fn joint_state(&self, joint: JointHandle) -> EngineResult<JointState> {
    let rapier = self.joint_handles.get(&joint).ok_or(...)?;
    let j = self.impulse_joints.get(*rapier).ok_or(...)?;
    // Read per-axis position and velocity
    let positions: [f32; 6] = [0.0; 6]; // from j.data.motor_position()
    let velocities: [f32; 6] = [0.0; 6]; // from j.data.motor_velocity()
    Ok(JointState {
        handle: joint,
        positions,
        velocities,
    })
}
```

Note: `JointState` currently holds a `JointDesc` (creation params) — we need to add runtime fields:

```rust
pub struct JointState {
    pub handle: JointHandle,
    pub positions: [f32; 6],   // per-DOF position
    pub velocities: [f32; 6],  // per-DOF velocity
    pub desc: JointDesc,       // creation descriptor (existing)
}
```

### Fix: set_joint_motor axis dispatch

```rust
fn set_joint_motor(&mut self, joint: JointHandle, motor: JointMotor) -> EngineResult<()> {
    let rapier = self.joint_handles.get(&joint).ok_or(...)?;
    let j = self.impulse_joints.get_mut(*rapier).ok_or(...)?;
    let axis = match /* lookup joint type */ {
        JointType::Hinge { .. } => rp::JointAxis::AngX,
        JointType::Slider { .. } => rp::JointAxis::LinX,
        JointType::Generic6DOF { .. } => rp::JointAxis::AngX, // default, user can override per-axis
        _ => return Err("unsupported motor axis"),
    };
    if motor.enabled {
        j.data.set_motor(axis, motor.target_velocity, 0.0, motor.max_force, 0.0);
    }
    Ok(())
}
```

### Multibody Joint Integration

Rapier's `MultibodyJointSet` provides reduced-coordinate articulated body simulation — more stable for chains (ragdolls, robotic arms). We should:

1. Add `use_multibody: bool` to `JointDesc` (or auto-detect based on topology)
2. When `use_multibody` is true, create joint in `MultibodyJointSet` instead of `ImpulseJointSet`
3. Multibody joints have different API (`MultibodyJoint::motor_velocity()` vs `set_motor()`)

Priority: This is lower than the defect fixes above. Marked for a follow-up after P1-P3.

---

## Phase 4: Physics Profiler

### API

```rust
#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsStats {
    /// Wall-clock step duration (µs).
    pub step_us: u64,
    /// Number of rigid bodies.
    pub body_count: usize,
    /// Number of colliders.
    pub collider_count: usize,
    /// Number of active contact pairs.
    pub contact_count: usize,
    /// Number of active simulation islands.
    pub island_count: usize,
    /// Number of sleeping bodies.
    pub sleeping_count: usize,
    /// Number of active joints.
    pub joint_count: usize,
    /// Number of active vehicles.
    pub vehicle_count: usize,
    /// CCD sub-step count from the last step.
    pub ccd_sub_steps: u32,
}

impl PhysicsWorld {
    /// Returns stats collected during the most recent `fixed_update`.
    pub fn stats(&self) -> PhysicsStats;
}
```

### Rapier Implementation

Rapier 0.22 exposes `NarrowPhase::num ContactPairs()`, `IslandManager::num islands()`, etc. Wrap `pipeline.step()` with `Instant` to measure duration.

```rust
let start = Instant::now();
self.pipeline.step(/* ... */);
self.stats.step_us = start.elapsed().as_micros() as u64;
self.stats.body_count = self.bodies.len();
self.stats.collider_count = self.colliders.len();
self.stats.contact_count = self.narrow_phase.num_contact_pairs();
self.stats.island_count = self.islands.num_islands();
```

### SimplePhysicsBackend Implementation

Count bodies, colliders, contacts from internal `HashMap`s. Time the `fixed_update` call.

---

## Phase 5: Fix Existing Defects

### Consolidated Fix List

| # | Issue | Severity | File | Effort |
|---|---|---|---|---|
| 1 | set_joint_motor hardcoded to AngX | Medium | lib.rs ~line 2820 | 30 lines |
| 2 | joint_state returns UnsupportedCapability (Rapier) | High | lib.rs ~line 2806 | 20 lines |
| 3 | joint_forces returns (0,0) (Rapier) | Low | lib.rs ~line 2850 | 10 lines |
| 4 | ConeTwist mapping incomplete | Medium | lib.rs ~line 2743 | 25 lines |
| 5 | SpringArm maps to FixedJoint | High | lib.rs ~line 2739 | 30 lines |
| 6 | JointState lacks runtime position/velocity fields | Medium | joints.rs | 10 lines |

Total effort: ~125 lines of changes + tests.

---

## Implementation Order

```
Week 1:
  Day 1: Phase 5 (defect fixes) → fixes existing bugs, unlocks P3
  Day 2: Phase 3 (complete joint system, minus Multibody)
  Day 3: Phase 1 (vehicle system)
  Day 4: Phase 2 (contact filter chain)
  Day 5: Phase 4 (profiler)

Week 2:
  Day 1-2: Integration tests, ecs components for vehicle
  Day 3-5: Scripting bindings (Rhai + declarative) for new APIs
```

## Test Plan

Each phase includes:

- **Unit tests**: Backend-level tests for each new method (vehicle creation, step, state query)
- **Integration tests**: Create scene with vehicle + ramps, step 100+ frames, verify position/velocity
- **Regression tests**: All existing 25+ tests must pass unchanged
- **Null backend**: All new methods must return appropriate errors/stubs in NullPhysicsBackend
- **Simple backend**: Vehicle and new joint types return `UnsupportedCapability`

## Feature Flags

| Feature | Gates |
|---|---|
| `runtime-game` | Rapier backend + full vehicle + all joint types |
| `runtime-min` | SimplePhysicsBackend with vehicle/joint stubs |
| `editor` | Rapier + profiler stats display in console |

New vehicle module gated: `#[cfg(feature = "runtime-game")]` in Rapier path, stubs otherwise.

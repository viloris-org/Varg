//! Camera visibility and level-of-detail selection.

use engine_core::math::Vec3;

use crate::{RenderCamera, RenderObject, RenderProjection, RenderWorld};

/// Conservative local-space sphere used for visibility selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderBounds {
    /// Sphere center relative to the render object's transform.
    pub center: Vec3,
    /// Sphere radius before object scale is applied.
    pub radius: f32,
}

impl Default for RenderBounds {
    fn default() -> Self {
        Self {
            center: Vec3::ZERO,
            radius: 1.0,
        }
    }
}

/// One distance-based mesh level.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderLod {
    /// Distance at which this level becomes active.
    pub min_distance: f32,
    /// Mesh identifier selected at or beyond `min_distance`.
    pub mesh: String,
}

/// Visibility and LOD result for one Render World.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VisibilityResult {
    /// Object indices selected for rendering.
    pub visible_indices: Vec<usize>,
    /// Selected mesh for every visible index, including LOD substitution.
    pub selected_meshes: Vec<String>,
    /// Number of objects rejected by the camera frustum.
    pub culled_objects: usize,
}

/// Selects visible objects and their distance-based mesh levels.
pub fn select_visibility(world: &RenderWorld, aspect: f32) -> VisibilityResult {
    let Some(camera) = world.camera.as_ref() else {
        return VisibilityResult {
            visible_indices: (0..world.objects.len()).collect(),
            selected_meshes: world
                .objects
                .iter()
                .map(|object| object.mesh.clone())
                .collect(),
            culled_objects: 0,
        };
    };

    let mut result = VisibilityResult::default();
    for (index, object) in world.objects.iter().enumerate() {
        let distance = (object.transform.translation - camera.transform.translation).length();
        if sphere_visible(camera, object, aspect) {
            result.visible_indices.push(index);
            result.selected_meshes.push(select_lod(object, distance));
        } else {
            result.culled_objects += 1;
        }
    }
    result
}

fn select_lod(object: &RenderObject, distance: f32) -> String {
    object
        .lods
        .iter()
        .filter(|lod| lod.min_distance.is_finite() && distance >= lod.min_distance)
        .max_by(|left, right| left.min_distance.total_cmp(&right.min_distance))
        .map_or_else(|| object.mesh.clone(), |lod| lod.mesh.clone())
}

fn sphere_visible(camera: &RenderCamera, object: &RenderObject, aspect: f32) -> bool {
    let eye = camera.transform.translation;
    let forward = camera_forward(camera);
    let (right, up) = camera_basis(camera, forward);
    let scaled_center = Vec3::new(
        object.bounds.center.x * object.transform.scale.x,
        object.bounds.center.y * object.transform.scale.y,
        object.bounds.center.z * object.transform.scale.z,
    );
    let world_center =
        object.transform.translation + rotate(object.transform.rotation, scaled_center);
    let relative = world_center - eye;
    let depth = relative.dot(forward);
    let scale = object
        .transform
        .scale
        .x
        .abs()
        .max(object.transform.scale.y.abs())
        .max(object.transform.scale.z.abs());
    let radius = object.bounds.radius.max(0.0) * scale.max(0.0001);

    if depth + radius < camera.near || depth - radius > camera.far {
        return false;
    }

    let horizontal = relative.dot(right).abs();
    let vertical = relative.dot(up).abs();
    match camera.projection {
        RenderProjection::Perspective => {
            let half_vertical =
                depth.max(0.0) * (camera.vertical_fov_degrees.to_radians() * 0.5).tan();
            let half_horizontal = half_vertical * aspect.max(0.001);
            horizontal <= half_horizontal + radius && vertical <= half_vertical + radius
        }
        RenderProjection::Orthographic { vertical_size } => {
            let half_vertical = vertical_size.max(0.001) * 0.5;
            let half_horizontal = half_vertical * aspect.max(0.001);
            horizontal <= half_horizontal + radius && vertical <= half_vertical + radius
        }
    }
}

fn camera_forward(camera: &RenderCamera) -> Vec3 {
    if let Some(target) = camera.look_at_target {
        return (target - camera.transform.translation).normalized();
    }
    rotate(camera.transform.rotation, Vec3::new(0.0, 0.0, -1.0)).normalized()
}

fn camera_basis(camera: &RenderCamera, forward: Vec3) -> (Vec3, Vec3) {
    let preferred_up = if camera.look_at_target.is_some() {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        rotate(camera.transform.rotation, Vec3::new(0.0, 1.0, 0.0)).normalized()
    };
    let fallback_up = if forward.y.abs() > 0.99 {
        Vec3::new(0.0, 0.0, 1.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let up_seed = if cross(forward, preferred_up).length_squared() > 1e-8 {
        preferred_up
    } else {
        fallback_up
    };
    let right = cross(forward, up_seed).normalized();
    let up = cross(right, forward).normalized();
    (right, up)
}

fn rotate(rotation: engine_core::math::Quat, vector: Vec3) -> Vec3 {
    let q = Vec3::new(rotation.x, rotation.y, rotation.z);
    let t = cross(q, vector) * 2.0;
    vector + t * rotation.w + cross(q, t)
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

#[cfg(test)]
mod tests {
    use engine_core::{EntityId, math::Transform};

    use super::*;

    fn camera() -> RenderCamera {
        RenderCamera {
            object: EntityId::from_u128(1),
            transform: Transform::IDENTITY,
            projection: RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.1,
            far: 100.0,
            look_at_target: Some(Vec3::new(0.0, 0.0, -1.0)),
        }
    }

    fn object(position: Vec3) -> RenderObject {
        RenderObject {
            object: EntityId::from_u128(2),
            transform: Transform {
                translation: position,
                ..Transform::IDENTITY
            },
            mesh: "near".to_owned(),
            material: String::new(),
            casts_shadows: true,
            receive_shadows: true,
            bounds: RenderBounds::default(),
            lods: vec![RenderLod {
                min_distance: 10.0,
                mesh: "far".to_owned(),
            }],
        }
    }

    #[test]
    fn rejects_objects_behind_camera() {
        let world = RenderWorld {
            camera: Some(camera()),
            objects: vec![object(Vec3::new(0.0, 0.0, 5.0))],
            ..RenderWorld::default()
        };
        let result = select_visibility(&world, 16.0 / 9.0);
        assert!(result.visible_indices.is_empty());
        assert_eq!(result.culled_objects, 1);
    }

    #[test]
    fn selects_far_lod_for_visible_object() {
        let world = RenderWorld {
            camera: Some(camera()),
            objects: vec![object(Vec3::new(0.0, 0.0, -20.0))],
            ..RenderWorld::default()
        };
        let result = select_visibility(&world, 16.0 / 9.0);
        assert_eq!(result.visible_indices, [0]);
        assert_eq!(result.selected_meshes, ["far"]);
    }

    #[test]
    fn keeps_orbit_camera_visible_when_looking_nearly_straight_down() {
        let camera = RenderCamera {
            transform: Transform {
                translation: Vec3::new(0.0, 20.0, 0.01),
                ..Transform::IDENTITY
            },
            look_at_target: Some(Vec3::ZERO),
            ..camera()
        };
        let world = RenderWorld {
            camera: Some(camera),
            objects: vec![object(Vec3::ZERO)],
            ..RenderWorld::default()
        };

        let result = select_visibility(&world, 16.0 / 9.0);

        assert_eq!(result.visible_indices, [0]);
        assert_eq!(result.culled_objects, 0);
    }
}

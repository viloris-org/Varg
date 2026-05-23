#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Skeletal animation: bone hierarchy, skinning matrices, IK solvers, and bone modifiers.

use std::collections::HashMap;

use engine_core::math::{Quat, Transform, Vec3};
use serde::{Deserialize, Serialize};

/// A 4x4 matrix stored in column-major order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [[f32; 4]; 4]);

impl Mat4 {
    /// Identity matrix.
    pub const IDENTITY: Self = Self([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    /// Creates a matrix from a Transform.
    pub fn from_transform(t: Transform) -> Self {
        let (x, y, z, w) = (t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w);
        let (sx, sy, sz) = (t.scale.x, t.scale.y, t.scale.z);
        let (tx, ty, tz) = (t.translation.x, t.translation.y, t.translation.z);

        let x2 = x + x;
        let y2 = y + y;
        let z2 = z + z;
        let xx = x * x2;
        let xy = x * y2;
        let xz = x * z2;
        let yy = y * y2;
        let yz = y * z2;
        let zz = z * z2;
        let wx = w * x2;
        let wy = w * y2;
        let wz = w * z2;

        Self([
            [(1.0 - (yy + zz)) * sx, (xy + wz) * sx, (xz - wy) * sx, 0.0],
            [(xy - wz) * sy, (1.0 - (xx + zz)) * sy, (yz + wx) * sy, 0.0],
            [(xz + wy) * sz, (yz - wx) * sz, (1.0 - (xx + yy)) * sz, 0.0],
            [tx, ty, tz, 1.0],
        ])
    }

    /// Multiplies two matrices.
    pub fn mul(&self, other: &Self) -> Self {
        let mut result = Self::IDENTITY;
        for i in 0..4 {
            for j in 0..4 {
                result.0[j][i] = (0..4)
                    .map(|k| self.0[k][i] * other.0[j][k])
                    .sum();
            }
        }
        result
    }

    /// Inverts this matrix (assumes it's a rigid-body transform).
    pub fn inverse(&self) -> Self {
        let m = &self.0;
        let inv_translation = [
            -(m[3][0] * m[0][0] + m[3][1] * m[0][1] + m[3][2] * m[0][2]),
            -(m[3][0] * m[1][0] + m[3][1] * m[1][1] + m[3][2] * m[1][2]),
            -(m[3][0] * m[2][0] + m[3][1] * m[2][1] + m[3][2] * m[2][2]),
        ];
        Self([
            [m[0][0], m[1][0], m[2][0], 0.0],
            [m[0][1], m[1][1], m[2][1], 0.0],
            [m[0][2], m[1][2], m[2][2], 0.0],
            [
                inv_translation[0],
                inv_translation[1],
                inv_translation[2],
                1.0,
            ],
        ])
    }

    /// Returns this matrix as a flat column-major array of 16 floats.
    pub fn as_array(&self) -> [f32; 16] {
        let mut out = [0.0; 16];
        for col in 0..4 {
            for row in 0..4 {
                out[col * 4 + row] = self.0[col][row];
            }
        }
        out
    }
}

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A single bone in a skeleton.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bone {
    /// Bone name.
    pub name: String,
    /// Parent bone index, if any.
    pub parent: Option<usize>,
    /// Rest pose transform relative to parent.
    pub rest_transform: Transform,
}

/// Skeleton component holding bone hierarchy and skinning data.
#[derive(Clone, Debug, Default)]
pub struct Skeleton {
    /// All bones in the skeleton.
    pub bones: Vec<Bone>,
    /// Bone name to index lookup.
    pub bone_map: HashMap<String, usize>,
    /// Inverse bind matrices for skinning.
    pub inverse_bind_matrices: Vec<Mat4>,
    /// Current animated bone transforms (world space).
    pub bone_transforms: Vec<Transform>,
    /// Final skinning matrices (ready for GPU upload).
    pub skinning_matrices: Vec<Mat4>,
}

impl Skeleton {
    /// Creates an empty skeleton.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a bone and returns its index.
    pub fn add_bone(&mut self, name: impl Into<String>, parent: Option<usize>) -> usize {
        let index = self.bones.len();
        let name = name.into();
        self.bone_map.insert(name.clone(), index);
        self.bones.push(Bone {
            name,
            parent,
            rest_transform: Transform::IDENTITY,
        });
        index
    }

    /// Computes world-space bone transforms from the hierarchy.
    ///
    /// Uses animated transforms from `self.bone_transforms` when available,
    /// falling back to `bone.rest_transform` for bones without animation data.
    pub fn compute_world_transforms(&self) -> Vec<Transform> {
        let mut world = vec![Transform::IDENTITY; self.bones.len()];
        for (i, bone) in self.bones.iter().enumerate() {
            let local = self
                .bone_transforms
                .get(i)
                .copied()
                .unwrap_or(bone.rest_transform);
            world[i] = if let Some(parent_idx) = bone.parent {
                world[parent_idx].compose(&local)
            } else {
                local
            };
        }
        world
    }

    /// Computes skinning matrices from given bone transforms.
    pub fn compute_skinning_matrices(&mut self, animated_transforms: &[Transform]) {
        self.bone_transforms = animated_transforms.to_vec();
        self.skinning_matrices.clear();
        let world = self.compute_world_transforms();
        for (i, _bone) in self.bones.iter().enumerate() {
            let ibm = self
                .inverse_bind_matrices
                .get(i)
                .copied()
                .unwrap_or(Mat4::IDENTITY);
            let world_mat = Mat4::from_transform(world[i]);
            self.skinning_matrices.push(ibm.mul(&world_mat));
        }
    }
}

/// Bone attachment that follows a specific bone.
#[derive(Clone, Debug)]
pub struct BoneAttachment {
    /// Name of the bone to follow.
    pub bone_name: String,
    /// Offset from the bone.
    pub offset: Transform,
}

/// CCD (Cyclic Coordinate Descent) IK solver.
#[derive(Clone, Debug)]
pub struct CCDIKSolver {
    /// Chain of bone indices from end effector to root.
    pub chain: Vec<usize>,
    /// Target position in world space.
    pub target: Vec3,
    /// Convergence tolerance.
    pub tolerance: f32,
    /// Maximum iterations.
    pub max_iterations: u32,
}

impl Default for CCDIKSolver {
    fn default() -> Self {
        Self {
            chain: Vec::new(),
            target: Vec3::ZERO,
            tolerance: 0.001,
            max_iterations: 50,
        }
    }
}

impl CCDIKSolver {
    /// Solves the IK chain to reach the target.
    ///
    /// Returns true if converged within tolerance. Modified bone rotations are
    /// written back to `skeleton.bone_transforms`.
    pub fn solve(&self, skeleton: &mut Skeleton) -> bool {
        let _world = skeleton.compute_world_transforms();
        let mut local_rotations: Vec<Quat> = self
            .chain
            .iter()
            .map(|&i| {
                skeleton
                    .bone_transforms
                    .get(i)
                    .map(|t| t.rotation)
                    .unwrap_or_else(|| {
                        skeleton.bones.get(i).map(|b| b.rest_transform.rotation).unwrap_or(Quat::IDENTITY)
                    })
            })
            .collect();

        for _ in 0..self.max_iterations {
            let world = skeleton.compute_world_transforms();
            let end_idx = *self.chain.first().unwrap_or(&0);
            let end_effector = world
                .get(end_idx)
                .map(|t| t.translation)
                .unwrap_or(Vec3::ZERO);
            let delta = self.target - end_effector;
            if delta.length_squared() < self.tolerance * self.tolerance {
                // Write back converged rotations to skeleton
                for (chain_pos, &bone_idx) in self.chain.iter().enumerate() {
                    if let Some(t) = skeleton.bone_transforms.get_mut(bone_idx) {
                        t.rotation = local_rotations[chain_pos];
                    }
                }
                return true;
            }

            // CCD: rotate each bone from end effector toward root
            for (chain_pos, &bone_idx) in self.chain.iter().enumerate() {
                let joint_pos = world
                    .get(bone_idx)
                    .map(|t| t.translation)
                    .unwrap_or(Vec3::ZERO);
                let to_effector = (end_effector - joint_pos).normalized();
                let to_target = (self.target - joint_pos).normalized();

                let dot = to_effector.dot(to_target);
                if dot > 0.9999 {
                    continue;
                }

                let rotation_axis = to_effector.cross(to_target).normalized();
                let angle = dot.clamp(-1.0, 1.0).acos();
                let delta_rot = Quat::from_axis_angle(rotation_axis, angle);

                local_rotations[chain_pos] = delta_rot * local_rotations[chain_pos];
            }
        }

        // Write final rotations even if not fully converged
        for (chain_pos, &bone_idx) in self.chain.iter().enumerate() {
            if let Some(t) = skeleton.bone_transforms.get_mut(bone_idx) {
                t.rotation = local_rotations[chain_pos];
            }
        }
        false
    }

    /// Sets the chain from bone names.
    pub fn set_chain_bones(
        &mut self,
        skeleton: &Skeleton,
        names: &[&str],
    ) {
        self.chain = names
            .iter()
            .filter_map(|name| skeleton.bone_map.get(*name).copied())
            .collect();
    }
}

/// FABRIK (Forward And Backward Reaching Inverse Kinematics) solver.
#[derive(Clone, Debug)]
pub struct FABRIKSolver {
    /// Chain of bone indices from root to end effector.
    pub chain: Vec<usize>,
    /// Target position in world space.
    pub target: Vec3,
    /// Convergence tolerance.
    pub tolerance: f32,
    /// Maximum iterations.
    pub max_iterations: u32,
}

impl Default for FABRIKSolver {
    fn default() -> Self {
        Self {
            chain: Vec::new(),
            target: Vec3::ZERO,
            tolerance: 0.001,
            max_iterations: 20,
        }
    }
}

impl FABRIKSolver {
    /// Solves the IK chain using FABRIK algorithm.
    ///
    /// Returns true if converged within tolerance. Modified bone positions and
    /// rotations are written back to `skeleton.bone_transforms`.
    pub fn solve(&self, skeleton: &mut Skeleton) -> bool {
        let world = skeleton.compute_world_transforms();
        let mut positions: Vec<Vec3> = self
            .chain
            .iter()
            .map(|&i| world.get(i).map(|t| t.translation).unwrap_or(Vec3::ZERO))
            .collect();
        let bone_lengths: Vec<f32> = positions
            .windows(2)
            .map(|w| (w[1] - w[0]).length())
            .collect();

        let n = positions.len();
        if n == 0 {
            return false;
        }

        let mut converged = false;
        for _ in 0..self.max_iterations {
            positions[n - 1] = self.target;

            for i in (0..n - 1).rev() {
                let dir = (positions[i] - positions[i + 1]).normalized();
                positions[i] = positions[i + 1] + dir * bone_lengths[i];
            }

            let root_world = world
                .get(self.chain[0])
                .map(|t| t.translation)
                .unwrap_or(Vec3::ZERO);
            positions[0] = root_world;

            for i in 0..n - 1 {
                let dir = (positions[i + 1] - positions[i]).normalized();
                positions[i + 1] = positions[i] + dir * bone_lengths[i];
            }

            let end = positions[n - 1];
            if (end - self.target).length_squared() < self.tolerance * self.tolerance {
                converged = true;
                break;
            }
        }

        // Write back computed local transforms to skeleton
        skeleton.bone_transforms.resize(skeleton.bones.len(), Transform::IDENTITY);

        for (chain_pos, &bone_idx) in self.chain.iter().enumerate() {
            let bone_pos = positions[chain_pos];
            let bone_dir = if chain_pos < n - 1 {
                (positions[chain_pos + 1] - bone_pos).normalized()
            } else if chain_pos > 0 {
                (bone_pos - positions[chain_pos - 1]).normalized()
            } else {
                Vec3::new(1.0, 0.0, 0.0)
            };

            let rotation = Quat::from_direction(bone_dir);
            if let Some(t) = skeleton.bone_transforms.get_mut(bone_idx) {
                t.translation = bone_pos;
                t.rotation = rotation;
            }
        }

        converged
    }

    /// Sets the chain from bone names.
    pub fn set_chain_bones(
        &mut self,
        skeleton: &Skeleton,
        names: &[&str],
    ) {
        self.chain = names
            .iter()
            .filter_map(|name| skeleton.bone_map.get(*name).copied())
            .collect();
    }
}

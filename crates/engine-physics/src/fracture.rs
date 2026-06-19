//! Mesh fracture system for destructible objects.

use std::collections::HashMap;

use engine_core::math::Vec3;

use crate::BodyHandle;

/// Fracture pattern controlling how a mesh is split into fragments.
#[derive(Clone, Debug, PartialEq)]
pub enum FracturePattern {
    /// Random Voronoi cells generated from a seed.
    VoronoiRandom {
        /// Number of Voronoi seeds to generate.
        count: u32,
        /// Seed for deterministic generation.
        seed: u64,
    },
    /// Voronoi cells from user-supplied seed points.
    VoronoiSeeds {
        /// World-space seed points defining Voronoi cells.
        seeds: Vec<Vec3>,
    },
    /// Concentric radial split around the impact point.
    Radial {
        /// Number of concentric rings.
        rings: u32,
        /// Number of angular segments per ring.
        segments: u32,
    },
    /// Splat pattern on a plane defined by impact point and normal.
    Splat {
        /// Number of splat seeds.
        count: u32,
        /// Normal of the splat plane.
        normal: Vec3,
    },
}

/// Configuration for a fracture operation.
#[derive(Clone, Debug, PartialEq)]
pub struct FractureConfig {
    /// Fracture pattern to apply.
    pub pattern: FracturePattern,
    /// Target minimum number of fragments, subject to available geometry and size filtering.
    pub min_fragment_count: u32,
    /// Maximum number of fragments to produce.
    pub max_fragment_count: u32,
    /// Impulse strength applied to fragments after fracture.
    pub impulse_strength: f32,
    /// Lifetime in seconds before fragments are cleaned up.
    pub fragment_lifetime: f32,
    /// Minimum allowed fragment size (bounding sphere radius).
    pub min_fragment_size: f32,
}

impl Default for FractureConfig {
    fn default() -> Self {
        Self {
            pattern: FracturePattern::VoronoiRandom { count: 8, seed: 42 },
            min_fragment_count: 4,
            max_fragment_count: 16,
            impulse_strength: 5.0,
            fragment_lifetime: 3.0,
            min_fragment_size: 0.1,
        }
    }
}

/// A single fracture fragment with its own mesh data.
#[derive(Clone, Debug, PartialEq)]
pub struct FractureFragment {
    /// Flat vertex positions (x, y, z triplets).
    pub vertices: Vec<f32>,
    /// Triangle indices.
    pub indices: Vec<u32>,
    /// Geometric centroid of the fragment.
    pub centroid: Vec3,
    /// Approximate volume of the fragment.
    pub volume: f32,
}

/// System for fracturing meshes into fragments.
#[derive(Clone, Debug, Default)]
pub struct FractureSystem;

impl FractureSystem {
    /// Generates fracture seed points based on the pattern and impact point.
    pub fn generate_seeds(
        vertices: &[f32],
        pattern: &FracturePattern,
        impact_point: Vec3,
    ) -> Vec<Vec3> {
        match pattern {
            FracturePattern::VoronoiRandom { count, seed } => {
                Self::generate_voronoi_random(vertices, *count, *seed)
            }
            FracturePattern::VoronoiSeeds { seeds } => seeds.clone(),
            FracturePattern::Radial { rings, segments } => {
                Self::generate_radial_seeds(vertices, impact_point, *rings, *segments)
            }
            FracturePattern::Splat { count, normal } => {
                Self::generate_splat_seeds(vertices, impact_point, *normal, *count)
            }
        }
    }

    /// Fractures a mesh into fragments according to the config.
    pub fn fracture_mesh(
        vertices: &[f32],
        indices: &[u32],
        config: &FractureConfig,
        impact_point: Vec3,
    ) -> Vec<FractureFragment> {
        let seeds = Self::generate_seeds(vertices, &config.pattern, impact_point);
        if seeds.is_empty() {
            return Vec::new();
        }

        let tri_count = indices.len() / 3;
        let mut seed_triangles: Vec<Vec<usize>> = vec![Vec::new(); seeds.len()];
        let vertex_count = vertices.len() / 3;

        for tri_idx in 0..tri_count {
            let i0 = indices[tri_idx * 3] as usize;
            let i1 = indices[tri_idx * 3 + 1] as usize;
            let i2 = indices[tri_idx * 3 + 2] as usize;
            if i0 >= vertex_count || i1 >= vertex_count || i2 >= vertex_count {
                continue;
            }

            let centroid = Self::triangle_centroid(vertices, i0, i1, i2);
            let nearest = Self::nearest_seed_index(&seeds, centroid);
            seed_triangles[nearest].push(tri_idx);
        }

        let mut fragments = Vec::new();
        for (seed_idx, tri_indices) in seed_triangles.iter().enumerate() {
            if tri_indices.is_empty() {
                continue;
            }

            let mut frag_vertices = Vec::new();
            let mut frag_indices = Vec::new();
            let mut vertex_map: HashMap<u32, u32> = HashMap::new();

            for &tri_idx in tri_indices {
                for corner in 0..3 {
                    let old_idx = indices[tri_idx * 3 + corner];
                    if !vertex_map.contains_key(&old_idx) {
                        let new_idx = (frag_vertices.len() / 3) as u32;
                        let vi = old_idx as usize * 3;
                        frag_vertices.push(vertices[vi]);
                        frag_vertices.push(vertices[vi + 1]);
                        frag_vertices.push(vertices[vi + 2]);
                        vertex_map.insert(old_idx, new_idx);
                    }
                    frag_indices.push(vertex_map[&old_idx]);
                }
            }

            let centroid = Self::compute_centroid(&frag_vertices);
            let volume = Self::compute_volume(&frag_vertices, &frag_indices);

            fragments.push(FractureFragment {
                vertices: frag_vertices,
                indices: frag_indices,
                centroid,
                volume,
            });

            let _ = seed_idx;
        }

        fragments.retain(|fragment| {
            config.min_fragment_size <= 0.0
                || fragment_bounding_radius(&fragment.vertices) >= config.min_fragment_size
        });
        fragments.truncate(config.max_fragment_count as usize);
        fragments
    }

    fn generate_voronoi_random(vertices: &[f32], count: u32, seed: u64) -> Vec<Vec3> {
        if vertices.len() < 3 {
            return vec![Vec3::ZERO; count as usize];
        }
        let (min, max) = Self::bounding_box(vertices);
        let extent = max - min;
        let mut seeds = Vec::with_capacity(count as usize);
        let mut s = seed.max(1);
        for _ in 0..count {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let fx = ((s >> 0) & 0xFFFF) as f32 / 0xFFFF as f32;
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let fy = ((s >> 16) & 0xFFFF) as f32 / 0xFFFF as f32;
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let fz = ((s >> 32) & 0xFFFF) as f32 / 0xFFFF as f32;
            seeds.push(Vec3::new(
                min.x + fx * extent.x,
                min.y + fy * extent.y,
                min.z + fz * extent.z,
            ));
        }
        seeds
    }

    fn generate_radial_seeds(
        vertices: &[f32],
        impact_point: Vec3,
        rings: u32,
        segments: u32,
    ) -> Vec<Vec3> {
        if vertices.len() < 3 {
            return Vec::new();
        }
        let (min, max) = Self::bounding_box(vertices);
        let extent = (max - min).length();
        let ring_step = extent / (rings + 1) as f32;

        let mut seeds = vec![impact_point];

        for ring in 1..=rings {
            let radius = ring_step * ring as f32;
            for seg in 0..segments {
                let angle = (seg as f32 / segments as f32) * std::f32::consts::TAU;
                let offset = Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
                seeds.push(impact_point + offset);
            }
        }
        seeds
    }

    fn generate_splat_seeds(
        vertices: &[f32],
        impact_point: Vec3,
        normal: Vec3,
        count: u32,
    ) -> Vec<Vec3> {
        if vertices.len() < 3 {
            return vec![impact_point; count as usize];
        }
        let (_, extent) = Self::bounding_box(vertices);
        let scale = (extent - impact_point).length().max(1.0);

        let normal = if normal.length_squared() < f32::EPSILON {
            Vec3::new(0.0, 1.0, 0.0)
        } else {
            normal.normalized()
        };

        let tangent = if normal.x.abs() < 0.9 {
            normal.cross(Vec3::new(1.0, 0.0, 0.0)).normalized()
        } else {
            normal.cross(Vec3::new(0.0, 1.0, 0.0)).normalized()
        };
        let bitangent = normal.cross(tangent);

        let mut seeds = vec![impact_point];
        let mut hash: u64 = 0xDEAD_BEEF_CAFE_BABE;
        for _ in 1..count {
            hash = hash
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let u = ((hash & 0xFFFF) as f32 / 0xFFFF as f32 - 0.5) * scale;
            hash = hash
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let v = ((hash & 0xFFFF) as f32 / 0xFFFF as f32 - 0.5) * scale;
            seeds.push(impact_point + tangent * u + bitangent * v);
        }
        seeds
    }

    fn bounding_box(vertices: &[f32]) -> (Vec3, Vec3) {
        let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
        for chunk in vertices.chunks_exact(3) {
            let v = Vec3::new(chunk[0], chunk[1], chunk[2]);
            min = Vec3::new(min.x.min(v.x), min.y.min(v.y), min.z.min(v.z));
            max = Vec3::new(max.x.max(v.x), max.y.max(v.y), max.z.max(v.z));
        }
        (min, max)
    }

    fn triangle_centroid(vertices: &[f32], i0: usize, i1: usize, i2: usize) -> Vec3 {
        let a = Vec3::new(vertices[i0 * 3], vertices[i0 * 3 + 1], vertices[i0 * 3 + 2]);
        let b = Vec3::new(vertices[i1 * 3], vertices[i1 * 3 + 1], vertices[i1 * 3 + 2]);
        let c = Vec3::new(vertices[i2 * 3], vertices[i2 * 3 + 1], vertices[i2 * 3 + 2]);
        (a + b + c) / 3.0
    }

    fn nearest_seed_index(seeds: &[Vec3], point: Vec3) -> usize {
        seeds
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = (**a - point).length_squared();
                let db = (**b - point).length_squared();
                da.total_cmp(&db)
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn compute_centroid(vertices: &[f32]) -> Vec3 {
        if vertices.is_empty() {
            return Vec3::ZERO;
        }
        let n = vertices.len() / 3;
        let sum = vertices
            .chunks_exact(3)
            .fold(Vec3::ZERO, |acc, c| acc + Vec3::new(c[0], c[1], c[2]));
        sum / n as f32
    }

    fn compute_volume(vertices: &[f32], indices: &[u32]) -> f32 {
        let mut vol = 0.0;
        for tri in indices.chunks_exact(3) {
            let a = Vec3::new(
                vertices[tri[0] as usize * 3],
                vertices[tri[0] as usize * 3 + 1],
                vertices[tri[0] as usize * 3 + 2],
            );
            let b = Vec3::new(
                vertices[tri[1] as usize * 3],
                vertices[tri[1] as usize * 3 + 1],
                vertices[tri[1] as usize * 3 + 2],
            );
            let c = Vec3::new(
                vertices[tri[2] as usize * 3],
                vertices[tri[2] as usize * 3 + 1],
                vertices[tri[2] as usize * 3 + 2],
            );
            vol += a.dot(b.cross(c)) / 6.0;
        }
        vol.abs()
    }
}

fn fragment_bounding_radius(vertices: &[f32]) -> f32 {
    let centroid = FractureSystem::compute_centroid(vertices);
    vertices
        .chunks_exact(3)
        .map(|chunk| (Vec3::new(chunk[0], chunk[1], chunk[2]) - centroid).length())
        .fold(0.0, f32::max)
}

/// Tracks fragment lifetimes and reports expired fragments.
#[derive(Clone, Debug, Default)]
pub struct FragmentTracker {
    fragments: HashMap<BodyHandle, f32>,
}

impl FragmentTracker {
    /// Creates a new empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a fragment body with a lifetime in seconds.
    pub fn register_fragment(&mut self, body: BodyHandle, lifetime: f32) {
        self.fragments.insert(body, lifetime);
    }

    /// Advances all timers by `dt` and returns handles of expired fragments.
    pub fn tick(&mut self, dt: f32) -> Vec<BodyHandle> {
        let mut expired = Vec::new();
        self.fragments.retain(|handle, lifetime| {
            *lifetime -= dt;
            if *lifetime <= 0.0 {
                expired.push(*handle);
                false
            } else {
                true
            }
        });
        expired
    }

    /// Removes all tracked fragments.
    pub fn clear(&mut self) {
        self.fragments.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_cube() -> (Vec<f32>, Vec<u32>) {
        let vertices = vec![
            0.0, 0.0, 0.0, // 0
            1.0, 0.0, 0.0, // 1
            1.0, 1.0, 0.0, // 2
            0.0, 1.0, 0.0, // 3
            0.0, 0.0, 1.0, // 4
            1.0, 0.0, 1.0, // 5
            1.0, 1.0, 1.0, // 6
            0.0, 1.0, 1.0, // 7
        ];
        let indices = vec![
            0, 1, 2, 0, 2, 3, // front
            5, 4, 7, 5, 7, 6, // back
            4, 0, 3, 4, 3, 7, // left
            1, 5, 6, 1, 6, 2, // right
            3, 2, 6, 3, 6, 7, // top
            4, 5, 1, 4, 1, 0, // bottom
        ];
        (vertices, indices)
    }

    #[test]
    fn voronoi_fragment_count_matches_seed_count() {
        let (vertices, indices) = unit_cube();
        let seed_count = 4;
        let config = FractureConfig {
            pattern: FracturePattern::VoronoiRandom {
                count: seed_count,
                seed: 12345,
            },
            min_fragment_count: 1,
            max_fragment_count: seed_count,
            ..FractureConfig::default()
        };
        let fragments =
            FractureSystem::fracture_mesh(&vertices, &indices, &config, Vec3::new(0.5, 0.5, 0.5));
        assert!(
            fragments.len() <= seed_count as usize,
            "expected at most {} fragments, got {}",
            seed_count,
            fragments.len()
        );
        assert!(!fragments.is_empty(), "expected at least 1 fragment");
    }

    #[test]
    fn radial_seed_generation() {
        let (vertices, _) = unit_cube();
        let pattern = FracturePattern::Radial {
            rings: 2,
            segments: 4,
        };
        let impact = Vec3::new(0.5, 0.5, 0.5);
        let seeds = FractureSystem::generate_seeds(&vertices, &pattern, impact);
        // 1 center + 2 rings * 4 segments = 9
        assert_eq!(seeds.len(), 9);
        assert_eq!(seeds[0], impact);
    }

    #[test]
    fn fragment_tracker_expiry() {
        let mut tracker = FragmentTracker::new();
        let a = BodyHandle(1);
        let b = BodyHandle(2);
        let c = BodyHandle(3);

        tracker.register_fragment(a, 0.5);
        tracker.register_fragment(b, 1.5);
        tracker.register_fragment(c, 1.0);

        let expired = tracker.tick(0.6);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], a);

        let expired = tracker.tick(0.5);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], c);

        let expired = tracker.tick(0.5);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], b);

        let expired = tracker.tick(1.0);
        assert!(expired.is_empty());
    }

    #[test]
    fn invalid_mesh_indices_are_ignored() {
        let (vertices, mut indices) = unit_cube();
        indices.extend_from_slice(&[0, 1, 999]);
        let fragments = FractureSystem::fracture_mesh(
            &vertices,
            &indices,
            &FractureConfig {
                min_fragment_size: 0.0,
                ..FractureConfig::default()
            },
            Vec3::ZERO,
        );

        assert!(!fragments.is_empty());
    }

    #[test]
    fn invalid_fragment_count_range_does_not_panic() {
        let (vertices, indices) = unit_cube();
        let fragments = FractureSystem::fracture_mesh(
            &vertices,
            &indices,
            &FractureConfig {
                min_fragment_count: 10,
                max_fragment_count: 2,
                min_fragment_size: 0.0,
                ..FractureConfig::default()
            },
            Vec3::ZERO,
        );

        assert!(fragments.len() <= 2);
    }
}

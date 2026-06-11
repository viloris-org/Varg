//! Three.js-compatible Geometry types for Rhai scripts.
//!
//! Mirrors `THREE.BoxGeometry`, `THREE.SphereGeometry`, etc.
//! Each type stores parameters; Aster ECS translates them into mesh components.

/// Geometry kind — mirrors `THREE.Geometry` subclasses.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    /// `new THREE.BoxGeometry(width, height, depth)`.
    Box {
        /// Width (X).
        width: f32,
        /// Height (Y).
        height: f32,
        /// Depth (Z).
        depth: f32,
    },
    /// `new THREE.SphereGeometry(radius, widthSegments, heightSegments)`.
    Sphere {
        /// Sphere radius.
        radius: f32,
        /// Horizontal segments.
        width_segments: u32,
        /// Vertical segments.
        height_segments: u32,
    },
    /// `new THREE.PlaneGeometry(width, height)`.
    Plane {
        /// Plane width (X).
        width: f32,
        /// Plane height (Y).
        height: f32,
    },
    /// `new THREE.CylinderGeometry(radiusTop, radiusBottom, height, segments)`.
    Cylinder {
        /// Top radius.
        radius_top: f32,
        /// Bottom radius.
        radius_bottom: f32,
        /// Cylinder height.
        height: f32,
        /// Radial segments.
        segments: u32,
    },
    /// External model file (glTF, OBJ, etc.).
    Model {
        /// Asset path relative to project root.
        path: String,
    },
    /// Capsule geometry (three.js r140+ extension).
    Capsule {
        /// Capsule radius.
        radius: f32,
        /// Capsule height (excluding end caps).
        height: f32,
        /// Radial segments.
        segments: u32,
    },
}

impl Geometry {
    /// `new THREE.BoxGeometry(width, height, depth)`.
    pub fn box_geometry(width: f32, height: f32, depth: f32) -> Self {
        Self::Box {
            width,
            height,
            depth,
        }
    }

    /// `new THREE.SphereGeometry(radius, segments, rings)` (three.js signature).
    pub fn sphere_geometry(radius: f32, width_segments: i64, height_segments: i64) -> Self {
        Self::Sphere {
            radius,
            width_segments: width_segments.max(3) as u32,
            height_segments: height_segments.max(2) as u32,
        }
    }

    /// `new THREE.PlaneGeometry(width, height)`.
    pub fn plane_geometry(width: f32, height: f32) -> Self {
        Self::Plane { width, height }
    }

    /// `new THREE.CylinderGeometry(radiusTop, radiusBottom, height, segments)`.
    pub fn cylinder_geometry(
        radius_top: f32,
        radius_bottom: f32,
        height: f32,
        segments: i64,
    ) -> Self {
        Self::Cylinder {
            radius_top,
            radius_bottom,
            height,
            segments: segments.max(3) as u32,
        }
    }

    /// `new THREE.CapsuleGeometry(radius, height, segments)` (three.js r140+).
    pub fn capsule_geometry(radius: f32, height: f32, segments: i64) -> Self {
        Self::Capsule {
            radius,
            height,
            segments: segments.max(3) as u32,
        }
    }

    /// Load a model file (glTF, OBJ, etc.).
    pub fn model_geometry(path: &str) -> Self {
        Self::Model {
            path: path.to_string(),
        }
    }

    /// Translate into `engine_ecs` component data for mesh rendering.
    /// Returns (primitive type name, params map) for the engine to consume.
    pub fn to_engine_params(&self) -> (String, Vec<(String, f32)>) {
        match self {
            Self::Box {
                width,
                height,
                depth,
            } => (
                "box".to_string(),
                vec![
                    ("width".to_string(), *width),
                    ("height".to_string(), *height),
                    ("depth".to_string(), *depth),
                ],
            ),
            Self::Sphere {
                radius,
                width_segments,
                height_segments,
            } => (
                "sphere".to_string(),
                vec![
                    ("radius".to_string(), *radius),
                    ("width_segments".to_string(), *width_segments as f32),
                    ("height_segments".to_string(), *height_segments as f32),
                ],
            ),
            Self::Plane { width, height } => (
                "plane".to_string(),
                vec![
                    ("width".to_string(), *width),
                    ("height".to_string(), *height),
                ],
            ),
            Self::Cylinder {
                radius_top,
                radius_bottom,
                height,
                segments,
            } => (
                "cylinder".to_string(),
                vec![
                    ("radius_top".to_string(), *radius_top),
                    ("radius_bottom".to_string(), *radius_bottom),
                    ("height".to_string(), *height),
                    ("segments".to_string(), *segments as f32),
                ],
            ),
            Self::Model { path: _ } => ("model".to_string(), vec![]),
            Self::Capsule {
                radius,
                height,
                segments,
            } => (
                "capsule".to_string(),
                vec![
                    ("radius".to_string(), *radius),
                    ("height".to_string(), *height),
                    ("segments".to_string(), *segments as f32),
                ],
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_geometry_creates() {
        let g = Geometry::box_geometry(1.0, 2.0, 3.0);
        assert_eq!(
            g,
            Geometry::Box {
                width: 1.0,
                height: 2.0,
                depth: 3.0,
            }
        );
    }

    #[test]
    fn sphere_geometry_clamps_segments() {
        let g = Geometry::sphere_geometry(5.0, 1, 1);
        assert_eq!(
            g,
            Geometry::Sphere {
                radius: 5.0,
                width_segments: 3,
                height_segments: 2,
            }
        );
    }

    #[test]
    fn to_engine_params_box() {
        let g = Geometry::Box {
            width: 1.0,
            height: 2.0,
            depth: 3.0,
        };
        let (name, params) = g.to_engine_params();
        assert_eq!(name, "box");
        assert_eq!(params[0], ("width".to_string(), 1.0));
        assert_eq!(params[1], ("height".to_string(), 2.0));
        assert_eq!(params[2], ("depth".to_string(), 3.0));
    }
}

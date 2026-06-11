//! Three.js-like scene graph system for declarative scene description.

use serde::{Deserialize, Serialize};

/// Scene container (similar to THREE.Scene).
///
/// AI can generate this JSON to describe complete game scenes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSchema {
    /// Scene name.
    pub name: String,

    /// Root-level objects in the scene graph.
    pub children: Vec<Object3D>,

    /// Environment configuration.
    #[serde(default)]
    pub environment: Environment,
}

/// Base class for all scene objects (similar to THREE.Object3D).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Object3D {
    /// Generic object container.
    Object {
        name: String,
        #[serde(default)]
        position: [f32; 3],
        #[serde(default)]
        rotation: [f32; 3],
        #[serde(default = "default_scale")]
        scale: [f32; 3],
        #[serde(default)]
        children: Vec<Object3D>,
        #[serde(skip_serializing_if = "Option::is_none")]
        behavior: Option<String>,
    },

    /// Mesh with geometry and material (similar to THREE.Mesh).
    Mesh {
        name: String,
        #[serde(default)]
        position: [f32; 3],
        #[serde(default)]
        rotation: [f32; 3],
        #[serde(default = "default_scale")]
        scale: [f32; 3],
        geometry: GeometryRef,
        material: MaterialRef,
        #[serde(default)]
        children: Vec<Object3D>,
        #[serde(skip_serializing_if = "Option::is_none")]
        behavior: Option<String>,
    },

    /// Light source (similar to THREE.Light).
    Light {
        name: String,
        #[serde(default)]
        position: [f32; 3],
        light_type: LightType,
        #[serde(default = "default_white")]
        color: [f32; 3],
        #[serde(default = "default_intensity")]
        intensity: f32,
    },

    /// Camera (similar to THREE.Camera).
    Camera {
        name: String,
        #[serde(default)]
        position: [f32; 3],
        #[serde(default)]
        rotation: [f32; 3],
        camera_type: CameraType,
    },

    /// Group container (similar to THREE.Group).
    Group {
        name: String,
        #[serde(default)]
        position: [f32; 3],
        #[serde(default)]
        rotation: [f32; 3],
        #[serde(default = "default_scale")]
        scale: [f32; 3],
        #[serde(default)]
        children: Vec<Object3D>,
    },
}

/// Light types (similar to THREE.Light subclasses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LightType {
    Directional,
    Point,
    Spot,
    Ambient,
}

/// Camera types (similar to THREE.Camera subclasses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CameraType {
    Perspective { fov: f32, near: f32, far: f32 },
    Orthographic { size: f32, near: f32, far: f32 },
}

/// Material reference (similar to THREE.Material).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MaterialRef {
    /// Basic unlit material.
    Basic {
        color: [f32; 3],
        #[serde(skip_serializing_if = "Option::is_none")]
        map: Option<String>,
    },
    /// PBR standard material.
    Standard {
        color: [f32; 3],
        #[serde(default)]
        metalness: f32,
        #[serde(default = "default_roughness")]
        roughness: f32,
        #[serde(skip_serializing_if = "Option::is_none")]
        map: Option<String>,
    },
    /// Phong shaded material.
    Phong {
        color: [f32; 3],
        specular: [f32; 3],
        #[serde(default = "default_shininess")]
        shininess: f32,
    },
}

/// Geometry reference (similar to THREE.Geometry).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GeometryRef {
    /// Box geometry.
    Box { width: f32, height: f32, depth: f32 },
    /// Sphere geometry.
    Sphere {
        radius: f32,
        #[serde(default = "default_segments")]
        segments: u32,
    },
    /// Plane geometry.
    Plane { width: f32, height: f32 },
    /// Cylinder geometry.
    Cylinder { radius: f32, height: f32 },
    /// External model file.
    Model { path: String },
}

/// Environment configuration (similar to THREE.Scene.fog, etc).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Environment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skybox: Option<String>,

    #[serde(default = "default_ambient")]
    pub ambient_light: [f32; 3],

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog: Option<FogConfig>,
}

/// Fog configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FogConfig {
    pub density: f32,
    pub color: [f32; 3],
}

// Default value functions
fn default_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

fn default_white() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

fn default_intensity() -> f32 {
    1.0
}

fn default_roughness() -> f32 {
    0.5
}

fn default_shininess() -> f32 {
    30.0
}

fn default_segments() -> u32 {
    32
}

fn default_ambient() -> [f32; 3] {
    [0.2, 0.2, 0.2]
}

impl SceneSchema {
    /// Validates the scene schema.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("Scene name cannot be empty".to_string());
        }

        // Validate all objects recursively
        for child in &self.children {
            self.validate_object(child)?;
        }

        Ok(())
    }

    fn validate_object(&self, obj: &Object3D) -> Result<(), String> {
        match obj {
            Object3D::Object { children, .. }
            | Object3D::Mesh { children, .. }
            | Object3D::Group { children, .. } => {
                for child in children {
                    self.validate_object(child)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_schema_validates() {
        let scene = SceneSchema {
            name: "TestScene".to_string(),
            children: vec![],
            environment: Environment::default(),
        };

        assert!(scene.validate().is_ok());
    }

    #[test]
    fn scene_with_mesh_serializes() {
        let scene = SceneSchema {
            name: "MeshScene".to_string(),
            children: vec![Object3D::Mesh {
                name: "Cube".to_string(),
                position: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                geometry: GeometryRef::Box {
                    width: 1.0,
                    height: 1.0,
                    depth: 1.0,
                },
                material: MaterialRef::Basic {
                    color: [1.0, 0.0, 0.0],
                    map: None,
                },
                children: vec![],
                behavior: None,
            }],
            environment: Environment::default(),
        };

        let json = serde_json::to_string_pretty(&scene).unwrap();
        assert!(json.contains("Cube"));
        assert!(json.contains("Mesh"));
    }
}

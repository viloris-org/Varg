//! Three.js-compatible Material types for Rhai scripts.
//!
//! Mirrors `THREE.MeshBasicMaterial`, `THREE.MeshStandardMaterial`,
//! `THREE.MeshPhongMaterial`. Colors are `[r, g, b]` arrays (0-1 range),
//! matching three.js convention.

use rhai::Array;

/// Three.js-compatible material.
#[derive(Clone, Debug, PartialEq)]
pub enum Material {
    /// `new THREE.MeshBasicMaterial({ color, map, ... })` — unlit.
    Basic {
        color: [f32; 3],
        map: Option<String>,
        transparent: bool,
        opacity: f32,
    },
    /// `new THREE.MeshStandardMaterial({ color, roughness, metalness, map, ... })` — PBR.
    Standard {
        color: [f32; 3],
        roughness: f32,
        metalness: f32,
        map: Option<String>,
        normal_map: Option<String>,
        transparent: bool,
        opacity: f32,
    },
    /// `new THREE.MeshPhongMaterial({ color, specular, shininess, map, ... })` — Blinn-Phong.
    Phong {
        color: [f32; 3],
        specular: [f32; 3],
        shininess: f32,
        map: Option<String>,
        transparent: bool,
        opacity: f32,
    },
}

impl Material {
    /// `new THREE.MeshBasicMaterial({ color: [r, g, b] })`.
    ///
    /// Accepts a Rhai object map for three.js compatibility:
    /// ```rhai
    /// let mat = MeshBasicMaterial(#{
    ///     color: [1.0, 0.0, 0.0],
    ///     map: "textures/diffuse.png",
    /// });
    /// ```
    pub fn mesh_basic_material(props: rhai::Map) -> Self {
        Self::Basic {
            color: get_color(&props, "color", [1.0, 1.0, 1.0]),
            map: get_string(&props, "map"),
            transparent: get_bool(&props, "transparent", false),
            opacity: get_float(&props, "opacity", 1.0),
        }
    }

    /// `new THREE.MeshStandardMaterial({ color, roughness, metalness })`.
    pub fn mesh_standard_material(props: rhai::Map) -> Self {
        Self::Standard {
            color: get_color(&props, "color", [1.0, 1.0, 1.0]),
            roughness: get_float(&props, "roughness", 0.5).clamp(0.0, 1.0),
            metalness: get_float(&props, "metalness", 0.0).clamp(0.0, 1.0),
            map: get_string(&props, "map"),
            normal_map: get_string(&props, "normalMap"),
            transparent: get_bool(&props, "transparent", false),
            opacity: get_float(&props, "opacity", 1.0),
        }
    }

    /// `new THREE.MeshPhongMaterial({ color, specular, shininess })`.
    pub fn mesh_phong_material(props: rhai::Map) -> Self {
        Self::Phong {
            color: get_color(&props, "color", [1.0, 1.0, 1.0]),
            specular: get_color(&props, "specular", [0.3, 0.3, 0.3]),
            shininess: get_float(&props, "shininess", 30.0).max(0.0),
            map: get_string(&props, "map"),
            transparent: get_bool(&props, "transparent", false),
            opacity: get_float(&props, "opacity", 1.0),
        }
    }

    /// Convenience: red basic material.
    pub fn basic_red() -> Self {
        Self::Basic {
            color: [1.0, 0.0, 0.0],
            map: None,
            transparent: false,
            opacity: 1.0,
        }
    }

    /// Convenience: green basic material.
    pub fn basic_green() -> Self {
        Self::Basic {
            color: [0.0, 1.0, 0.0],
            map: None,
            transparent: false,
            opacity: 1.0,
        }
    }

    /// Convenience: blue basic material.
    pub fn basic_blue() -> Self {
        Self::Basic {
            color: [0.0, 0.0, 1.0],
            map: None,
            transparent: false,
            opacity: 1.0,
        }
    }
}

// ── Helpers for extracting values from Rhai object maps ──

fn get_color(props: &rhai::Map, key: &str, default: [f32; 3]) -> [f32; 3] {
    if let Some(val) = props.get(key) {
        if let Some(arr) = val.clone().try_cast::<Array>() {
            let r = arr
                .first()
                .and_then(|v| v.as_float().ok())
                .unwrap_or(default[0] as f64) as f32;
            let g = arr
                .get(1)
                .and_then(|v| v.as_float().ok())
                .unwrap_or(default[1] as f64) as f32;
            let b = arr
                .get(2)
                .and_then(|v| v.as_float().ok())
                .unwrap_or(default[2] as f64) as f32;
            return [r, g, b];
        }
    }
    default
}

fn get_float(props: &rhai::Map, key: &str, default: f32) -> f32 {
    props
        .get(key)
        .and_then(|v| v.as_float().ok())
        .unwrap_or(default as f64) as f32
}

fn get_string(props: &rhai::Map, key: &str) -> Option<String> {
    props.get(key).and_then(|v| v.clone().try_cast::<String>())
}

fn get_bool(props: &rhai::Map, key: &str, default: bool) -> bool {
    props
        .get(key)
        .and_then(|v| v.as_bool().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_map(entries: Vec<(&str, rhai::Dynamic)>) -> rhai::Map {
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string().into(), v))
            .collect()
    }

    #[test]
    fn basic_material_defaults() {
        let mat = Material::mesh_basic_material(rhai::Map::new());
        assert_eq!(
            mat,
            Material::Basic {
                color: [1.0, 1.0, 1.0],
                map: None,
                transparent: false,
                opacity: 1.0,
            }
        );
    }

    #[test]
    fn standard_material_with_props() {
        let props = make_map(vec![
            (
                "color",
                rhai::Dynamic::from(rhai::Array::from([
                    rhai::Dynamic::from(1.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                    rhai::Dynamic::from(0.0_f64),
                ])),
            ),
            ("roughness", rhai::Dynamic::from(0.3_f64)),
            ("metalness", rhai::Dynamic::from(0.8_f64)),
        ]);
        let mat = Material::mesh_standard_material(props);
        assert_eq!(
            mat,
            Material::Standard {
                color: [1.0, 0.0, 0.0],
                roughness: 0.3,
                metalness: 0.8,
                map: None,
                normal_map: None,
                transparent: false,
                opacity: 1.0,
            }
        );
    }
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::CombineMode;

/// A physical material asset defining surface properties.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct PhysicalMaterial {
    /// Asset name.
    pub name: String,
    /// Friction coefficient.
    pub friction: f32,
    /// Restitution (bounciness) coefficient.
    pub restitution: f32,
    /// Density in kg/m³ (used for mass computation).
    pub density: f32,
    /// Friction combine mode override.
    pub friction_combine: CombineMode,
    /// Restitution combine mode override.
    pub restitution_combine: CombineMode,
}

impl Default for PhysicalMaterial {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            friction: 0.5,
            restitution: 0.0,
            density: 1000.0,
            friction_combine: CombineMode::Average,
            restitution_combine: CombineMode::Average,
        }
    }
}

/// Registry of named physical materials.
#[derive(Clone, Debug, Default)]
pub struct PhysicalMaterialRegistry {
    materials: HashMap<String, PhysicalMaterial>,
}

impl PhysicalMaterialRegistry {
    /// Creates a registry with built-in materials.
    pub fn with_defaults() -> Self {
        let mut reg = Self::default();
        reg.register(PhysicalMaterial::default());
        reg.register(PhysicalMaterial {
            name: "Wood".into(),
            friction: 0.4,
            restitution: 0.2,
            density: 700.0,
            ..PhysicalMaterial::default()
        });
        reg.register(PhysicalMaterial {
            name: "Metal".into(),
            friction: 0.6,
            restitution: 0.1,
            density: 7800.0,
            ..PhysicalMaterial::default()
        });
        reg.register(PhysicalMaterial {
            name: "Rubber".into(),
            friction: 0.9,
            restitution: 0.8,
            density: 1100.0,
            ..PhysicalMaterial::default()
        });
        reg.register(PhysicalMaterial {
            name: "Ice".into(),
            friction: 0.05,
            restitution: 0.05,
            density: 917.0,
            ..PhysicalMaterial::default()
        });
        reg
    }

    /// Registers a physical material.
    pub fn register(&mut self, material: PhysicalMaterial) {
        self.materials.insert(material.name.clone(), material);
    }

    /// Looks up a material by name.
    pub fn get(&self, name: &str) -> Option<&PhysicalMaterial> {
        self.materials.get(name).or_else(|| {
            self.materials
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
                .map(|(_, material)| material)
        })
    }

    /// Returns the friction for a named material, or the default.
    pub fn friction(&self, name: &str) -> f32 {
        self.get(name).map_or(0.5, |m| m.friction)
    }

    /// Returns the restitution for a named material, or the default.
    pub fn restitution(&self, name: &str) -> f32 {
        self.get(name).map_or(0.0, |m| m.restitution)
    }

    /// Returns the density for a named material, or the default.
    pub fn density(&self, name: &str) -> f32 {
        self.get(name).map_or(1000.0, |m| m.density)
    }
}

/// Resolves a named built-in physical material, falling back to [`PhysicalMaterial::default`].
pub fn built_in_physical_material(name: &str) -> PhysicalMaterial {
    PhysicalMaterialRegistry::with_defaults()
        .get(name)
        .cloned()
        .unwrap_or_default()
}

//! Game systems configuration.
//!
//! Declarative configuration for game systems like combat, economy, progression, etc.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Game systems configuration schema.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemsConfig {
    /// Combat system configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub combat: Option<CombatSystem>,

    /// Economy/currency system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub economy: Option<EconomySystem>,

    /// Player progression/leveling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progression: Option<ProgressionSystem>,

    /// Physics settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physics: Option<PhysicsSystem>,

    /// Audio settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioSystem>,

    /// Custom system parameters (key-value pairs).
    #[serde(default)]
    pub custom: HashMap<String, SystemParam>,
}

/// Combat system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatSystem {
    /// Global damage multiplier.
    #[serde(default = "default_one")]
    pub damage_multiplier: f32,

    /// Enable friendly fire.
    #[serde(default)]
    pub friendly_fire: bool,

    /// Critical hit chance (0.0-1.0).
    #[serde(default)]
    pub crit_chance: f32,

    /// Critical hit multiplier.
    #[serde(default = "default_crit_multiplier")]
    pub crit_multiplier: f32,

    /// Invincibility duration after taking damage (seconds).
    #[serde(default)]
    pub invincibility_duration: f32,
}

/// Economy system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomySystem {
    /// Starting currency amount.
    #[serde(default = "default_starting_currency")]
    pub starting_currency: u32,

    /// Currency name (e.g., "Gold", "Credits").
    #[serde(default = "default_currency_name")]
    pub currency_name: String,

    /// Price scaling factor.
    #[serde(default = "default_one")]
    pub price_multiplier: f32,

    /// Enable currency drops from enemies.
    #[serde(default = "default_true")]
    pub currency_drops: bool,
}

/// Progression system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressionSystem {
    /// XP curve type.
    #[serde(default)]
    pub xp_curve: XPCurve,

    /// Base XP required for level 2.
    #[serde(default = "default_base_xp")]
    pub base_xp: u32,

    /// XP multiplier per level.
    #[serde(default = "default_xp_multiplier")]
    pub xp_multiplier: f32,

    /// Maximum level (0 = no cap).
    #[serde(default)]
    pub max_level: u32,

    /// Stats gained per level.
    #[serde(default)]
    pub stats_per_level: StatsPerLevel,
}

/// XP curve types.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum XPCurve {
    /// Linear scaling (base * level).
    #[default]
    Linear,
    /// Exponential scaling (base * multiplier^level).
    Exponential,
    /// Custom formula.
    Custom(String),
}

/// Stats gained per level up.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatsPerLevel {
    #[serde(default)]
    pub health: f32,
    #[serde(default)]
    pub damage: f32,
    #[serde(default)]
    pub speed: f32,
}

/// Physics system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsSystem {
    /// Gravity vector [x, y, z].
    #[serde(default = "default_gravity")]
    pub gravity: [f32; 3],

    /// Fixed timestep for physics updates (seconds).
    #[serde(default = "default_fixed_timestep")]
    pub fixed_timestep: f32,

    /// Enable debug visualization.
    #[serde(default)]
    pub debug_draw: bool,
}

/// Audio system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSystem {
    /// Master volume (0.0-1.0).
    #[serde(default = "default_volume")]
    pub master_volume: f32,

    /// Music volume (0.0-1.0).
    #[serde(default = "default_volume")]
    pub music_volume: f32,

    /// SFX volume (0.0-1.0).
    #[serde(default = "default_volume")]
    pub sfx_volume: f32,

    /// Enable 3D positional audio.
    #[serde(default = "default_true")]
    pub positional_audio: bool,
}

/// Generic system parameter (for custom systems).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemParam {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<SystemParam>),
    Object(HashMap<String, SystemParam>),
}

// Default value functions
fn default_one() -> f32 {
    1.0
}

fn default_crit_multiplier() -> f32 {
    2.0
}

fn default_starting_currency() -> u32 {
    100
}

fn default_currency_name() -> String {
    "Gold".to_string()
}

fn default_true() -> bool {
    true
}

fn default_base_xp() -> u32 {
    100
}

fn default_xp_multiplier() -> f32 {
    1.5
}

fn default_gravity() -> [f32; 3] {
    [0.0, -9.81, 0.0]
}

fn default_fixed_timestep() -> f32 {
    1.0 / 60.0
}

fn default_volume() -> f32 {
    1.0
}

impl SystemsConfig {
    /// Validates the systems configuration.
    pub fn validate(&self) -> Result<(), String> {
        // Validate combat settings
        if let Some(combat) = &self.combat {
            if combat.crit_chance < 0.0 || combat.crit_chance > 1.0 {
                return Err("Crit chance must be between 0.0 and 1.0".to_string());
            }
        }

        // Validate audio settings
        if let Some(audio) = &self.audio {
            if audio.master_volume < 0.0 || audio.master_volume > 1.0 {
                return Err("Master volume must be between 0.0 and 1.0".to_string());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systems_config_validates() {
        let config = SystemsConfig {
            combat: Some(CombatSystem {
                damage_multiplier: 1.5,
                friendly_fire: false,
                crit_chance: 0.1,
                crit_multiplier: 2.0,
                invincibility_duration: 1.0,
            }),
            economy: None,
            progression: None,
            physics: None,
            audio: None,
            custom: HashMap::new(),
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn full_config_serializes() {
        let config = SystemsConfig {
            combat: Some(CombatSystem {
                damage_multiplier: 1.0,
                friendly_fire: false,
                crit_chance: 0.05,
                crit_multiplier: 2.0,
                invincibility_duration: 0.5,
            }),
            economy: Some(EconomySystem {
                starting_currency: 100,
                currency_name: "Gold".to_string(),
                price_multiplier: 1.0,
                currency_drops: true,
            }),
            progression: None,
            physics: Some(PhysicsSystem {
                gravity: [0.0, -9.81, 0.0],
                fixed_timestep: 1.0 / 60.0,
                debug_draw: false,
            }),
            audio: None,
            custom: HashMap::new(),
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(json.contains("combat"));
        assert!(json.contains("economy"));
    }
}

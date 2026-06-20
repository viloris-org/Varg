//! Concrete resource types implementing the Resource trait.

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

use crate::resource_trait::Resource;

/// Font resource containing font data and configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontResource {
    /// Font family name.
    pub family: String,
    /// Font style (e.g., "Regular", "Bold", "Italic").
    pub style: String,
    /// Font size in points.
    pub size: f32,
    /// Raw font file bytes.
    #[serde(skip)]
    pub data: Vec<u8>,
}

impl Default for FontResource {
    fn default() -> Self {
        Self {
            family: "default".to_string(),
            style: "Regular".to_string(),
            size: 16.0,
            data: Vec::new(),
        }
    }
}

impl Resource for FontResource {
    fn type_name() -> &'static str {
        "FontResource"
    }

    fn to_json(&self) -> EngineResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| EngineError::other(format!("FontResource serialization failed: {e}")))
    }

    fn from_json(input: &str) -> EngineResult<Self> {
        serde_json::from_str(input)
            .map_err(|e| EngineError::other(format!("FontResource parse failed: {e}")))
    }

    fn to_binary(&self) -> EngineResult<Vec<u8>> {
        binary_serialize(self, "FontResource")
    }

    fn from_binary(bytes: &[u8]) -> EngineResult<Self> {
        binary_deserialize(bytes, "FontResource")
    }

    fn preview_summary(&self) -> String {
        format!("{} {} ({}pt)", self.family, self.style, self.size)
    }
}

/// Curve resource for animation easing and material parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurveResource {
    /// Curve name.
    pub name: String,
    /// Keyframe points (time in [0.0, 1.0], value).
    pub points: Vec<CurvePoint>,
    /// Loop mode.
    pub loop_mode: CurveLoopMode,
}

/// A single point on a curve.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    /// Normalized time in [0.0, 1.0].
    pub time: f32,
    /// Value at this point.
    pub value: f32,
    /// Left tangent for cubic interpolation.
    #[serde(default)]
    pub left_tangent: f32,
    /// Right tangent for cubic interpolation.
    #[serde(default)]
    pub right_tangent: f32,
}

/// Curve loop behavior.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum CurveLoopMode {
    /// Clamp to endpoints.
    #[default]
    Clamp,
    /// Loop back to start.
    Loop,
    /// Ping-pong back and forth.
    PingPong,
}

impl Default for CurveResource {
    fn default() -> Self {
        Self {
            name: String::new(),
            points: Vec::new(),
            loop_mode: CurveLoopMode::Clamp,
        }
    }
}

impl Resource for CurveResource {
    fn type_name() -> &'static str {
        "CurveResource"
    }

    fn to_json(&self) -> EngineResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| EngineError::other(format!("CurveResource serialization failed: {e}")))
    }

    fn from_json(input: &str) -> EngineResult<Self> {
        serde_json::from_str(input)
            .map_err(|e| EngineError::other(format!("CurveResource parse failed: {e}")))
    }

    fn to_binary(&self) -> EngineResult<Vec<u8>> {
        binary_serialize(self, "CurveResource")
    }

    fn from_binary(bytes: &[u8]) -> EngineResult<Self> {
        binary_deserialize(bytes, "CurveResource")
    }

    fn preview_summary(&self) -> String {
        format!("{} ({} points)", self.name, self.points.len())
    }
}

/// Theme resource for UI styling.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeResource {
    /// Theme name.
    pub name: String,
    /// Base background color.
    pub base_color: [f32; 4],
    /// Accent/highlight color.
    pub accent_color: [f32; 4],
    /// Text color.
    pub text_color: [f32; 4],
    /// Color for disabled elements.
    pub disabled_color: [f32; 4],
    /// Default font size.
    pub font_size: f32,
    /// Default spacing.
    pub spacing: f32,
    /// Named color palette entries.
    #[serde(default)]
    pub palette: Vec<(String, [f32; 4])>,
}

impl Default for ThemeResource {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            base_color: [0.15, 0.15, 0.15, 1.0],
            accent_color: [0.3, 0.6, 1.0, 1.0],
            text_color: [1.0, 1.0, 1.0, 1.0],
            disabled_color: [0.5, 0.5, 0.5, 0.5],
            font_size: 14.0,
            spacing: 8.0,
            palette: Vec::new(),
        }
    }
}

impl Resource for ThemeResource {
    fn type_name() -> &'static str {
        "ThemeResource"
    }

    fn to_json(&self) -> EngineResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| EngineError::other(format!("ThemeResource serialization failed: {e}")))
    }

    fn from_json(input: &str) -> EngineResult<Self> {
        serde_json::from_str(input)
            .map_err(|e| EngineError::other(format!("ThemeResource parse failed: {e}")))
    }

    fn to_binary(&self) -> EngineResult<Vec<u8>> {
        binary_serialize(self, "ThemeResource")
    }

    fn from_binary(bytes: &[u8]) -> EngineResult<Self> {
        binary_deserialize(bytes, "ThemeResource")
    }

    fn preview_summary(&self) -> String {
        format!(
            "Theme: {} ({} palette entries)",
            self.name,
            self.palette.len()
        )
    }
}

/// Input map resource for data-driven input binding configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputMapResource {
    /// Map name.
    pub name: String,
    /// Action definitions.
    pub actions: Vec<InputActionDef>,
}

/// A named input action with its bindings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputActionDef {
    /// Action name.
    pub name: String,
    /// Keyboard bindings.
    #[serde(default)]
    pub keys: Vec<String>,
    /// Mouse button bindings.
    #[serde(default)]
    pub mouse_buttons: Vec<String>,
    /// Gamepad button bindings.
    #[serde(default)]
    pub gamepad_buttons: Vec<String>,
    /// Gamepad axis bindings.
    #[serde(default)]
    pub gamepad_axes: Vec<String>,
    /// Dead zone for analog input.
    #[serde(default = "default_deadzone")]
    pub deadzone: f32,
}

fn default_deadzone() -> f32 {
    0.2
}

impl Default for InputMapResource {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            actions: Vec::new(),
        }
    }
}

impl Resource for InputMapResource {
    fn type_name() -> &'static str {
        "InputMapResource"
    }

    fn to_json(&self) -> EngineResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| EngineError::other(format!("InputMapResource serialization failed: {e}")))
    }

    fn from_json(input: &str) -> EngineResult<Self> {
        serde_json::from_str(input)
            .map_err(|e| EngineError::other(format!("InputMapResource parse failed: {e}")))
    }

    fn to_binary(&self) -> EngineResult<Vec<u8>> {
        binary_serialize(self, "InputMapResource")
    }

    fn from_binary(bytes: &[u8]) -> EngineResult<Self> {
        binary_deserialize(bytes, "InputMapResource")
    }

    fn preview_summary(&self) -> String {
        format!("InputMap: {} ({} actions)", self.name, self.actions.len())
    }
}

fn binary_serialize<T: Serialize>(value: &T, name: &str) -> EngineResult<Vec<u8>> {
    serde_json::to_vec(value)
        .map_err(|e| EngineError::other(format!("{name} binary serialization failed: {e}")))
}

fn binary_deserialize<T: for<'de> Deserialize<'de>>(bytes: &[u8], name: &str) -> EngineResult<T> {
    serde_json::from_slice(bytes)
        .map_err(|e| EngineError::other(format!("{name} binary parse failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ResourceTypeRegistry;

    #[test]
    fn font_resource_roundtrip_json() {
        let font = FontResource {
            family: "Arial".to_string(),
            style: "Bold".to_string(),
            size: 24.0,
            data: vec![1, 2, 3],
        };
        let json = font.to_json().unwrap();
        let loaded = FontResource::from_json(&json).unwrap();
        assert_eq!(font.family, loaded.family);
        assert_eq!(font.style, loaded.style);
        assert_eq!(font.size, loaded.size);
        assert_eq!(font.preview_summary(), "Arial Bold (24pt)");
    }

    #[test]
    fn font_resource_roundtrip_binary() {
        let font = FontResource {
            family: "Roboto".to_string(),
            style: "Regular".to_string(),
            size: 16.0,
            data: vec![],
        };
        let binary = font.to_binary().unwrap();
        let loaded = FontResource::from_binary(&binary).unwrap();
        assert_eq!(font.family, loaded.family);
    }

    #[test]
    fn curve_resource_roundtrip() {
        let curve = CurveResource {
            name: "test".to_string(),
            points: vec![
                CurvePoint {
                    time: 0.0,
                    value: 0.0,
                    left_tangent: 0.0,
                    right_tangent: 1.0,
                },
                CurvePoint {
                    time: 1.0,
                    value: 1.0,
                    left_tangent: -1.0,
                    right_tangent: 0.0,
                },
            ],
            loop_mode: CurveLoopMode::PingPong,
        };
        let json = curve.to_json().unwrap();
        let loaded = CurveResource::from_json(&json).unwrap();
        assert_eq!(loaded.points.len(), 2);
        assert_eq!(loaded.loop_mode, CurveLoopMode::PingPong);
    }

    #[test]
    fn theme_resource_roundtrip() {
        let theme = ThemeResource::default();
        let json = theme.to_json().unwrap();
        let loaded = ThemeResource::from_json(&json).unwrap();
        assert_eq!(loaded.name, "Default");
        assert_eq!(loaded.font_size, 14.0);
        assert_eq!(loaded.spacing, 8.0);
    }

    #[test]
    fn input_map_resource_roundtrip() {
        let map = InputMapResource {
            name: "Player".to_string(),
            actions: vec![InputActionDef {
                name: "Jump".to_string(),
                keys: vec!["Space".to_string()],
                mouse_buttons: vec![],
                gamepad_buttons: vec!["A".to_string()],
                gamepad_axes: vec!["LeftStickX".to_string()],
                deadzone: 0.2,
            }],
        };
        let json = map.to_json().unwrap();
        let loaded = InputMapResource::from_json(&json).unwrap();
        assert_eq!(loaded.actions.len(), 1);
        assert_eq!(loaded.actions[0].name, "Jump");
    }

    #[test]
    fn registry_register_and_validate() {
        let mut registry = ResourceTypeRegistry::default();
        registry.register::<FontResource>();
        registry.register::<ThemeResource>();

        assert!(registry.contains("FontResource"));
        assert!(registry.contains("ThemeResource"));
        assert!(!registry.contains("UnknownType"));
        assert_eq!(registry.len(), 2);

        let theme = ThemeResource::default();
        let json = theme.to_json().unwrap();
        assert!(registry.validate_json("ThemeResource", &json).is_ok());
        assert!(registry.validate_json("FontResource", &json).is_err());
    }
}

//! UI layout and element system.
//!
//! Declarative UI description for game interfaces, menus, HUD elements, etc.

use serde::{Deserialize, Serialize};

/// UI layout schema (root container).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UISchema {
    /// UI name/identifier.
    pub name: String,

    /// Root layout type.
    #[serde(default)]
    pub layout: LayoutType,

    /// UI elements tree.
    pub elements: Vec<UIElement>,

    /// Data bindings (connect UI to game state).
    #[serde(default)]
    pub bindings: Vec<DataBinding>,
}

/// Layout types for containers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum LayoutType {
    /// Anchored positioning (absolute coordinates with anchors).
    #[default]
    Anchored,
    /// Vertical stack layout.
    Vertical,
    /// Horizontal stack layout.
    Horizontal,
    /// Grid layout.
    Grid { columns: u32, rows: u32 },
}

/// UI element (similar to HTML/React components).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UIElement {
    /// Text label.
    Text {
        content: String,
        #[serde(default)]
        style: TextStyle,
        #[serde(default)]
        position: Position,
    },

    /// Button with action callback.
    Button {
        text: String,
        action: String, // Action identifier
        #[serde(default)]
        style: ButtonStyle,
        #[serde(default)]
        position: Position,
    },

    /// Health/progress bar.
    Bar {
        binding: String, // e.g., "player.health"
        #[serde(default)]
        style: BarStyle,
        #[serde(default)]
        position: Position,
    },

    /// Image/icon display.
    Image {
        path: String,
        #[serde(default)]
        size: [f32; 2],
        #[serde(default)]
        position: Position,
    },

    /// Container with child elements.
    Container {
        #[serde(default)]
        layout: LayoutType,
        children: Vec<UIElement>,
        #[serde(default)]
        style: ContainerStyle,
        #[serde(default)]
        position: Position,
    },

    /// Input field (text entry).
    Input {
        placeholder: String,
        binding: String,
        #[serde(default)]
        style: InputStyle,
        #[serde(default)]
        position: Position,
    },

    /// Slider control.
    Slider {
        binding: String,
        min: f32,
        max: f32,
        #[serde(default)]
        step: f32,
        #[serde(default)]
        position: Position,
    },
}

/// Position with anchoring (similar to CSS positioning).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Position {
    #[serde(default)]
    pub anchor: Anchor,
    #[serde(default)]
    pub offset: [f32; 2],
}

/// Anchor point (like CSS position + alignment).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum Anchor {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Text styling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextStyle {
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_white")]
    pub color: [f32; 4],
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            color: [1.0, 1.0, 1.0, 1.0],
            font: None,
            bold: false,
            italic: false,
        }
    }
}

/// Button styling.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ButtonStyle {
    #[serde(default = "default_button_bg")]
    pub background: [f32; 4],
    #[serde(default = "default_white")]
    pub text_color: [f32; 4],
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_button_padding")]
    pub padding: [f32; 2],
}

/// Bar (health/progress) styling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarStyle {
    #[serde(default = "default_bar_size")]
    pub size: [f32; 2],
    #[serde(default = "default_green")]
    pub fill_color: [f32; 4],
    #[serde(default = "default_gray")]
    pub background_color: [f32; 4],
    #[serde(default)]
    pub border: bool,
}

impl Default for BarStyle {
    fn default() -> Self {
        Self {
            size: [100.0, 20.0],
            fill_color: [0.0, 1.0, 0.0, 1.0],
            background_color: [0.3, 0.3, 0.3, 1.0],
            border: true,
        }
    }
}

/// Container styling.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<[f32; 4]>,
    #[serde(default)]
    pub padding: [f32; 4],
    #[serde(default)]
    pub border: bool,
}

/// Input field styling.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InputStyle {
    #[serde(default = "default_input_size")]
    pub size: [f32; 2],
    #[serde(default = "default_white")]
    pub text_color: [f32; 4],
    #[serde(default = "default_input_bg")]
    pub background: [f32; 4],
}

/// Data binding (connect UI to game state).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBinding {
    /// Binding identifier (e.g., "player.health").
    pub id: String,
    /// Path to game data (e.g., "entities.player.components.health.current").
    pub source: String,
}

// Default value functions
fn default_font_size() -> f32 {
    16.0
}

fn default_white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_button_bg() -> [f32; 4] {
    [0.2, 0.2, 0.8, 1.0]
}

fn default_button_padding() -> [f32; 2] {
    [10.0, 5.0]
}

fn default_green() -> [f32; 4] {
    [0.0, 1.0, 0.0, 1.0]
}

fn default_gray() -> [f32; 4] {
    [0.3, 0.3, 0.3, 1.0]
}

fn default_bar_size() -> [f32; 2] {
    [100.0, 20.0]
}

fn default_input_size() -> [f32; 2] {
    [200.0, 30.0]
}

fn default_input_bg() -> [f32; 4] {
    [0.1, 0.1, 0.1, 0.9]
}

impl UISchema {
    /// Validates the UI schema.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("UI name cannot be empty".to_string());
        }

        // Validate all elements
        for element in &self.elements {
            self.validate_element(element)?;
        }

        Ok(())
    }

    fn validate_element(&self, element: &UIElement) -> Result<(), String> {
        match element {
            UIElement::Container { children, .. } => {
                for child in children {
                    self.validate_element(child)?;
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
    fn ui_schema_validates() {
        let ui = UISchema {
            name: "GameHUD".to_string(),
            layout: LayoutType::Anchored,
            elements: vec![],
            bindings: vec![],
        };

        assert!(ui.validate().is_ok());
    }

    #[test]
    fn ui_with_health_bar_serializes() {
        let ui = UISchema {
            name: "HUD".to_string(),
            layout: LayoutType::Anchored,
            elements: vec![
                UIElement::Bar {
                    binding: "player.health".to_string(),
                    style: BarStyle::default(),
                    position: Position {
                        anchor: Anchor::TopLeft,
                        offset: [10.0, 10.0],
                    },
                },
                UIElement::Button {
                    text: "Pause".to_string(),
                    action: "pause_game".to_string(),
                    style: ButtonStyle::default(),
                    position: Position {
                        anchor: Anchor::TopRight,
                        offset: [-10.0, 10.0],
                    },
                },
            ],
            bindings: vec![DataBinding {
                id: "player.health".to_string(),
                source: "entities.player.components.health.current".to_string(),
            }],
        };

        let json = serde_json::to_string_pretty(&ui).unwrap();
        assert!(json.contains("HUD"));
        assert!(json.contains("health"));
    }
}

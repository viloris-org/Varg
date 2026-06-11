#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Retained-mode UI system with control tree, layout engine, theme, and rendering.

use std::any::Any;
use std::collections::HashMap;

use engine_core::AssetId;
use serde::{Deserialize, Serialize};

/// 2D vector for UI layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
}

impl Vec2 {
    /// Creates a new Vec2.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Rectangle for UI positioning.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// X position.
    pub x: f32,
    /// Y position.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Margin for UI elements.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margin {
    /// Left margin.
    pub left: f32,
    /// Right margin.
    pub right: f32,
    /// Top margin.
    pub top: f32,
    /// Bottom margin.
    pub bottom: f32,
}

/// Style box types for rendering UI element backgrounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StyleBox {
    /// No background.
    Empty,
    /// Flat color fill.
    Flat {
        /// Background color.
        color: [f32; 4],
        /// Corner radius.
        border_radius: f32,
    },
    /// Textured background.
    Texture {
        /// Texture asset GUID.
        texture: AssetId,
        /// Nine-patch border.
        border: [f32; 4],
    },
}

/// UI theme configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    /// Default font asset GUID.
    pub default_font: Option<AssetId>,
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
    /// Named style boxes.
    #[serde(default)]
    pub styles: HashMap<String, StyleBox>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            default_font: None,
            base_color: [0.15, 0.15, 0.15, 1.0],
            accent_color: [0.3, 0.6, 1.0, 1.0],
            text_color: [1.0, 1.0, 1.0, 1.0],
            disabled_color: [0.5, 0.5, 0.5, 0.5],
            font_size: 14.0,
            spacing: 8.0,
            styles: HashMap::new(),
        }
    }
}

/// UI event types.
#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    /// Mouse moved.
    MouseMove {
        /// X position.
        x: f32,
        /// Y position.
        y: f32,
    },
    /// Mouse button pressed.
    MouseDown {
        /// Button index.
        button: u8,
        /// X position.
        x: f32,
        /// Y position.
        y: f32,
    },
    /// Mouse button released.
    MouseUp {
        /// Button index.
        button: u8,
        /// X position.
        x: f32,
        /// Y position.
        y: f32,
    },
    /// Key pressed.
    KeyDown {
        /// Key name.
        key: String,
    },
    /// Text input.
    TextInput(String),
    /// Scroll.
    Scroll {
        /// Horizontal scroll.
        x: f32,
        /// Vertical scroll.
        y: f32,
    },
}

/// Result of handling a UI event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventResult {
    /// Event was consumed.
    Consumed,
    /// Event was ignored.
    Ignored,
}

/// Layout data for a control node.
#[derive(Clone, Debug, Default)]
struct LayoutData {
    min_size: Vec2,
    position: Vec2,
    margin: Margin,
}

/// Base control node in the UI tree.
pub struct ControlNode {
    /// Control name.
    pub name: String,
    /// Layout data.
    layout: LayoutData,
    /// Child controls.
    pub children: Vec<ControlNode>,
    /// Whether this control is visible.
    pub visible: bool,
    /// Whether this control is enabled.
    pub enabled: bool,
    /// Widget-specific data.
    widget: Box<dyn Widget>,
}

/// Widget trait for UI controls.
pub trait Widget: Any {
    /// Returns the widget type name.
    fn type_name(&self) -> &'static str;
    /// Measures the minimum size.
    fn measure(&self, theme: &Theme) -> Vec2;
    /// Returns the style box for rendering.
    fn style(&self, theme: &Theme) -> StyleBox;
    /// Handles an event.
    fn handle_event(&mut self, event: &UiEvent, theme: &Theme) -> EventResult;
    /// Returns mutable any reference.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// A label widget.
pub struct LabelWidget {
    /// Label text.
    pub text: String,
}

impl Widget for LabelWidget {
    fn type_name(&self) -> &'static str {
        "Label"
    }

    fn measure(&self, theme: &Theme) -> Vec2 {
        Vec2::new(
            self.text.len() as f32 * theme.font_size * 0.6,
            theme.font_size * 1.2,
        )
    }

    fn style(&self, _theme: &Theme) -> StyleBox {
        StyleBox::Empty
    }

    fn handle_event(&mut self, _event: &UiEvent, _theme: &Theme) -> EventResult {
        EventResult::Ignored
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A button widget.
pub struct ButtonWidget {
    /// Button text.
    pub text: String,
    /// Whether the button was clicked this frame.
    pub clicked: bool,
    /// Whether the button is currently pressed.
    pub pressed: bool,
    /// Whether the button is hovered.
    pub hovered: bool,
}

impl Widget for ButtonWidget {
    fn type_name(&self) -> &'static str {
        "Button"
    }

    fn measure(&self, theme: &Theme) -> Vec2 {
        Vec2::new(
            self.text.len() as f32 * theme.font_size * 0.6 + theme.spacing * 2.0,
            theme.font_size * 2.0,
        )
    }

    fn style(&self, theme: &Theme) -> StyleBox {
        if self.pressed {
            StyleBox::Flat {
                color: theme.accent_color,
                border_radius: 4.0,
            }
        } else if self.hovered {
            let mut color = theme.accent_color;
            color[3] *= 0.8;
            StyleBox::Flat {
                color,
                border_radius: 4.0,
            }
        } else {
            StyleBox::Flat {
                color: theme.base_color,
                border_radius: 4.0,
            }
        }
    }

    fn handle_event(&mut self, event: &UiEvent, _theme: &Theme) -> EventResult {
        match event {
            UiEvent::MouseMove { .. } => {
                self.hovered = true;
                EventResult::Ignored
            }
            UiEvent::MouseDown { button: 0, .. } => {
                self.pressed = true;
                EventResult::Consumed
            }
            UiEvent::MouseUp { button: 0, .. } => {
                if self.pressed {
                    self.clicked = true;
                    self.pressed = false;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Root control tree for the UI system.
pub struct ControlTree {
    root: ControlNode,
    theme: Theme,
}

impl ControlTree {
    /// Creates a new control tree with default theme.
    pub fn new() -> Self {
        Self {
            root: ControlNode {
                name: "Root".to_string(),
                layout: LayoutData::default(),
                children: Vec::new(),
                visible: true,
                enabled: true,
                widget: Box::new(LabelWidget {
                    text: String::new(),
                }),
            },
            theme: Theme::default(),
        }
    }

    /// Returns the theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Returns a mutable reference to the theme.
    pub fn theme_mut(&mut self) -> &mut Theme {
        &mut self.theme
    }

    /// Adds a button to the control tree.
    pub fn add_button(&mut self, name: impl Into<String>, text: impl Into<String>) {
        self.root.children.push(ControlNode {
            name: name.into(),
            layout: LayoutData::default(),
            children: Vec::new(),
            visible: true,
            enabled: true,
            widget: Box::new(ButtonWidget {
                text: text.into(),
                clicked: false,
                pressed: false,
                hovered: false,
            }),
        });
    }

    /// Adds a label to the control tree.
    pub fn add_label(&mut self, name: impl Into<String>, text: impl Into<String>) {
        self.root.children.push(ControlNode {
            name: name.into(),
            layout: LayoutData::default(),
            children: Vec::new(),
            visible: true,
            enabled: true,
            widget: Box::new(LabelWidget { text: text.into() }),
        });
    }

    /// Performs layout for all controls.
    pub fn layout(&mut self, _available: Vec2) {
        let theme = self.theme.clone();
        layout_node(&mut self.root, &theme);
    }

    /// Routes an event through the control tree.
    pub fn handle_event(&mut self, event: &UiEvent) -> EventResult {
        let theme = self.theme.clone();
        handle_event_node(&mut self.root, event, &theme)
    }

    /// Collects draw data for all visible controls.
    pub fn collect_draw_data(&self) -> Vec<DrawCommand> {
        let mut commands = Vec::new();
        for child in &self.root.children {
            self.collect_node_draw(child, &mut commands);
        }
        commands
    }

    fn collect_node_draw(&self, node: &ControlNode, commands: &mut Vec<DrawCommand>) {
        if !node.visible {
            return;
        }
        commands.push(DrawCommand {
            position: node.layout.position,
            size: node.layout.min_size,
            style: node.widget.style(&self.theme),
        });
        for child in &node.children {
            self.collect_node_draw(child, commands);
        }
    }
}

impl Default for ControlTree {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlNode {
    fn handle_event(&mut self, _theme: &Theme, event: &UiEvent) -> EventResult {
        if !self.visible || !self.enabled {
            return EventResult::Ignored;
        }
        self.widget.handle_event(event, &Theme::default())
    }
}

fn layout_node(node: &mut ControlNode, theme: &Theme) {
    node.layout.min_size = node.widget.measure(theme);
    let mut y_offset = node.layout.margin.top;
    for child in &mut node.children {
        child.layout.position = Vec2::new(node.layout.margin.left, y_offset);
        y_offset += child.layout.margin.top + child.layout.margin.bottom + child.layout.min_size.y;
        layout_node(child, theme);
    }
}

fn handle_event_node(node: &mut ControlNode, event: &UiEvent, theme: &Theme) -> EventResult {
    if !node.visible || !node.enabled {
        return EventResult::Ignored;
    }
    for child in &mut node.children {
        if child.handle_event(theme, event) == EventResult::Consumed {
            return EventResult::Consumed;
        }
    }
    node.widget.handle_event(event, theme)
}

/// A draw command for batched UI rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawCommand {
    /// Screen position.
    pub position: Vec2,
    /// Element size.
    pub size: Vec2,
    /// Style box.
    pub style: StyleBox,
}

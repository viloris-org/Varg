//! Input map with named actions, bindings, dead zones, and chord detection.

use std::collections::HashMap;

use crate::input::{InputState, KeyCode, MouseButton};

/// Gamepad button identifiers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GamepadButton {
    /// A button (south face button).
    A,
    /// B button (east face button).
    B,
    /// X button (west face button).
    X,
    /// Y button (north face button).
    Y,
    /// Left bumper.
    LB,
    /// Right bumper.
    RB,
    /// Left trigger (analog).
    LT,
    /// Right trigger (analog).
    RT,
    /// Start button.
    Start,
    /// Select/Back button.
    Select,
    /// Left stick press.
    LeftStick,
    /// Right stick press.
    RightStick,
    /// D-pad up.
    DPadUp,
    /// D-pad down.
    DPadDown,
    /// D-pad left.
    DPadLeft,
    /// D-pad right.
    DPadRight,
}

/// Dead zone configuration for analog input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeadZone {
    /// Input values below this threshold are treated as zero.
    pub inner: f32,
    /// Input values above this threshold are treated as 1.0.
    pub outer: f32,
}

impl Default for DeadZone {
    fn default() -> Self {
        Self {
            inner: 0.2,
            outer: 0.95,
        }
    }
}

/// Axis type for an input action.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AxisType {
    /// Digital on/off (0.0 or 1.0).
    #[default]
    Digital,
    /// One-dimensional axis (-1.0 to 1.0).
    Axis1D,
    /// Two-dimensional axis.
    Axis2D,
}

/// A single input binding for an action.
#[derive(Clone, Debug, PartialEq)]
pub struct InputBinding {
    /// Keys that produce a positive action value.
    pub positive_keys: Vec<KeyCode>,
    /// Keys that produce a negative action value.
    pub negative_keys: Vec<KeyCode>,
    /// Mouse buttons for positive action.
    pub positive_mouse: Vec<MouseButton>,
    /// Gamepad buttons for positive action.
    pub positive_gamepad: Vec<GamepadButton>,
    /// Gamepad buttons for negative action.
    pub negative_gamepad: Vec<GamepadButton>,
    /// Dead zone for this binding.
    pub dead_zone: Option<DeadZone>,
    /// Number of frames to buffer this input for.
    pub buffer_frames: u32,
    /// Keys that must all be held simultaneously for this binding to activate.
    pub chord_keys: Vec<KeyCode>,
    /// Axis type.
    pub axis_type: AxisType,
}

impl Default for InputBinding {
    fn default() -> Self {
        Self {
            positive_keys: Vec::new(),
            negative_keys: Vec::new(),
            positive_mouse: Vec::new(),
            positive_gamepad: Vec::new(),
            negative_gamepad: Vec::new(),
            dead_zone: None,
            buffer_frames: 0,
            chord_keys: Vec::new(),
            axis_type: AxisType::Digital,
        }
    }
}

impl InputBinding {
    /// Creates a digital action from keys.
    pub fn digital(keys: impl IntoIterator<Item = KeyCode>) -> Self {
        Self {
            positive_keys: keys.into_iter().collect(),
            ..Default::default()
        }
    }

    /// Creates a 1D axis binding.
    pub fn axis(
        negative: impl IntoIterator<Item = KeyCode>,
        positive: impl IntoIterator<Item = KeyCode>,
    ) -> Self {
        Self {
            positive_keys: positive.into_iter().collect(),
            negative_keys: negative.into_iter().collect(),
            axis_type: AxisType::Axis1D,
            ..Default::default()
        }
    }
}

/// Data-driven input map that binds actions to physical inputs.
#[derive(Clone, Debug, Default)]
pub struct InputMap {
    /// Map name.
    pub name: String,
    /// Named action bindings.
    pub actions: HashMap<String, InputBinding>,
}

impl InputMap {
    /// Evaluates all actions against the current input state.
    pub fn evaluate(&self, input: &InputState) -> HashMap<String, f32> {
        let mut values = HashMap::new();
        for (name, binding) in &self.actions {
            let value = self.evaluate_binding(binding, input);
            if value.abs() > f32::EPSILON {
                values.insert(name.clone(), value);
            }
        }
        values
    }

    fn evaluate_binding(&self, binding: &InputBinding, input: &InputState) -> f32 {
        let deadzone = binding.dead_zone.unwrap_or_default();

        let key_value = Self::key_axis_value(binding, input);
        let mouse_value = Self::mouse_value(binding, input);
        let gamepad_value = Self::gamepad_value(binding, input, deadzone);

        let raw = if key_value.abs() > mouse_value.abs() && key_value.abs() > gamepad_value.abs() {
            key_value
        } else if mouse_value.abs() > gamepad_value.abs() {
            mouse_value
        } else {
            gamepad_value
        };

        if !binding.chord_keys.is_empty()
            && !binding
                .chord_keys
                .iter()
                .all(|k| input.key_down(*k))
        {
            return 0.0;
        }

        match binding.axis_type {
            AxisType::Digital => {
                if raw.abs() > deadzone.inner {
                    raw.signum()
                } else {
                    0.0
                }
            }
            AxisType::Axis1D => apply_deadzone(raw, deadzone),
            AxisType::Axis2D => raw,
        }
    }

    fn key_axis_value(binding: &InputBinding, input: &InputState) -> f32 {
        let positive = binding
            .positive_keys
            .iter()
            .any(|k| input.key_down(*k));
        let negative = binding
            .negative_keys
            .iter()
            .any(|k| input.key_down(*k));
        match (negative, positive) {
            (true, false) => -1.0,
            (false, true) => 1.0,
            _ => 0.0,
        }
    }

    fn mouse_value(binding: &InputBinding, input: &InputState) -> f32 {
        if binding
            .positive_mouse
            .iter()
            .any(|b| input.mouse_button_down(*b))
        {
            1.0
        } else {
            0.0
        }
    }

    fn gamepad_value(binding: &InputBinding, input: &InputState, _deadzone: DeadZone) -> f32 {
        if binding
            .positive_gamepad
            .iter()
            .any(|b| input.gamepad_button_down(*b))
        {
            1.0
        } else if binding
            .negative_gamepad
            .iter()
            .any(|b| input.gamepad_button_down(*b))
        {
            -1.0
        } else {
            0.0
        }
    }
}

fn apply_deadzone(value: f32, deadzone: DeadZone) -> f32 {
    let abs = value.abs();
    if abs <= deadzone.inner {
        return 0.0;
    }
    if abs >= deadzone.outer {
        return value.signum();
    }
    let scaled = (abs - deadzone.inner) / (deadzone.outer - deadzone.inner);
    value.signum() * scaled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputState;

    #[test]
    fn deadzone_filters_inner_range() {
        let dz = DeadZone::default();
        assert_eq!(apply_deadzone(0.1, dz), 0.0);
        assert_eq!(apply_deadzone(-0.1, dz), 0.0);
    }

    #[test]
    fn deadzone_passes_outer_range() {
        let dz = DeadZone::default();
        assert_eq!(apply_deadzone(1.0, dz), 1.0);
        assert_eq!(apply_deadzone(-1.0, dz), -1.0);
    }

    #[test]
    fn chord_detection_requires_all_keys() {
        let mut input = InputState::default();
        input.apply_event(crate::input::InputEvent::KeyDown(KeyCode::Character('a')));

        let mut map = InputMap::default();
        map.actions.insert(
            "CtrlA".to_string(),
            InputBinding {
                positive_keys: vec![KeyCode::Character('a')],
                chord_keys: vec![KeyCode::Character('z')],
                ..Default::default()
            },
        );

        let values = map.evaluate(&input);
        assert!(!values.contains_key("CtrlA"), "chord should not fire without all keys");

        input.apply_event(crate::input::InputEvent::KeyDown(KeyCode::Character('z')));
        let values = map.evaluate(&input);
        assert!(values.contains_key("CtrlA"), "chord should fire with all keys");
    }

    #[test]
    fn gamepad_button_maps_to_action() {
        use crate::gamepad::GamepadState;

        let mut gamepad = GamepadState::default();
        gamepad.press_button(GamepadButton::A);

        let mut input = InputState::default();
        input.apply_gamepad_state(gamepad);

        let mut map = InputMap::default();
        map.actions.insert(
            "Jump".to_string(),
            InputBinding {
                positive_gamepad: vec![GamepadButton::A],
                ..Default::default()
            },
        );

        let values = map.evaluate(&input);
        assert!(values.contains_key("Jump"));
    }

    #[test]
    fn axis_binding_returns_negative_and_positive() {
        let mut input = InputState::default();
        input.apply_event(crate::input::InputEvent::KeyDown(KeyCode::Character('a')));

        let mut map = InputMap::default();
        map.actions.insert(
            "MoveX".to_string(),
            InputBinding::axis(
                [KeyCode::Character('a')],
                [KeyCode::Character('d')],
            ),
        );

        let values = map.evaluate(&input);
        assert_eq!(values.get("MoveX"), Some(&-1.0));
    }
}

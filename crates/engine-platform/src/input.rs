//! Input abstraction.

use std::collections::{HashMap, HashSet};

use crate::gamepad::GamepadState;
use crate::input_map::GamepadButton;

/// Keyboard key codes used by the platform abstraction.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum KeyCode {
    /// Escape key.
    Escape,
    /// Enter key.
    Enter,
    /// Space key.
    Space,
    /// Up arrow key.
    ArrowUp,
    /// Down arrow key.
    ArrowDown,
    /// Left arrow key.
    ArrowLeft,
    /// Right arrow key.
    ArrowRight,
    /// Character key.
    Character(char),
}

impl KeyCode {
    /// Normalizes character keys to lowercase ASCII for stable action matching.
    pub fn normalized(self) -> Self {
        match self {
            Self::Character(value) => Self::Character(value.to_ascii_lowercase()),
            other => other,
        }
    }
}

/// Mouse buttons used by the platform abstraction.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MouseButton {
    /// Left mouse button.
    Left,
    /// Right mouse button.
    Right,
    /// Middle mouse button.
    Middle,
    /// Other mouse button.
    Other(u16),
}

/// Input event emitted by a platform backend.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// Key pressed.
    KeyDown(KeyCode),
    /// Key released.
    KeyUp(KeyCode),
    /// Mouse button pressed.
    MouseButtonDown(MouseButton),
    /// Mouse button released.
    MouseButtonUp(MouseButton),
    /// Mouse moved in logical pixels.
    MouseMove {
        /// X position.
        x: f32,
        /// Y position.
        y: f32,
    },
    /// Mouse wheel delta in logical scroll units.
    MouseWheel {
        /// Horizontal scroll.
        x: f32,
        /// Vertical scroll.
        y: f32,
    },
}

/// Bindings that can drive a named input action.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionBinding {
    /// Keys that produce a positive action value.
    pub positive_keys: Vec<KeyCode>,
    /// Keys that produce a negative action value.
    pub negative_keys: Vec<KeyCode>,
}

impl ActionBinding {
    /// Creates a digital action from positive keys.
    pub fn digital(keys: impl IntoIterator<Item = KeyCode>) -> Self {
        Self {
            positive_keys: keys.into_iter().map(KeyCode::normalized).collect(),
            negative_keys: Vec::new(),
        }
    }

    /// Creates a one-dimensional axis from negative and positive keys.
    pub fn axis(
        negative: impl IntoIterator<Item = KeyCode>,
        positive: impl IntoIterator<Item = KeyCode>,
    ) -> Self {
        Self {
            positive_keys: positive.into_iter().map(KeyCode::normalized).collect(),
            negative_keys: negative.into_iter().map(KeyCode::normalized).collect(),
        }
    }
}

/// Per-frame input state with stable down state and transient pressed/released state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputState {
    down_keys: HashSet<KeyCode>,
    pressed_keys: HashSet<KeyCode>,
    released_keys: HashSet<KeyCode>,
    down_mouse_buttons: HashSet<MouseButton>,
    pressed_mouse_buttons: HashSet<MouseButton>,
    released_mouse_buttons: HashSet<MouseButton>,
    cursor_position: Option<(f32, f32)>,
    mouse_delta: (f32, f32),
    wheel_delta: (f32, f32),
    actions: HashMap<String, ActionBinding>,
    gamepads: Vec<GamepadState>,
}

impl InputState {
    /// Applies a gamepad state for this frame.
    pub fn apply_gamepad_state(&mut self, state: GamepadState) {
        self.apply_gamepad_states([state]);
    }

    /// Applies all connected gamepad states for this frame.
    pub fn apply_gamepad_states(&mut self, states: impl IntoIterator<Item = GamepadState>) {
        self.gamepads = states.into_iter().collect();
    }

    /// Returns the primary gamepad state.
    pub fn gamepad_state(&self) -> Option<&GamepadState> {
        self.gamepads.first()
    }

    /// Returns all current gamepad states.
    pub fn gamepad_states(&self) -> &[GamepadState] {
        &self.gamepads
    }

    /// Returns whether a gamepad button is currently held.
    pub fn gamepad_button_down(&self, button: GamepadButton) -> bool {
        self.gamepads
            .iter()
            .any(|gamepad| gamepad.button_down(button))
    }

    /// Returns whether a gamepad button was pressed this frame.
    pub fn gamepad_button_pressed(&self, button: GamepadButton) -> bool {
        self.gamepads
            .iter()
            .any(|gamepad| gamepad.button_pressed(button))
    }

    /// Returns whether a gamepad button was released this frame.
    pub fn gamepad_button_released(&self, button: GamepadButton) -> bool {
        self.gamepads
            .iter()
            .any(|gamepad| gamepad.button_released(button))
    }

    /// Returns all keys pressed this frame.
    pub fn pressed_keys(&self) -> Vec<KeyCode> {
        self.pressed_keys.iter().copied().collect()
    }
    /// Registers or replaces an action binding.
    pub fn bind_action(&mut self, name: impl Into<String>, binding: ActionBinding) {
        self.actions.insert(name.into(), binding);
    }

    /// Registers the default player movement actions.
    pub fn bind_default_player_actions(&mut self) {
        self.bind_action(
            "MoveForward",
            ActionBinding::digital([KeyCode::Character('w'), KeyCode::ArrowUp]),
        );
        self.bind_action(
            "MoveBackward",
            ActionBinding::digital([KeyCode::Character('s'), KeyCode::ArrowDown]),
        );
        self.bind_action(
            "MoveRight",
            ActionBinding::digital([KeyCode::Character('d'), KeyCode::ArrowRight]),
        );
        self.bind_action(
            "MoveLeft",
            ActionBinding::digital([KeyCode::Character('a'), KeyCode::ArrowLeft]),
        );
        self.bind_action(
            "MoveX",
            ActionBinding::axis(
                [KeyCode::Character('a'), KeyCode::ArrowLeft],
                [KeyCode::Character('d'), KeyCode::ArrowRight],
            ),
        );
        self.bind_action(
            "MoveY",
            ActionBinding::axis(
                [KeyCode::Character('s'), KeyCode::ArrowDown],
                [KeyCode::Character('w'), KeyCode::ArrowUp],
            ),
        );
    }

    /// Applies a platform input event.
    pub fn apply_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::KeyDown(key) => {
                let key = key.normalized();
                if self.down_keys.insert(key) {
                    self.pressed_keys.insert(key);
                }
            }
            InputEvent::KeyUp(key) => {
                let key = key.normalized();
                if self.down_keys.remove(&key) {
                    self.released_keys.insert(key);
                }
            }
            InputEvent::MouseButtonDown(button) => {
                if self.down_mouse_buttons.insert(button) {
                    self.pressed_mouse_buttons.insert(button);
                }
            }
            InputEvent::MouseButtonUp(button) => {
                if self.down_mouse_buttons.remove(&button) {
                    self.released_mouse_buttons.insert(button);
                }
            }
            InputEvent::MouseMove { x, y } => {
                if let Some((previous_x, previous_y)) = self.cursor_position {
                    self.mouse_delta.0 += x - previous_x;
                    self.mouse_delta.1 += y - previous_y;
                }
                self.cursor_position = Some((x, y));
            }
            InputEvent::MouseWheel { x, y } => {
                self.wheel_delta.0 += x;
                self.wheel_delta.1 += y;
            }
        }
    }

    /// Clears transient pressed/released, mouse delta, and wheel state at the end of a frame.
    pub fn end_frame(&mut self) {
        self.pressed_keys.clear();
        self.released_keys.clear();
        self.pressed_mouse_buttons.clear();
        self.released_mouse_buttons.clear();
        self.mouse_delta = (0.0, 0.0);
        self.wheel_delta = (0.0, 0.0);
        for gamepad in &mut self.gamepads {
            gamepad.end_frame();
        }
    }

    /// Returns whether a key is currently held.
    pub fn key_down(&self, key: KeyCode) -> bool {
        self.down_keys.contains(&key.normalized())
    }

    /// Returns whether a key was pressed this frame.
    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.pressed_keys.contains(&key.normalized())
    }

    /// Returns whether a key was released this frame.
    pub fn key_released(&self, key: KeyCode) -> bool {
        self.released_keys.contains(&key.normalized())
    }

    /// Returns whether a mouse button is currently held.
    pub fn mouse_button_down(&self, button: MouseButton) -> bool {
        self.down_mouse_buttons.contains(&button)
    }

    /// Returns current cursor position.
    pub fn cursor_position(&self) -> Option<(f32, f32)> {
        self.cursor_position
    }

    /// Returns accumulated mouse delta for this frame.
    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    /// Returns accumulated wheel delta for this frame.
    pub fn wheel_delta(&self) -> (f32, f32) {
        self.wheel_delta
    }

    /// Returns whether a digital action is currently active.
    pub fn action_down(&self, name: &str) -> bool {
        self.action_value(name) > 0.0
    }

    /// Returns an action value in the `-1..=1` range for digital bindings.
    pub fn action_value(&self, name: &str) -> f32 {
        let Some(binding) = self.actions.get(name) else {
            return 0.0;
        };
        let positive = binding
            .positive_keys
            .iter()
            .any(|key| self.down_keys.contains(key));
        let negative = binding
            .negative_keys
            .iter()
            .any(|key| self.down_keys.contains(key));
        match (negative, positive) {
            (true, false) => -1.0,
            (false, true) => 1.0,
            _ => 0.0,
        }
    }
}

/// Maps physical keys to logical action names so game code queries actions
/// instead of raw key codes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionMap {
    /// Map from action name to bound key codes.
    pub bindings: HashMap<String, Vec<KeyCode>>,
}

impl ActionMap {
    /// Creates an ActionMap with default movement and jump bindings.
    pub fn new() -> Self {
        let mut map = Self::default();
        map.bind_defaults();
        map
    }

    /// Populates the default bindings (WASD, arrows, space, F, E, Escape).
    pub fn bind_defaults(&mut self) {
        // Movement actions (digital)
        self.bind("MoveForward", KeyCode::Character('w'));
        self.bind("MoveForward", KeyCode::ArrowUp);
        self.bind("MoveBack", KeyCode::Character('s'));
        self.bind("MoveBack", KeyCode::ArrowDown);
        self.bind("MoveLeft", KeyCode::Character('a'));
        self.bind("MoveLeft", KeyCode::ArrowLeft);
        self.bind("MoveRight", KeyCode::Character('d'));
        self.bind("MoveRight", KeyCode::ArrowRight);
        self.bind("Jump", KeyCode::Space);
        // Additional game actions
        self.bind("Fire", KeyCode::Character('f'));
        self.bind("Fire", KeyCode::Character('e'));
        self.bind("Interact", KeyCode::Character('e'));
        self.bind("Pause", KeyCode::Escape);
    }

    /// Binds a key to an action name.
    pub fn bind(&mut self, action_name: impl Into<String>, key: KeyCode) {
        self.bindings
            .entry(action_name.into())
            .or_default()
            .push(key.normalized());
    }

    /// Returns true if any key bound to `action_name` was pressed this frame.
    pub fn action_pressed(&self, input: &InputState, action_name: &str) -> bool {
        self.bindings
            .get(action_name)
            .map(|keys| keys.iter().any(|k| input.key_pressed(*k)))
            .unwrap_or(false)
    }

    /// Returns true if any key bound to `action_name` is held (including the first frame).
    pub fn action_held(&self, input: &InputState, action_name: &str) -> bool {
        self.bindings
            .get(action_name)
            .map(|keys| keys.iter().any(|k| input.key_down(*k)))
            .unwrap_or(false)
    }

    /// Returns an axis value in `-1..=1` for a pair of negative/positive actions.
    ///
    /// This is a convenience for two-action axis patterns like
    /// `MoveHorizontal = MoveLeft(-1) | MoveRight(+1)`.
    pub fn axis_value(
        &self,
        input: &InputState,
        negative_action: &str,
        positive_action: &str,
    ) -> f32 {
        let neg = self.action_held(input, negative_action);
        let pos = self.action_held(input, positive_action);
        match (neg, pos) {
            (true, false) => -1.0,
            (false, true) => 1.0,
            _ => 0.0,
        }
    }

    /// Parses a key name string into a `KeyCode`.
    ///
    /// Recognized names: Escape, Enter, Space, ArrowUp, ArrowDown, ArrowLeft,
    /// ArrowRight. Single characters map to `KeyCode::Character`.
    pub fn parse_key_name(name: &str) -> Option<KeyCode> {
        match name {
            "Escape" => Some(KeyCode::Escape),
            "Enter" => Some(KeyCode::Enter),
            "Space" => Some(KeyCode::Space),
            "ArrowUp" => Some(KeyCode::ArrowUp),
            "ArrowDown" => Some(KeyCode::ArrowDown),
            "ArrowLeft" => Some(KeyCode::ArrowLeft),
            "ArrowRight" => Some(KeyCode::ArrowRight),
            c if c.chars().count() == 1 => c
                .chars()
                .next()
                .map(|ch| KeyCode::Character(ch.to_ascii_lowercase())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_transients_reset_at_frame_boundary() {
        let mut input = InputState::default();
        input.apply_event(InputEvent::KeyDown(KeyCode::Character('W')));

        assert!(input.key_down(KeyCode::Character('w')));
        assert!(input.key_pressed(KeyCode::Character('w')));

        input.end_frame();

        assert!(input.key_down(KeyCode::Character('w')));
        assert!(!input.key_pressed(KeyCode::Character('w')));

        input.apply_event(InputEvent::KeyUp(KeyCode::Character('w')));
        assert!(input.key_released(KeyCode::Character('w')));
        assert!(!input.key_down(KeyCode::Character('w')));
    }

    #[test]
    fn default_actions_map_wasd_and_arrows() {
        let mut input = InputState::default();
        input.bind_default_player_actions();
        input.apply_event(InputEvent::KeyDown(KeyCode::ArrowRight));

        assert!(input.action_down("MoveRight"));
        assert_eq!(input.action_value("MoveX"), 1.0);
    }

    // ActionMap tests
    #[test]
    fn action_map_default_bindings_exist() {
        let map = ActionMap::new();
        let mut input = InputState::default();
        input.bind_default_player_actions();
        input.apply_event(InputEvent::KeyDown(KeyCode::Character('w')));
        assert!(map.action_pressed(&input, "MoveForward"));
        assert!(map.action_held(&input, "MoveForward"));
        input.end_frame();
        assert!(!map.action_pressed(&input, "MoveForward"));
        assert!(map.action_held(&input, "MoveForward"));
    }

    #[test]
    fn action_map_jump_with_space() {
        let map = ActionMap::new();
        let mut input = InputState::default();
        input.bind_default_player_actions();
        input.apply_event(InputEvent::KeyDown(KeyCode::Space));
        assert!(map.action_pressed(&input, "Jump"));
        assert!(map.action_held(&input, "Jump"));
        input.end_frame();
        assert!(!map.action_pressed(&input, "Jump"));
        assert!(map.action_held(&input, "Jump"));
    }

    #[test]
    fn action_map_multi_key_single_action() {
        let mut map = ActionMap::default();
        map.bind("Fire", KeyCode::Character('f'));
        map.bind("Fire", KeyCode::Character('e'));
        let mut input = InputState::default();
        input.apply_event(InputEvent::KeyDown(KeyCode::Character('e')));
        assert!(map.action_pressed(&input, "Fire"));
        input.apply_event(InputEvent::KeyUp(KeyCode::Character('e')));
        input.end_frame();
        input.apply_event(InputEvent::KeyDown(KeyCode::Character('f')));
        assert!(map.action_pressed(&input, "Fire"));
    }

    #[test]
    fn action_map_unknown_action_returns_false() {
        let map = ActionMap::new();
        let input = InputState::default();
        assert!(!map.action_pressed(&input, "DoesNotExist"));
        assert!(!map.action_held(&input, "DoesNotExist"));
    }
}

//! Gamepad state and provider abstraction.

use std::collections::HashSet;

use super::input_map::GamepadButton;

/// Current state of a connected gamepad.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GamepadState {
    /// Currently held buttons.
    pub down_buttons: HashSet<GamepadButton>,
    /// Buttons pressed this frame.
    pub pressed_buttons: HashSet<GamepadButton>,
    /// Buttons released this frame.
    pub released_buttons: HashSet<GamepadButton>,
    /// Left stick X axis (-1.0 to 1.0).
    pub left_stick_x: f32,
    /// Left stick Y axis (-1.0 to 1.0).
    pub left_stick_y: f32,
    /// Right stick X axis (-1.0 to 1.0).
    pub right_stick_x: f32,
    /// Right stick Y axis (-1.0 to 1.0).
    pub right_stick_y: f32,
    /// Left trigger value (0.0 to 1.0).
    pub left_trigger: f32,
    /// Right trigger value (0.0 to 1.0).
    pub right_trigger: f32,
}

impl GamepadState {
    /// Clears transient pressed/released state at frame end.
    pub fn end_frame(&mut self) {
        self.pressed_buttons.clear();
        self.released_buttons.clear();
    }

    /// Applies a button press event.
    pub fn press_button(&mut self, button: GamepadButton) {
        if self.down_buttons.insert(button) {
            self.pressed_buttons.insert(button);
        }
    }

    /// Applies a button release event.
    pub fn release_button(&mut self, button: GamepadButton) {
        if self.down_buttons.remove(&button) {
            self.released_buttons.insert(button);
        }
    }

    /// Returns whether a button is currently held.
    pub fn button_down(&self, button: GamepadButton) -> bool {
        self.down_buttons.contains(&button)
    }

    /// Returns whether a button was pressed this frame.
    pub fn button_pressed(&self, button: GamepadButton) -> bool {
        self.pressed_buttons.contains(&button)
    }
}

/// Trait for platform-specific gamepad backends.
pub trait GamepadProvider: Send + Sync {
    /// Returns the current state of all connected gamepads.
    fn poll_gamepads(&mut self) -> Vec<GamepadState>;

    /// Returns the number of connected gamepads.
    fn gamepad_count(&self) -> usize;
}

/// Null gamepad provider that returns no gamepads.
#[derive(Default)]
pub struct NullGamepadProvider;

impl GamepadProvider for NullGamepadProvider {
    fn poll_gamepads(&mut self) -> Vec<GamepadState> {
        Vec::new()
    }

    fn gamepad_count(&self) -> usize {
        0
    }
}

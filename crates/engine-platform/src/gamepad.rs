//! Gamepad state and provider abstraction.

use std::{collections::HashSet, fmt};

use super::input_map::GamepadButton;

/// Stable gamepad identifier assigned by the platform backend.
pub type GamepadId = u32;

/// Current state of a connected gamepad.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GamepadState {
    /// Backend-provided stable identifier for the device.
    pub id: GamepadId,
    /// Human-readable device name, when reported by the backend.
    pub name: String,
    /// Whether the device is currently connected.
    pub connected: bool,
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
    /// Creates a connected gamepad state with identity metadata.
    pub fn connected(id: GamepadId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            connected: true,
            ..Default::default()
        }
    }

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

    /// Returns whether a button was released this frame.
    pub fn button_released(&self, button: GamepadButton) -> bool {
        self.released_buttons.contains(&button)
    }
}

/// Trait for platform-specific gamepad backends.
pub trait GamepadProvider: Send + fmt::Debug {
    /// Returns the current state of all connected gamepads.
    fn poll_gamepads(&mut self) -> Vec<GamepadState>;

    /// Returns the number of connected gamepads.
    fn gamepad_count(&self) -> usize;
}

/// Null gamepad provider that returns no gamepads.
#[derive(Debug, Default)]
pub struct NullGamepadProvider;

impl GamepadProvider for NullGamepadProvider {
    fn poll_gamepads(&mut self) -> Vec<GamepadState> {
        Vec::new()
    }

    fn gamepad_count(&self) -> usize {
        0
    }
}

/// Gamepad provider backed by `gilrs`.
#[cfg(feature = "runtime-game")]
pub struct GilrsGamepadProvider {
    gilrs: gilrs::Gilrs,
    previous: Vec<GamepadState>,
}

#[cfg(feature = "runtime-game")]
impl fmt::Debug for GilrsGamepadProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GilrsGamepadProvider")
            .field("gamepad_count", &self.gamepad_count())
            .finish()
    }
}

#[cfg(feature = "runtime-game")]
impl GilrsGamepadProvider {
    /// Creates a `gilrs` provider using the platform's native gamepad backends.
    pub fn new() -> Result<Self, gilrs::Error> {
        Ok(Self {
            gilrs: gilrs::Gilrs::new()?,
            previous: Vec::new(),
        })
    }

    fn state_from_gamepad(id: gilrs::GamepadId, gamepad: gilrs::Gamepad<'_>) -> GamepadState {
        use gilrs::{Axis, Button};

        let mut state = GamepadState::connected(usize::from(id) as GamepadId, gamepad.name());
        state.left_stick_x = gamepad.value(Axis::LeftStickX);
        state.left_stick_y = gamepad.value(Axis::LeftStickY);
        state.right_stick_x = gamepad.value(Axis::RightStickX);
        state.right_stick_y = gamepad.value(Axis::RightStickY);
        state.left_trigger = trigger_value(&gamepad, Button::LeftTrigger2, Axis::LeftZ);
        state.right_trigger = trigger_value(&gamepad, Button::RightTrigger2, Axis::RightZ);

        for (gilrs_button, engine_button) in STANDARD_BUTTONS {
            if gamepad
                .button_data(*gilrs_button)
                .is_some_and(|button| button.is_pressed())
            {
                state.down_buttons.insert(*engine_button);
            }
        }

        if state.left_trigger > 0.5 {
            state.down_buttons.insert(GamepadButton::LT);
        }
        if state.right_trigger > 0.5 {
            state.down_buttons.insert(GamepadButton::RT);
        }

        state
    }

    fn update_transients(&self, mut next: GamepadState) -> GamepadState {
        if let Some(previous) = self.previous.iter().find(|state| state.id == next.id) {
            next.pressed_buttons = next
                .down_buttons
                .difference(&previous.down_buttons)
                .copied()
                .collect();
            next.released_buttons = previous
                .down_buttons
                .difference(&next.down_buttons)
                .copied()
                .collect();
        } else {
            next.pressed_buttons = next.down_buttons.clone();
        }
        next
    }
}

#[cfg(feature = "runtime-game")]
impl GamepadProvider for GilrsGamepadProvider {
    fn poll_gamepads(&mut self) -> Vec<GamepadState> {
        while self.gilrs.next_event().is_some() {}

        let connected_states = self
            .gilrs
            .gamepads()
            .map(|(id, gamepad)| Self::state_from_gamepad(id, gamepad))
            .map(|state| self.update_transients(state))
            .collect::<Vec<_>>();

        let mut states = connected_states.clone();
        states.extend(self.previous.iter().filter_map(|previous| {
            let still_connected = connected_states.iter().any(|state| state.id == previous.id);
            if still_connected {
                return None;
            }

            let mut disconnected = GamepadState {
                id: previous.id,
                name: previous.name.clone(),
                connected: false,
                released_buttons: previous.down_buttons.clone(),
                ..GamepadState::default()
            };
            disconnected.down_buttons.clear();
            Some(disconnected)
        }));

        self.previous = connected_states;
        states
    }

    fn gamepad_count(&self) -> usize {
        self.gilrs.gamepads().count()
    }
}

#[cfg(feature = "runtime-game")]
const STANDARD_BUTTONS: &[(gilrs::Button, GamepadButton)] = &[
    (gilrs::Button::South, GamepadButton::A),
    (gilrs::Button::East, GamepadButton::B),
    (gilrs::Button::West, GamepadButton::X),
    (gilrs::Button::North, GamepadButton::Y),
    (gilrs::Button::LeftTrigger, GamepadButton::LB),
    (gilrs::Button::RightTrigger, GamepadButton::RB),
    (gilrs::Button::LeftTrigger2, GamepadButton::LT),
    (gilrs::Button::RightTrigger2, GamepadButton::RT),
    (gilrs::Button::Start, GamepadButton::Start),
    (gilrs::Button::Select, GamepadButton::Select),
    (gilrs::Button::LeftThumb, GamepadButton::LeftStick),
    (gilrs::Button::RightThumb, GamepadButton::RightStick),
    (gilrs::Button::DPadUp, GamepadButton::DPadUp),
    (gilrs::Button::DPadDown, GamepadButton::DPadDown),
    (gilrs::Button::DPadLeft, GamepadButton::DPadLeft),
    (gilrs::Button::DPadRight, GamepadButton::DPadRight),
];

#[cfg(feature = "runtime-game")]
fn trigger_value(
    gamepad: &gilrs::Gamepad<'_>,
    button: gilrs::Button,
    fallback_axis: gilrs::Axis,
) -> f32 {
    gamepad
        .button_data(button)
        .map(|button| button.value())
        .unwrap_or_else(|| ((gamepad.value(fallback_axis) + 1.0) * 0.5).clamp(0.0, 1.0))
}

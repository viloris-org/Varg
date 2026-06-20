#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Platform capability abstraction.

pub mod action_context;
pub mod callbacks;
pub mod filesystem;
pub mod gamepad;
pub mod input;
pub mod input_buffer;
pub mod input_map;
pub mod library;
pub mod window;

pub use action_context::{ActionContext, ActionContextManager};
pub use callbacks::{CallbackThread, ThreadBoundCallback};
pub use filesystem::{FileSystem, HostFileSystem};
#[cfg(feature = "runtime-game")]
pub use gamepad::GilrsGamepadProvider;
pub use gamepad::{GamepadId, GamepadProvider, GamepadState, NullGamepadProvider};
pub use input::{ActionBinding, ActionMap, InputEvent, InputState, KeyCode, MouseButton};
pub use input_buffer::InputBuffer;
pub use input_map::{
    AxisType, DeadZone, GamepadAxis, GamepadButton, InputBinding as InputBindingV2, InputMap,
};
pub use library::DynamicLibraryProvider;
pub use window::{WindowDescriptor, WindowProvider};

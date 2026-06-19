//! Layered logging facade.
//!
//! Three targets separate engine, game, and editor concerns:
//! - `engine` — core subsystems (render, ECS, assets, physics)
//! - `game`   — user scripts and gameplay logic
//! - `editor` — Tauri editor shell and tooling

/// Tracing target for engine subsystems.
pub const TARGET_ENGINE: &str = "engine";

/// Tracing target for user game logic.
pub const TARGET_GAME: &str = "game";

/// Tracing target for editor tooling.
pub const TARGET_EDITOR: &str = "editor";

/// Logs a structured runtime startup event.
pub fn log_runtime_start(app_name: &str, profile: &str) {
    tracing::info!(target: TARGET_ENGINE, app_name, profile, "runtime starting");
}

/// Logs a structured frame event.
pub fn log_frame(frame_index: u64) {
    tracing::trace!(target: TARGET_ENGINE, frame_index, "frame tick");
}

/// Convenience: engine-targeted info.
#[macro_export]
macro_rules! engine_info {
    ($($arg:tt)*) => {
        tracing::info!(target: $crate::logging::TARGET_ENGINE, $($arg)*)
    };
}

/// Convenience: engine-targeted warn.
#[macro_export]
macro_rules! engine_warn {
    ($($arg:tt)*) => {
        tracing::warn!(target: $crate::logging::TARGET_ENGINE, $($arg)*)
    };
}

/// Convenience: engine-targeted error.
#[macro_export]
macro_rules! engine_error {
    ($($arg:tt)*) => {
        tracing::error!(target: $crate::logging::TARGET_ENGINE, $($arg)*)
    };
}

/// Convenience: engine-targeted debug.
#[macro_export]
macro_rules! engine_debug {
    ($($arg:tt)*) => {
        tracing::debug!(target: $crate::logging::TARGET_ENGINE, $($arg)*)
    };
}

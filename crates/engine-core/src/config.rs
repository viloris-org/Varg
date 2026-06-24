//! Runtime configuration primitives.

use std::path::PathBuf;

/// Minimal engine configuration shared by runtime entry points.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineConfig {
    /// Application name for logs, windows, and diagnostics.
    pub app_name: String,
    /// Root directory used for relative asset and project paths.
    pub root_path: PathBuf,
    /// Runtime feature profile selected by the launcher.
    pub profile: RuntimeProfile,
}

impl EngineConfig {
    /// Creates a new configuration value.
    pub fn new(
        app_name: impl Into<String>,
        root_path: impl Into<PathBuf>,
        profile: RuntimeProfile,
    ) -> Self {
        Self {
            app_name: app_name.into(),
            root_path: root_path.into(),
            profile,
        }
    }
}

/// Build/runtime profile names supported by the workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RuntimeProfile {
    /// Minimal runtime without editor, scripting, importers, audio, physics, or concrete rendering.
    RuntimeMin,
    /// Game runtime profile.
    RuntimeGame,
    /// Editor profile.
    Editor,
    /// Agent tooling profile.
    AgentTools,
    /// Developer convenience profile.
    DevFull,
}

impl RuntimeProfile {
    /// Stable profile name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeMin => "runtime-min",
            Self::RuntimeGame => "runtime-game",
            Self::Editor => "editor",
            Self::AgentTools => "agent-tools",
            Self::DevFull => "dev-full",
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            app_name: "Aster".to_owned(),
            root_path: PathBuf::from("."),
            profile: RuntimeProfile::RuntimeMin,
        }
    }
}

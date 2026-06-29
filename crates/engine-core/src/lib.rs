#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Core runtime primitives shared by all Varg engine crates.

pub mod config;
pub mod error;
pub mod handle;
pub mod ids;
pub mod logging;
pub mod math;
pub mod task;
pub mod time;

pub use config::{EngineConfig, RuntimeProfile};
pub use error::{EngineError, EngineResult};
pub use handle::{Generation, Handle, HandleAllocator};
pub use ids::{AssetId, EntityId, ResourceId};
pub use task::{
    EngineTaskRuntime, TaskHandle, TaskJoinError, TaskPriority, TaskRuntimeConfig,
    TaskRuntimeStats, shared_task_runtime,
};
pub use time::{FrameCounter, TimeState, TimeStep};

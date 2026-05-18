#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Core runtime primitives shared by all Aster engine crates.

pub mod config;
pub mod error;
pub mod handle;
pub mod ids;
pub mod logging;
pub mod math;
pub mod time;

pub use config::{EngineConfig, RuntimeProfile};
pub use error::{EngineError, EngineResult};
pub use handle::{Generation, Handle, HandleAllocator};
pub use ids::{AssetId, EntityId, ResourceId};
pub use time::{FrameCounter, TimeStep};

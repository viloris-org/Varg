//! Generic resource trait for typed asset resource handles.

use engine_core::{EngineResult, Handle};
use std::marker::PhantomData;

/// Trait for resources that can be serialized to/from JSON and binary formats.
pub trait Resource: Sized + Send + Sync + 'static {
    /// Stable type name for registry lookups.
    fn type_name() -> &'static str;

    /// Serializes to JSON for .res format.
    fn to_json(&self) -> EngineResult<String>;

    /// Deserializes from JSON.
    fn from_json(input: &str) -> EngineResult<Self>;

    /// Serializes to binary for .binres format.
    fn to_binary(&self) -> EngineResult<Vec<u8>>;

    /// Deserializes from binary.
    fn from_binary(bytes: &[u8]) -> EngineResult<Self>;

    /// Preview summary for editor displays.
    fn preview_summary(&self) -> String;
}

/// Typed resource handle wrapping a generational handle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ResourceHandle<T: Resource> {
    handle: Handle,
    _phantom: PhantomData<T>,
}

impl<T: Resource> ResourceHandle<T> {
    /// Creates a resource handle from a raw handle.
    pub const fn new(handle: Handle) -> Self {
        Self {
            handle,
            _phantom: PhantomData,
        }
    }

    /// Returns the underlying raw handle.
    pub const fn raw(self) -> Handle {
        self.handle
    }
}

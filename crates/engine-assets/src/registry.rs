//! Type registry for resources, mapping type names to load/validate functions.

use std::any::TypeId;
use std::collections::HashMap;

use engine_core::EngineResult;

/// Registry that maps resource type names to validation functions.
///
/// Used by the asset pipeline to validate `.res` and `.binres` files
/// without knowing the concrete resource type at compile time.
#[derive(Default)]
pub struct ResourceTypeRegistry {
    entries: HashMap<String, ResourceTypeEntry>,
}

struct ResourceTypeEntry {
    validate_json: fn(&str) -> EngineResult<()>,
    validate_binary: fn(&[u8]) -> EngineResult<()>,
    type_id: TypeId,
}

impl ResourceTypeRegistry {
    /// Registers a resource type. Call once per type at startup.
    pub fn register<T: crate::resource_trait::Resource>(&mut self) {
        self.entries.insert(
            T::type_name().to_string(),
            ResourceTypeEntry {
                validate_json: |input| {
                    T::from_json(input)?;
                    Ok(())
                },
                validate_binary: |bytes| {
                    T::from_binary(bytes)?;
                    Ok(())
                },
                type_id: TypeId::of::<T>(),
            },
        );
    }

    /// Returns whether a resource type name is registered.
    pub fn contains(&self, type_name: &str) -> bool {
        self.entries.contains_key(type_name)
    }

    /// Returns the number of registered types.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns whether a concrete type is registered.
    pub fn contains_type<T: crate::resource_trait::Resource>(&self) -> bool {
        self.entries
            .values()
            .any(|entry| entry.type_id == TypeId::of::<T>())
    }

    /// Validates that JSON data can be deserialized as the given resource type.
    pub fn validate_json(&self, type_name: &str, input: &str) -> EngineResult<()> {
        let entry = self.entries.get(type_name).ok_or_else(|| {
            engine_core::EngineError::other(format!("unknown resource type: {type_name}"))
        })?;
        (entry.validate_json)(input)
    }

    /// Validates that binary data can be deserialized as the given resource type.
    pub fn validate_binary(&self, type_name: &str, bytes: &[u8]) -> EngineResult<()> {
        let entry = self.entries.get(type_name).ok_or_else(|| {
            engine_core::EngineError::other(format!("unknown resource type: {type_name}"))
        })?;
        (entry.validate_binary)(bytes)
    }
}

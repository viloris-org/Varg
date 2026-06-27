//! Generational handles for stable references.

use crate::{EngineError, EngineResult};

/// A generation counter used to distinguish recycled handle slots.
#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
pub struct Generation(u32);

impl Generation {
    /// Initial generation for a newly allocated slot.
    pub const FIRST: Self = Self(1);

    /// Creates a generation from a raw non-zero value.
    pub fn from_raw(value: u32) -> EngineResult<Self> {
        if value == 0 {
            return Err(EngineError::invalid_handle("generation must be non-zero"));
        }
        Ok(Self(value))
    }

    /// Returns the raw generation value.
    pub const fn get(self) -> u32 {
        self.0
    }

    fn next(self) -> Self {
        Self(self.0.saturating_add(1).max(1))
    }
}

/// Stable handle with slot and generation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Handle {
    slot: u32,
    generation: Generation,
}

impl Handle {
    /// Creates a handle from checked raw parts.
    pub const fn new(slot: u32, generation: Generation) -> Self {
        Self { slot, generation }
    }

    /// Slot index.
    pub const fn slot(self) -> u32 {
        self.slot
    }

    /// Generation value.
    pub const fn generation(self) -> Generation {
        self.generation
    }
}

#[derive(Clone, Debug)]
struct Slot {
    generation: Generation,
    occupied: bool,
}

/// Small generational handle allocator.
#[derive(Clone, Debug, Default)]
pub struct HandleAllocator {
    slots: Vec<Slot>,
    free: Vec<u32>,
}

impl HandleAllocator {
    /// Allocates a new live handle.
    pub fn allocate(&mut self) -> EngineResult<Handle> {
        if let Some(slot) = self.free.pop() {
            let index = slot as usize;
            let entry = self.slots.get_mut(index).ok_or_else(|| {
                EngineError::invalid_handle("free list referenced a missing slot")
            })?;
            entry.occupied = true;
            return Ok(Handle::new(slot, entry.generation));
        }

        let slot = u32::try_from(self.slots.len())
            .map_err(|_| EngineError::invalid_handle("handle slot capacity exceeded"))?;
        self.slots.push(Slot {
            generation: Generation::FIRST,
            occupied: true,
        });
        Ok(Handle::new(slot, Generation::FIRST))
    }

    /// Frees a live handle and invalidates stale copies.
    pub fn free(&mut self, handle: Handle) -> EngineResult<()> {
        let entry = self
            .slots
            .get_mut(handle.slot as usize)
            .ok_or_else(|| EngineError::invalid_handle("handle slot does not exist"))?;
        if !entry.occupied || entry.generation != handle.generation {
            return Err(EngineError::invalid_handle(
                "handle is stale or already free",
            ));
        }
        entry.occupied = false;
        entry.generation = entry.generation.next();
        self.free.push(handle.slot);
        Ok(())
    }

    /// Returns whether the handle is currently live.
    pub fn is_live(&self, handle: Handle) -> bool {
        self.slots
            .get(handle.slot as usize)
            .is_some_and(|slot| slot.occupied && slot.generation == handle.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freed_handle_is_not_live_after_reuse() {
        let mut allocator = HandleAllocator::default();
        let first = allocator.allocate().unwrap();
        allocator.free(first).unwrap();
        let second = allocator.allocate().unwrap();

        assert!(!allocator.is_live(first));
        assert!(allocator.is_live(second));
        assert_eq!(first.slot(), second.slot());
        assert_ne!(first.generation(), second.generation());
    }
}

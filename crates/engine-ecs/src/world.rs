//! Entity and native component storage.

use std::{any::Any, collections::HashMap, fmt};

use engine_core::{EngineError, EngineResult, Handle, HandleAllocator};

/// Entity handle.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Entity(Handle);

impl Entity {
    /// Creates an entity from an engine handle.
    pub const fn from_handle(handle: Handle) -> Self {
        Self(handle)
    }

    /// Returns the backing handle.
    pub const fn handle(self) -> Handle {
        self.0
    }
}

/// Native component lifecycle hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lifecycle {
    /// Called once before the first update.
    Start,
    /// Called once per variable frame.
    Update,
    /// Called on fixed timestep ticks.
    FixedUpdate,
    /// Called after regular updates.
    LateUpdate,
    /// Called by editor-only ticking.
    EditorUpdate,
}

/// Native Rust component contract.
pub trait Component: Any + Send {
    /// Returns a type-stable display name for diagnostics.
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Called once before the first update.
    fn start(&mut self) {}

    /// Called once per variable frame.
    fn update(&mut self) {}

    /// Called on fixed timestep ticks.
    fn fixed_update(&mut self) {}

    /// Called after regular updates.
    fn late_update(&mut self) {}

    /// Called by editor-only ticking.
    fn editor_update(&mut self) {}

    /// Type-erased mutable access.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct ComponentEntry {
    component: Box<dyn Component>,
    started: bool,
}

impl fmt::Debug for ComponentEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ComponentEntry")
            .field("type_name", &self.component.type_name())
            .field("started", &self.started)
            .finish()
    }
}

/// Component storage keyed by live entity. Components are indexed by TypeId for O(1) lookup.
#[derive(Default)]
pub struct ComponentStorage {
    entries: HashMap<Entity, Vec<ComponentEntry>>,
    /// Per-entity, per-TypeId index into `entries` for O(1) access.
    type_index: HashMap<(Entity, std::any::TypeId), usize>,
}

impl fmt::Debug for ComponentStorage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ComponentStorage")
            .field("entities", &self.entries.len())
            .finish()
    }
}

impl ComponentStorage {
    /// Inserts a native component for an entity.
    pub fn insert<C: Component>(&mut self, entity: Entity, component: C) {
        let type_id = std::any::TypeId::of::<C>();
        let component_list = self.entries.entry(entity).or_default();
        let index = component_list.len();
        component_list.push(ComponentEntry {
            component: Box::new(component),
            started: false,
        });
        self.type_index.insert((entity, type_id), index);
    }

    /// Returns a mutable component reference by concrete type (O(1) via TypeId index).
    pub fn get_mut<C: Component>(&mut self, entity: Entity) -> Option<&mut C> {
        let type_id = std::any::TypeId::of::<C>();
        if let Some(&index) = self.type_index.get(&(entity, type_id)) {
            return self
                .entries
                .get_mut(&entity)?
                .get_mut(index)?
                .component
                .as_any_mut()
                .downcast_mut::<C>();
        }
        // Fallback: linear scan for types registered before the index was added
        self.entries
            .get_mut(&entity)?
            .iter_mut()
            .find_map(|entry| entry.component.as_any_mut().downcast_mut::<C>())
    }

    /// Removes every component attached to an entity.
    pub fn remove_entity(&mut self, entity: Entity) {
        self.entries.remove(&entity);
        self.type_index.retain(|&(e, _), _| e != entity);
    }

    /// Ticks lifecycle hooks in deterministic entity and insertion order.
    pub fn run_lifecycle(&mut self, lifecycle: Lifecycle) {
        let mut entities = self.entries.keys().copied().collect::<Vec<_>>();
        entities.sort_by_key(|entity| entity.handle().slot());

        for entity in entities {
            let Some(components) = self.entries.get_mut(&entity) else {
                continue;
            };
            for entry in components {
                match lifecycle {
                    Lifecycle::Start => {
                        if !entry.started {
                            entry.component.start();
                            entry.started = true;
                        }
                    }
                    Lifecycle::Update => entry.component.update(),
                    Lifecycle::FixedUpdate => entry.component.fixed_update(),
                    Lifecycle::LateUpdate => entry.component.late_update(),
                    Lifecycle::EditorUpdate => entry.component.editor_update(),
                }
            }
        }
    }
}

/// ECS world with entity lifetime and native component tracking.
#[derive(Default)]
pub struct World {
    allocator: HandleAllocator,
    components: ComponentStorage,
}

impl fmt::Debug for World {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("World")
            .field("components", &self.components)
            .finish_non_exhaustive()
    }
}

impl World {
    /// Spawns an empty entity.
    pub fn spawn(&mut self) -> EngineResult<Entity> {
        self.allocator.allocate().map(Entity)
    }

    /// Destroys a live entity.
    pub fn despawn(&mut self, entity: Entity) -> EngineResult<()> {
        self.components.remove_entity(entity);
        self.allocator.free(entity.handle())
    }

    /// Returns whether an entity is currently live.
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.allocator.is_live(entity.handle())
    }

    /// Inserts a component for a live entity.
    pub fn insert_component<C: Component>(
        &mut self,
        entity: Entity,
        component: C,
    ) -> EngineResult<()> {
        if !self.is_alive(entity) {
            return Err(EngineError::invalid_handle(
                "cannot attach a component to a dead entity",
            ));
        }
        self.components.insert(entity, component);
        Ok(())
    }

    /// Returns a mutable component reference by concrete type.
    pub fn component_mut<C: Component>(&mut self, entity: Entity) -> Option<&mut C> {
        self.components.get_mut(entity)
    }

    /// Runs component lifecycle hooks.
    pub fn run_lifecycle(&mut self, lifecycle: Lifecycle) {
        self.components.run_lifecycle(lifecycle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_despawn_entity() {
        let mut world = World::default();
        let entity = world.spawn().unwrap();
        assert!(world.is_alive(entity));
        world.despawn(entity).unwrap();
        assert!(!world.is_alive(entity));
    }

    #[derive(Default)]
    struct Counter {
        updates: u32,
    }

    impl Component for Counter {
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn stores_native_components() {
        let mut world = World::default();
        let entity = world.spawn().unwrap();
        world.insert_component(entity, Counter::default()).unwrap();

        world.component_mut::<Counter>(entity).unwrap().updates += 1;

        assert_eq!(world.component_mut::<Counter>(entity).unwrap().updates, 1);
    }
}

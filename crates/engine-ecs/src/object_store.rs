//! Scene object identity and metadata storage.

use std::{collections::HashMap, fmt};

use engine_core::math::Transform;
use engine_core::{EngineError, EngineResult, EntityId};

use crate::{
    scene::{GameObject, ObjectIdAllocator},
    transform::TransformHierarchy,
    world::{Entity, World},
};

/// Stable reference to a scene object inside serialized data.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ObjectRef {
    id: EntityId,
}

impl ObjectRef {
    /// Creates a stable object reference from a serialized object ID.
    pub(crate) const fn new(id: EntityId) -> Self {
        Self { id }
    }

    /// Returns the referenced stable object ID.
    pub(crate) const fn id(self) -> EntityId {
        self.id
    }
}

impl From<EntityId> for ObjectRef {
    fn from(id: EntityId) -> Self {
        Self::new(id)
    }
}

/// Maps serialized source object references to live entities created during import.
#[derive(Default)]
pub(crate) struct ObjectImportMap {
    by_source: HashMap<ObjectRef, Entity>,
}

impl ObjectImportMap {
    /// Records the live entity created for a serialized source object reference.
    pub(crate) fn insert(&mut self, source: ObjectRef, entity: Entity) {
        self.by_source.insert(source, entity);
    }

    /// Resolves a required serialized source reference to a live imported entity.
    pub(crate) fn resolve_required(&self, source: ObjectRef) -> EngineResult<Entity> {
        self.by_source.get(&source).copied().ok_or_else(|| {
            EngineError::invalid_handle(format!(
                "object import reference {} did not resolve",
                source.id().as_u128()
            ))
        })
    }

    /// Resolves an optional serialized source reference to a live imported entity.
    pub(crate) fn resolve_optional(
        &self,
        source: Option<ObjectRef>,
    ) -> EngineResult<Option<Entity>> {
        source
            .map(|source| self.resolve_required(source))
            .transpose()
    }
}

/// Stores live scene objects, stable object IDs, and object metadata behind one interface.
#[derive(Default)]
pub(crate) struct ObjectStore {
    world: World,
    transforms: TransformHierarchy,
    objects: HashMap<Entity, GameObject>,
    by_id: HashMap<EntityId, Entity>,
    id_allocator: ObjectIdAllocator,
}

impl fmt::Debug for ObjectStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ObjectStore")
            .field("objects", &self.objects.len())
            .finish()
    }
}

impl ObjectStore {
    /// Returns the ECS world that owns live entity handles and native components.
    pub(crate) fn world(&self) -> &World {
        &self.world
    }

    /// Returns the mutable ECS world that owns live entity handles and native components.
    pub(crate) fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Returns the transform hierarchy for live objects.
    pub(crate) fn transforms(&self) -> &TransformHierarchy {
        &self.transforms
    }

    /// Returns the mutable transform hierarchy for live objects.
    pub(crate) fn transforms_mut(&mut self) -> &mut TransformHierarchy {
        &mut self.transforms
    }

    /// Creates a new object with a freshly allocated stable object ID.
    pub(crate) fn spawn(&mut self, name: impl Into<String>) -> EngineResult<Entity> {
        let entity = self.world.spawn()?;
        let object = GameObject::new(self.id_allocator.allocate(), name);
        self.insert_spawned(entity, object);
        self.transforms.set_local(entity, Transform::IDENTITY);
        self.transforms.set_parent(entity, None)?;
        Ok(entity)
    }

    /// Creates an object by cloning metadata from an existing live object.
    pub(crate) fn clone_object(&mut self, source: Entity) -> EngineResult<Entity> {
        self.ensure_alive(source)?;
        let mut cloned = self
            .objects
            .get(&source)
            .ok_or_else(|| EngineError::invalid_handle("source object is missing metadata"))?
            .clone();
        let entity = self.world.spawn()?;
        cloned.id = self.id_allocator.allocate();
        cloned.name = format!("{} (Copy)", cloned.name);
        self.insert_spawned(entity, cloned);
        self.transforms
            .set_local(entity, self.transforms.local(source).unwrap_or_default());
        self.transforms
            .set_parent(entity, self.transforms.parent(source))?;
        Ok(entity)
    }

    /// Creates an object from serialized metadata while preserving its stable object ID.
    pub(crate) fn load_object(
        &mut self,
        mut object: GameObject,
        local_transform: Transform,
    ) -> EngineResult<Entity> {
        let entity = self.world.spawn()?;
        self.id_allocator.observe(object.id);
        self.migrate_duplicate_id(&mut object);
        self.insert_spawned(entity, object);
        self.transforms.set_local(entity, local_transform);
        Ok(entity)
    }

    /// Creates an object from prefab metadata and assigns a new stable object ID.
    pub(crate) fn instantiate_object(
        &mut self,
        mut object: GameObject,
        local_transform: Transform,
    ) -> EngineResult<Entity> {
        let entity = self.world.spawn()?;
        object.id = self.id_allocator.allocate();
        self.insert_spawned(entity, object);
        self.transforms.set_local(entity, local_transform);
        Ok(entity)
    }

    /// Sets a parent relationship between live objects.
    pub(crate) fn set_parent(&mut self, child: Entity, parent: Option<Entity>) -> EngineResult<()> {
        self.ensure_alive(child)?;
        if let Some(parent) = parent {
            self.ensure_alive(parent)?;
        }
        self.transforms.set_parent(child, parent)
    }

    /// Removes object metadata and despawns the live entity if it is still alive.
    pub(crate) fn remove(&mut self, entity: Entity) -> EngineResult<Option<GameObject>> {
        let object = self.objects.remove(&entity);
        if let Some(object) = &object {
            self.by_id.remove(&object.id);
        }
        self.transforms.remove(entity);
        if self.world.is_alive(entity) {
            self.world.despawn(entity)?;
        }
        Ok(object)
    }

    /// Returns immutable object metadata.
    pub(crate) fn object(&self, entity: Entity) -> Option<&GameObject> {
        self.objects.get(&entity)
    }

    /// Returns mutable object metadata.
    pub(crate) fn object_mut(&mut self, entity: Entity) -> Option<&mut GameObject> {
        self.objects.get_mut(&entity)
    }

    /// Iterates object entities and metadata without allocating or sorting.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (Entity, &GameObject)> {
        self.objects
            .iter()
            .map(|(entity, object)| (*entity, object))
    }

    /// Iterates mutable object metadata without exposing storage ownership.
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut GameObject)> {
        self.objects
            .iter_mut()
            .map(|(entity, object)| (*entity, object))
    }

    /// Returns the number of live objects.
    pub(crate) fn len(&self) -> usize {
        self.objects.len()
    }

    /// Resolves a stable object reference to a live entity.
    pub(crate) fn resolve(&self, reference: ObjectRef) -> Option<Entity> {
        self.by_id.get(&reference.id()).copied()
    }

    /// Ensures an entity is currently live in this store.
    pub(crate) fn ensure_alive(&self, entity: Entity) -> EngineResult<()> {
        if self.world.is_alive(entity) {
            Ok(())
        } else {
            Err(EngineError::invalid_handle("scene object is not live"))
        }
    }

    fn insert_spawned(&mut self, entity: Entity, object: GameObject) {
        self.by_id.insert(object.id, entity);
        self.objects.insert(entity, object);
    }

    fn migrate_duplicate_id(&mut self, object: &mut GameObject) {
        if self.by_id.contains_key(&object.id) {
            object.id = self.id_allocator.allocate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_preserves_stable_id() {
        let mut store = ObjectStore::default();
        let id = EntityId::from_u128(42);
        let entity = store
            .load_object(GameObject::new(id, "Loaded"), Transform::IDENTITY)
            .unwrap();

        assert_eq!(store.resolve(ObjectRef::new(id)), Some(entity));
        assert_eq!(store.object(entity).unwrap().id, id);
    }

    #[test]
    fn instantiate_assigns_new_stable_id() {
        let mut store = ObjectStore::default();
        let source_id = EntityId::from_u128(7);
        let entity = store
            .instantiate_object(GameObject::new(source_id, "Prefab"), Transform::IDENTITY)
            .unwrap();

        assert_ne!(store.object(entity).unwrap().id, source_id);
        assert_eq!(store.resolve(ObjectRef::new(source_id)), None);
    }

    #[test]
    fn remove_invalidates_live_entity_and_stable_id_lookup() {
        let mut store = ObjectStore::default();
        let entity = store.spawn("Temp").unwrap();
        let id = store.object(entity).unwrap().id;

        assert!(store.remove(entity).unwrap().is_some());

        assert!(!store.world().is_alive(entity));
        assert_eq!(store.resolve(ObjectRef::new(id)), None);
    }

    #[test]
    fn remove_detaches_children_from_removed_parent() {
        let mut store = ObjectStore::default();
        let parent = store.spawn("Parent").unwrap();
        let child = store.spawn("Child").unwrap();
        store.set_parent(child, Some(parent)).unwrap();

        store.remove(parent).unwrap();

        assert_eq!(store.transforms().parent(child), None);
        assert_eq!(store.transforms().roots(), &[child]);
    }

    #[test]
    fn import_map_resolves_optional_and_required_refs() {
        let mut store = ObjectStore::default();
        let entity = store.spawn("Imported").unwrap();
        let source = ObjectRef::new(EntityId::from_u128(99));
        let mut map = ObjectImportMap::default();

        map.insert(source, entity);

        assert_eq!(map.resolve_required(source).unwrap(), entity);
        assert_eq!(map.resolve_optional(Some(source)).unwrap(), Some(entity));
        assert_eq!(map.resolve_optional(None).unwrap(), None);
        assert!(
            map.resolve_required(ObjectRef::new(EntityId::from_u128(100)))
                .is_err()
        );
    }
}

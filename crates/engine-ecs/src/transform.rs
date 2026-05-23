//! Transform hierarchy storage.

use std::collections::{HashMap, HashSet};

use engine_core::{math::Transform, EngineError, EngineResult};

use crate::Entity;

/// Parent/child transform hierarchy with cached world transforms.
#[derive(Clone, Debug, Default)]
pub struct TransformHierarchy {
    locals: HashMap<Entity, Transform>,
    parents: HashMap<Entity, Entity>,
    children: HashMap<Entity, Vec<Entity>>,
    roots: Vec<Entity>,
    dirty: HashSet<Entity>,
    world_cache: HashMap<Entity, Transform>,
}

impl TransformHierarchy {
    /// Sets or replaces the local transform for an entity.
    pub fn set_local(&mut self, entity: Entity, transform: Transform) {
        self.locals.insert(entity, transform);
        self.world_cache.remove(&entity);
        self.mark_dirty(entity);
    }

    /// Returns the local transform if present.
    pub fn local(&self, entity: Entity) -> Option<Transform> {
        self.locals.get(&entity).copied()
    }

    /// Sets the world-space transform, converting it to local space.
    ///
    /// When the entity has a parent, the local transform is computed as
    /// `parent_world⁻¹ * world`. When there is no parent, local = world.
    pub fn set_world(&mut self, entity: Entity, world: Transform) {
        let local = match self.parent(entity) {
            Some(parent) => self
                .world(parent)
                .map(|parent_world| parent_world.inverse().compose(&world))
                .unwrap_or(world),
            None => world,
        };
        self.set_local(entity, local);
    }

    /// Sets a parent relationship. Parent and child must be distinct and acyclic.
    pub fn set_parent(&mut self, child: Entity, parent: Option<Entity>) -> EngineResult<()> {
        self.clear_parent(child);
        if let Some(parent) = parent {
            if child == parent {
                return Err(EngineError::other("entity cannot parent itself"));
            }
            if self.is_descendant(parent, child) {
                return Err(EngineError::other("transform hierarchy cycle rejected"));
            }
            self.parents.insert(child, parent);
            self.children.entry(parent).or_default().push(child);
            self.roots.retain(|candidate| *candidate != child);
        } else if !self.roots.contains(&child) {
            self.roots.push(child);
        }
        self.mark_dirty(child);
        Ok(())
    }

    /// Clears any parent relationship for an entity.
    pub fn clear_parent(&mut self, child: Entity) {
        if let Some(parent) = self.parents.remove(&child) {
            if let Some(children) = self.children.get_mut(&parent) {
                children.retain(|candidate| *candidate != child);
            }
        }
        if !self.roots.contains(&child) {
            self.roots.push(child);
        }
        self.mark_dirty(child);
    }

    /// Returns the parent for an entity.
    pub fn parent(&self, child: Entity) -> Option<Entity> {
        self.parents.get(&child).copied()
    }

    /// Returns a copy of the current children list.
    pub fn children(&self, parent: Entity) -> Vec<Entity> {
        self.children.get(&parent).cloned().unwrap_or_default()
    }

    /// Returns a reference to the children list (zero-allocation read path).
    pub fn children_ref(&self, parent: Entity) -> &[Entity] {
        self.children.get(&parent).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Returns root entities in sibling order.
    pub fn roots(&self) -> &[Entity] {
        &self.roots
    }

    /// Returns the zero-based sibling index for an entity.
    pub fn sibling_index(&self, entity: Entity) -> Option<usize> {
        if let Some(parent) = self.parent(entity) {
            return self
                .children
                .get(&parent)?
                .iter()
                .position(|candidate| *candidate == entity);
        }
        self.roots.iter().position(|candidate| *candidate == entity)
    }

    /// Removes an entity from hierarchy storage.
    pub fn remove(&mut self, entity: Entity) {
        let children = self.children(entity);
        for child in children {
            self.clear_parent(child);
        }
        self.clear_parent(entity);
        self.locals.remove(&entity);
        self.parents.remove(&entity);
        self.children.remove(&entity);
        self.roots.retain(|candidate| *candidate != entity);
        self.dirty.remove(&entity);
        self.world_cache.remove(&entity);
    }

    /// Returns whether an entity's transform subtree needs recomputation.
    pub fn is_dirty(&self, entity: Entity) -> bool {
        self.dirty.contains(&entity)
    }

    /// Computes the world-space transform for an entity by walking the parent chain.
    ///
    /// If the entity has no local transform, returns `None`.
    /// Composes transforms top-down: root → ... → entity.
    /// Results are cached; the cache is invalidated when any ancestor's local
    /// transform changes via `set_local` or the hierarchy is modified.
    pub fn world(&self, entity: Entity) -> Option<Transform> {
        // Check cache first for clean entities
        if !self.is_dirty(entity) {
            if let Some(&cached) = self.world_cache.get(&entity) {
                return Some(cached);
            }
        }

        let _local = self.locals.get(&entity)?;
        // Walk up to root, collecting entities from child to root
        let mut chain = vec![entity];
        let mut current = entity;
        while let Some(parent) = self.parents.get(&current).copied() {
            chain.push(parent);
            current = parent;
        }
        // Compose top-down: root first, then down to entity
        let mut world = Transform::IDENTITY;
        for ancestor in chain.into_iter().rev() {
            if let Some(local) = self.locals.get(&ancestor).copied() {
                world = world.compose(&local);
            }
        }
        Some(world)
    }

    /// Clears all dirty transform markers after recomputation, caching computed worlds.
    pub fn clear_dirty(&mut self) {
        // Cache world transforms for all dirty entities before clearing flags
        let dirty_entities: Vec<Entity> = self.dirty.iter().copied().collect();
        for entity in dirty_entities {
            if let Some(world) = self.world(entity) {
                self.world_cache.insert(entity, world);
            }
        }
        self.dirty.clear();
    }

    fn mark_dirty(&mut self, entity: Entity) {
        self.dirty.insert(entity);
        self.world_cache.remove(&entity);
        for child in self.children(entity) {
            self.mark_dirty(child);
        }
    }

    fn is_descendant(&self, entity: Entity, possible_ancestor: Entity) -> bool {
        let mut current = Some(entity);
        while let Some(candidate) = current {
            if candidate == possible_ancestor {
                return true;
            }
            current = self.parent(candidate);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use engine_core::math::{Transform, Vec3};
    use engine_core::HandleAllocator;

    use super::*;

    fn entity(allocator: &mut HandleAllocator) -> Entity {
        Entity::from_handle(allocator.allocate().unwrap())
    }

    #[test]
    fn rejects_cycles() {
        let mut allocator = HandleAllocator::default();
        let root = entity(&mut allocator);
        let child = entity(&mut allocator);
        let mut hierarchy = TransformHierarchy::default();

        hierarchy.set_parent(child, Some(root)).unwrap();
        assert!(hierarchy.set_parent(root, Some(child)).is_err());
    }

    #[test]
    fn computes_world_transform_from_parent_chain() {
        let mut allocator = HandleAllocator::default();
        let root = entity(&mut allocator);
        let child = entity(&mut allocator);
        let grandchild = entity(&mut allocator);
        let mut hierarchy = TransformHierarchy::default();

        hierarchy.set_parent(child, Some(root)).unwrap();
        hierarchy.set_parent(grandchild, Some(child)).unwrap();

        hierarchy.set_local(
            root,
            Transform {
                translation: Vec3::new(1.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
        );
        hierarchy.set_local(
            child,
            Transform {
                translation: Vec3::new(2.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
        );
        hierarchy.set_local(
            grandchild,
            Transform {
                translation: Vec3::new(4.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
        );

        let world_root = hierarchy.world(root).unwrap();
        assert!((world_root.translation.x - 1.0).abs() < 0.001);

        let world_child = hierarchy.world(child).unwrap();
        assert!((world_child.translation.x - 3.0).abs() < 0.001);

        let world_grandchild = hierarchy.world(grandchild).unwrap();
        assert!((world_grandchild.translation.x - 7.0).abs() < 0.001);
    }

    #[test]
    fn set_world_correctly_preserves_local() {
        let mut allocator = HandleAllocator::default();
        let root = entity(&mut allocator);
        let child = entity(&mut allocator);
        let mut hierarchy = TransformHierarchy::default();

        hierarchy.set_parent(child, Some(root)).unwrap();
        hierarchy.set_local(
            root,
            Transform {
                translation: Vec3::new(10.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
        );
        hierarchy.set_local(child, Transform::IDENTITY);

        // Set child world position to (15, 0, 0)
        hierarchy.set_world(
            child,
            Transform {
                translation: Vec3::new(15.0, 0.0, 0.0),
                ..Transform::IDENTITY
            },
        );

        // Child local should be (5, 0, 0) since parent is at 10
        let local_child = hierarchy.local(child).unwrap();
        assert!((local_child.translation.x - 5.0).abs() < 0.001);

        // World should be 15
        let world_child = hierarchy.world(child).unwrap();
        assert!((world_child.translation.x - 15.0).abs() < 0.001);
    }

    #[test]
    fn tracks_roots_and_dirty_children() {
        let mut allocator = HandleAllocator::default();
        let root = entity(&mut allocator);
        let child = entity(&mut allocator);
        let mut hierarchy = TransformHierarchy::default();

        hierarchy.set_parent(root, None).unwrap();
        hierarchy.set_parent(child, Some(root)).unwrap();
        hierarchy.clear_dirty();
        hierarchy.set_local(root, Transform::IDENTITY);

        assert_eq!(hierarchy.roots(), &[root]);
        assert_eq!(hierarchy.sibling_index(child), Some(0));
        assert!(hierarchy.is_dirty(root));
        assert!(hierarchy.is_dirty(child));
    }
}

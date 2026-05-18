//! Transform hierarchy storage.

use std::collections::{HashMap, HashSet};

use engine_core::{math::Transform, EngineError, EngineResult};

use crate::Entity;

/// Parent/child transform hierarchy.
#[derive(Clone, Debug, Default)]
pub struct TransformHierarchy {
    locals: HashMap<Entity, Transform>,
    parents: HashMap<Entity, Entity>,
    children: HashMap<Entity, Vec<Entity>>,
    roots: Vec<Entity>,
    dirty: HashSet<Entity>,
}

impl TransformHierarchy {
    /// Sets or replaces the local transform for an entity.
    pub fn set_local(&mut self, entity: Entity, transform: Transform) {
        self.locals.insert(entity, transform);
        self.mark_dirty(entity);
    }

    /// Returns the local transform if present.
    pub fn local(&self, entity: Entity) -> Option<Transform> {
        self.locals.get(&entity).copied()
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
    }

    /// Returns whether an entity's transform subtree needs recomputation.
    pub fn is_dirty(&self, entity: Entity) -> bool {
        self.dirty.contains(&entity)
    }

    /// Clears all dirty transform markers after recomputation.
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    fn mark_dirty(&mut self, entity: Entity) {
        self.dirty.insert(entity);
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

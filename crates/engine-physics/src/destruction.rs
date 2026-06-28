use std::collections::HashMap;

use crate::{
    BodyHandle, Vec3,
    fracture::{FractureConfig, FractureFragment, FractureSystem},
};

/// Stable identifier for a destructible object tracked by [`DestructionWorld`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DestructibleHandle(pub u64);

/// Mesh and fracture settings for a destructible rigid body.
#[derive(Clone, Debug, PartialEq)]
pub struct DestructibleDesc {
    /// Body that owns the intact object.
    pub body: BodyHandle,
    /// Flat vertex positions (x, y, z triplets).
    pub vertices: Vec<f32>,
    /// Triangle indices.
    pub indices: Vec<u32>,
    /// Fracture configuration used once the damage threshold is crossed.
    pub fracture: FractureConfig,
    /// Accumulated strain needed before the object breaks.
    pub damage_threshold: f32,
    /// Whether damage is ignored.
    pub unbreakable: bool,
}

impl DestructibleDesc {
    /// Creates a destructible description for a body and mesh.
    pub fn new(body: BodyHandle, vertices: Vec<f32>, indices: Vec<u32>) -> Self {
        Self {
            body,
            vertices,
            indices,
            fracture: FractureConfig::default(),
            damage_threshold: 1.0,
            unbreakable: false,
        }
    }
}

/// Damage applied to a destructible object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DestructionDamage {
    /// Strain added by this hit.
    pub amount: f32,
    /// World-space hit point used by fracture seed generation.
    pub impact_point: Vec3,
    /// Direction used to build fragment impulses.
    pub impulse_direction: Vec3,
}

impl DestructionDamage {
    /// Creates radial damage at an impact point.
    pub fn radial(amount: f32, impact_point: Vec3, impulse_direction: Vec3) -> Self {
        Self {
            amount,
            impact_point,
            impulse_direction,
        }
    }
}

/// Fragment body description emitted after a destructible object breaks.
#[derive(Clone, Debug, PartialEq)]
pub struct DestructionFragment {
    /// Mesh fragment.
    pub mesh: FractureFragment,
    /// Linear impulse suggested for the fragment body.
    pub impulse: Vec3,
    /// Lifetime in seconds before cleanup.
    pub lifetime: f32,
}

/// Destruction event stream.
#[derive(Clone, Debug, PartialEq)]
pub enum DestructionEvent {
    /// A destructible object crossed its strain threshold and produced fragments.
    Breaking {
        /// Broken destructible object.
        handle: DestructibleHandle,
        /// Body that owned the intact object.
        body: BodyHandle,
        /// World-space hit point that triggered the break.
        impact_point: Vec3,
        /// Fragments to spawn.
        fragments: Vec<DestructionFragment>,
    },
    /// A spawned fragment should be removed.
    Removal {
        /// Fragment body whose lifetime expired.
        body: BodyHandle,
    },
}

#[derive(Clone, Debug)]
struct DestructibleState {
    desc: DestructibleDesc,
    strain: f32,
    broken: bool,
}

#[derive(Clone, Debug)]
struct FragmentLifetime {
    body: BodyHandle,
    remaining: f32,
}

/// Runtime module for destructible body strain, fracture, and cleanup events.
#[derive(Clone, Debug, Default)]
pub struct DestructionWorld {
    next_handle: u64,
    destructibles: HashMap<DestructibleHandle, DestructibleState>,
    by_body: HashMap<BodyHandle, DestructibleHandle>,
    fragment_lifetimes: Vec<FragmentLifetime>,
    events: Vec<DestructionEvent>,
}

impl DestructionWorld {
    /// Creates an empty destruction world.
    pub fn new() -> Self {
        Self {
            next_handle: 1,
            ..Self::default()
        }
    }

    /// Registers a destructible body and returns its handle.
    pub fn register_body(&mut self, desc: DestructibleDesc) -> DestructibleHandle {
        if let Some(handle) = self.by_body.get(&desc.body).copied() {
            if let Some(state) = self.destructibles.get_mut(&handle) {
                *state = DestructibleState {
                    desc,
                    strain: 0.0,
                    broken: false,
                };
            }
            return handle;
        }

        let handle = DestructibleHandle(self.next_handle);
        self.next_handle = self.next_handle.saturating_add(1).max(1);
        self.by_body.insert(desc.body, handle);
        self.destructibles.insert(
            handle,
            DestructibleState {
                desc,
                strain: 0.0,
                broken: false,
            },
        );
        handle
    }

    /// Returns the destructible handle for a body, if registered.
    pub fn handle_for_body(&self, body: BodyHandle) -> Option<DestructibleHandle> {
        self.by_body.get(&body).copied()
    }

    /// Returns accumulated strain for a destructible object.
    pub fn strain(&self, handle: DestructibleHandle) -> Option<f32> {
        self.destructibles.get(&handle).map(|state| state.strain)
    }

    /// Applies damage. If the threshold is crossed, a breaking event is queued.
    pub fn apply_damage(&mut self, handle: DestructibleHandle, damage: DestructionDamage) -> bool {
        let Some(state) = self.destructibles.get_mut(&handle) else {
            return false;
        };
        if state.broken || state.desc.unbreakable || damage.amount <= 0.0 {
            return false;
        }

        state.strain += damage.amount;
        let threshold = state.desc.damage_threshold.max(0.0);
        if state.strain < threshold {
            return false;
        }

        state.broken = true;
        let fragments = FractureSystem::fracture_mesh(
            &state.desc.vertices,
            &state.desc.indices,
            &state.desc.fracture,
            damage.impact_point,
        )
        .into_iter()
        .map(|mesh| {
            let direction = fragment_impulse_direction(
                mesh.centroid,
                damage.impact_point,
                damage.impulse_direction,
            );
            DestructionFragment {
                mesh,
                impulse: direction * state.desc.fracture.impulse_strength.max(0.0),
                lifetime: state.desc.fracture.fragment_lifetime.max(0.0),
            }
        })
        .collect::<Vec<_>>();

        self.events.push(DestructionEvent::Breaking {
            handle,
            body: state.desc.body,
            impact_point: damage.impact_point,
            fragments,
        });
        true
    }

    /// Tracks a spawned fragment body for later cleanup.
    pub fn register_fragment_body(&mut self, body: BodyHandle, lifetime: f32) {
        self.fragment_lifetimes.push(FragmentLifetime {
            body,
            remaining: lifetime.max(0.0),
        });
    }

    /// Advances fragment lifetimes and queues removal events.
    pub fn tick(&mut self, dt: f32) {
        if dt <= f32::EPSILON {
            return;
        }

        let mut index = 0;
        while index < self.fragment_lifetimes.len() {
            let lifetime = &mut self.fragment_lifetimes[index];
            lifetime.remaining -= dt;
            if lifetime.remaining <= 0.0 {
                let expired = self.fragment_lifetimes.swap_remove(index);
                self.events
                    .push(DestructionEvent::Removal { body: expired.body });
            } else {
                index += 1;
            }
        }
    }

    /// Drains queued destruction events.
    pub fn drain_events(&mut self) -> Vec<DestructionEvent> {
        std::mem::take(&mut self.events)
    }
}

fn fragment_impulse_direction(centroid: Vec3, impact_point: Vec3, fallback: Vec3) -> Vec3 {
    let radial = centroid - impact_point;
    if radial.length_squared() > f32::EPSILON {
        radial.normalized()
    } else if fallback.length_squared() > f32::EPSILON {
        fallback.normalized()
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fracture::FracturePattern;

    fn unit_cube() -> (Vec<f32>, Vec<u32>) {
        let vertices = vec![
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0,
            1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0,
        ];
        let indices = vec![
            0, 1, 2, 0, 2, 3, 5, 4, 7, 5, 7, 6, 4, 0, 3, 4, 3, 7, 1, 5, 6, 1, 6, 2, 3, 2, 6, 3, 6,
            7, 4, 5, 1, 4, 1, 0,
        ];
        (vertices, indices)
    }

    #[test]
    fn damage_below_threshold_accumulates_without_breaking() {
        let (vertices, indices) = unit_cube();
        let mut world = DestructionWorld::new();
        let handle = world.register_body(DestructibleDesc {
            body: BodyHandle(7),
            vertices,
            indices,
            damage_threshold: 10.0,
            ..DestructibleDesc::new(BodyHandle(7), Vec::new(), Vec::new())
        });

        assert!(!world.apply_damage(
            handle,
            DestructionDamage::radial(3.0, Vec3::new(0.5, 0.5, 0.5), Vec3::new(0.0, 1.0, 0.0)),
        ));
        assert_eq!(world.strain(handle), Some(3.0));
        assert!(world.drain_events().is_empty());
    }

    #[test]
    fn threshold_crossing_emits_breaking_event_once() {
        let (vertices, indices) = unit_cube();
        let mut world = DestructionWorld::new();
        let handle = world.register_body(DestructibleDesc {
            body: BodyHandle(3),
            vertices,
            indices,
            fracture: FractureConfig {
                pattern: FracturePattern::VoronoiRandom { count: 4, seed: 1 },
                min_fragment_size: 0.0,
                fragment_lifetime: 2.5,
                impulse_strength: 9.0,
                ..FractureConfig::default()
            },
            damage_threshold: 5.0,
            unbreakable: false,
        });

        assert!(world.apply_damage(
            handle,
            DestructionDamage::radial(5.0, Vec3::new(0.5, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
        ));
        assert!(!world.apply_damage(
            handle,
            DestructionDamage::radial(5.0, Vec3::new(0.5, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0)),
        ));

        let events = world.drain_events();
        assert_eq!(events.len(), 1);
        let DestructionEvent::Breaking {
            body, fragments, ..
        } = &events[0]
        else {
            panic!("expected breaking event");
        };
        assert_eq!(*body, BodyHandle(3));
        assert!(!fragments.is_empty());
        assert!(fragments.iter().all(|fragment| fragment.lifetime == 2.5));
        assert!(
            fragments
                .iter()
                .any(|fragment| fragment.impulse.length() > 0.0)
        );
    }

    #[test]
    fn fragment_lifetime_emits_removal_event() {
        let mut world = DestructionWorld::new();
        world.register_fragment_body(BodyHandle(11), 0.5);

        world.tick(0.25);
        assert!(world.drain_events().is_empty());

        world.tick(0.25);
        assert_eq!(
            world.drain_events(),
            vec![DestructionEvent::Removal {
                body: BodyHandle(11)
            }]
        );
    }
}

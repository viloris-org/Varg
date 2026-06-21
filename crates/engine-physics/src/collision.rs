use std::collections::HashMap;

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

use crate::ColliderShape;

// ── Collision profile system ─────────────────────────────────────────────────

/// Collision response type for a channel pair.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionResponse {
    /// Bodies block each other (physical contact).
    #[default]
    Block,
    /// Bodies fire overlap events but do not physically interact.
    Overlap,
    /// Bodies ignore each other completely.
    Ignore,
}

/// A named collision channel with a default response.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CollisionChannel {
    /// Name of the channel (e.g. "WorldStatic", "WorldDynamic", "Pawn", "Projectile").
    pub name: String,
    /// Default response when this channel interacts with itself.
    pub self_response: CollisionResponse,
}

/// Collision profile: a named preset that maps channel pairs to responses.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CollisionProfile {
    /// Name of this profile (e.g. "Pawn", "PhysicsBody", "NoCollision").
    pub name: String,
    /// The collision channel this object belongs to.
    pub channel: String,
    /// Per-channel response overrides. If a channel is not listed, the default
    /// response is [`CollisionResponse::Block`].
    pub responses: HashMap<String, CollisionResponse>,
    /// Whether this collider can generate overlap events.
    pub generate_overlap_events: bool,
}

impl Default for CollisionProfile {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            channel: "WorldStatic".into(),
            responses: HashMap::new(),
            generate_overlap_events: false,
        }
    }
}

/// Manages collision channels and profile presets.
#[derive(Clone, Debug)]
pub struct CollisionProfileRegistry {
    channels: Vec<CollisionChannel>,
    matrix: HashMap<(String, String), CollisionResponse>,
}

impl Default for CollisionProfileRegistry {
    fn default() -> Self {
        let mut reg = Self {
            channels: Vec::new(),
            matrix: HashMap::new(),
        };
        reg.register_channel(CollisionChannel {
            name: "WorldStatic".into(),
            self_response: CollisionResponse::Block,
        });
        reg.register_channel(CollisionChannel {
            name: "WorldDynamic".into(),
            self_response: CollisionResponse::Block,
        });
        reg.register_channel(CollisionChannel {
            name: "Pawn".into(),
            self_response: CollisionResponse::Block,
        });
        reg.register_channel(CollisionChannel {
            name: "Projectile".into(),
            self_response: CollisionResponse::Overlap,
        });
        reg.register_channel(CollisionChannel {
            name: "Trigger".into(),
            self_response: CollisionResponse::Overlap,
        });
        // Default matrix: everything blocks everything except triggers/overlap channels.
        reg.set_response("Pawn", "Pawn", CollisionResponse::Block);
        reg.set_response("Pawn", "Projectile", CollisionResponse::Overlap);
        reg.set_response("Projectile", "Trigger", CollisionResponse::Overlap);
        reg.set_response("WorldStatic", "Projectile", CollisionResponse::Overlap);
        reg
    }
}

impl CollisionProfileRegistry {
    /// Registers a new collision channel.
    pub fn register_channel(&mut self, channel: CollisionChannel) {
        self.channels.push(channel);
    }

    /// Sets the response between two channels (symmetric).
    pub fn set_response(&mut self, a: &str, b: &str, response: CollisionResponse) {
        self.matrix.insert((a.into(), b.into()), response);
        self.matrix.insert((b.into(), a.into()), response);
    }

    /// Returns the response between two channels.
    pub fn response(&self, a: &str, b: &str) -> CollisionResponse {
        self.matrix
            .get(&(a.into(), b.into()))
            .copied()
            .unwrap_or_else(|| {
                if a == b {
                    self.channels
                        .iter()
                        .find(|channel| channel.name == a)
                        .map_or(CollisionResponse::Block, |channel| channel.self_response)
                } else {
                    CollisionResponse::Block
                }
            })
    }

    fn profile_response(
        &self,
        source: &CollisionProfile,
        target: &CollisionProfile,
    ) -> CollisionResponse {
        source
            .responses
            .get(&target.channel)
            .copied()
            .unwrap_or_else(|| self.response(&source.channel, &target.channel))
    }

    fn combined_response(&self, a: &CollisionProfile, b: &CollisionProfile) -> CollisionResponse {
        use CollisionResponse::{Block, Ignore, Overlap};

        match (self.profile_response(a, b), self.profile_response(b, a)) {
            (Ignore, _) | (_, Ignore) => Ignore,
            (Overlap, _) | (_, Overlap) => Overlap,
            (Block, Block) => Block,
        }
    }

    /// Resolves whether two profiles should generate a blocking contact.
    pub fn should_collide(&self, a: &CollisionProfile, b: &CollisionProfile) -> bool {
        self.combined_response(a, b) == CollisionResponse::Block
    }

    /// Resolves whether two profiles should generate an overlap event.
    pub fn should_overlap(&self, a: &CollisionProfile, b: &CollisionProfile) -> bool {
        if !a.generate_overlap_events && !b.generate_overlap_events {
            return false;
        }
        self.combined_response(a, b) == CollisionResponse::Overlap
    }
}

pub(crate) fn validate_collider_shape(shape: &ColliderShape) -> EngineResult<()> {
    match shape {
        ColliderShape::Heightfield {
            num_x,
            num_z,
            heights,
            scale,
        } => {
            if *num_x < 2 || *num_z < 2 {
                return Err(EngineError::config(
                    "heightfield dimensions must both be at least 2",
                ));
            }
            let expected = (*num_x as usize)
                .checked_mul(*num_z as usize)
                .ok_or_else(|| EngineError::config("heightfield dimensions overflow"))?;
            if heights.len() != expected {
                return Err(EngineError::config(format!(
                    "heightfield expected {expected} samples, got {}",
                    heights.len()
                )));
            }
            if !scale.x.is_finite()
                || !scale.y.is_finite()
                || !scale.z.is_finite()
                || scale.x <= 0.0
                || scale.y <= 0.0
                || scale.z <= 0.0
                || heights.iter().any(|height| !height.is_finite())
            {
                return Err(EngineError::config(
                    "heightfield scale and samples must be finite and scale must be positive",
                ));
            }
        }
        ColliderShape::Mesh { vertices } | ColliderShape::TriMesh { vertices, .. } => {
            if vertices.len() % 3 != 0 || vertices.iter().any(|value| !value.is_finite()) {
                return Err(EngineError::config(
                    "mesh collider vertices must contain finite xyz triplets",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

// ── Physical material ────────────────────────────────────────────────────────

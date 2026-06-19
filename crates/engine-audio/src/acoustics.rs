//! Device-independent practical acoustic propagation.

use std::collections::HashMap;

use engine_core::math::Vec3;
use serde::{Deserialize, Serialize};

use crate::{PropagationFrame, SourceHandle};

/// Frequency-band acoustic material parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcousticMaterial {
    /// Low/mid/high absorption coefficients in `[0.0, 1.0]`.
    pub absorption: [f32; 3],
    /// Low/mid/high transmission coefficients in `[0.0, 1.0]`.
    pub transmission: [f32; 3],
    /// Diffuse scattering coefficient in `[0.0, 1.0]`.
    pub scattering: f32,
}

impl Default for AcousticMaterial {
    fn default() -> Self {
        Self {
            absorption: [0.2, 0.3, 0.45],
            transmission: [0.35, 0.2, 0.08],
            scattering: 0.25,
        }
    }
}

/// Simplified acoustic geometry primitive.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcousticAabb {
    /// Minimum world-space corner.
    pub min: Vec3,
    /// Maximum world-space corner.
    pub max: Vec3,
    /// Material parameters for this blocker.
    pub material: AcousticMaterial,
    /// Whether this blocker affects direct-path propagation.
    pub blocks_direct_path: bool,
}

/// Source sample consumed by the acoustic solver.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcousticSourceSample {
    /// Source handle.
    pub handle: SourceHandle,
    /// World-space source position.
    pub position: Vec3,
}

/// Immutable acoustic scene snapshot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AcousticSceneSnapshot {
    /// Active listener position.
    pub listener_position: Vec3,
    /// Relevant source samples.
    pub sources: Vec<AcousticSourceSample>,
    /// Simplified direct-path blockers.
    pub blockers: Vec<AcousticAabb>,
}

/// Acoustic quality tier controlling query budgets.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcousticQuality {
    /// Disable environmental acoustics.
    Off,
    /// Single direct-path blocker query.
    #[default]
    Low,
    /// Multiple blockers can accumulate.
    Medium,
}

/// Acoustic solver configuration.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcousticSolverConfig {
    /// Quality tier.
    pub quality: AcousticQuality,
    /// Maximum blockers tested per source.
    pub max_blockers_per_source: usize,
}

impl Default for AcousticSolverConfig {
    fn default() -> Self {
        Self {
            quality: AcousticQuality::Low,
            max_blockers_per_source: 8,
        }
    }
}

/// Computes direct-path propagation frames for the supplied snapshot.
pub fn solve_direct_propagation(
    snapshot: &AcousticSceneSnapshot,
    config: AcousticSolverConfig,
) -> HashMap<SourceHandle, PropagationFrame> {
    let mut frames = HashMap::with_capacity(snapshot.sources.len());
    for source in &snapshot.sources {
        let mut direct_gain = 1.0_f32;
        let mut cutoff = 20_000.0_f32;
        let mut reverb_send = 0.0_f32;
        if config.quality != AcousticQuality::Off {
            let mut tested = 0_usize;
            for blocker in snapshot
                .blockers
                .iter()
                .filter(|blocker| blocker.blocks_direct_path)
            {
                if tested >= config.max_blockers_per_source {
                    break;
                }
                tested += 1;
                if !segment_intersects_aabb(source.position, snapshot.listener_position, *blocker) {
                    continue;
                }
                let transmission = blocker.material.transmission;
                let high_loss = 1.0 - transmission[2].clamp(0.0, 1.0);
                let mid_loss = 1.0 - transmission[1].clamp(0.0, 1.0);
                direct_gain *= transmission.iter().copied().sum::<f32>().clamp(0.0, 3.0) / 3.0;
                cutoff = cutoff.min(20_000.0 - high_loss * 15_000.0 - mid_loss * 3_000.0);
                reverb_send = reverb_send.max(blocker.material.scattering.clamp(0.0, 1.0) * 0.35);
                if config.quality == AcousticQuality::Low {
                    break;
                }
            }
        }
        frames.insert(
            source.handle,
            PropagationFrame {
                direct_gain,
                low_pass_hz: cutoff,
                reverb_send,
                delay_seconds: 0.0,
            }
            .sanitized(),
        );
    }
    frames
}

fn segment_intersects_aabb(start: Vec3, end: Vec3, aabb: AcousticAabb) -> bool {
    let direction = end - start;
    let mut t_min = 0.0_f32;
    let mut t_max = 1.0_f32;
    for axis in 0..3 {
        let origin = component(start, axis);
        let delta = component(direction, axis);
        let min = component(aabb.min, axis).min(component(aabb.max, axis));
        let max = component(aabb.min, axis).max(component(aabb.max, axis));
        if delta.abs() <= f32::EPSILON {
            if origin < min || origin > max {
                return false;
            }
            continue;
        }
        let inv_delta = 1.0 / delta;
        let mut near = (min - origin) * inv_delta;
        let mut far = (max - origin) * inv_delta;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        t_min = t_min.max(near);
        t_max = t_max.min(far);
        if t_min > t_max {
            return false;
        }
    }
    true
}

fn component(value: Vec3, axis: usize) -> f32 {
    match axis {
        0 => value.x,
        1 => value.y,
        _ => value.z,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocker_reduces_direct_gain_and_cutoff() {
        let material = AcousticMaterial {
            transmission: [0.5, 0.25, 0.05],
            ..AcousticMaterial::default()
        };
        let snapshot = AcousticSceneSnapshot {
            listener_position: Vec3::ZERO,
            sources: vec![AcousticSourceSample {
                handle: SourceHandle(1),
                position: Vec3::new(0.0, 0.0, -10.0),
            }],
            blockers: vec![AcousticAabb {
                min: Vec3::new(-1.0, -1.0, -5.0),
                max: Vec3::new(1.0, 1.0, -4.0),
                material,
                blocks_direct_path: true,
            }],
        };
        let frames = solve_direct_propagation(&snapshot, AcousticSolverConfig::default());
        let frame = frames[&SourceHandle(1)];
        assert!(frame.direct_gain < 1.0);
        assert!(frame.low_pass_hz < 20_000.0);
    }

    #[test]
    fn off_quality_preserves_direct_path() {
        let snapshot = AcousticSceneSnapshot {
            listener_position: Vec3::ZERO,
            sources: vec![AcousticSourceSample {
                handle: SourceHandle(1),
                position: Vec3::new(0.0, 0.0, -10.0),
            }],
            blockers: vec![AcousticAabb {
                min: Vec3::new(-1.0, -1.0, -5.0),
                max: Vec3::new(1.0, 1.0, -4.0),
                material: AcousticMaterial::default(),
                blocks_direct_path: true,
            }],
        };
        let frames = solve_direct_propagation(
            &snapshot,
            AcousticSolverConfig {
                quality: AcousticQuality::Off,
                ..AcousticSolverConfig::default()
            },
        );
        assert_eq!(frames[&SourceHandle(1)], PropagationFrame::default());
    }
}

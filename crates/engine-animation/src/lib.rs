#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Animation system: keyframe clips, animation player, blend tree, and tweens.

use std::collections::HashMap;

use engine_core::math::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// Interpolation mode between keyframes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum InterpolationMode {
    /// No interpolation; snap to next value.
    Step,
    /// Linear interpolation.
    Linear,
    /// Cubic spline interpolation.
    Cubic,
}

/// A keyframe value at a specific time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    /// Time in seconds.
    pub time: f32,
    /// Value at this keyframe.
    pub value: KeyframeValue,
    /// Interpolation mode from this keyframe to the next.
    pub interpolation: InterpolationMode,
}

/// Value stored at a keyframe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum KeyframeValue {
    /// Float value.
    Float(f32),
    /// 3D vector.
    Vec3(Vec3),
    /// Quaternion rotation.
    Quat(Quat),
    /// Boolean value.
    Bool(bool),
}

/// A track of keyframes targeting a specific property path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationTrack {
    /// Property path (e.g., "transform.translation", "components.Light.intensity").
    pub path: String,
    /// Keyframes for this track.
    pub keyframes: Vec<Keyframe>,
}

/// Animation clip containing keyframe tracks and metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationClip {
    /// Clip name.
    pub name: String,
    /// Duration in seconds.
    pub duration: f32,
    /// Tracks keyed by property path.
    pub tracks: Vec<AnimationTrack>,
    /// Loop behavior.
    #[serde(default)]
    pub loop_mode: LoopMode,
}

/// Loop behavior for animation clips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum LoopMode {
    /// Play once and stop.
    #[default]
    Once,
    /// Loop continuously.
    Loop,
    /// Play forward then reverse.
    PingPong,
}

/// Easing functions for tweens.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    /// Linear interpolation.
    Linear,
    /// Quadratic ease in.
    EaseInQuad,
    /// Quadratic ease out.
    EaseOutQuad,
    /// Quadratic ease in-out.
    EaseInOutQuad,
    /// Cubic ease in.
    EaseInCubic,
    /// Cubic ease out.
    EaseOutCubic,
    /// Cubic ease in-out.
    EaseInOutCubic,
    /// Elastic ease out.
    EaseOutElastic,
    /// Bounce ease out.
    EaseOutBounce,
}

/// Evaluates an easing function at time t in [0, 1].
pub fn evaluate_easing(easing: Easing, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match easing {
        Easing::Linear => t,
        Easing::EaseInQuad => t * t,
        Easing::EaseOutQuad => t * (2.0 - t),
        Easing::EaseInOutQuad => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                -1.0 + (4.0 - 2.0 * t) * t
            }
        }
        Easing::EaseInCubic => t * t * t,
        Easing::EaseOutCubic => {
            let t1 = t - 1.0;
            t1 * t1 * t1 + 1.0
        }
        Easing::EaseInOutCubic => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                let t1 = t - 1.0;
                4.0 * t1 * t1 * t1 + 1.0
            }
        }
        Easing::EaseOutElastic => {
            if t == 0.0 || t == 1.0 {
                return t;
            }
            let c4 = (2.0 * std::f32::consts::PI) / 3.0;
            -2.0_f32.powf(10.0 * t - 10.0) * ((t * 10.0 - 10.75) * c4).sin() + 1.0
        }
        Easing::EaseOutBounce => {
            let n1 = 7.5625;
            let d1 = 2.75;
            if t < 1.0 / d1 {
                n1 * t * t
            } else if t < 2.0 / d1 {
                let t1 = t - 1.5 / d1;
                n1 * t1 * t1 + 0.75
            } else if t < 2.5 / d1 {
                let t1 = t - 2.25 / d1;
                n1 * t1 * t1 + 0.9375
            } else {
                let t1 = t - 2.625 / d1;
                n1 * t1 * t1 + 0.984375
            }
        }
    }
}

/// Samples an animation clip at a given time.
pub fn sample_clip(clip: &AnimationClip, time: f32) -> HashMap<String, KeyframeValue> {
    let time = match clip.loop_mode {
        LoopMode::Once => time.min(clip.duration),
        LoopMode::Loop => {
            if clip.duration > 0.0 {
                time % clip.duration
            } else {
                time
            }
        }
        LoopMode::PingPong => {
            if clip.duration > 0.0 {
                let cycle = time / clip.duration;
                let phase = cycle % 2.0;
                if phase < 1.0 {
                    phase * clip.duration
                } else {
                    (2.0 - phase) * clip.duration
                }
            } else {
                time
            }
        }
    };

    let mut values = HashMap::new();
    for track in &clip.tracks {
        if let Some(value) = sample_track(track, time) {
            values.insert(track.path.clone(), value);
        }
    }
    values
}

fn sample_track(track: &AnimationTrack, time: f32) -> Option<KeyframeValue> {
    if track.keyframes.is_empty() {
        return None;
    }

    if time <= track.keyframes[0].time {
        return Some(track.keyframes[0].value.clone());
    }

    if time >= track.keyframes.last().unwrap().time {
        return Some(track.keyframes.last().unwrap().value.clone());
    }

    // Binary search: find first keyframe with time > target
    let idx = track
        .keyframes
        .partition_point(|kf| kf.time <= time);
    if idx > 0 && idx < track.keyframes.len() {
        let k0 = &track.keyframes[idx - 1];
        let k1 = &track.keyframes[idx];
        let t = if (k1.time - k0.time).abs() > f32::EPSILON {
            (time - k0.time) / (k1.time - k0.time)
        } else {
            0.0
        };
        return Some(interpolate_value(&k0.value, &k1.value, t, k0.interpolation));
    }

    Some(track.keyframes.last().unwrap().value.clone())
}

fn interpolate_value(
    a: &KeyframeValue,
    b: &KeyframeValue,
    t: f32,
    mode: InterpolationMode,
) -> KeyframeValue {
    match mode {
        InterpolationMode::Step => a.clone(),
        InterpolationMode::Linear | InterpolationMode::Cubic => match (a, b) {
            (KeyframeValue::Float(av), KeyframeValue::Float(bv)) => {
                KeyframeValue::Float(av + (bv - av) * t)
            }
            (KeyframeValue::Vec3(av), KeyframeValue::Vec3(bv)) => {
                KeyframeValue::Vec3(av.lerp(*bv, t))
            }
            (KeyframeValue::Quat(av), KeyframeValue::Quat(bv)) => {
                KeyframeValue::Quat(slerp(*av, *bv, t))
            }
            _ => a.clone(),
        },
    }
}

fn slerp(a: Quat, b: Quat, t: f32) -> Quat {
    let dot = (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w).clamp(-1.0, 1.0);
    if dot > 0.9995 {
        let result = Quat {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
            z: a.z + (b.z - a.z) * t,
            w: a.w + (b.w - a.w) * t,
        };
        let len = (result.x * result.x + result.y * result.y + result.z * result.z + result.w * result.w).sqrt();
        if len > f32::EPSILON {
            return Quat {
                x: result.x / len,
                y: result.y / len,
                z: result.z / len,
                w: result.w / len,
            };
        }
        return result;
    }

    let theta_0 = dot.acos();
    let theta = theta_0 * t;
    let sin_theta = theta.sin();
    let sin_theta_0 = theta_0.sin();

    let scale_a = (theta_0 - theta).cos() - dot * sin_theta / sin_theta_0;
    let scale_b = sin_theta / sin_theta_0;

    Quat {
        x: scale_a * a.x + scale_b * b.x,
        y: scale_a * a.y + scale_b * b.y,
        z: scale_a * a.z + scale_b * b.z,
        w: scale_a * a.w + scale_b * b.w,
    }
}

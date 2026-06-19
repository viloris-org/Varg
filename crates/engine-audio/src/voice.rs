//! Deterministic voice scoring, physical/virtual selection, and HRTF budgeting.

use crate::{SourceHandle, SpatialMode, VirtualizationPolicy, VoiceCategory};

/// Per-voice input data used for scoring and selection.
#[derive(Clone, Copy, Debug)]
pub struct VoiceScoreInput {
    /// Source handle (used as a deterministic tie-breaker).
    pub handle: SourceHandle,
    /// Voice category.
    pub category: VoiceCategory,
    /// Explicit per-source priority.
    pub priority: u8,
    /// Whether the source is marked gameplay-critical.
    pub critical: bool,
    /// Virtualization policy.
    pub virtualization: VirtualizationPolicy,
    /// Spatial rendering mode.
    pub spatial_mode: SpatialMode,
    /// Whether the source is allowed to use HRTF.
    pub use_hrtf: bool,
    /// Source volume in `[0.0, 1.0]`.
    pub volume: f32,
    /// Estimated final gain after distance attenuation and directivity.
    pub estimated_gain: f32,
}

impl VoiceScoreInput {
    /// Returns `true` if this voice should be protected from virtualization.
    fn is_protected(&self) -> bool {
        self.critical
            || matches!(self.category, VoiceCategory::Critical)
            || matches!(self.virtualization, VirtualizationPolicy::Protected)
    }

    /// Returns the deterministic score tuple used for ranking.
    ///
    /// Tuples are compared lexicographically: protected voices always win over
    /// unprotected voices, then category rank, priority, estimated gain, and
    /// finally the source handle for total determinism.
    fn score_key(&self) -> (bool, u8, u8, u32, u64) {
        let gain_step = (self.estimated_gain.clamp(0.0, 1.0) * 65_535.0) as u32;
        (
            self.is_protected(),
            self.category.rank(),
            self.priority,
            gain_step,
            self.handle.0,
        )
    }
}

/// Result of voice selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoiceSelection {
    /// Handles selected for physical rendering.
    pub physical: Vec<SourceHandle>,
    /// Subset of `physical` handles selected for HRTF rendering.
    pub hrtf: Vec<SourceHandle>,
}

/// Scores a collection of candidate voices and returns the deterministic selection.
///
/// `max_physical` caps the number of voices that will be rendered.
/// `max_hrtf` caps how many of those physical voices may use HRTF; it is only
/// applied to object-mode voices that have `use_hrtf` set.
pub fn select_voices(
    inputs: &[VoiceScoreInput],
    max_physical: usize,
    max_hrtf: usize,
) -> VoiceSelection {
    let mut ranked: Vec<VoiceScoreInput> = inputs.to_vec();
    ranked.sort_by(|a, b| {
        // Higher scores first; the tuple naturally orders protected > rank > priority > gain.
        b.score_key()
            .partial_cmp(&a.score_key())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let physical: Vec<SourceHandle> = ranked
        .iter()
        .take(max_physical)
        .map(|input| input.handle)
        .collect();
    let physical_set: std::collections::HashSet<SourceHandle> = physical.iter().copied().collect();

    let mut hrtf_candidates: Vec<VoiceScoreInput> = ranked
        .iter()
        .filter(|input| {
            physical_set.contains(&input.handle)
                && input.use_hrtf
                && input.spatial_mode == SpatialMode::Object
                && input.estimated_gain > f32::EPSILON
        })
        .copied()
        .collect();
    hrtf_candidates.sort_by(|a, b| {
        b.score_key()
            .partial_cmp(&a.score_key())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let hrtf: Vec<SourceHandle> = hrtf_candidates
        .into_iter()
        .take(max_hrtf)
        .map(|input| input.handle)
        .collect();

    VoiceSelection { physical, hrtf }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(handle: u64, category: VoiceCategory, priority: u8, gain: f32) -> VoiceScoreInput {
        VoiceScoreInput {
            handle: SourceHandle(handle),
            category,
            priority,
            critical: false,
            virtualization: VirtualizationPolicy::Virtualize,
            spatial_mode: SpatialMode::Object,
            use_hrtf: true,
            volume: 1.0,
            estimated_gain: gain,
        }
    }

    #[test]
    fn protected_voices_win_over_louder_disposable_voices() {
        let low_priority_loud = input(1, VoiceCategory::Disposable, 1, 1.0);
        let high_priority_quiet = input(2, VoiceCategory::Critical, 1, 0.1);
        let selection = select_voices(&[low_priority_loud, high_priority_quiet], 1, 1);
        assert_eq!(selection.physical, vec![SourceHandle(2)]);
    }

    #[test]
    fn hrtf_budget_demotes_to_stereo_without_dropping_voice() {
        let a = input(1, VoiceCategory::Sfx, 128, 1.0);
        let b = input(2, VoiceCategory::Sfx, 128, 1.0);
        let selection = select_voices(&[a, b], 2, 1);
        assert_eq!(selection.physical.len(), 2);
        assert_eq!(selection.hrtf.len(), 1);
    }

    #[test]
    fn direct_sources_are_never_hrtf() {
        let mut direct = input(1, VoiceCategory::Sfx, 128, 1.0);
        direct.spatial_mode = SpatialMode::Direct;
        let selection = select_voices(&[direct], 1, 1);
        assert_eq!(selection.physical, vec![SourceHandle(1)]);
        assert!(selection.hrtf.is_empty());
    }

    #[test]
    fn selection_is_deterministic_for_equal_scores() {
        let a = input(1, VoiceCategory::Sfx, 128, 1.0);
        let b = input(2, VoiceCategory::Sfx, 128, 1.0);
        let first = select_voices(&[a, b], 1, 1);
        let second = select_voices(&[b, a], 1, 1);
        assert_eq!(first.physical, second.physical);
    }
}

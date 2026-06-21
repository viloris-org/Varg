use serde::{Deserialize, Serialize};

use crate::BodyHandle;

/// A runtime contact filter evaluated before a collision pair is reported.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ContactFilter {
    /// Ignore all contacts involving this body.
    IgnoreBody {
        /// Body to ignore.
        body: BodyHandle,
    },
    /// Ignore contacts between this specific pair.
    IgnorePair {
        /// First body.
        body_a: BodyHandle,
        /// Second body.
        body_b: BodyHandle,
    },
    /// Always allow contacts between this pair, overriding earlier filters.
    ForcePair {
        /// First body.
        body_a: BodyHandle,
        /// Second body.
        body_b: BodyHandle,
    },
}

/// Ordered chain of contact filters evaluated each step.
#[derive(Clone, Debug, Default)]
pub struct ContactFilterChain {
    filters: Vec<ContactFilter>,
}

impl ContactFilterChain {
    /// Adds a filter to the end of the chain.
    pub fn push(&mut self, filter: ContactFilter) {
        self.filters.push(filter);
    }

    /// Removes all filters matching a predicate.
    pub fn retain(&mut self, predicate: impl Fn(&ContactFilter) -> bool) {
        self.filters.retain(predicate);
    }

    /// Clears all filters.
    pub fn clear(&mut self) {
        self.filters.clear();
    }

    /// Evaluates the filter chain for a pair of bodies.
    /// Returns `true` if the contact should be processed (reported).
    pub fn should_process(&self, body_a: BodyHandle, body_b: BodyHandle) -> bool {
        for filter in &self.filters {
            match filter {
                ContactFilter::ForcePair {
                    body_a: fa,
                    body_b: fb,
                } => {
                    if (*fa == body_a && *fb == body_b) || (*fa == body_b && *fb == body_a) {
                        return true;
                    }
                }
                ContactFilter::IgnoreBody { body } => {
                    if *body == body_a || *body == body_b {
                        return false;
                    }
                }
                ContactFilter::IgnorePair {
                    body_a: ia,
                    body_b: ib,
                } => {
                    if (*ia == body_a && *ib == body_b) || (*ia == body_b && *ib == body_a) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

// ── Physics profiler ──────────────────────────────────────────────────────────

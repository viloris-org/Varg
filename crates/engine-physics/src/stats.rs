/// Per-step physics profiling statistics.
#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsStats {
    /// Wall-clock step duration in microseconds.
    pub step_us: u64,
    /// Number of live rigid bodies.
    pub body_count: usize,
    /// Number of live colliders.
    pub collider_count: usize,
    /// Number of active contact pairs this step.
    pub contact_count: usize,
    /// Number of active simulation islands.
    pub island_count: usize,
    /// Number of sleeping bodies.
    pub sleeping_count: usize,
    /// Number of active joints.
    pub joint_count: usize,
    /// Number of active vehicles.
    pub vehicle_count: usize,
}

// ── World-level physics context ───────────────────────────────────────────────

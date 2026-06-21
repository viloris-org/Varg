/// Default physics layer.
pub const LAYER_DEFAULT: u32 = 0;
/// Player physics layer.
pub const LAYER_PLAYER: u32 = 1;
/// Enemy physics layer.
pub const LAYER_ENEMY: u32 = 2;
/// Trigger physics layer (sensors).
pub const LAYER_TRIGGER: u32 = 3;
/// Projectile physics layer.
pub const LAYER_PROJECTILE: u32 = 4;

// ── Layer matrix ─────────────────────────────────────────────────────────────

/// 32-layer collision matrix.
#[derive(Clone, Debug)]
pub struct LayerMatrix {
    rows: [u32; 32],
}

impl Default for LayerMatrix {
    fn default() -> Self {
        // All layers collide with all layers by default.
        Self { rows: [!0u32; 32] }
    }
}

impl LayerMatrix {
    /// Returns whether layer `a` collides with layer `b`.
    pub fn collides(&self, a: u32, b: u32) -> bool {
        let a = (a as usize).min(31);
        let b = (b as usize).min(31);
        self.rows[a] & (1 << b) != 0
    }

    /// Sets whether layer `a` collides with layer `b` (symmetric).
    pub fn set(&mut self, a: u32, b: u32, enabled: bool) {
        let a = (a as usize).min(31);
        let b = (b as usize).min(31);
        if enabled {
            self.rows[a] |= 1 << b;
            self.rows[b] |= 1 << a;
        } else {
            self.rows[a] &= !(1 << b);
            self.rows[b] &= !(1 << a);
        }
    }
}

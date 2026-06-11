//! Three.js-compatible Vector3 for Rhai scripts.
//!
//! Mirrors `THREE.Vector3` API so models trained on three.js/web
//! can use Aster without learning a new math API.
//!
//! All methods are plain Rust — Rhai registration happens in [`super::mod.rs`].

/// Three.js-compatible 3D vector.
///
/// Mutable like `THREE.Vector3` — methods mutate in place and return `()`.
/// Use `clone_vec()` for immutable copies.
#[derive(Clone, Debug, PartialEq)]
pub struct Vector3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vector3 {
    /// Create a new Vector3 (`new THREE.Vector3(x, y, z)`).
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Set all components (`v.set(x, y, z)`).
    pub fn set(&mut self, x: f32, y: f32, z: f32) {
        self.x = x;
        self.y = y;
        self.z = z;
    }

    /// Clone this vector (`v.clone()`).
    pub fn clone_vec(&self) -> Self {
        Self {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }

    // ── Arithmetic (mutating, returns () like three.js) ──

    /// Add another vector in place (`v.add(w)`).
    pub fn add(&mut self, other: &Vector3) {
        self.x += other.x;
        self.y += other.y;
        self.z += other.z;
    }

    /// Add scalar to each component (`v.addScalar(s)`).
    pub fn add_scalar(&mut self, s: f32) {
        self.x += s;
        self.y += s;
        self.z += s;
    }

    /// Subtract another vector in place (`v.sub(w)`).
    pub fn sub(&mut self, other: &Vector3) {
        self.x -= other.x;
        self.y -= other.y;
        self.z -= other.z;
    }

    /// Subtract scalar from each component (`v.subScalar(s)`).
    pub fn sub_scalar(&mut self, s: f32) {
        self.x -= s;
        self.y -= s;
        self.z -= s;
    }

    /// Multiply by another vector component-wise (`v.multiply(w)`).
    pub fn multiply(&mut self, other: &Vector3) {
        self.x *= other.x;
        self.y *= other.y;
        self.z *= other.z;
    }

    /// Multiply by scalar (`v.multiplyScalar(s)`).
    pub fn multiply_scalar(&mut self, s: f32) {
        self.x *= s;
        self.y *= s;
        self.z *= s;
    }

    /// Divide by another vector component-wise (`v.divide(w)`).
    pub fn divide(&mut self, other: &Vector3) {
        self.x /= other.x;
        self.y /= other.y;
        self.z /= other.z;
    }

    /// Divide by scalar (`v.divideScalar(s)`).
    pub fn divide_scalar(&mut self, s: f32) {
        self.x /= s;
        self.y /= s;
        self.z /= s;
    }

    /// Negate all components (`v.negate()`).
    pub fn negate(&mut self) {
        self.x = -self.x;
        self.y = -self.y;
        self.z = -self.z;
    }

    // ── Math ──

    /// Vector length / magnitude (`v.length()`).
    pub fn length_vec(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// Squared length (`v.lengthSq()`).
    pub fn length_sq(&self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// Normalize in place (`v.normalize()`).
    pub fn normalize_vec(&mut self) {
        let len = self.length_vec();
        if len > f32::EPSILON {
            self.x /= len;
            self.y /= len;
            self.z /= len;
        }
    }

    /// Dot product (`v.dot(w)`).
    pub fn dot(&self, other: &Vector3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product, returns new Vector3 (`v.cross(w)`).
    pub fn cross(&self, other: &Vector3) -> Vector3 {
        Vector3 {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// Distance to another vector (`v.distanceTo(w)`).
    pub fn distance_to(&self, other: &Vector3) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Squared distance to another vector (`v.distanceToSquared(w)`).
    pub fn distance_to_squared(&self, other: &Vector3) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx * dx + dy * dy + dz * dz
    }

    /// Linear interpolation (`v.lerp(target, alpha)`).
    pub fn lerp(&mut self, target: &Vector3, alpha: f32) {
        self.x += (target.x - self.x) * alpha;
        self.y += (target.y - self.y) * alpha;
        self.z += (target.z - self.z) * alpha;
    }

    /// Clamp components to [min, max] range (`v.clamp(min, max)`).
    pub fn clamp_vec(&mut self, min: &Vector3, max: &Vector3) {
        self.x = self.x.max(min.x).min(max.x);
        self.y = self.y.max(min.y).min(max.y);
        self.z = self.z.max(min.z).min(max.z);
    }

    // ── Convenience ──

    /// Access component by index: 0=x, 1=y, 2=z.
    /// Rhai calls this via `v.get(0)`.
    pub fn get_component(&self, index: i64) -> f32 {
        match index {
            0 => self.x,
            1 => self.y,
            2 => self.z,
            _ => 0.0,
        }
    }

    /// Convert to array `[x, y, z]`.
    pub fn to_array(&self) -> rhai::Array {
        vec![
            rhai::Dynamic::from(self.x as f64),
            rhai::Dynamic::from(self.y as f64),
            rhai::Dynamic::from(self.z as f64),
        ]
    }

    /// Convert to Aster engine Vec3.
    pub fn to_engine_vec3(&self) -> engine_core::math::Vec3 {
        engine_core::math::Vec3::new(self.x, self.y, self.z)
    }
}

// ── Static constructors (registered as module functions) ──

/// `Vector3::ZERO` equivalent.
pub fn vector3_zero() -> Vector3 {
    Vector3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    }
}

/// `Vector3::ONE` equivalent.
pub fn vector3_one() -> Vector3 {
    Vector3 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    }
}

/// `Vector3::UP` equivalent.
pub fn vector3_up() -> Vector3 {
    Vector3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    }
}

/// `Vector3::FORWARD` equivalent (-Z in three.js convention).
pub fn vector3_forward() -> Vector3 {
    Vector3 {
        x: 0.0,
        y: 0.0,
        z: -1.0,
    }
}

/// Create Vector3 from array `[x, y, z]`.
pub fn vector3_from_array(arr: rhai::Array) -> Vector3 {
    let x = arr.first().and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
    let y = arr.get(1).and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
    let z = arr.get(2).and_then(|v| v.as_float().ok()).unwrap_or(0.0) as f32;
    Vector3 { x, y, z }
}

impl From<Vector3> for engine_core::math::Vec3 {
    fn from(v: Vector3) -> Self {
        Self::new(v.x, v.y, v.z)
    }
}

impl From<engine_core::math::Vec3> for Vector3 {
    fn from(v: engine_core::math::Vec3) -> Self {
        Self {
            x: v.x,
            y: v.y,
            z: v.z,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector3_new_and_set() {
        let mut v = Vector3::new(0.0, 0.0, 0.0);
        v.set(1.0, 2.0, 3.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.z, 3.0);
    }

    #[test]
    fn vector3_add() {
        let mut v = Vector3::new(1.0, 2.0, 3.0);
        let w = Vector3::new(4.0, 5.0, 6.0);
        v.add(&w);
        assert_eq!(v, Vector3::new(5.0, 7.0, 9.0));
    }

    #[test]
    fn vector3_length() {
        let v = Vector3::new(3.0, 4.0, 0.0);
        assert!((v.length_vec() - 5.0).abs() < 0.001);
    }

    #[test]
    fn vector3_normalize() {
        let mut v = Vector3::new(3.0, 0.0, 0.0);
        v.normalize_vec();
        assert!((v.x - 1.0).abs() < 0.001);
    }

    #[test]
    fn vector3_dot() {
        let a = Vector3::new(1.0, 0.0, 0.0);
        let b = Vector3::new(0.0, 1.0, 0.0);
        assert_eq!(a.dot(&b), 0.0);
        assert_eq!(a.dot(&a), 1.0);
    }

    #[test]
    fn vector3_cross() {
        let a = Vector3::new(1.0, 0.0, 0.0);
        let b = Vector3::new(0.0, 1.0, 0.0);
        let c = a.cross(&b);
        assert_eq!(c, Vector3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn vector3_distance_to() {
        let a = Vector3::new(0.0, 0.0, 0.0);
        let b = Vector3::new(3.0, 4.0, 0.0);
        assert!((a.distance_to(&b) - 5.0).abs() < 0.001);
    }

    #[test]
    fn vector3_lerp() {
        let mut v = Vector3::new(0.0, 0.0, 0.0);
        let target = Vector3::new(10.0, 0.0, 0.0);
        v.lerp(&target, 0.5);
        assert_eq!(v, Vector3::new(5.0, 0.0, 0.0));
    }

    #[test]
    fn vector3_to_from_engine() {
        let v = Vector3::new(1.0, 2.0, 3.0);
        let ev: engine_core::math::Vec3 = v.clone().into();
        assert_eq!(ev.x, 1.0);
        assert_eq!(ev.y, 2.0);
        assert_eq!(ev.z, 3.0);

        let v2 = Vector3::from(ev);
        assert_eq!(v, v2);
    }
}

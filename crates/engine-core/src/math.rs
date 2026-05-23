//! Public math types.

/// 3D vector with single-precision components.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vec3 {
    /// Zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    /// One vector.
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);

    /// Creates a vector.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Dot product.
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Squared vector length.
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    /// Vector length.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns a normalized copy, or zero when the vector is too small.
    pub fn normalized(self) -> Self {
        let length = self.length();
        if length <= 1e-8 {
            Self::ZERO
        } else {
            self / length
        }
    }

    /// Component-wise minimum.
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    /// Component-wise maximum.
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }

    /// Cross product.
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Linear interpolation between two vectors.
    pub fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl std::ops::AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl std::ops::SubAssign for Vec3 {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl std::ops::Mul for Vec3 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self::new(self.x * rhs.x, self.y * rhs.y, self.z * rhs.z)
    }
}

impl std::ops::Div<f32> for Vec3 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}

impl std::ops::Neg for Vec3 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y, -self.z)
    }
}

/// Quaternion in `(x, y, z, w)` order.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Quat {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
    /// W component.
    pub w: f32,
}

impl Quat {
    /// Identity rotation.
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Quaternion multiplication (rotation composition).
    pub fn mul(self, rhs: Self) -> Self {
        Self {
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
        }
    }

    /// Rotates a vector by this quaternion.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let q = Vec3::new(self.x, self.y, self.z);
        let t = q.cross(v) * 2.0;
        v + t * self.w + q.cross(t)
    }

    /// Inverse of this quaternion.
    pub fn inverse(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }

    /// Decomposes rotation into Euler angles (yaw, pitch, roll) in degrees.
    /// Convention: YXZ intrinsic (yaw around Y, pitch around X, roll around Z).
    pub fn to_euler_deg(self) -> (f32, f32, f32) {
        let (yaw, pitch, roll) = self.to_euler();
        (yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees())
    }

    /// Decomposes rotation into Euler angles (yaw, pitch, roll) in radians.
    pub fn to_euler(self) -> (f32, f32, f32) {
        // YXZ intrinsic: yaw (Y), pitch (X), roll (Z)
        let sinr_cosp = 2.0 * (self.w * self.x + self.y * self.z);
        let cosr_cosp = 1.0 - 2.0 * (self.x * self.x + self.y * self.y);
        let roll = sinr_cosp.atan2(cosr_cosp);

        let sinp = 2.0 * (self.w * self.y - self.z * self.x);
        let pitch = if sinp.abs() >= 1.0 {
            std::f32::consts::FRAC_PI_2.copysign(sinp)
        } else {
            sinp.asin()
        };

        let siny_cosp = 2.0 * (self.w * self.z + self.x * self.y);
        let cosy_cosp = 1.0 - 2.0 * (self.y * self.y + self.z * self.z);
        let yaw = siny_cosp.atan2(cosy_cosp);

        (yaw, pitch, roll)
    }

    /// Creates rotation from Euler angles (yaw, pitch, roll) in degrees.
    pub fn from_euler_deg(yaw_deg: f32, pitch_deg: f32, roll_deg: f32) -> Self {
        Self::from_euler(
            yaw_deg.to_radians(),
            pitch_deg.to_radians(),
            roll_deg.to_radians(),
        )
    }

    /// Creates rotation from Euler angles in radians (YXZ intrinsic).
    pub fn from_euler(yaw: f32, pitch: f32, roll: f32) -> Self {
        let (sr, cr) = (roll * 0.5).sin_cos();
        let (sp, cp) = (pitch * 0.5).sin_cos();
        let (sy, cy) = (yaw * 0.5).sin_cos();

        Self {
            w: cr * cp * cy + sr * sp * sy,
            x: sr * cp * cy - cr * sp * sy,
            y: cr * sp * cy + sr * cp * sy,
            z: cr * cp * sy - sr * sp * cy,
        }
    }
    /// Creates a rotation from an axis and angle (in radians).
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let half_angle = angle * 0.5;
        let (sin_half, cos_half) = half_angle.sin_cos();
        let n = axis.normalized();
        Self {
            x: n.x * sin_half,
            y: n.y * sin_half,
            z: n.z * sin_half,
            w: cos_half,
        }
    }

    /// Creates a rotation that aligns the positive X-axis with the given direction.
    pub fn from_direction(dir: Vec3) -> Self {
        let dir = dir.normalized();
        let dot = dir.dot(Vec3::new(1.0, 0.0, 0.0));
        if dot > 0.9999 {
            return Self::IDENTITY;
        }
        if dot < -0.9999 {
            return Self::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), std::f32::consts::PI);
        }
        let axis = Vec3::new(1.0, 0.0, 0.0).cross(dir).normalized();
        let angle = dot.clamp(-1.0, 1.0).acos();
        Self::from_axis_angle(axis, angle)
    }
}

impl std::ops::Mul for Quat {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        self.mul(rhs)
    }
}

impl Default for Quat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Translation, rotation, and scale transform.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Transform {
    /// Position in parent space.
    pub translation: Vec3,
    /// Orientation in parent space.
    pub rotation: Quat,
    /// Scale in parent space.
    pub scale: Vec3,
}

impl Transform {
    /// Identity transform.
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    /// Transforms a point from local space to parent space.
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        self.translation + self.rotation.rotate(point * self.scale)
    }

    /// Composes two transforms: `self` (parent world) * `child` (child local).
    pub fn compose(&self, child: &Self) -> Self {
        Self {
            translation: self.transform_point(child.translation),
            rotation: self.rotation.mul(child.rotation),
            scale: Vec3::new(
                self.scale.x * child.scale.x,
                self.scale.y * child.scale.y,
                self.scale.z * child.scale.z,
            ),
        }
    }

    /// Inverse of this transform (world-to-local).
    pub fn inverse(&self) -> Self {
        let inv_rotation = self.rotation.inverse();
        let inv_scale = Vec3::new(
            1.0 / self.scale.x.max(f32::EPSILON),
            1.0 / self.scale.y.max(f32::EPSILON),
            1.0 / self.scale.z.max(f32::EPSILON),
        );
        let inv_translation = inv_rotation.rotate(-self.translation) * inv_scale;
        Self {
            translation: inv_translation,
            rotation: inv_rotation,
            scale: inv_scale,
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

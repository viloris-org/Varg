pub(crate) fn rotate_vec3(
    rotation: engine_core::math::Quat,
    vector: engine_core::math::Vec3,
) -> engine_core::math::Vec3 {
    let q = engine_core::math::Vec3::new(rotation.x, rotation.y, rotation.z);
    let t = cross(q, vector) * 2.0;
    vector + t * rotation.w + cross(q, t)
}

pub(crate) const IDENTITY_MAT4: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

pub(crate) fn look_at_rh(
    eye: engine_core::math::Vec3,
    target: engine_core::math::Vec3,
    up: engine_core::math::Vec3,
) -> [[f32; 4]; 4] {
    let f = (target - eye).normalized();
    let r = cross(f, up).normalized();
    let u = cross(r, f);
    [
        [r.x, u.x, -f.x, 0.0],
        [r.y, u.y, -f.y, 0.0],
        [r.z, u.z, -f.z, 0.0],
        [-r.dot(eye), -u.dot(eye), f.dot(eye), 1.0],
    ]
}

pub(crate) fn cross(
    a: engine_core::math::Vec3,
    b: engine_core::math::Vec3,
) -> engine_core::math::Vec3 {
    engine_core::math::Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

pub(crate) fn perspective_rh(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov_y * 0.5).tan();
    let range_inv = 1.0 / (near - far);
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far * range_inv, -1.0],
        [0.0, 0.0, far * near * range_inv, 0.0],
    ]
}

pub(crate) fn orthographic_rh(
    vertical_size: f32,
    aspect: f32,
    near: f32,
    far: f32,
) -> [[f32; 4]; 4] {
    let top = vertical_size * 0.5;
    let bottom = -top;
    let right = top * aspect;
    let left = -right;
    let range_inv = 1.0 / (near - far);
    [
        [2.0 / (right - left), 0.0, 0.0, 0.0],
        [0.0, 2.0 / (top - bottom), 0.0, 0.0],
        [0.0, 0.0, range_inv, 0.0],
        [
            -(right + left) / (right - left),
            -(top + bottom) / (top - bottom),
            near * range_inv,
            1.0,
        ],
    ]
}

pub(crate) fn mul_mat4(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            result[col][row] = a[0][row] * b[col][0]
                + a[1][row] * b[col][1]
                + a[2][row] * b[col][2]
                + a[3][row] * b[col][3];
        }
    }
    result
}

pub(crate) fn mul_mat4_vec3(
    m: &[[f32; 4]; 4],
    v: engine_core::math::Vec3,
) -> engine_core::math::Vec3 {
    let x = m[0][0] * v.x + m[1][0] * v.y + m[2][0] * v.z + m[3][0];
    let y = m[0][1] * v.x + m[1][1] * v.y + m[2][1] * v.z + m[3][1];
    let z = m[0][2] * v.x + m[1][2] * v.y + m[2][2] * v.z + m[3][2];
    engine_core::math::Vec3::new(x, y, z)
}

pub(crate) fn inverse_mat4(m: &[[f32; 4]; 4]) -> Option<[[f32; 4]; 4]> {
    let mut a = [[0.0f32; 8]; 4];
    for row in 0..4 {
        for col in 0..4 {
            a[row][col] = m[col][row];
        }
        a[row][4 + row] = 1.0;
    }

    for col in 0..4 {
        let mut pivot = col;
        let mut pivot_abs = a[col][col].abs();
        for row in (col + 1)..4 {
            let candidate = a[row][col].abs();
            if candidate > pivot_abs {
                pivot = row;
                pivot_abs = candidate;
            }
        }
        if pivot_abs <= 1e-8 {
            return None;
        }
        if pivot != col {
            a.swap(col, pivot);
        }

        let inv_pivot = 1.0 / a[col][col];
        for value in &mut a[col] {
            *value *= inv_pivot;
        }

        for row in 0..4 {
            if row == col {
                continue;
            }
            let factor = a[row][col];
            if factor == 0.0 {
                continue;
            }
            for idx in 0..8 {
                a[row][idx] -= factor * a[col][idx];
            }
        }
    }

    let mut inverse = [[0.0f32; 4]; 4];
    for row in 0..4 {
        for col in 0..4 {
            inverse[col][row] = a[row][4 + col];
        }
    }
    Some(inverse)
}

pub(crate) fn orthographic_rh_custom(
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    near: f32,
    far: f32,
) -> [[f32; 4]; 4] {
    let range_inv = 1.0 / (far - near);
    [
        [2.0 / (right - left), 0.0, 0.0, 0.0],
        [0.0, 2.0 / (top - bottom), 0.0, 0.0],
        [0.0, 0.0, range_inv, 0.0],
        [
            -(right + left) / (right - left),
            -(top + bottom) / (top - bottom),
            -near * range_inv,
            1.0,
        ],
    ]
}

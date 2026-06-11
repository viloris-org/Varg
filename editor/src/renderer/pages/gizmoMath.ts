// ─── 3D Math Utilities for Gizmo Rendering ─────────────────────────────────
// Mirrors the Rust camera math in engine-render-wgpu and
// Shared viewport projection and picking helpers for the Tauri editor.

// ─── Types ──────────────────────────────────────────────────────────────────

export type Vec3 = [number, number, number];
export type Mat4 = Float64Array; // 16 elements, column-major

// ─── Vec3 Helpers ──────────────────────────────────────────────────────────

export function vec3(x: number, y: number, z: number): Vec3 {
  return [x, y, z];
}

export function vec3Add(a: Vec3, b: Vec3): Vec3 {
  return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
}

export function vec3Sub(a: Vec3, b: Vec3): Vec3 {
  return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
}

export function vec3Scale(v: Vec3, s: number): Vec3 {
  return [v[0] * s, v[1] * s, v[2] * s];
}

export function vec3Length(v: Vec3): number {
  return Math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
}

export function vec3Normalize(v: Vec3): Vec3 {
  const len = vec3Length(v);
  if (len < 1e-10) return [0, 0, 0];
  return [v[0] / len, v[1] / len, v[2] / len];
}

export function vec3Cross(a: Vec3, b: Vec3): Vec3 {
  return [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ];
}

export function vec3Dot(a: Vec3, b: Vec3): number {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

export function vec3Lerp(a: Vec3, b: Vec3, t: number): Vec3 {
  return [
    a[0] + (b[0] - a[0]) * t,
    a[1] + (b[1] - a[1]) * t,
    a[2] + (b[2] - a[2]) * t,
  ];
}

// ─── Matrix Utilities ──────────────────────────────────────────────────────

export function mat4Identity(): Mat4 {
  const m = new Float64Array(16);
  m[0] = 1; m[5] = 1; m[10] = 1; m[15] = 1;
  return m;
}

export function mat4Multiply(a: Mat4, b: Mat4): Mat4 {
  const out = new Float64Array(16);
  for (let col = 0; col < 4; col++) {
    for (let row = 0; row < 4; row++) {
      let sum = 0;
      for (let k = 0; k < 4; k++) {
        sum += a[k * 4 + row] * b[col * 4 + k];
      }
      out[col * 4 + row] = sum;
    }
  }
  return out;
}

export function mat4Transpose(m: Mat4): Mat4 {
  const out = new Float64Array(16);
  for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 4; j++) {
      out[j * 4 + i] = m[i * 4 + j];
    }
  }
  return out;
}

// Inverts a 4x4 matrix using Gauss-Jordan elimination
export function mat4Inverse(m: Mat4): Mat4 | null {
  const a = new Float64Array(m);
  const inv = mat4Identity();

  for (let i = 0; i < 4; i++) {
    // Find pivot
    let pivot = i;
    for (let j = i + 1; j < 4; j++) {
      if (Math.abs(a[j * 4 + i]) > Math.abs(a[pivot * 4 + i])) {
        pivot = j;
      }
    }
    if (Math.abs(a[pivot * 4 + i]) < 1e-10) return null;

    // Swap rows
    if (pivot !== i) {
      for (let k = 0; k < 4; k++) {
        [a[i * 4 + k], a[pivot * 4 + k]] = [a[pivot * 4 + k], a[i * 4 + k]];
        [inv[i * 4 + k], inv[pivot * 4 + k]] = [inv[pivot * 4 + k], inv[i * 4 + k]];
      }
    }

    const pivotVal = a[i * 4 + i];
    for (let k = 0; k < 4; k++) {
      a[i * 4 + k] /= pivotVal;
      inv[i * 4 + k] /= pivotVal;
    }

    for (let j = 0; j < 4; j++) {
      if (j === i) continue;
      const factor = a[j * 4 + i];
      for (let k = 0; k < 4; k++) {
        a[j * 4 + k] -= factor * a[i * 4 + k];
        inv[j * 4 + k] -= factor * inv[i * 4 + k];
      }
    }
  }
  return inv;
}

// Transform a Vec3 by a Mat4 (returns Vec3 with perspective divide)
export function mat4TransformPoint(m: Mat4, v: Vec3): Vec3 {
  const x = v[0], y = v[1], z = v[2];
  const w = m[3] * x + m[7] * y + m[11] * z + m[15];
  if (Math.abs(w) < 1e-10) return [0, 0, 0];
  return [
    (m[0] * x + m[4] * y + m[8] * z + m[12]) / w,
    (m[1] * x + m[5] * y + m[9] * z + m[13]) / w,
    (m[2] * x + m[6] * y + m[10] * z + m[14]) / w,
  ];
}

// Transform a direction (Vec3) by a Mat4 (no translation, no perspective divide)
export function mat4TransformDirection(m: Mat4, v: Vec3): Vec3 {
  return [
    m[0] * v[0] + m[4] * v[1] + m[8] * v[2],
    m[1] * v[0] + m[5] * v[1] + m[9] * v[2],
    m[2] * v[0] + m[6] * v[1] + m[10] * v[2],
  ];
}

// ─── Projection ────────────────────────────────────────────────────────────

export function createPerspectiveMatrix(
  fovRadians: number,
  aspect: number,
  near: number,
  far: number,
): Mat4 {
  const f = 1.0 / Math.tan(fovRadians / 2);
  const nf = 1 / (near - far);
  const m = new Float64Array(16);
  m[0] = f / aspect;
  m[5] = f;
  m[10] = (far + near) * nf;
  m[11] = -1;
  m[14] = 2 * far * near * nf;
  return m;
}

export function createOrthographicMatrix(
  verticalSize: number,
  aspect: number,
  near: number,
  far: number,
): Mat4 {
  const halfHeight = Math.max(verticalSize, 0.001) * 0.5;
  const halfWidth = halfHeight * aspect;
  const rangeInv = 1 / (near - far);
  const m = new Float64Array(16);
  m[0] = 1 / halfWidth;
  m[5] = 1 / halfHeight;
  m[10] = 2 * rangeInv;
  m[14] = (far + near) * rangeInv;
  m[15] = 1;
  return m;
}

// ─── View Matrix ───────────────────────────────────────────────────────────

/**
 * Creates a view matrix from yaw/pitch/distance orbit camera params.
 * Mirrors the Rust camera:
 *   eye = target + distance * (pitch_cos * yaw_sin, pitch_sin, pitch_cos * yaw_cos)
 *   where yaw_cos = cos(yaw), yaw_sin = sin(yaw)
 */
export function createViewMatrix(
  yaw: number,
  pitch: number,
  distance: number,
  targetX: number,
  targetY: number,
  targetZ: number,
): Mat4 {
  const cosYaw = Math.cos(yaw);
  const sinYaw = Math.sin(yaw);
  const cosPitch = Math.cos(pitch);
  const sinPitch = Math.sin(pitch);

  // Forward direction (from eye toward target)
  const forwardX = -cosPitch * sinYaw;
  const forwardY = -sinPitch;
  const forwardZ = -cosPitch * cosYaw;
  const forward = vec3Normalize([forwardX, forwardY, forwardZ]);

  // Eye position
  const eyeX = targetX - forward[0] * distance;
  const eyeY = targetY - forward[1] * distance;
  const eyeZ = targetZ - forward[2] * distance;

  // Right vector
  const worldUp: Vec3 = [0, 1, 0];
  const right = vec3Normalize(vec3Cross(forward, worldUp));
  // Recompute up
  const up = vec3Cross(right, forward);

  // View matrix (lookAt)
  return mat4LookAt(
    [eyeX, eyeY, eyeZ],
    [targetX, targetY, targetZ],
    up,
  );
}

export function mat4LookAt(eye: Vec3, center: Vec3, up: Vec3): Mat4 {
  const f = vec3Normalize(vec3Sub(center, eye));
  const s = vec3Normalize(vec3Cross(f, up));
  const u = vec3Cross(s, f);

  const m = new Float64Array(16);
  m[0] = s[0];  m[4] = s[1];  m[8]  = s[2];  m[12] = -vec3Dot(s, eye);
  m[1] = u[0];  m[5] = u[1];  m[9]  = u[2];  m[13] = -vec3Dot(u, eye);
  m[2] = -f[0]; m[6] = -f[1]; m[10] = -f[2]; m[14] = vec3Dot(f, eye);
  m[3] = 0;     m[7] = 0;     m[11] = 0;     m[15] = 1;
  return m;
}

// ─── Screen Projection ─────────────────────────────────────────────────────

/**
 * Project a world-space point to screen coordinates (pixels).
 * Returns {x, y, depth} where depth is the NDC z value (0 = near, 1 = far).
 * Returns null if the point is behind the camera.
 */
export function projectToScreen(
  worldPos: Vec3,
  viewMatrix: Mat4,
  projMatrix: Mat4,
  vpWidth: number,
  vpHeight: number,
): { x: number; y: number; depth: number } | null {
  const vp = mat4Multiply(projMatrix, viewMatrix);
  const clip = mat4TransformPoint(vp, worldPos);

  const ndcX = clip[0];
  const ndcY = clip[1];
  const ndcZ = clip[2];

  // Check if behind camera (ndcZ < -1 or ndcZ > 1 means off screen in z)
  if (ndcZ < -1 || ndcZ > 1) return null;

  return {
    x: (ndcX * 0.5 + 0.5) * vpWidth,
    y: (1 - (ndcY * 0.5 + 0.5)) * vpHeight, // Flip Y
    depth: ndcZ,
  };
}

// ─── Ray Casting ───────────────────────────────────────────────────────────

/**
 * Compute a world-space ray from a screen-space point.
 */
export function rayFromScreen(
  screenX: number,
  screenY: number,
  viewMatrix: Mat4,
  projMatrix: Mat4,
  vpWidth: number,
  vpHeight: number,
): { origin: Vec3; direction: Vec3 } {
  // Convert to NDC
  const ndcX = (screenX / vpWidth) * 2 - 1;
  const ndcY = 1 - (screenY / vpHeight) * 2; // Flip Y

  const vp = mat4Multiply(projMatrix, viewMatrix);
  const invVp = mat4Inverse(vp) ?? mat4Identity();

  // Near and far points in NDC
  const nearPoint = mat4TransformPoint(invVp, [ndcX, ndcY, -1]);
  const farPoint = mat4TransformPoint(invVp, [ndcX, ndcY, 1]);

  const direction = vec3Normalize(vec3Sub(farPoint, nearPoint));
  return { origin: nearPoint, direction };
}

/**
 * Find the closest point on a line (defined by origin and direction of a ray)
 * to a given axis line.
 */
export function closestPointOnRayToAxis(
  rayOrigin: Vec3,
  rayDir: Vec3,
  axisOrigin: Vec3,
  axisDir: Vec3,
): number {
  const d = vec3Cross(rayDir, axisDir);
  const dLen = vec3Length(d);
  if (dLen < 1e-10) return 0; // Parallel

  const w = vec3Sub(rayOrigin, axisOrigin);
  const t = vec3Dot(vec3Cross(w, axisDir), d) / (dLen * dLen);
  return t;
}

/**
 * Compute world-space delta from screen-space drag.
 * Used for free movement in the view plane.
 */
export function screenDeltaToWorldDelta(
  dx: number,
  dy: number,
  cameraRight: Vec3,
  cameraUp: Vec3,
  distance: number,
): Vec3 {
  const scale = distance * 0.002;
  return vec3Add(
    vec3Scale(cameraRight, dx * scale),
    vec3Scale(cameraUp, -dy * scale),
  );
}

// ─── Gizmo Camera Helpers ──────────────────────────────────────────────────

/**
 * Compute gizmo world-scale (how large in world units the gizmo should be
 * to appear the same pixel size on screen regardless of camera distance).
 */
export function gizmoWorldScale(distance: number, targetScreenSize: number = 80): number {
  // At distance=1, 1 world unit ~ viewport_height/2 pixels
  // Scale linearly with distance
  return distance * targetScreenSize / 500;
}

/**
 * Compute the camera right and up vectors from yaw/pitch.
 */
export function cameraBasisVectors(yaw: number, pitch: number): { right: Vec3; up: Vec3; forward: Vec3 } {
  const cosYaw = Math.cos(yaw);
  const sinYaw = Math.sin(yaw);
  const cosPitch = Math.cos(pitch);
  const sinPitch = Math.sin(pitch);

  const forward: Vec3 = vec3Normalize([
    -cosPitch * sinYaw,
    -sinPitch,
    -cosPitch * cosYaw,
  ]);

  const worldUp: Vec3 = [0, 1, 0];
  const right = vec3Normalize(vec3Cross(forward, worldUp));
  const up = vec3Cross(right, forward);

  return { right, up, forward };
}

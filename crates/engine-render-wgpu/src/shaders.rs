pub(crate) const FORWARD_SHADER: &str = r#"
// Group 0: scene-level uniforms
struct CameraUniform {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    camera_forward: vec4<f32>,
};

struct TemporalUniform {
    previous_view_projection: mat4x4<f32>,
    current_view_projection: mat4x4<f32>,
    jitter_reset: vec4<f32>,
};

struct ForwardLight {
    position_type: vec4<f32>,
    direction_range: vec4<f32>,
    color_intensity: vec4<f32>,
    spot_angles: vec4<f32>,
};

struct LightingUniform {
    ambient: vec4<f32>,
    params: vec4<u32>,
    lights: array<ForwardLight, 32>,
};

struct CsmUniform {
    cascade_vps: array<mat4x4<f32>, 5>,
    cascade_splits: vec4<f32>,
    params: vec4<f32>,
};

struct FogUniform {
    density: f32,
    color: vec3<f32>,
    enabled: f32,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(0) @binding(1) var<uniform> lighting: LightingUniform;
@group(0) @binding(2) var<uniform> csm: CsmUniform;
@group(0) @binding(3) var csm_shadow_0: texture_depth_2d;
@group(0) @binding(4) var csm_shadow_1: texture_depth_2d;
@group(0) @binding(5) var csm_shadow_2: texture_depth_2d;
@group(0) @binding(6) var csm_shadow_3: texture_depth_2d;
@group(0) @binding(7) var csm_shadow_4: texture_depth_2d;
@group(0) @binding(8) var csm_sampler: sampler_comparison;
@group(0) @binding(9) var ibl_irradiance: texture_cube<f32>;
@group(0) @binding(10) var ibl_prefiltered: texture_cube<f32>;
@group(0) @binding(11) var ibl_brdf_lut: texture_2d<f32>;
@group(0) @binding(12) var ibl_sampler: sampler;
@group(0) @binding(13) var<uniform> fog: FogUniform;
@group(0) @binding(14) var<uniform> temporal: TemporalUniform;

// Group 1: material textures
@group(1) @binding(0) var base_color_tex: texture_2d<f32>;
@group(1) @binding(1) var normal_tex: texture_2d<f32>;
@group(1) @binding(2) var metallic_roughness_tex: texture_2d<f32>;
@group(1) @binding(3) var emissive_tex: texture_2d<f32>;
@group(1) @binding(4) var occlusion_tex: texture_2d<f32>;
@group(1) @binding(5) var mat_sampler: sampler;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
    @location(4) offset: vec3<f32>,
    @location(5) scale: vec3<f32>,
    @location(6) color: vec4<f32>,
    @location(7) rotation: vec4<f32>,
    @location(8) metallic: f32,
    @location(9) roughness: f32,
    @location(10) emissive: vec3<f32>,
    @location(11) receive_shadows: f32,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) world_position: vec3<f32>,
    @location(4) metallic: f32,
    @location(5) roughness: f32,
    @location(6) emissive: vec3<f32>,
    @location(7) world_tangent: vec3<f32>,
    @location(8) world_bitangent: vec3<f32>,
    @location(9) receive_shadows: f32,
    @location(10) previous_clip_position: vec4<f32>,
    @location(11) current_clip_position: vec4<f32>,
};

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) normal_roughness: vec4<f32>,
    @location(2) albedo_metallic: vec4<f32>,
    @location(3) motion: vec4<f32>,
};

const PI: f32 = 3.14159265359;
const EPSILON: f32 = 0.001;

fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let ndoth = max(dot(n, h), 0.0);
    let ndoth2 = ndoth * ndoth;
    let denom = ndoth2 * (a2 - 1.0) + 1.0;
    return a2 / max(PI * denom * denom, EPSILON);
}

fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = r * r / 8.0;
    let ndotv = max(dot(n, v), 0.0);
    let ndotl = max(dot(n, l), 0.0);
    let g1v = ndotv / (ndotv * (1.0 - k) + k);
    let g1l = ndotl / (ndotl * (1.0 - k) + k);
    return g1v * g1l;
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

fn sample_ibl_irradiance(n: vec3<f32>) -> vec3<f32> {
    return textureSample(ibl_irradiance, ibl_sampler, n).rgb;
}

fn sample_ibl_specular(n: vec3<f32>, v: vec3<f32>, roughness: f32) -> vec3<f32> {
    let r = reflect(-v, n);
    let mip = roughness * 4.0;
    return textureSampleLevel(ibl_prefiltered, ibl_sampler, r, mip).rgb;
}

fn sample_ibl_brdf(ndotv: f32, roughness: f32) -> vec2<f32> {
    return textureSample(ibl_brdf_lut, ibl_sampler, vec2<f32>(ndotv, roughness)).rg;
}

fn compute_ibl(n: vec3<f32>, v: vec3<f32>, base_color: vec3<f32>, metallic: f32, roughness: f32, f0: vec3<f32>) -> vec3<f32> {
    let ndotv = max(dot(n, v), 0.0);
    let f = fresnel_schlick(ndotv, f0);
    let kd = (1.0 - f) * (1.0 - metallic);
    let irradiance = sample_ibl_irradiance(n);
    let diffuse = kd * irradiance * base_color;
    let brdf = sample_ibl_brdf(ndotv, roughness);
    let specular_ibl = sample_ibl_specular(n, v, roughness);
    let specular = specular_ibl * (f * brdf.x + brdf.y);
    return diffuse + specular;
}

fn apply_fog(color: vec3<f32>, world_pos: vec3<f32>, camera_pos: vec3<f32>, dist: f32) -> vec3<f32> {
    if (fog.enabled < 0.5) {
        return color;
    }
    let fog_factor = 1.0 - exp(-fog.density * dist * dist);
    return mix(color, fog.color, clamp(fog_factor, 0.0, 1.0));
}

fn sample_cascade_shadow(cascade_idx: u32, uv: vec2<f32>, depth: f32, ndotl: f32) -> f32 {
    let bias = csm.params.z + csm.params.w * (1.0 - ndotl);
    let texel = csm.params.y;
    var blocker_count = 0.0;
    let search_radius = texel * 4.0;
    for (var bx = -2; bx <= 2; bx++) {
        for (var by = -2; by <= 2; by++) {
            let offset = vec2<f32>(f32(bx), f32(by)) * search_radius;
            var visible = 1.0;
            if (cascade_idx == 0u) {
                visible = textureSampleCompare(csm_shadow_0, csm_sampler, uv + offset, depth - bias);
            } else if (cascade_idx == 1u) {
                visible = textureSampleCompare(csm_shadow_1, csm_sampler, uv + offset, depth - bias);
            } else if (cascade_idx == 2u) {
                visible = textureSampleCompare(csm_shadow_2, csm_sampler, uv + offset, depth - bias);
            } else if (cascade_idx == 3u) {
                visible = textureSampleCompare(csm_shadow_3, csm_sampler, uv + offset, depth - bias);
            } else {
                visible = textureSampleCompare(csm_shadow_4, csm_sampler, uv + offset, depth - bias);
            }
            if (visible < 0.5) {
                blocker_count += 1.0;
            }
        }
    }
    let blocker_ratio = blocker_count / 25.0;
    let penumbra = mix(1.0, 6.0, blocker_ratio);
    var shadow_factor = 0.0;
    for (var dx = -1; dx <= 1; dx++) {
        for (var dy = -1; dy <= 1; dy++) {
            let offset = vec2<f32>(f32(dx), f32(dy)) * texel * penumbra;
            if (cascade_idx == 0u) {
                shadow_factor += textureSampleCompare(csm_shadow_0, csm_sampler, uv + offset, depth - bias);
            } else if (cascade_idx == 1u) {
                shadow_factor += textureSampleCompare(csm_shadow_1, csm_sampler, uv + offset, depth - bias);
            } else if (cascade_idx == 2u) {
                shadow_factor += textureSampleCompare(csm_shadow_2, csm_sampler, uv + offset, depth - bias);
            } else if (cascade_idx == 3u) {
                shadow_factor += textureSampleCompare(csm_shadow_3, csm_sampler, uv + offset, depth - bias);
            } else {
                shadow_factor += textureSampleCompare(csm_shadow_4, csm_sampler, uv + offset, depth - bias);
            }
        }
    }
    return shadow_factor / 9.0;
}

fn sample_csm_shadow(world_pos: vec3<f32>, view_depth: f32, n: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    var cascade_idx = 4u;
    var fade = 1.0;
    let fade_range = max(csm.params.x, EPSILON);
    if (view_depth < csm.cascade_splits.x) {
        cascade_idx = 0u;
        if (view_depth > csm.cascade_splits.x - fade_range) {
            fade = (csm.cascade_splits.x - view_depth) / fade_range;
        }
    } else if (view_depth < csm.cascade_splits.y) {
        cascade_idx = 1u;
        if (view_depth > csm.cascade_splits.y - fade_range) {
            fade = (csm.cascade_splits.y - view_depth) / fade_range;
        }
    } else if (view_depth < csm.cascade_splits.z) {
        cascade_idx = 2u;
        if (view_depth > csm.cascade_splits.z - fade_range) {
            fade = (csm.cascade_splits.z - view_depth) / fade_range;
        }
    } else if (view_depth < csm.cascade_splits.w) {
        cascade_idx = 3u;
        if (view_depth > csm.cascade_splits.w - fade_range) {
            fade = (csm.cascade_splits.w - view_depth) / fade_range;
        }
    }

    var shadow_coord: vec4<f32>;
    if (cascade_idx == 0u) { shadow_coord = csm.cascade_vps[0] * vec4<f32>(world_pos, 1.0); }
    else if (cascade_idx == 1u) { shadow_coord = csm.cascade_vps[1] * vec4<f32>(world_pos, 1.0); }
    else if (cascade_idx == 2u) { shadow_coord = csm.cascade_vps[2] * vec4<f32>(world_pos, 1.0); }
    else if (cascade_idx == 3u) { shadow_coord = csm.cascade_vps[3] * vec4<f32>(world_pos, 1.0); }
    else { shadow_coord = csm.cascade_vps[4] * vec4<f32>(world_pos, 1.0); }

    let ndc = shadow_coord.xyz / shadow_coord.w;
    let uv = ndc.xy * 0.5 + 0.5;
    let depth = ndc.z;

    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || depth < 0.0 || depth > 1.0) {
        return 1.0;
    }

    let ndotl = max(dot(n, light_dir), 0.0);
    let base_shadow = sample_cascade_shadow(cascade_idx, uv, depth, ndotl);
    if (fade < 1.0) {
        if (cascade_idx == 0u) {
            shadow_coord = csm.cascade_vps[1] * vec4<f32>(world_pos, 1.0);
        } else if (cascade_idx == 1u) {
            shadow_coord = csm.cascade_vps[2] * vec4<f32>(world_pos, 1.0);
        } else if (cascade_idx == 2u) {
            shadow_coord = csm.cascade_vps[3] * vec4<f32>(world_pos, 1.0);
        } else if (cascade_idx == 3u) {
            shadow_coord = csm.cascade_vps[4] * vec4<f32>(world_pos, 1.0);
        } else { return base_shadow; }
        let ndc2 = shadow_coord.xyz / shadow_coord.w;
        let uv2 = ndc2.xy * 0.5 + 0.5;
        let depth2 = ndc2.z;
        let next_shadow = sample_cascade_shadow(cascade_idx + 1u, uv2, depth2, ndotl);
        return mix(base_shadow, next_shadow, 1.0 - fade);
    }
    return base_shadow;
}

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    let scaled_position = input.position * input.scale;
    let rotated_position = scaled_position
        + 2.0 * cross(input.rotation.xyz, cross(input.rotation.xyz, scaled_position)
        + input.rotation.w * scaled_position);
    let world_pos = rotated_position + input.offset;
    out.position = camera.view_projection * vec4<f32>(world_pos, 1.0);

    let scaled_normal = input.normal / max(abs(input.scale), vec3<f32>(0.0001));
    let rotated_normal = scaled_normal
        + 2.0 * cross(input.rotation.xyz, cross(input.rotation.xyz, scaled_normal)
        + input.rotation.w * scaled_normal);
    let n = normalize(rotated_normal);

    let scaled_tangent = input.tangent.xyz / max(abs(input.scale), vec3<f32>(0.0001));
    let rotated_tangent = scaled_tangent
        + 2.0 * cross(input.rotation.xyz, cross(input.rotation.xyz, scaled_tangent)
        + input.rotation.w * scaled_tangent);
    let tt = normalize(rotated_tangent - n * dot(n, rotated_tangent));
    let b = normalize(cross(n, tt) * input.tangent.w);

    out.world_normal = n;
    out.world_tangent = tt;
    out.world_bitangent = b;
    out.receive_shadows = input.receive_shadows;
    out.uv = input.uv;
    out.color = input.color;
    out.world_position = world_pos;
    out.metallic = input.metallic;
    out.roughness = input.roughness;
    out.emissive = input.emissive;
    out.previous_clip_position = temporal.previous_view_projection * vec4<f32>(world_pos, 1.0);
    out.current_clip_position = temporal.current_view_projection * vec4<f32>(world_pos, 1.0);
    return out;
}

@fragment
fn fs_main(input: VsOut) -> FsOut {
    // Sample material textures
    let tex_color = textureSample(base_color_tex, mat_sampler, input.uv);
    let normal_sample = textureSample(normal_tex, mat_sampler, input.uv).rgb;
    let mra = textureSample(metallic_roughness_tex, mat_sampler, input.uv);
    let emissive_tex_color = textureSample(emissive_tex, mat_sampler, input.uv).rgb;
    let ao = textureSample(occlusion_tex, mat_sampler, input.uv).r;

    // Base color: vertex tint * texture
    let base_color = input.color.rgb * tex_color.rgb;

    // PBR parameters: per-instance fallback * texture modulation
    let metallic = clamp(input.metallic * mra.b, 0.0, 1.0);
    let roughness = clamp(input.roughness * mra.g, 0.04, 1.0);

    // Normal mapping: reconstruct TBN and transform sampled normal
    let tbn_t = normalize(input.world_tangent);
    let tbn_b = normalize(input.world_bitangent);
    let tbn_n = normalize(input.world_normal);
    let tbn = mat3x3<f32>(tbn_t, tbn_b, tbn_n);
    let tangent_normal = normalize(normal_sample * 2.0 - 1.0);
    let n = normalize(tbn * tangent_normal);

    let v = normalize(camera.camera_position.xyz - input.world_position);

    let f0 = mix(vec3<f32>(0.04), base_color, metallic);

    // Ambient from IBL
    var color = compute_ibl(n, v, base_color, metallic, roughness, f0) * ao;

    // CSM shadow
    let view_depth = max(
        dot(input.world_position - camera.camera_position.xyz, camera.camera_forward.xyz),
        0.0,
    );
    var shadow_factor = 1.0;
    if (input.receive_shadows > 0.5) {
        let shadow_light = lighting.lights[0];
        let shadow_light_dir = normalize(-shadow_light.direction_range.xyz);
        shadow_factor = sample_csm_shadow(input.world_position, view_depth, n, shadow_light_dir);
    }

    for (var i: u32 = 0u; i < lighting.params.x; i = i + 1u) {
        let light = lighting.lights[i];
        let light_type = light.position_type.w;
        let light_color = light.color_intensity.rgb;
        let intensity = light.color_intensity.w;
        var light_dir = vec3<f32>(0.0, 1.0, 0.0);
        var attenuation = 1.0;
        var spot = 1.0;

        if (light_type < 0.5) {
            light_dir = normalize(-light.direction_range.xyz);
        } else {
            let to_light = light.position_type.xyz - input.world_position;
            let distance = length(to_light);
            light_dir = to_light / max(distance, EPSILON);
            let range = max(light.direction_range.w, EPSILON);
            let falloff = max(1.0 - distance / range, 0.0);
            attenuation = falloff * falloff;

            if (light_type > 1.5) {
                let spot_alignment = dot(normalize(-light_dir), normalize(light.direction_range.xyz));
                spot = smoothstep(light.spot_angles.y, light.spot_angles.x, spot_alignment);
            }
        }

        let ndotl = max(dot(n, light_dir), 0.0);
        if (ndotl <= 0.0) {
            continue;
        }

        let h = normalize(v + light_dir);
        let ndotv = max(dot(n, v), 0.0);
        let vdoth = max(dot(v, h), 0.0);

        let d = distribution_ggx(n, h, roughness);
        let g = geometry_smith(n, v, light_dir, roughness);
        let f = fresnel_schlick(vdoth, f0);

        let specular = (d * g * f) / max(4.0 * ndotv * ndotl, EPSILON);
        let kd = (1.0 - f) * (1.0 - metallic);
        let diffuse = kd * base_color / PI;

        var radiance = (diffuse + specular) * light_color * intensity * ndotl;

        if (light_type < 0.5 && light.spot_angles.z > 0.5) {
            radiance = radiance * shadow_factor;
        }

        color = color + radiance * attenuation * spot;
    }

    // Emissive: per-instance factor * emissive texture
    color = color + input.emissive * emissive_tex_color;

    // Fog
    let dist = length(input.world_position - camera.camera_position.xyz);
    color = apply_fog(color, input.world_position, camera.camera_position.xyz, dist);

    let alpha = input.color.a * tex_color.a;
    var out: FsOut;
    out.color = vec4<f32>(color, alpha);
    out.normal_roughness = vec4<f32>(n * 0.5 + 0.5, roughness);
    out.albedo_metallic = vec4<f32>(base_color, metallic);
    let current_ndc = input.current_clip_position.xy / max(input.current_clip_position.w, EPSILON);
    let previous_ndc = input.previous_clip_position.xy / max(input.previous_clip_position.w, EPSILON);
    out.motion = vec4<f32>((current_ndc - previous_ndc) * 0.5, 0.0, 1.0);
    return out;
}
"#;

pub(crate) const GUI_SHADER: &str = r#"
struct GuiUniform {
    screen_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> gui: GuiUniform;
@group(0) @binding(1) var gui_texture: texture_2d<f32>;
@group(0) @binding(2) var gui_sampler: sampler;

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: u32,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

fn unpack_color(value: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(value & 255u),
        f32((value >> 8u) & 255u),
        f32((value >> 16u) & 255u),
        f32((value >> 24u) & 255u)
    ) / 255.0;
}

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    let ndc = vec2<f32>(
        input.position.x / max(gui.screen_size.x, 1.0) * 2.0 - 1.0,
        1.0 - input.position.y / max(gui.screen_size.y, 1.0) * 2.0
    );
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = input.uv;
    out.color = unpack_color(input.color);
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    return textureSample(gui_texture, gui_sampler, input.uv) * input.color;
}
"#;

pub(crate) const SKINNED_SHADER: &str = r#"
struct CameraUniform {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    camera_forward: vec4<f32>,
};

struct BonePalette {
    matrices: array<mat4x4<f32>>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;
@group(1) @binding(0) var<storage, read> bones: BonePalette;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    let skin = bones.matrices[input.joints.x] * input.weights.x
        + bones.matrices[input.joints.y] * input.weights.y
        + bones.matrices[input.joints.z] * input.weights.z
        + bones.matrices[input.joints.w] * input.weights.w;
    var out: VsOut;
    out.position = camera.view_projection * skin * vec4<f32>(input.position, 1.0);
    out.normal = normalize((skin * vec4<f32>(input.normal, 0.0)).xyz);
    out.uv = input.uv;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let light = max(dot(input.normal, normalize(vec3<f32>(0.4, 0.8, 0.2))), 0.15);
    return vec4<f32>(vec3<f32>(0.82, 0.86, 0.92) * light, 1.0);
}
"#;

pub(crate) const GRID_SHADER: &str = r#"
struct CameraUniform {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    camera_forward: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniform;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) alpha_factor: f32,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    out.position = camera.view_projection * vec4<f32>(input.position, 1.0);
    out.world_pos = input.position;
    out.alpha_factor = input.uv.x;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let half_extent = 50.0;
    let fade_start = half_extent * 0.7;
    let dist = length(input.world_pos.xz);
    let fade = 1.0 - smoothstep(fade_start, half_extent, dist);
    let alpha = input.alpha_factor * fade;
    return vec4<f32>(vec3<f32>(0.6), alpha);
}
"#;

pub(crate) const SHADOW_SHADER: &str = r#"
struct ShadowUniform {
    light_view_projection: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> shadow: ShadowUniform;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
    @location(4) offset: vec3<f32>,
    @location(5) scale: vec3<f32>,
    @location(6) color: vec4<f32>,
    @location(7) rotation: vec4<f32>,
    @location(8) metallic: f32,
    @location(9) roughness: f32,
    @location(10) emissive: vec3<f32>,
    @location(11) receive_shadows: f32,
};

@vertex
fn vs_main(input: VsIn) -> @builtin(position) vec4<f32> {
    let scaled_position = input.position * input.scale;
    let rotated_position = scaled_position
        + 2.0 * cross(input.rotation.xyz, cross(input.rotation.xyz, scaled_position)
        + input.rotation.w * scaled_position);
    let world_pos = rotated_position + input.offset;
    return shadow.light_view_projection * vec4<f32>(world_pos, 1.0);
}
"#;

pub(crate) const SKYBOX_SHADER: &str = r#"
struct SkyboxUniform {
    view_rotation_only: mat4x4<f32>,
    zenith_color: vec4<f32>,
    horizon_color: vec4<f32>,
    rotation_intensity: vec4<f32>,
    use_cubemap: vec4<u32>,
};

@group(0) @binding(0) var<uniform> skybox: SkyboxUniform;
@group(0) @binding(1) var cubemap_texture: texture_cube<f32>;
@group(0) @binding(2) var cubemap_sampler: sampler;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) direction: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    var out: VsOut;
    let uv = vec2<f32>(
        f32((vertex_index << 1u) & 2u),
        f32(vertex_index & 2u),
    );
    let position = uv * 2.0 - vec2<f32>(1.0);
    out.position = vec4<f32>(position, 0.0, 1.0);
    let inv_proj = vec4<f32>(position, 1.0, 1.0);
    let view_dir = (skybox.view_rotation_only * inv_proj).xyz;
    out.direction = view_dir;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let dir = normalize(input.direction);
    let rotation_rad = skybox.rotation_intensity.x * 3.14159265 / 180.0;
    let cos_r = cos(rotation_rad);
    let sin_r = sin(rotation_rad);
    let rotated_dir = vec3<f32>(
        dir.x * cos_r - dir.z * sin_r,
        dir.y,
        dir.x * sin_r + dir.z * cos_r
    );
    let intensity = skybox.rotation_intensity.y;
    var color: vec3<f32>;
    if (skybox.use_cubemap.x != 0u) {
        color = textureSample(cubemap_texture, cubemap_sampler, rotated_dir).rgb * intensity;
    } else {
        let t = clamp(dir.y * 0.5 + 0.5, 0.0, 1.0);
        color = mix(skybox.horizon_color.rgb, skybox.zenith_color.rgb, t) * intensity;
    }
    return vec4<f32>(color, 1.0);
}
"#;

pub(crate) const IBL_IRRADIANCE_SHADER: &str = r#"
struct IblBakeParams {
    face_idx: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
};

@group(0) @binding(0) var env_map: texture_cube<f32>;
@group(0) @binding(1) var env_sampler: sampler;
@group(0) @binding(2) var output_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(3) var<uniform> params: IblBakeParams;

fn cube_face_uv_to_dir(face: u32, uv: vec2<f32>) -> vec3<f32> {
    let uc = 2.0 * uv.x - 1.0;
    let vc = 2.0 * uv.y - 1.0;
    if (face == 0u) { return vec3<f32>( 1.0,  vc, -uc); }
    if (face == 1u) { return vec3<f32>(-1.0,  vc,  uc); }
    if (face == 2u) { return vec3<f32>(  uc, 1.0, -vc); }
    if (face == 3u) { return vec3<f32>(  uc,-1.0,  vc); }
    if (face == 4u) { return vec3<f32>(  uc,  vc,  1.0); }
    return vec3<f32>( -uc,  vc, -1.0);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = 32u;
    if (gid.x >= res || gid.y >= res) { return; }
    let uv = (vec2<f32>(gid.xy) + 0.5) / f32(res);
    let dir = normalize(cube_face_uv_to_dir(params.face_idx, uv));
    var color = vec3<f32>(0.0);
    var sample_count = 0.0;
    let delta = 0.05;
    for (var dphi: f32 = 0.0; dphi < 2.0 * 3.14159265; dphi += delta) {
        for (var dtheta: f32 = 0.0; dtheta < 0.5 * 3.14159265; dtheta += delta) {
            let tangent = vec3<f32>(cos(dphi) * sin(dtheta), cos(dtheta), sin(dphi) * sin(dtheta));
            let sample_dir = tangent.x * dir + tangent.y * vec3<f32>(0.0, 1.0, 0.0) + tangent.z * vec3<f32>(1.0, 0.0, 0.0);
            let sample_dir_normalized = normalize(sample_dir);
            color += textureSampleLevel(env_map, env_sampler, sample_dir_normalized, 0.0).rgb * cos(dtheta) * sin(dtheta);
            sample_count += 1.0;
        }
    }
    let output = color * 3.14159265 / sample_count * 2.0;
    textureStore(output_tex, vec2<i32>(gid.xy), vec4<f32>(output, 1.0));
}
"#;

pub(crate) const IBL_PREFILTER_SHADER: &str = r#"
struct IblBakeParams {
    face_idx: u32,
    roughness: f32,
    resolution: u32,
    pad1: u32,
};

@group(0) @binding(0) var env_map: texture_cube<f32>;
@group(0) @binding(1) var env_sampler: sampler;
@group(0) @binding(2) var output_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(3) var<uniform> params: IblBakeParams;

fn cube_face_uv_to_dir(face: u32, uv: vec2<f32>) -> vec3<f32> {
    let uc = 2.0 * uv.x - 1.0;
    let vc = 2.0 * uv.y - 1.0;
    switch face {
        case 0u { return vec3<f32>( 1.0,  vc, -uc); }
        case 1u { return vec3<f32>(-1.0,  vc,  uc); }
        case 2u { return vec3<f32>(  uc, 1.0, -vc); }
        case 3u { return vec3<f32>(  uc,-1.0,  vc); }
        case 4u { return vec3<f32>(  uc,  vc,  1.0); }
        default { return vec3<f32>( -uc,  vc, -1.0); }
    }
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = params.resolution;
    if (gid.x >= res || gid.y >= res) { return; }
    let uv = (vec2<f32>(gid.xy) + 0.5) / f32(res);
    let n = normalize(cube_face_uv_to_dir(params.face_idx, uv));
    let r = n;
    let v = r;
    var color = vec3<f32>(0.0);
    var weight = 0.0;
    let sample_count = 256u;
    for (var i = 0u; i < sample_count; i++) {
        let xi = hammersley(i, sample_count);
        let h = importance_sample_ggx(xi, n, params.roughness);
        let l = normalize(2.0 * dot(v, h) * h - v);
        let ndotl = max(dot(n, l), 0.0);
        if (ndotl > 0.0) {
            color += textureSampleLevel(env_map, env_sampler, l, 0.0).rgb * ndotl;
            weight += ndotl;
        }
    }
    let output = color / max(weight, 0.001);
    textureStore(output_tex, vec2<i32>(gid.xy), vec4<f32>(output, 1.0));
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(n), radical_inverse_vdc(i));
}

fn radical_inverse_vdc(bits: u32) -> f32 {
    var b = bits;
    b = (b << 16u) | (b >> 16u);
    b = ((b & 0x55555555u) << 1u) | ((b & 0xAAAAAAAAu) >> 1u);
    b = ((b & 0x33333333u) << 2u) | ((b & 0xCCCCCCCCu) >> 2u);
    b = ((b & 0x0F0F0F0Fu) << 4u) | ((b & 0xF0F0F0F0u) >> 4u);
    b = ((b & 0x00FF00FFu) << 8u) | ((b & 0xFF00FF00u) >> 8u);
    return f32(b) * 2.3283064365386963e-10;
}

fn importance_sample_ggx(xi: vec2<f32>, n: vec3<f32>, roughness: f32) -> vec3<f32> {
    let a = roughness * roughness;
    let phi = 2.0 * 3.14159265 * xi.x;
    let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
    let sin_theta = sqrt(1.0 - cos_theta * cos_theta);
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.999) { up = vec3<f32>(1.0, 0.0, 0.0); }
    let tangent = normalize(cross(up, n));
    let bitangent = cross(n, tangent);
    var h = tangent * (sin_theta * cos(phi)) + bitangent * (sin_theta * sin(phi)) + n * cos_theta;
    return normalize(h);
}
"#;

pub(crate) const IBL_BRDF_LUT_SHADER: &str = r#"
@group(0) @binding(0) var output: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = 256u;
    if (gid.x >= res || gid.y >= res) { return; }
    let ndotv = (f32(gid.x) + 0.5) / f32(res);
    let roughness = (f32(gid.y) + 0.5) / f32(res);
    let v = vec3<f32>(sqrt(1.0 - ndotv * ndotv), 0.0, ndotv);
    var scale = 0.0;
    var bias = 0.0;
    let sample_count = 256u;
    for (var i = 0u; i < sample_count; i++) {
        let xi = hammersley(i, sample_count);
        let h = importance_sample_ggx(xi, vec3<f32>(0.0, 0.0, 1.0), roughness);
        let l = normalize(2.0 * dot(v, h) * h - v);
        let ndotl = max(l.z, 0.0);
        let ndoth = max(h.z, 0.0);
        let vdoth = max(dot(v, h), 0.0);
        if (ndotl > 0.0) {
            let g = geometry_smith(ndotv, ndotl, roughness);
            let g_vis = (g * vdoth) / max(ndoth * ndotv, 0.001);
            let fc = pow(1.0 - vdoth, 5.0);
            scale += (1.0 - fc) * g_vis;
            bias += fc * g_vis;
        }
    }
    scale /= f32(sample_count);
    bias /= f32(sample_count);
    textureStore(output, vec2<i32>(gid.xy), vec4<f32>(scale, bias, 0.0, 1.0));
}

fn radical_inverse_vdc(bits: u32) -> f32 {
    var b = bits;
    b = (b << 16u) | (b >> 16u);
    b = ((b & 0x55555555u) << 1u) | ((b & 0xAAAAAAAAu) >> 1u);
    b = ((b & 0x33333333u) << 2u) | ((b & 0xCCCCCCCCu) >> 2u);
    b = ((b & 0x0F0F0F0Fu) << 4u) | ((b & 0xF0F0F0F0u) >> 4u);
    b = ((b & 0x00FF00FFu) << 8u) | ((b & 0xFF00FF00u) >> 8u);
    return f32(b) * 2.3283064365386963e-10;
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(n), radical_inverse_vdc(i));
}

fn importance_sample_ggx(xi: vec2<f32>, n: vec3<f32>, roughness: f32) -> vec3<f32> {
    let a = roughness * roughness;
    let phi = 2.0 * 3.14159265 * xi.x;
    let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
    let sin_theta = sqrt(1.0 - cos_theta * cos_theta);
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.999) { up = vec3<f32>(1.0, 0.0, 0.0); }
    let tangent = normalize(cross(up, n));
    let bitangent = cross(n, tangent);
    var h = tangent * (sin_theta * cos(phi)) + bitangent * (sin_theta * sin(phi)) + n * cos_theta;
    return normalize(h);
}

fn geometry_smith(ndotv: f32, ndotl: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = r * r / 8.0;
    let g1v = ndotv / (ndotv * (1.0 - k) + k);
    let g1l = ndotl / (ndotl * (1.0 - k) + k);
    return g1v * g1l;
}
"#;

pub(crate) const SSAO_SHADER: &str = r#"
struct SsaoUniform {
    radius: f32,
    bias: f32,
    intensity: f32,
    _pad: f32,
    width: f32,
    height: f32,
    inv_width: f32,
    inv_height: f32,
};

@group(0) @binding(0) var depth_tex: texture_depth_2d;
@group(0) @binding(1) var noise_tex: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: SsaoUniform;
@group(0) @binding(3) var<storage, read> kernel: array<vec4<f32>, 32>;
@group(0) @binding(4) var output_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let coord = vec2<i32>(gid.xy);
    let tex_w = i32(params.width);
    let tex_h = i32(params.height);
    if (coord.x >= tex_w || coord.y >= tex_h) {
        return;
    }
    if (coord.x < 1 || coord.y < 1 || coord.x >= tex_w - 1 || coord.y >= tex_h - 1) {
        textureStore(output_tex, coord, vec4<f32>(1.0, 0.0, 0.0, 1.0));
        return;
    }
    var depth = textureLoad(depth_tex, coord, 0);
    if (depth >= 1.0) {
        textureStore(output_tex, coord, vec4<f32>(1.0, 0.0, 0.0, 1.0));
        return;
    }
    // Normal from depth gradient
    var d0 = textureLoad(depth_tex, vec2<i32>(coord.x - 1, coord.y), 0);
    var d1 = textureLoad(depth_tex, vec2<i32>(coord.x + 1, coord.y), 0);
    var d2 = textureLoad(depth_tex, vec2<i32>(coord.x, coord.y - 1), 0);
    var d3 = textureLoad(depth_tex, vec2<i32>(coord.x, coord.y + 1), 0);
    var dx = (d1 - d0) * 100.0;
    var dy = (d3 - d2) * 100.0;
    var len = sqrt(dx * dx + dy * dy + 1.0);
    var normal = vec3<f32>(dx / len, dy / len, 1.0 / len);
    // Noise from 4x4 texture
    var noise_coord = vec2<i32>(gid.xy % 4u);
    var noise = textureLoad(noise_tex, noise_coord, 0);
    var random_vec = noise.xyz * 2.0 - 1.0;
    len = sqrt(random_vec.x * random_vec.x + random_vec.y * random_vec.y + random_vec.z * random_vec.z);
    random_vec = vec3<f32>(random_vec.x / len, random_vec.y / len, random_vec.z / len);
    // SSAO
    var occlusion = 0.0;
    let radius_px = params.radius * params.width;
    for (var i = 0u; i < 32u; i = i + 1u) {
        var sample_dir = kernel[i].xyz;
        var d = sample_dir.x * normal.x + sample_dir.y * normal.y + sample_dir.z * normal.z;
        if (d < 0.0) {
            sample_dir.x = sample_dir.x - 2.0 * d * normal.x;
            sample_dir.y = sample_dir.y - 2.0 * d * normal.y;
            sample_dir.z = sample_dir.z - 2.0 * d * normal.z;
        }
        var l2 = sample_dir.x * sample_dir.x + sample_dir.y * sample_dir.y + sample_dir.z * sample_dir.z;
        l2 = 1.0 / sqrt(l2);
        sample_dir = vec3<f32>(sample_dir.x * l2, sample_dir.y * l2, sample_dir.z * l2);
        var off_x = coord.x + i32(sample_dir.x * radius_px);
        var off_y = coord.y + i32(sample_dir.y * radius_px);
        var sample_depth = 1.0;
        if (off_x >= 0 && off_y >= 0 && off_x < tex_w && off_y < tex_h) {
            sample_depth = textureLoad(depth_tex, vec2<i32>(off_x, off_y), 0);
        }
        var diff = depth - sample_depth;
        if (diff < 0.0) { diff = 0.0 - diff; }
        diff = params.radius / (diff + 0.0001);
        if (diff > 1.0) { diff = 1.0; }
        if (diff < 0.0) { diff = 0.0; }
        var rc = diff * diff * (3.0 - 2.0 * diff);
        if (sample_depth < depth - params.bias) {
            occlusion = occlusion + rc;
        }
    }
    occlusion = 1.0 - (occlusion / 32.0) * params.intensity;
    if (occlusion < 0.0) { occlusion = 0.0; }
    if (occlusion > 1.0) { occlusion = 1.0; }
    textureStore(output_tex, coord, vec4<f32>(occlusion, 0.0, 0.0, 1.0));
}
"#;

pub(crate) const BLOOM_DOWNSAMPLE_SHADER: &str = r#"
@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var dst_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var bilinear: sampler;
@group(0) @binding(3) var<uniform> params: vec4<f32>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dst_size = vec2<u32>(textureDimensions(dst_tex));
    if (gid.x >= dst_size.x || gid.y >= dst_size.y) { return; }
    let uv = (vec2<f32>(gid.xy) + 0.5) / vec2<f32>(dst_size);
    let threshold = params.x;
    let knee = params.y;
    var color = textureSampleLevel(src_tex, bilinear, uv, 0.0).rgb;
    let brightness = max(max(color.r, color.g), color.b);
    var contribution = 0.0;
    if (threshold > 0.0) {
        contribution = max(brightness - threshold, 0.0);
        contribution /= max(brightness, 0.001);
    } else {
        contribution = 1.0;
    }
    color *= contribution;
    textureStore(dst_tex, vec2<i32>(gid.xy), vec4<f32>(color, 1.0));
}
"#;

pub(crate) const BLOOM_UPSAMPLE_SHADER: &str = r#"
@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var dst_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var bilinear: sampler;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dst_size = vec2<u32>(textureDimensions(dst_tex));
    if (gid.x >= dst_size.x || gid.y >= dst_size.y) { return; }
    let uv = (vec2<f32>(gid.xy) + 0.5) / vec2<f32>(dst_size);
    let color = textureSampleLevel(src_tex, bilinear, uv, 0.0).rgb;
    textureStore(dst_tex, vec2<i32>(gid.xy), vec4<f32>(color, 1.0));
}
"#;

pub(crate) const SSGI_SHADER: &str = r#"
struct SsgiUniform {
    width: f32,
    height: f32,
    inv_width: f32,
    inv_height: f32,
    radius: f32,
    intensity: f32,
    thickness: f32,
    sample_count: f32,
    frame_index: f32,
    history_blend: f32,
    reset_history: f32,
    pad0: f32,
};

@group(0) @binding(0) var hdr_tex: texture_2d<f32>;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var normal_tex: texture_2d<f32>;
@group(0) @binding(3) var albedo_tex: texture_2d<f32>;
@group(0) @binding(4) var output_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(5) var<uniform> ssgi: SsgiUniform;
@group(0) @binding(6) var lin_sampler: sampler;
@group(0) @binding(7) var motion_tex: texture_2d<f32>;
@group(0) @binding(8) var history_tex: texture_2d<f32>;

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn decode_normal(encoded: vec3<f32>) -> vec3<f32> {
    return normalize(encoded * 2.0 - 1.0);
}

fn tangent_basis(n: vec3<f32>) -> mat3x3<f32> {
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.95) {
        up = vec3<f32>(1.0, 0.0, 0.0);
    }
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return mat3x3<f32>(t, b, n);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let size = vec2<u32>(u32(ssgi.width), u32(ssgi.height));
    if (gid.x >= size.x || gid.y >= size.y) {
        return;
    }

    let pixel = vec2<i32>(gid.xy);
    let uv = (vec2<f32>(gid.xy) + vec2<f32>(0.5)) * vec2<f32>(ssgi.inv_width, ssgi.inv_height);
    let center_depth = textureLoad(depth_tex, pixel, 0);
    if (center_depth >= 0.9999) {
        textureStore(output_tex, pixel, vec4<f32>(0.0));
        return;
    }

    let normal_sample = textureLoad(normal_tex, pixel, 0);
    let n = decode_normal(normal_sample.rgb);
    let roughness = clamp(normal_sample.a, 0.05, 1.0);
    let albedo = textureLoad(albedo_tex, pixel, 0).rgb;
    let basis = tangent_basis(n);
    var gi = vec3<f32>(0.0);
    var weight_sum = 0.0;
    let samples = max(u32(ssgi.sample_count), 1u);

    for (var i = 0u; i < samples; i = i + 1u) {
        let fi = f32(i);
        let r0 = hash12(vec2<f32>(f32(gid.x) + fi * 13.7, f32(gid.y) + ssgi.frame_index));
        let r1 = hash12(vec2<f32>(f32(gid.y) + fi * 5.1, f32(gid.x) + ssgi.frame_index * 0.37));
        let phi = 6.28318530718 * (fi + r0) / f32(samples);
        let cos_theta = sqrt(1.0 - r1);
        let sin_theta = sqrt(max(1.0 - cos_theta * cos_theta, 0.0));
        let hemi = vec3<f32>(cos(phi) * sin_theta, sin(phi) * sin_theta, cos_theta);
        let ray = normalize(basis * hemi);
        let screen_dir = normalize(ray.xy + vec2<f32>(0.0001));
        let radius_px = ssgi.radius * mix(18.0, 95.0, 1.0 - center_depth);
        var hit_color = vec3<f32>(0.0);
        var hit_weight = 0.0;

        for (var step = 1u; step <= 12u; step = step + 1u) {
            let t = f32(step) / 12.0;
            let sample_uv = uv + screen_dir * radius_px * t * vec2<f32>(ssgi.inv_width, ssgi.inv_height);
            if (sample_uv.x <= 0.0 || sample_uv.x >= 1.0 || sample_uv.y <= 0.0 || sample_uv.y >= 1.0) {
                break;
            }
            let sample_pixel = vec2<i32>(
                clamp(i32(sample_uv.x * ssgi.width), 0, i32(ssgi.width) - 1),
                clamp(i32(sample_uv.y * ssgi.height), 0, i32(ssgi.height) - 1),
            );
            let sample_depth = textureLoad(depth_tex, sample_pixel, 0);
            let expected_depth = center_depth - ray.z * 0.015 * t;
            let depth_delta = abs(sample_depth - expected_depth);
            if (sample_depth < 0.9999 && depth_delta < ssgi.thickness + t * 0.02) {
                let sample_normal = decode_normal(textureSampleLevel(normal_tex, lin_sampler, sample_uv, 0.0).rgb);
                let sample_albedo = textureSampleLevel(albedo_tex, lin_sampler, sample_uv, 0.0).rgb;
                let sample_radiance = textureSampleLevel(hdr_tex, lin_sampler, sample_uv, 0.0).rgb;
                let facing = max(dot(n, ray), 0.0) * max(dot(sample_normal, -ray), 0.0);
                let attenuation = (1.0 - t) * (1.0 - t);
                hit_color = sample_radiance * sample_albedo * facing * attenuation;
                hit_weight = facing * attenuation;
                break;
            }
        }

        gi += hit_color;
        weight_sum += hit_weight;
    }

    let diffuse_gi = gi / max(weight_sum, 0.001);
    let raw_color = diffuse_gi * albedo * ssgi.intensity * roughness * roughness;
    let motion = textureLoad(motion_tex, pixel, 0).xy;
    let history_uv = clamp(uv - motion, vec2<f32>(0.0), vec2<f32>(1.0));
    let history = textureSampleLevel(history_tex, lin_sampler, history_uv, 0.0).rgb;
    let history_weight = select(clamp(ssgi.history_blend, 0.0, 0.95), 0.0, ssgi.reset_history > 0.5);
    let color = mix(raw_color, history, history_weight);
    textureStore(output_tex, pixel, vec4<f32>(color, 1.0));
}
"#;

pub(crate) const POST_SHADER: &str = r#"
struct PostProcessUniform {
    render_width: f32,
    render_height: f32,
    inv_render_width: f32,
    inv_render_height: f32,
    output_width: f32,
    output_height: f32,
    inv_output_width: f32,
    inv_output_height: f32,
    exposure: f32,
    bloom_intensity: f32,
    ssao_enabled: f32,
    upscale_sharpness: f32,
    ssgi_enabled: f32,
    ssgi_intensity: f32,
    ssr_enabled: f32,
    ssr_intensity: f32,
};

@group(0) @binding(0) var hdr_tex: texture_2d<f32>;
@group(0) @binding(1) var bloom_tex: texture_2d<f32>;
@group(0) @binding(2) var ssao_tex: texture_2d<f32>;
@group(0) @binding(3) var<uniform> post: PostProcessUniform;
@group(0) @binding(4) var lin_sampler: sampler;
@group(0) @binding(5) var ssgi_tex: texture_2d<f32>;
@group(0) @binding(6) var depth_tex: texture_depth_2d;
@group(0) @binding(7) var normal_tex: texture_2d<f32>;
@group(0) @binding(8) var albedo_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    var out: VsOut;
    let uv = vec2<f32>(
        f32((vertex_index << 1u) & 2u),
        f32(vertex_index & 2u),
    );
    out.position = vec4<f32>(uv * 2.0 - vec2<f32>(1.0), 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

fn aces_tonemap(color: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return saturate((color * (a * color + b)) / (color * (c * color + d) + e));
}

fn cubic_weight(distance: f32) -> f32 {
    let x = abs(distance);
    if (x <= 1.0) {
        return 1.5 * x * x * x - 2.5 * x * x + 1.0;
    }
    if (x < 2.0) {
        return -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0;
    }
    return 0.0;
}

fn load_hdr(pixel: vec2<i32>, size: vec2<i32>) -> vec3<f32> {
    return textureLoad(hdr_tex, clamp(pixel, vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
}

fn reconstruct_hdr(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(i32(post.render_width), i32(post.render_height));
    let source = uv * vec2<f32>(post.render_width, post.render_height) - vec2<f32>(0.5);
    let base = vec2<i32>(floor(source));
    let fraction = fract(source);
    var color = vec3<f32>(0.0);
    var weight_sum = 0.0;
    for (var y = -1; y <= 2; y = y + 1) {
        let wy = cubic_weight(f32(y) - fraction.y);
        for (var x = -1; x <= 2; x = x + 1) {
            let weight = cubic_weight(f32(x) - fraction.x) * wy;
            color = color + load_hdr(base + vec2<i32>(x, y), size) * weight;
            weight_sum = weight_sum + weight;
        }
    }
    return color / max(weight_sum, 0.0001);
}

fn sharpen_hdr(center: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    if (post.upscale_sharpness <= 0.0) {
        return center;
    }
    let texel = vec2<f32>(post.inv_render_width, post.inv_render_height);
    let north = textureSampleLevel(hdr_tex, lin_sampler, uv - vec2<f32>(0.0, texel.y), 0.0).rgb;
    let south = textureSampleLevel(hdr_tex, lin_sampler, uv + vec2<f32>(0.0, texel.y), 0.0).rgb;
    let west = textureSampleLevel(hdr_tex, lin_sampler, uv - vec2<f32>(texel.x, 0.0), 0.0).rgb;
    let east = textureSampleLevel(hdr_tex, lin_sampler, uv + vec2<f32>(texel.x, 0.0), 0.0).rgb;
    let neighborhood_min = min(center, min(min(north, south), min(west, east)));
    let neighborhood_max = max(center, max(max(north, south), max(west, east)));
    let detail = center - (north + south + west + east) * 0.25;
    return clamp(center + detail * post.upscale_sharpness, neighborhood_min, neighborhood_max);
}

fn decode_post_normal(encoded: vec3<f32>) -> vec3<f32> {
    return normalize(encoded * 2.0 - 1.0);
}

fn screen_space_reflection(uv: vec2<f32>) -> vec3<f32> {
    if (post.ssr_enabled < 0.5) {
        return vec3<f32>(0.0);
    }
    let pixel = vec2<i32>(
        clamp(i32(uv.x * post.render_width), 0, i32(post.render_width) - 1),
        clamp(i32(uv.y * post.render_height), 0, i32(post.render_height) - 1)
    );
    let depth = textureLoad(depth_tex, pixel, 0);
    if (depth >= 0.9999) {
        return vec3<f32>(0.0);
    }
    let normal_roughness = textureLoad(normal_tex, pixel, 0);
    let n = decode_post_normal(normal_roughness.rgb);
    let roughness = clamp(normal_roughness.a, 0.04, 1.0);
    let metallic = textureLoad(albedo_tex, pixel, 0).a;
    let view_dir = normalize(vec3<f32>(uv * 2.0 - vec2<f32>(1.0), 1.0));
    let r = reflect(view_dir, n);
    let screen_dir = normalize(r.xy + vec2<f32>(0.0001));
    let max_distance = mix(48.0, 8.0, roughness);
    for (var step = 1u; step <= 24u; step = step + 1u) {
        let t = f32(step) / 24.0;
        let sample_uv = uv + screen_dir * max_distance * t * vec2<f32>(post.inv_render_width, post.inv_render_height);
        if (sample_uv.x <= 0.0 || sample_uv.x >= 1.0 || sample_uv.y <= 0.0 || sample_uv.y >= 1.0) {
            break;
        }
        let sample_pixel = vec2<i32>(
            clamp(i32(sample_uv.x * post.render_width), 0, i32(post.render_width) - 1),
            clamp(i32(sample_uv.y * post.render_height), 0, i32(post.render_height) - 1)
        );
        let sample_depth = textureLoad(depth_tex, sample_pixel, 0);
        let expected_depth = depth - r.z * 0.012 * t;
        if (sample_depth < 0.9999 && abs(sample_depth - expected_depth) < 0.025 + t * 0.035) {
            let edge_fade = smoothstep(0.0, 0.12, sample_uv.x) * smoothstep(1.0, 0.88, sample_uv.x)
                * smoothstep(0.0, 0.12, sample_uv.y) * smoothstep(1.0, 0.88, sample_uv.y);
            let material_weight = mix(0.18, 1.0, metallic) * (1.0 - roughness);
            return textureSampleLevel(hdr_tex, lin_sampler, sample_uv, 0.0).rgb
                * edge_fade * material_weight * post.ssr_intensity;
        }
    }
    return vec3<f32>(0.0);
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let scaling = post.render_width != post.output_width || post.render_height != post.output_height;
    var hdr = textureSampleLevel(hdr_tex, lin_sampler, input.uv, 0.0).rgb;
    if (scaling) {
        hdr = sharpen_hdr(reconstruct_hdr(input.uv), input.uv);
    }
    let bloom = textureSampleLevel(bloom_tex, lin_sampler, input.uv, 0.0).rgb * post.bloom_intensity;
    var color = hdr + bloom;
    if (post.ssgi_enabled > 0.5) {
        color = color + textureSampleLevel(ssgi_tex, lin_sampler, input.uv, 0.0).rgb * post.ssgi_intensity;
    }
    color = color + screen_space_reflection(input.uv);
    color = color * post.exposure;
    if (post.ssao_enabled > 0.5) {
        let ao = textureSampleLevel(ssao_tex, lin_sampler, input.uv, 0.0).r;
        color = color * (0.5 + ao * 0.5);
    }
    color = aces_tonemap(color);
    return vec4<f32>(color, 1.0);
}
"#;

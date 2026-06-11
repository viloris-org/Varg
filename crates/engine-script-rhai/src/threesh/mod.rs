//! Three.js-compatible API for Rhai scripts.
//!
//! # Usage from Rhai
//!
//! ```rhai
//! // Geometry
//! let geo = THREE::box_geometry(1.0, 1.0, 1.0);
//! let mat = THREE::mesh_standard_material(#{
//!     color: [1.0, 0.2, 0.2],
//!     roughness: 0.5,
//!     metalness: 0.1,
//! });
//!
//! // Create mesh (auto-registers in engine scene)
//! let cube = THREE::create_mesh("Player", geo, mat);
//! cube.position.set(0.0, 2.0, 0.0);
//!
//! // Lights
//! let light = THREE::directional_light([1.0, 1.0, 1.0], 1.0);
//! light.position.set(5.0, 10.0, 5.0);
//!
//! // Camera
//! let cam = THREE::perspective_camera(75.0, 16.0/9.0, 0.1, 1000.0);
//! cam.position.set(0.0, 5.0, 10.0);
//! cam.look_at(0.0, 0.0, 0.0);
//!
//! // Lifecycle
//! fn on_update(dt) {
//!     cube.rotation().set(0.0, cube_angle, 0.0);
//!     cube_angle += dt * 0.5;
//! }
//! ```
//!
//! # Design
//!
//! - Custom types mirror THREE.js object model
//! - All types are `Clone` (Rhai requirement for custom types)
//! - Entity creation writes through shared [`SceneContext`]
//! - Transform mutations immediately update Aster ECS transforms

pub mod geometry;
pub mod material;
pub mod scene;
pub mod vector3;

use geometry::Geometry;
use material::Material;
use scene::{Camera, Light, Mesh, Object3D, SceneContext};
use vector3::Vector3;

/// Build a Rhai module containing all THREE namespace constructor functions.
///
/// Returns a module ready to be registered as `"THREE"` via
/// [`rhai::Engine::register_static_module`]. Functions that create scene
/// objects capture a clone of `ctx` so they write through to the engine.
pub fn build_three_module(ctx: &SceneContext) -> rhai::Module {
    let mut module = rhai::Module::new();

    // ── Geometry constructors ──
    module.set_native_fn("box_geometry", |w: f32, h: f32, d: f32| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Geometry::box_geometry(w, h, d)))
    });
    module.set_native_fn("sphere_geometry", |r: f32, ws: i64, hs: i64| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Geometry::sphere_geometry(r, ws, hs)))
    });
    module.set_native_fn("plane_geometry", |w: f32, h: f32| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Geometry::plane_geometry(w, h)))
    });
    module.set_native_fn("cylinder_geometry", |rt: f32, rb: f32, h: f32, s: i64| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Geometry::cylinder_geometry(
            rt, rb, h, s,
        )))
    });
    module.set_native_fn("capsule_geometry", |r: f32, h: f32, s: i64| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Geometry::capsule_geometry(r, h, s)))
    });
    module.set_native_fn("model_geometry", |path: rhai::ImmutableString| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Geometry::model_geometry(
            path.as_str(),
        )))
    });

    // ── Material constructors ──
    module.set_native_fn("mesh_basic_material", |props: rhai::Map| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Material::mesh_basic_material(props)))
    });
    module.set_native_fn("mesh_standard_material", |props: rhai::Map| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Material::mesh_standard_material(
            props,
        )))
    });
    module.set_native_fn("mesh_phong_material", |props: rhai::Map| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Material::mesh_phong_material(props)))
    });
    module.set_native_fn("basic_red", || {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Material::basic_red()))
    });
    module.set_native_fn("basic_green", || {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Material::basic_green()))
    });
    module.set_native_fn("basic_blue", || {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Material::basic_blue()))
    });

    // ── Vector3 constructors ──
    module.set_native_fn("vec3", |x: f32, y: f32, z: f32| {
        Ok::<_, Box<rhai::EvalAltResult>>(rhai::Dynamic::from(Vector3::new(x, y, z)))
    });
    module.set_native_fn("vector3_zero", || {
        Ok::<_, Box<rhai::EvalAltResult>>(vector3::vector3_zero())
    });
    module.set_native_fn("vector3_one", || {
        Ok::<_, Box<rhai::EvalAltResult>>(vector3::vector3_one())
    });
    module.set_native_fn("vector3_up", || {
        Ok::<_, Box<rhai::EvalAltResult>>(vector3::vector3_up())
    });
    module.set_native_fn("vector3_forward", || {
        Ok::<_, Box<rhai::EvalAltResult>>(vector3::vector3_forward())
    });
    module.set_native_fn("vector3_from_array", |arr: rhai::Array| {
        Ok::<_, Box<rhai::EvalAltResult>>(vector3::vector3_from_array(arr))
    });

    // ── Scene object creators (capture ctx) ──
    {
        let ctx = ctx.clone();
        module.set_native_fn(
            "create_mesh",
            move |name: rhai::ImmutableString, geo: rhai::Dynamic, mat: rhai::Dynamic| {
                let geo: Geometry = geo.try_cast::<Geometry>().unwrap_or(Geometry::Box {
                    width: 1.0,
                    height: 1.0,
                    depth: 1.0,
                });
                let mat: Material = mat.try_cast::<Material>().unwrap_or(Material::basic_red());
                Ok::<_, Box<rhai::EvalAltResult>>(Mesh::new(name.as_str(), geo, mat, &ctx))
            },
        );
    }
    {
        let ctx = ctx.clone();
        module.set_native_fn(
            "directional_light",
            move |color: rhai::Array, intensity: f32| {
                let c = array_to_color(color);
                Ok::<_, Box<rhai::EvalAltResult>>(Light::directional_light(c, intensity, &ctx))
            },
        );
    }
    {
        let ctx = ctx.clone();
        module.set_native_fn(
            "point_light",
            move |color: rhai::Array, intensity: f32, range: f32| {
                let c = array_to_color(color);
                Ok::<_, Box<rhai::EvalAltResult>>(Light::point_light(c, intensity, range, &ctx))
            },
        );
    }
    {
        let ctx = ctx.clone();
        module.set_native_fn(
            "spot_light",
            move |color: rhai::Array, intensity: f32, range: f32, angle: f32| {
                let c = array_to_color(color);
                Ok::<_, Box<rhai::EvalAltResult>>(Light::spot_light(
                    c, intensity, range, angle, &ctx,
                ))
            },
        );
    }
    {
        let ctx = ctx.clone();
        module.set_native_fn(
            "ambient_light",
            move |color: rhai::Array, intensity: f32| {
                let c = array_to_color(color);
                Ok::<_, Box<rhai::EvalAltResult>>(Light::ambient_light(c, intensity, &ctx))
            },
        );
    }
    {
        let ctx = ctx.clone();
        module.set_native_fn(
            "perspective_camera",
            move |fov: f32, aspect: f32, near: f32, far: f32| {
                Ok::<_, Box<rhai::EvalAltResult>>(Camera::perspective_camera(
                    fov, aspect, near, far, &ctx,
                ))
            },
        );
    }
    {
        let ctx = ctx.clone();
        module.set_native_fn(
            "orthographic_camera",
            move |left: f32, right: f32, top: f32, bottom: f32, near: f32, far: f32| {
                Ok::<_, Box<rhai::EvalAltResult>>(Camera::orthographic_camera(
                    left, right, top, bottom, near, far, &ctx,
                ))
            },
        );
    }
    {
        let ctx = ctx.clone();
        module.set_native_fn("create_object", move |name: rhai::ImmutableString| {
            Ok::<_, Box<rhai::EvalAltResult>>(Object3D::new(name.as_str(), &ctx))
        });
    }

    module
}

/// Register the complete THREE.js-compatible API on a Rhai engine.
///
/// Call this during [`RhaiScriptBackend::new()`] setup.
/// Types are registered globally (method dispatch) and all constructor
/// functions are available both as global functions and under the `THREE`
/// static-module namespace (`THREE::create_mesh(...)` etc.).
pub fn register_threesh_api(engine: &mut rhai::Engine, ctx: &SceneContext) {
    // ── Register custom types ──
    engine.register_type::<Vector3>();
    // Geometry and Material are NOT registered as Rhai types because Rhai 1.24
    // does not support enums as custom types at runtime. They are passed as
    // opaque Dynamic values between constructors and entity creators.
    engine.register_type::<Object3D>();
    engine.register_type::<Mesh>();
    engine.register_type::<Light>();
    engine.register_type::<Camera>();
    engine.register_type::<scene::PositionProxy>();
    engine.register_type::<scene::RotationProxy>();
    engine.register_type::<scene::ScaleProxy>();

    // ── Vector3 methods ──
    register_vector3_methods(engine);

    // ── Geometry methods ──
    register_geometry_methods(engine);

    // ── Material methods ──
    register_material_methods(engine);

    // ── Object3D methods ──
    register_object3d_methods(engine);

    // ── Mesh methods ──
    register_mesh_methods(engine);

    // ── Light methods ──
    register_light_methods(engine);

    // ── Camera methods ──
    register_camera_methods(engine);

    // ── Position/Rotation/Scale proxy methods ──
    register_proxy_methods(engine);

    // ── THREE module functions (constructors) ──
    register_module_functions(engine, ctx);
}

// ── Vector3 ──

fn register_vector3_methods(engine: &mut rhai::Engine) {
    engine.register_fn("set", Vector3::set);
    engine.register_fn("clone_vec", Vector3::clone_vec);
    engine.register_fn("add", Vector3::add);
    engine.register_fn("add_scalar", Vector3::add_scalar);
    engine.register_fn("sub", Vector3::sub);
    engine.register_fn("sub_scalar", Vector3::sub_scalar);
    engine.register_fn("multiply", Vector3::multiply);
    engine.register_fn("multiply_scalar", Vector3::multiply_scalar);
    engine.register_fn("divide", Vector3::divide);
    engine.register_fn("divide_scalar", Vector3::divide_scalar);
    engine.register_fn("negate", Vector3::negate);
    engine.register_fn("length", Vector3::length_vec);
    engine.register_fn("length_sq", Vector3::length_sq);
    engine.register_fn("normalize", Vector3::normalize_vec);
    engine.register_fn("dot", Vector3::dot);
    engine.register_fn("cross", Vector3::cross);
    engine.register_fn("distance_to", Vector3::distance_to);
    engine.register_fn("distance_to_squared", Vector3::distance_to_squared);
    engine.register_fn("lerp", Vector3::lerp);
    engine.register_fn("clamp", Vector3::clamp_vec);
    engine.register_fn("get_component", Vector3::get_component);
    engine.register_fn("to_array", Vector3::to_array);
}

// ── Geometry ──

fn register_geometry_methods(_engine: &mut rhai::Engine) {
    // Geometry is a pure data type — constructors are in module functions.
    // No mutable methods needed beyond construction.
}

// ── Material ──

fn register_material_methods(_engine: &mut rhai::Engine) {
    // Material is a pure data type — constructors are in module functions.
    // No mutable methods needed beyond construction.
}

// ── Object3D ──

fn register_object3d_methods(engine: &mut rhai::Engine) {
    engine.register_fn("set_position", Object3D::set_position);
    engine.register_fn("get_position", Object3D::get_position);
    engine.register_fn("position", Object3D::position);
    engine.register_fn("set_rotation", Object3D::set_rotation);
    engine.register_fn("get_rotation", Object3D::get_rotation);
    engine.register_fn("rotation", Object3D::rotation);
    engine.register_fn("set_scale", Object3D::set_scale);
    engine.register_fn("get_scale", Object3D::get_scale);
    engine.register_fn("scale", Object3D::scale);
    engine.register_fn("translate_x", Object3D::translate_x);
    engine.register_fn("translate_y", Object3D::translate_y);
    engine.register_fn("translate_z", Object3D::translate_z);
    engine.register_fn("look_at", Object3D::look_at);
    engine.register_fn("destroy", Object3D::destroy);
}

// ── Mesh ──

fn register_mesh_methods(engine: &mut rhai::Engine) {
    engine.register_fn("set_position", Mesh::set_position);
    engine.register_fn("get_position", Mesh::get_position);
    engine.register_fn("position", Mesh::position);
    engine.register_fn("set_rotation", Mesh::set_rotation);
    engine.register_fn("get_rotation", Mesh::get_rotation);
    engine.register_fn("rotation", Mesh::rotation);
    engine.register_fn("set_scale", Mesh::set_scale);
    engine.register_fn("get_scale", Mesh::get_scale);
    engine.register_fn("scale", Mesh::scale);
    engine.register_fn("translate_x", Mesh::translate_x);
    engine.register_fn("translate_y", Mesh::translate_y);
    engine.register_fn("translate_z", Mesh::translate_z);
    engine.register_fn("look_at", Mesh::look_at);
    engine.register_fn("destroy", Mesh::destroy);
    engine.register_fn("entity_id", Mesh::entity_id);
}

// ── Light ──

fn register_light_methods(engine: &mut rhai::Engine) {
    engine.register_fn("set_position", Light::set_position);
    engine.register_fn("get_position", Light::get_position);
    engine.register_fn("position", Light::position);
    engine.register_fn("entity_id", Light::entity_id);
}

// ── Camera ──

fn register_camera_methods(engine: &mut rhai::Engine) {
    engine.register_fn("set_position", Camera::set_position);
    engine.register_fn("get_position", Camera::get_position);
    engine.register_fn("position", Camera::position);
    engine.register_fn("look_at", Camera::look_at);
    engine.register_fn("entity_id", Camera::entity_id);
}

// ── Position / Rotation / Scale Proxy ──

fn register_proxy_methods(engine: &mut rhai::Engine) {
    engine.register_fn("set", scene::PositionProxy::set);
    engine.register_fn("set", scene::RotationProxy::set);
    engine.register_fn("set", scene::ScaleProxy::set);
}

// ── THREE module functions ──

fn register_module_functions(engine: &mut rhai::Engine, ctx: &SceneContext) {
    // Vector3 constructors
    engine.register_fn("vector3_from_array", vector3::vector3_from_array);

    // Geometry constructors — return Dynamic because Geometry is not registered as Rhai type
    engine.register_fn("box_geometry", |w: f32, h: f32, d: f32| -> rhai::Dynamic {
        rhai::Dynamic::from(Geometry::box_geometry(w, h, d))
    });
    engine.register_fn(
        "sphere_geometry",
        |r: f32, ws: i64, hs: i64| -> rhai::Dynamic {
            rhai::Dynamic::from(Geometry::sphere_geometry(r, ws, hs))
        },
    );
    engine.register_fn("plane_geometry", |w: f32, h: f32| -> rhai::Dynamic {
        rhai::Dynamic::from(Geometry::plane_geometry(w, h))
    });
    engine.register_fn(
        "cylinder_geometry",
        |rt: f32, rb: f32, h: f32, s: i64| -> rhai::Dynamic {
            rhai::Dynamic::from(Geometry::cylinder_geometry(rt, rb, h, s))
        },
    );
    engine.register_fn(
        "capsule_geometry",
        |r: f32, h: f32, s: i64| -> rhai::Dynamic {
            rhai::Dynamic::from(Geometry::capsule_geometry(r, h, s))
        },
    );
    engine.register_fn("model_geometry", |path: &str| -> rhai::Dynamic {
        rhai::Dynamic::from(Geometry::model_geometry(path))
    });

    // Material constructors — return Dynamic because Material is not registered as Rhai type
    engine.register_fn("mesh_basic_material", |props: rhai::Map| -> rhai::Dynamic {
        rhai::Dynamic::from(Material::mesh_basic_material(props))
    });
    engine.register_fn(
        "mesh_standard_material",
        |props: rhai::Map| -> rhai::Dynamic {
            rhai::Dynamic::from(Material::mesh_standard_material(props))
        },
    );
    engine.register_fn("mesh_phong_material", |props: rhai::Map| -> rhai::Dynamic {
        rhai::Dynamic::from(Material::mesh_phong_material(props))
    });
    engine.register_fn("basic_red", || -> rhai::Dynamic {
        rhai::Dynamic::from(Material::basic_red())
    });
    engine.register_fn("basic_green", || -> rhai::Dynamic {
        rhai::Dynamic::from(Material::basic_green())
    });
    engine.register_fn("basic_blue", || -> rhai::Dynamic {
        rhai::Dynamic::from(Material::basic_blue())
    });

    // Object creators (capture ctx) — receive Dynamic for geo/mat
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "create_mesh",
            move |name: &str, geo: rhai::Dynamic, mat: rhai::Dynamic| -> Mesh {
                let geo: Geometry = geo.try_cast::<Geometry>().unwrap_or(Geometry::Box {
                    width: 1.0,
                    height: 1.0,
                    depth: 1.0,
                });
                let mat: Material = mat.try_cast::<Material>().unwrap_or(Material::basic_red());
                Mesh::new(name, geo, mat, &ctx)
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "directional_light",
            move |color: rhai::Array, intensity: f32| -> Light {
                let c = array_to_color(color);
                Light::directional_light(c, intensity, &ctx)
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "point_light",
            move |color: rhai::Array, intensity: f32, range: f32| -> Light {
                let c = array_to_color(color);
                Light::point_light(c, intensity, range, &ctx)
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "spot_light",
            move |color: rhai::Array, intensity: f32, range: f32, angle: f32| -> Light {
                let c = array_to_color(color);
                Light::spot_light(c, intensity, range, angle, &ctx)
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "ambient_light",
            move |color: rhai::Array, intensity: f32| -> Light {
                let c = array_to_color(color);
                Light::ambient_light(c, intensity, &ctx)
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "perspective_camera",
            move |fov: f32, aspect: f32, near: f32, far: f32| -> Camera {
                Camera::perspective_camera(fov, aspect, near, far, &ctx)
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn(
            "orthographic_camera",
            move |left: f32, right: f32, top: f32, bottom: f32, near: f32, far: f32| -> Camera {
                Camera::orthographic_camera(left, right, top, bottom, near, far, &ctx)
            },
        );
    }
    {
        let ctx = ctx.clone();
        engine.register_fn("create_object", move |name: &str| -> Object3D {
            Object3D::new(name, &ctx)
        });
    }

    // Convenience math — use `vec3(1, 2, 3)` to create vectors in scripts.
    // Returns Dynamic because Rhai requires explicit Dynamic conversion for custom types
    // in register_fn closures.
    engine.register_fn("vec3", |x: f32, y: f32, z: f32| -> rhai::Dynamic {
        rhai::Dynamic::from(Vector3::new(x, y, z))
    });
    engine.register_fn("vector3_zero", vector3::vector3_zero);
    engine.register_fn("vector3_one", vector3::vector3_one);
    engine.register_fn("vector3_up", vector3::vector3_up);
    engine.register_fn("vector3_forward", vector3::vector3_forward);

    // ── THREE static module — enables `THREE::create_mesh(...)` etc. ──
    let three_module = build_three_module(ctx);
    engine.register_static_module("THREE", three_module.into());
}

/// Convert Rhai `[r, g, b]` array to `[f32; 3]`.
fn array_to_color(arr: rhai::Array) -> [f32; 3] {
    let r = arr.first().and_then(|v| v.as_float().ok()).unwrap_or(1.0) as f32;
    let g = arr.get(1).and_then(|v| v.as_float().ok()).unwrap_or(1.0) as f32;
    let b = arr.get(2).and_then(|v| v.as_float().ok()).unwrap_or(1.0) as f32;
    [r, g, b]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Engine;

    fn setup_engine() -> (Engine, SceneContext) {
        let mut engine = Engine::new();
        let ctx = SceneContext::new();

        // Set a scene on the context
        let scene = engine_ecs::Scene::new();
        ctx.set_scene(scene);

        register_threesh_api(&mut engine, &ctx);
        (engine, ctx)
    }

    // ── THREE:: module namespace tests ──

    #[test]
    fn three_module_box_geometry() {
        let (engine, _ctx) = setup_engine();
        let geo: rhai::Dynamic = engine.eval("THREE::box_geometry(2.0, 3.0, 4.0)").unwrap();
        let g = geo.try_cast::<Geometry>().unwrap();
        assert_eq!(
            g,
            Geometry::Box {
                width: 2.0,
                height: 3.0,
                depth: 4.0
            }
        );
    }

    #[test]
    fn three_module_sphere_geometry() {
        let (engine, _ctx) = setup_engine();
        let geo: rhai::Dynamic = engine.eval("THREE::sphere_geometry(1.0, 8, 6)").unwrap();
        let g = geo.try_cast::<Geometry>().unwrap();
        assert_eq!(
            g,
            Geometry::Sphere {
                radius: 1.0,
                width_segments: 8,
                height_segments: 6
            }
        );
    }

    #[test]
    fn three_module_plane_geometry() {
        let (engine, _ctx) = setup_engine();
        let geo: rhai::Dynamic = engine.eval("THREE::plane_geometry(5.0, 5.0)").unwrap();
        let g = geo.try_cast::<Geometry>().unwrap();
        assert_eq!(
            g,
            Geometry::Plane {
                width: 5.0,
                height: 5.0
            }
        );
    }

    #[test]
    fn three_module_cylinder_geometry() {
        let (engine, _ctx) = setup_engine();
        let geo: rhai::Dynamic = engine
            .eval("THREE::cylinder_geometry(0.5, 0.5, 2.0, 8)")
            .unwrap();
        let g = geo.try_cast::<Geometry>().unwrap();
        assert_eq!(
            g,
            Geometry::Cylinder {
                radius_top: 0.5,
                radius_bottom: 0.5,
                height: 2.0,
                segments: 8,
            }
        );
    }

    #[test]
    fn three_module_capsule_geometry() {
        let (engine, _ctx) = setup_engine();
        let geo: rhai::Dynamic = engine
            .eval("THREE::capsule_geometry(0.4, 1.5, 12)")
            .unwrap();
        let g = geo.try_cast::<Geometry>().unwrap();
        assert_eq!(
            g,
            Geometry::Capsule {
                radius: 0.4,
                height: 1.5,
                segments: 12
            }
        );
    }

    #[test]
    fn three_module_model_geometry() {
        let (engine, _ctx) = setup_engine();
        let geo: rhai::Dynamic = engine
            .eval(r#"THREE::model_geometry("models/tree.glb")"#)
            .unwrap();
        let g = geo.try_cast::<Geometry>().unwrap();
        assert_eq!(
            g,
            Geometry::Model {
                path: "models/tree.glb".to_string()
            }
        );
    }

    #[test]
    fn three_module_mesh_standard_material() {
        let (engine, _ctx) = setup_engine();
        let mat: rhai::Dynamic = engine
            .eval(r#"THREE::mesh_standard_material(#{ color: [1.0, 0.0, 0.0], roughness: 0.5 })"#)
            .unwrap();
        // Just verify it's a Material (cast succeeds)
        assert!(mat.try_cast::<Material>().is_some());
    }

    #[test]
    fn three_module_basic_colors() {
        let (engine, _ctx) = setup_engine();
        let red: rhai::Dynamic = engine.eval("THREE::basic_red()").unwrap();
        let green: rhai::Dynamic = engine.eval("THREE::basic_green()").unwrap();
        let blue: rhai::Dynamic = engine.eval("THREE::basic_blue()").unwrap();
        assert!(red.try_cast::<Material>().is_some());
        assert!(green.try_cast::<Material>().is_some());
        assert!(blue.try_cast::<Material>().is_some());
    }

    #[test]
    fn three_module_vec3() {
        let (engine, _ctx) = setup_engine();
        let v: Vector3 = engine.eval("THREE::vec3(1.0, 2.0, 3.0)").unwrap();
        assert!((v.x - 1.0).abs() < 0.01);
        assert!((v.y - 2.0).abs() < 0.01);
        assert!((v.z - 3.0).abs() < 0.01);
    }

    #[test]
    fn three_module_vector3_constants() {
        let (engine, _ctx) = setup_engine();
        let up: Vector3 = engine.eval("THREE::vector3_up()").unwrap();
        assert!((up.y - 1.0).abs() < 0.01);
        let fwd: Vector3 = engine.eval("THREE::vector3_forward()").unwrap();
        assert!((fwd.z - (-1.0)).abs() < 0.01);
    }

    #[test]
    fn three_module_create_mesh() {
        let (engine, ctx) = setup_engine();
        let mesh: Mesh = engine
            .eval(
                r#"
            let geo = THREE::box_geometry(1.0, 1.0, 1.0);
            let mat = THREE::basic_red();
            THREE::create_mesh("Cube", geo, mat)
        "#,
            )
            .unwrap();
        assert!(!mesh.entity_id().is_empty());
        let _ = ctx.take_scene();
    }

    #[test]
    fn three_module_create_mesh_and_set_position() {
        let (engine, ctx) = setup_engine();
        let mesh: Mesh = engine
            .eval(
                r#"
            let geo = THREE::box_geometry(2.0, 2.0, 2.0);
            let mat = THREE::mesh_standard_material(#{ color: [0.0, 1.0, 0.0] });
            let cube = THREE::create_mesh("GreenCube", geo, mat);
            cube.position.set(7.0, 8.0, 9.0);
            cube
        "#,
            )
            .unwrap();
        let pos = mesh.get_position();
        assert!((pos.x - 7.0).abs() < 0.01);
        assert!((pos.y - 8.0).abs() < 0.01);
        assert!((pos.z - 9.0).abs() < 0.01);

        let scene = ctx.take_scene().unwrap();
        let entity = SceneContext::parse_entity(&mesh.entity_id()).unwrap();
        let transform = scene.transforms().local(entity).unwrap();
        assert!((transform.translation.x - 7.0).abs() < 0.01);
    }

    #[test]
    fn three_module_directional_light() {
        let (engine, ctx) = setup_engine();
        let light: Light = engine
            .eval(r#"THREE::directional_light([1.0, 1.0, 1.0], 1.5)"#)
            .unwrap();
        assert!(!light.entity_id().is_empty());
        let _ = ctx.take_scene();
    }

    #[test]
    fn three_module_point_light() {
        let (engine, ctx) = setup_engine();
        let light: Light = engine
            .eval(r#"THREE::point_light([1.0, 0.5, 0.0], 2.0, 10.0)"#)
            .unwrap();
        assert!(!light.entity_id().is_empty());
        let _ = ctx.take_scene();
    }

    #[test]
    fn three_module_spot_light() {
        let (engine, ctx) = setup_engine();
        let light: Light = engine
            .eval(r#"THREE::spot_light([1.0, 1.0, 0.8], 1.0, 20.0, 0.4)"#)
            .unwrap();
        assert!(!light.entity_id().is_empty());
        let _ = ctx.take_scene();
    }

    #[test]
    fn three_module_ambient_light() {
        let (engine, ctx) = setup_engine();
        let light: Light = engine
            .eval(r#"THREE::ambient_light([0.3, 0.3, 0.3], 0.5)"#)
            .unwrap();
        assert!(!light.entity_id().is_empty());
        let _ = ctx.take_scene();
    }

    #[test]
    fn three_module_perspective_camera() {
        let (engine, _ctx) = setup_engine();
        let cam: Camera = engine
            .eval(r#"THREE::perspective_camera(75.0, 1.77, 0.1, 1000.0)"#)
            .unwrap();
        assert!(!cam.entity_id().is_empty());
        assert!((cam.fov - 75.0).abs() < 0.01);
    }

    #[test]
    fn three_module_orthographic_camera() {
        let (engine, _ctx) = setup_engine();
        let cam: Camera = engine
            .eval(r#"THREE::orthographic_camera(-5.0, 5.0, 5.0, -5.0, 0.1, 100.0)"#)
            .unwrap();
        assert!(!cam.entity_id().is_empty());
        assert!(cam.is_orthographic);
    }

    #[test]
    fn three_module_create_object() {
        let (engine, ctx) = setup_engine();
        let obj: Object3D = engine.eval(r#"THREE::create_object("EmptyNode")"#).unwrap();
        assert!(!obj.entity_id.is_empty());
        let _ = ctx.take_scene();
    }

    #[test]
    fn three_module_full_scene_setup() {
        let (engine, ctx) = setup_engine();
        engine
            .eval::<()>(
                r#"
            let ground_geo = THREE::box_geometry(10.0, 0.5, 10.0);
            let ground_mat = THREE::mesh_standard_material(#{ color: [0.3, 0.7, 0.3] });
            let ground = THREE::create_mesh("Ground", ground_geo, ground_mat);
            ground.position.set(0.0, -0.25, 0.0);

            let player_geo = THREE::capsule_geometry(0.4, 1.8, 16);
            let player_mat = THREE::mesh_standard_material(#{ color: [0.2, 0.5, 1.0] });
            let player = THREE::create_mesh("Player", player_geo, player_mat);
            player.position.set(0.0, 1.0, 0.0);

            let sun = THREE::directional_light([1.0, 0.9, 0.8], 1.5);
            sun.position.set(10.0, 20.0, 10.0);

            let ambient = THREE::ambient_light([0.2, 0.2, 0.3], 0.4);

            let cam = THREE::perspective_camera(60.0, 1.77, 0.1, 500.0);
            cam.position.set(0.0, 5.0, 10.0);
            cam.look_at(0.0, 1.0, 0.0);
        "#,
            )
            .unwrap();

        let scene = ctx.take_scene().unwrap();
        let count = scene.iter_objects().count();
        assert!(count >= 5, "Expected at least 5 entities, got {}", count);
    }

    #[test]
    fn three_module_vec3_from_array() {
        let (engine, _ctx) = setup_engine();
        let v: Vector3 = engine
            .eval("THREE::vector3_from_array([4.0, 5.0, 6.0])")
            .unwrap();
        assert!((v.x - 4.0).abs() < 0.01);
        assert!((v.y - 5.0).abs() < 0.01);
        assert!((v.z - 6.0).abs() < 0.01);
    }

    // ── Existing global-function tests (unchanged) ──

    #[test]
    fn incremental_registration_test() {
        // Test each step of register_threesh_api to find the breaking point
        let mut engine = rhai::Engine::new();
        let ctx = SceneContext::new();
        let scene = engine_ecs::Scene::new();
        ctx.set_scene(scene);

        // Step 1: Just Vector3
        engine.register_type::<Vector3>();
        engine.register_fn("vec3", |x: f32, y: f32, z: f32| -> Vector3 {
            Vector3::new(x, y, z)
        });
        let _: Vector3 = engine.eval("vec3(1.0, 2.0, 3.0)").unwrap();
        eprintln!("Step 1 OK: Vector3");

        // Step 2: Add box_geometry
        engine.register_fn("box_geo", |w: f32, h: f32, d: f32| -> rhai::Dynamic {
            rhai::Dynamic::from(Geometry::box_geometry(w, h, d))
        });
        let _: rhai::Dynamic = engine.eval("box_geo(1.0, 2.0, 3.0)").unwrap();
        eprintln!("Step 2 OK: box_geometry");

        // Step 3: Add Material constructor
        engine.register_fn("red_mat", || -> rhai::Dynamic {
            rhai::Dynamic::from(Material::basic_red())
        });
        let _: rhai::Dynamic = engine.eval("red_mat()").unwrap();
        eprintln!("Step 3 OK: Material");

        // Step 4: Register Mesh type
        engine.register_type::<Mesh>();
        eprintln!("Step 4 OK: register_type::<Mesh>()");

        // Step 5: create_mesh function
        {
            let ctx = ctx.clone();
            engine.register_fn(
                "create_mesh",
                move |name: &str, geo: rhai::Dynamic, mat: rhai::Dynamic| -> Mesh {
                    let geo: Geometry = geo.try_cast::<Geometry>().unwrap_or(Geometry::Box {
                        width: 1.0,
                        height: 1.0,
                        depth: 1.0,
                    });
                    let mat: Material = mat.try_cast::<Material>().unwrap_or(Material::basic_red());
                    Mesh::new(name, geo, mat, &ctx)
                },
            );
        }
        eprintln!("Step 5 OK: create_mesh registered");

        // Test create_mesh
        let mesh: Mesh = engine
            .eval(
                r#"
            let geo = box_geo(1.0, 2.0, 3.0);
            let mat = red_mat();
            let m = create_mesh("Test", geo, mat);
            m
        "#,
            )
            .unwrap();
        assert!(!mesh.entity_id().is_empty());
        eprintln!("Step 6 OK: create_mesh works!");

        // Step 7: Light and Camera registration
        engine.register_type::<Light>();
        engine.register_type::<Camera>();
        eprintln!("Step 7 OK: Light and Camera types");
    }

    #[test]
    fn vector3_create_and_set() {
        let (engine, _ctx) = setup_engine();

        let result: Vector3 = engine
            .eval(
                r#"
            let v = vec3(1.0, 2.0, 3.0);
            v.set(4.0, 5.0, 6.0);
            v
        "#,
            )
            .unwrap();
        assert!((result.x - 4.0).abs() < 0.01);
        assert!((result.y - 5.0).abs() < 0.01);
        assert!((result.z - 6.0).abs() < 0.01);
    }

    #[test]
    fn vector3_arithmetic() {
        let (engine, _ctx) = setup_engine();
        let result: Vector3 = engine
            .eval(
                r#"
            let a = vec3(1.0, 2.0, 3.0);
            let b = vec3(4.0, 5.0, 6.0);
            a.add(b);
            a.multiply_scalar(2.0);
            a
        "#,
            )
            .unwrap();
        // (1+4)*2=10, (2+5)*2=14, (3+6)*2=18
        assert!((result.x - 10.0).abs() < 0.01);
        assert!((result.y - 14.0).abs() < 0.01);
        assert!((result.z - 18.0).abs() < 0.01);
    }

    #[test]
    fn vector3_dot_and_cross() {
        let (engine, _ctx) = setup_engine();
        let (dot, cross): (f32, Vector3) = engine
            .eval(
                r#"
            let a = vec3(1.0, 0.0, 0.0);
            let b = vec3(0.0, 1.0, 0.0);
            let d = a.dot(b);
            let c = a.cross(b);
            [d, c]
        "#,
            )
            .unwrap();
        assert!((dot - 0.0).abs() < 0.01);
        assert!((cross.x - 0.0).abs() < 0.01);
        assert!((cross.y - 0.0).abs() < 0.01);
        assert!((cross.z - 1.0).abs() < 0.01);
    }

    #[test]
    fn vector3_length_and_normalize() {
        let (engine, _ctx) = setup_engine();
        let result: Vector3 = engine
            .eval(
                r#"
            let v = vec3(3.0, 4.0, 0.0);
            let len = v.length();
            v.normalize();
            v
        "#,
            )
            .unwrap();
        assert!((result.x - 0.6).abs() < 0.01);
        assert!((result.y - 0.8).abs() < 0.01);
        assert!((result.z - 0.0).abs() < 0.01);
    }

    #[test]
    fn create_mesh_and_set_position() {
        let (engine, ctx) = setup_engine();

        let mesh: Mesh = engine
            .eval(
                r#"
            let geo = box_geometry(1.0, 2.0, 3.0);
            let mat = mesh_standard_material(#{
                color: [1.0, 0.0, 0.0],
                roughness: 0.5,
                metalness: 0.1,
            });
            let cube = create_mesh("TestCube", geo, mat);
            cube.position.set(5.0, 10.0, 15.0);
            cube
        "#,
            )
            .unwrap();

        let pos = mesh.get_position();
        assert!((pos.x - 5.0).abs() < 0.01);
        assert!((pos.y - 10.0).abs() < 0.01);
        assert!((pos.z - 15.0).abs() < 0.01);

        // Verify entity exists in engine scene
        let scene = ctx.take_scene().unwrap();
        let entity = SceneContext::parse_entity(&mesh.entity_id()).unwrap();
        let transform = scene.transforms().local(entity).unwrap();
        assert!((transform.translation.x - 5.0).abs() < 0.01);
        assert!((transform.translation.y - 10.0).abs() < 0.01);
        assert!((transform.translation.z - 15.0).abs() < 0.01);
    }

    #[test]
    fn directional_light_creates() {
        let (engine, ctx) = setup_engine();

        let light: Light = engine
            .eval(
                r#"
            let light = directional_light([1.0, 0.8, 0.6], 2.0);
            light.position.set(10.0, 20.0, 30.0);
            light
        "#,
            )
            .unwrap();

        assert!(!light.entity_id().is_empty());
        let pos = light.get_position();
        assert!((pos.x - 10.0).abs() < 0.01);
        assert!((pos.y - 20.0).abs() < 0.01);
        assert!((pos.z - 30.0).abs() < 0.01);

        // Clean up
        let _ = ctx.take_scene();
    }

    #[test]
    fn camera_look_at() {
        let (engine, _ctx) = setup_engine();

        let cam: Camera = engine
            .eval(
                r#"
            let cam = perspective_camera(75.0, 1.77, 0.1, 1000.0);
            cam.position.set(0.0, 5.0, 10.0);
            cam.look_at(0.0, 0.0, 0.0);
            cam
        "#,
            )
            .unwrap();

        // Camera should now face the origin (roughly looking down and back)
        let rot = cam.base.get_rotation();
        // We expect non-zero rotation (pitch down, yaw pointing to origin)
        assert!(
            (rot.x).abs() > 0.0 || (rot.y).abs() > 0.0,
            "Camera should have non-zero rotation after lookAt"
        );
    }

    #[test]
    fn three_js_like_scene_setup() {
        let (engine, ctx) = setup_engine();

        // Full scene setup like a three.js tutorial
        engine
            .eval::<()>(
                r#"
            // Create ground
            let ground_geo = box_geometry(10.0, 0.5, 10.0);
            let ground_mat = mesh_standard_material(#{
                color: [0.3, 0.8, 0.3],
                roughness: 0.9,
            });
            let ground = create_mesh("Ground", ground_geo, ground_mat);
            ground.position.set(0.0, -0.5, 0.0);

            // Create player
            let player_geo = capsule_geometry(0.5, 2.0, 16);
            let player_mat = mesh_standard_material(#{
                color: [0.2, 0.4, 1.0],
                metalness: 0.1,
                roughness: 0.4,
            });
            let player = create_mesh("Player", player_geo, player_mat);
            player.position.set(0.0, 1.0, 0.0);

            // Add lights
            let sun = directional_light([1.0, 0.95, 0.8], 1.5);
            sun.position.set(5.0, 10.0, 5.0);

            let ambient = ambient_light([0.3, 0.3, 0.4], 0.5);

            // Add camera
            let cam = perspective_camera(75.0, 1.77, 0.1, 1000.0);
            cam.position.set(0.0, 8.0, 12.0);
            cam.look_at(0.0, 1.0, 0.0);
        "#,
            )
            .unwrap();

        // Verify entities are in the scene
        let scene = ctx.take_scene().unwrap();
        let entity_count = scene.iter_objects().count();
        assert!(
            entity_count >= 5,
            "Expected 5+ entities, got {}",
            entity_count
        );
    }
}

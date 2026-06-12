//! Integration test: render one frame with a debug cube.

use engine_core::math::Transform;
use engine_render::{RenderCamera, RenderDevice, RenderFrame, RenderObject, RenderWorld};
use engine_render_wgpu::WgpuRenderDevice;
use winit::window::WindowAttributes;

#[cfg(target_os = "linux")]
fn has_linux_display() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("WAYLAND_SOCKET").is_some()
        || std::env::var_os("DISPLAY").is_some()
}

#[allow(deprecated)]
#[test]
fn render_one_frame_with_debug_cube_succeeds() {
    #[cfg(target_os = "linux")]
    if !has_linux_display() {
        eprintln!("skipping wgpu render test: neither Wayland nor X11 display is available");
        return;
    }

    let mut builder = winit::event_loop::EventLoop::builder();
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_any_thread(true);
    }
    let event_loop = builder
        .build()
        .expect("failed to create event loop (no display?)");
    let window = event_loop
        .create_window(
            WindowAttributes::default()
                .with_title("wgpu render test")
                .with_inner_size(winit::dpi::PhysicalSize::new(256, 256)),
        )
        .expect("failed to create window");
    let mut device =
        WgpuRenderDevice::new(&window).expect("failed to create WgpuRenderDevice with surface");

    let world = RenderWorld {
        camera: Some(RenderCamera {
            object: engine_core::EntityId::from_u128(1),
            transform: Transform::default(),
            projection: engine_render::RenderProjection::Perspective,
            vertical_fov_degrees: 60.0,
            near: 0.1,
            far: 100.0,
            look_at_target: None,
        }),
        objects: vec![RenderObject {
            object: engine_core::EntityId::from_u128(2),
            transform: Transform::default(),
            mesh: "debug/cube".to_string(),
            material: "debug/default".to_string(),
        }],
        sprites: vec![],
        lights: vec![],
        particles: vec![],
        skybox: None,
    };

    device
        .submit_render_world(&world, RenderFrame { frame_index: 0 })
        .expect("submit_render_world should succeed");

    assert_eq!(device.submitted_worlds(), 1, "should submit one world");
}

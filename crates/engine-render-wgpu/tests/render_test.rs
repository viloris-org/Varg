//! Integration test: render one frame with a debug cube.

use engine_core::math::Transform;
use engine_render::{RenderCamera, RenderDevice, RenderFrame, RenderObject, RenderWorld};
use engine_render_wgpu::WgpuRenderDevice;
use winit::window::WindowAttributes;

#[allow(deprecated)]
#[test]
fn render_one_frame_with_debug_cube_succeeds() {
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
            vertical_fov_degrees: 60.0,
            near: 0.1,
            far: 100.0,
        }),
        objects: vec![RenderObject {
            object: engine_core::EntityId::from_u128(2),
            transform: Transform::default(),
            mesh: "debug/cube".to_string(),
            material: "debug/default".to_string(),
        }],
        lights: vec![],
    };

    device
        .submit_render_world(&world, RenderFrame { frame_index: 0 })
        .expect("submit_render_world should succeed");

    assert_eq!(device.submitted_worlds(), 1, "should submit one world");
}

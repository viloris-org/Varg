//! Integration test: surface creation from a real winit window.

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
fn surface_creation_succeeds_with_winit_window() {
    #[cfg(target_os = "linux")]
    if !has_linux_display() {
        eprintln!("skipping wgpu surface test: neither Wayland nor X11 display is available");
        return;
    }

    let mut builder = winit::event_loop::EventLoop::builder();
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_any_thread(true);
    }
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        builder.with_any_thread(true);
    }
    let Ok(event_loop) = builder.build() else {
        eprintln!("skipping wgpu surface test: failed to create event loop");
        return;
    };
    let window = event_loop
        .create_window(
            WindowAttributes::default()
                .with_title("wgpu surface test")
                .with_inner_size(winit::dpi::PhysicalSize::new(256, 256)),
        )
        .expect("failed to create window");
    let device =
        WgpuRenderDevice::new(&window).expect("failed to create WgpuRenderDevice with surface");
    assert_eq!(device.submitted_worlds(), 0);
}

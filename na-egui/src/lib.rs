use egui_wgpu::wgpu;
use egui_winit::winit;

use winit::event_loop::{EventLoop, EventLoopBuilder, EventLoopWindowTarget};

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

use winit::event_loop::ControlFlow;

use egui_wgpu::winit::Painter;
use egui_winit::State;
//use egui_winit_platform::{Platform, PlatformDescriptor};
use winit::event::Event::*;
const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

/// A custom event type for the winit app.
enum Event {
    RequestRedraw,
}

/// Enable egui to request redraws via a custom Winit event...
#[derive(Clone)]
struct RepaintSignal(std::sync::Arc<std::sync::Mutex<winit::event_loop::EventLoopProxy<Event>>>);

fn create_window<T>(
    event_loop: &EventLoopWindowTarget<T>,
    state: &mut State,
    painter: &mut Painter,
) -> Option<winit::window::Window> {
    let window = winit::window::WindowBuilder::new()
        .with_decorations(true)
        .with_resizable(true)
        .with_transparent(false)
        .with_title("egui winit + wgpu example")
        .with_inner_size(winit::dpi::PhysicalSize {
            width: INITIAL_WIDTH,
            height: INITIAL_HEIGHT,
        })
        .build(event_loop)
        .unwrap();

    if let Err(err) = pollster::block_on(painter.set_window(Some(&window))) {
        log::error!("Failed to associate new Window with Painter: {err:?}");
        return None;
    }

    // NB: calling set_window will lazily initialize render state which
    // means we will be able to query the maximum supported texture
    // dimensions
    if let Some(max_size) = painter.max_texture_side() {
        state.set_max_texture_side(max_size);
    }

    let pixels_per_point = window.scale_factor() as f32;
    state.set_pixels_per_point(pixels_per_point);

    window.request_redraw();

    Some(window)
}

fn _main(event_loop: EventLoop<Event>) {
    let ctx = egui::Context::default();
    let repaint_signal = RepaintSignal(std::sync::Arc::new(std::sync::Mutex::new(
        event_loop.create_proxy(),
    )));
    ctx.set_request_repaint_callback(move |_info| {
        log::debug!("Request Repaint Callback");
        repaint_signal
            .0
            .lock()
            .unwrap()
            .send_event(Event::RequestRedraw)
            .ok();
    });

    let mut state = State::new(&event_loop);
    let mut painter = Painter::new(
        egui_wgpu::WgpuConfiguration {
            supported_backends: wgpu::Backends::all(),
            power_preference: wgpu::PowerPreference::LowPower,
            device_descriptor: std::sync::Arc::new(|_adapter| wgpu::DeviceDescriptor {
                label: None,
                features: wgpu::Features::default(),
                limits: wgpu::Limits::default(),
            }),
            present_mode: wgpu::PresentMode::Fifo,
            ..Default::default()
        },
        1, // msaa samples
        Some(wgpu::TextureFormat::Depth24Plus),
        false,
    );
    let mut window: Option<winit::window::Window> = None;
    let mut egui_demo_windows = egui_demo_lib::DemoWindows::default();

    event_loop.run(move |event, event_loop, control_flow| match event {
        Resumed => match window {
            None => {
                window = create_window(event_loop, &mut state, &mut painter);
            }
            Some(ref window) => {
                pollster::block_on(painter.set_window(Some(window))).unwrap_or_else(|err| {
                    log::error!(
                        "Failed to associate window with painter after resume event: {err:?}"
                    )
                });
                window.request_redraw();
            }
        },
        Suspended => {
            window = None;
        }
        RedrawRequested(..) => {
            if let Some(window) = window.as_ref() {
                log::debug!("RedrawRequested, with window set");
                let raw_input = state.take_egui_input(window);

                log::debug!("RedrawRequested: calling ctx.run()");
                let full_output = ctx.run(raw_input, |ctx| {
                    egui_demo_windows.ui(ctx);
                });
                log::debug!("RedrawRequested: called ctx.run()");
                state.handle_platform_output(window, &ctx, full_output.platform_output);

                log::debug!("RedrawRequested: calling paint_and_update_textures()");
                painter.paint_and_update_textures(
                    state.pixels_per_point(),
                    [0.0, 0.0, 0.0, 0.0],
                    &ctx.tessellate(full_output.shapes),
                    &full_output.textures_delta,
                    false, // capture
                );

                if full_output.repaint_after.is_zero() {
                    window.request_redraw();
                }
            } else {
                log::debug!("RedrawRequested, with no window set");
            }
        }
        MainEventsCleared | UserEvent(Event::RequestRedraw) => {
            if let Some(window) = window.as_ref() {
                log::debug!("Winit event (main events cleared or user event) - request_redraw()");
                window.request_redraw();
            }
        }
        WindowEvent { event, .. } => {
            log::debug!("Window Event: {event:?}");
            match event {
                winit::event::WindowEvent::Resized(size) => {
                    painter.on_window_resized(size.width, size.height);
                }
                winit::event::WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            }

            let response = state.on_event(&ctx, &event);
            if response.repaint {
                if let Some(window) = window.as_ref() {
                    window.request_redraw();
                }
            }
        }
        _ => (),
    });
}

#[allow(dead_code)]
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Trace) // Default comes from `log::max_level`, i.e. Off
            .with_filter(
                android_logger::FilterBuilder::new()
                    .filter_level(log::LevelFilter::Debug)
                    //.filter_module("android_activity", log::LevelFilter::Trace)
                    //.filter_module("winit", log::LevelFilter::Trace)
                    .build(),
            ),
    );

    let event_loop = EventLoopBuilder::with_user_event()
        .with_android_app(app)
        .build();
    _main(event_loop);
}

#[allow(dead_code)]
#[cfg(not(target_os = "android"))]
fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Warn) // Default Log Level
        .parse_default_env()
        .init();

    let event_loop = EventLoopBuilder::with_user_event().build();
    _main(event_loop);
}

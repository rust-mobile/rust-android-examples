use std::{num::NonZeroU32, sync::Arc};

use egui::ViewportId;
use egui_wgpu::{winit::Painter, RendererOptions};
use egui_winit::{winit, State};
use tracing::{debug, error};
#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;
use winit::{
    event::Event::*,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
};
const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

/// A custom event type for the winit app.
#[derive(Debug)]
enum Event {
    RequestRedraw,
}

/// Enable egui to request redraws via a custom Winit event...
#[derive(Clone)]
struct RepaintSignal(std::sync::Arc<std::sync::Mutex<winit::event_loop::EventLoopProxy<Event>>>);

struct Window {
    window: Arc<winit::window::Window>,
    state: egui_winit::State,
}

fn create_window(
    event_loop: &ActiveEventLoop,
    ctx: egui::Context,
    painter: &mut Painter,
) -> Option<Window> {
    let window_attributes = winit::window::Window::default_attributes()
        .with_decorations(true)
        .with_resizable(true)
        .with_transparent(false)
        .with_title("egui winit + wgpu example")
        .with_inner_size(winit::dpi::PhysicalSize {
            width: INITIAL_WIDTH,
            height: INITIAL_HEIGHT,
        });

    let window = Arc::new(event_loop.create_window(window_attributes).unwrap());

    if let Err(err) =
        pollster::block_on(painter.set_window(egui::ViewportId::ROOT, Some(window.clone())))
    {
        error!("Failed to associate new Window with Painter: {err:?}");
        return None;
    }

    let native_pixels_per_point = Some(window.scale_factor() as f32);
    let mut state = State::new(
        ctx.clone(),
        ViewportId::ROOT,
        &window,
        native_pixels_per_point,
        None,
        None,
    );

    // NB: calling set_window will lazily initialize render state which
    // means we will be able to query the maximum supported texture
    // dimensions
    if let Some(max_size) = painter.max_texture_side() {
        state.set_max_texture_side(max_size);
    }

    window.request_redraw();

    Some(Window { window, state })
}

fn _main(event_loop: EventLoop<Event>) {
    let ctx = egui::Context::default();
    let repaint_signal = RepaintSignal(std::sync::Arc::new(std::sync::Mutex::new(
        event_loop.create_proxy(),
    )));
    ctx.set_request_repaint_callback(move |_info| {
        debug!("Request Repaint Callback");
        repaint_signal
            .0
            .lock()
            .unwrap()
            .send_event(Event::RequestRedraw)
            .ok();
    });

    let mut painter = pollster::block_on(Painter::new(
        ctx.clone(),
        egui_wgpu::WgpuConfiguration::default(),
        false, // don't require transparent backbuffer
        RendererOptions::default(),
    ));
    let mut window: Option<Window> = None;
    let mut egui_demo_windows = egui_demo_lib::DemoWindows::default();

    #[allow(deprecated)]
    event_loop
        .run(move |event, event_loop| {
            event_loop.set_control_flow(ControlFlow::Wait);

            debug!("handling winit event: {event:?}");

            match (&mut window, event) {
                (None, Resumed) => {
                    window = create_window(event_loop, ctx.clone(), &mut painter);
                }
                (Some(ref window), Resumed) => {
                    pollster::block_on(
                        painter.set_window(ViewportId::ROOT, Some(window.window.clone())),
                    )
                    .unwrap_or_else(|err| {
                        error!(
                            "Failed to associate window with painter after resume event: {err:?}"
                        )
                    });
                    window.window.request_redraw();
                }
                (_, Suspended) => {
                    window = None;
                    pollster::block_on(
                        painter.set_window(ViewportId::ROOT, None),
                    )
                    .unwrap_or_else(|err| {
                        error!(
                            "Failed to disassociate window from painter after Suspended event: {err:?}"
                        )
                    });
                }
                (_, UserEvent(Event::RequestRedraw)) => {
                    if let Some(window) = window.as_ref() {
                        debug!("Winit request redraw, user event");
                        window.window.request_redraw();
                    }
                }
                (
                    Some(window),
                    WindowEvent {
                        window_id, event, ..
                    },
                ) if window.window.id() == window_id => {
                    debug!("Window Event: {event:?}");

                    let response = window.state.on_window_event(&window.window, &event);
                    // egui_winit probably shouldn't be returning repaint=true for RedrawRequested
                    // events but in any case we special case RedrawRequested events here so we can
                    // avoid creating an infinite repaint cycle.
                    if !matches!(event, winit::event::WindowEvent::RedrawRequested)
                        && response.repaint
                    {
                        window.window.request_redraw();
                    }

                    if !response.consumed {
                        match event {
                            winit::event::WindowEvent::RedrawRequested => {
                                let raw_input = window.state.take_egui_input(&window.window);
                                let full_output = ctx.run(raw_input, |ctx| {
                                    egui_demo_windows.ui(ctx);
                                });
                                window.state.handle_platform_output(
                                    &window.window,
                                    full_output.platform_output,
                                );
                                painter.paint_and_update_textures(
                                    ViewportId::ROOT,
                                    full_output.pixels_per_point,
                                    [0.0, 0.0, 0.0, 0.0],
                                    &ctx.tessellate(
                                        full_output.shapes,
                                        full_output.pixels_per_point,
                                    ),
                                    &full_output.textures_delta,
                                    vec![],
                                );
                            }
                            winit::event::WindowEvent::Resized(size) => {
                                let width = NonZeroU32::new(size.width).unwrap_or(NonZeroU32::MIN);
                                let height =
                                    NonZeroU32::new(size.height).unwrap_or(NonZeroU32::MIN);
                                painter.on_window_resized(ViewportId::ROOT, width, height);
                            }
                            winit::event::WindowEvent::CloseRequested => {
                                event_loop.exit();
                            }
                            _ => {}
                        }
                    }
                }
                _ => (),
            }
        })
        .unwrap();
}

const DEFAULT_ENV_FILTER: &str = "debug,wgpu_hal=info,winit=info,naga=info";

#[allow(dead_code)]
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: AndroidApp) {
    use std::sync::OnceLock;

    use winit::platform::android::EventLoopBuilderExtAndroid;

    std::env::set_var("RUST_BACKTRACE", "full");
    std::env::set_var("WGPU_BACKEND", "vulkan");

    // NB: android_main can be called multiple times if the application Activity
    // is destroyed and recreated so we use a OnceLock to ensure that we only
    // initialize our global state once (otherwise tracing_subscriber will panic
    // if we try to initialize it multiple times).
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        use tracing_subscriber::prelude::*;

        let filter_layer = tracing_subscriber::EnvFilter::new(DEFAULT_ENV_FILTER);
        let android_layer = paranoid_android::layer(env!("CARGO_PKG_NAME"))
            .with_ansi(false)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .with_thread_names(true);
        tracing_subscriber::registry()
            .with(filter_layer)
            .with(android_layer)
            .init();
    });

    eprintln!("Egui demo started");
    tracing::info!("Egui demo started");

    let event_loop = EventLoop::with_user_event()
        .with_android_app(app)
        .build()
        .unwrap();
    _main(event_loop);
}

#[allow(dead_code)]
#[cfg(not(target_os = "android"))]
fn main() {
    if !std::option_env!("RUST_LOG").is_some() {
        std::env::set_var("RUST_LOG", DEFAULT_ENV_FILTER);
    }
    tracing_subscriber::fmt::init();

    let event_loop = EventLoop::with_user_event().build().unwrap();
    _main(event_loop);
}

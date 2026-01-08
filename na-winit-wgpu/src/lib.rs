use std::borrow::Cow;
#[cfg(target_os = "android")]
use std::ffi::c_void;
#[cfg(target_os = "android")]
use std::ptr::NonNull;
use std::sync::Arc;

use tracing::{info, trace, warn};

use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};
use wgpu::{Adapter, Device, Instance, PipelineLayout, Queue, RenderPipeline, ShaderModule};
use wgpu::{PipelineCompilationOptions, TextureFormat};

use winit::error::EventLoopError;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
};

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

struct RenderState {
    device: Device,
    queue: Queue,
    shader: ShaderModule,
    target_format: TextureFormat,
    pipeline_layout: PipelineLayout,
    render_pipeline: RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
}

/// On Android it's not safe to rely on Winit's Window implementing
/// HasWindowHandle because it doesn't (and can't) own the `ANativeWindow` that
/// is used as a raw window handle. (If it acquired a reference then it would
/// have to effectively leak that reference every time the app gets a new native
/// window)
///
/// When an Android application suspends, the `ANativeWindow`s associated with
/// surfaces may be released and so a raw window handle obtained via
/// `winit::Window::window_handle()` may become an invalid pointer if nothing
/// `_acquire()`d a reference to it.
///
/// To work around this, we create our own wrapper around the `ANativeWindow` so
/// we can `_acquire()` an owning reference that we `_release()` when Dropped.
struct OwnedWindowHandle {
    #[cfg(not(target_os = "android"))]
    window: Arc<winit::window::Window>,
    #[cfg(target_os = "android")]
    native_window: NonNull<c_void>, // ANativeWindow*
}
unsafe impl Send for OwnedWindowHandle {}
unsafe impl Sync for OwnedWindowHandle {}
impl OwnedWindowHandle {
    fn new(window: Arc<winit::window::Window>) -> Result<Self, HandleError> {
        #[cfg(not(target_os = "android"))]
        {
            Ok(Self { window })
        }

        #[cfg(target_os = "android")]
        {
            let raw_handle = window.window_handle()?.as_raw();
            if let raw_window_handle::RawWindowHandle::AndroidNdk(handle) = raw_handle {
                let native_window = handle.a_native_window;
                extern "C" {
                    fn ANativeWindow_acquire(window: *mut c_void);
                }
                // SAFETY: We assume the caller has ensured that `native_window` is a valid
                // pointer to an `ANativeWindow` and that we own a reference to it.
                unsafe {
                    ANativeWindow_acquire(native_window.as_ptr());
                }
                Ok(Self { native_window })
            } else {
                panic!("Expected AndroidNdk window handle");
            }
        }
    }
}
impl Drop for OwnedWindowHandle {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        {
            extern "C" {
                fn ANativeWindow_release(window: *mut c_void);
            }
            // SAFETY: We assume that `native_window` is a valid pointer to an
            // `ANativeWindow` that we own a reference to.
            unsafe {
                ANativeWindow_release(self.native_window.as_ptr());
            }
        }
    }
}
impl HasWindowHandle for OwnedWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        #[cfg(not(target_os = "android"))]
        {
            self.window.window_handle()
        }

        #[cfg(target_os = "android")]
        unsafe {
            Ok(raw_window_handle::WindowHandle::borrow_raw(
                raw_window_handle::AndroidNdkWindowHandle::new(self.native_window).into(),
            ))
        }
    }
}
impl HasDisplayHandle for OwnedWindowHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        #[cfg(not(target_os = "android"))]
        {
            self.window.display_handle()
        }

        #[cfg(target_os = "android")]
        unsafe {
            Ok(raw_window_handle::DisplayHandle::borrow_raw(
                raw_window_handle::AndroidDisplayHandle::new().into(),
            ))
        }
    }
}

struct SurfaceState {
    window: Arc<winit::window::Window>,
    surface: wgpu::Surface<'static>,
}

struct App {
    instance: Instance,
    adapter: Option<Adapter>,
    surface_state: Option<SurfaceState>,
    render_state: Option<RenderState>,
    rotation: f32,
    position_x: f32,
    position_y: f32,
    last_drag_pos: Option<(f32, f32)>,
    is_dragging: bool,
}

impl App {
    fn new(instance: Instance) -> Self {
        Self {
            instance,
            adapter: None,
            surface_state: None,
            render_state: None,
            rotation: 0.0,
            position_x: 0.0,
            position_y: 0.0,
            last_drag_pos: None,
            is_dragging: false,
        }
    }
}

impl App {
    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Arc<winit::window::Window> {
        let window_attributes = winit::window::WindowAttributes::default();
        let window = event_loop.create_window(window_attributes).unwrap();
        let window = Arc::new(window);
        window
    }

    fn create_surface(&mut self, event_loop: &ActiveEventLoop) {
        let window = if let Some(existing_surface_state) = self.surface_state.as_ref() {
            existing_surface_state.window.clone()
        } else {
            self.create_window(event_loop)
        };
        info!("WGPU: creating surface for native window");

        let window_handle =
            OwnedWindowHandle::new(Arc::clone(&window)).expect("Failed to get owned window handle");
        // # Panics
        // Currently create_surface is documented to only possibly fail with with WebGL2
        let surface = self
            .instance
            .create_surface(window_handle)
            .expect("Failed to create surface");
        self.surface_state = Some(SurfaceState { window, surface });
    }

    fn create_render_pipeline(
        device: &wgpu::Device,
        pipeline_layout: &wgpu::PipelineLayout,
        shader_module: &wgpu::ShaderModule,
        target_format: TextureFormat,
    ) -> wgpu::RenderPipeline {
        info!("WGPU: creating render pipeline");
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader_module,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader_module,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(target_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    async fn init_render_state(adapter: &Adapter, target_format: TextureFormat) -> RenderState {
        info!("Initializing render state for target format: {target_format:?}");

        info!("WGPU: requesting device");
        // Create the logical device and command queue
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                required_features: wgpu::Features::empty(),
                // Make sure we use the texture resolution limits from the adapter, so we can support images the size of the swapchain.
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .expect("Failed to create device");

        info!("WGPU: loading shader");
        // Load the shaders from disk
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        info!("WGPU: creating uniform buffer");
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Uniform Buffer"),
            size: (std::mem::size_of::<f32>() * 4) as u64, // rotation, position_x, position_y, padding
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        info!("WGPU: creating pipeline layout");
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let render_pipeline =
            Self::create_render_pipeline(&device, &pipeline_layout, &shader, target_format);

        RenderState {
            device,
            queue,
            shader,
            target_format,
            pipeline_layout,
            render_pipeline,
            uniform_buffer,
            uniform_bind_group,
        }
    }

    fn ensure_render_pipeline_for_target_format(&mut self, target_format: TextureFormat) {
        if let Some(render_state) = &self.render_state {
            if render_state.target_format == target_format {
                return;
            }
        }
        if self.render_state.is_none() {
            warn!("Can't update render pipeline because render state is not initialized yet");
            return;
        }

        let RenderState {
            device,
            queue,
            shader,
            target_format,
            render_pipeline: _,
            pipeline_layout,
            uniform_buffer,
            uniform_bind_group,
        } = self.render_state.take().unwrap();

        let render_pipeline =
            Self::create_render_pipeline(&device, &pipeline_layout, &shader, target_format);

        self.render_state = Some(RenderState {
            device,
            queue,
            shader,
            target_format,
            pipeline_layout,
            render_pipeline,
            uniform_buffer,
            uniform_bind_group,
        });
    }

    fn find_swapchain_texture_format(&self) -> Option<TextureFormat> {
        self.surface_state.as_ref().map(|surface_state| {
            let adapter = self
                .adapter
                .as_ref()
                .expect("Adapter should be initialized");
            info!("WGPU: finding supported swapchain format");
            let surface_caps = surface_state.surface.get_capabilities(adapter);
            for format in surface_caps.formats.iter() {
                info!("WGPU:   supported format: {format:?}");
            }
            surface_caps.formats[0]
        })
    }

    // We want to defer the initialization of our render state until
    // we have a surface so we can take its format into account.
    //
    // After we've initialized our render state once though we
    // expect all future surfaces will have the same format and we
    // so this stat will remain valid.
    async fn ensure_render_state_for_surface(&mut self) {
        if let Some(surface_state) = &self.surface_state {
            if self.adapter.is_none() {
                info!("WGPU: requesting a suitable adapter (compatible with our surface)");
                let adapter = self
                    .instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::default(),
                        force_fallback_adapter: false,
                        // Request an adapter which can render to our surface
                        compatible_surface: Some(&surface_state.surface),
                    })
                    .await
                    .expect("Failed to find an appropriate adapter");

                self.adapter = Some(adapter);
            }
            let adapter = self.adapter.as_ref().unwrap();

            let swapchain_format = self
                .find_swapchain_texture_format()
                .expect("Failed to find a supported swapchain format for our surface");

            if self.render_state.is_none() {
                let rs = Self::init_render_state(adapter, swapchain_format).await;
                self.render_state = Some(rs);
            } else {
                self.ensure_render_pipeline_for_target_format(swapchain_format);
            }
        }
    }

    fn configure_surface_swapchain(&mut self) {
        if let (Some(adapter), Some(render_state), Some(surface_state)) =
            (&self.adapter, &self.render_state, &self.surface_state)
        {
            let swapchain_format = render_state.target_format;
            let size = surface_state.window.inner_size();

            let config = surface_state
                .surface
                .get_default_config(adapter, u32::max(size.width, 1), u32::max(size.height, 1))
                .expect("Window surface can't be rendered to by adapter");

            info!("WGPU: Configuring surface swapchain: format = {swapchain_format:?}, size = {size:?}");
            surface_state
                .surface
                .configure(&render_state.device, &config);
        }
    }

    fn queue_redraw(&self) {
        if let Some(surface_state) = &self.surface_state {
            trace!("Making Redraw Request");
            surface_state.window.request_redraw();
        }
    }

    fn resume(&mut self, event_loop: &ActiveEventLoop) {
        info!("Resumed, creating render state...");
        self.create_surface(event_loop);
        pollster::block_on(self.ensure_render_state_for_surface());
        self.configure_surface_swapchain();
        self.queue_redraw();
    }

    fn start_drag(&mut self, x: f32, y: f32) {
        self.is_dragging = true;
        self.last_drag_pos = Some((x, y));
        self.update_position(x, y);
    }

    fn update_drag(&mut self, x: f32, y: f32) {
        if let Some((last_x, last_y)) = self.last_drag_pos {
            let delta_x = x - last_x;
            let delta_y = y - last_y;
            let distance = (delta_x * delta_x + delta_y * delta_y).sqrt();
            self.rotation += distance * 0.01;
            self.last_drag_pos = Some((x, y));
            self.update_position(x, y);
            self.queue_redraw();
        } else {
            // First move after drag started without position
            self.last_drag_pos = Some((x, y));
            self.update_position(x, y);
        }
    }

    fn end_drag(&mut self) {
        self.is_dragging = false;
        self.last_drag_pos = None;
    }

    fn update_position(&mut self, x: f32, y: f32) {
        if let Some(ref surface_state) = self.surface_state {
            let size = surface_state.window.inner_size();
            self.position_x = (x / size.width as f32) * 2.0 - 1.0;
            self.position_y = -((y / size.height as f32) * 2.0 - 1.0);
        }
    }
}

fn run(event_loop: EventLoop<()>) -> Result<(), EventLoopError> {
    info!("Running mainloop...");

    // doesn't need to be re-considered later
    let instance = Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or_default(),
        //backends: wgpu::Backends::VULKAN,
        //backends: wgpu::Backends::GL,
        flags: wgpu::InstanceFlags::from_env_or_default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::from_env_or_default(),
        display: None,
    });

    let mut app = App::new(instance);

    // It's not recommended to use `run` on Android because it will call
    // `std::process::exit` when finished which will short-circuit any
    // Java lifecycle handling
    #[allow(deprecated)]
    event_loop.run(move |event, event_loop| {
        info!("Received Winit event: {event:?}");

        event_loop.set_control_flow(ControlFlow::Wait);
        match event {
            Event::Resumed => {
                app.resume(event_loop);
            }
            Event::Suspended => {
                info!("Suspended, dropping render state...");
                app.surface_state = None;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_size),
                ..
            } => {
                app.configure_surface_swapchain();
                // Winit: doesn't currently implicitly request a redraw
                // for a resize which may be required on some platforms...
                app.queue_redraw();
            }
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                info!("Handling Redraw Request");

                let mut queue_surface_reconfigure = false;
                let mut queue_surface_recreate = false;

                if let Some(ref surface_state) = app.surface_state {
                    if let Some(ref rs) = app.render_state {
                        // Update uniform buffer with rotation and position
                        rs.queue.write_buffer(
                            &rs.uniform_buffer,
                            0,
                            bytemuck::cast_slice(&[
                                app.rotation,
                                app.position_x,
                                app.position_y,
                                0.0,
                            ]),
                        );

                        let frame_view = match surface_state.surface.get_current_texture() {
                            wgpu::CurrentSurfaceTexture::Success(surface_texture) => {
                                let view = surface_texture
                                    .texture
                                    .create_view(&wgpu::TextureViewDescriptor::default());
                                Some((surface_texture, view))
                            }
                            wgpu::CurrentSurfaceTexture::Suboptimal(surface_texture) => {
                                warn!(
                                    "Surface texture is suboptimal, it may not be displayed correctly"
                                );
                                let view = surface_texture
                                    .texture
                                    .create_view(&wgpu::TextureViewDescriptor::default());
                                queue_surface_reconfigure = true;
                                Some((surface_texture, view))
                            }
                            wgpu::CurrentSurfaceTexture::Timeout => {
                                warn!(
                                    "Timeout while acquiring next surface texture, skipping frame"
                                );
                                None
                            }
                            wgpu::CurrentSurfaceTexture::Occluded => None,
                            wgpu::CurrentSurfaceTexture::Outdated => {
                                queue_surface_reconfigure = true;
                                None
                            }
                            wgpu::CurrentSurfaceTexture::Lost => {
                                warn!("Surface lost, recreating surface and render state");
                                queue_surface_recreate = true;
                                None
                            }
                            wgpu::CurrentSurfaceTexture::Validation => {
                                warn!(
                                    "Validation error while acquiring next surface texture, skipping frame"
                                );
                                None
                            },
                        };

                        if let Some((frame, view)) = frame_view {
                            let mut encoder =
                                rs.device
                                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                        label: None,
                                    });
                            {
                                let mut rpass =
                                    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                        label: None,
                                        color_attachments: &[Some(
                                            wgpu::RenderPassColorAttachment {
                                                view: &view,
                                                resolve_target: None,
                                                ops: wgpu::Operations {
                                                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                                    store: wgpu::StoreOp::Store,
                                                },
                                                depth_slice: None,
                                            },
                                        )],
                                        depth_stencil_attachment: None,
                                        timestamp_writes: None,
                                        occlusion_query_set: None,
                                        multiview_mask: None,
                                    });
                                rpass.set_pipeline(&rs.render_pipeline);
                                rpass.set_bind_group(0, &rs.uniform_bind_group, &[]);
                                rpass.draw(0..3, 0..1);
                            }

                            rs.queue.submit(Some(encoder.finish()));
                            surface_state.window.pre_present_notify();
                            frame.present();
                        }
                        //surface_state.window.request_redraw();
                    }
                }

                if queue_surface_recreate {
                    let prev_texture_format = app.render_state.as_ref().map(|rs| rs.target_format);
                    app.surface_state = None;
                    app.create_surface(event_loop);
                    queue_surface_reconfigure = true;
                    if let Some(prev_texture_format) = prev_texture_format {
                        let swapchain_format = app.find_swapchain_texture_format().expect("Failed to find a supported swapchain format for our surface");

                       if prev_texture_format != swapchain_format {
                            info!("Surface lost and recreated with different format, updating render pipeline");
                            app.ensure_render_pipeline_for_target_format(swapchain_format);
                        }
                    }

                }
                if queue_surface_reconfigure {
                    app.configure_surface_swapchain();
                    app.queue_redraw();
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => event_loop.exit(),
            Event::WindowEvent {
                event: WindowEvent::Touch(touch),
                ..
            } => {
                use winit::event::TouchPhase;
                match touch.phase {
                    TouchPhase::Started => {
                        app.start_drag(touch.location.x as f32, touch.location.y as f32);
                    }
                    TouchPhase::Moved => {
                        if app.is_dragging {
                            app.update_drag(touch.location.x as f32, touch.location.y as f32);
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        app.end_drag();
                    }
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                if app.is_dragging {
                    println!("dragging, cursor moved to: {:?}", position);
                    app.update_drag(position.x as f32, position.y as f32);
                } else {
                    println!("not dragging, cursor moved to: {:?}", position);
                }
            }
            Event::WindowEvent {
                event: WindowEvent::MouseInput { state, button, .. },
                ..
            } => {
                use winit::event::{ElementState, MouseButton};
                if button == MouseButton::Left {
                    match state {
                        ElementState::Pressed => {
                            app.is_dragging = true;
                            // Position will be set on first CursorMoved event
                        }
                        ElementState::Released => {
                            app.end_drag();
                        }
                    }
                }
            }
            Event::WindowEvent { event: _, .. } => {
                info!("Window event {:#?}", event);
                if let Some(ref surface_state) = app.surface_state {
                    surface_state.window.request_redraw();
                }
            }
            _ => {}
        }
    })
}

fn _main(event_loop: EventLoop<()>) -> Result<(), EventLoopError> {
    run(event_loop)
}

const DEFAULT_ENV_FILTER: &str = "debug,wgpu_hal=info,winit=info,naga=info";

#[allow(dead_code)]
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
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

    let event_loop = EventLoop::builder().with_android_app(app).build().unwrap();
    if let Err(err) = _main(event_loop) {
        eprintln!("Error while running event loop: {err:?}");
    }
}

#[allow(dead_code)]
#[cfg(not(target_os = "android"))]
fn main() -> Result<(), EventLoopError> {
    if !std::option_env!("RUST_LOG").is_some() {
        std::env::set_var("RUST_LOG", DEFAULT_ENV_FILTER);
    }
    tracing_subscriber::fmt::init();

    let event_loop = EventLoop::builder()
        .build()
        .expect("Failed to create event loop");
    _main(event_loop)
}

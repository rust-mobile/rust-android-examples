#[cfg(target_os = "android")]
use std::ffi::c_void;
#[cfg(target_os = "android")]
use std::ptr::NonNull;
use std::{borrow::Cow, sync::Arc};

use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};
use tracing::{error, info, trace, warn};
use wgpu::{
    Adapter, Device, Instance, PipelineCompilationOptions, PipelineLayout, Queue, RenderPipeline,
    ShaderModule, TextureFormat,
};
use winit::{
    error::EventLoopError,
    event::{Event, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
};

#[derive(Debug)]
pub enum AppEvent {}

struct DeviceState {
    adapter: Adapter,
    device: Device,
    queue: Queue,
}

struct RenderState {
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
    needs_reconfigure: bool,
}

struct AppInner {
    instance: Instance,
    _event_loop_proxy: EventLoopProxy<AppEvent>,
    #[cfg(target_arch = "wasm32")]
    web_canvas_id: &'static str,
    is_running: bool,
    /// Set while we're asynchronously looking for an adaptor that's compatible with our surface
    /// and connecting to a corresponding device + queue.
    connecting_device: bool,
    surface_state: Option<SurfaceState>,
    device_state: Option<DeviceState>,
    render_state: Option<RenderState>,
    rotation: f32,
    position_x: f32,
    position_y: f32,
    last_drag_pos: Option<(f32, f32)>,
    is_dragging: bool,
}

// We need a Send closure for `set_device_lost_callback` but
// `wgpu::backend::webgpu::WebDevice` is not `Send` even though we can assume we
// have a single-threaded environment.
#[cfg(target_arch = "wasm32")]
unsafe impl Send for AppInner {}

pub struct App {
    inner: Arc<std::sync::Mutex<AppInner>>,
}

impl App {
    #[allow(unused)]
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self::new_with_canvas_id(proxy, "canvas")
    }

    pub fn new_with_canvas_id(proxy: EventLoopProxy<AppEvent>, _canvas: &'static str) -> Self {
        let instance = Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or_default(),
            //backends: wgpu::Backends::VULKAN,
            //backends: wgpu::Backends::GL,
            flags: wgpu::InstanceFlags::from_env_or_default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::from_env_or_default(),
            display: None,
        });

        Self {
            inner: Arc::new(std::sync::Mutex::new(AppInner {
                instance,
                _event_loop_proxy: proxy,
                #[cfg(target_arch = "wasm32")]
                web_canvas_id: _canvas,
                is_running: false,
                connecting_device: false,
                surface_state: None,
                device_state: None,
                render_state: None,
                rotation: 0.0,
                position_x: 0.0,
                position_y: 0.0,
                last_drag_pos: None,
                is_dragging: false,
            })),
        }
    }
}

impl AppInner {
    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Arc<winit::window::Window> {
        info!("Creating Winit Window");
        let mut window_attributes = winit::window::WindowAttributes::default();

        #[cfg(not(target_arch = "wasm32"))]
        {
            window_attributes = window_attributes.with_title("WebGPU example");
        }

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::{JsCast, UnwrapThrowExt as _};
            use web_sys::HtmlCanvasElement;
            use winit::platform::web::WindowAttributesExtWebSys;

            let window = wgpu::web_sys::window().unwrap_throw();
            let document = window.document().unwrap_throw();
            let canvas = document
                .get_element_by_id(self.web_canvas_id)
                .unwrap_throw();
            let html_canvas_element: HtmlCanvasElement = canvas.unchecked_into();
            // Make sure the canvas can be given focus.
            // https://developer.mozilla.org/en-US/docs/Web/HTML/Global_attributes/tabindex
            html_canvas_element.set_tab_index(0);

            // Don't outline the canvas when it has focus:
            html_canvas_element
                .style()
                .set_property("outline", "none")
                .unwrap();

            window_attributes = window_attributes.with_canvas(Some(html_canvas_element));
        }

        let window = event_loop.create_window(window_attributes).unwrap();
        Arc::new(window)
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

        let surface_state = match self.instance.create_surface(window_handle) {
            Ok(surface) => Some(SurfaceState {
                window,
                surface,
                needs_reconfigure: true,
            }),
            Err(err) => {
                tracing::error!("Failed to create surface: {err}");
                None
            }
        };
        self.surface_state = surface_state;
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

    fn find_swapchain_texture_format(&self) -> Option<TextureFormat> {
        if let (Some(surface_state), Some(device_state)) = (&self.surface_state, &self.device_state)
        {
            info!("WGPU: finding supported swapchain format");
            let surface_caps = surface_state
                .surface
                .get_capabilities(&device_state.adapter);
            for format in surface_caps.formats.iter() {
                info!("WGPU:   supported format: {format:?}");
            }
            Some(surface_caps.formats[0])
        } else {
            None
        }
    }

    fn ensure_render_pipeline_for_target_format(&mut self, target_format: TextureFormat) -> bool {
        let Some(device_state) = self.device_state.as_ref() else {
            error!("Can't update render pipeline because device state is not initialized yet");
            return false;
        };

        if let Some(render_state) = &self.render_state {
            if render_state.target_format == target_format {
                return false;
            }
        }
        if self.render_state.is_none() {
            error!("Can't update render pipeline because render state is not initialized yet");
            return false;
        }

        let RenderState {
            shader,
            target_format,
            render_pipeline: _,
            pipeline_layout,
            uniform_buffer,
            uniform_bind_group,
        } = self.render_state.take().unwrap();

        let render_pipeline = Self::create_render_pipeline(
            &device_state.device,
            &pipeline_layout,
            &shader,
            target_format,
        );

        self.render_state = Some(RenderState {
            shader,
            target_format,
            pipeline_layout,
            render_pipeline,
            uniform_buffer,
            uniform_bind_group,
        });
        true
    }

    fn configure_surface_swapchain(&mut self) {
        if let (Some(device_state), Some(surface_state)) =
            (&self.device_state, &mut self.surface_state)
        {
            let size = surface_state.window.inner_size();

            let config = surface_state
                .surface
                .get_default_config(
                    &device_state.adapter,
                    u32::max(size.width, 1),
                    u32::max(size.height, 1),
                )
                .expect("Window surface can't be rendered to by adapter");

            let swapchain_format = config.format;
            info!("WGPU: Configuring surface swapchain: format = {swapchain_format:?}, size = {size:?}");
            surface_state
                .surface
                .configure(&device_state.device, &config);
            surface_state.needs_reconfigure = false;
        }
    }

    fn init_render_state(&mut self, target_format: TextureFormat) -> RenderState {
        info!("Initializing render state for target format: {target_format:?}");

        let DeviceState { device, .. } = self
            .device_state
            .as_ref()
            .expect("Device should be initialized");

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
            Self::create_render_pipeline(device, &pipeline_layout, &shader, target_format);

        RenderState {
            shader,
            target_format,
            pipeline_layout,
            render_pipeline,
            uniform_buffer,
            uniform_bind_group,
        }
    }

    fn ensure_render_state_for_surface(&mut self) {
        let surface_format = self.surface_state.as_ref().and_then(|surface_state| {
            surface_state
                .surface
                .get_configuration()
                .map(|config| config.format)
        });

        if let Some(swapchain_format) = surface_format {
            info!("Ensuring render state is initialized for surface swapchain format: {swapchain_format:?}");
            if self.render_state.is_none() {
                let rs = self.init_render_state(swapchain_format);
                self.render_state = Some(rs);
                self.queue_redraw();
            } else {
                info!("Render state already initialized, checking if it needs to be updated for new surface swapchain format");
                if self.ensure_render_pipeline_for_target_format(swapchain_format) {
                    self.queue_redraw();
                }
            }
        } else {
            warn!("Can't ensure render state for surface because surface format is not available");
        }
    }

    fn queue_redraw(&self) {
        if let Some(surface_state) = &self.surface_state {
            trace!("Making Redraw Request");
            surface_state.window.request_redraw();
        }
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

    fn render(&mut self, event_loop: &ActiveEventLoop) {
        let mut queue_surface_reconfigure = false;
        let mut queue_surface_recreate = false;

        if self.device_state.is_none() {
            if self.connecting_device {
                info!("Still connecting to device, skipping render");
            } else {
                error!("Can't render because device state has been lost");
            }
            return;
        }

        if self
            .surface_state
            .as_ref()
            .is_some_and(|surface_state| surface_state.needs_reconfigure)
        {
            info!("Surface needs reconfigure, reconfiguring now...");
            self.configure_surface_swapchain();
        } else {
            info!("Surface doesn't need reconfigure, skipping");
        }

        self.ensure_render_state_for_surface();

        let Some(device_state) = self.device_state.as_ref() else {
            return;
        };

        let Some(surface_state) = self.surface_state.as_ref() else {
            error!("Can't render because surface state has been lost");
            return;
        };

        let Some(rs) = self.render_state.as_ref() else {
            error!("Can't render because render state has been lost");
            return;
        };

        // Update uniform buffer with rotation and position
        device_state.queue.write_buffer(
            &rs.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.rotation, self.position_x, self.position_y, 0.0]),
        );

        let frame_view = match surface_state.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(surface_texture) => {
                let view = surface_texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                Some((surface_texture, view))
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(surface_texture) => {
                warn!("Surface texture is suboptimal, it may not be displayed correctly");
                let view = surface_texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                queue_surface_reconfigure = true;
                Some((surface_texture, view))
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                warn!("Timeout while acquiring next surface texture, skipping frame");
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
                warn!("Validation error while acquiring next surface texture, skipping frame");
                None
            }
        };

        if let Some((frame, view)) = frame_view {
            let mut encoder = device_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                rpass.set_pipeline(&rs.render_pipeline);
                rpass.set_bind_group(0, &rs.uniform_bind_group, &[]);
                rpass.draw(0..3, 0..1);
            }

            device_state.queue.submit(Some(encoder.finish()));
            surface_state.window.pre_present_notify();
            frame.present();
        }
        //surface_state.window.request_redraw();

        if queue_surface_recreate {
            let prev_texture_format = self.render_state.as_ref().map(|rs| rs.target_format);
            self.surface_state = None;
            self.create_surface(event_loop);
            queue_surface_reconfigure = true;
            if let Some(prev_texture_format) = prev_texture_format {
                let swapchain_format = self
                    .find_swapchain_texture_format()
                    .expect("Failed to find a supported swapchain format for our surface");

                if prev_texture_format != swapchain_format {
                    info!("Surface lost and recreated with different format, updating render pipeline");
                    self.ensure_render_pipeline_for_target_format(swapchain_format);
                }
            }
        }
        if queue_surface_reconfigure {
            self.configure_surface_swapchain();
            self.queue_redraw();
        }
    }
}

impl App {
    /// Once we have a surface we can find an adapter that is compatible with it
    /// and connect to a device + create a corresponding render queue
    ///
    /// Note: the lifetime requirements for passing `compatible_surface:
    /// Option<&Surface<'window_handle>>` to `request_adapter` are quite painful
    /// to work with.
    ///
    /// We follow the common pattern of ensuring `'window_handle` is `'static`.
    ///
    /// Since the `Surface` is not a cheaply-cloneable handle we also give this
    /// function temporary ownership of the `Surface` so it can be borrowed for
    /// the duration of the async future that calls `request_adapter`.
    ///
    /// Since this function temporarily owns the `Surface` that means it's also
    /// responsible for filling out the `SurfaceState` for the `App` once it is
    /// finished.
    ///
    async fn ensure_adapter_and_connected_device_for_surface(
        app_inner: Arc<std::sync::Mutex<AppInner>>,
    ) {
        let (instance, window, surface) = {
            let mut app = app_inner.lock().unwrap();

            if let Some(device_state) = &app.device_state {
                if let Some(surface_state) = &app.surface_state {
                    if device_state
                        .adapter
                        .is_surface_supported(&surface_state.surface)
                    {
                        info!("Already have a compatible connected device");
                        return;
                    }
                }
            }

            // In this case we implicitly know that any pre-existing device or render state
            // is not compatible with any previous adapter
            if app.device_state.is_some() {
                info!("Dropping existing device / queue since we need to find a new compatible adapter");
            }
            app.device_state = None;
            if app.render_state.is_some() {
                info!(
                    "Dropping existing render state since we need to find a new compatible adapter"
                );
            }
            app.render_state = None;

            // In case we need to find a compatible adapter we need temporary ownership
            // of the Surface so it can be passed to the async `request_adapter` call.
            let Some(SurfaceState {
                window, surface, ..
            }) = app.surface_state.take()
            else {
                error!("Surface should have been created before requesting adapter");
                return;
            };
            (app.instance.clone(), window, surface)
        };

        info!("WGPU: finding a suitable adapter (compatible with our surface)");
        let adapter_result = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                // Request an adapter which can render to our surface
                compatible_surface: Some(&surface),
            })
            .await;

        {
            let mut app = app_inner.lock().unwrap();

            // Unconditionally put the borrowed surface back before we can return
            app.surface_state = Some(SurfaceState {
                window,
                surface,
                needs_reconfigure: true,
            });
        }

        let adapter = match adapter_result {
            Ok(adapter) => adapter,
            Err(err) => {
                error!("Failed to find an appropriate adapter: {err}");
                return;
            }
        };

        // If the required adapter matches any existing adapter then we're done
        // If the adapter has changed (or not previously set) then we need to create a new device + queue and update render state

        info!("WGPU: requesting device");
        // Create the logical device and command queue
        let res = adapter
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
            .await;
        let (device, queue) = match res {
            Ok((device, queue)) => (device, queue),
            Err(err) => {
                error!("Failed to connect to device: {err}");
                return;
            }
        };

        let weak_inner = Arc::downgrade(&app_inner);
        device.set_device_lost_callback(move |reason, message| {
            error!("Device lost: {:?}, {}", reason, message);
            if let Some(app_inner) = weak_inner.upgrade() {
                let mut app = app_inner.lock().unwrap();
                app.device_state = None;
                app.render_state = None;
            }
        });

        let mut app = app_inner.lock().unwrap();
        app.device_state = Some(DeviceState {
            adapter,
            device,
            queue,
        });
    }

    fn ensure_device_state_for_surface(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        pollster::block_on(App::ensure_adapter_and_connected_device_for_surface(
            Arc::clone(&self.inner),
        ));
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(App::find_adapter_and_connect_device_for_surface(
            Arc::clone(&self.inner),
        ));
    }

    fn resume(&self, event_loop: &ActiveEventLoop) {
        info!("Resumed, creating render surface...");

        let have_surface = {
            let mut app = self.inner.lock().unwrap();
            app.is_running = true;

            app.create_surface(event_loop);
            app.surface_state.is_some()
        };

        if have_surface {
            self.ensure_device_state_for_surface();
        }
    }

    pub fn handle_winit_event(&self, event: Event<AppEvent>, event_loop: &ActiveEventLoop) {
        info!("Received Winit event: {event:?}");

        event_loop.set_control_flow(ControlFlow::Wait);
        match event {
            Event::Resumed => {
                self.resume(event_loop);
            }
            Event::Suspended => {
                info!("Suspended, dropping surface state...");
                let mut app = self.inner.lock().unwrap();
                app.is_running = false;
                app.surface_state = None;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_size),
                ..
            } => {
                let mut app = self.inner.lock().unwrap();
                if let Some(surface_state) = app.surface_state.as_mut() {
                    info!("Window resized, marking surface as needing reconfigure");
                    surface_state.needs_reconfigure = true;
                }

                // Winit: doesn't currently implicitly request a redraw
                // for a resize which may be required on some platforms...
                app.queue_redraw();
            }
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                info!("Handling Redraw Request");

                let try_init_device = {
                    let inner = self.inner.lock().unwrap();
                    if inner.device_state.is_none()
                        && inner.surface_state.is_some()
                        && !inner.connecting_device
                    {
                        warn!("Device state is not initialized but we have a surface and we're not currently connecting to a device, trying to ensure device state is initialized...");
                        true
                    } else {
                        false
                    }
                };

                if try_init_device {
                    self.ensure_device_state_for_surface();
                }

                let mut inner = self.inner.lock().unwrap();
                inner.render(event_loop);
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
                let mut app = self.inner.lock().unwrap();

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
                let mut app = self.inner.lock().unwrap();
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
                let mut app = self.inner.lock().unwrap();
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
                let app = self.inner.lock().unwrap();
                if let Some(ref surface_state) = app.surface_state {
                    info!("Requesting redraw from window event handler");
                    surface_state.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

#[allow(unused)]
pub const DEFAULT_ENV_FILTER: &str =
    "debug,wgpu_hal=info,winit=info,naga=info,android-activity=trace";

#[allow(unused)]
pub fn run(app: App, event_loop: EventLoop<AppEvent>) -> Result<(), EventLoopError> {
    info!("Running mainloop...");

    #[allow(deprecated)]
    event_loop.run(move |event, event_loop| {
        //info!("Received Winit event: {event:?}");
        app.handle_winit_event(event, event_loop);
    })
}

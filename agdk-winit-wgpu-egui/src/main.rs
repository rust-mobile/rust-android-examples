#[cfg(not(target_os = "android"))]
mod app;

#[allow(dead_code)]
#[cfg(target_arch = "wasm32")]
pub fn main() {
    use app::App;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use winit::event_loop::EventLoop;
    use winit::platform::web::EventLoopExtWebSys;

    console_error_panic_hook::set_once();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .without_time()
                .with_writer(tracing_web::MakeWebConsoleWriter::new()),
        )
        .init();

    tracing::warn!("Starting application");

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        const CANVAS_ID: &str = "the_canvas_id";

        let event_loop = EventLoop::with_user_event().build().unwrap();

        let proxy = event_loop.create_proxy();
        let app = App::new_with_canvas_id(proxy, CANVAS_ID);

        #[allow(deprecated)]
        event_loop.spawn(move |event, event_loop| app.handle_winit_event(event, event_loop));

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            loading_text.remove();
        }
    });
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
use winit::error::EventLoopError;

#[allow(dead_code)]
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn main() -> Result<(), EventLoopError> {
    use winit::event_loop::EventLoop;

    use app::App;

    if std::option_env!("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", app::DEFAULT_ENV_FILTER);
    }
    tracing_subscriber::fmt::init();

    let event_loop = EventLoop::with_user_event()
        .build()
        .expect("Failed to create event loop");

    let proxy = event_loop.create_proxy();
    let app = App::new(proxy);

    app::run(app, event_loop)
}

#[cfg(target_os = "android")]
fn main() {}

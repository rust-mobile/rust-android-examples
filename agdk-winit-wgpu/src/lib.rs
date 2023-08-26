#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[cfg(target_os = "android")]
mod app;

#[allow(dead_code)]
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    use std::sync::OnceLock;

    use app::App;
    use tracing::error;
    use winit::{event_loop::EventLoop, platform::android::EventLoopBuilderExtAndroid};

    std::env::set_var("RUST_BACKTRACE", "full");
    std::env::set_var("WGPU_BACKEND", "vulkan");

    // NB: android_main can be called multiple times if the application Activity
    // is destroyed and recreated so we use a OnceLock to ensure that we only
    // initialize our global state once (otherwise tracing_subscriber will panic
    // if we try to initialize it multiple times).
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        use tracing_subscriber::prelude::*;

        let filter_layer = tracing_subscriber::EnvFilter::new(app::DEFAULT_ENV_FILTER);
        let android_layer = paranoid_android::layer(env!("CARGO_PKG_NAME"))
            .with_ansi(false)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
            .with_thread_names(true);
        tracing_subscriber::registry()
            .with(filter_layer)
            .with(android_layer)
            .init();
    });

    let event_loop = EventLoop::with_user_event()
        .with_android_app(app)
        .build()
        .unwrap();

    let proxy = event_loop.create_proxy();
    let app = App::new(proxy);

    if let Err(err) = app::run(app, event_loop) {
        error!("Error while running event loop: {err:?}");
    }
}

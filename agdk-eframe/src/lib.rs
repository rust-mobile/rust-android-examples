use eframe::egui;
use eframe::{NativeOptions, Renderer};
use tracing::{error, info};

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

#[derive(Default)]
struct DemoApp {
    demo_windows: egui_demo_lib::DemoWindows,
}

impl eframe::App for DemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.demo_windows.ui(ctx);
    }
}

fn _main(mut options: NativeOptions) -> eframe::Result<()> {
    options.renderer = Renderer::Wgpu;
    eframe::run_native(
        "My egui App",
        options,
        Box::new(|_cc| Ok(Box::<DemoApp>::default())),
    )
}

const DEFAULT_ENV_FILTER: &str = "debug,wgpu_hal=info,winit=info,naga=info";

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: AndroidApp) {
    use std::sync::OnceLock;

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

    eprintln!("agdk-eframe demo started");
    info!("agdk-eframe demo started");

    let options = NativeOptions {
        android_app: Some(app.clone()),
        ..Default::default()
    };

    _main(options).unwrap_or_else(|err| {
        error!("Failure while running EFrame application: {err:?}");
    });
}

#[cfg(not(target_os = "android"))]
fn main() {
    if !std::option_env!("RUST_LOG").is_some() {
        std::env::set_var("RUST_LOG", DEFAULT_ENV_FILTER);
    }
    tracing_subscriber::fmt::init();

    _main(NativeOptions::default());
}

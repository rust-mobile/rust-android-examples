use android_activity::{
    ndk::{hardware_buffer_format::HardwareBufferFormat, native_window::NativeWindow},
    AndroidApp, InputStatus, MainEvent, PollEvent,
};
use log::info;

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_min_level(log::Level::Info));

    std::env::set_var("RUST_BACKTRACE", "full");

    let mut quit = false;
    let mut redraw_pending = true;
    let mut native_window = None;

    while !quit {
        app.poll_events(
            Some(std::time::Duration::from_secs(1)), /* timeout */
            |event| {
                match event {
                    PollEvent::Wake => {
                        info!("Early wake up");
                    }
                    PollEvent::Timeout => {
                        //info!("Timed out");
                        // Real app would probably rely on vblank sync via graphics API...
                        redraw_pending = true;
                    }
                    PollEvent::Main(main_event) => {
                        info!("Main event: {:?}", main_event);
                        match main_event {
                            MainEvent::SaveState { saver, .. } => {
                                saver.store("foo://bar".as_bytes());
                            }
                            MainEvent::Pause => {}
                            MainEvent::Resume { loader, .. } => {
                                if let Some(state) = loader.load() {
                                    if let Ok(uri) = String::from_utf8(state) {
                                        info!("Resumed with saved state = {uri:#?}");
                                    }
                                }
                            }
                            MainEvent::InitWindow { .. } => {
                                native_window = app.native_window();
                                if let Some(nw) = &native_window {
                                    // Set the backing buffer to a known format (without changing
                                    // the size) so that we can safely draw to it in dummy_render().
                                    nw.set_buffers_geometry(
                                        0,
                                        0,
                                        Some(HardwareBufferFormat::R8G8B8A8_UNORM),
                                    )
                                    .unwrap()
                                }
                                redraw_pending = true;
                            }
                            MainEvent::TerminateWindow { .. } => {
                                native_window = None;
                            }
                            MainEvent::WindowResized { .. } => {
                                redraw_pending = true;
                            }
                            MainEvent::RedrawNeeded { .. } => {
                                redraw_pending = true;
                            }
                            MainEvent::InputAvailable { .. } => {
                                redraw_pending = true;
                            }
                            MainEvent::ConfigChanged { .. } => {
                                info!("Config Changed: {:#?}", app.config());
                            }
                            MainEvent::LowMemory => {}

                            MainEvent::Destroy => quit = true,
                            _ => { /* ... */ }
                        }
                    }
                    _ => {}
                }

                if redraw_pending {
                    if let Some(native_window) = &native_window {
                        redraw_pending = false;

                        // Handle input, via a lending iterator
                        match app.input_events_iter() {
                            Ok(mut iter) => loop {
                                if !iter.next(|event| {
                                    info!("Input Event: {event:?}");
                                    InputStatus::Unhandled
                                }) {
                                    break;
                                }
                            },
                            Err(err) => {
                                log::error!("Failed to get input events iterator: {err:?}");
                            }
                        }

                        dummy_render(native_window);
                    }
                }
            },
        );
    }
}

/// Post a NOP frame to the window
///
/// Since this is a bare minimum test app we don't depend
/// on any GPU graphics APIs but we do need to at least
/// convince Android that we're drawing something and are
/// responsive, otherwise it will stop delivering input
/// events to us.
fn dummy_render(native_window: &NativeWindow) {
    let mut lock = native_window.lock(None).unwrap();
    let (w, h) = (lock.width(), lock.height());

    assert_eq!(
        lock.format(),
        HardwareBufferFormat::R8G8B8A8_UNORM,
        "Expected the buffer format to be R8G8B8A8_UNORM since we set that in `InitWindow` handling"
    );

    for (y, line) in lock.lines().unwrap().enumerate() {
        let r = y * 255 / h;
        for (x, pixels) in line.chunks_mut(4).enumerate() {
            let g = x * 255 / w;
            pixels[0].write(r as u8);
            pixels[1].write(g as u8);
            pixels[2].write(0);
            pixels[3].write(255);
        }
    }
}

#[jni::jni_mangle("com.github.rust_mobile.nasubclassjni.MainActivity")]
pub fn notify_on_new_intent<'local>(
    mut unowned_env: jni::EnvUnowned<'local>,
    _class: jni::objects::JClass<'local>,
    message: jni::objects::JString<'local>,
    _activity: jni::objects::JObject<'local>,
) {
    info!("onNewIntent was called!");

    unowned_env
        .with_env(|_env| -> jni::errors::Result<()> {
            info!("onNewIntent message: {message}");
            Ok(())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

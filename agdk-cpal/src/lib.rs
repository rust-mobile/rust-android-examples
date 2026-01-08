///! Based on https://github.com/RustAudio/cpal/blob/master/examples/android.rs
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use std::sync::OnceLock;
use tracing::{error, info};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SizedSample,
};
use cpal::{FromSample, Sample};

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: Sample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels) {
        let value: T = T::from_sample(next_sample());
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}

fn make_audio_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
) -> Result<cpal::Stream, anyhow::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate as f32;
    let channels = config.channels as usize;

    // Produce a sinusoid of maximum amplitude.
    let mut sample_clock = 0f32;
    let mut next_value = move || {
        sample_clock = (sample_clock + 1.0) % sample_rate;
        (sample_clock * 440.0 * 2.0 * std::f32::consts::PI / sample_rate).sin()
    };

    let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            write_data(data, channels, &mut next_value)
        },
        err_fn,
        None,
    )?;

    Ok(stream)
}

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        use tracing_subscriber::prelude::*;

        unsafe { std::env::set_var("RUST_BACKTRACE", "full") };

        const DEFAULT_ENV_FILTER: &str = "debug,wgpu_hal=info,winit=info,naga=info";
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

    let mut quit = false;
    let mut redraw_pending = true;
    let mut native_window: Option<ndk::native_window::NativeWindow> = None;

    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .expect("failed to find output device");

    let config = device.default_output_config().unwrap();

    let stream = match config.sample_format() {
        cpal::SampleFormat::I8 => make_audio_stream::<i8>(&device, &config.into()).unwrap(),
        cpal::SampleFormat::I16 => make_audio_stream::<i16>(&device, &config.into()).unwrap(),
        // cpal::SampleFormat::I24 => run::<I24>(&device, &config.into()).unwrap(),
        cpal::SampleFormat::I32 => make_audio_stream::<i32>(&device, &config.into()).unwrap(),
        // cpal::SampleFormat::I48 => run::<I48>(&device, &config.into()).unwrap(),
        cpal::SampleFormat::I64 => make_audio_stream::<i64>(&device, &config.into()).unwrap(),
        cpal::SampleFormat::U8 => make_audio_stream::<u8>(&device, &config.into()).unwrap(),
        cpal::SampleFormat::U16 => make_audio_stream::<u16>(&device, &config.into()).unwrap(),
        // cpal::SampleFormat::U24 => run::<U24>(&device, &config.into()).unwrap(),
        cpal::SampleFormat::U32 => make_audio_stream::<u32>(&device, &config.into()).unwrap(),
        // cpal::SampleFormat::U48 => run::<U48>(&device, &config.into()).unwrap(),
        cpal::SampleFormat::U64 => make_audio_stream::<u64>(&device, &config.into()).unwrap(),
        cpal::SampleFormat::F32 => make_audio_stream::<f32>(&device, &config.into()).unwrap(),
        cpal::SampleFormat::F64 => make_audio_stream::<f64>(&device, &config.into()).unwrap(),
        sample_format => panic!("Unsupported sample format '{sample_format}'"),
    };

    while !quit {
        app.poll_events(
            Some(std::time::Duration::from_millis(500)), /* timeout */
            |event| {
                match event {
                    PollEvent::Wake => {
                        info!("Early wake up");
                    }
                    PollEvent::Timeout => {
                        info!("Timed out");
                        // Real app would probably rely on vblank sync via graphics API...
                        redraw_pending = true;
                    }
                    PollEvent::Main(main_event) => {
                        info!("Main event: {:?}", main_event);
                        match main_event {
                            MainEvent::SaveState { saver, .. } => {
                                saver.store("foo://bar".as_bytes());
                            }
                            MainEvent::Pause => {
                                if let Err(err) = stream.pause() {
                                    error!("Failed to pause audio playback: {err}");
                                }
                            }
                            MainEvent::Resume { loader, .. } => {
                                if let Some(state) = loader.load() {
                                    if let Ok(uri) = String::from_utf8(state) {
                                        info!("Resumed with saved state = {uri:#?}");
                                    }
                                }

                                if let Err(err) = stream.play() {
                                    error!("Failed to start audio playback: {err}");
                                }
                            }
                            MainEvent::InitWindow { .. } => {
                                native_window = app.native_window();
                                redraw_pending = true;
                            }
                            MainEvent::TerminateWindow { .. } => {
                                native_window = None;
                                redraw_pending = false;
                            }
                            MainEvent::WindowResized { .. } => {
                                redraw_pending = true;
                            }
                            MainEvent::RedrawNeeded { .. } => {
                                redraw_pending = true;
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
                                info!("Checking for next input event...");
                                iter.next(|event| {
                                    info!("Input Event: {event:?}");
                                    InputStatus::Unhandled
                                });
                            },
                            Err(err) => error!("Failed to get input events iterator: {err}"),
                        }

                        info!("Render...");
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
fn dummy_render(native_window: &ndk::native_window::NativeWindow) {
    unsafe {
        let mut buf: ndk_sys::ANativeWindow_Buffer = std::mem::zeroed();
        let mut rect: ndk_sys::ARect = std::mem::zeroed();
        ndk_sys::ANativeWindow_lock(
            native_window.ptr().as_ptr() as _,
            &mut buf as _,
            &mut rect as _,
        );
        // Note: we don't try and touch the buffer since that
        // also requires us to handle various buffer formats
        ndk_sys::ANativeWindow_unlockAndPost(native_window.ptr().as_ptr() as _);
    }
}

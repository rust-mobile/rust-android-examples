This tests using `GameActivity` with winit, wgpu and egui

This example also aims to show how it's possible to use Winit + WGPU to write
portable code that can run on Android, desktop platforms (enable "desktop"
feature) and Web (enable "web" feature)

Egui is then used to show a demo UI that overlays the wgpu rendering.

The aim with this example is to show how Egui can be integrated with an existing
winit + wgpu app (as opposed to making egui-wgpu be fully responsible for the
rendering state).

# Android Build

```bash
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build
./gradlew build
./gradlew installDebug
adb shell am start -n com.github.rust_mobile.agdkwinitwgpuegui/.MainActivity
```

# Desktop Build

```bash
cargo run --features desktop
```

# WebGL Build

```bash
cargo install --locked trunk
trunk serve --features web --open
```

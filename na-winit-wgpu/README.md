This is the same as agdk-winit-wgpu except it runs with `NativeActivity`
instead of `GameActivity`

Note: you may need to use compile time variable WGPU_BACKEND=GL for older devices until autodetection is fixed:
https://github.com/gfx-rs/wgpu/issues/2384

# Gradle Build

```bash
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build
./gradlew build
./gradlew installDebug
```

# Cargo APK Build (deprecated)

```bash
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_SDK_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-apk

cargo apk run
```

# Cargo xbuild Build (supersedes cargo apk, supports iOS)

https://github.com/rust-mobile/xbuild

```
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_SDK_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install xbuild

x run [--release] --device adb:[adb device id]
```

Or:

```
[WGPU_BACKEND=GL] x build [--release] --format apk --platform android --arch arm64

adb install -r target/x/release/android/na-winit-wgpu.apk
adb shell am start -n com.example.na_winit_wgpu/android.app.NativeActivity
```

## Access Android devices in WSL:

From powershell:

```
adb shell ip -f inet addr show wlan0
adb tcpip 5555
```

Accept debugging access on the device, then in WSL:

```
adb connect <device ip from previous command>:5555
adb devices
```

# Windows build (also from WSL)

```
rustup target add x86_64-pc-windows-gnu

cargo build --target x86_64-pc-windows-gnu --release --features="desktop" --bin test-winit-wgpu

powershell.exe Start target/x86_64-pc-windows-gnu/release/test-winit-wgpu.exe
```

# WASM build

Optionally install wasm-server-runner if you want cargo run to serve the canvas.
https://github.com/jakobhellermann/wasm-server-runner

```
RUSTFLAGS=--cfg=web_sys_unstable_apis cargo run --target wasm32-unknown-unknown [--release] --features="desktop"
```

Another suggestion if you work with javascript is to use Vite with a WASM hot reload plugin.

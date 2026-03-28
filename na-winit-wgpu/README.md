This is the same as agdk-winit-wgpu except it runs with `NativeActivity`
instead of `GameActivity`

# Android Build

```bash
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build
./gradlew build
./gradlew installDebug
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

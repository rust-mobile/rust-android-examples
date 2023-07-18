This is a minimal test application based on `NativeActivity` that just
runs a mainloop based on android_activity::poll_events() and traces
the events received without doing any rendering. It also saves and
restores some minimal application state.

Since this test doesn't require a custom `Activity` subclass it's
optionally possible to build this example with `xbuild` or `cargo apk`.

# xbuild

```bash
cargo install xbuild

# Get ID of connected debug device, of the form adb:DEVICEID
x devices

# Build and run for that configuration
x run --device adb:DEVICEID

# Or build for a generic arm64 Android device
x build --platform android --arch arm64
```

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

# Cargo APK Build

```bash
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_SDK_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-apk

cargo apk build
cargo apk run
```

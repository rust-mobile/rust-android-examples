This is a minimal example that shows how to draw a triangle using GL[ES] via the
Glutin crate.

This example demonstrates how to re-create the application's surface state as
necessary when the application is paused and resumed on Android.

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

cargo apk run
```

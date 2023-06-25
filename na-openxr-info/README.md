This is a minimal OpenXR application that builds for desktop or
Android and simply prints out extension information for the OpenXR
library.

This is based on the [hello](https://github.com/Ralith/openxrs/blob/master/openxr/examples/hello.rs)
example from the [openxrs](https://github.com/Ralith/openxrs) repo.

# Oculus Quest

To build for the Oculus Quest then you first need to download
the Oculus OpenXR Mobile SDK from:
https://developer.oculus.com/downloads/package/oculus-openxr-mobile-sdk/

and after unpacking the zip file you need to copy a suitable `libopenxr_loader.so`
library to `app/src/main/jniLibs/<abi>`

For example if building for arm64-v8a:

```bash
mkdir app/src/main/jniLibs/arm64-v8a
cp ovr_openxr_mobile_sdk_53.0/OpenXR/Libs/Android/arm64-v8a/Debug/libopenxr_loader.so app/src/main/jniLibs/arm64-v8a/libopenxr_loader.so
```

```bash
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/ build --features=android
./gradlew build
./gradlew installDebug
```

# Oculus Quest: Vulkan Validation Layer

To enable the Vulkan validation layer on the Oculus Quest run:

```bash
adb shell setprop debug.oculus.loadandinjectpackagedvvl.co.realfit.naopenxrwgpu 1
```

# Desktop

To build for PC you need to build with the "desktop" feature

`cargo run --features=desktop`
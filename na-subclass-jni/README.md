This is a minimal test application similar to `na-mainloop` that
demonstrates running with a subclass of `NativeActivity` and overriding
the `onNewIntent` `Activity` method and notifying rust whenever it's called.

Note: unlike the `na-mainloop` example, this one can't be built via
`cargo apk` since it needs to compile some Java code.

# Gradle Build

```bash
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_HOME="path/to/sdk"

rustup target add aarch64-linux-android
cargo install cargo-ndk

cargo ndk -t arm64-v8a -o app/src/main/jniLibs/  build
./gradlew build
./gradlew installDebug
adb shell am start -n com.github.rust_mobile.nasubclassjni/.MainActivity
```

Since this example configured the MainActivity with
`android:launchMode="singleTop"` in the `AndroidManifest.xml`, you can test the
`onNewIntent` override by re-running:

```
adb shell am start -n com.github.rust_mobile.nasubclassjni/.MainActivity
```

You should see the log message from Rust indicating that `onNewIntent` was called.
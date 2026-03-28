package com.github.rust_mobile.nasubclassjni;

import android.app.NativeActivity;
import android.os.Bundle;
import android.util.Log;
import android.content.Intent;

public class MainActivity extends NativeActivity {

    private static final String TAG = "MainActivity";
    static {
        System.loadLibrary("na_subclass_jni");
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        Log.i(TAG, "onCreate");
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        Log.i(TAG, "onNewIntent: " + intent.toString());
        notifyOnNewIntent(intent.toString());
    }

    private static native void notifyOnNewIntent(String message);
}

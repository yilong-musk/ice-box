package com.yilongmusk.icebox

import android.os.Bundle
import android.util.Log
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Wry loads the Rust library inside super.onCreate. Init the platform
    // verifier after that so subscription HTTPS can use the OS trust store.
    runCatching { Native.initVerifier(applicationContext) }
      .onSuccess { Log.i(TAG, it) }
      .onFailure { Log.e(TAG, "verifier init failed", it) }
  }

  companion object {
    private const val TAG = "IceBox"
  }
}

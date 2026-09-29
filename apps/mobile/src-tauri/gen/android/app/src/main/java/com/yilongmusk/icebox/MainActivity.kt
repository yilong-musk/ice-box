package com.yilongmusk.icebox

import android.os.Bundle
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import kotlin.math.roundToInt

class MainActivity : TauriActivity() {
  private var navigationBarInsetCssPx: Int = 0
  private var navigationBarInsetInstalled = false

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Wry loads the Rust library inside super.onCreate. Init the platform
    // verifier after that so subscription HTTPS can use the OS trust store.
    runCatching { Native.initVerifier(applicationContext) }
      .onSuccess { Log.i(TAG, it) }
      .onFailure { Log.e(TAG, "verifier init failed", it) }
    // The WebView is added after onCreate returns.
    watchForWebView(0)
  }

  override fun onStart() {
    super.onStart()
    watchForWebView(0)
  }

  /**
   * The page draws edge-to-edge, but this WebView reports
   * safe-area-inset-bottom as 0 in gesture navigation. Publish the navigation
   * bar height so the tab bar sits above the system bar.
   */
  private fun watchForWebView(attempt: Int) {
    if (navigationBarInsetInstalled) return
    val content = window.decorView
    val webView = findWebView(content)
    if (webView == null) {
      if (attempt >= 50) {
        Log.w(TAG, "navigation bar inset skipped: WebView not found")
        return
      }
      content.postDelayed({ watchForWebView(attempt + 1) }, 50)
      return
    }
    navigationBarInsetInstalled = true
    installNavigationBarInset(content, webView)
  }

  private fun installNavigationBarInset(content: View, webView: WebView) {
    webView.addJavascriptInterface(NavigationBarInsetBridge(), "IceBoxAndroid")
    if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
      WebViewCompat.addDocumentStartJavaScript(
        webView,
        DOCUMENT_START_SCRIPT,
        setOf("*"),
      )
    }
    ViewCompat.setOnApplyWindowInsetsListener(content) { _, insets ->
      rememberNavigationBarInset(insets)
      publishNavigationBarInset(webView)
      insets
    }
    // requestApplyInsets during onCreate often runs before the window is
    // attached, and the page may already exist by the time insets arrive.
    // Publish again after layout and after the first paint.
    content.post {
      ViewCompat.getRootWindowInsets(content)?.let { rememberNavigationBarInset(it) }
      ViewCompat.requestApplyInsets(content)
      publishNavigationBarInset(webView)
      webView.postVisualStateCallback(
        1,
        object : WebView.VisualStateCallback() {
          override fun onComplete(requestId: Long) {
            ViewCompat.getRootWindowInsets(content)?.let { rememberNavigationBarInset(it) }
            publishNavigationBarInset(webView)
          }
        },
      )
    }
  }

  private fun rememberNavigationBarInset(insets: WindowInsetsCompat) {
    val nav = insets.getInsets(WindowInsetsCompat.Type.navigationBars())
    val density = resources.displayMetrics.density.coerceAtLeast(1f)
    navigationBarInsetCssPx = (nav.bottom / density).roundToInt()
    Log.i(TAG, "navigation bar inset ${navigationBarInsetCssPx}px")
  }

  private fun publishNavigationBarInset(webView: WebView) {
    val px = navigationBarInsetCssPx
    webView.evaluateJavascript(
      "document.documentElement.style.setProperty('--ice-nav-inset','${px}px')",
      null,
    )
  }

  private fun findWebView(view: View): WebView? {
    if (view is WebView) return view
    if (view is ViewGroup) {
      for (i in 0 until view.childCount) {
        findWebView(view.getChildAt(i))?.let { return it }
      }
    }
    return null
  }

  private inner class NavigationBarInsetBridge {
    @JavascriptInterface
    fun navigationBarInsetCssPx(): Int = navigationBarInsetCssPx
  }

  companion object {
    private const val TAG = "IceBox"
    private const val DOCUMENT_START_SCRIPT = """
      (function () {
        try {
          var bridge = window.IceBoxAndroid;
          if (!bridge) return;
          var px = bridge.navigationBarInsetCssPx();
          document.documentElement.style.setProperty('--ice-nav-inset', px + 'px');
        } catch (e) {}
      })();
    """
  }
}

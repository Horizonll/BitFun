package com.bitfun.glasses.ui

import android.util.Log
import android.webkit.JavascriptInterface

/**
 * Minimal WebView host bridge for glasses-web. Keep methods narrow and UI-thread safe.
 */
class GlassesHostBridge(
    private val host: MobileWebActivity,
) {
    @JavascriptInterface
    fun rescanQr() {
        host.runOnUiThread {
            try {
                host.openQrScan()
            } catch (error: Exception) {
                Log.e(TAG, "Failed to open QR scan", error)
            }
        }
    }

    @JavascriptInterface
    fun isVoiceInputAvailable(): Boolean = host.isVoiceInputAvailable()

    @JavascriptInterface
    fun startVoiceInput() {
        host.runOnUiThread {
            host.startVoiceInputFromWeb()
        }
    }

    @JavascriptInterface
    fun stopVoiceInput() {
        host.runOnUiThread {
            host.stopVoiceInputFromWeb()
        }
    }

    companion object {
        const val JS_NAME = "BitFunGlassesHost"
        private const val TAG = "BitFunGlassesHost"
    }
}

package com.bitfun.glasses.ui

import android.util.Log
import android.webkit.JavascriptInterface

/**
 * WebView host bridge for the left-eye glasses SPA.
 * Right eye is a PixelCopy clone and does not run JavaScript.
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

    /** Exit the paired SPA host (session-list temple double-click). */
    @JavascriptInterface
    fun exitApp() {
        host.runOnUiThread {
            try {
                host.exitFromWeb()
            } catch (error: Exception) {
                Log.e(TAG, "Failed to exit app", error)
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

    @JavascriptInterface
    fun getSharedInstallId(): String = GlassesIdentityStore.getOrCreateInstallId(host)

    @JavascriptInterface
    fun getSharedLanguage(): String = GlassesIdentityStore.getLanguage(host).orEmpty()

    @JavascriptInterface
    fun putSharedLanguage(language: String?) {
        val value = language?.trim().orEmpty()
        if (value.isEmpty()) return
        GlassesIdentityStore.putLanguage(host, value)
    }

    /** Pull pending PCM (base64) for a desktop ASR transcription request. */
    @JavascriptInterface
    fun takePendingAsrPcm(requestId: String?): String {
        val id = requestId?.trim().orEmpty()
        if (id.isEmpty()) return ""
        return host.takePendingAsrPcm(id).orEmpty()
    }

    @JavascriptInterface
    fun completeDesktopAsr(requestId: String?, text: String?) {
        host.runOnUiThread {
            host.completeDesktopAsr(requestId.orEmpty(), text, null)
        }
    }

    @JavascriptInterface
    fun failDesktopAsr(requestId: String?, error: String?) {
        host.runOnUiThread {
            host.completeDesktopAsr(requestId.orEmpty(), null, error)
        }
    }

    companion object {
        const val JS_NAME = "BitFunGlassesHost"
        private const val TAG = "BitFunGlassesHost"
    }
}

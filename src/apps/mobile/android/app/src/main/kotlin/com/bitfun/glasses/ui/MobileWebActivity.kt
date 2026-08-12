package com.bitfun.glasses.ui

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.Color
import android.os.Bundle
import android.util.Log
import android.view.View
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.bitfun.glasses.databinding.ActivityMobileWebBinding
import com.bitfun.glasses.R
import com.ffalcon.mercury.android.sdk.touch.TempleAction
import com.ffalcon.mercury.android.sdk.ui.activity.BaseMirrorActivity
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.io.IOException
import java.nio.charset.StandardCharsets

/**
 * Binocular host for glasses-web on RayNeo X3.
 *
 * Left eye runs the SPA WebView. Right eye is a live pixel clone via
 * [EyeSurfaceMirror] so deletes/scroll/voice UI cannot drift between eyes.
 * Temple gestures are forwarded to JS (`window.__bitfunTemple`) on the left only.
 */
class MobileWebActivity : BaseMirrorActivity<ActivityMobileWebBinding>() {
    private var voiceController: VoiceInputController? = null
    private var pendingStartVoice = false
    private var primaryWebView: WebView? = null
    private var eyeMirror: EyeSurfaceMirror? = null

    private val speechLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { result ->
        voiceController?.onActivityResult(result.resultCode, result.data)
    }

    private val micPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) {
            if (pendingStartVoice) {
                pendingStartVoice = false
                beginVoiceInput()
            }
        } else {
            pendingStartVoice = false
            val message = getString(R.string.voice_error_permission)
            GlassesToast.show(this, message)
            dispatchVoiceEvent("error", message)
            dispatchVoiceEvent("ended")
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.setBackgroundDrawableResource(R.color.bf_black)

        val url = intent.getStringExtra(EXTRA_URL).orEmpty()
        if (url.isBlank()) {
            GlassesToast.show(this, R.string.scan_invalid_qr, longDuration = true)
            finish()
            return
        }
        if (!hasBundledGlassesWeb()) {
            GlassesToast.show(this, R.string.glasses_web_missing, longDuration = true)
            finish()
            return
        }

        primaryWebView = mBindingPair.left.webView
        voiceController = VoiceInputController(
            this,
            speechLauncher,
            object : VoiceInputController.Listener {
                override fun onVoiceStarted() {
                    dispatchVoiceEvent("started")
                }

                override fun onVoiceResult(text: String) {
                    dispatchVoiceEvent("result", text)
                }

                override fun onVoiceError(message: String) {
                    GlassesToast.show(this@MobileWebActivity, message)
                    dispatchVoiceEvent("error", message)
                }

                override fun onVoiceEnded() {
                    dispatchVoiceEvent("ended")
                }

                override fun onVoiceInfo(message: String) {
                    GlassesToast.show(this@MobileWebActivity, message)
                    dispatchVoiceEvent("info", message)
                }

                override fun onDesktopAsrRequest(requestId: String) {
                    dispatchDesktopAsrRequest(requestId)
                }
            },
        )

        // Left = sole SPA runtime. Right = ImageView pixel clone (no second WebView).
        configureEyeWebView(mBindingPair.left.webView)
        mBindingPair.left.eyeMirror.visibility = View.GONE
        mBindingPair.left.webView.visibility = View.VISIBLE

        mBindingPair.right.webView.stopLoading()
        mBindingPair.right.webView.visibility = View.GONE
        mBindingPair.right.eyeMirror.visibility = View.VISIBLE

        mBindingPair.left.webView.loadUrl(url)
        eyeMirror = EyeSurfaceMirror(
            this,
            mBindingPair.left.webView,
            mBindingPair.right.eyeMirror,
        )
        // Started in onResume so pause/resume owns the PixelCopy lifecycle.

        onBackPressedDispatcher.addCallback(
            this,
            object : OnBackPressedCallback(true) {
                override fun handleOnBackPressed() {
                    handleTempleBack()
                }
            },
        )
        collectTempleActions()
    }

    private fun collectTempleActions() {
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                templeActionViewModel.state.collect { action ->
                    when (action) {
                        is TempleAction.SlideForward ->
                            dispatchTempleEvent("forward")
                        is TempleAction.SlideBackward ->
                            dispatchTempleEvent("backward")
                        is TempleAction.SlideUpwards ->
                            dispatchTempleEvent("up")
                        is TempleAction.SlideDownwards ->
                            dispatchTempleEvent("down")
                        is TempleAction.Click ->
                            dispatchTempleEvent("click")
                        is TempleAction.DoubleClick ->
                            // SPA owns in-app back (chat → sessions). System back still finishes.
                            dispatchTempleEvent("doubleclick")
                        else -> Unit
                    }
                }
            }
        }
    }

    private fun handleTempleBack() {
        val web = primaryWebView
        if (web != null && web.canGoBack()) {
            web.goBack()
        } else {
            finish()
        }
    }

    /** Called from SPA when temple double-click should leave the app (session list). */
    fun exitFromWeb() {
        finish()
    }

    fun openQrScan() {
        startActivity(
            Intent(this, QrScanActivity::class.java).apply {
                addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            },
        )
        finish()
    }

    fun isVoiceInputAvailable(): Boolean = voiceController != null

    fun startVoiceInputFromWeb() {
        val listening = voiceController?.isListening() == true
        Log.i(TAG, "startVoiceInputFromWeb listening=$listening")
        // Stuck native listen + SPA idle desync: abort binder session, then
        // restart after the OEM RemoteSpeechRecognitionService releases.
        if (listening) {
            Log.i(TAG, "start while listening → abort then restart")
            pendingStartVoice = false
            voiceController?.abortAndReadyForRestart()
            window.decorView.postDelayed({
                if (isDestroyed || isFinishing) return@postDelayed
                if (voiceController?.isListening() == true) return@postDelayed
                if (!hasMicPermission()) {
                    pendingStartVoice = true
                    micPermissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
                    return@postDelayed
                }
                beginVoiceInput()
            }, 800L)
            return
        }
        if (!hasMicPermission()) {
            pendingStartVoice = true
            micPermissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        beginVoiceInput()
    }

    fun stopVoiceInputFromWeb() {
        Log.i(TAG, "stopVoiceInputFromWeb listening=${voiceController?.isListening()}")
        pendingStartVoice = false
        voiceController?.stop()
    }

    private fun beginVoiceInput() {
        val controller = voiceController
        if (controller == null) {
            val message = getString(R.string.voice_unavailable)
            GlassesToast.show(this, message)
            dispatchVoiceEvent("error", message)
            dispatchVoiceEvent("ended")
            return
        }
        controller.start()
    }

    private fun hasMicPermission(): Boolean =
        ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED

    private fun dispatchVoiceEvent(type: String, payload: String? = null) {
        // Left SPA owns voice UX; right eye is a pixel clone.
        val typeLiteral = JSONObject.quote(type)
        val payloadLiteral = if (payload == null) "null" else JSONObject.quote(payload)
        Log.d(TAG, "dispatchVoiceEvent type=$type payload=${payload?.take(80)}")
        val script =
            "(function(){try{var fn=window.__bitfunOnVoiceEvent;" +
                "if(typeof fn==='function'){fn($typeLiteral,$payloadLiteral);}" +
                "else{console.error('[BitFunVoice] missing handler', $typeLiteral);}" +
                "}catch(e){console.error('[BitFunVoice]', e);}})();"
        evaluateOnPrimaryWebView(script)
    }

    private fun dispatchDesktopAsrRequest(requestId: String) {
        val idLiteral = JSONObject.quote(requestId)
        Log.d(TAG, "dispatchDesktopAsrRequest id=$requestId")
        val script =
            "(function(){try{var fn=window.__bitfunOnDesktopAsrRequest;" +
                "if(typeof fn==='function'){fn($idLiteral);}" +
                "else{console.error('[BitFunVoice] missing desktop ASR handler');}" +
                "}catch(e){console.error('[BitFunVoice]', e);}})();"
        evaluateOnPrimaryWebView(script)
    }

    fun takePendingAsrPcm(requestId: String): String? =
        voiceController?.takePendingAsrPcm(requestId)

    fun completeDesktopAsr(requestId: String, text: String?, error: String?) {
        voiceController?.completeDesktopAsr(requestId, text, error)
    }

    private fun dispatchTempleEvent(action: String) {
        val actionLiteral = JSONObject.quote(action)
        val script =
            "(function(){try{var fn=window.__bitfunTemple;" +
                "if(typeof fn==='function'){fn($actionLiteral);}" +
                "else{console.warn('[BitFunTemple] missing handler', $actionLiteral);}" +
                "}catch(e){console.error('[BitFunTemple]', e);}})();"
        evaluateOnPrimaryWebView(script)
    }

    override fun onPause() {
        eyeMirror?.stop()
        super.onPause()
    }

    override fun onResume() {
        super.onResume()
        if (eyeMirror == null && this::mBindingPair.isInitialized) {
            eyeMirror = EyeSurfaceMirror(
                this,
                mBindingPair.left.webView,
                mBindingPair.right.eyeMirror,
            )
        }
        eyeMirror?.start()
    }

    private fun evaluateOnPrimaryWebView(script: String) {
        mBindingPair.left.webView.post {
            mBindingPair.left.webView.evaluateJavascript(script, null)
        }
    }

    private fun setLoadingVisible(visible: Boolean) {
        // Glasses UX: never show the WebView page-load spinner.
        if (visible) return
        mBindingPair.left.loading.visibility = View.GONE
        mBindingPair.right.loading.visibility = View.GONE
    }

    private fun hasBundledGlassesWeb(): Boolean =
        runCatching {
            assets.open("${MobileWebAssetRouter.ASSET_ROOT}/index.html").close()
            true
        }.getOrDefault(false)

    private fun configureEyeWebView(webView: WebView) {
        configureWebView(webView)
        webView.addJavascriptInterface(
            GlassesHostBridge(this),
            GlassesHostBridge.JS_NAME,
        )
    }

    private fun configureWebView(webView: WebView) {
        webView.setBackgroundColor(Color.BLACK)
        webView.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            loadWithOverviewMode = true
            useWideViewPort = true
            builtInZoomControls = false
            displayZoomControls = false
            mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
            cacheMode = WebSettings.LOAD_NO_CACHE
            mediaPlaybackRequiresUserGesture = false
            allowFileAccess = false
            allowContentAccess = false
        }
        webView.webChromeClient = WebChromeClient()
        webView.webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(
                view: WebView,
                request: WebResourceRequest,
            ): WebResourceResponse? {
                if (request.method != "GET") return null
                val uri = request.url ?: return null
                val assetPath = MobileWebAssetRouter.assetPathFor(uri) ?: return null
                return openAssetResponse(assetPath)
            }

            override fun shouldOverrideUrlLoading(
                view: WebView,
                request: WebResourceRequest,
            ): Boolean {
                val target = request.url?.toString().orEmpty()
                return !isAllowedNavigation(target)
            }

            override fun onPageStarted(view: WebView?, url: String?, favicon: Bitmap?) {
                setLoadingVisible(true)
            }

            override fun onPageFinished(view: WebView?, url: String?) {
                setLoadingVisible(false)
            }
        }
    }

    private fun openAssetResponse(assetPath: String): WebResourceResponse? {
        return try {
            val stream = assets.open(assetPath)
            val mime = MobileWebAssetRouter.mimeTypeFor(assetPath)
            val charset =
                if (mime.startsWith("text/") ||
                    mime == "application/javascript" ||
                    mime == "application/json"
                ) {
                    StandardCharsets.UTF_8.name()
                } else {
                    null
                }
            WebResourceResponse(
                mime,
                charset,
                200,
                "OK",
                mapOf("Cache-Control" to "no-store"),
                stream,
            )
        } catch (error: IOException) {
            Log.w(TAG, "Missing bundled asset: $assetPath", error)
            null
        }
    }

    override fun onDestroy() {
        pendingStartVoice = false
        eyeMirror?.stop()
        eyeMirror = null
        voiceController?.destroy()
        voiceController = null
        primaryWebView = null
        if (this::mBindingPair.isInitialized) {
            mBindingPair.left.webView.stopLoading()
            mBindingPair.left.webView.destroy()
            // Right WebView was never loaded; still release the unused instance.
            mBindingPair.right.webView.stopLoading()
            mBindingPair.right.webView.destroy()
        }
        super.onDestroy()
    }

    companion object {
        private const val TAG = "BitFunGlassesWeb"
        const val EXTRA_URL = "pairing_url"

        fun intent(from: Context, pairingUrl: String): Intent =
            Intent(from, MobileWebActivity::class.java).putExtra(EXTRA_URL, pairingUrl)

        fun isAllowedNavigation(url: String): Boolean {
            val lower = url.lowercase()
            return lower.startsWith("http://") ||
                lower.startsWith("https://") ||
                lower.startsWith("about:blank") ||
                lower.startsWith("data:")
        }
    }
}

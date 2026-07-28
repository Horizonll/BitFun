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
import android.view.ViewTreeObserver
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.ProgressBar
import android.widget.Toast
import androidx.activity.OnBackPressedCallback
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.camera.core.CameraSelector
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import com.bitfun.glasses.R
import org.json.JSONObject
import java.io.IOException
import java.nio.charset.StandardCharsets

/**
 * WebView loads the scanned relay URL (keeps same-origin for /api), but serves
 * HTML/JS/CSS from APK `assets/glasses-web`. On phone debug builds, a live
 * camera preview sits under the transparent WebView to simulate optical AR.
 */
class MobileWebActivity : AppCompatActivity() {
    private lateinit var root: View
    private lateinit var webView: WebView
    private lateinit var loading: ProgressBar
    private lateinit var worldPreview: PreviewView
    private var cameraBound = false
    private var phoneVrLayoutListener: ViewTreeObserver.OnGlobalLayoutListener? = null
    private var voiceController: VoiceInputController? = null
    private var pendingStartVoice = false

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
            Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
            dispatchVoiceEvent("error", message)
            dispatchVoiceEvent("ended")
        }
    }

    override fun attachBaseContext(newBase: Context) {
        super.attachBaseContext(PhonePreviewScale.wrap(newBase))
    }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_mobile_web)
        root = findViewById(R.id.mobileWebRoot)
        webView = findViewById(R.id.webView)
        loading = findViewById(R.id.loading)
        worldPreview = findViewById(R.id.worldPreview)

        val url = intent.getStringExtra(EXTRA_URL).orEmpty()
        if (url.isBlank()) {
            Toast.makeText(this, R.string.scan_invalid_qr, Toast.LENGTH_LONG).show()
            finish()
            return
        }
        if (!hasBundledGlassesWeb()) {
            Toast.makeText(this, R.string.glasses_web_missing, Toast.LENGTH_LONG).show()
            finish()
            return
        }

        configureWebView()
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
                    Toast.makeText(this@MobileWebActivity, message, Toast.LENGTH_SHORT).show()
                    dispatchVoiceEvent("error", message)
                }

                override fun onVoiceEnded() {
                    dispatchVoiceEvent("ended")
                }
            },
        )
        webView.addJavascriptInterface(GlassesHostBridge(this), GlassesHostBridge.JS_NAME)
        maybeApplyPhoneVrScale()
        maybeStartPhoneWorldPreview()
        onBackPressedDispatcher.addCallback(
            this,
            object : OnBackPressedCallback(true) {
                override fun handleOnBackPressed() {
                    if (webView.canGoBack()) {
                        webView.goBack()
                    } else {
                        finish()
                    }
                }
            },
        )
        webView.loadUrl(url)
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
        Log.i(TAG, "startVoiceInputFromWeb listening=${voiceController?.isListening()}")
        if (voiceController?.isListening() == true) return
        if (!hasMicPermission()) {
            pendingStartVoice = true
            micPermissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        beginVoiceInput()
    }

    fun stopVoiceInputFromWeb() {
        pendingStartVoice = false
        voiceController?.stop()
    }

    private fun beginVoiceInput() {
        val controller = voiceController
        if (controller == null) {
            val message = getString(R.string.voice_unavailable)
            Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
            dispatchVoiceEvent("error", message)
            dispatchVoiceEvent("ended")
            return
        }
        // Always attempt start; OEM capability probes are unreliable.
        controller.start()
    }

    private fun hasMicPermission(): Boolean =
        ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED

    private fun dispatchVoiceEvent(type: String, payload: String? = null) {
        if (!this::webView.isInitialized) return
        val typeLiteral = JSONObject.quote(type)
        val payloadLiteral = if (payload == null) "null" else JSONObject.quote(payload)
        val script =
            "(function(){try{var fn=window.__bitfunOnVoiceEvent;" +
                "if(typeof fn==='function'){fn($typeLiteral,$payloadLiteral);}" +
                "else{console.warn('[BitFunVoice] missing handler', $typeLiteral);}" +
                "}catch(e){console.error('[BitFunVoice]', e);}})();"
        // Post to the WebView queue so the callback always runs on the UI thread
        // after the current JavascriptInterface binder call returns.
        webView.post {
            webView.evaluateJavascript(script, null)
        }
    }

    private fun hasBundledGlassesWeb(): Boolean =
        runCatching {
            assets.open("${MobileWebAssetRouter.ASSET_ROOT}/index.html").close()
            true
        }.getOrDefault(false)

    /**
     * Phone-as-VR: lay out glasses-web at a larger logical canvas, then scale
     * the WebView down so the dual-pane shell fits the small phone screen.
     */
    private fun maybeApplyPhoneVrScale() {
        if (!PhonePreviewScale.enabled) {
            PhonePreviewScale.clearWebViewScale(webView)
            return
        }
        val listener = ViewTreeObserver.OnGlobalLayoutListener {
            PhonePreviewScale.applyToWebView(root, webView)
        }
        phoneVrLayoutListener = listener
        root.viewTreeObserver.addOnGlobalLayoutListener(listener)
    }

    private fun maybeStartPhoneWorldPreview() {
        // Real RayNeo optical see-through does not need a camera underlay.
        if (!PhonePreviewScale.enabled) return
        if (!hasCameraPermission()) {
            Log.w(TAG, "Camera permission missing; phone AR preview disabled")
            return
        }
        worldPreview.visibility = View.VISIBLE
        val providerFuture = ProcessCameraProvider.getInstance(this)
        providerFuture.addListener(
            {
                try {
                    val provider = providerFuture.get()
                    if (cameraBound) return@addListener
                    val preview = Preview.Builder().build().also {
                        it.surfaceProvider = worldPreview.surfaceProvider
                    }
                    provider.unbindAll()
                    provider.bindToLifecycle(
                        this,
                        CameraSelector.DEFAULT_BACK_CAMERA,
                        preview,
                    )
                    cameraBound = true
                } catch (error: Exception) {
                    Log.e(TAG, "Failed to start phone world camera preview", error)
                    worldPreview.visibility = View.GONE
                }
            },
            ContextCompat.getMainExecutor(this),
        )
    }

    private fun hasCameraPermission(): Boolean =
        ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED

    private fun configureWebView() {
        webView.setBackgroundColor(Color.TRANSPARENT)
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
                return if (isAllowedNavigation(target)) {
                    false
                } else {
                    true
                }
            }

            override fun onPageStarted(view: WebView?, url: String?, favicon: Bitmap?) {
                loading.visibility = View.VISIBLE
            }

            override fun onPageFinished(view: WebView?, url: String?) {
                loading.visibility = View.GONE
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
        voiceController?.destroy()
        voiceController = null
        phoneVrLayoutListener?.let { listener ->
            if (this::root.isInitialized && root.viewTreeObserver.isAlive) {
                root.viewTreeObserver.removeOnGlobalLayoutListener(listener)
            }
        }
        phoneVrLayoutListener = null
        webView.stopLoading()
        webView.destroy()
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

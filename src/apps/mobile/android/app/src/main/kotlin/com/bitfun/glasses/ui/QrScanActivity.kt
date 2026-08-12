package com.bitfun.glasses.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import android.view.View
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.bitfun.glasses.R
import com.bitfun.glasses.databinding.ActivityQrScanBinding
import com.bitfun.glasses.remote.PairingUrlParser
import com.ffalcon.mercury.android.sdk.touch.TempleAction
import com.ffalcon.mercury.android.sdk.ui.activity.BaseMirrorActivity
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.launch

/**
 * Binocular QR launch page (same BindingPair model as the main WebView host).
 * Analysis-only camera — no preview, no aiming frame. Centered "扫码连接" only.
 */
class QrScanActivity : BaseMirrorActivity<ActivityQrScanBinding>() {
    private val cameraExecutor = Executors.newSingleThreadExecutor()
    private val handled = AtomicBoolean(false)
    private var cameraBound = false
    private var cameraProvider: ProcessCameraProvider? = null

    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) {
            mBindingPair.updateView {
                permissionButton.visibility = View.GONE
                scanHint.setText(R.string.scan_title)
            }
            startCamera()
        } else {
            mBindingPair.updateView {
                permissionButton.visibility = View.VISIBLE
            }
            GlassesToast.show(this, R.string.scan_camera_denied, longDuration = true)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.setBackgroundDrawableResource(R.color.bf_black)

        mBindingPair.updateView {
            scanHint.setText(R.string.scan_title)
            permissionButton.setOnClickListener { requestCamera() }
        }

        if (hasCameraPermission()) {
            startCamera()
        } else {
            mBindingPair.updateView {
                permissionButton.visibility = View.VISIBLE
            }
            requestCamera()
        }

        collectTempleActions()
    }

    private fun collectTempleActions() {
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                templeActionViewModel.state.collect { action ->
                    when (action) {
                        is TempleAction.DoubleClick -> {
                            Log.i(TAG, "Temple double-click: exit scan activity")
                            finish()
                        }
                        else -> Unit
                    }
                }
            }
        }
    }

    override fun onDestroy() {
        cameraProvider?.unbindAll()
        cameraProvider = null
        cameraExecutor.shutdown()
        super.onDestroy()
    }

    private fun hasCameraPermission(): Boolean =
        ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED

    private fun requestCamera() {
        permissionLauncher.launch(Manifest.permission.CAMERA)
    }

    private fun startCamera() {
        if (cameraBound) return
        val providerFuture = ProcessCameraProvider.getInstance(this)
        providerFuture.addListener(
            {
                try {
                    val provider = providerFuture.get()
                    cameraProvider = provider
                    bindCamera(provider)
                } catch (error: Exception) {
                    Log.e(TAG, "Camera provider failed", error)
                    val message = error.message ?: getString(R.string.status_error)
                    GlassesToast.show(this, message, longDuration = true)
                }
            },
            ContextCompat.getMainExecutor(this),
        )
    }

    private fun bindCamera(provider: ProcessCameraProvider) {
        // No Preview use-case: HUD-only; both eyes share identical BindingPair UI.
        val analysis = ImageAnalysis.Builder()
            .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
            .build()

        val options = BarcodeScannerOptions.Builder()
            .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
            .build()
        val scanner = BarcodeScanning.getClient(options)

        analysis.setAnalyzer(cameraExecutor) { imageProxy ->
            val mediaImage = imageProxy.image
            if (mediaImage == null || handled.get()) {
                imageProxy.close()
                return@setAnalyzer
            }
            val image = InputImage.fromMediaImage(
                mediaImage,
                imageProxy.imageInfo.rotationDegrees,
            )
            scanner.process(image)
                .addOnSuccessListener { barcodes ->
                    val raw = barcodes.firstNotNullOfOrNull { it.rawValue } ?: return@addOnSuccessListener
                    onQrDetected(raw)
                }
                .addOnCompleteListener {
                    imageProxy.close()
                }
        }

        provider.unbindAll()
        val selector = when {
            provider.hasCamera(CameraSelector.DEFAULT_BACK_CAMERA) ->
                CameraSelector.DEFAULT_BACK_CAMERA
            provider.hasCamera(CameraSelector.DEFAULT_FRONT_CAMERA) ->
                CameraSelector.DEFAULT_FRONT_CAMERA
            else -> throw IllegalStateException("No camera available on this device")
        }
        provider.bindToLifecycle(this, selector, analysis)
        cameraBound = true
    }

    private fun onQrDetected(raw: String) {
        if (!handled.compareAndSet(false, true)) return
        runOnUiThread {
            try {
                PairingUrlParser.parse(raw)
                startActivity(MobileWebActivity.intent(this, raw.trim()))
                finish()
            } catch (error: Exception) {
                handled.set(false)
                Log.w(TAG, "Invalid QR: ${error.message}")
                val message = error.message ?: getString(R.string.scan_invalid_qr)
                GlassesToast.show(this, message, longDuration = true)
            }
        }
    }

    companion object {
        private const val TAG = "BitFunGlassesScan"
    }
}

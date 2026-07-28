package com.bitfun.glasses.ui

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import android.view.View
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import com.bitfun.glasses.R
import com.bitfun.glasses.remote.PairingUrlParser
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Launcher: scan Desktop Remote Connect QR, then open glasses-web in WebView.
 */
class QrScanActivity : AppCompatActivity() {
    private lateinit var previewView: PreviewView
    private lateinit var scanHint: TextView
    private lateinit var scanStatus: TextView
    private lateinit var permissionButton: Button

    private val cameraExecutor = Executors.newSingleThreadExecutor()
    private val handled = AtomicBoolean(false)
    private var cameraBound = false

    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) {
            permissionButton.visibility = View.GONE
            scanStatus.setText(R.string.scan_hint)
            startCamera()
        } else {
            permissionButton.visibility = View.VISIBLE
            scanStatus.setText(R.string.scan_camera_denied)
        }
    }

    override fun attachBaseContext(newBase: Context) {
        super.attachBaseContext(PhonePreviewScale.wrap(newBase))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_qr_scan)
        previewView = findViewById(R.id.previewView)
        scanHint = findViewById(R.id.scanHint)
        scanStatus = findViewById(R.id.scanStatus)
        permissionButton = findViewById(R.id.permissionButton)

        scanHint.setText(R.string.scan_title)
        scanStatus.setText(R.string.scan_hint)
        permissionButton.setOnClickListener { requestCamera() }

        if (hasCameraPermission()) {
            startCamera()
        } else {
            permissionButton.visibility = View.VISIBLE
            scanStatus.setText(R.string.scan_camera_needed)
            requestCamera()
        }
    }

    override fun onDestroy() {
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
                    bindCamera(provider)
                } catch (error: Exception) {
                    Log.e(TAG, "Camera provider failed", error)
                    scanStatus.text = error.message ?: getString(R.string.status_error)
                }
            },
            ContextCompat.getMainExecutor(this),
        )
    }

    private fun bindCamera(provider: ProcessCameraProvider) {
        val preview = Preview.Builder().build().also {
            it.surfaceProvider = previewView.surfaceProvider
        }
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
        provider.bindToLifecycle(this, selector, preview, analysis)
        cameraBound = true
        scanStatus.setText(R.string.scan_hint)
    }

    private fun onQrDetected(raw: String) {
        if (!handled.compareAndSet(false, true)) return
        runOnUiThread {
            try {
                // Validate relay reachability rules; WebView still loads the full QR URL.
                PairingUrlParser.parse(raw)
                scanStatus.setText(R.string.scan_opening)
                scanStatus.setTextColor(ContextCompat.getColor(this, R.color.bf_scan_ok))
                startActivity(MobileWebActivity.intent(this, raw.trim()))
                finish()
            } catch (error: Exception) {
                handled.set(false)
                Log.w(TAG, "Invalid QR: ${error.message}")
                Toast.makeText(
                    this,
                    error.message ?: getString(R.string.scan_invalid_qr),
                    Toast.LENGTH_LONG,
                ).show()
                scanStatus.text = error.message ?: getString(R.string.scan_invalid_qr)
            }
        }
    }

    companion object {
        private const val TAG = "BitFunGlassesScan"
    }
}

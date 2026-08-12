package com.bitfun.glasses.ui

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Rect
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.PixelCopy
import android.view.View
import android.widget.ImageView
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

/**
 * Continuously copies pixels from the left-eye [source] view into the right-eye
 * [target] ImageView so the secondary eye is a true visual clone.
 */
class EyeSurfaceMirror(
    private val activity: Activity,
    private val source: View,
    private val target: ImageView,
) {
    private val mainHandler = Handler(Looper.getMainLooper())
    private val running = AtomicBoolean(false)
    private val generation = AtomicInteger(0)
    private val buffers = arrayOfNulls<Bitmap>(2)
    private var writeIndex = 0
    private var inFlight = false
    private val location = IntArray(2)

    private val tick = Runnable { captureFrame() }

    fun start() {
        if (!running.compareAndSet(false, true)) return
        generation.incrementAndGet()
        target.setBackgroundColor(Color.BLACK)
        target.visibility = View.VISIBLE
        target.scaleType = ImageView.ScaleType.FIT_XY
        schedule(0L)
        Log.i(TAG, "EyeSurfaceMirror started")
    }

    fun stop() {
        if (!running.getAndSet(false)) return
        val gen = generation.incrementAndGet()
        mainHandler.removeCallbacks(tick)
        target.setImageDrawable(null)
        // Defer recycle until any in-flight PixelCopy callback observes the new
        // generation and exits without touching the buffers.
        mainHandler.post {
            if (generation.get() != gen) return@post
            inFlight = false
            buffers[0]?.recycle()
            buffers[1]?.recycle()
            buffers[0] = null
            buffers[1] = null
        }
        Log.i(TAG, "EyeSurfaceMirror stopped")
    }

    private fun schedule(delayMs: Long) {
        if (!running.get()) return
        mainHandler.removeCallbacks(tick)
        mainHandler.postDelayed(tick, delayMs)
    }

    private fun captureFrame() {
        if (!running.get()) return
        if (inFlight) {
            schedule(FRAME_INTERVAL_MS)
            return
        }
        val width = source.width
        val height = source.height
        if (width <= 0 || height <= 0 || !source.isShown) {
            schedule(FRAME_INTERVAL_MS)
            return
        }

        val bitmap = obtainWriteBitmap(width, height) ?: run {
            schedule(FRAME_INTERVAL_MS)
            return
        }

        source.getLocationInWindow(location)
        val rect = Rect(
            location[0],
            location[1],
            location[0] + width,
            location[1] + height,
        )
        val window = activity.window
        if (window == null || Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            schedule(FRAME_INTERVAL_MS)
            return
        }

        val gen = generation.get()
        inFlight = true
        try {
            PixelCopy.request(window, rect, bitmap, { result ->
                if (generation.get() != gen) {
                    inFlight = false
                    return@request
                }
                inFlight = false
                if (!running.get()) return@request
                if (result == PixelCopy.SUCCESS) {
                    target.setImageBitmap(bitmap)
                    writeIndex = 1 - writeIndex
                }
                schedule(FRAME_INTERVAL_MS)
            }, mainHandler)
        } catch (error: Exception) {
            Log.e(TAG, "PixelCopy failed", error)
            inFlight = false
            schedule(FRAME_INTERVAL_MS)
        }
    }

    private fun obtainWriteBitmap(width: Int, height: Int): Bitmap? {
        val existing = buffers[writeIndex]
        if (existing != null && existing.width == width && existing.height == height && !existing.isRecycled) {
            return existing
        }
        existing?.recycle()
        return try {
            Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888).also {
                buffers[writeIndex] = it
            }
        } catch (error: Exception) {
            Log.e(TAG, "createBitmap failed ${width}x$height", error)
            buffers[writeIndex] = null
            null
        }
    }

    companion object {
        private const val TAG = "BitFunEyeMirror"
        /** ~20 FPS — enough for chat UI on X3. */
        private const val FRAME_INTERVAL_MS = 50L
    }
}

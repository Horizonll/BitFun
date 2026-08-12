package com.bitfun.glasses.ui

import android.content.Context
import android.content.res.Configuration
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import kotlin.math.roundToInt

/**
 * Retired phone-debug scale helper. Real glasses builds never scale; layout
 * math remains for unit tests.
 */
object PhonePreviewScale {
    /**
     * Visual scale of the VR canvas on the phone.
     * `0.55` ≈ layout at ~1.8× phone pixels, then shrink to fit.
     */
    const val FACTOR = 0.55f

    /**
     * Phone simulation scale is retired — real glasses builds never scale.
     * Kept only so unit tests can exercise layout math when forced on.
     */
    val enabled: Boolean
        get() = false

    data class ScaledWebViewLayout(
        val widthPx: Int,
        val heightPx: Int,
        val scale: Float,
    )

    /** Logical WebView size + scale for a phone parent of [parentWidth]×[parentHeight]. */
    fun scaledWebViewLayout(parentWidth: Int, parentHeight: Int): ScaledWebViewLayout? {
        if (!enabled || parentWidth <= 0 || parentHeight <= 0) return null
        val scale = FACTOR
        return ScaledWebViewLayout(
            widthPx = (parentWidth / scale).roundToInt().coerceAtLeast(1),
            heightPx = (parentHeight / scale).roundToInt().coerceAtLeast(1),
            scale = scale,
        )
    }

    fun wrap(base: Context): Context {
        if (!enabled) return base
        val metrics = base.resources.displayMetrics
        val targetDpi = (metrics.densityDpi * FACTOR).toInt().coerceIn(80, 480)
        if (targetDpi == metrics.densityDpi) return base
        val config = Configuration(base.resources.configuration)
        config.densityDpi = targetDpi
        return base.createConfigurationContext(config)
    }

    /**
     * Layout [webView] as a larger VR canvas and scale it into [parent].
     * Parent must have `clipChildren=false` so the oversized view is not clipped.
     */
    fun applyToWebView(parent: View, webView: WebView) {
        val layout = scaledWebViewLayout(parent.width, parent.height) ?: return
        val lp = webView.layoutParams
        val needsSize =
            lp.width != layout.widthPx || lp.height != layout.heightPx
        if (needsSize) {
            lp.width = layout.widthPx
            lp.height = layout.heightPx
            webView.layoutParams = lp
        }
        webView.pivotX = 0f
        webView.pivotY = 0f
        if (webView.scaleX != layout.scale || webView.scaleY != layout.scale) {
            webView.scaleX = layout.scale
            webView.scaleY = layout.scale
        }
    }

    fun clearWebViewScale(webView: WebView) {
        val lp = webView.layoutParams
        if (lp.width != ViewGroup.LayoutParams.MATCH_PARENT ||
            lp.height != ViewGroup.LayoutParams.MATCH_PARENT
        ) {
            lp.width = ViewGroup.LayoutParams.MATCH_PARENT
            lp.height = ViewGroup.LayoutParams.MATCH_PARENT
            webView.layoutParams = lp
        }
        webView.scaleX = 1f
        webView.scaleY = 1f
    }
}

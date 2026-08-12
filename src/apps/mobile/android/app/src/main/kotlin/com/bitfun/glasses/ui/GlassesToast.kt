package com.bitfun.glasses.ui

import android.content.Context
import android.widget.Toast
import com.bitfun.glasses.BuildConfig
import com.ffalcon.mercury.android.sdk.ui.toast.FToast

/** Prefer Mercury FToast on vendor builds; fall back to system Toast. */
object GlassesToast {
    fun show(context: Context, message: String, longDuration: Boolean = false) {
        if (BuildConfig.USE_VENDOR_MERCURY) {
            try {
                FToast.show(message, longDuration)
                return
            } catch (_: Exception) {
                // Fall through to system Toast.
            }
        }
        val duration = if (longDuration) Toast.LENGTH_LONG else Toast.LENGTH_SHORT
        Toast.makeText(context.applicationContext, message, duration).show()
    }

    fun show(context: Context, resId: Int, longDuration: Boolean = false) {
        show(context, context.getString(resId), longDuration)
    }
}

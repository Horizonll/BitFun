package com.ffalcon.mercury.android.sdk.ui.toast

import android.widget.Toast
import com.ffalcon.mercury.android.sdk.MercurySDK

object FToast {
    @JvmOverloads
    fun show(message: String, longDuration: Boolean = false, gravity: Int = 0) {
        val app = runCatching { MercurySDK.mApplication }.getOrNull() ?: return
        val duration = if (longDuration) Toast.LENGTH_LONG else Toast.LENGTH_SHORT
        Toast.makeText(app, message, duration).show()
    }

    @JvmOverloads
    fun show(resId: Int, longDuration: Boolean = false, gravity: Int = 0) {
        val app = runCatching { MercurySDK.mApplication }.getOrNull() ?: return
        show(app.getString(resId), longDuration, gravity)
    }
}

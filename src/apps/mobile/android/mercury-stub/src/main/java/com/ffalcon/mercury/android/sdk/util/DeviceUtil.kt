package com.ffalcon.mercury.android.sdk.util

import android.os.Build

object DeviceUtil {
    const val TAG = "DeviceUtil"

    @JvmStatic
    fun isX3Device(): Boolean {
        val haystack =
            "${Build.MODEL} ${Build.BRAND} ${Build.MANUFACTURER} ${Build.PRODUCT}"
        return haystack.contains("rayneo", ignoreCase = true) ||
            haystack.contains("ffalcon", ignoreCase = true) ||
            Build.MODEL.contains("BRQ", ignoreCase = true)
    }
}

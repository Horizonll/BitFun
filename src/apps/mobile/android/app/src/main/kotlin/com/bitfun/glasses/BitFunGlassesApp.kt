package com.bitfun.glasses

import android.app.Application
import com.ffalcon.mercury.android.sdk.MercurySDK

class BitFunGlassesApp : Application() {
    override fun onCreate() {
        super.onCreate()
        MercurySDK.init(this)
    }
}

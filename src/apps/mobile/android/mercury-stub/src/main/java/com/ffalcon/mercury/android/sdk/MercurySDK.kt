package com.ffalcon.mercury.android.sdk

import android.app.Application

/**
 * Local stand-in for the vendor MercurySDK when `app/libs/mercury-sdk.aar`
 * is not present. Replace with the RayNeo AAR for glasses release builds.
 */
object MercurySDK {
    @Volatile
    lateinit var mApplication: Application
        private set

    private var initialized = false

    fun init(application: Application) {
        mApplication = application
        initialized = true
    }

    fun isInitialized(): Boolean = initialized
}

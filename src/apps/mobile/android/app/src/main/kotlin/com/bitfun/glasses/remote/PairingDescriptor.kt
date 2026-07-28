package com.bitfun.glasses.remote

/** Parsed Desktop remote-control pairing QR / URL fields used for validation. */
data class PairingDescriptor(
    val relayUrl: String,
    val roomId: String,
    val publicKey: String,
    val deviceId: String? = null,
    val deviceName: String? = null,
    val accountAuth: Boolean = false,
    val accountUsername: String? = null,
)

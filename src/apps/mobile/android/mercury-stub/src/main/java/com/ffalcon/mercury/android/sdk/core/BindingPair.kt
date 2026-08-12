package com.ffalcon.mercury.android.sdk.core

import androidx.viewbinding.ViewBinding

/**
 * Stub stand-in for MercurySDK BindingPair when the vendor AAR is absent.
 */
open class BaseMirrorAction<T>(
    val left: T,
    val right: T,
) {
    fun update(block: T.() -> Unit) {
        left.block()
        right.block()
    }

    fun updateView(block: T.() -> Unit) = update(block)

    fun setLeft(block: T.() -> Unit) {
        left.block()
    }

    fun getLeft(): T = left

    fun getRight(): T = right
}

class BindingPair<B : ViewBinding>(
    left: B,
    right: B,
) : BaseMirrorAction<B>(left, right) {
    /** Vendor API: true when [binding] is the left-eye ViewBinding instance. */
    fun checkIsLeft(binding: B): Boolean = binding === left
}

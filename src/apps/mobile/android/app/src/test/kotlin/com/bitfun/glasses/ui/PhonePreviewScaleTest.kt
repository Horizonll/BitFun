package com.bitfun.glasses.ui

import kotlin.math.roundToInt
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class PhonePreviewScaleTest {
    @Test
    fun scaledWebViewLayoutExpandsThenShrinksToParent() {
        // Unit tests run as debug/stub by default → enabled.
        val layout = PhonePreviewScale.scaledWebViewLayout(1080, 600)
        if (!PhonePreviewScale.enabled) {
            assertNull(layout)
            return
        }
        assertNotNull(layout)
        val sized = layout!!
        assertEquals(PhonePreviewScale.FACTOR, sized.scale, 0.0001f)
        assertEquals((1080 / PhonePreviewScale.FACTOR).roundToInt(), sized.widthPx)
        assertEquals((600 / PhonePreviewScale.FACTOR).roundToInt(), sized.heightPx)
        // Visual size after scale equals parent (allow 1px rounding).
        assertEquals(1080f, sized.widthPx * sized.scale, 1f)
        assertEquals(600f, sized.heightPx * sized.scale, 1f)
    }

    @Test
    fun scaledWebViewLayoutRejectsEmptyParent() {
        assertNull(PhonePreviewScale.scaledWebViewLayout(0, 600))
        assertNull(PhonePreviewScale.scaledWebViewLayout(1080, 0))
    }
}

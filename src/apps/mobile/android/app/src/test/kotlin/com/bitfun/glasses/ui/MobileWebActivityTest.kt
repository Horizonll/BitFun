package com.bitfun.glasses.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MobileWebActivityTest {
    @Test
    fun allowsHttpNavigation() {
        assertTrue(MobileWebActivity.isAllowedNavigation("http://192.168.1.2:9700/#/pair?room=1"))
        assertTrue(MobileWebActivity.isAllowedNavigation("https://remote.example/r/abc/#/pair"))
        assertTrue(MobileWebActivity.isAllowedNavigation("about:blank"))
    }

    @Test
    fun blocksNonWebSchemes() {
        assertFalse(MobileWebActivity.isAllowedNavigation("file:///sdcard/x.html"))
        assertFalse(MobileWebActivity.isAllowedNavigation("javascript:alert(1)"))
        assertFalse(MobileWebActivity.isAllowedNavigation("intent://scan"))
    }
}

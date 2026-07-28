package com.bitfun.glasses.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MobileWebAssetRouterTest {
    @Test
    fun lanRootMapsToIndex() {
        assertEquals(
            "glasses-web/index.html",
            MobileWebAssetRouter.assetPathForHttpUrl("http://192.168.1.2:9700/"),
        )
    }

    @Test
    fun lanAssetMapsThrough() {
        assertEquals(
            "glasses-web/assets/index-abc.js",
            MobileWebAssetRouter.assetPathForHttpUrl(
                "http://192.168.1.2:9700/assets/index-abc.js",
            ),
        )
    }

    @Test
    fun cloudRoomPrefixStripped() {
        assertEquals(
            "glasses-web/index.html",
            MobileWebAssetRouter.assetPathForHttpUrl("https://relay.example/r/room123/"),
        )
        assertEquals(
            "glasses-web/assets/x.css",
            MobileWebAssetRouter.assetPathForHttpUrl(
                "https://relay.example/r/room123/assets/x.css",
            ),
        )
    }

    @Test
    fun apiNotIntercepted() {
        assertNull(
            MobileWebAssetRouter.assetPathForHttpUrl(
                "http://192.168.1.2:9700/api/rooms/abc/command",
            ),
        )
    }
}

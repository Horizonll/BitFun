package com.bitfun.glasses.remote

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PairingUrlParserTest {
    @Test
    fun parseHashPairUrl() {
        val url =
            "https://relay.example.com/r/abc#/pair?room=room-1&did=desk-1&pk=PUBKEY&dn=Desktop&relay=wss://relay.example.com/ws&v=1"
        val descriptor = PairingUrlParser.parse(url)
        assertEquals("https://relay.example.com", descriptor.relayUrl)
        assertEquals("room-1", descriptor.roomId)
        assertEquals("PUBKEY", descriptor.publicKey)
        assertEquals("desk-1", descriptor.deviceId)
    }

    @Test
    fun parseQueryOnlyFallsBack() {
        val url = "room=r2&pk=KEY2&relay=https://relay.local/"
        val descriptor = PairingUrlParser.parse(url)
        assertEquals("https://relay.local", descriptor.relayUrl)
        assertEquals("r2", descriptor.roomId)
        assertEquals("KEY2", descriptor.publicKey)
    }

    @Test(expected = IllegalArgumentException::class)
    fun missingParamsThrows() {
        PairingUrlParser.parse("#/pair?room=only")
    }

    @Test
    fun emptyThrows() {
        try {
            PairingUrlParser.parse("  ")
            assertTrue(false)
        } catch (error: IllegalArgumentException) {
            assertTrue(error.message!!.contains("required"))
        }
    }

    @Test(expected = IllegalArgumentException::class)
    fun loopbackRelayRejected() {
        PairingUrlParser.parse(
            "#/pair?room=r1&pk=KEY&relay=ws://127.0.0.1:9700/ws",
        )
    }

    @Test
    fun accountAuthParsed() {
        val descriptor = PairingUrlParser.parse(
            "#/pair?room=r1&pk=KEY&relay=https://remote.example.com/relay&auth=account&user=alice",
        )
        assertTrue(descriptor.accountAuth)
        assertEquals("alice", descriptor.accountUsername)
        assertEquals("https://remote.example.com/relay", descriptor.relayUrl)
    }
}

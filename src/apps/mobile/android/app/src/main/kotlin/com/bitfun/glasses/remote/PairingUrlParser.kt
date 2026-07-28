package com.bitfun.glasses.remote

import java.net.URLDecoder
import java.nio.charset.StandardCharsets

object PairingUrlParser {
    fun parse(input: String): PairingDescriptor {
        val value = input.trim()
        require(value.isNotEmpty()) { "Pairing URL is required" }

        val query = extractQuery(value)
        val params = parseQuery(query)
        val roomId = params["room"].orEmpty()
        val publicKey = params["pk"].orEmpty()
        val relay = params["relay"].orEmpty()

        require(roomId.isNotEmpty() && publicKey.isNotEmpty()) {
            "Pairing URL missing room or pk"
        }

        val relayUrl = resolveRelayBaseUrl(value, relay)
        require(relayUrl.isNotBlank() && (relayUrl.startsWith("http://") || relayUrl.startsWith("https://"))) {
            "Could not resolve relay URL from pairing link"
        }
        require(!isLoopbackRelay(relayUrl)) {
            "Relay points to localhost ($relayUrl). Glasses cannot reach the PC loopback. " +
                "On Desktop switch remote mode to LAN (use PC LAN IP) or BitFun Server / cloud relay, then copy a new URL."
        }

        return PairingDescriptor(
            relayUrl = relayUrl,
            roomId = roomId,
            publicKey = publicKey,
            deviceId = params["did"],
            deviceName = params["dn"],
            accountAuth = params["auth"] == "account",
            accountUsername = params["user"]?.trim()?.takeIf { it.isNotEmpty() },
        )
    }

    fun isLoopbackRelay(relayUrl: String): Boolean {
        val host = runCatching { java.net.URI(relayUrl).host?.lowercase() }.getOrNull().orEmpty()
        return host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]"
    }

    private fun extractQuery(value: String): String {
        val pairIndex = value.indexOf("#/pair?")
        if (pairIndex >= 0) {
            return value.substring(pairIndex + "#/pair?".length)
        }
        val questionIndex = value.indexOf('?')
        if (questionIndex >= 0) {
            return value.substring(questionIndex + 1)
        }
        return value
    }

    private fun parseQuery(query: String): Map<String, String> {
        val params = linkedMapOf<String, String>()
        for (pair in query.split('&')) {
            if (pair.isEmpty()) continue
            val eq = pair.indexOf('=')
            val rawKey = if (eq >= 0) pair.substring(0, eq) else pair
            val rawValue = if (eq >= 0) pair.substring(eq + 1) else ""
            if (rawKey.isEmpty()) continue
            params[decode(rawKey)] = decode(rawValue)
        }
        return params
    }

    private fun resolveRelayBaseUrl(fullUrl: String, relayParam: String): String {
        val relay = relayParam
            .replace(Regex("^wss://"), "https://")
            .replace(Regex("^ws://"), "http://")
            .replace(Regex("/ws/?$"), "")
            .trimEnd('/')
        if (relay.isNotEmpty()) {
            return relay
        }

        val hashIndex = fullUrl.indexOf('#')
        val withoutHash = if (hashIndex >= 0) fullUrl.substring(0, hashIndex) else fullUrl
        val questionIndex = withoutHash.indexOf('?')
        val withoutQuery =
            if (questionIndex >= 0) withoutHash.substring(0, questionIndex) else withoutHash
        return withoutQuery
            .replace(Regex("/r/[^/]*$"), "")
            .replace(Regex("/[^/]*$"), "")
            .trimEnd('/')
    }

    private fun decode(value: String): String =
        URLDecoder.decode(value, StandardCharsets.UTF_8.name())
}

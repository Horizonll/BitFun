package com.bitfun.glasses.ui

import java.net.URI

/**
 * Maps relay-hosted SPA HTTP paths to APK asset paths under
 * `assets/glasses-web/`, so the glasses WebView can keep the relay origin
 * (same-origin API) while serving UI from the APK.
 */
object MobileWebAssetRouter {
    const val ASSET_ROOT = "glasses-web"

    fun assetPathFor(uri: android.net.Uri): String? = assetPathForHttpUrl(uri.toString())

    /**
     * @return asset path relative to assets/ (e.g. `glasses-web/index.html`),
     * or null when the request must go to the network (API / websocket / unknown).
     */
    fun assetPathForHttpUrl(url: String): String? {
        val uri = runCatching { URI(url) }.getOrNull() ?: return null
        var path = uri.path
        if (path.isNullOrEmpty()) path = "/"

        // Never intercept relay/control APIs or websocket upgrades.
        if (path == "/api" || path.startsWith("/api/") || path.startsWith("/ws")) {
            return null
        }

        // Cloud relay SPA paths are under /r/{roomId}/...
        val roomPrefix = Regex("^/r/[^/]+(/.*)?$").matchEntire(path)
        if (roomPrefix != null) {
            val rest = roomPrefix.groupValues.getOrNull(1).orEmpty()
            path = if (rest.isEmpty()) "/" else rest
        }

        val normalized = when {
            path == "/" || path.isEmpty() -> "/index.html"
            path.endsWith("/") -> "${path}index.html"
            looksLikeSpaRoute(path) -> "/index.html"
            else -> path
        }

        return ASSET_ROOT + normalized
    }

    fun mimeTypeFor(assetPath: String): String {
        val lower = assetPath.lowercase()
        return when {
            lower.endsWith(".html") -> "text/html"
            lower.endsWith(".js") -> "application/javascript"
            lower.endsWith(".css") -> "text/css"
            lower.endsWith(".png") -> "image/png"
            lower.endsWith(".jpg") || lower.endsWith(".jpeg") -> "image/jpeg"
            lower.endsWith(".svg") -> "image/svg+xml"
            lower.endsWith(".webp") -> "image/webp"
            lower.endsWith(".woff2") -> "font/woff2"
            lower.endsWith(".woff") -> "font/woff"
            lower.endsWith(".json") -> "application/json"
            lower.endsWith(".map") -> "application/json"
            else -> "application/octet-stream"
        }
    }

    private fun looksLikeSpaRoute(path: String): Boolean {
        val last = path.substringAfterLast('/')
        return !last.contains('.')
    }
}

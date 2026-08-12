package com.bitfun.glasses.ui

import android.content.Context
import java.util.UUID

/**
 * Process-wide facts for the glasses SPA (install id + language).
 * UI/session mirrors were removed when the right eye became a PixelCopy clone.
 */
object GlassesIdentityStore {
    private const val PREFS = "bitfun_glasses_identity"
    private const val KEY_INSTALL_ID = "install_id"
    private const val KEY_LANGUAGE = "language"

    fun getOrCreateInstallId(context: Context): String {
        val prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val existing = prefs.getString(KEY_INSTALL_ID, null)?.trim().orEmpty()
        if (existing.isNotEmpty()) return existing
        val created = UUID.randomUUID().toString()
        prefs.edit().putString(KEY_INSTALL_ID, created).apply()
        return created
    }

    fun getLanguage(context: Context): String? {
        val value = context.applicationContext
            .getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY_LANGUAGE, null)
            ?.trim()
            .orEmpty()
        return value.ifEmpty { null }
    }

    fun putLanguage(context: Context, language: String) {
        context.applicationContext
            .getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(KEY_LANGUAGE, language)
            .apply()
    }
}

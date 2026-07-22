package com.laniot.companion.pair

import android.content.Context
import android.content.SharedPreferences

/**
 * Persists Hub pairing credentials for the Companion.
 *
 * Flow (P5 auth stub):
 * 1. `POST {hub}/api/v1/auth/pair` → opaque Bearer `lit_…`
 * 2. Store token + Hub base URL here
 * 3. Attach `Authorization: Bearer …` on Companion→Hub calls when `AUTH_REQUIRED=true`
 *
 * Stub uses plain SharedPreferences; swap to EncryptedSharedPreferences before production.
 */
class PairTokenStore(context: Context) {

    private val prefs: SharedPreferences =
        context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun token(): String? = prefs.getString(KEY_TOKEN, null)

    fun hubBaseUrl(): String? = prefs.getString(KEY_HUB, null)

    fun save(hubBaseUrl: String, token: String) {
        prefs.edit()
            .putString(KEY_HUB, hubBaseUrl.trimEnd('/'))
            .putString(KEY_TOKEN, token)
            .apply()
    }

    fun clear() {
        prefs.edit().clear().apply()
    }

    companion object {
        private const val PREFS = "laniot_companion_pair"
        private const val KEY_TOKEN = "pair_token"
        private const val KEY_HUB = "hub_base_url"
    }
}

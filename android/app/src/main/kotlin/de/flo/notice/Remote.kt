package de.flo.notice

import android.content.Context

/**
 * Optionaler Mehrgeräte-Betrieb: statt des eingebauten Servers wird ein zentraler Notice-Server
 * (HTTPS + Token) verwendet. Alle Geräte sehen so denselben Datenstand.
 */
object Remote {
    private const val PREFS = "notice_prefs"

    private fun prefs(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun url(ctx: Context): String = prefs(ctx).getString("remote_url", "") ?: ""
    fun token(ctx: Context): String = prefs(ctx).getString("remote_token", "") ?: ""
    fun enabled(ctx: Context): Boolean = url(ctx).isNotBlank()

    fun set(ctx: Context, url: String, token: String) {
        prefs(ctx).edit().putString("remote_url", url.trim().trimEnd('/')).putString("remote_token", token.trim()).apply()
    }

    /** Klartext-HTTP ist nur lokal erlaubt (network_security_config) – Remote muss HTTPS sein. */
    fun valid(url: String): Boolean = url.trim().startsWith("https://") && url.trim().length > 8
}

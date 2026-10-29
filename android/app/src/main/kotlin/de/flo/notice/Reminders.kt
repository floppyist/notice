package de.flo.notice

import android.Manifest
import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId

/**
 * Erinnerungen für Termine (und Fälligkeiten) als Benachrichtigung.
 *
 * Die geplanten Erinnerungen werden zusätzlich in SharedPreferences gehalten, damit sie nach einem
 * Neustart (BootReceiver) ohne laufenden Server neu geplant werden können.
 */
object Reminders {
    const val CHANNEL = "reminders"
    private const val PREFS = "notice_prefs"
    private const val KEY_LEAD = "lead_min"
    private const val KEY_ITEMS = "reminders"
    private const val MAX_ALARMS = 60
    /** Uhrzeit für Termine/Fälligkeiten ohne Uhrzeit. */
    private val ALL_DAY = LocalTime.of(8, 0)

    val LEAD_OPTIONS = listOf(0, 5, 15, 30, 60)

    private class Item(val at: Long, val title: String, val body: String) {
        val id: Int get() = (at.toString() + title).hashCode()
    }

    fun leadMinutes(ctx: Context): Int = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getInt(KEY_LEAD, 15)

    fun setLeadMinutes(ctx: Context, minutes: Int) {
        ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putInt(KEY_LEAD, minutes).apply()
    }

    fun ensureChannel(ctx: Context) {
        if (Build.VERSION.SDK_INT < 26) return
        val nm = ctx.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, ctx.getString(R.string.reminders), NotificationManager.IMPORTANCE_HIGH),
        )
    }

    fun hasPermission(ctx: Context): Boolean =
        Build.VERSION.SDK_INT < 33 || ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    /** Berechnet alle künftigen Erinnerungen aus den Notizen und plant sie neu. */
    fun sync(ctx: Context, notes: List<Note>) {
        val zone = ZoneId.systemDefault()
        val now = System.currentTimeMillis()
        val lead = leadMinutes(ctx)
        val items = mutableListOf<Item>()
        for (n in notes) {
            if (n.status == "done" || n.status == "archived") continue
            for (a in n.appointments) {
                if (a.done || a.start.isBlank()) continue
                val date = runCatching { LocalDate.parse(a.start) }.getOrNull() ?: continue
                val hasTime = a.time.isNotBlank()
                val time = if (hasTime) runCatching { LocalTime.parse(a.time) }.getOrNull() ?: ALL_DAY else ALL_DAY
                val start = LocalDateTime.of(date, time)
                val fire = if (hasTime) start.minusMinutes(lead.toLong()) else start
                val body = if (hasTime) a.time + (if (lead > 0) " · " + ctx.getString(R.string.reminder_in, lead) else "") else ctx.getString(R.string.reminder_today)
                items += Item(fire.atZone(zone).toInstant().toEpochMilli(), a.title.ifBlank { n.title }, if (a.title.isBlank()) body else "${n.title} · $body")
            }
            val due = n.dueDate?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
            if (due != null) {
                items += Item(LocalDateTime.of(due, ALL_DAY).atZone(zone).toInstant().toEpochMilli(), n.title, ctx.getString(R.string.reminder_due))
            }
        }
        val upcoming = items.filter { it.at > now }.sortedBy { it.at }.take(MAX_ALARMS)
        val arr = JSONArray()
        upcoming.forEach { arr.put(JSONObject().put("at", it.at).put("title", it.title).put("body", it.body)) }
        val prefs = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        cancelAll(ctx, prefs.getString(KEY_ITEMS, null))
        prefs.edit().putString(KEY_ITEMS, arr.toString()).apply()
        upcoming.forEach { schedule(ctx, it) }
    }

    /** Nach Neustart: aus dem Zwischenspeicher neu planen. */
    fun restore(ctx: Context) {
        val raw = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_ITEMS, null) ?: return
        val now = System.currentTimeMillis()
        parse(raw).filter { it.at > now }.forEach { schedule(ctx, it) }
    }

    private fun parse(raw: String?): List<Item> = try {
        val a = JSONArray(raw ?: "[]")
        (0 until a.length()).map { val o = a.getJSONObject(it); Item(o.getLong("at"), o.getString("title"), o.optString("body")) }
    } catch (e: Exception) {
        emptyList()
    }

    private fun pending(ctx: Context, item: Item): PendingIntent {
        val i = Intent(ctx, ReminderReceiver::class.java)
            .putExtra("title", item.title).putExtra("body", item.body).putExtra("id", item.id)
        return PendingIntent.getBroadcast(ctx, item.id, i, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    }

    private fun cancelAll(ctx: Context, raw: String?) {
        val am = ctx.getSystemService(AlarmManager::class.java)
        parse(raw).forEach { am.cancel(pending(ctx, it)) }
    }

    private fun schedule(ctx: Context, item: Item) {
        val am = ctx.getSystemService(AlarmManager::class.java)
        val pi = pending(ctx, item)
        val exact = Build.VERSION.SDK_INT < 31 || am.canScheduleExactAlarms()
        try {
            if (exact) am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, item.at, pi)
            else am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, item.at, pi)
        } catch (e: SecurityException) {
            am.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, item.at, pi)
        }
    }

    fun show(ctx: Context, id: Int, title: String, body: String) {
        if (!hasPermission(ctx)) return
        ensureChannel(ctx)
        val open = PendingIntent.getActivity(
            ctx, 0, Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val n = NotificationCompat.Builder(ctx, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_popup_reminder)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setAutoCancel(true)
            .setContentIntent(open)
            .build()
        try {
            NotificationManagerCompat.from(ctx).notify(id, n)
        } catch (e: SecurityException) {
        }
    }
}

class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        Reminders.show(ctx, intent.getIntExtra("id", 0), intent.getStringExtra("title") ?: "", intent.getStringExtra("body") ?: "")
    }
}

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        Reminders.restore(ctx)
    }
}

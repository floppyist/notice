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
    private const val KEY_ITEMS = "reminders"
    private const val MAX_ALARMS = 200
    /** Uhrzeit für Termine/Fälligkeiten ohne Uhrzeit. */
    private val ALL_DAY = LocalTime.of(8, 0)

    /** Vorwarnungen für Termine mit Uhrzeit (Minuten vor Beginn); 0 = zum Beginn. Zusätzlich: Vortag um [EVE]. */
    private val LEADS = listOf(60L, 15L, 0L)
    /** Uhrzeit der Vortags-Erinnerung. */
    private val EVE = LocalTime.of(18, 0)
    private const val SNOOZE_MIN = 10L
    const val ACTION_SNOOZE = "de.flo.notice.SNOOZE"
    const val ACTION_DISMISS = "de.flo.notice.DISMISS"

    /** [key] identifiziert den Termin (alle Stufen teilen sich eine Benachrichtigung, die spätere ersetzt die frühere). */
    private class Item(val at: Long, val title: String, val body: String, val key: Int) {
        val requestCode: Int get() = (at.toString() + title + key).hashCode()
    }

    fun ensureChannel(ctx: Context) {
        if (Build.VERSION.SDK_INT < 26) return
        val nm = ctx.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(
            NotificationChannel(CHANNEL, ctx.getString(R.string.reminders), NotificationManager.IMPORTANCE_HIGH).apply {
                enableVibration(true)
                lockscreenVisibility = android.app.Notification.VISIBILITY_PUBLIC
            },
        )
    }

    fun hasPermission(ctx: Context): Boolean =
        Build.VERSION.SDK_INT < 33 || ContextCompat.checkSelfPermission(ctx, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    /** Berechnet alle künftigen Erinnerungen aus den Notizen und plant sie neu. */
    fun sync(ctx: Context, notes: List<Note>) {
        val zone = ZoneId.systemDefault()
        val now = System.currentTimeMillis()
        val items = mutableListOf<Item>()
        fun at(dt: LocalDateTime) = dt.atZone(zone).toInstant().toEpochMilli()
        for (n in notes) {
            if (n.status == "done" || n.status == "archived") continue
            n.appointments.forEachIndexed { idx, a ->
                if (a.done || a.start.isBlank()) return@forEachIndexed
                val date = runCatching { LocalDate.parse(a.start) }.getOrNull() ?: return@forEachIndexed
                val hasTime = a.time.isNotBlank()
                val time = if (hasTime) runCatching { LocalTime.parse(a.time) }.getOrNull() ?: ALL_DAY else ALL_DAY
                val start = LocalDateTime.of(date, time)
                val title = a.title.ifBlank { n.title }
                val key = listOf(n.id, idx, a.start, a.time, a.title).hashCode()
                fun add(fire: LocalDateTime, body: String) {
                    items += Item(at(fire), title, if (a.title.isBlank()) body else "${n.title} · $body", key)
                }
                // Vortag
                add(LocalDateTime.of(date.minusDays(1), EVE), ctx.getString(if (hasTime) R.string.reminder_tomorrow_at else R.string.reminder_tomorrow, a.time))
                if (hasTime) {
                    for (m in LEADS) {
                        val body = when (m) {
                            0L -> ctx.getString(R.string.reminder_now, a.time)
                            60L -> ctx.getString(R.string.reminder_in_hour, a.time)
                            else -> ctx.getString(R.string.reminder_in_min, m.toInt(), a.time)
                        }
                        add(start.minusMinutes(m), body)
                    }
                } else {
                    add(start, ctx.getString(R.string.reminder_today))
                }
            }
            val due = n.dueDate?.let { runCatching { LocalDate.parse(it) }.getOrNull() }
            if (due != null) {
                val key = listOf(n.id, "due", n.dueDate).hashCode()
                items += Item(at(LocalDateTime.of(due.minusDays(1), EVE)), n.title, ctx.getString(R.string.reminder_due_tomorrow), key)
                items += Item(at(LocalDateTime.of(due, ALL_DAY)), n.title, ctx.getString(R.string.reminder_due), key)
            }
        }
        val upcoming = items.filter { it.at > now }.sortedBy { it.at }.take(MAX_ALARMS)
        val arr = JSONArray()
        upcoming.forEach { arr.put(JSONObject().put("at", it.at).put("title", it.title).put("body", it.body).put("key", it.key)) }
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
        (0 until a.length()).map { val o = a.getJSONObject(it); Item(o.getLong("at"), o.getString("title"), o.optString("body"), o.optInt("key")) }
    } catch (e: Exception) {
        emptyList()
    }

    private fun pending(ctx: Context, item: Item): PendingIntent {
        val i = Intent(ctx, ReminderReceiver::class.java)
            .putExtra("title", item.title).putExtra("body", item.body).putExtra("id", item.key)
        return PendingIntent.getBroadcast(ctx, item.requestCode, i, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
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

    /** Erinnerung nach [SNOOZE_MIN] Minuten erneut anzeigen. */
    fun snooze(ctx: Context, id: Int, title: String, body: String) {
        NotificationManagerCompat.from(ctx).cancel(id)
        schedule(ctx, Item(System.currentTimeMillis() + SNOOZE_MIN * 60_000, title, body, id))
    }

    private fun action(ctx: Context, act: String, id: Int, title: String, body: String, code: Int): PendingIntent =
        PendingIntent.getBroadcast(
            ctx, code,
            Intent(ctx, ReminderReceiver::class.java).setAction(act).putExtra("id", id).putExtra("title", title).putExtra("body", body),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

    fun canScheduleExact(ctx: Context): Boolean =
        Build.VERSION.SDK_INT < 31 || ctx.getSystemService(AlarmManager::class.java).canScheduleExactAlarms()

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
            // Bleibt stehen, bis der Nutzer quittiert (OK) oder verschiebt (Später).
            .setOngoing(true)
            .setAutoCancel(false)
            .setOnlyAlertOnce(false)
            .setWhen(System.currentTimeMillis())
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setContentIntent(open)
            .addAction(0, ctx.getString(R.string.reminder_ok), action(ctx, ACTION_DISMISS, id, title, body, id * 31 + 1))
            .addAction(0, ctx.getString(R.string.reminder_snooze, SNOOZE_MIN.toInt()), action(ctx, ACTION_SNOOZE, id, title, body, id * 31 + 2))
            .build()
        try {
            NotificationManagerCompat.from(ctx).notify(id, n)
        } catch (e: SecurityException) {
        }
    }
}

class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        val id = intent.getIntExtra("id", 0)
        val title = intent.getStringExtra("title") ?: ""
        val body = intent.getStringExtra("body") ?: ""
        when (intent.action) {
            Reminders.ACTION_DISMISS -> NotificationManagerCompat.from(ctx).cancel(id)
            Reminders.ACTION_SNOOZE -> Reminders.snooze(ctx, id, title, body)
            else -> Reminders.show(ctx, id, title, body)
        }
    }
}

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        Reminders.restore(ctx)
    }
}

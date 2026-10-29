package de.flo.notice

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.time.LocalDate

/** YYYY-MM-DD → TT.MM.JJJJ */
fun fmtDate(d: String?): String {
    if (d.isNullOrBlank() || d.length < 10) return d ?: ""
    return "${d.substring(8, 10)}.${d.substring(5, 7)}.${d.substring(0, 4)}"
}

@Composable
fun statusLabel(s: String): String = when (s) {
    "backlog" -> stringResource(R.string.status_backlog)
    "in_progress" -> stringResource(R.string.status_in_progress)
    "review" -> stringResource(R.string.status_review)
    "done" -> stringResource(R.string.status_done)
    else -> s
}

@Composable
fun priorityLabel(p: String): String = when (p) {
    "high" -> stringResource(R.string.prio_high)
    "medium" -> stringResource(R.string.prio_medium)
    "low" -> stringResource(R.string.prio_low)
    else -> p
}

val STATUSES = listOf("backlog", "in_progress", "review", "done")

/** Kompakte Zeile für Listen: Statusbalken, Titel, Fälligkeit. */
@Composable
fun NoteLine(n: Note, onClick: () -> Unit, showStatus: Boolean = false) {
    Tap(Modifier.fillMaxWidth().padding(bottom = 6.dp), onClick = onClick) {
        BarRow(C.status(n.status), bg = C.surface) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (n.pinned) Tx("★", color = C.yellow, size = 12)
                    Tx(n.title, size = 14, maxLines = 2, modifier = Modifier.weight(1f))
                    if (n.priority == "high") Tx("!", color = C.red, size = 14, bold = true)
                }
                val sub = buildList {
                    if (showStatus) add(Pair(n.status, true))
                    if (n.dueDate != null) add(Pair(fmtDate(n.dueDate), false))
                }
                if (sub.isNotEmpty()) {
                    Row(Modifier.padding(top = 2.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (showStatus) Tx(statusLabel(n.status), color = C.status(n.status), size = 11)
                        if (n.dueDate != null) Tx(
                            (if (n.overdue) "! " else "") + fmtDate(n.dueDate),
                            color = if (n.overdue) C.red else if (n.dueToday) C.orange else C.muted,
                            size = 11,
                        )
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun Tags(items: List<String>, color: Color = C.aqua) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        items.forEach { Tag(it, color) }
    }
}

@Composable
fun Empty(text: String) {
    Box(Modifier.fillMaxWidth().padding(vertical = 24.dp), contentAlignment = Alignment.Center) { Tx(text, color = C.muted, size = 12) }
}

/** Ereignisse für Dashboard und Kalender. */
data class CalEvent(val date: String, val title: String, val note: Note, val kind: Kind, val time: String = "") {
    enum class Kind { Due, Overdue, Appt }
}

fun calendarEvents(notes: List<Note>): List<CalEvent> {
    val out = mutableListOf<CalEvent>()
    notes.forEach { n ->
        if (n.status == "archived") return@forEach
        if (n.dueDate != null && n.status != "done") {
            out += CalEvent(n.dueDate, n.title, n, if (n.overdue) CalEvent.Kind.Overdue else CalEvent.Kind.Due)
        }
        n.appointments.forEach { a ->
            if (a.start.length < 10) return@forEach
            val s = runCatching { LocalDate.parse(a.start) }.getOrNull() ?: return@forEach
            val e = runCatching { LocalDate.parse(a.end) }.getOrNull()?.takeIf { it >= s } ?: s
            var d = s
            var guard = 0
            while (d <= e && guard++ < 60) {
                out += CalEvent(d.toString(), a.title.ifBlank { n.title }, n, CalEvent.Kind.Appt, if (d == s) a.time else "")
                d = d.plusDays(1)
            }
        }
    }
    return out
}

fun eventColor(k: CalEvent.Kind): Color = when (k) {
    CalEvent.Kind.Due -> C.blue
    CalEvent.Kind.Overdue -> C.red
    CalEvent.Kind.Appt -> C.aqua
}

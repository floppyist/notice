package de.flo.notice

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp

@Composable
fun DashboardScreen(store: Store, nav: Nav) {
    val notes = store.notes
    val overdue = notes.filter { it.overdue }
    val dueToday = notes.filter { it.dueToday }
    val count = { s: String -> notes.count { it.status == s } }
    val t = today()
    val upcoming = calendarEvents(notes).filter { it.date >= t && it.kind != CalEvent.Kind.Overdue }.sortedWith(compareBy({ it.date }, { it.time })).take(8)

    Column(Modifier.fillMaxSize()) {
        TopBar(stringResource(R.string.tab_today), actions = {
            BarAction(stringResource(R.string.new_note), primary = true) { nav.push(Page.NoteEdit(null)) }
        })
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 4.dp)) {
            Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Stat(stringResource(R.string.stat_overdue), overdue.size, C.red, Modifier.weight(1f))
                Stat(stringResource(R.string.stat_today), dueToday.size, C.orange, Modifier.weight(1f))
                Stat(stringResource(R.string.status_done), count("done"), C.green, Modifier.weight(1f))
            }

            SectionTitle(stringResource(R.string.stat_overdue), right = overdue.size.toString())
            if (overdue.isEmpty()) Empty(stringResource(R.string.none_overdue))
            overdue.take(8).forEach { n -> NoteLine(n, { nav.push(Page.NoteEdit(n)) }, showStatus = true) }

            SectionTitle(stringResource(R.string.stat_today), right = dueToday.size.toString())
            if (dueToday.isEmpty()) Empty(stringResource(R.string.none_today))
            dueToday.take(8).forEach { n -> NoteLine(n, { nav.push(Page.NoteEdit(n)) }, showStatus = true) }

            SectionTitle(stringResource(R.string.upcoming))
            if (upcoming.isEmpty()) Empty(stringResource(R.string.none_upcoming))
            upcoming.forEach { e ->
                Tap(Modifier.fillMaxWidth().padding(bottom = 6.dp), onClick = { nav.push(Page.NoteEdit(e.note)) }) {
                    BarRow(eventColor(e.kind), bg = C.surface) {
                        Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            Tx(fmtDate(e.date).take(6) + if (e.time.isNotBlank()) " ${e.time}" else "", color = C.subtle, size = 12)
                            Tx(e.title, size = 13, maxLines = 1, modifier = Modifier.weight(1f))
                        }
                    }
                }
            }

            SectionTitle(stringResource(R.string.status))
            Panel {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    val total = notes.size.coerceAtLeast(1)
                    STATUSES.forEach { s ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Tx(statusLabel(s), color = C.subtle, size = 12, modifier = Modifier.padding(end = 8.dp).fillMaxWidth(0.3f))
                            Box(Modifier.weight(1f).height(10.dp).background(C.bg)) {
                                Box(Modifier.fillMaxHeight().fillMaxWidth(count(s).toFloat() / total).background(C.status(s)))
                            }
                            Tx(count(s).toString(), size = 12, modifier = Modifier.padding(start = 8.dp))
                        }
                    }
                }
            }
            Box(Modifier.height(16.dp))
        }
    }
}

@Composable
private fun Stat(label: String, value: Int, color: Color, modifier: Modifier = Modifier) {
    BarRow(if (value > 0) color else C.overlay, modifier, bg = C.surface) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp)) {
            Tx(value.toString(), color = if (value > 0) color else C.muted, size = 24, bold = true)
            Tx(label.uppercase(), color = C.subtle, size = 10, spacing = 1, maxLines = 1)
        }
    }
}

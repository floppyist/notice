package de.flo.notice

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.intl.Locale
import androidx.compose.ui.unit.dp
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.YearMonth
import java.time.format.TextStyle as DTextStyle

@Composable
fun CalendarScreen(store: Store, nav: Nav) {
    var month by remember { mutableStateOf(YearMonth.now()) }
    var selected by remember { mutableStateOf(LocalDate.now()) }
    val events = remember(store.notes) { calendarEvents(store.notes).groupBy { it.date } }
    val loc = java.util.Locale.getDefault()
    val first = month.atDay(1)
    val lead = (first.dayOfWeek.value - DayOfWeek.MONDAY.value + 7) % 7
    val cells = lead + month.lengthOfMonth()
    val rows = (cells + 6) / 7

    Column(Modifier.fillMaxSize()) {
        TopBar(stringResource(R.string.tab_calendar), actions = {
            BarAction(stringResource(R.string.today)) { month = YearMonth.now(); selected = LocalDate.now() }
            BarAction(stringResource(R.string.new_note), primary = true) { nav.push(Page.NoteEdit(Note(dueDate = selected.toString()))) }
        })
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(12.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(44.dp).background(C.surface).clickable { month = month.minusMonths(1) }, contentAlignment = Alignment.Center) { Tx("‹", color = C.accentText, size = 20, bold = true) }
                Tx(
                    month.month.getDisplayName(DTextStyle.FULL, loc) + " " + month.year,
                    bold = true, size = 14, modifier = Modifier.weight(1f).padding(horizontal = 8.dp),
                )
                Box(Modifier.size(44.dp).background(C.surface).clickable { month = month.plusMonths(1) }, contentAlignment = Alignment.Center) { Tx("›", color = C.accentText, size = 20, bold = true) }
            }
            Row(Modifier.fillMaxWidth().padding(top = 10.dp, bottom = 4.dp)) {
                for (i in 0 until 7) {
                    Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                        Tx(DayOfWeek.of(i + 1).getDisplayName(DTextStyle.SHORT, loc).take(2).uppercase(), color = C.muted, size = 10, bold = true)
                    }
                }
            }
            for (r in 0 until rows) {
                Row(Modifier.fillMaxWidth().padding(bottom = 2.dp), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                    for (c in 0 until 7) {
                        val dayNum = r * 7 + c - lead + 1
                        if (dayNum < 1 || dayNum > month.lengthOfMonth()) {
                            Box(Modifier.weight(1f).height(50.dp))
                        } else {
                            val date = month.atDay(dayNum)
                            val ds = date.toString()
                            val evs = events[ds].orEmpty()
                            val isSel = date == selected
                            val isToday = date == LocalDate.now()
                            Column(
                                Modifier.weight(1f).height(50.dp)
                                    .background(if (isSel) C.surface else C.bg)
                                    .clickable { selected = date },
                            ) {
                                Box(Modifier.fillMaxWidth().height(3.dp).background(if (isSel) C.accent else C.overlay))
                                Box(Modifier.fillMaxWidth().padding(top = 4.dp), contentAlignment = Alignment.Center) {
                                    Box(Modifier.background(if (isToday) C.accent else androidx.compose.ui.graphics.Color.Transparent).padding(horizontal = 5.dp)) {
                                        Tx(dayNum.toString(), color = if (isToday) C.onAccent else C.fg, size = 12, bold = isToday || isSel)
                                    }
                                }
                                Row(Modifier.fillMaxWidth().padding(top = 4.dp), horizontalArrangement = Arrangement.Center) {
                                    evs.map { it.kind }.distinct().take(3).forEach { Box(Modifier.padding(horizontal = 1.dp).size(6.dp).background(eventColor(it))) }
                                }
                            }
                        }
                    }
                }
            }
            SectionTitle(fmtDate(selected.toString()))
            val dayEvents = events[selected.toString()].orEmpty().sortedBy { it.time }
            if (dayEvents.isEmpty()) Empty(stringResource(R.string.nothing_on_day))
            dayEvents.forEach { e ->
                Tap(Modifier.fillMaxWidth().padding(bottom = 6.dp), onClick = { nav.push(Page.NoteEdit(e.note)) }) {
                    BarRow(eventColor(e.kind), bg = C.surface) {
                        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp)) {
                            Tx(e.title, size = 13, maxLines = 2)
                            Tx(
                                when (e.kind) {
                                    CalEvent.Kind.Appt -> "◆ " + stringResource(R.string.appointment) + if (e.time.isNotBlank()) " ${e.time}" else ""
                                    CalEvent.Kind.Overdue -> "! " + stringResource(R.string.stat_overdue)
                                    CalEvent.Kind.Due -> stringResource(R.string.due)
                                },
                                color = eventColor(e.kind), size = 11,
                            )
                        }
                    }
                }
            }
        }
    }
}

package de.flo.notice

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp

private enum class Filter { All, Overdue, Due, NoDate }

@Composable
fun BoardScreen(store: Store, nav: Nav) {
    var status by rememberSaveable { mutableStateOf("in_progress") }
    var filter by rememberSaveable { mutableStateOf(Filter.All) }
    var dept by rememberSaveable { mutableStateOf("") }
    var query by rememberSaveable { mutableStateOf("") }
    var sheetFor by remember { mutableStateOf<Note?>(null) }
    var deptSheet by remember { mutableStateOf(false) }

    val visible = store.notes.filter { n ->
        val q = query.trim().lowercase()
        (q.isEmpty() || n.title.lowercase().contains(q) || n.text.lowercase().contains(q)) &&
            (dept.isEmpty() || n.departments.any { it.equals(dept, true) }) &&
            when (filter) {
                Filter.All -> true
                Filter.Overdue -> n.overdue
                Filter.Due -> n.dueDate != null && n.status != "done" && !n.overdue
                Filter.NoDate -> n.dueDate == null
            }
    }
    val shown = visible.filter { it.status == status }

    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            TopBar(stringResource(R.string.tab_board), actions = {
                BarAction(stringResource(R.string.new_note), primary = true) { nav.push(Page.NoteEdit(Note(status = status))) }
            })
            Column(Modifier.padding(horizontal = 12.dp).padding(top = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Field(query, { query = it }, placeholder = stringResource(R.string.search))
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Chip(stringResource(R.string.filter_all), filter == Filter.All, { filter = Filter.All })
                    Chip(stringResource(R.string.filter_overdue), filter == Filter.Overdue, { filter = Filter.Overdue })
                    Chip(stringResource(R.string.filter_due), filter == Filter.Due, { filter = Filter.Due })
                    Chip(stringResource(R.string.filter_nodate), filter == Filter.NoDate, { filter = Filter.NoDate })
                    Chip(if (dept.isEmpty()) stringResource(R.string.all_depts) else "▸ $dept", dept.isNotEmpty(), { deptSheet = true })
                }
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    STATUSES.forEach { s ->
                        Chip("${statusLabel(s)} ${visible.count { it.status == s }}", status == s, { status = s }, dot = C.status(s))
                    }
                }
            }
            LazyColumn(Modifier.weight(1f).padding(horizontal = 12.dp).padding(top = 10.dp)) {
                if (shown.isEmpty()) item { Empty(stringResource(R.string.no_notes)) }
                items(shown, key = { it.id }) { n -> NoteCard(n, onClick = { nav.push(Page.NoteEdit(n)) }, onLong = { sheetFor = n }) }
            }
        }
        sheetFor?.let { n ->
            val dismiss = { sheetFor = null }
            val items = STATUSES.map { s ->
                SheetItem(statusLabel(s), dot = C.status(s), current = n.status == s) { store.setStatus(n, s); dismiss() }
            } + listOf(
                SheetItem(stringResource(if (n.pinned) R.string.unpin else R.string.pin)) { store.togglePin(n); dismiss() },
                SheetItem(stringResource(R.string.duplicate)) { store.act { duplicateNote(n.id) }; dismiss() },
                SheetItem(stringResource(R.string.archive)) { store.act { archiveNote(n.id) }; dismiss() },
                SheetItem(stringResource(R.string.delete), color = C.red) { store.act { deleteNote(n.id) }; dismiss() },
            )
            Sheet(n.title, items, dismiss)
        }
        if (deptSheet) {
            val dismiss = { deptSheet = false }
            val items = listOf(SheetItem(stringResource(R.string.all_depts), current = dept.isEmpty()) { dept = ""; dismiss() }) +
                store.depts.map { d -> SheetItem("${d.name} (${d.count})", current = dept == d.name) { dept = d.name; dismiss() } }
            Sheet(stringResource(R.string.departments), items, dismiss)
        }
    }
}

@Composable
private fun NoteCard(n: Note, onClick: () -> Unit, onLong: () -> Unit) {
    Tap(Modifier.fillMaxWidth().padding(bottom = 8.dp), onLong = onLong, onClick = onClick) {
        BarRow(C.status(n.status), bg = C.surface) {
            Column(Modifier.fillMaxWidth().padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (n.pinned) Tx("★", color = C.yellow, size = 13)
                    Tx(n.title, size = 14, bold = true, modifier = Modifier.weight(1f))
                    if (n.priority != "low") Badge(priorityLabelShort(n.priority), C.priority(n.priority))
                }
                if (n.dueDate != null) {
                    Tx(
                        (if (n.overdue) "! " else "") + fmtDate(n.dueDate) + if (n.repeat != null) "  ↻" else "",
                        color = if (n.overdue) C.red else if (n.dueToday) C.orange else C.subtle, size = 12,
                    )
                }
                val done = n.checklist.count { it.done }
                if (n.checklist.isNotEmpty() || n.appointments.isNotEmpty()) {
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        if (n.checklist.isNotEmpty()) Tx("✓ $done/${n.checklist.size}", color = if (done == n.checklist.size) C.green else C.subtle, size = 11)
                        if (n.appointments.isNotEmpty()) Tx("◆ ${n.appointments.size}", color = C.aqua, size = 11)
                    }
                }
                if (n.departments.isNotEmpty()) Tags(n.departments)
            }
        }
    }
}

@Composable
private fun priorityLabelShort(p: String): String = priorityLabel(p).uppercase()

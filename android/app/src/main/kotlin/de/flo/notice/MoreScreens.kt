package de.flo.notice

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.json.JSONObject

@Composable
private fun MenuRow(label: String, hint: String? = null, color: androidx.compose.ui.graphics.Color = C.fg, onClick: () -> Unit) {
    Tap(Modifier.fillMaxWidth().padding(bottom = 6.dp), onClick = onClick) {
        BarRow(C.overlay, bg = C.surface) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                Tx(label, color = color, size = 14, modifier = Modifier.weight(1f))
                if (hint != null) Tx(hint, color = C.muted, size = 12)
                Tx("  ›", color = C.accentText, size = 14, bold = true)
            }
        }
    }
}

@Composable
fun MoreScreen(store: Store, nav: Nav) {
    Column(Modifier.fillMaxSize()) {
        TopBar(stringResource(R.string.tab_more))
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(12.dp)) {
            MenuRow(stringResource(R.string.contacts), store.contacts.size.toString()) { nav.push(Page.Contacts) }
            MenuRow(stringResource(R.string.departments), store.depts.size.toString()) { nav.push(Page.Depts) }
            MenuRow(stringResource(R.string.archive_title), store.archive.size.toString()) { nav.push(Page.Archive) }
            MenuRow(stringResource(R.string.trash), (store.trash.notes.size + store.trash.contacts.size).toString()) { nav.push(Page.Trash) }
            MenuRow(stringResource(R.string.settings)) { nav.push(Page.Settings) }
            Tx("NOTICE ${BuildConfigVersion}", color = C.muted, size = 11, modifier = Modifier.padding(top = 20.dp))
        }
    }
}

const val BuildConfigVersion = "2.0"

// ───────────── Kontakte ─────────────

@Composable
fun ContactsScreen(store: Store, nav: Nav) {
    var query by rememberSaveable { mutableStateOf("") }
    val q = query.trim().lowercase()
    val list = store.contacts.filter { q.isEmpty() || it.name.lowercase().contains(q) || it.email.lowercase().contains(q) }
    Column(Modifier.fillMaxSize()) {
        TopBar(stringResource(R.string.contacts), back = { nav.pop() }, actions = {
            BarAction(stringResource(R.string.new_note), primary = true) { nav.push(Page.ContactEdit(null)) }
        })
        Box(Modifier.padding(12.dp)) { Field(query, { query = it }, placeholder = stringResource(R.string.search)) }
        LazyColumn(Modifier.weight(1f).padding(horizontal = 12.dp)) {
            if (list.isEmpty()) item { Empty(stringResource(R.string.no_contacts)) }
            items(list, key = { it.id }) { c ->
                Tap(Modifier.fillMaxWidth().padding(bottom = 6.dp), onClick = { nav.push(Page.ContactEdit(c)) }) {
                    BarRow(C.purple, bg = C.surface) {
                        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp)) {
                            Tx(c.name, size = 14, maxLines = 1)
                            val sub = listOf(c.title, c.mobile.ifBlank { c.phone }, c.email).filter { it.isNotBlank() }.joinToString("  ·  ")
                            if (sub.isNotBlank()) Tx(sub, color = C.subtle, size = 11, maxLines = 1)
                            if (c.departments.isNotEmpty()) Box(Modifier.padding(top = 4.dp)) { Tags(c.departments) }
                        }
                    }
                }
            }
        }
    }
}

// ───────────── Einrichtungen ─────────────

@Composable
fun DeptsScreen(store: Store, nav: Nav) {
    var name by remember { mutableStateOf("") }
    var confirm by remember { mutableStateOf<Dept?>(null) }
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            TopBar(stringResource(R.string.departments), back = { nav.pop() })
            Row(Modifier.padding(12.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                Field(name, { name = it }, modifier = Modifier.weight(1f), placeholder = stringResource(R.string.dept_new))
                Btn(stringResource(R.string.create), { if (name.isNotBlank()) { val n = name.trim(); name = ""; store.act { addDepartment(n) } } }, primary = true)
            }
            LazyColumn(Modifier.weight(1f).padding(horizontal = 12.dp)) {
                if (store.depts.isEmpty()) item { Empty(stringResource(R.string.no_depts)) }
                items(store.depts, key = { it.name }) { d ->
                    BarRow(C.orange, Modifier.fillMaxWidth().padding(bottom = 6.dp), bg = C.surface) {
                        Row(Modifier.fillMaxWidth().padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                            Tx(d.name, size = 14, modifier = Modifier.weight(1f))
                            Tx(stringResource(R.string.dept_used, d.count), color = C.muted, size = 11)
                            Box(Modifier.size(44.dp).clickable { confirm = d }, contentAlignment = Alignment.Center) { Tx("✕", color = C.muted, size = 13) }
                        }
                    }
                }
            }
        }
        confirm?.let { d ->
            Confirm(stringResource(R.string.confirm_dept, d.name), stringResource(R.string.yes), stringResource(R.string.no),
                onYes = { confirm = null; store.act { removeDepartment(d.name) } }, onNo = { confirm = null })
        }
    }
}

// ───────────── Archiv ─────────────

@Composable
fun ArchiveScreen(store: Store, nav: Nav) {
    var sheet by remember { mutableStateOf<Note?>(null) }
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            TopBar(stringResource(R.string.archive_title), back = { nav.pop() })
            LazyColumn(Modifier.weight(1f).padding(12.dp)) {
                if (store.archive.isEmpty()) item { Empty(stringResource(R.string.archive_empty)) }
                items(store.archive, key = { it.id }) { n -> NoteLine(n, { sheet = n }, showStatus = false) }
            }
        }
        sheet?.let { n ->
            val dismiss = { sheet = null }
            Sheet(n.title, listOf(
                SheetItem(stringResource(R.string.restore)) { store.act { restoreArchived(n.id) }; dismiss() },
                SheetItem(stringResource(R.string.delete_forever), color = C.red) { store.act { deleteArchived(n.id) }; dismiss() },
            ), dismiss)
        }
    }
}

// ───────────── Papierkorb ─────────────

@Composable
fun TrashScreen(store: Store, nav: Nav) {
    var sheetNote by remember { mutableStateOf<Note?>(null) }
    var sheetContact by remember { mutableStateOf<Contact?>(null) }
    var confirmClear by remember { mutableStateOf(false) }
    val empty = store.trash.notes.isEmpty() && store.trash.contacts.isEmpty()
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            TopBar(stringResource(R.string.trash), back = { nav.pop() }, actions = {
                if (!empty) BarAction(stringResource(R.string.clear)) { confirmClear = true }
            })
            LazyColumn(Modifier.weight(1f).padding(12.dp)) {
                if (empty) item { Empty(stringResource(R.string.trash_empty)) }
                items(store.trash.notes, key = { "n" + it.id }) { n -> NoteLine(n, { sheetNote = n }) }
                items(store.trash.contacts, key = { "c" + it.id }) { c ->
                    Tap(Modifier.fillMaxWidth().padding(bottom = 6.dp), onClick = { sheetContact = c }) {
                        BarRow(C.purple, bg = C.surface) { Box(Modifier.padding(12.dp)) { Tx(c.name, size = 14) } }
                    }
                }
            }
        }
        sheetNote?.let { n ->
            val dismiss = { sheetNote = null }
            Sheet(n.title, listOf(
                SheetItem(stringResource(R.string.restore)) { store.act { restoreNote(n.id) }; dismiss() },
                SheetItem(stringResource(R.string.delete_forever), color = C.red) { store.act { forceDeleteNote(n.id) }; dismiss() },
            ), dismiss)
        }
        sheetContact?.let { c ->
            val dismiss = { sheetContact = null }
            Sheet(c.name, listOf(
                SheetItem(stringResource(R.string.restore)) { store.act { restoreContact(c.id) }; dismiss() },
                SheetItem(stringResource(R.string.delete_forever), color = C.red) { store.act { forceDeleteContact(c.id) }; dismiss() },
            ), dismiss)
        }
        if (confirmClear) Confirm(stringResource(R.string.confirm_clear), stringResource(R.string.yes), stringResource(R.string.no),
            onYes = { confirmClear = false; store.act { clearTrash() } }, onNo = { confirmClear = false })
    }
}

// ───────────── Einstellungen ─────────────

@Composable
fun SettingsScreen(store: Store, nav: Nav, activity: MainActivity) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val app = store.config.optJSONObject("app") ?: JSONObject()
    var lead by remember { mutableStateOf(Reminders.leadMinutes(ctx)) }
    var remoteUrl by remember { mutableStateOf(Remote.url(ctx)) }
    var remoteToken by remember { mutableStateOf(Remote.token(ctx)) }
    var archDay by remember { mutableStateOf(app.optInt("auto_archive_day", 1).toString()) }
    var purgeDay by remember { mutableStateOf(app.optInt("trash_purge_day", 30).toString()) }

    val export = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri != null) scope.launch {
            try {
                val json = store.io { exportJson() }
                ctx.contentResolver.openOutputStream(uri)?.use { it.write(json.toByteArray(Charsets.UTF_8)) }
                store.message = ctx.getString(R.string.export_done)
            } catch (e: Exception) { store.message = e.message }
        }
    }
    val import = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            try {
                val text = ctx.contentResolver.openInputStream(uri)?.bufferedReader(Charsets.UTF_8)?.use { it.readText() } ?: ""
                val obj = JSONObject(text).put("mode", "merge")
                store.io { importData(obj.toString()) }
                store.refresh()
                store.message = ctx.getString(R.string.import_done)
            } catch (e: Exception) { store.message = e.message }
        }
    }

    fun patch(k: String, v: Any) = store.act { patchConfig(JSONObject().put("app", JSONObject().put(k, v))) }

    Column(Modifier.fillMaxSize()) {
        TopBar(stringResource(R.string.settings), back = { nav.pop() })
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            SectionTitle(stringResource(R.string.reminders))
            Tx(stringResource(R.string.reminder_lead), color = C.muted, size = 11)
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Reminders.LEAD_OPTIONS.forEach { m ->
                    Chip(if (m == 0) stringResource(R.string.reminder_at_start) else "$m min", lead == m, {
                        lead = m
                        Reminders.setLeadMinutes(ctx, m)
                        Reminders.sync(ctx, store.notes)
                    })
                }
            }

            SectionTitle(stringResource(R.string.auto_archive))
            Toggle(stringResource(R.string.auto_archive_desc), app.optBoolean("auto_archive_enabled")) { patch("auto_archive_enabled", it) }
            Field(archDay, { archDay = it; it.toIntOrNull()?.takeIf { d -> d in 1..31 }?.let { d -> patch("auto_archive_day", d) } }, stringResource(R.string.day_of_month), keyboard = KeyboardOptions(keyboardType = KeyboardType.Number))

            SectionTitle(stringResource(R.string.trash))
            Toggle(stringResource(R.string.trash_purge_desc), app.optBoolean("trash_purge_enabled")) { patch("trash_purge_enabled", it) }
            Field(purgeDay, { purgeDay = it; it.toIntOrNull()?.takeIf { d -> d in 1..365 }?.let { d -> patch("trash_purge_day", d) } }, stringResource(R.string.days), keyboard = KeyboardOptions(keyboardType = KeyboardType.Number))

            SectionTitle(stringResource(R.string.sync))
            Tx(stringResource(R.string.sync_hint), color = C.muted, size = 11)
            Field(remoteUrl, { remoteUrl = it }, stringResource(R.string.sync_url), placeholder = "https://notice.example.com", keyboard = KeyboardOptions(keyboardType = KeyboardType.Uri))
            Field(remoteToken, { remoteToken = it }, stringResource(R.string.sync_token), keyboard = KeyboardOptions(keyboardType = KeyboardType.Password), password = true)
            Btn(stringResource(R.string.sync_connect), {
                if (!Remote.valid(remoteUrl)) store.message = ctx.getString(R.string.sync_https)
                else { Remote.set(ctx, remoteUrl, remoteToken); activity.retry() }
            }, Modifier.fillMaxWidth(), primary = true)
            if (Remote.enabled(ctx)) Btn(stringResource(R.string.use_local), { Remote.set(ctx, "", ""); activity.retry() }, Modifier.fillMaxWidth())

            SectionTitle(stringResource(R.string.data))
            Btn(stringResource(R.string.export_json), { export.launch("notice-export.json") }, Modifier.fillMaxWidth())
            Btn(stringResource(R.string.import_json), { import.launch(arrayOf("application/json", "text/plain", "*/*")) }, Modifier.fillMaxWidth())
            Tx(stringResource(R.string.import_hint), color = C.muted, size = 11)
        }
    }
}

@Composable
private fun Toggle(label: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().background(C.surface).clickable { onChange(!on) }.padding(12.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        CheckBox(on) { onChange(!on) }
        Tx(label, size = 13, modifier = Modifier.weight(1f))
    }
}

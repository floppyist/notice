package de.flo.notice

import android.app.DatePickerDialog
import android.app.TimePickerDialog
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import java.time.LocalTime

// ───────────── Bausteine ─────────────

@Composable
fun DateField(label: String, value: String, onChange: (String) -> Unit, modifier: Modifier = Modifier) {
    val ctx = LocalContext.current
    Column(modifier) {
        Tx(label.uppercase(), color = C.muted, size = 10, bold = true, spacing = 1, modifier = Modifier.padding(bottom = 4.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.weight(1f).heightMin().background(C.bg).border(BorderStroke(1.dp, C.overlay))
                    .clickable {
                        val d = runCatching { LocalDate.parse(value) }.getOrElse { LocalDate.now() }
                        DatePickerDialog(ctx, { _, y, m, day -> onChange("%04d-%02d-%02d".format(y, m + 1, day)) }, d.year, d.monthValue - 1, d.dayOfMonth).show()
                    }.padding(horizontal = 10.dp, vertical = 10.dp),
            ) { Tx(if (value.isBlank()) "—" else fmtDate(value), color = if (value.isBlank()) C.muted else C.fg) }
            if (value.isNotBlank()) Btn("✕", { onChange("") })
        }
    }
}

private fun Modifier.heightMin() = this.height(42.dp)

@Composable
fun TimeField(label: String, value: String, onChange: (String) -> Unit, modifier: Modifier = Modifier) {
    val ctx = LocalContext.current
    Column(modifier) {
        Tx(label.uppercase(), color = C.muted, size = 10, bold = true, spacing = 1, modifier = Modifier.padding(bottom = 4.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.weight(1f).heightMin().background(C.bg).border(BorderStroke(1.dp, C.overlay))
                    .clickable {
                        val t = runCatching { LocalTime.parse(value) }.getOrElse { LocalTime.of(9, 0) }
                        TimePickerDialog(ctx, { _, h, m -> onChange("%02d:%02d".format(h, m)) }, t.hour, t.minute, true).show()
                    }.padding(horizontal = 10.dp, vertical = 10.dp),
            ) { Tx(if (value.isBlank()) "—" else value, color = if (value.isBlank()) C.muted else C.fg) }
            if (value.isNotBlank()) Btn("✕", { onChange("") })
        }
    }
}

@Composable
fun CheckBox(checked: Boolean, onToggle: () -> Unit) {
    Box(
        Modifier.size(24.dp).background(if (checked) C.accent else C.bg).border(BorderStroke(1.dp, if (checked) C.accent else C.muted)).clickable(onClick = onToggle),
        contentAlignment = Alignment.Center,
    ) { if (checked) Tx("✓", color = C.onAccent, size = 14, bold = true) }
}

@Composable
private fun Section(title: String, content: @Composable () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(top = 18.dp)) {
        Tx(title.uppercase(), color = C.accentText, size = 11, bold = true, spacing = 1, modifier = Modifier.padding(bottom = 8.dp))
        content()
    }
}

// ───────────── Notiz-Editor ─────────────

@Composable
fun NoteEditor(store: Store, nav: Nav, note: Note?) {
    val base = note ?: Note()
    var title by remember { mutableStateOf(base.title) }
    var text by remember { mutableStateOf(base.text) }
    var status by remember { mutableStateOf(base.status) }
    var priority by remember { mutableStateOf(base.priority) }
    var due by remember { mutableStateOf(base.dueDate ?: "") }
    var repeat by remember { mutableStateOf(base.repeat ?: "") }
    var pinned by remember { mutableStateOf(base.pinned) }
    val deps = remember { mutableStateListOf<String>().apply { addAll(base.departments) } }
    val checks = remember { mutableStateListOf<Check>().apply { addAll(base.checklist) } }
    val appts = remember { mutableStateListOf<Appt>().apply { addAll(base.appointments) } }
    var newDept by remember { mutableStateOf("") }
    var newCheck by remember { mutableStateOf("") }
    var aTitle by remember { mutableStateOf("") }
    var aDate by remember { mutableStateOf("") }
    var aTime by remember { mutableStateOf("") }
    var confirmDelete by remember { mutableStateOf(false) }
    val isNew = base.id == 0L
    val titleRequired = stringResource(R.string.title_required)

    fun save() {
        if (title.isBlank()) { store.message = titleRequired; return }
        val pendingCheck = newCheck.trim()
        val finalChecks = checks.toList() + if (pendingCheck.isNotEmpty()) listOf(Check(pendingCheck, false)) else emptyList()
        val n = base.copy(
            title = title.trim(), text = text, status = status, priority = priority,
            dueDate = due.ifBlank { null }, repeat = repeat.ifBlank { null }, pinned = pinned,
            departments = deps.toList(), checklist = finalChecks, appointments = appts.toList(),
        )
        store.act({ nav.pop() }) { if (isNew) createNote(n) else saveNote(n) }
    }

    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            TopBar(
                stringResource(if (isNew) R.string.note_new else R.string.note_edit), back = { nav.pop() },
                actions = { BarAction(stringResource(R.string.save), primary = true) { save() } },
            )
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 12.dp).padding(bottom = 24.dp)) {
                Box(Modifier.padding(top = 14.dp)) { Field(title, { title = it }, stringResource(R.string.title), placeholder = stringResource(R.string.title_ph)) }

                Section(stringResource(R.string.status)) {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Choice(stringResource(R.string.status), STATUSES.map { it to statusLabel(it) }, status) { status = it }
                        Choice(stringResource(R.string.priority), listOf("low", "medium", "high").map { it to priorityLabel(it) }, priority) { priority = it }
                        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            Chip((if (pinned) "★ " else "☆ ") + stringResource(R.string.pin), pinned, { pinned = !pinned })
                        }
                    }
                }

                Section(stringResource(R.string.due)) {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        DateField(stringResource(R.string.due), due, { due = it })
                        Choice(
                            stringResource(R.string.repeat),
                            listOf("" to stringResource(R.string.repeat_none), "daily" to stringResource(R.string.repeat_daily), "weekly" to stringResource(R.string.repeat_weekly), "monthly" to stringResource(R.string.repeat_monthly)),
                            repeat,
                        ) { repeat = it }
                    }
                }

                Section(stringResource(R.string.content)) {
                    Field(text, { text = it }, singleLine = false, minLines = 6, placeholder = stringResource(R.string.content_ph))
                }

                Section(stringResource(R.string.checklist)) {
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        checks.forEachIndexed { i, c ->
                            Row(Modifier.fillMaxWidth().background(C.surface).padding(8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                                CheckBox(c.done) { checks[i] = c.copy(done = !c.done) }
                                Tx(c.text, color = if (c.done) C.muted else C.fg, modifier = Modifier.weight(1f))
                                Box(Modifier.size(32.dp).clickable { checks.removeAt(i) }, contentAlignment = Alignment.Center) { Tx("✕", color = C.muted, size = 12) }
                            }
                        }
                        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                            Field(newCheck, { newCheck = it }, modifier = Modifier.weight(1f), placeholder = stringResource(R.string.checklist_add))
                            Btn("+", { if (newCheck.isNotBlank()) { checks.add(Check(newCheck.trim(), false)); newCheck = "" } })
                        }
                    }
                }

                Section(stringResource(R.string.appointments)) {
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        appts.forEachIndexed { i, a ->
                            Row(Modifier.fillMaxWidth().background(C.surface).padding(8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                                CheckBox(a.done) { appts[i] = a.copy(done = !a.done) }
                                Column(Modifier.weight(1f)) {
                                    Tx(a.title, color = if (a.done) C.muted else C.fg)
                                    Tx(fmtDate(a.start) + if (a.time.isNotBlank()) " ${a.time}" else "", color = C.aqua, size = 11)
                                }
                                Box(Modifier.size(32.dp).clickable { appts.removeAt(i) }, contentAlignment = Alignment.Center) { Tx("✕", color = C.muted, size = 12) }
                            }
                        }
                        Field(aTitle, { aTitle = it }, placeholder = stringResource(R.string.apt_title))
                        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            DateField(stringResource(R.string.date), aDate, { aDate = it }, Modifier.weight(1f))
                            TimeField(stringResource(R.string.time), aTime, { aTime = it }, Modifier.weight(1f))
                        }
                        Btn(stringResource(R.string.apt_add), {
                            if (aTitle.isNotBlank() && aDate.isNotBlank()) {
                                appts.add(Appt(aTitle.trim(), aDate, aTime)); aTitle = ""; aDate = ""; aTime = ""
                            }
                        }, Modifier.fillMaxWidth())
                    }
                }

                Section(stringResource(R.string.departments)) {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        val all = (store.depts.map { it.name } + deps).distinctBy { it.lowercase() }
                        Tags2(all, deps)
                        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                            Field(newDept, { newDept = it }, modifier = Modifier.weight(1f), placeholder = stringResource(R.string.dept_new))
                            Btn("+", { if (newDept.isNotBlank()) { if (deps.none { it.equals(newDept.trim(), true) }) deps.add(newDept.trim()); newDept = "" } })
                        }
                    }
                }

                if (!isNew) {
                    Section(stringResource(R.string.actions)) {
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Btn(stringResource(R.string.duplicate), { store.act({ nav.pop() }) { duplicateNote(base.id) } }, Modifier.fillMaxWidth())
                            Btn(stringResource(R.string.archive), { store.act({ nav.pop() }) { archiveNote(base.id) } }, Modifier.fillMaxWidth())
                            Btn(stringResource(R.string.delete), { confirmDelete = true }, Modifier.fillMaxWidth(), color = C.red)
                        }
                    }
                }
            }
        }
        if (confirmDelete) {
            Confirm(stringResource(R.string.confirm_trash, base.title), stringResource(R.string.yes), stringResource(R.string.no),
                onYes = { confirmDelete = false; store.act({ nav.pop() }) { deleteNote(base.id) } }, onNo = { confirmDelete = false })
        }
    }
}

/** Auswahlbare Einrichtungs-Tags: aktiv = Akzent. */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
private fun Tags2(all: List<String>, selected: MutableList<String>) {
    androidx.compose.foundation.layout.FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        all.forEach { d ->
            val on = selected.any { it.equals(d, true) }
            Chip(d, on, { if (on) selected.removeAll { it.equals(d, true) } else selected.add(d) })
        }
    }
}

// ───────────── Kontakt-Editor ─────────────

@Composable
fun ContactEditor(store: Store, nav: Nav, contact: Contact?) {
    val base = contact ?: Contact()
    var name by remember { mutableStateOf(base.name) }
    var title by remember { mutableStateOf(base.title) }
    var phone by remember { mutableStateOf(base.phone) }
    var mobile by remember { mutableStateOf(base.mobile) }
    var fax by remember { mutableStateOf(base.fax) }
    var email by remember { mutableStateOf(base.email) }
    var desc by remember { mutableStateOf(base.description) }
    val deps = remember { mutableStateListOf<String>().apply { addAll(base.departments) } }
    var confirmDelete by remember { mutableStateOf(false) }
    val ctx = LocalContext.current
    val isNew = base.id == 0L

    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            TopBar(stringResource(if (isNew) R.string.contact_new else R.string.contact_edit), back = { nav.pop() }, actions = {
                BarAction(stringResource(R.string.save), primary = true) {
                    if (name.isNotBlank()) store.act({ nav.pop() }) {
                        saveContact(base.copy(name = name.trim(), title = title, phone = phone, mobile = mobile, fax = fax, email = email, description = desc, departments = deps.toList()))
                    }
                }
            })
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Field(name, { name = it }, stringResource(R.string.name))
                Field(title, { title = it }, stringResource(R.string.contact_title))
                Field(phone, { phone = it }, stringResource(R.string.phone), keyboard = KeyboardOptions(keyboardType = KeyboardType.Phone))
                Field(mobile, { mobile = it }, stringResource(R.string.mobile), keyboard = KeyboardOptions(keyboardType = KeyboardType.Phone))
                Field(fax, { fax = it }, stringResource(R.string.fax), keyboard = KeyboardOptions(keyboardType = KeyboardType.Phone))
                Field(email, { email = it }, stringResource(R.string.email), keyboard = KeyboardOptions(keyboardType = KeyboardType.Email))
                Tx(stringResource(R.string.departments).uppercase(), color = C.muted, size = 10, bold = true, spacing = 1)
                Tags2((store.depts.map { it.name } + deps).distinctBy { it.lowercase() }, deps)
                Field(desc, { desc = it }, stringResource(R.string.description), singleLine = false, minLines = 3)
                if (!isNew) {
                    if (phone.isNotBlank() || mobile.isNotBlank()) Btn(stringResource(R.string.call), {
                        ctx.startActivity(Intent(Intent.ACTION_DIAL, Uri.parse("tel:" + mobile.ifBlank { phone })))
                    }, Modifier.fillMaxWidth())
                    if (email.isNotBlank()) Btn(stringResource(R.string.mail), {
                        ctx.startActivity(Intent(Intent.ACTION_SENDTO, Uri.parse("mailto:$email")))
                    }, Modifier.fillMaxWidth())
                    Btn(stringResource(R.string.delete), { confirmDelete = true }, Modifier.fillMaxWidth(), color = C.red)
                }
            }
        }
        if (confirmDelete) {
            Confirm(stringResource(R.string.confirm_trash, base.name), stringResource(R.string.yes), stringResource(R.string.no),
                onYes = { confirmDelete = false; store.act({ nav.pop() }) { deleteContact(base.id) } }, onNo = { confirmDelete = false })
        }
    }
}

// ───────────── Wiki-Editor ─────────────

@Composable
fun WikiEditor(store: Store, nav: Nav, wiki: Wiki?) {
    val base = wiki ?: Wiki()
    var title by remember { mutableStateOf(base.title) }
    var content by remember { mutableStateOf(base.content) }
    Column(Modifier.fillMaxSize()) {
        TopBar(stringResource(if (base.id == 0L) R.string.wiki_new else R.string.wiki_edit), back = { nav.pop() }, actions = {
            BarAction(stringResource(R.string.save), primary = true) {
                if (title.isNotBlank()) store.act({ nav.pop() }) { saveWiki(base.copy(title = title.trim(), content = content)) }
            }
        })
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Field(title, { title = it }, stringResource(R.string.title))
            Field(content, { content = it }, stringResource(R.string.content), singleLine = false, minLines = 14, placeholder = stringResource(R.string.wiki_ph))
        }
    }
}

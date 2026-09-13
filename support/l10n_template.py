import re

p = 'frontend/index.html'
s = open(p).read()

def rep(old, new, count=None):
    global s
    n = s.count(old)
    if count is not None and n != count:
        raise SystemExit(f'COUNT MISMATCH for {old!r}: expected {count}, got {n}')
    s = s.replace(old, new)

# length-decreasing order handled by explicit distinct strings

# --- BOARD ---
rep(':title="status.title + \' öffnen\'"', ":title=\"status.title + ' ' + t('board.expand')\"", 1)
rep("{id:'all',label:'Alle'},{id:'overdue',label:'Überfällig'},{id:'due',label:'Mit Datum'},{id:'nodate',label:'Ohne Datum'}",
    "{id:'all',label:t('board.filter_all')},{id:'overdue',label:t('board.filter_overdue')},{id:'due',label:t('board.filter_due')},{id:'nodate',label:t('board.filter_nodate')}", 1)
rep("{{ column.id === 'archived' ? 'Keine archivierten Notizen.' : 'Keine Notizen.' }}",
    "{{ column.id === 'archived' ? t('board.empty_archived') : t('board.empty') }}", 1)
rep('{{ column.title }}', "{{ statusTitle(column.id) }}", 1)
rep('title="Duplizieren"', ':title="t(\'board.duplicate\')"', 1)
rep('title="Wiederherstellen"', ':title="t(\'board.restore\')"', 1)
rep('title="Archivieren"', ':title="t(\'board.archive\')"', 1)
rep('Fällig: {{ fmtDate(', "{{ t('common.due') }}: {{ fmtDate(", 3)
rep('>Kein Datum<', ">{{ t('board.no_date') }}<", 2)
rep('ÜBERFÄLLIG (', "{{ t('board.overdue') }} (", 1)
rep('FÄLLIG (', "{{ t('board.due_prefix') }} (", 1)
rep('Termine: {{', "{{ t('common.appointments') }}: {{", 1)
rep("{daily: 'täglich', weekly: 'wöchentlich', monthly: 'monatlich'}",
    "{daily: t('repeat.daily'), weekly: t('repeat.weekly'), monthly: t('repeat.monthly')}", 2)
rep(":title=\"'Wiederholt ' + (", ":title=\"t('board.repeats') + ' ' + (", 1)

# --- CALENDAR ---
rep('>HEUTE<', ">{{ t('cal.today') }}<", 1)
rep('>Woche<', ">{{ t('cal.week') }}<", 1)
rep('>Monat<', ">{{ t('cal.month') }}<", 1)
rep('>Jahr<', ">{{ t('cal.year') }}<", 1)
rep("['Mo','Di','Mi','Do','Fr','Sa','So']", 'dowShorts()', 1)
rep("['M','D','M','D','F','S','S']", "dowShorts().map(d => d.charAt(0))", 1)
rep('+{{ day.events.length - 3 }} weitere', "+{{ day.events.length - 3 }} {{ t('cal.more') }}", 1)
rep('Keine Termine an diesem Tag.', "{{ t('cal.no_events_day') }}", 1)

# --- DASHBOARD ---
rep('>Übersicht<', ">{{ t('dash.title') }}<", 1)
rep('Heutige Aufgaben', "{{ t('dash.today_tasks') }}", 1)
rep('× heute', '× {{ t(\'cal.today\') }}', 1)
rep('Keine Termine oder Fälligkeiten für heute.', "{{ t('dash.today_empty') }}", 1)
rep("{{ ev.aptTitle !== 'Fällig' ? ev.title : ('Fällig: ' + ev.noteTitle) }}",
    "{{ ev.aptTitle !== t('common.due') ? ev.title : (t('common.due') + ': ' + ev.noteTitle) }}", 1)
rep('Überfällig ({{ overdueNotes.length }})', "{{ t('dash.overdue') }} ({{ overdueNotes.length }})", 1)
rep('Kommende Termine', "{{ t('dash.upcoming') }}", 1)
rep("? 'Heute' : fmtDate(u.start)", "? t('cal.today') : fmtDate(u.start)", 1)
rep('Status</span>', "{{ t('dash.status') }}</span>", 1)
rep('Priorität</span>', "{{ t('dash.priority') }}</span>", 1)
rep('Fertiggestellt (letzte {{ dashboardActivityDays }} Tage)',
    "{{ t('dash.completed') }} ({{ t('dash.last_x_days', { days: dashboardActivityDays }) }})", 1)
rep('Keine abgeschlossenen Notizen im Zeitraum.', "{{ t('dash.activity_empty') }}", 1)
rep('To-do-Fortschritt', "{{ t('dash.todo_progress') }}", 1)
rep('{{ dashboardTodo.done }} von {{ dashboardTodo.total }} erledigt',
    "{{ t('dash.todo_done', { done: dashboardTodo.done, total: dashboardTodo.total }) }}", 1)
rep('Keine Checklisten-Einträge.', "{{ t('dash.todo_empty') }}", 1)
rep('Aktivität nach Einrichtung', "{{ t('dash.dept_activity') }}", 1)
rep("row.name + ': ' + row.count + ' Notizen'",
    "row.name + ': ' + row.count + ' ' + t('common.notes')", 1)

# --- DATA ---
rep('Daten-Übersicht', "{{ t('data.title') }}", 1)
rep('Einrichtungen / Departments', "{{ t('data.departments') }}", 1)
rep('Keine Departments angelegt.', "{{ t('data.depts_empty') }}", 1)
rep("{{ d.count }} Notiz{{ d.count === 1 ? '' : 'en' }}", "{{ t('data.note_count', { count: d.count }) }}", 1)
rep('placeholder="Neues Department..."', ":placeholder=\"t('data.new_dept')\"", 1)
rep('>Erstellen<', ">{{ t('data.create') }}<", 1)
rep('>Kontakte</span>', ">{{ t('data.contacts') }}</span>", 1)
rep('placeholder="Suchen..."', ':placeholder="t(\'common.search\')"', 2)
rep('Keine Kontakte gefunden.', "{{ t('data.contacts_empty') }}", 1)
rep('>Name</th>', ">{{ t('data.name') }}</th>", 1)
rep('>Einrichtung</th>', ">{{ t('data.org') }}</th>", 1)
rep('>Telefon</th>', ">{{ t('data.phone') }}</th>", 1)
rep('>E-Mail</th>', ">{{ t('data.email') }}</th>", 1)
rep('>Papierkorb</span>', ">{{ t('data.trash') }}</span>", 1)
rep("{{ trashNotes.length + trashContacts.length }} {{ trashNotes.length + trashContacts.length === 1 ? 'Element' : 'Elemente' }}",
    "{{ t('data.element_count', { count: trashNotes.length + trashContacts.length }) }}", 1)
rep('>[Leeren]</button>', ">[{{ t('data.empty') }}]</button>", 1)
rep('Papierkorb ist leer.', "{{ t('data.trash_empty') }}", 1)
rep('>Note</span>', ">{{ t('data.note') }}</span>", 1)
rep('>Kontakt</span>', ">{{ t('data.contact') }}</span>", 1)
rep('>Wiederherstellen</button>', ">{{ t('data.restore') }}</button>", 2)
rep('title="Endgültig löschen"', ':title="t(\'data.delete_forever\')"', 2)

# --- WIKI ---
rep('+ NEUE SEITE', "+ {{ t('wiki.new_page') }}", 1)
rep('Keine Wiki-Seiten.', "{{ t('wiki.empty') }}", 1)
rep('Keine Seite geöffnet — wähle links eine Seite oder erstelle eine neue.', "{{ t('wiki.none_open') }}", 1)
rep('>Inhalt (Markdown)</span>', ">{{ t('common.markdown_content') }}</span>", 3)
rep("{{ wikiPreviewOpen ? 'Bearbeiten' : 'Vorschau' }}",
    "{{ wikiPreviewOpen ? t('common.edit') : t('common.preview') }}", 1)
rep("{{ newNotePreviewMode ? 'Bearbeiten' : 'Vorschau' }}",
    "{{ newNotePreviewMode ? t('common.edit') : t('common.preview') }}", 1)
rep("{{ isPreviewMode ? 'Bearbeiten' : 'Vorschau' }}",
    "{{ isPreviewMode ? t('common.edit') : t('common.preview') }}", 1)
rep('placeholder="Wiki-Inhalt (Markdown, <<Seitenname>> als Link) ... (Rechtsklick für Formatierung)"',
    ':placeholder="t(\'wiki.content_placeholder\')"', 1)
rep("{{ autocompleteType === 'address' ? 'Kontakt verlinken' : (autocompleteType === 'wiki' ? 'Wiki-Seite verlinken' : 'Notiz verlinken') }}",
    "{{ autocompleteType === 'address' ? t('ac.contact') : (autocompleteType === 'wiki' ? t('ac.wiki') : t('ac.note')) }}", 3)
rep('Keine Treffer.', "{{ t('ac.no_results') }}", 3)
rep('Markdown Format', "{{ t('fmt.title') }}", 3)
rep('>Fett</span>', ">{{ t('fmt.bold') }}</span>", 3)
rep('>Kursiv</span>', ">{{ t('fmt.italic') }}</span>", 3)
rep('>Durchgestrichen</span>', ">{{ t('fmt.strike') }}</span>", 3)
rep('>Code-Snippet</span>', ">{{ t('fmt.code') }}</span>", 3)
rep('Farben (HTML)', "{{ t('fmt.colors') }}", 3)
rep('> Smaragdgrün<', "> {{ t('color.emerald') }}<", 3)
rep('> Rot<', "> {{ t('color.red') }}<", 3)
rep('> Orange<', "> {{ t('color.orange') }}<", 3)
rep('> Gelb<', "> {{ t('color.yellow') }}<", 3)
rep('> Blau<', "> {{ t('color.blue') }}<", 3)
rep('> Lila<', "> {{ t('color.violet') }}<", 3)
rep('> Rosa<', "> {{ t('color.pink') }}<", 3)
rep('> Grau<', "> {{ t('color.gray') }}<", 3)
rep('>Bild</div>', ">{{ t('fmt.image') }}</div>", 3)
rep('↑ Bild einfügen', "{{ t('fmt.insert_image') }}", 3)
rep('[Seite löschen]', "[{{ t('wiki.delete_page') }}]", 1)
rep('>SPEICHERN</button>', ">{{ t('wiki.save') }}</button>", 1)

# --- GRAPH ---
rep('Nodes (Nodemap)', "{{ t('graph.title') }}", 1)
rep('title="Neues Layout berechnen"', ':title="t(\'graph.layout_recalc\')"', 1)
rep('↻ Layout', "↻ {{ t('graph.layout') }}", 1)
rep('Keine Daten vorhanden.', "{{ t('graph.no_data') }}", 1)
rep('>Öffnen</button>', ">{{ t('graph.open') }}</button>", 3)
rep('>Eigenschaften</div>', ">{{ t('graph.props') }}</div>", 1)
rep('>Verknüpfungen</div>', ">{{ t('graph.links') }}</div>", 1)
rep('Keine Verknüpfungen.', "{{ t('graph.no_links') }}", 1)

# --- IMPORT ---
rep('Import-Vorschau', "{{ t('import.title') }}", 1)
rep('Datei: <span', "{{ t('import.file') }}: <span", 1)
rep('Modus: <span', "{{ t('import.mode') }}: <span", 1)
rep("'Ersetzen' : 'Zusammenführen'", "t('import.replace') : t('import.merge')", 1)
rep('</span> Notizen', "</span> {{ t('import.notes') }}", 1)
rep('</span> Kontakte', "</span> {{ t('import.contacts') }}", 1)
rep('Achtung: Beim Ersetzen werden {{ existingCount }} vorhandene Notizen und {{ existingContactCount }} vorhandene Kontakte gelöscht und durch die Import-Datei ersetzt.',
    "{{ t('import.warn_replace', { notes: existingCount, contacts: existingContactCount }) }}", 1)
rep('Enthaltene Notizen', "{{ t('import.included_notes') }}", 1)
rep('(ohne Titel)', "({{ t('common.no_title') }})", 1)
rep('+ {{ pendingImport.notes.length - 40 }} weitere', "+ {{ pendingImport.notes.length - 40 }} {{ t('import.more') }}", 1)
rep('>Abbrechen</button>', ">{{ t('common.cancel') }}</button>", 3)
rep('>Importieren</button>', ">{{ t('import.do') }}</button>", 1)
rep('Rückgängig', "{{ t('undo.undo') }}", 1)

# --- SETTINGS ---
rep('>Einstellungen</span>', ">{{ t('settings.title') }}</span>", 1)
rep('Auto-Archiv', "{{ t('settings.auto_archive') }}", 1)
rep('Abgeschlossene Notizen automatisch archivieren', "{{ t('settings.auto_archive_label') }}", 1)
rep('Ab Tag des Monats:', "{{ t('settings.month_day') }}", 1)
rep('Beim Start der App werden ab dem {{ autoArchiveDay }}. des Monats alle Notizen mit Status "Abgeschlossen" ins Archiv verschoben.',
    "{{ t('settings.auto_archive_hint', { day: autoArchiveDay }) }}", 1)
rep('Papierkorb-Autolöschung', "{{ t('settings.trash_purge') }}", 1)
rep('Papierkorb nach Tagen automatisch leeren', "{{ t('settings.trash_purge_label') }}", 1)
rep('Löschen nach:', "{{ t('settings.delete_after') }}", 1)
rep('>Tagen</span>', ">{{ t('settings.days') }}</span>", 1)
rep('Beim Start der App werden gelöschte Notizen und Kontakte, die länger als {{ trashPurgeDay }} Tage im Papierkorb liegen, endgültig entfernt.',
    "{{ t('settings.trash_purge_hint', { day: trashPurgeDay }) }}", 1)
rep('JETZT AUFRÄUMEN', "{{ t('settings.purge_now') }}", 1)

# --- CONTACT MODAL ---
rep("{{ contactForm.id ? 'Kontakt bearbeiten' : 'Neuer Kontakt' }}",
    "{{ contactForm.id ? t('contact.edit') : t('contact.new') }}", 1)
rep('Einrichtungen (Departments)', "{{ t('contact.orgs') }}", 1)
rep('placeholder="Department hinzufügen..."', ':placeholder="t(\'contact.add_dept\')"', 3)
rep('+ "{{ contactDeptInput.trim() }}" erstellen', '+ "{{ contactDeptInput.trim() }}" {{ t(\'contact.create\') }}', 1)
rep('+ "{{ newNoteDeptInput.trim() }}" erstellen', '+ "{{ newNoteDeptInput.trim() }}" {{ t(\'contact.create\') }}', 1)
rep('+ "{{ modalDeptInput.trim() }}" erstellen', '+ "{{ modalDeptInput.trim() }}" {{ t(\'contact.create\') }}', 1)
rep('Keine Departments vorhanden.', "{{ t('contact.no_depts') }}", 3)
rep('>Speichern</button>', ">{{ t('common.save') }}</button>", 1)

# --- REMINDER ---
rep('Abgelaufene Termine', "{{ t('reminder.title') }}", 1)
rep("{{ overdueReminders.length }} {{ overdueReminders.length === 1 ? 'Termin/Fälligkeit ist' : 'Termine/Fälligkeiten sind' }} bereits vorbei.",
    "{{ t('reminder.past_count', { count: overdueReminders.length }) }}", 1)
rep("r.kind === 'apt' ? (r.note.title + ' – ' + (r.apt.title || 'Termin')) : ('Fällig: ' + r.note.title)",
    "r.kind === 'apt' ? (r.note.title + ' – ' + (r.apt.title || t('reminder.apt'))) : (t('common.due') + ': ' + r.note.title)", 1)
rep('>Erledigt</button>', ">{{ t('reminder.done') }}</button>", 1)

# --- NEW / DETAIL MODALS ---
rep('>Neue Notiz</span>', ">{{ t('note.new') }}</span>", 1)
rep('>Notiz bearbeiten</span>', ">{{ t('note.edit') }}</span>", 1)
rep('placeholder="Notiz-Titel..."', ':placeholder="t(\'note.title_ph\')"', 2)
rep('>Backlog</option>', ">{{ t('status.backlog') }}</option>", 2)
rep('>In Arbeit</option>', ">{{ t('status.in_progress') }}</option>", 2)
rep('>Review</option>', ">{{ t('status.review') }}</option>", 2)
rep('>Abgeschlossen</option>', ">{{ t('status.done') }}</option>", 2)
rep('>Archiv</option>', ">{{ t('status.archived') }}</option>", 1)
rep('>Niedrig</option>', ">{{ t('priority.low') }}</option>", 2)
rep('>Mittel</option>', ">{{ t('priority.medium') }}</option>", 2)
rep('>Hoch</option>', ">{{ t('priority.high') }}</option>", 2)
rep('>Keine</option>', ">{{ t('repeat.none') }}</option>", 2)
rep('>Täglich</option>', ">{{ t('repeat.daily') }}</option>", 2)
rep('>Wöchentlich</option>', ">{{ t('repeat.weekly') }}</option>", 2)
rep('>Monatlich</option>', ">{{ t('repeat.monthly') }}</option>", 2)
rep("{{ newNoteMarkdownMax ? '⤡ Verkleinern' : '⤢ Maximieren' }}",
    "{{ newNoteMarkdownMax ? t('note.shrink') : t('note.maximize') }}", 1)
rep("{{ detailMarkdownMax ? '⤡ Verkleinern' : '⤢ Maximieren' }}",
    "{{ detailMarkdownMax ? t('note.shrink') : t('note.maximize') }}", 1)
rep('placeholder="Inhalt schreiben... ([[ Für Notiz-Links) (Rechtsklick für Formatierung)"',
    ':placeholder="t(\'note.content_ph\')"', 2)
rep('>Termine</span>', ">{{ t('note.appointments') }}</span>", 2)
rep('placeholder="Termin..."', ':placeholder="t(\'note.apt_title_ph\')"', 2)
rep('title="Uhrzeit im 24h-Format (z.B. 14:30)"', ':title="t(\'note.time_title\')"', 2)
rep('> Bis-Datum', "> {{ t('note.end_date') }}", 2)
rep('+ Hinzufügen', "+ {{ t('note.add') }}", 2)
rep('>Keine Termine.</div>', ">{{ t('note.no_apt') }}</div>", 2)
rep('Checklist / Steps', "{{ t('note.checklist') }}", 2)
rep('placeholder="Neuer Step..."', ':placeholder="t(\'note.step_ph\')"', 2)
rep('Keine Zwischensteps.', "{{ t('note.no_steps') }}", 2)
rep('ERSTELLEN & SCHLIESSEN', "{{ t('note.create_close') }}", 1)
rep('SPEICHERN & SCHLIESSEN', "{{ t('note.save_close') }}", 1)

# --- CONFIRM / HELP ---
rep('>Bestätigung</span>', ">{{ t('confirm.title') }}</span>", 1)
rep('Tastaturkürzel', "{{ t('help.title') }}", 1)
rep('Neue Notiz öffnen', "{{ t('help.new_note') }}", 1)
rep('Suche fokussieren', "{{ t('help.search') }}", 1)
rep('Notiz speichern & schließen', "{{ t('help.save_note') }}", 1)
rep('Overlay schließen', "{{ t('help.close_overlay') }}", 1)
rep('Diese Hilfe', "{{ t('help.help') }}", 1)
rep('>Schließen</button>', ">{{ t('help.close') }}</button>", 1)

# Label lines for modals (Status/Priorität/Fällig/Wiederholung/Department/Titel/Name/Telefon/E-Mail/Beschreibung)
# Modal labels (each appears in BOTH new-note and detail modal, identical strings)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400 md:col-span-4">\n                            Titel',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400 md:col-span-4">\n                            {{ t(\'common.title\') }}', 2)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                            Status',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                            {{ t(\'note.status\') }}', 2)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                            Priorität',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                            {{ t(\'note.priority\') }}', 2)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                            Fällig',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                            {{ t(\'note.due\') }}', 2)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                            Wiederholung',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                            {{ t(\'note.repeat\') }}', 2)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400 md:col-span-4">\n                            Department',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400 md:col-span-4">\n                            {{ t(\'note.department\') }}', 2)
# Contact labels (multiline text node inside label)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                        Name',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                        {{ t(\'common.name\') }}', 1)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                        Telefon',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                        {{ t(\'common.phone\') }}', 1)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                        E-Mail',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                        {{ t(\'common.email\') }}', 1)
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                        Beschreibung',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">\n                        {{ t(\'common.description\') }}', 1)
# Wiki title label
rep('<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400 mb-2">\n                                    Titel',
    '<label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400 mb-2">\n                                    {{ t(\'common.title\') }}', 1)

# Draft footer
rep("{{ draftRestored ? 'Entwurf vom ' : 'Entwurf gespeichert um ' }}{{ fmtClock(draftTs) }} <span class=\"text-zinc-400 dark:text-zinc-600 italic\">(automatisch)</span>",
    "{{ draftRestored ? t('draft.restored') : t('draft.saved') }} {{ fmtClock(draftTs) }} <span class=\"text-zinc-400 dark:text-zinc-600 italic\">({{ t('draft.auto') }})</span>", 1)
rep('Entwurf löschen', "{{ t('draft.delete') }}", 1)

open(p, 'w').write(s)
print('OK all replacements applied')
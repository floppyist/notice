use crate::models::{first_department, Contact, Note, WikiPage};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use crate::tui::links::LinkKind;
use crate::tui::theme as th;
use serde_json::{json, Value};

const STATUSES: &[&str] = &["backlog", "in_progress", "review", "done"];
const PRIORITIES: &[&str] = &["low", "medium", "high"];
const REPEATS: &[&str] = &["none", "daily", "weekly", "monthly"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Note,
    Contact,
    Wiki,
}

pub enum EditorAction {
    None,
    Cancel,
    Save,
}

pub enum SaveData {
    Note(NoteDraft),
    Contact(ContactDraft),
    Wiki(WikiDraft),
}

pub struct NoteDraft {
    pub id: Option<i64>,
    pub title: String,
    pub content: String,
    pub status: String,
    pub priority: String,
    pub due_date: Option<String>,
    pub department: Option<String>,
    pub departments: String,
    pub appointments: Option<String>,
    pub repeat_rule: Option<String>,
    pub date: Option<String>,
    pub sort_order: Option<i64>,
    pub pinned: i64,
    pub original_status: Option<String>,
    pub original_completed_at: Option<String>,
}

pub struct ContactDraft {
    pub id: Option<i64>,
    pub name: String,
    pub title: Option<String>,
    pub department: Option<String>,
    pub departments: String,
    pub phone: Option<String>,
    pub mobile: Option<String>,
    pub fax: Option<String>,
    pub email: Option<String>,
    pub description: Option<String>,
}

pub struct WikiDraft {
    pub id: Option<i64>,
    pub title: String,
    pub content: Option<String>,
}

struct Field {
    key: &'static str,
    label: &'static str,
    chars: Vec<char>,
    cursor: usize,
    multiline: bool,
    choices: Option<&'static [&'static str]>,
}

impl Field {
    fn text(key: &'static str, label: &'static str, value: &str) -> Self {
        Field {
            key,
            label,
            chars: value.chars().collect(),
            cursor: value.chars().count(),
            multiline: false,
            choices: None,
        }
    }

    fn multiline(key: &'static str, label: &'static str, value: &str) -> Self {
        Field {
            key,
            label,
            chars: value.chars().collect(),
            cursor: value.chars().count(),
            multiline: true,
            choices: None,
        }
    }

    fn choice(key: &'static str, label: &'static str, value: &str, choices: &'static [&'static str]) -> Self {
        let value = if choices.contains(&value) { value } else { choices[0] };
        Field {
            key,
            label,
            chars: value.chars().collect(),
            cursor: value.chars().count(),
            multiline: false,
            choices: Some(choices),
        }
    }

    fn value(&self) -> String {
        self.chars.iter().collect()
    }

    fn set(&mut self, value: &str) {
        self.chars = value.chars().collect();
        self.cursor = self.chars.len();
    }

    fn insert(&mut self, ch: char) {
        self.chars.insert(self.cursor, ch);
        self.cursor += 1;
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        if self.cursor < self.chars.len() {
            self.cursor += 1;
        }
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    fn cycle(&mut self, delta: isize) {
        let Some(choices) = self.choices else {
            return;
        };
        let current = self.value();
        let index = choices.iter().position(|choice| *choice == current).unwrap_or(0) as isize;
        let next = (index + delta).rem_euclid(choices.len() as isize) as usize;
        self.set(choices[next]);
    }

    fn line_col(&self) -> (usize, usize) {
        let mut line = 0;
        let mut column = 0;
        for ch in self.chars.iter().take(self.cursor) {
            if *ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }
        }
        (line, column)
    }
}

struct ApptEdit {
    /// Startdatum (JSON-Feld `start`)
    pub date: String,
    pub time: String,
    /// Titel (JSON-Feld `title`)
    pub text: String,
    /// Übrige Felder des Termins (`end`, `endTime`, `done` …) bleiben erhalten
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Clone, Debug)]
struct CheckEdit {
    pub done: bool,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
enum ListFocus {
    None,
    Appointments,
    Checklist,
}

pub struct Editor {
    target: Target,
    fields: Vec<Field>,
    pub(crate) focus: usize,
    pub(crate) error: Option<String>,
    pub(crate) scroll: u16,
    original_note: Option<Note>,
    original_contact: Option<Contact>,
    original_wiki: Option<WikiPage>,
    pub(crate) appointments: Vec<ApptEdit>,
    pub(crate) checklist: Vec<CheckEdit>,
    pub(crate) list_focus: ListFocus,
    pub(crate) list_idx: usize,
    /// Mögliche Link-Ziele (vom App gesetzt) für die Autovervollständigung.
    pub candidates: Vec<(LinkKind, String)>,
    suggest: Option<Suggest>,
    suppress: bool,
}

/// Aktive Link-Vorschläge: `start` = Zeichenindex des Öffners (`[[`, `<<`, `{{`).
struct Suggest {
    kind: LinkKind,
    start: usize,
    items: Vec<String>,
    sel: usize,
}

impl Editor {
    pub fn new_note() -> Self {
        let content = json!({"text": "", "checklist": []}).to_string();
        let note = Note {
            id: 0,
            title: String::new(),
            content,
            status: "backlog".to_string(),
            priority: "medium".to_string(),
            date: String::new(),
            due_date: None,
            department: None,
            departments: Some("[]".to_string()),
            appointments: None,
            sort_order: 0,
            completed_at: None,
            repeat_rule: None,
            pinned: 0,
        };
        let mut editor = Self::from_note(note, None);
        editor.original_note = None;
        editor.appointments.clear();
        editor.checklist.clear();
        editor
    }

    pub fn edit_note(note: &Note) -> Self {
        Self::from_note(note.clone(), Some(note.clone()))
    }

    fn from_note(note: Note, original: Option<Note>) -> Self {
        let fields = vec![
            Field::text("title", "Titel", &note.title),
            Field::choice("status", "Status", &note.status, STATUSES),
            Field::choice("priority", "Priorität", &note.priority, PRIORITIES),
            Field::text("due", "Fällig (YYYY-MM-DD)", note.due_date.as_deref().unwrap_or("")),
            Field::choice("repeat", "Wiederholung", note.repeat_rule.as_deref().unwrap_or("none"), REPEATS),
            Field::text(
                "departments",
                "Departments",
                &departments_text(note.departments.as_deref()),
            ),
            Field::multiline("content", "Inhalt (Markdown)", &content_text(&note.content)),
        ];
        let mut appointments = Vec::new();
        let mut checklist = Vec::new();
        if let Some(appts) = &note.appointments {
            if let Ok(v) = serde_json::from_str::<Value>(appts) {
                if let Some(arr) = v.as_array() {
                    for a in arr {
                        let get = |k: &str, alt: &str| {
                            a.get(k).or_else(|| a.get(alt)).and_then(|x| x.as_str()).unwrap_or("").to_string()
                        };
                        let mut extra = a.as_object().cloned().unwrap_or_default();
                        for k in ["start", "date", "time", "title", "text"] {
                            extra.remove(k);
                        }
                        appointments.push(ApptEdit {
                            date: get("start", "date"),
                            time: get("time", "time"),
                            text: get("title", "text"),
                            extra,
                        });
                    }
                }
            }
        }
        if let Ok(v) = serde_json::from_str::<Value>(&note.content) {
            if let Some(arr) = v.get("checklist").and_then(|x| x.as_array()) {
                for c in arr {
                    checklist.push(CheckEdit {
                        done: c.get("done").and_then(|x| x.as_bool()).unwrap_or(false),
                        text: c.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    });
                }
            }
        }
        Editor {
            target: Target::Note,
            fields,
            focus: 0,
            error: None,
            scroll: 0,
            original_note: original,
            original_contact: None,
            original_wiki: None,
            appointments,
            checklist,
            list_focus: ListFocus::None,
            list_idx: 0,
            candidates: Vec::new(),
            suggest: None,
            suppress: false,
        }
    }

    pub fn new_contact() -> Self {
        Self::from_contact(
            Contact {
                id: 0,
                name: String::new(),
                title: None,
                department: None,
                departments: Some("[]".to_string()),
                phone: None,
                mobile: None,
                fax: None,
                email: None,
                description: None,
            },
            None,
        )
    }

    pub fn edit_contact(contact: &Contact) -> Self {
        Self::from_contact(contact.clone(), Some(contact.clone()))
    }

    fn from_contact(contact: Contact, original: Option<Contact>) -> Self {
        let fields = vec![
            Field::text("name", "Name", &contact.name),
            Field::text("title", "Titel", contact.title.as_deref().unwrap_or("")),
            Field::text(
                "departments",
                "Departments",
                &departments_text(contact.departments.as_deref()),
            ),
            Field::text("phone", "Telefon", contact.phone.as_deref().unwrap_or("")),
            Field::text("mobile", "Mobil", contact.mobile.as_deref().unwrap_or("")),
            Field::text("fax", "Fax", contact.fax.as_deref().unwrap_or("")),
            Field::text("email", "E-Mail", contact.email.as_deref().unwrap_or("")),
            Field::multiline(
                "description",
                "Beschreibung",
                contact.description.as_deref().unwrap_or(""),
            ),
        ];
        Editor {
            target: Target::Contact,
            fields,
            focus: 0,
            error: None,
            scroll: 0,
            original_note: None,
            original_contact: original,
            original_wiki: None,
            appointments: Vec::new(),
            checklist: Vec::new(),
            list_focus: ListFocus::None,
            list_idx: 0,
            candidates: Vec::new(),
            suggest: None,
            suppress: false,
        }
    }

    pub fn new_wiki() -> Self {
        Self::from_wiki(
            WikiPage {
                id: 0,
                title: String::new(),
                content: None,
                created_at: None,
                updated_at: None,
            },
            None,
        )
    }

    pub fn edit_wiki(page: &WikiPage) -> Self {
        Self::from_wiki(page.clone(), Some(page.clone()))
    }

    fn from_wiki(page: WikiPage, original: Option<WikiPage>) -> Self {
        let fields = vec![
            Field::text("title", "Titel", &page.title),
            Field::multiline("content", "Inhalt (Markdown, [[Verlinkung]])", page.content.as_deref().unwrap_or("")),
        ];
        Editor {
            target: Target::Wiki,
            fields,
            focus: 0,
            error: None,
            scroll: 0,
            original_note: None,
            original_contact: None,
            original_wiki: original,
            appointments: Vec::new(),
            checklist: Vec::new(),
            list_focus: ListFocus::None,
            list_idx: 0,
            candidates: Vec::new(),
            suggest: None,
            suppress: false,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.target {
            Target::Note if self.original_note.is_some() => "Notiz bearbeiten",
            Target::Note => "Neue Notiz",
            Target::Contact if self.original_contact.is_some() => "Kontakt bearbeiten",
            Target::Contact => "Neuer Kontakt",
            Target::Wiki if self.original_wiki.is_some() => "Wiki-Seite bearbeiten",
            Target::Wiki => "Neue Wiki-Seite",
        }
    }

    fn field(&self, key: &str) -> &Field {
        self.fields
            .iter()
            .find(|field| field.key == key)
            .expect("editor field")
    }

    fn field_mut(&mut self, key: &str) -> &mut Field {
        self.fields
            .iter_mut()
            .find(|field| field.key == key)
            .expect("editor field")
    }

    fn value(&self, key: &str) -> String {
        self.field(key).value()
    }

    fn next_focus(&mut self) {
        if !self.fields.is_empty() {
            self.focus = (self.focus + 1) % self.fields.len();
        }
    }

    fn previous_focus(&mut self) {
        if !self.fields.is_empty() {
            self.focus = (self.focus + self.fields.len() - 1) % self.fields.len();
        }
    }

    /// Vorschläge für Links: `(Anzeigetexte, Auswahlindex, Zeile, Spalte)` – Position relativ zum Cursor.
    pub fn suggestions(&self) -> Option<(Vec<String>, usize)> {
        let sg = self.suggest.as_ref()?;
        let (open, close) = Self::delims(sg.kind);
        Some((sg.items.iter().map(|t| format!("{open}{t}{close}")).collect(), sg.sel))
    }

    fn delims(kind: LinkKind) -> (&'static str, &'static str) {
        match kind {
            LinkKind::Note => ("[[", "]]"),
            LinkKind::Wiki => ("<<", ">>"),
            LinkKind::Contact => ("{{", "}}"),
        }
    }

    /// Prüft, ob der Cursor in einem offenen Link steht, und berechnet die Vorschlagsliste neu.
    fn update_suggest(&mut self) {
        self.suggest = None;
        if self.suppress {
            return;
        }
        let Some(field) = self.fields.get(self.focus) else { return };
        if !field.multiline {
            return;
        }
        let before = &field.chars[..field.cursor];
        let line_start = before.iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1);
        let line = &before[line_start..];
        // Spätester Öffner in dieser Zeile
        let mut best: Option<(usize, LinkKind)> = None;
        for i in 0..line.len().saturating_sub(1) {
            let kind = match (line[i], line[i + 1]) {
                ('[', '[') => LinkKind::Note,
                ('<', '<') => LinkKind::Wiki,
                ('{', '{') => LinkKind::Contact,
                _ => continue,
            };
            best = Some((i, kind));
        }
        let Some((i, kind)) = best else { return };
        let typed: String = line[i + 2..].iter().collect();
        let (_, close) = Self::delims(kind);
        // Bereits geschlossen oder Anzeigename-Teil (`|`) → keine Vorschläge
        if typed.contains(close.chars().next().unwrap()) || typed.contains('|') {
            return;
        }
        let needle = typed.to_lowercase();
        let items: Vec<String> = self
            .candidates
            .iter()
            .filter(|(k, name)| *k == kind && name.to_lowercase().contains(&needle) && !name.contains(['|', ']', '>', '}']))
            .map(|(_, name)| name.clone())
            .take(8)
            .collect();
        if items.is_empty() {
            return;
        }
        let prev_sel = 0;
        self.suggest = Some(Suggest { kind, start: line_start + i, items, sel: prev_sel });
    }

    fn accept_suggest(&mut self) {
        let Some(sg) = self.suggest.take() else { return };
        let Some(title) = sg.items.get(sg.sel).cloned() else { return };
        let (_, close) = Self::delims(sg.kind);
        if let Some(field) = self.fields.get_mut(self.focus) {
            let tail: Vec<char> = field.chars[field.cursor..].to_vec();
            field.chars.truncate(sg.start + 2);
            field.chars.extend(title.chars());
            field.chars.extend(close.chars());
            field.cursor = field.chars.len();
            // Schließer, der schon hinter dem Cursor stand, nicht doppeln
            let skip = if tail.starts_with(&close.chars().collect::<Vec<_>>()) { close.chars().count() } else { 0 };
            field.chars.extend(tail.into_iter().skip(skip));
        }
        self.suppress = true;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> EditorAction {
        let action = self.handle_key_inner(key);
        self.update_suggest();
        action
    }

    fn handle_key_inner(&mut self, key: KeyEvent) -> EditorAction {
        // Link-Vorschläge haben Vorrang vor der normalen Bedienung
        if let Some(sg) = self.suggest.as_mut() {
            let plain = !key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Down => {
                    sg.sel = (sg.sel + 1) % sg.items.len();
                    return EditorAction::None;
                }
                KeyCode::Up => {
                    sg.sel = (sg.sel + sg.items.len() - 1) % sg.items.len();
                    return EditorAction::None;
                }
                KeyCode::Char('n') if !plain => {
                    sg.sel = (sg.sel + 1) % sg.items.len();
                    return EditorAction::None;
                }
                KeyCode::Char('p') if !plain => {
                    sg.sel = (sg.sel + sg.items.len() - 1) % sg.items.len();
                    return EditorAction::None;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    self.accept_suggest();
                    return EditorAction::None;
                }
                KeyCode::Esc => {
                    self.suggest = None;
                    self.suppress = true;
                    return EditorAction::None;
                }
                _ => {}
            }
        }
        // Jede Texteingabe hebt die Unterdrückung (nach Esc/Übernahme) wieder auf
        if matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace) && !key.modifiers.contains(KeyModifiers::CONTROL) {
            self.suppress = false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('s') => return EditorAction::Save,
                KeyCode::Char('c') => return EditorAction::Cancel,
                _ => {}
            }
        }

        // h/j/k/l navigieren nur auf Auswahlfeldern; in Textfeldern sind es normale Zeichen.
        let on_choice = self.fields.get(self.focus).is_some_and(|f| f.choices.is_some());
        let plain = !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
        let vim = |c: char| on_choice && plain && key.code == KeyCode::Char(c);

        match key.code {
            KeyCode::Esc => return EditorAction::Cancel,
            KeyCode::Tab | KeyCode::Down => self.next_focus(),
            KeyCode::BackTab | KeyCode::Up => self.previous_focus(),
            _ if vim('j') => self.next_focus(),
            _ if vim('k') => self.previous_focus(),
            KeyCode::Left => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    if field.choices.is_some() {
                        field.cycle(-1);
                    } else {
                        field.move_left();
                    }
                }
            }
            KeyCode::Right => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    if field.choices.is_some() {
                        field.cycle(1);
                    } else {
                        field.move_right();
                    }
                }
            }
            _ if vim('h') => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    if field.choices.is_some() {
                        field.cycle(-1);
                    } else {
                        field.move_left();
                    }
                }
            }
            _ if vim('l') => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    if field.choices.is_some() {
                        field.cycle(1);
                    } else {
                        field.move_right();
                    }
                }
            }
            KeyCode::Home => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    if field.choices.is_some() {
                        field.cycle(-100);
                    } else {
                        field.home();
                    }
                }
            }
            KeyCode::End => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    if field.choices.is_some() {
                        field.cycle(100);
                    } else {
                        field.end();
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    field.backspace();
                }
            }
            KeyCode::Delete => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    field.delete();
                }
            }
            KeyCode::Enter => {
                let multiline = self.fields.get(self.focus).is_some_and(|field| field.multiline);
                if multiline {
                    if let Some(field) = self.fields.get_mut(self.focus) {
                        field.insert('\n');
                    }
                } else {
                    self.next_focus();
                }
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    if field.choices.is_none() {
                        field.insert(ch);
                    }
                }
            }
            _ => {}
        }

        self.error = None;
        EditorAction::None
    }

    pub fn save_data(&self) -> Result<SaveData, String> {
        match self.target {
            Target::Note => self.note_data().map(SaveData::Note),
            Target::Contact => self.contact_data().map(SaveData::Contact),
            Target::Wiki => self.wiki_data().map(SaveData::Wiki),
        }
    }

    fn note_data(&self) -> Result<NoteDraft, String> {
        let title = self.value("title").trim().to_string();
        if title.is_empty() {
            return Err("Titel darf nicht leer sein.".to_string());
        }
        let status = self.value("status");
        let priority = self.value("priority");
        let due_raw = self.value("due").trim().to_string();
        let due_date = if due_raw.is_empty() {
            None
        } else {
            if chrono::NaiveDate::parse_from_str(&due_raw, "%Y-%m-%d").is_err() {
                return Err("Fälligkeitsdatum muss YYYY-MM-DD sein.".to_string());
            }
            Some(due_raw)
        };
        let repeat = self.value("repeat");
        let repeat_rule = if repeat == "none" { None } else { Some(repeat) };
        let departments = departments_json(&self.value("departments"));
        let department = first_department(&Some(departments.clone()));
        let checklist_arr: Vec<Value> = self
            .checklist
            .iter()
            .map(|c| json!({ "done": c.done, "text": c.text }))
            .collect();
        let appointments_arr: Vec<Value> = self
            .appointments
            .iter()
            .filter(|a| !a.text.trim().is_empty() || !a.date.trim().is_empty() || !a.time.trim().is_empty())
            .map(|a| {
                let mut o = a.extra.clone();
                o.insert("title".into(), json!(a.text));
                o.insert("start".into(), json!(a.date));
                if !a.time.is_empty() {
                    o.insert("time".into(), json!(a.time));
                }
                Value::Object(o)
            })
            .collect();
        let content = json!({
            "text": self.value("content"),
            "checklist": checklist_arr
        })
        .to_string();

        let (id, orig_appointments, date, sort_order, pinned, original_status, original_completed_at) =
            match &self.original_note {
                Some(note) => (
                    Some(note.id),
                    note.appointments.clone(),
                    Some(note.date.clone()),
                    Some(note.sort_order),
                    note.pinned,
                    Some(note.status.clone()),
                    note.completed_at.clone(),
                ),
                None => (None, None, None, None, 0, None, None),
            };
        let appointments_out = if appointments_arr.is_empty() { orig_appointments } else { Some(json!(appointments_arr).to_string()) };

        Ok(NoteDraft {
            id,
            title,
            content,
            status,
            priority,
            due_date,
            department,
            departments,
            appointments: appointments_out,
            repeat_rule,
            date,
            sort_order,
            pinned,
            original_status,
            original_completed_at,
        })
    }

    fn contact_data(&self) -> Result<ContactDraft, String> {
        let name = self.value("name").trim().to_string();
        if name.is_empty() {
            return Err("Name darf nicht leer sein.".to_string());
        }
        let departments = departments_json(&self.value("departments"));
        let department = first_department(&Some(departments.clone()));
        let optional = |key: &str| {
            let value = self.value(key).trim().to_string();
            if value.is_empty() { None } else { Some(value) }
        };

        Ok(ContactDraft {
            id: self.original_contact.as_ref().map(|contact| contact.id),
            name,
            title: optional("title"),
            department,
            departments,
            phone: optional("phone"),
            mobile: optional("mobile"),
            fax: optional("fax"),
            email: optional("email"),
            description: optional("description"),
        })
    }

    fn wiki_data(&self) -> Result<WikiDraft, String> {
        let title = self.value("title").trim().to_string();
        if title.is_empty() {
            return Err("Title must not be empty.".to_string());
        }
        let content = self.value("content").trim().to_string();

        Ok(WikiDraft {
            id: self.original_wiki.as_ref().map(|page| page.id),
            title,
            content: if content.is_empty() { None } else { Some(content) },
        })
    }

    pub fn lines(&self) -> (Vec<Line<'static>>, Option<(usize, usize)>) {
        let mut lines = Vec::new();
        let mut cursor = None;
        let label_style = Style::default()
            .fg(th::accent_text())
            .add_modifier(Modifier::BOLD);
        let value_style = Style::default().fg(th::fg());
        let dim_style = Style::default().fg(th::muted());

        for (index, field) in self.fields.iter().enumerate() {
            let focused = index == self.focus;
            let style = if focused { label_style } else { dim_style };

            if field.multiline {
                lines.push(Line::from(Span::styled(format!("{}:", field.label), style)));

                let value = field.value();
                let value_lines: Vec<&str> = value.split('\n').collect();
                let (cursor_line, cursor_column) = field.line_col();

                for (line_index, text) in value_lines.iter().enumerate() {
                    lines.push(Line::from(vec![
                        Span::styled("  ", value_style),
                        Span::styled((*text).to_string(), value_style),
                    ]));

                    if focused && line_index == cursor_line {
                        cursor = Some((lines.len() - 1, 2 + cursor_column));
                    }
                }
            } else {
                let value = field.value();
                let display = if field.choices.is_some() {
                    format!("[ {} ]", value)
                } else {
                    value
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{}: ", field.label), style),
                    Span::styled(display, value_style),
                ]));

                if focused {
                    let offset = field.label.chars().count() + 2;
                    let extra = if field.choices.is_some() { 2 } else { 0 };
                    cursor = Some((lines.len() - 1, offset + extra + field.cursor));
                }
            }

            lines.push(Line::from(""));
        }

        if let Some(error) = &self.error {
            lines.push(Line::from(Span::styled(
                error.clone(),
                Style::default().fg(th::red()),
            )));
        }

        (lines, cursor)
    }

    pub fn ensure_cursor_visible(&mut self, height: u16) {
        if height == 0 {
            return;
        }
        let (lines, cursor) = self.lines();
        let Some((line, _)) = cursor else {
            return;
        };
        let scroll = self.scroll as usize;
        if line < scroll {
            self.scroll = line as u16;
        } else if line >= scroll + height as usize {
            self.scroll = line.saturating_sub(height as usize - 1) as u16;
        } else if self.scroll as usize >= lines.len() {
            self.scroll = lines.len().saturating_sub(1) as u16;
        }
    }
}

fn departments_text(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return String::new();
    };
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| {
            value.as_array().map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
        })
        .unwrap_or_default()
}

fn content_text(raw: &str) -> String {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("text")
                .and_then(|text| text.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| raw.to_string())
}

fn departments_json(raw: &str) -> String {
    let mut names: Vec<String> = Vec::new();
    for part in raw.split(',') {
        let name = part.trim();
        if name.is_empty() {
            continue;
        }
        if !names
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(name))
        {
            names.push(name.to_string());
        }
    }
    serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string())
}

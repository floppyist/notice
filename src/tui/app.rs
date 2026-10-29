use std::collections::HashSet;
use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{backend::Backend, Terminal};
use sqlx::SqlitePool;

use crate::models::{AppState, Contact, Note, WikiPage};
use crate::recurrence;
use crate::tui::editor::{ContactDraft, Editor, EditorAction, NoteDraft, SaveData, WikiDraft};
use crate::tui::links::{self, link_key, Link, LinkKind};
use crate::tui::render;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Kanban,
    Notes,
    Wiki,
    Data,
    Calendar,
    Archive,
    Trash,
}

impl View {
    pub fn title(self) -> &'static str {
        match self {
            View::Dashboard => "Dashboard",
            View::Kanban => "Kanban",
            View::Notes => "Notizen",
            View::Wiki => "Wiki",
            View::Data => "Daten",
            View::Calendar => "Kalender",
            View::Archive => "Archiv",
            View::Trash => "Papierkorb",
        }
    }

    pub fn index(self) -> usize {
        match self {
            View::Dashboard => 0,
            View::Kanban => 1,
            View::Notes => 2,
            View::Wiki => 3,
            View::Data => 4,
            View::Calendar => 5,
            View::Archive => 6,
            View::Trash => 7,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Detail,
}

fn status_rank(status: &str) -> u8 {
    match status {
        "backlog" => 0,
        "in_progress" => 1,
        "review" => 2,
        "done" => 3,
        _ => 4,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NoteFilter {
    All,
    Backlog,
    InProgress,
    Review,
    Done,
}

impl NoteFilter {
    pub(crate) fn next(self) -> Self {
        match self {
            NoteFilter::All => NoteFilter::Backlog,
            NoteFilter::Backlog => NoteFilter::InProgress,
            NoteFilter::InProgress => NoteFilter::Review,
            NoteFilter::Review => NoteFilter::Done,
            NoteFilter::Done => NoteFilter::All,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            NoteFilter::All => "alle",
            NoteFilter::Backlog => "backlog",
            NoteFilter::InProgress => "in_progress",
            NoteFilter::Review => "review",
            NoteFilter::Done => "done",
        }
    }

    pub(crate) fn matches(self, note: &Note) -> bool {
        match self {
            NoteFilter::All => true,
            NoteFilter::Backlog => note.status == "backlog",
            NoteFilter::InProgress => note.status == "in_progress",
            NoteFilter::Review => note.status == "review",
            NoteFilter::Done => note.status == "done",
        }
    }
}

#[derive(Clone)]
pub struct TrashItem {
    pub note: Option<Note>,
    pub contact: Option<Contact>,
}

impl TrashItem {
    pub fn title(&self) -> &str {
        if let Some(note) = &self.note {
            &note.title
        } else if let Some(contact) = &self.contact {
            &contact.name
        } else {
            ""
        }
    }

    pub fn is_note(&self) -> bool {
        self.note.is_some()
    }
}

pub struct Confirm {
    pub message: String,
    pub(crate) action: ConfirmAction,
}

#[derive(Clone)]
pub(crate) enum ConfirmAction {
    DeleteNote(i64),
    ArchiveNote(i64),
    DeleteContact(i64),
    DeleteWiki(i64),
    RestoreArchive(i64),
    PurgeArchive(i64),
    RestoreTrashNote(i64),
    PurgeTrashNote(i64),
    RestoreTrashContact(i64),
    PurgeTrashContact(i64),
    ClearTrash,
    DeleteDepartment(String),
}

#[derive(Clone)]
pub struct Department {
    pub name: String,
    pub count: i64,
}

#[derive(Clone)]
pub struct AutocompleteItem {
    pub id: i64,
    pub name: String,
    pub kind: AutocompleteKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AutocompleteKind {
    Note,
    Wiki,
    Contact,
}

pub struct App {
    pub state: AppState,
    pub view: View,
    pub focus: Focus,
    pub notes: Vec<Note>,
    pub archive: Vec<Note>,
    pub trash: Vec<TrashItem>,
    pub contacts: Vec<Contact>,
    pub wiki: Vec<WikiPage>,
    pub departments: Vec<Department>,
    pub selected: usize,
    pub detail_scroll: u16,
    pub filter: NoteFilter,
    pub search: String,
    pub search_edit: bool,
    pub status: String,
    pub editor: Option<Editor>,
    pub confirm: Option<Confirm>,
    pub help: bool,
    pub should_quit: bool,
    pub normal_mode: bool,
    pub g_pressed: bool,
    pub pending_d: bool,
    pub command: String,
    pub command_mode: bool,
    pub autocomplete: Option<Vec<AutocompleteItem>>,
    pub autocomplete_index: usize,
    pub link_mode: Option<AutocompleteKind>,
    pub wiki_links: HashSet<String>,
    pub pending_links: Option<Vec<Link>>,
}

impl App {
    pub fn new(state: AppState) -> Self {
        App {
            state,
            view: View::Dashboard,
            focus: Focus::List,
            notes: Vec::new(),
            archive: Vec::new(),
            trash: Vec::new(),
            contacts: Vec::new(),
            wiki: Vec::new(),
            departments: Vec::new(),
            selected: 0,
            detail_scroll: 0,
            filter: NoteFilter::All,
            search: String::new(),
            search_edit: false,
            status: String::new(),
            editor: None,
            confirm: None,
            help: false,
            should_quit: false,
            normal_mode: true,
            g_pressed: false,
            pending_d: false,
            command: String::new(),
            command_mode: false,
            autocomplete: None,
            autocomplete_index: 0,
            link_mode: None,
            wiki_links: HashSet::new(),
            pending_links: None,
        }
    }

    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()>
    where
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        let _ = self.reload().await;
        let _ = self.load_departments().await;
        let _ = self.load_wiki_links().await;

        loop {
            terminal
                .draw(|frame| render::render(self, frame))
                .map_err(io::Error::other)?;

            if self.should_quit {
                break;
            }

            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        self.handle_key(key).await;
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
        }

        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        if self.help {
            self.help = false;
            self.g_pressed = false;
            self.pending_d = false;
            return;
        }

        if self.confirm.is_some() {
            self.handle_confirm_key(key).await;
            return;
        }

        if self.editor.is_some() {
            self.handle_editor_key(key).await;
            return;
        }

        if self.command_mode {
            self.handle_command_key(key).await;
            return;
        }

        if self.search_edit {
            self.handle_search_key(key);
            return;
        }

        if self.autocomplete.is_some() {
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(items) = &self.autocomplete {
                        if !items.is_empty() {
                            self.autocomplete_index = (self.autocomplete_index + 1) % items.len();
                        }
                    }
                    return;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(items) = &self.autocomplete {
                        if !items.is_empty() {
                            self.autocomplete_index =
                                (self.autocomplete_index + items.len() - 1) % items.len();
                        }
                    }
                    return;
                }
                KeyCode::Enter | KeyCode::Tab => {
                    self.accept_autocomplete();
                    return;
                }
                KeyCode::Esc => {
                    self.autocomplete = None;
                    self.link_mode = None;
                    return;
                }
                _ => {
                    self.autocomplete = None;
                    self.link_mode = None;
                }
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        self.handle_normal_key(key).await;
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) {
        if let Some(links) = self.pending_links.take() {
            if let KeyCode::Char(c) = key.code {
                if let Some(d) = c.to_digit(10) {
                    if d >= 1 {
                        if let Some(l) = links.get(d as usize - 1) {
                            self.jump_to(&l.clone());
                            return;
                        }
                    }
                }
            }
        }
        match key.code {
            KeyCode::Char('o') => {
                self.follow_links();
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char(':') => {
                self.command_mode = true;
                self.command.clear();
                return;
            }
            KeyCode::Char('?') => {
                self.help = true;
                return;
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('h') => {
                if self.view == View::Kanban && self.focus == Focus::List {
                    self.kanban_move_column(-1);
                } else {
                    self.focus = Focus::List;
                }
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('l') => {
                if self.view == View::Kanban && self.focus == Focus::List {
                    self.kanban_move_column(1);
                } else if self.has_detail() {
                    self.focus = Focus::Detail;
                }
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.focus == Focus::Detail {
                    self.detail_scroll = self.detail_scroll.saturating_add(1);
                } else {
                    self.move_selection(1);
                }
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.focus == Focus::Detail {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                } else {
                    self.move_selection(-1);
                }
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('g') => {
                if self.g_pressed {
                    if self.focus == Focus::List {
                        self.selected = 0;
                    } else {
                        self.detail_scroll = 0;
                    }
                    self.g_pressed = false;
                } else {
                    self.g_pressed = true;
                }
                self.pending_d = false;
                return;
            }
            KeyCode::Char('G') => {
                if self.focus == Focus::List {
                    self.selected = self.list_len().saturating_sub(1);
                } else {
                    self.detail_scroll = u16::MAX / 2;
                }
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('0') => {
                if self.focus == Focus::Detail {
                    self.detail_scroll = 0;
                }
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('H') => {
                self.focus = Focus::List;
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('L') => {
                if self.has_detail() {
                    self.focus = Focus::Detail;
                }
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Tab => {
                if self.has_detail() {
                    self.focus = match self.focus {
                        Focus::List => Focus::Detail,
                        Focus::Detail => Focus::List,
                    };
                }
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('1') => { self.set_view(View::Dashboard); self.g_pressed = false; self.pending_d = false; return; }
            KeyCode::Char('2') => { self.set_view(View::Kanban); self.g_pressed = false; self.pending_d = false; return; }
            KeyCode::Char('3') => { self.set_view(View::Notes); self.g_pressed = false; self.pending_d = false; return; }
            KeyCode::Char('4') => { self.set_view(View::Wiki); self.g_pressed = false; self.pending_d = false; return; }
            KeyCode::Char('5') => { self.set_view(View::Data); self.g_pressed = false; self.pending_d = false; return; }
            KeyCode::Char('6') => { self.set_view(View::Calendar); self.g_pressed = false; self.pending_d = false; return; }
            KeyCode::Char('7') => { self.set_view(View::Archive); self.g_pressed = false; self.pending_d = false; return; }
            KeyCode::Char('8') => { self.set_view(View::Trash); self.g_pressed = false; self.pending_d = false; return; }
            KeyCode::Char('/') => {
                self.search_edit = true;
                self.search.clear();
                self.focus = Focus::List;
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('n') => {
                self.new_selected();
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('f') if self.view == View::Notes || self.view == View::Kanban => {
                self.filter = self.filter.next();
                self.selected = 0;
                self.detail_scroll = 0;
                self.status = format!("Filter: {}", self.filter.label());
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('r') => {
                let status = match self.reload().await {
                    Ok(()) => {
                        let _ = self.load_departments().await;
                        let _ = self.load_wiki_links().await;
                        format!("Aktualisiert")
                    }
                    Err(error) => error,
                };
                self.status = status;
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('e') => {
                self.edit_selected();
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Enter => {
                if self.has_detail() && self.focus == Focus::List {
                    self.focus = Focus::Detail;
                } else if self.view == View::Dashboard || self.view == View::Kanban {
                    self.edit_selected();
                } else {
                    self.edit_selected();
                }
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('A') if self.view == View::Notes || self.view == View::Kanban => {
                self.request_archive();
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('d') => {
                if self.pending_d {
                    self.request_delete();
                    self.pending_d = false;
                } else {
                    self.pending_d = true;
                }
                self.g_pressed = false;
                return;
            }
            KeyCode::Char('u') => {
                self.request_restore();
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('x') | KeyCode::Char('D') => {
                self.request_purge();
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('c') if self.view == View::Trash => {
                self.request_clear_trash();
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('i') => {
                self.edit_selected();
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Char('a') => {
                self.new_selected();
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            KeyCode::Esc => {
                if self.search_edit {
                    self.search.clear();
                    self.search_edit = false;
                } else if !self.search.is_empty() {
                    self.search.clear();
                } else if self.filter != NoteFilter::All {
                    self.filter = NoteFilter::All;
                }
                self.selected = 0;
                self.detail_scroll = 0;
                self.g_pressed = false;
                self.pending_d = false;
                return;
            }
            _ => {}
        }
        self.g_pressed = false;
        self.pending_d = false;
    }



    async fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.command_mode = false;
                self.command.clear();
            }
            KeyCode::Enter => {
                self.execute_command().await;
                self.command_mode = false;
                self.command.clear();
            }
            KeyCode::Backspace => {
                self.command.pop();
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.command.push(ch);
            }
            _ => {}
        }
    }

    async fn execute_command(&mut self) {
        let cmd = self.command.trim().to_string();
        match cmd.as_str() {
            "q" | "quit" => self.should_quit = true,
            "w" | "write" => {
                if let Some(editor) = self.editor.as_mut() {
                    if let Err(e) = editor.save_data() {
                        self.status = e;
                    }
                } else {
                    self.status = "Nichts zu speichern".to_string();
                }
            }
            "wq" => {
                if let Some(editor) = self.editor.as_mut() {
                    if let Err(e) = editor.save_data() {
                        self.status = e;
                        self.should_quit = false;
                    } else {
                        self.should_quit = true;
                    }
                } else {
                    self.should_quit = true;
                }
            }
            "help" => self.help = true,
            "reload" | "r" => {
                let _ = self.reload().await;
                let _ = self.load_departments().await;
                self.status = "Aktualisiert".to_string();
            }
            "dashboard" => self.set_view(View::Dashboard),
            "kanban" => self.set_view(View::Kanban),
            "notes" | "notizen" => self.set_view(View::Notes),
            "archive" | "archiv" => self.set_view(View::Archive),
            "trash" | "papierkorb" => self.set_view(View::Trash),
            "wiki" => self.set_view(View::Wiki),
            "data" | "daten" => self.set_view(View::Data),
            "calendar" | "kalender" => self.set_view(View::Calendar),
            cmd if cmd.starts_with("new-dept ") => {
                let name = cmd.strip_prefix("new-dept ").unwrap_or("").trim().to_string();
                if !name.is_empty() {
                    let _ = sqlx::query("INSERT OR IGNORE INTO departments_store (name) VALUES (?)").bind(&name).execute(&self.state.pool).await;
                    let _ = self.load_departments().await;
                    self.status = format!("Department '{}' hinzugefügt", name);
                }
            }
            _ => {
                if cmd.starts_with("set ") {
                    self.status = "Option gesetzt".to_string();
                } else {
                    self.status = format!("Unbekanntes Kommando: {}", cmd);
                }
            }
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.search_edit = false,
            KeyCode::Esc => {
                self.search.clear();
                self.search_edit = false;
                self.selected = 0;
                self.detail_scroll = 0;
            }
            KeyCode::Backspace => {
                self.search.pop();
                self.selected = 0;
                self.detail_scroll = 0;
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.search.push(ch);
                self.selected = 0;
                self.detail_scroll = 0;
            }
            _ => {}
        }
    }

    async fn handle_editor_key(&mut self, key: KeyEvent) {
        let mut candidates: Vec<(LinkKind, String)> = Vec::new();
        candidates.extend(self.notes.iter().map(|n| (LinkKind::Note, n.title.clone())));
        candidates.extend(self.wiki.iter().map(|w| (LinkKind::Wiki, w.title.clone())));
        candidates.extend(self.contacts.iter().map(|c| (LinkKind::Contact, c.name.clone())));
        if let Some(editor) = self.editor.as_mut() {
            editor.candidates = candidates;
        }
        let action = self
            .editor
            .as_mut()
            .map(|editor| editor.handle_key(key))
            .unwrap_or(EditorAction::None);

        match action {
            EditorAction::None => {}
            EditorAction::Cancel => {
                self.editor = None;
                self.status = "Abgebrochen".to_string();
            }
            EditorAction::Save => self.save_editor().await,
        }
    }

    async fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(confirm) = self.confirm.take() {
                    self.execute_confirm(confirm.action).await;
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.confirm = None;
                self.status = "Abgebrochen".to_string();
            }
            _ => {}
        }
    }

    fn set_view(&mut self, view: View) {
        self.view = view;
        self.focus = Focus::List;
        self.selected = 0;
        self.detail_scroll = 0;
        self.search_edit = false;
        self.autocomplete = None;
        self.g_pressed = false;
        self.pending_d = false;
        self.clamp_selection();
    }

    fn has_detail(&self) -> bool {
        matches!(self.view, View::Notes | View::Archive | View::Wiki | View::Trash | View::Data | View::Kanban | View::Calendar)
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.list_len();
        if len == 0 {
            self.selected = 0;
            self.detail_scroll = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, (len as isize) - 1) as usize;
        self.detail_scroll = 0;
    }

    fn clamp_selection(&mut self) {
        let len = self.list_len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(len - 1);
        }
    }

    fn kanban_move_column(&mut self, delta: isize) {
        let target = {
            let list = self.kanban_list();
            if list.is_empty() {
                return;
            }
            let mut statuses: Vec<&str> = Vec::new();
            for note in &list {
                if !statuses.contains(&note.status.as_str()) {
                    statuses.push(note.status.as_str());
                }
            }
            statuses.sort_by_key(|status| status_rank(status));
            let current = list.get(self.selected).map(|n| n.status.as_str()).unwrap_or("");
            let pos = statuses.iter().position(|s| *s == current).unwrap_or(0);
            let next = (pos as isize + delta).clamp(0, statuses.len() as isize - 1) as usize;
            if next == pos {
                return;
            }
            let wanted = statuses[next];
            list.iter().position(|n| n.status == wanted)
        };
        if let Some(idx) = target {
            self.selected = idx;
            self.detail_scroll = 0;
        }
    }

    pub fn calendar_list(&self) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.due_date.is_some() && self.search_matches(&[&n.title, &n.content]))
            .collect();
        notes.sort_by(|a, b| a.due_date.cmp(&b.due_date));
        notes
    }

    pub fn selected_calendar_note(&self) -> Option<&Note> {
        self.calendar_list().get(self.selected).copied()
    }

    pub fn kanban_list(&self) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self.notes.iter().filter(|n| self.filter.matches(n) && self.search_matches(&[&n.title, &n.content])).collect();
        notes.sort_by(|a, b| status_rank(&a.status).cmp(&status_rank(&b.status)).then(a.sort_order.cmp(&b.sort_order)));
        notes
    }

    fn list_len(&self) -> usize {
        match self.view {
            View::Dashboard => 4,
            View::Kanban => self.kanban_list().len(),
            View::Notes => self.notes.iter().filter(|n| self.note_matches(n)).count(),
            View::Archive => self.archive.iter().filter(|n| self.search_matches(&[&n.title, &n.content])).count(),
            View::Trash => self.trash.iter().filter(|t| self.trash_matches(t)).count(),
            View::Wiki => self.wiki.iter().filter(|w| self.wiki_matches(w)).count(),
            View::Data => self.visible_depts().len() + self.visible_contacts().len(),
            View::Calendar => self.calendar_list().len(),
        }
    }

    pub fn note_matches(&self, note: &Note) -> bool {
        self.filter.matches(note) && self.search_matches(&[&note.title, &note.content])
    }

    fn contact_matches(&self, contact: &Contact) -> bool {
        self.search_matches(&[&contact.name, contact.title.as_deref().unwrap_or(""), contact.email.as_deref().unwrap_or("")])
    }

    fn wiki_matches(&self, page: &WikiPage) -> bool {
        self.search_matches(&[&page.title, page.content.as_deref().unwrap_or("")])
    }

    fn trash_matches(&self, item: &TrashItem) -> bool {
        self.search_matches(&[item.title()])
    }

    fn dept_matches(&self, dept: &Department) -> bool {
        self.search_matches(&[&dept.name])
    }

    fn search_matches(&self, parts: &[&str]) -> bool {
        let needle = self.search.trim().to_lowercase();
        if needle.is_empty() {
            return true;
        }
        parts.iter().any(|p| p.to_lowercase().contains(&needle))
    }

    pub fn selected_note(&self) -> Option<&Note> {
        self.notes.iter().filter(|n| self.note_matches(n)).nth(self.selected)
    }

    pub fn selected_archived(&self) -> Option<&Note> {
        self.archive.iter().filter(|n| self.search_matches(&[&n.title, &n.content])).nth(self.selected)
    }

    pub fn selected_contact(&self) -> Option<&Contact> {
        self.contacts.iter().filter(|c| self.contact_matches(c)).nth(self.selected)
    }

    pub fn selected_wiki(&self) -> Option<&WikiPage> {
        self.wiki.iter().filter(|w| self.wiki_matches(w)).nth(self.selected)
    }

    pub fn selected_trash(&self) -> Option<&TrashItem> {
        self.trash.iter().filter(|t| self.trash_matches(t)).nth(self.selected)
    }

    pub fn selected_dept(&self) -> Option<&Department> {
        self.departments.iter().filter(|d| self.dept_matches(d)).nth(self.selected)
    }

    pub fn visible_depts(&self) -> Vec<&Department> {
        self.departments.iter().filter(|d| self.dept_matches(d)).collect()
    }

    pub fn visible_contacts(&self) -> Vec<&Contact> {
        self.contacts.iter().filter(|c| self.contact_matches(c)).collect()
    }

    /// Kontakt unter der Auswahl in der Daten-Ansicht (Abteilungen stehen davor).
    pub fn selected_contact_data(&self) -> Option<&Contact> {
        let n = self.visible_depts().len();
        if self.selected < n { return None; }
        self.visible_contacts().get(self.selected - n).copied()
    }

    pub fn visible_archive(&self) -> Vec<&Note> {
        self.archive.iter().filter(|n| self.search_matches(&[&n.title, &n.content])).collect()
    }

    pub fn visible_wiki(&self) -> Vec<&WikiPage> {
        self.wiki.iter().filter(|w| self.wiki_matches(w)).collect()
    }

    pub fn visible_trash(&self) -> Vec<&TrashItem> {
        self.trash.iter().filter(|t| self.trash_matches(t)).collect()
    }

    fn new_selected(&mut self) {
        match self.view {
            View::Notes | View::Kanban | View::Calendar | View::Dashboard => {
                self.editor = Some(Editor::new_note());
            }
            View::Data => {
                if self.selected >= self.visible_depts().len() {
                    self.editor = Some(Editor::new_contact());
                } else {
                    self.command_mode = true;
                    self.command = "new-dept ".to_string();
                }
            }
            View::Wiki => self.editor = Some(Editor::new_wiki()),
            View::Archive | View::Trash => {
                self.status = "In dieser Ansicht kein Neuerstellen".to_string();
            }
        }
    }

    fn edit_selected(&mut self) {
        match self.view {
            View::Notes => {
                if let Some(note) = self.selected_note().cloned() {
                    self.editor = Some(Editor::edit_note(&note));
                }
            }
            View::Kanban => {
                let list = self.kanban_list();
                if let Some(note) = list.get(self.selected).cloned() {
                    self.editor = Some(Editor::edit_note(note));
                }
            }
            View::Wiki => {
                if let Some(page) = self.selected_wiki().cloned() {
                    self.editor = Some(Editor::edit_wiki(&page));
                } else {
                    self.editor = Some(Editor::new_wiki());
                }
            }
            View::Data => {
                if self.selected < self.visible_depts().len() {
                    self.status = "Department ausgewählt".to_string();
                } else if let Some(contact) = self.selected_contact_data().cloned() {
                    self.editor = Some(Editor::edit_contact(&contact));
                }
            }
            View::Archive | View::Trash | View::Dashboard => {
                self.status = "Drücke n zum Erstellen".to_string();
            }
            View::Calendar => {
                if let Some(note) = self.selected_calendar_note().cloned() {
                    self.editor = Some(Editor::edit_note(&note));
                }
            }
        }
    }

    fn request_archive(&mut self) {
        let note = match self.view {
            View::Notes => self.selected_note().cloned(),
            View::Kanban => self.kanban_list().get(self.selected).map(|n| (*n).clone()),
            _ => None,
        };
        if let Some(note) = note {
            self.confirm = Some(Confirm {
                message: format!("\"{}\" archivieren?", note.title),
                action: ConfirmAction::ArchiveNote(note.id),
            });
        }
    }

    fn request_delete(&mut self) {
        match self.view {
            View::Notes => {
                if let Some(note) = self.selected_note() {
                    self.confirm = Some(Confirm {
                        message: format!("\"{}\" in Papierkorb verschieben?", note.title),
                        action: ConfirmAction::DeleteNote(note.id),
                    });
                }
            }
            View::Kanban => {
                let note = self.kanban_list().get(self.selected).map(|n| (*n).clone());
                if let Some(note) = note {
                    self.confirm = Some(Confirm {
                        message: format!("\"{}\" in Papierkorb verschieben?", note.title),
                        action: ConfirmAction::DeleteNote(note.id),
                    });
                }
            }
            View::Data => {
                if self.selected < self.visible_depts().len() {
                    if let Some(dept) = self.selected_dept() {
                        self.confirm = Some(Confirm {
                            message: format!("Department \"{}\" löschen?", dept.name),
                            action: ConfirmAction::DeleteDepartment(dept.name.clone()),
                        });
                    }
                } else if let Some(contact) = self.selected_contact_data() {
                    self.confirm = Some(Confirm {
                        message: format!("\"{}\" in Papierkorb verschieben?", contact.name),
                        action: ConfirmAction::DeleteContact(contact.id),
                    });
                }
            }
            View::Wiki => {
                if let Some(page) = self.selected_wiki() {
                    self.confirm = Some(Confirm {
                        message: format!("Wiki-Seite \"{}\" löschen?", page.title),
                        action: ConfirmAction::DeleteWiki(page.id),
                    });
                }
            }
            _ => self.status = "Nutze x zum endgültigen Löschen".to_string(),
        }
    }

    fn request_restore(&mut self) {
        match self.view {
            View::Archive => {
                if let Some(note) = self.selected_archived() {
                    self.confirm = Some(Confirm {
                        message: format!("\"{}\" wiederherstellen?", note.title),
                        action: ConfirmAction::RestoreArchive(note.id),
                    });
                }
            }
            View::Trash => {
                if let Some(item) = self.selected_trash() {
                    if let Some(note) = &item.note {
                        self.confirm = Some(Confirm {
                            message: format!("\"{}\" wiederherstellen?", note.title),
                            action: ConfirmAction::RestoreTrashNote(note.id),
                        });
                    } else if let Some(contact) = &item.contact {
                        self.confirm = Some(Confirm {
                            message: format!("\"{}\" wiederherstellen?", contact.name),
                            action: ConfirmAction::RestoreTrashContact(contact.id),
                        });
                    }
                }
            }
            _ => self.status = "Nichts zum Wiederherstellen".to_string(),
        }
    }

    fn request_purge(&mut self) {
        match self.view {
            View::Archive => {
                if let Some(note) = self.selected_archived() {
                    self.confirm = Some(Confirm {
                        message: format!("\"{}\" endgültig löschen?", note.title),
                        action: ConfirmAction::PurgeArchive(note.id),
                    });
                }
            }
            View::Trash => {
                if let Some(item) = self.selected_trash() {
                    if let Some(note) = &item.note {
                        self.confirm = Some(Confirm {
                            message: format!("\"{}\" endgültig löschen?", note.title),
                            action: ConfirmAction::PurgeTrashNote(note.id),
                        });
                    } else if let Some(contact) = &item.contact {
                        self.confirm = Some(Confirm {
                            message: format!("\"{}\" endgültig löschen?", contact.name),
                            action: ConfirmAction::PurgeTrashContact(contact.id),
                        });
                    }
                }
            }
            _ => self.status = "Nutze d zum Verschieben in Papierkorb".to_string(),
        }
    }

    fn request_clear_trash(&mut self) {
        self.confirm = Some(Confirm {
            message: "Papierkorb leeren? Nicht rückgängig machbar.".to_string(),
            action: ConfirmAction::ClearTrash,
        });
    }

    async fn execute_confirm(&mut self, action: ConfirmAction) {
        let result = match action {
            ConfirmAction::DeleteNote(id) => self.delete_note(id).await,
            ConfirmAction::ArchiveNote(id) => self.archive_note(id).await,
            ConfirmAction::DeleteContact(id) => self.delete_contact(id).await,
            ConfirmAction::DeleteWiki(id) => self.delete_wiki(id).await,
            ConfirmAction::RestoreArchive(id) => self.restore_archive(id).await,
            ConfirmAction::PurgeArchive(id) => self.purge_archive(id).await,
            ConfirmAction::RestoreTrashNote(id) => self.restore_trash_note(id).await,
            ConfirmAction::PurgeTrashNote(id) => self.purge_trash_note(id).await,
            ConfirmAction::RestoreTrashContact(id) => self.restore_trash_contact(id).await,
            ConfirmAction::PurgeTrashContact(id) => self.purge_trash_contact(id).await,
            ConfirmAction::ClearTrash => self.clear_trash().await,
            ConfirmAction::DeleteDepartment(name) => self.delete_department(name).await,
        };

        match result {
            Ok(message) => {
                self.status = message;
                let _ = self.reload().await;
                let _ = self.load_departments().await;
            }
            Err(error) => self.status = error,
        }
    }

    async fn save_editor(&mut self) {
        let Some(editor) = self.editor.take() else {
            return;
        };

        let result = match editor.save_data() {
            Ok(data) => self.persist(data).await,
            Err(error) => Err(error),
        };

        match result {
            Ok(message) => {
                self.status = message;
                let _ = self.reload().await;
                let _ = self.load_departments().await;
                let _ = self.load_wiki_links().await;
            }
            Err(error) => {
                let mut editor = editor;
                editor.error = Some(error);
                self.editor = Some(editor);
            }
        }
    }

    async fn persist(&mut self, data: SaveData) -> Result<String, String> {
        match data {
            SaveData::Note(draft) => self.persist_note(draft).await,
            SaveData::Contact(draft) => self.persist_contact(draft).await,
            SaveData::Wiki(draft) => self.persist_wiki(draft).await,
        }
    }

    async fn persist_note(&self, draft: NoteDraft) -> Result<String, String> {
        let mut status = draft.status.clone();
        let mut due_date = draft.due_date.clone();
        let mut appointments = draft.appointments.clone();

        if status == "done" && draft.original_status.as_deref() != Some("done") {
            if let Some(rule) = draft.repeat_rule.as_deref() {
                let (next_due, next_appts) =
                    recurrence::apply_recurrence(rule, due_date.clone(), appointments.clone());
                if next_due != due_date || next_appts != appointments {
                    status = "backlog".to_string();
                    due_date = next_due;
                    appointments = next_appts;
                }
            }
        }

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let completed_at = if status == "done" {
            if draft.original_status.as_deref() == Some("done") {
                draft.original_completed_at.clone().or(Some(today.clone()))
            } else {
                Some(today.clone())
            }
        } else {
            None
        };

        let sort_order = match draft.sort_order {
            Some(value) => value,
            None => next_sort_order(&self.state.pool, &status).await?,
        };
        let date = draft.date.clone().unwrap_or_else(|| chrono::Local::now().format("%d.%m.%Y").to_string());

        if let Some(id) = draft.id {
            sqlx::query(
                r#"
                UPDATE notes SET title = ?, content = ?, status = ?, priority = ?, due_date = ?,
                    department = ?, departments = ?, appointments = ?, sort_order = ?, completed_at = ?,
                    repeat_rule = ?, pinned = ? WHERE id = ?
                "#,
            )
            .bind(&draft.title)
            .bind(&draft.content)
            .bind(&status)
            .bind(&draft.priority)
            .bind(&due_date)
            .bind(&draft.department)
            .bind(&draft.departments)
            .bind(&appointments)
            .bind(sort_order)
            .bind(&completed_at)
            .bind(&draft.repeat_rule)
            .bind(draft.pinned)
            .bind(id)
            .execute(&self.state.pool)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        } else {
            sqlx::query(
                r#"
                INSERT INTO notes (title, content, status, priority, date, due_date, department,
                    departments, appointments, sort_order, completed_at, repeat_rule, pinned)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&draft.title)
            .bind(&draft.content)
            .bind(&status)
            .bind(&draft.priority)
            .bind(&date)
            .bind(&due_date)
            .bind(&draft.department)
            .bind(&draft.departments)
            .bind(&appointments)
            .bind(sort_order)
            .bind(&completed_at)
            .bind(&draft.repeat_rule)
            .bind(draft.pinned)
            .execute(&self.state.pool)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        }

        Ok("Gespeichert".to_string())
    }

    async fn persist_contact(&self, draft: ContactDraft) -> Result<String, String> {
        if let Some(id) = draft.id {
            sqlx::query(
                "UPDATE contacts SET name = ?, title = ?, department = ?, departments = ?, phone = ?, mobile = ?, fax = ?, email = ?, description = ? WHERE id = ?",
            )
            .bind(&draft.name)
            .bind(&draft.title)
            .bind(&draft.department)
            .bind(&draft.departments)
            .bind(&draft.phone)
            .bind(&draft.mobile)
            .bind(&draft.fax)
            .bind(&draft.email)
            .bind(&draft.description)
            .bind(id)
            .execute(&self.state.db)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        } else {
            sqlx::query(
                "INSERT INTO contacts (name, title, department, departments, phone, mobile, fax, email, description) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&draft.name)
            .bind(&draft.title)
            .bind(&draft.department)
            .bind(&draft.departments)
            .bind(&draft.phone)
            .bind(&draft.mobile)
            .bind(&draft.fax)
            .bind(&draft.email)
            .bind(&draft.description)
            .execute(&self.state.db)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        }

        Ok("Gespeichert".to_string())
    }

    async fn persist_wiki(&self, draft: WikiDraft) -> Result<String, String> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        if let Some(id) = draft.id {
            sqlx::query("UPDATE wiki SET title = ?, content = ?, updated_at = ? WHERE id = ?")
                .bind(&draft.title)
                .bind(&draft.content)
                .bind(&now)
                .bind(id)
                .execute(&self.state.pool)
                .await
                .map_err(|error| format!("Fehler: {error}"))?;
        } else {
            sqlx::query("INSERT INTO wiki (title, content, created_at, updated_at) VALUES (?, ?, ?, ?)")
                .bind(&draft.title)
                .bind(&draft.content)
                .bind(&now)
                .bind(&now)
                .execute(&self.state.pool)
                .await
                .map_err(|error| format!("Fehler: {error}"))?;
        }

        Ok("Gespeichert".to_string())
    }

    async fn delete_note(&self, id: i64) -> Result<String, String> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        sqlx::query("UPDATE notes SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(now)
            .bind(id)
            .execute(&self.state.pool)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        Ok("In Papierkorb verschoben".to_string())
    }

    async fn archive_note(&self, id: i64) -> Result<String, String> {
        let note = sqlx::query_as::<_, Note>(
            "SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned FROM notes WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.state.pool)
        .await
        .map_err(|error| format!("Fehler: {error}"))?
        .ok_or_else(|| "Nicht gefunden".to_string())?;

        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        sqlx::query(
            r#"INSERT INTO notes (title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&note.title)
        .bind(&note.content)
        .bind(&note.status)
        .bind(&note.priority)
        .bind(&note.date)
        .bind(&note.due_date)
        .bind(&note.department)
        .bind(&note.departments)
        .bind(&note.appointments)
        .bind(note.sort_order)
        .bind(&now)
        .bind(&note.repeat_rule)
        .bind(note.pinned)
        .execute(&self.state.archive_pool)
        .await
        .map_err(|error| format!("Fehler: {error}"))?;

        sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(id)
            .execute(&self.state.pool)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;

        Ok("Archiviert".to_string())
    }

    async fn restore_archive(&self, id: i64) -> Result<String, String> {
        let note = sqlx::query_as::<_, Note>(
            "SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned FROM notes WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.state.archive_pool)
        .await
        .map_err(|error| format!("Fehler: {error}"))?
        .ok_or_else(|| "Nicht gefunden".to_string())?;

        let sort_order = next_sort_order(&self.state.pool, &note.status).await?;
        sqlx::query(
            r#"INSERT INTO notes (title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)"#,
        )
        .bind(&note.title)
        .bind(&note.content)
        .bind(&note.status)
        .bind(&note.priority)
        .bind(&note.date)
        .bind(&note.due_date)
        .bind(&note.department)
        .bind(&note.departments)
        .bind(&note.appointments)
        .bind(sort_order)
        .bind(&note.repeat_rule)
        .bind(note.pinned)
        .execute(&self.state.pool)
        .await
        .map_err(|error| format!("Fehler: {error}"))?;

        sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(id)
            .execute(&self.state.archive_pool)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;

        Ok("Wiederhergestellt".to_string())
    }

    async fn purge_archive(&self, id: i64) -> Result<String, String> {
        sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(id)
            .execute(&self.state.archive_pool)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        Ok("Endgültig gelöscht".to_string())
    }

    async fn restore_trash_note(&self, id: i64) -> Result<String, String> {
        sqlx::query("UPDATE notes SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(&self.state.pool)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        Ok("Wiederhergestellt".to_string())
    }

    async fn purge_trash_note(&self, id: i64) -> Result<String, String> {
        sqlx::query("DELETE FROM notes WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(&self.state.pool)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        Ok("Endgültig gelöscht".to_string())
    }

    async fn delete_contact(&self, id: i64) -> Result<String, String> {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        sqlx::query("UPDATE contacts SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(now)
            .bind(id)
            .execute(&self.state.db)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        Ok("In Papierkorb verschoben".to_string())
    }

    async fn restore_trash_contact(&self, id: i64) -> Result<String, String> {
        sqlx::query("UPDATE contacts SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(&self.state.db)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        Ok("Wiederhergestellt".to_string())
    }

    async fn purge_trash_contact(&self, id: i64) -> Result<String, String> {
        sqlx::query("DELETE FROM contacts WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(&self.state.db)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        Ok("Endgültig gelöscht".to_string())
    }

    async fn delete_wiki(&self, id: i64) -> Result<String, String> {
        sqlx::query("DELETE FROM wiki WHERE id = ?")
            .bind(id)
            .execute(&self.state.pool)
            .await
            .map_err(|error| format!("Fehler: {error}"))?;
        Ok("Gelöscht".to_string())
    }

    async fn clear_trash(&self) -> Result<String, String> {
        sqlx::query("DELETE FROM notes WHERE deleted_at IS NOT NULL").execute(&self.state.pool).await.map_err(|e| format!("Fehler: {e}"))?;
        sqlx::query("DELETE FROM contacts WHERE deleted_at IS NOT NULL").execute(&self.state.db).await.map_err(|e| format!("Fehler: {e}"))?;
        Ok("Papierkorb geleert".to_string())
    }

    async fn delete_department(&self, name: String) -> Result<String, String> {
        let name_lc = name.to_lowercase();
        let _ = sqlx::query("DELETE FROM departments_store WHERE LOWER(TRIM(name)) = ?").bind(&name_lc).execute(&self.state.pool).await;
        let _ = sqlx::query("UPDATE notes SET departments = (SELECT COALESCE(json_group_array(e.value), '[]') FROM json_each(COALESCE(notes.departments, '[]')) AS e WHERE LOWER(TRIM(e.value)) != ? AND TRIM(e.value) != ''), department = CASE WHEN LOWER(TRIM(department)) = ? THEN NULL ELSE department END")
            .bind(&name_lc).bind(&name_lc).execute(&self.state.pool).await;
        let _ = sqlx::query("UPDATE contacts SET departments = (SELECT COALESCE(json_group_array(e.value), '[]') FROM json_each(COALESCE(contacts.departments, '[]')) AS e WHERE LOWER(TRIM(e.value)) != ? AND TRIM(e.value) != ''), department = CASE WHEN LOWER(TRIM(department)) = ? THEN NULL ELSE department END")
            .bind(&name_lc).bind(&name_lc).execute(&self.state.db).await;
        Ok("Department gelöscht".to_string())
    }

    async fn load_departments(&mut self) -> Result<(), String> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            r#"SELECT name, cnt FROM (SELECT dep.value AS name, COUNT(*) AS cnt, LOWER(dep.value) AS lname FROM notes, json_each(COALESCE(notes.departments, '[]')) AS dep WHERE json_type(COALESCE(notes.departments, '[]')) IS NOT NULL AND notes.deleted_at IS NULL AND TRIM(dep.value) != '' GROUP BY LOWER(dep.value) UNION SELECT s.name AS name, 0 AS cnt, LOWER(s.name) AS lname FROM departments_store s WHERE TRIM(s.name) != '') GROUP BY lname ORDER BY cnt DESC, LOWER(name) ASC"#,
        )
        .fetch_all(&self.state.pool)
        .await
        .map_err(|e| format!("Fehler: {e}"))?;

        self.departments = rows.into_iter().filter(|(n, _)| !n.is_empty()).map(|(name, count)| Department { name, count }).collect();
        Ok(())
    }

    /// Index aller existierenden Link-Ziele (Notizen, Wiki, Kontakte) für die Darstellung.
    async fn load_wiki_links(&mut self) -> Result<(), String> {
        self.rebuild_links();
        Ok(())
    }

    fn rebuild_links(&mut self) {
        let mut set = HashSet::new();
        for n in &self.notes {
            set.insert(link_key(LinkKind::Note, &n.title));
        }
        for w in &self.wiki {
            set.insert(link_key(LinkKind::Wiki, &w.title));
        }
        for c in &self.contacts {
            set.insert(link_key(LinkKind::Contact, &c.name));
        }
        self.wiki_links = set;
    }

    /// Text (ohne Checkliste) des aktuell ausgewählten Eintrags.
    fn current_text(&self) -> Option<String> {
        let note_text = |n: &Note| {
            serde_json::from_str::<serde_json::Value>(&n.content)
                .ok()
                .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(str::to_string))
                .unwrap_or_else(|| n.content.clone())
        };
        match self.view {
            View::Notes => self.selected_note().map(note_text),
            View::Kanban => self.kanban_list().get(self.selected).map(|n| note_text(n)),
            View::Calendar => self.selected_calendar_note().map(note_text),
            View::Archive => self.selected_archived().map(note_text),
            View::Wiki => self.selected_wiki().and_then(|w| w.content.clone()),
            _ => None,
        }
    }

    /// `o`: Links des aktuellen Eintrags folgen.
    fn follow_links(&mut self) {
        let links = self.current_text().map(|t| links::extract(&t)).unwrap_or_default();
        match links.len() {
            0 => self.status = "Keine Links im Eintrag".to_string(),
            1 => self.jump_to(&links[0]),
            _ => {
                let list: Vec<String> = links.iter().take(9).enumerate().map(|(i, l)| format!("{}:{}", i + 1, l.display)).collect();
                self.status = format!("Link wählen: {}", list.join("  "));
                self.pending_links = Some(links);
            }
        }
    }

    fn jump_to(&mut self, link: &Link) {
        let found = match link.kind {
            LinkKind::Note => {
                self.set_view(View::Notes);
                self.filter = NoteFilter::All;
                self.search.clear();
                self.notes.iter().position(|n| n.title.eq_ignore_ascii_case(&link.target) || n.title.to_lowercase() == link.target.to_lowercase())
            }
            LinkKind::Wiki => {
                self.set_view(View::Wiki);
                self.search.clear();
                self.wiki.iter().position(|w| w.title.to_lowercase() == link.target.to_lowercase())
            }
            LinkKind::Contact => {
                self.set_view(View::Data);
                self.search.clear();
                let offset = self.visible_depts().len();
                self.contacts.iter().position(|c| c.name.to_lowercase() == link.target.to_lowercase()).map(|i| i + offset)
            }
        };
        match found {
            Some(i) => {
                self.selected = i;
                self.clamp_selection();
                self.status = format!("→ {}", link.display);
            }
            None => self.status = format!("Nicht gefunden: {}", link.raw()),
        }
    }

    async fn reload(&mut self) -> Result<(), String> {
        self.notes = sqlx::query_as::<_, Note>(
            "SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned FROM notes WHERE deleted_at IS NULL AND status != 'archived' ORDER BY sort_order ASC, id ASC",
        )
        .fetch_all(&self.state.pool)
        .await
        .map_err(|e| format!("Fehler: {e}"))?;

        self.archive = sqlx::query_as::<_, Note>(
            "SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned FROM notes ORDER BY completed_at DESC, id DESC",
        )
        .fetch_all(&self.state.archive_pool)
        .await
        .map_err(|e| format!("Fehler: {e}"))?;

        let trash_notes = sqlx::query_as::<_, Note>(
            "SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned FROM notes WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, id DESC",
        )
        .fetch_all(&self.state.pool)
        .await
        .map_err(|e| format!("Fehler: {e}"))?;

        let trash_contacts = sqlx::query_as::<_, Contact>(
            "SELECT id, name, title, department, departments, phone, mobile, fax, email, description FROM contacts WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, id DESC",
        )
        .fetch_all(&self.state.db)
        .await
        .map_err(|e| format!("Fehler: {e}"))?;

        self.trash.clear();
        for n in trash_notes {
            self.trash.push(TrashItem { note: Some(n), contact: None });
        }
        for c in trash_contacts {
            self.trash.push(TrashItem { note: None, contact: Some(c) });
        }

        self.contacts = sqlx::query_as::<_, Contact>(
            "SELECT id, name, title, department, departments, phone, mobile, fax, email, description FROM contacts WHERE deleted_at IS NULL ORDER BY LOWER(name) ASC, id ASC",
        )
        .fetch_all(&self.state.db)
        .await
        .map_err(|e| format!("Fehler: {e}"))?;

        self.wiki = sqlx::query_as::<_, WikiPage>(
            "SELECT id, title, content, created_at, updated_at FROM wiki ORDER BY LOWER(title) ASC, id ASC",
        )
        .fetch_all(&self.state.pool)
        .await
        .map_err(|e| format!("Fehler: {e}"))?;

        self.rebuild_links();
        self.clamp_selection();
        Ok(())
    }

    fn accept_autocomplete(&mut self) {
        if let Some(items) = self.autocomplete.take() {
            if let Some(item) = items.get(self.autocomplete_index) {
                self.status = format!("Verlinkung zu {} ({})", item.name, match item.kind { AutocompleteKind::Note => "Notiz", AutocompleteKind::Wiki => "Wiki", AutocompleteKind::Contact => "Kontakt" });
                self.link_mode = None;
            }
        }
        self.autocomplete_index = 0;
    }
}

async fn next_sort_order(pool: &SqlitePool, status: &str) -> Result<i64, String> {
    let max: Option<i64> =
        sqlx::query_scalar("SELECT MAX(sort_order) FROM notes WHERE status = ? AND deleted_at IS NULL")
            .bind(status)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("Fehler: {e}"))?;
    Ok(max.unwrap_or(0) + 1)
}

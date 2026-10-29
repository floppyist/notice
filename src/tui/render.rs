use std::collections::HashMap;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
    Frame,
};

use crate::models::{first_department, Note};
use crate::tui::app::{App, AutocompleteKind, Confirm, Focus, View};
use crate::tui::theme as th;

const VIEWS: [View; 8] = [View::Dashboard, View::Kanban, View::Notes, View::Wiki, View::Data, View::Calendar, View::Archive, View::Trash];

pub fn render(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    frame.render_widget(Block::default().style(th::base()), area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    render_tabs(app, frame, chunks[0]);
    render_body(app, frame, chunks[1]);
    render_status(app, frame, chunks[2]);

    if app.help {
        render_help(frame, area);
        return;
    }
    if let Some(confirm) = &app.confirm {
        render_confirm(frame, area, confirm);
        return;
    }
    if let Some(editor) = app.editor.as_mut() {
        render_editor(frame, area, editor);
    }
    if app.autocomplete.is_some() {
        render_autocomplete(app, frame, area);
    }
    if app.command_mode {
        render_command(app, frame, chunks[2]);
    }
}

// ───────────────────────── Kopfzeile ─────────────────────────

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn is_overdue(note: &Note, today: &str) -> bool {
    note.status != "done" && note.due_date.as_deref().map(|d| d < today).unwrap_or(false)
}

fn render_tabs(app: &App, frame: &mut Frame, area: Rect) {
    let mut spans = vec![
        Span::styled(th::bar_glyph(), Style::default().fg(th::accent()).bg(th::surface())),
        Span::styled(" NOTICE ", Style::default().fg(th::accent_text()).bg(th::surface()).add_modifier(Modifier::BOLD)),
    ];
    for (i, v) in VIEWS.iter().enumerate() {
        let label = format!(" {} {} ", i + 1, v.title());
        if *v == app.view {
            spans.push(Span::styled(label, th::block_accent()));
        } else {
            spans.push(Span::styled(label, th::bar()));
        }
    }
    let t = today();
    let overdue = app.notes.iter().filter(|n| is_overdue(n, &t)).count();
    let right = if overdue > 0 { format!(" ! {} überfällig ", overdue) } else { String::new() };
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let pad = (area.width as usize).saturating_sub(used + right.chars().count());
    spans.push(Span::styled(" ".repeat(pad), th::bar()));
    if !right.is_empty() {
        spans.push(Span::styled(right, Style::default().fg(th::on_accent()).bg(th::red()).add_modifier(Modifier::BOLD)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)).style(th::bar()), area);
}

fn render_body(app: &mut App, frame: &mut Frame, area: Rect) {
    let split = |pct: u16| {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(pct), Constraint::Percentage(100 - pct)])
            .split(area)
    };
    match app.view {
        View::Dashboard => render_dashboard(app, frame, area),
        View::Kanban => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(9)])
                .split(area);
            render_kanban(app, frame, rows[0]);
            render_kanban_detail(app, frame, rows[1]);
        }
        View::Notes | View::Archive | View::Wiki => {
            let c = split(30);
            render_list(app, frame, c[0]);
            render_detail(app, frame, c[1]);
        }
        View::Data => {
            let c = split(35);
            render_data_list(app, frame, c[0]);
            render_data_detail(app, frame, c[1]);
        }
        View::Trash => {
            let c = split(30);
            render_trash_list(app, frame, c[0]);
            render_trash_detail(app, frame, c[1]);
        }
        View::Calendar => {
            let c = split(38);
            render_calendar_list(app, frame, c[0]);
            render_calendar_detail(app, frame, c[1]);
        }
    }
}

// ───────────────────────── Dashboard ─────────────────────────

fn capsule(label: &str, value: usize, color: ratatui::style::Color) -> Vec<Span<'static>> {
    let active = value > 0;
    vec![
        Span::styled(th::bar_glyph(), Style::default().fg(if active { color } else { th::overlay() }).bg(th::surface())),
        Span::styled(format!(" {} ", label), th::capsule().fg(th::subtle())),
        Span::styled(format!("{} ", value), th::capsule().fg(if active { color } else { th::muted() }).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
    ]
}

fn render_dashboard(app: &App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(6), Constraint::Min(0)])
        .split(area);

    let t = today();
    let overdue = app.notes.iter().filter(|n| is_overdue(n, &t)).count();
    let due_today = app.notes.iter().filter(|n| n.status != "done" && n.due_date.as_deref() == Some(t.as_str())).count();
    let count = |s: &str| app.notes.iter().filter(|n| n.status == s).count();

    let mut spans = vec![Span::raw(" ")];
    spans.extend(capsule("Überfällig", overdue, th::red()));
    spans.extend(capsule("Heute", due_today, th::orange()));
    spans.extend(capsule("Backlog", count("backlog"), th::status_color("backlog")));
    spans.extend(capsule("In Arbeit", count("in_progress"), th::status_color("in_progress")));
    spans.extend(capsule("Review", count("review"), th::status_color("review")));
    spans.extend(capsule("Erledigt", count("done"), th::status_color("done")));
    frame.render_widget(Paragraph::new(Line::from(spans)).block(th::panel("Übersicht", false)), chunks[0]);

    // Verteilung nach Status als Balken
    let total = app.notes.len().max(1);
    let bar_w = (chunks[1].width as usize).saturating_sub(30).clamp(10, 60);
    let mut lines = Vec::new();
    for (s, label) in [("backlog", "Backlog"), ("in_progress", "In Arbeit"), ("review", "Review"), ("done", "Erledigt")] {
        let n = count(s);
        let filled = n * bar_w / total;
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<10}", label), th::dim()),
            Span::styled("█".repeat(filled), Style::default().fg(th::status_color(s))),
            Span::styled("░".repeat(bar_w - filled), Style::default().fg(th::overlay())),
            Span::styled(format!(" {}", n), th::text()),
        ]));
    }
    frame.render_widget(Paragraph::new(lines).block(th::panel("Status", false)), chunks[1]);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);

    let list = |items: Vec<&Note>, color, empty: &str| -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        for note in items.into_iter().take(12) {
            let mut spans = vec![Span::styled(format!("{} ", th::bar_glyph()), Style::default().fg(color)), Span::styled(note.title.clone(), th::text())];
            if let Some(d) = &note.due_date {
                spans.push(Span::styled(format!("  {}", d), th::faint()));
            }
            lines.push(Line::from(spans));
        }
        if lines.is_empty() {
            lines.push(Line::from(Span::styled(empty.to_string(), th::faint())));
        }
        lines
    };
    let over: Vec<&Note> = app.notes.iter().filter(|n| is_overdue(n, &t)).collect();
    let tod: Vec<&Note> = app.notes.iter().filter(|n| n.status != "done" && n.due_date.as_deref() == Some(t.as_str())).collect();
    frame.render_widget(
        Paragraph::new(list(over, th::red(), "Nichts überfällig")).block(th::panel("Überfällig", false).padding(ratatui::widgets::Padding::horizontal(1))),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(list(tod, th::orange(), "Heute nichts fällig")).block(th::panel("Heute", false).padding(ratatui::widgets::Padding::horizontal(1))),
        columns[1],
    );
}

// ───────────────────────── Kanban ─────────────────────────

fn render_kanban(app: &mut App, frame: &mut Frame, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25); 4])
        .split(area);
    let statuses = [("backlog", "BACKLOG"), ("in_progress", "IN ARBEIT"), ("review", "REVIEW"), ("done", "ERLEDIGT")];
    let list = app.kanban_list();
    let t = today();
    let list_focus = app.focus == Focus::List;

    for (col_idx, (status, title)) in statuses.iter().enumerate() {
        let color = th::status_color(status);
        let mut lines: Vec<Line> = Vec::new();
        let mut count = 0;
        let mut sel_line: Option<usize> = None;
        for (idx, note) in list.iter().enumerate() {
            if note.status != *status {
                continue;
            }
            count += 1;
            let sel = app.selected == idx;
            if sel {
                sel_line = Some(lines.len());
            }
            let mut head = vec![];
            if note.pinned != 0 {
                head.push(Span::styled("★ ", Style::default().fg(th::yellow())));
            }
            head.push(Span::styled(truncate(&note.title, columns[col_idx].width.saturating_sub(8) as usize), th::text()));
            if note.priority == "high" {
                head.push(Span::styled(" !", Style::default().fg(th::red()).add_modifier(Modifier::BOLD)));
            }
            let bar_style = Style::default().fg(if sel && list_focus { th::accent() } else { color });
            let mut l1 = vec![Span::styled(th::bar_glyph(), bar_style), Span::raw(" ")];
            l1.extend(head);
            let mut l2 = vec![Span::styled(th::bar_glyph(), bar_style), Span::raw(" ")];
            match &note.due_date {
                Some(d) if is_overdue(note, &t) => l2.push(Span::styled(format!("! {}", d), Style::default().fg(th::red()))),
                Some(d) => l2.push(Span::styled(d.clone(), th::faint())),
                None => l2.push(Span::styled("–", th::faint())),
            }
            let style = if sel && list_focus { Style::default().bg(th::surface()) } else { Style::default() };
            lines.push(Line::from(l1).style(style));
            lines.push(Line::from(l2).style(style));
            lines.push(Line::from(""));
        }
        if count == 0 {
            lines.push(Line::from(Span::styled(" leer", th::faint())));
        }
        let inner_h = columns[col_idx].height.saturating_sub(2) as usize;
        let scroll = sel_line.map(|l| (l + 2).saturating_sub(inner_h)).unwrap_or(0) as u16;
        let block = th::panel_colored(format!("{} {}", title, count), color, sel_line.is_some() && list_focus);
        frame.render_widget(Paragraph::new(lines).block(block).scroll((scroll, 0)), columns[col_idx]);
    }
}

fn render_kanban_detail(app: &App, frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    if let Some(note) = app.kanban_list().get(app.selected).copied() {
        lines.extend(note_header(note));
        lines.extend(detail_content(&note.content, &app.wiki_links).into_iter().take(4));
        lines.extend(appointment_lines(note).into_iter().take(3));
    } else {
        lines.push(Line::from(Span::styled("Keine Auswahl", th::faint())));
    }
    render_detail_panel(frame, area, lines, app.detail_scroll, app.focus == Focus::Detail, "Detail");
}

// ───────────────────────── Listen ─────────────────────────

fn title_spans(title: &str, needle: &str) -> Vec<Span<'static>> {
    if needle.is_empty() || !title.to_lowercase().contains(needle) {
        vec![Span::styled(title.to_string(), th::text())]
    } else {
        highlight_text(title, needle)
    }
}

fn list_title(app: &App, base: &str) -> String {
    if app.search_edit || !app.search.is_empty() {
        format!("{} /{}", base, app.search)
    } else if app.view == View::Notes {
        format!("{} [{}]", base, app.filter.label())
    } else {
        base.to_string()
    }
}

fn render_list(app: &App, frame: &mut Frame, area: Rect) {
    let needle = app.search.trim().to_lowercase();
    let focused = app.focus == Focus::List;
    let t = today();
    let mut lines: Vec<Line> = Vec::new();
    match app.view {
        View::Notes => {
            for (pos, note) in app.notes.iter().filter(|n| app.note_matches(n)).enumerate() {
                let mut spans = vec![Span::styled("● ", Style::default().fg(th::status_color(&note.status)))];
                if note.pinned != 0 {
                    spans.push(Span::styled("★ ", Style::default().fg(th::yellow())));
                }
                spans.extend(title_spans(&note.title, &needle));
                if is_overdue(note, &t) {
                    spans.push(Span::styled(" !", Style::default().fg(th::red()).add_modifier(Modifier::BOLD)));
                }
                lines.push(th::row(app.selected == pos, focused, spans));
            }
        }
        View::Archive => {
            for (pos, note) in app.visible_archive().into_iter().enumerate() {
                lines.push(th::row(app.selected == pos, focused, title_spans(&note.title, &needle)));
            }
        }
        View::Wiki => {
            for (pos, page) in app.visible_wiki().into_iter().enumerate() {
                let mut spans = vec![Span::styled("◆ ", Style::default().fg(th::purple()))];
                spans.extend(title_spans(&page.title, &needle));
                lines.push(th::row(app.selected == pos, focused, spans));
            }
        }
        _ => {}
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(" leer", th::faint())));
    }
    let title = list_title(app, app.view.title());
    let scroll = scroll_for(app.selected, area.height);
    frame.render_widget(Paragraph::new(lines).block(th::panel(&title, focused)).scroll((scroll, 0)), area);
}

/// Scroll-Offset, damit die ausgewählte Zeile sichtbar bleibt.
fn scroll_for(selected: usize, height: u16) -> u16 {
    let inner = height.saturating_sub(2) as usize;
    if inner == 0 { 0 } else { (selected + 1).saturating_sub(inner) as u16 }
}

fn render_trash_list(app: &App, frame: &mut Frame, area: Rect) {
    let needle = app.search.trim().to_lowercase();
    let focused = app.focus == Focus::List;
    let mut lines: Vec<Line> = Vec::new();
    for (pos, item) in app.visible_trash().into_iter().enumerate() {
        lines.push(th::row(app.selected == pos, focused, title_spans(item.title(), &needle)));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(" leer", th::faint())));
    }
    let scroll = scroll_for(app.selected, area.height);
    frame.render_widget(Paragraph::new(lines).block(th::panel("Papierkorb", focused)).scroll((scroll, 0)), area);
}

fn render_data_list(app: &App, frame: &mut Frame, area: Rect) {
    let needle = app.search.trim().to_lowercase();
    let focused = app.focus == Focus::List;
    let depts = app.visible_depts();
    let mut lines: Vec<Line> = Vec::new();
    for (pos, dept) in depts.iter().enumerate() {
        let mut spans = vec![Span::styled("▸ ", Style::default().fg(th::orange()))];
        spans.extend(title_spans(&dept.name, &needle));
        spans.push(Span::styled(format!(" ({})", dept.count), th::faint()));
        lines.push(th::row(app.selected == pos, focused, spans));
    }
    for (cpos, contact) in app.visible_contacts().into_iter().enumerate() {
        let mut spans = vec![Span::styled("• ", Style::default().fg(th::purple()))];
        spans.extend(title_spans(&contact.name, &needle));
        lines.push(th::row(app.selected == depts.len() + cpos, focused, spans));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(" leer", th::faint())));
    }
    let scroll = scroll_for(app.selected, area.height);
    frame.render_widget(Paragraph::new(lines).block(th::panel(&list_title(app, "Daten"), focused)).scroll((scroll, 0)), area);
}

// ───────────────────────── Details ─────────────────────────

fn render_detail_panel(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>, scroll: u16, focused: bool, title: &str) {
    let lines: Vec<Line<'static>> = lines.into_iter().skip(scroll as usize).collect();
    let block = th::panel(title, focused).padding(ratatui::widgets::Padding::horizontal(1));
    frame.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

fn badge(text: String, color: ratatui::style::Color) -> Span<'static> {
    Span::styled(format!(" {} ", text), th::block_color(color))
}

fn status_label(status: &str) -> &'static str {
    match status {
        "backlog" => "BACKLOG",
        "in_progress" => "IN ARBEIT",
        "review" => "REVIEW",
        "done" => "ERLEDIGT",
        "archived" => "ARCHIV",
        _ => "?",
    }
}

fn note_header(note: &Note) -> Vec<Line<'static>> {
    let t = today();
    let mut lines = vec![Line::from(Span::styled(note.title.clone(), th::bold()))];
    let mut meta = vec![badge(status_label(&note.status).to_string(), th::status_color(&note.status)), Span::raw(" ")];
    meta.push(Span::styled(format!("Prio {}", note.priority), Style::default().fg(th::priority_color(&note.priority))));
    if let Some(due) = &note.due_date {
        if is_overdue(note, &t) {
            meta.push(Span::styled(format!("  ! fällig {} (überfällig)", due), Style::default().fg(th::red()).add_modifier(Modifier::BOLD)));
        } else {
            meta.push(Span::styled(format!("  fällig {}", due), th::dim()));
        }
    }
    lines.push(Line::from(meta));
    let mut extra = Vec::new();
    if let Some(dept) = first_department(&note.departments) {
        extra.push(Span::styled(format!("▸ {}", dept), Style::default().fg(th::orange())));
    }
    if let Some(rule) = &note.repeat_rule {
        extra.push(Span::styled(format!("  ↻ {}", rule), th::dim()));
    }
    if !extra.is_empty() {
        lines.push(Line::from(extra));
    }
    lines.push(Line::from(""));
    lines
}

fn render_detail(app: &App, frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    match app.view {
        View::Notes => {
            if let Some(note) = app.selected_note() {
                lines.extend(note_header(note));
                lines.extend(detail_content(&note.content, &app.wiki_links));
                lines.extend(appointment_lines(note));
            } else {
                lines.push(Line::from(Span::styled("Keine Notiz", th::faint())));
            }
        }
        View::Archive => {
            if let Some(note) = app.selected_archived() {
                lines.push(Line::from(Span::styled(note.title.clone(), th::bold())));
                lines.push(Line::from(vec![badge("ARCHIV".into(), th::muted())]));
                lines.push(Line::from(""));
                lines.extend(detail_content(&note.content, &app.wiki_links));
            } else {
                lines.push(Line::from(Span::styled("Kein Eintrag", th::faint())));
            }
        }
        View::Wiki => {
            if let Some(page) = app.selected_wiki() {
                lines.push(Line::from(Span::styled(page.title.clone(), th::bold())));
                if let Some(updated) = &page.updated_at {
                    lines.push(Line::from(Span::styled(format!("aktualisiert {}", updated), th::faint())));
                }
                lines.push(Line::from(""));
                if let Some(content) = &page.content {
                    for l in content.split('\n') {
                        lines.push(render_line_with_links(l, &app.wiki_links));
                    }
                }
            } else {
                lines.push(Line::from(Span::styled("Keine Seite", th::faint())));
            }
        }
        _ => {}
    }
    render_detail_panel(frame, area, lines, app.detail_scroll, app.focus == Focus::Detail, "Detail");
}

fn render_trash_detail(app: &App, frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    if let Some(item) = app.selected_trash() {
        if let Some(note) = &item.note {
            lines.push(Line::from(Span::styled(note.title.clone(), th::bold())));
            lines.push(Line::from(vec![badge("NOTIZ".into(), th::muted()), Span::styled(format!(" {} · Prio {}", note.status, note.priority), th::dim())]));
            lines.push(Line::from(""));
            lines.extend(detail_content(&note.content, &app.wiki_links));
        } else if let Some(contact) = &item.contact {
            lines.push(Line::from(Span::styled(contact.name.clone(), th::bold())));
            lines.push(Line::from(vec![badge("KONTAKT".into(), th::purple())]));
            lines.push(Line::from(""));
            if let Some(email) = &contact.email { lines.push(kv("Mail", email, th::blue())); }
            if let Some(phone) = &contact.phone { lines.push(kv("Tel", phone, th::fg())); }
            if let Some(desc) = &contact.description {
                lines.push(Line::from(""));
                for l in desc.split('\n') { lines.push(Line::from(Span::styled(l.to_string(), th::text()))); }
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("u wiederherstellen · x endgültig löschen · c leeren", th::faint())));
    } else {
        lines.push(Line::from(Span::styled("leer", th::faint())));
    }
    render_detail_panel(frame, area, lines, app.detail_scroll, app.focus == Focus::Detail, "Detail");
}

fn kv(key: &str, value: &str, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![Span::styled(format!("{:<7}", key), th::faint()), Span::styled(value.to_string(), Style::default().fg(color))])
}

fn render_data_detail(app: &App, frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    if let Some(dept) = app.selected_dept() {
        lines.push(Line::from(Span::styled(dept.name.clone(), th::bold())));
        lines.push(Line::from(vec![badge("EINRICHTUNG".into(), th::orange()), Span::styled(format!(" {}× verwendet", dept.count), th::dim())]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("dd löschen · n neue Einrichtung", th::faint())));
    } else if let Some(contact) = app.selected_contact_data() {
        lines.push(Line::from(Span::styled(contact.name.clone(), th::bold())));
        if let Some(title) = &contact.title { lines.push(Line::from(Span::styled(title.clone(), th::dim()))); }
        lines.push(Line::from(""));
        if let Some(dept) = first_department(&contact.departments) { lines.push(kv("Einr.", &dept, th::orange())); }
        if let Some(phone) = &contact.phone { lines.push(kv("Tel", phone, th::fg())); }
        if let Some(mobile) = &contact.mobile { lines.push(kv("Mobil", mobile, th::fg())); }
        if let Some(email) = &contact.email { lines.push(kv("Mail", email, th::blue())); }
        if let Some(desc) = &contact.description {
            lines.push(Line::from(""));
            for l in desc.split('\n') { lines.push(Line::from(Span::styled(l.to_string(), th::text()))); }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("e bearbeiten · dd löschen · n neuer Kontakt", th::faint())));
    } else {
        lines.push(Line::from(Span::styled("Keine Auswahl", th::faint())));
    }
    render_detail_panel(frame, area, lines, app.detail_scroll, app.focus == Focus::Detail, "Detail");
}

fn detail_content(raw: &str, wiki_links: &HashMap<String, i64>) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
            for line in text.split('\n') {
                lines.push(render_line_with_links(line, wiki_links));
            }
        }
        if let Some(checklist) = value.get("checklist").and_then(|v| v.as_array()) {
            if !checklist.is_empty() {
                let done_count = checklist
                    .iter()
                    .filter(|i| i.get("done").or_else(|| i.get("checked")).and_then(|v| v.as_bool()).unwrap_or(false))
                    .count();
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Checkliste", th::heading()),
                    Span::styled(format!("  {}/{}", done_count, checklist.len()), th::faint()),
                ]));
                for item in checklist {
                    if let Some(obj) = item.as_object() {
                        let done = obj.get("done").and_then(|v| v.as_bool()).or_else(|| obj.get("checked").and_then(|v| v.as_bool())).unwrap_or(false);
                        let text = obj.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        if done {
                            lines.push(Line::from(vec![Span::styled("✓ ", Style::default().fg(th::green())), Span::styled(text.to_string(), th::faint())]));
                        } else {
                            lines.push(Line::from(vec![Span::styled("○ ", th::dim()), Span::styled(text.to_string(), th::text())]));
                        }
                    }
                }
            }
        }
    } else {
        for line in raw.split('\n') {
            lines.push(render_line_with_links(line, wiki_links));
        }
    }
    lines
}

fn appointment_lines(note: &Note) -> Vec<Line<'static>> {
    let Some(raw) = &note.appointments else { return Vec::new() };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else { return Vec::new() };
    let Some(items) = value.as_array() else { return Vec::new() };
    if items.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from(""), Line::from(Span::styled("Termine", th::heading()))];
    for item in items {
        let date = item.get("start").or_else(|| item.get("date")).and_then(|v| v.as_str()).unwrap_or("");
        let time = item.get("time").and_then(|v| v.as_str()).unwrap_or("");
        let text = item.get("title").or_else(|| item.get("text")).and_then(|v| v.as_str()).unwrap_or("");
        let stamp = [date, time].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ");
        lines.push(Line::from(vec![
            Span::styled("◆ ", Style::default().fg(th::aqua())),
            Span::styled(format!("{} ", stamp), th::dim()),
            Span::styled(text.to_string(), th::text()),
        ]));
    }
    lines
}

fn render_line_with_links(line: &str, wiki_links: &HashMap<String, i64>) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start + 2..].find("]]") else { break };
        if len == 0 {
            spans.push(Span::styled(rest[..start + 4].to_string(), th::text()));
            rest = &rest[start + 4..];
            continue;
        }
        if start > 0 {
            spans.push(Span::styled(rest[..start].to_string(), th::text()));
        }
        let inner = &rest[start + 2..start + 2 + len];
        let mut style = Style::default().fg(th::purple());
        if wiki_links.contains_key(&inner.to_lowercase()) {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        spans.push(Span::styled(format!("[[{}]]", inner), style));
        rest = &rest[start + 2 + len + 2..];
    }
    if !rest.is_empty() || spans.is_empty() {
        spans.push(Span::styled(rest.to_string(), th::text()));
    }
    Line::from(spans)
}

// ───────────────────────── Statuszeile ─────────────────────────

fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    let (mode_label, mode_style) = if app.command_mode {
        (" CMD ", th::block_color(th::blue()))
    } else if app.search_edit {
        (" SUCHE ", th::block_color(th::aqua()))
    } else if app.editor.is_some() {
        (" EDITOR ", th::block_accent())
    } else if app.normal_mode {
        (" NORMAL ", th::block_color(th::yellow()))
    } else {
        (" INSERT ", th::block_accent())
    };

    let mut spans = vec![Span::styled(mode_label, mode_style)];
    if app.g_pressed {
        spans.push(Span::styled(" g…", Style::default().fg(th::yellow()).bg(th::surface())));
    } else if app.pending_d {
        spans.push(Span::styled(" d…", Style::default().fg(th::yellow()).bg(th::surface())));
    }

    let hints: &[(&str, &str)] = if app.editor.is_some() {
        &[("Tab", "Feld"), ("h/l", "Wahl"), ("^S", "speichern"), ("Esc", "abbrechen")]
    } else if app.search_edit {
        &[("Enter", "übernehmen"), ("Esc", "abbrechen")]
    } else if app.normal_mode {
        &[("1-8", "Ansicht"), ("j/k", "Auswahl"), ("h/l", "Spalte"), ("n", "neu"), ("e", "bearbeiten"), ("dd", "löschen"), ("/", "suchen"), ("?", "Hilfe"), ("q", "beenden")]
    } else {
        &[("Esc", "normal")]
    };
    if !app.command_mode {
        for (k, l) in hints {
            spans.extend(th::hint(k, l));
        }
    }

    let status = if app.status.is_empty() { String::new() } else { format!("{} ", app.status) };
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let pad = (area.width as usize).saturating_sub(used + status.chars().count());
    spans.push(Span::styled(" ".repeat(pad), th::bar()));
    spans.push(Span::styled(status, Style::default().fg(th::yellow()).bg(th::surface())));
    frame.render_widget(Paragraph::new(Line::from(spans)).style(th::bar()), area);
}

// ───────────────────────── Overlays ─────────────────────────

fn overlay_block(title: &str) -> Block<'static> {
    th::panel(title, true).border_style(Style::default().fg(th::accent_text())).style(th::base())
}

fn render_help(frame: &mut Frame, area: Rect) {
    let sec = |s: &str| Line::from(Span::styled(s.to_string(), th::heading()));
    let key = |k: &str, d: &str| Line::from(vec![Span::styled(format!("  {:<14}", k), Style::default().fg(th::accent_text())), Span::styled(d.to_string(), th::dim())]);
    let text = vec![
        sec("Navigation"),
        key("h j k l / Pfeile", "bewegen"),
        key("gg  G", "Anfang / Ende"),
        key("Tab", "Fokus Liste ↔ Detail"),
        key("1-8", "Ansicht wechseln"),
        Line::from(""),
        sec("Aktionen"),
        key("n", "neu"),
        key("e / Enter", "bearbeiten"),
        key("dd", "löschen / Papierkorb"),
        key("u", "wiederherstellen"),
        key("x / D", "endgültig löschen"),
        key("a", "archivieren"),
        key("c", "Papierkorb leeren"),
        key("f", "Filter umschalten"),
        key("r", "aktualisieren"),
        Line::from(""),
        sec("Suchen & Befehle"),
        key("/", "suchen"),
        key(":q :w :wq", "Befehle"),
        key(":dashboard :kanban …", "Ansicht wechseln"),
        Line::from(""),
        sec("Links"),
        key("[[Seite]]", "Wiki-Verlinkung"),
        Line::from(""),
        Line::from(Span::styled("  Esc oder ? schließt die Hilfe", th::faint())),
    ];
    let popup = centered_rect(60, 85, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(text).block(overlay_block("Hilfe")), popup);
}

fn render_confirm(frame: &mut Frame, area: Rect, confirm: &Confirm) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(format!(" {}", confirm.message), th::text())),
        Line::from(""),
        Line::from(vec![
            Span::styled(" y ", th::block_accent()),
            Span::styled(" Ja   ", th::dim()),
            Span::styled(" n ", th::capsule()),
            Span::styled(" Nein", th::dim()),
        ]),
    ];
    let popup = centered_rect(50, 22, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(text).block(overlay_block("Bestätigen")), popup);
}

fn render_editor(frame: &mut Frame, area: Rect, editor: &mut crate::tui::editor::Editor) {
    let popup = centered_rect(90, 85, area);
    editor.ensure_cursor_visible(popup.height.saturating_sub(2));
    let (lines, cursor_pos) = editor.lines();
    let title = editor.title();
    let paragraph = Paragraph::new(lines).block(overlay_block(&title)).scroll((editor.scroll, 0));
    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
    if let Some((row, col)) = cursor_pos {
        if row >= editor.scroll as usize {
            let screen_row = popup.y + (row as u16 - editor.scroll) + 1;
            let screen_col = popup.x + col as u16 + 1;
            if screen_row < popup.y + popup.height - 1 && screen_col < popup.x + popup.width - 1 {
                frame.set_cursor_position((screen_col, screen_row));
            }
        }
    }
}

fn render_autocomplete(app: &App, frame: &mut Frame, area: Rect) {
    if let Some(items) = &app.autocomplete {
        let mut lines: Vec<Line> = Vec::new();
        for (i, item) in items.iter().take(12).enumerate() {
            let text = match item.kind {
                AutocompleteKind::Wiki | AutocompleteKind::Note => format!("[[{}]]", item.name),
                AutocompleteKind::Contact => format!("{{{{{}}}}}", item.name),
            };
            lines.push(th::row(i == app.autocomplete_index, true, vec![Span::styled(text, th::text())]));
        }
        let popup = Rect { x: area.x + 3, y: area.y + area.height / 2, width: 45.min(area.width), height: items.len().min(12) as u16 + 2 };
        frame.render_widget(Clear, popup);
        frame.render_widget(Paragraph::new(lines).block(overlay_block("Vorschläge")), popup);
    }
}

fn render_command(app: &App, frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(" CMD ", th::block_color(th::blue())),
        Span::styled(" ❯ ", Style::default().fg(th::accent_text()).bg(th::surface())),
        Span::styled(format!(":{}", app.command), Style::default().fg(th::fg()).bg(th::surface())),
    ]);
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(line).style(th::bar()), area);
    let col = area.x + 5 + 3 + 1 + app.command.chars().count() as u16;
    frame.set_cursor_position((col.min(area.x + area.width.saturating_sub(1)), area.y));
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage((100 - percent_y) / 2), Constraint::Percentage(percent_y), Constraint::Percentage((100 - percent_y) / 2)])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage((100 - percent_x) / 2), Constraint::Percentage(percent_x), Constraint::Percentage((100 - percent_x) / 2)])
        .split(v[1])[1]
}

fn highlight_text(text: &str, needle: &str) -> Vec<Span<'static>> {
    let lower = text.to_lowercase();
    let needle_l = needle.to_lowercase();
    // Byte-Offsets von `lower` passen nur bei gleicher Länge; sonst ohne Hervorhebung
    if needle_l.is_empty() || lower.len() != text.len() {
        return vec![Span::styled(text.to_string(), th::text())];
    }
    let mut spans = Vec::new();
    let mut start = 0;
    while let Some(pos) = lower[start..].find(&needle_l) {
        let byte_pos = start + pos;
        if byte_pos > start {
            spans.push(Span::styled(text[start..byte_pos].to_string(), th::text()));
        }
        let end = byte_pos + needle_l.len();
        spans.push(Span::styled(text[byte_pos..end].to_string(), Style::default().fg(th::on_accent()).bg(th::yellow())));
        start = end;
        if start >= text.len() {
            break;
        }
    }
    if start < text.len() {
        spans.push(Span::styled(text[start..].to_string(), th::text()));
    }
    spans
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if max == 0 || chars.len() <= max { s.to_string() } else { format!("{}…", chars[..max - 1].iter().collect::<String>()) }
}

// ───────────────────────── Kalender ─────────────────────────

fn render_calendar_list(app: &App, frame: &mut Frame, area: Rect) {
    let now = chrono::Local::now();
    let t = today();
    let needle = app.search.trim().to_lowercase();
    let focused = app.focus == Focus::List;
    let mut lines: Vec<Line> = Vec::new();
    for (idx, note) in app.calendar_list().iter().enumerate() {
        let due = note.due_date.as_deref().unwrap_or("");
        let (marker, color) = if is_overdue(note, &t) {
            ("! ", th::red())
        } else if due == t.as_str() {
            ("● ", th::orange())
        } else if note.status == "done" {
            ("✓ ", th::green())
        } else {
            ("○ ", th::aqua())
        };
        let mut spans = vec![Span::styled(marker, Style::default().fg(color)), Span::styled(format!("{} ", due), Style::default().fg(color))];
        spans.extend(title_spans(&note.title, &needle));
        lines.push(th::row(app.selected == idx, focused, spans));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(" Keine Einträge mit Fälligkeitsdatum", th::faint())));
    }
    let title = if !app.search.is_empty() { format!("Termine /{}", app.search) } else { format!("Termine · {}", now.format("%m/%Y")) };
    let scroll = scroll_for(app.selected, area.height);
    frame.render_widget(Paragraph::new(lines).block(th::panel(&title, focused)).scroll((scroll, 0)), area);
}

fn render_calendar_detail(app: &App, frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    if let Some(note) = app.selected_calendar_note() {
        lines.extend(note_header(note));
        lines.extend(detail_content(&note.content, &app.wiki_links));
        lines.extend(appointment_lines(note));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("e bearbeiten · dd löschen · n neue Notiz", th::faint())));
    } else {
        lines.push(Line::from(Span::styled("Keine Auswahl", th::faint())));
    }
    render_detail_panel(frame, area, lines, app.detail_scroll, app.focus == Focus::Detail, "Detail");
}

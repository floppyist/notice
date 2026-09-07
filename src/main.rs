use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::net::SocketAddr;
use chrono::Datelike;

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    db: SqlitePool,
}

#[derive(Serialize, Deserialize, FromRow, Clone)]
struct Note {
    id: i64,
    title: String,
    content: String,
    status: String,
    priority: String,
    date: String,
    due_date: Option<String>,
    department: Option<String>,
    departments: Option<String>,
    appointments: Option<String>,
    sort_order: i64,
    completed_at: Option<String>,
    repeat_rule: Option<String>,
    pinned: i64,
}

#[derive(Serialize, Deserialize, FromRow, Clone)]
struct Contact {
    id: i64,
    name: String,
    department: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct CreateContact {
    name: String,
    department: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct UpdateContact {
    name: Option<String>,
    department: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct DeleteDepartment {
    name: String,
}

#[derive(Deserialize)]
struct CreateNote {
    title: String,
    content: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    due_date: Option<String>,
    department: Option<String>,
    departments: Option<String>,
    appointments: Option<String>,
    repeat_rule: Option<String>,
}

#[derive(Deserialize)]
struct UpdateNote {
    title: Option<String>,
    content: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    due_date: Option<String>,
    department: Option<String>,
    departments: Option<String>,
    appointments: Option<String>,
    sort_order: Option<i64>,
    repeat_rule: Option<String>,
    pinned: Option<Option<bool>>,
}

#[derive(Deserialize, Clone)]
struct ImportNote {
    title: String,
    content: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    date: Option<String>,
    due_date: Option<String>,
    department: Option<String>,
    departments: Option<String>,
    appointments: Option<String>,
    repeat_rule: Option<String>,
}

#[derive(Deserialize, Clone)]
struct ImportContact {
    name: String,
    department: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct ImportPayload {
    mode: Option<String>,
    notes: Option<Vec<ImportNote>>,
    contacts: Option<Vec<ImportContact>>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

// Create a consistent snapshot of a database into backups/ using VACUUM INTO,
// keeping a rolling window of 10 snapshots per database.
// Advance a YYYY-MM-DD date to its next occurrence for a repeat rule.
fn add_month_clamped(d: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    let (y, m) = if d.month() == 12 {
        (d.year() + 1, 1)
    } else {
        (d.year(), d.month() + 1)
    };
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let last = chrono::NaiveDate::from_ymd_opt(ny, nm, 1)?.pred_opt()?.day();
    chrono::NaiveDate::from_ymd_opt(y, m, d.day().min(last))
}

fn advance_repeat_date(iso: &str, rule: &str) -> Option<String> {
    let d = chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d").ok()?;
    let next = match rule {
        "daily" => d.succ_opt(),
        "weekly" => d.checked_add_days(chrono::Days::new(7)),
        "monthly" => add_month_clamped(d),
        _ => None,
    };
    next.map(|nd| nd.format("%Y-%m-%d").to_string())
}

// Advance the start/end dates of every appointment inside the JSON-array string.
fn advance_appointments(json: &str, rule: &str) -> String {
    let fallback = json.to_string();
    let mut arr: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_else(|_| return vec![]);
    for apt in arr.iter_mut() {
        if let Some(obj) = apt.as_object_mut() {
            if let Some(s) = obj.get("start").and_then(|v| v.as_str()) {
                if let Some(ns) = advance_repeat_date(s, rule) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&format!("\"{}\"", ns)) {
                        obj.insert("start".to_string(), v);
                    }
                }
            }
            if let Some(e) = obj.get("end").and_then(|v| v.as_str()) {
                if let Some(ne) = advance_repeat_date(e, rule) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&format!("\"{}\"", ne)) {
                        obj.insert("end".to_string(), v);
                    }
                }
            }
        }
    }
    if arr.is_empty() {
        fallback
    } else {
        serde_json::to_string(&arr).unwrap_or(fallback)
    }
}

// Advance a note's recurrence once: due_date + appointment dates move forward
// and the note returns to the backlog.
fn apply_recurrence(
    rule: &str,
    due_date: Option<String>,
    appointments: Option<String>,
) -> (Option<String>, Option<String>) {
    let new_due = match due_date {
        Some(ref d) if !d.trim().is_empty() => advance_repeat_date(d, rule).or(due_date),
        _ => advance_repeat_date(
            &chrono::Local::now().format("%Y-%m-%d").to_string(),
            rule,
        ),
    };
    let new_appts = appointments.map(|a| advance_appointments(&a, rule));
    (new_due, new_appts)
}

async fn backup_db(pool: &SqlitePool, prefix: &str) {
    let _ = std::fs::create_dir_all("backups");
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let path = format!("backups/{}_{}.db", prefix, ts);
    let _ = sqlx::query(&format!("VACUUM INTO '{}'", path))
        .execute(pool)
        .await;

    if let Ok(entries) = std::fs::read_dir("backups") {
        let mut files: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(&format!("{}_", prefix)) && n.ends_with(".db"))
            .collect();
        files.sort();
        while files.len() > 10 {
            if let Some(oldest) = files.first() {
                let _ = std::fs::remove_file(format!("backups/{}", oldest));
            }
            files.remove(0);
        }
    }
}

#[tokio::main]
async fn main() {
    let database_url = "sqlite://notice.db?mode=rwc";
    let pool = SqlitePool::connect(database_url)
        .await
        .expect("Fehler beim Verbinden mit der SQLite-Datenbank");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            status TEXT NOT NULL,
            priority TEXT NOT NULL,
            date TEXT NOT NULL,
            due_date TEXT,
            department TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Fehler beim Erstellen der Tabelle");

    // Migration: add sort_order if an existing table is missing the column
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0")
        .execute(&pool)
        .await;

    // Migration: existing databases still use the old column name "einrichtung"
    let _ = sqlx::query("ALTER TABLE notes RENAME COLUMN einrichtung TO department")
        .execute(&pool)
        .await;

    // Migration: add departments (JSON array) column
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN departments TEXT")
        .execute(&pool)
        .await;

    // Migration: add appointments (JSON array) column
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN appointments TEXT")
        .execute(&pool)
        .await;

    // Migration: migrate existing single department values into the departments JSON array
    let _ = sqlx::query(
        r#"
        UPDATE notes
        SET departments = json_array(department)
        WHERE departments IS NULL
          AND department IS NOT NULL
          AND TRIM(department) != ''
        "#,
    )
    .execute(&pool)
    .await;

    // Migration: set empty department values to an empty JSON array
    let _ = sqlx::query(
        r#"
        UPDATE notes
        SET departments = '[]'
        WHERE departments IS NULL
        "#,
    )
    .execute(&pool)
    .await;

    // Migration: add completed_at column (tracks when a note was finished/archived)
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN completed_at TEXT")
        .execute(&pool)
        .await;

    // Migration: add deleted_at column (soft-delete / Papierkorb)
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN deleted_at TEXT")
        .execute(&pool)
        .await;

    // Migration: add repeat_rule column (daily/weekly/monthly recurrence)
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN repeat_rule TEXT")
        .execute(&pool)
        .await;

    // Migration: add pinned column (board pins)
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0")
        .execute(&pool)
        .await;

    // Data database (contacts.db): separate SQLite DB for contacts & future data-view types
    let db = SqlitePool::connect("sqlite://contacts.db?mode=rwc")
        .await
        .expect("Fehler beim Verbinden mit der Kontakte-Datenbank");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS contacts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            department TEXT,
            phone TEXT,
            email TEXT,
            description TEXT
        )
        "#,
    )
    .execute(&db)
    .await
    .expect("Fehler beim Erstellen der Kontakte-Tabelle");

    // Migration: add deleted_at column (soft-delete / Papierkorb)
    let _ = sqlx::query("ALTER TABLE contacts ADD COLUMN deleted_at TEXT")
        .execute(&db)
        .await;

    // Seed sample contacts on first run (empty table)
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts")
        .fetch_one(&db)
        .await
        .unwrap_or(0);
    if count == 0 {
        let _ = sqlx::query(
            r#"
            INSERT INTO contacts (name, department, phone, email, description) VALUES
                ('Max Mustermann', 'Küche', '030 12345678', 'max.mustermann@example.de', 'Küchenchef, verantwortlich für den Speiseplan.'),
                ('Erika Musterfrau', 'Verwaltung', '030 87654321', 'erika.musterfrau@example.de', 'Leiterin der Verwaltung, Ansprechpartnerin für Rechnungen.')
            "#,
        )
        .execute(&db)
        .await;
    }

    // Snapshot both databases on every startup (rolling 10 per DB)
    backup_db(&pool, "notice").await;
    backup_db(&db, "contacts").await;

    let state = AppState { pool, db };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/notes", get(get_notes).post(create_note))
        .route("/api/notes/search", get(search_notes))
        .route("/api/departments", get(get_departments))
        .route("/api/departments/delete", post(delete_department))
        .route("/api/notes/:id", post(update_note).put(update_note).delete(delete_note))
        .route("/api/notes/:id/duplicate", post(duplicate_note))
        .route("/api/notes/:id/restore", post(restore_note))
        .route("/api/notes/:id/force", delete(force_delete_note))
        .route("/api/import", post(import_data))
        .route("/api/trash", get(get_trash))
        .route("/api/trash/clear", post(clear_trash))
        .route("/api/contacts", get(get_contacts).post(create_contact))
        .route("/api/contacts/search", get(search_contacts))
        .route("/api/contacts/:id", put(update_contact).delete(delete_contact))
        .route("/api/contacts/:id/restore", post(restore_contact))
        .route("/api/contacts/:id/force", delete(force_delete_contact))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("Server läuft auf http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn index_handler() -> Html<String> {
    let html = FRONTEND_HTML
        .replace("__TAILWIND_JS__", TAILWIND_JS)
        .replace("__VUE_JS__", VUE_JS)
        .replace("__MARKED_JS__", MARKED_JS);
    Html(html)
}

async fn get_notes(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Note>("SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned FROM notes WHERE deleted_at IS NULL ORDER BY sort_order ASC, id ASC")
        .fetch_all(&state.pool)
        .await
    {
        Ok(notes) => Json(notes).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn get_departments(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT dep.value AS name, COUNT(*) AS cnt
        FROM notes,
             json_each(COALESCE(notes.departments, '[]')) AS dep
        WHERE json_type(COALESCE(notes.departments, '[]')) IS NOT NULL
          AND notes.deleted_at IS NULL
        GROUP BY LOWER(dep.value)
        ORDER BY cnt DESC, LOWER(dep.value) ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(results) => {
            let json: Vec<serde_json::Value> = results
                .into_iter()
                .filter(|(name, _)| !name.is_empty())
                .map(|(name, count)| serde_json::json!({ "name": name, "count": count }))
                .collect();
            Json(json).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn resolve_department(
    pool: &SqlitePool,
    department: Option<String>,
) -> Option<String> {
    let val = department.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    })?;
    let existing: Option<String> = sqlx::query_scalar(
        r#"
        SELECT dep.value FROM notes, json_each(COALESCE(notes.departments, '[]')) AS dep
        WHERE LOWER(dep.value) = LOWER(?) AND dep.value != '' AND notes.deleted_at IS NULL
        LIMIT 1
        "#,
    )
    .bind(&val)
    .fetch_optional(pool)
    .await
    .ok()?;
    Some(existing.unwrap_or(val))
}

async fn search_notes(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let q = params.q.unwrap_or_default().trim().to_lowercase();
    if q.is_empty() {
        return Json(Vec::<serde_json::Value>::new()).into_response();
    }
    let pattern = format!("%{}%", q);
    match sqlx::query_as::<_, (i64, String)>(
        "SELECT id, title FROM notes WHERE LOWER(title) LIKE ? AND deleted_at IS NULL ORDER BY sort_order ASC LIMIT 8",
    )
    .bind(pattern)
    .fetch_all(&state.pool)
    .await
    {
        Ok(results) => {
            let json: Vec<serde_json::Value> = results
                .into_iter()
                .map(|(id, title)| serde_json::json!({ "id": id, "title": title }))
                .collect();
            Json(json).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn create_note(
    State(state): State<AppState>,
    Json(payload): Json<CreateNote>,
) -> impl IntoResponse {
    let date = chrono::Local::now().format("%d.%m.%Y").to_string();
    let content = payload.content.unwrap_or_default();
    let status = payload.status.unwrap_or_else(|| "backlog".to_string());
    let priority = payload.priority.unwrap_or_else(|| "medium".to_string());
    let due_date = payload.due_date.filter(|s| !s.is_empty());
    let department = resolve_department(&state.pool, payload.department).await;
    let departments = payload.departments;
    let appointments = payload.appointments;
    let repeat_rule = payload
        .repeat_rule
        .map(|s| {
            let t = s.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        })
        .flatten();

    // sort_order: maximum value in the column + 1
    let max_sort: Result<Option<i64>, _> =
        sqlx::query_scalar("SELECT MAX(sort_order) FROM notes WHERE status = ? AND deleted_at IS NULL")
            .bind(&status)
            .fetch_one(&state.pool)
            .await;
    let sort_order = max_sort.unwrap_or(None).unwrap_or(0) + 1;

    let result = sqlx::query(
        r#"
        INSERT INTO notes (title, content, status, priority, date, due_date, department, departments, appointments, sort_order, repeat_rule, pinned)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)
        "#,
    )
    .bind(payload.title)
    .bind(content)
    .bind(status)
    .bind(priority)
    .bind(date)
    .bind(due_date)
    .bind(department)
    .bind(departments)
    .bind(appointments)
    .bind(sort_order)
    .bind(repeat_rule)
    .execute(&state.pool)
    .await;

    match result {
        Ok(res) => {
            let id = res.last_insert_rowid();
            if let Ok(note) = sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ?")
                .bind(id)
                .fetch_one(&state.pool)
                .await
            {
                return (StatusCode::CREATED, Json(note)).into_response();
            }
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn update_note(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Json(payload): Json<UpdateNote>,
) -> impl IntoResponse {
    let existing = match sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(note)) => note,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let was_active = existing.status != "done" && existing.status != "archived";
    let was_done = existing.status == "done";
    let title = payload.title.unwrap_or(existing.title);
    let content = payload.content.unwrap_or(existing.content);
    let status = payload.status.unwrap_or(existing.status);
    let priority = payload.priority.unwrap_or(existing.priority);
    let due_date = payload.due_date.or(existing.due_date);
    let department = match payload.department {
        Some(s) => resolve_department(&state.pool, Some(s)).await,
        None => existing.department,
    };
    let departments = payload.departments.or(existing.departments);
    let appointments = payload.appointments.or(existing.appointments);
    let sort_order = payload.sort_order.unwrap_or(existing.sort_order);
    let repeat_rule = payload
        .repeat_rule
        .map(|s| {
            let t = s.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        })
        .flatten()
        .or(existing.repeat_rule);
    let pinned = match payload.pinned {
        Some(Some(v)) => if v { 1 } else { 0 },
        _ => existing.pinned,
    };

    // Recurring notes: completing one occurrence advances due_date + appointment
    // dates to the next occurrence and the note returns to the backlog.
    let (final_status, final_due_date, final_appointments, recurring) =
        if status == "done" && !was_done {
            if let Some(rule) = repeat_rule.as_deref() {
                let (nd, na) = apply_recurrence(rule, due_date.clone(), appointments.clone());
                if nd != due_date || na != appointments {
                    ("backlog".to_string(), nd, na, true)
                } else {
                    (status.clone(), due_date, appointments.clone(), false)
                }
            } else {
                (status.clone(), due_date, appointments.clone(), false)
            }
        } else {
            (status.clone(), due_date, appointments.clone(), false)
        };

    let is_active = final_status != "done" && final_status != "archived";
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let completed_at = if recurring {
        None
    } else if !was_active && !is_active {
        existing.completed_at.clone()
    } else if is_active {
        None
    } else {
        Some(today)
    };

    let result = sqlx::query(
        r#"
        UPDATE notes SET title = ?, content = ?, status = ?, priority = ?, due_date = ?, department = ?, departments = ?, appointments = ?, sort_order = ?, completed_at = ?, repeat_rule = ?, pinned = ? WHERE id = ?
        "#,
    )
    .bind(title)
    .bind(content)
    .bind(final_status)
    .bind(priority)
    .bind(final_due_date)
    .bind(department)
    .bind(departments)
    .bind(final_appointments)
    .bind(sort_order)
    .bind(completed_at)
    .bind(repeat_rule)
    .bind(pinned)
    .bind(id)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn delete_note(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    match sqlx::query("UPDATE notes SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL")
        .bind(now)
        .bind(id)
        .execute(&state.pool)
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn restore_note(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("UPDATE notes SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL")
        .bind(id)
        .execute(&state.pool)
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn force_delete_note(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query("DELETE FROM notes WHERE id = ? AND deleted_at IS NOT NULL")
        .bind(id)
        .execute(&state.pool)
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn get_trash(State(state): State<AppState>) -> impl IntoResponse {
    let notes = sqlx::query_as::<_, Note>(
        "SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned FROM notes WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, id DESC",
    )
    .fetch_all(&state.pool)
    .await;
    let contacts = sqlx::query_as::<_, Contact>(
        "SELECT id, name, department, phone, email, description FROM contacts WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, id DESC",
    )
    .fetch_all(&state.db)
    .await;
    match (notes, contacts) {
        (Ok(n), Ok(c)) => {
            Json(serde_json::json!({ "notes": n, "contacts": c })).into_response()
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn clear_trash(State(state): State<AppState>) -> impl IntoResponse {
    let notes_res = sqlx::query("DELETE FROM notes WHERE deleted_at IS NOT NULL")
        .execute(&state.pool)
        .await;
    let contacts_res = sqlx::query("DELETE FROM contacts WHERE deleted_at IS NOT NULL")
        .execute(&state.db)
        .await;
    if notes_res.is_ok() && contacts_res.is_ok() {
        StatusCode::OK.into_response()
    } else {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

async fn import_data(
    State(state): State<AppState>,
    Json(payload): Json<ImportPayload>,
) -> impl IntoResponse {
    let mode = payload.mode.as_deref().unwrap_or("merge");
    let pnotes = payload.notes.unwrap_or_default();
    let pcontacts = payload.contacts.unwrap_or_default();
    let mut notes_skipped = 0usize;
    let mut contacts_skipped = 0usize;

    if mode == "replace" {
        let _ = sqlx::query("DELETE FROM notes").execute(&state.pool).await;
        let _ = sqlx::query("DELETE FROM contacts").execute(&state.db).await;
    }

    let mut notes_imported = 0usize;
    for n in &pnotes {
        let title = n.title.trim().to_string();
        if title.is_empty() {
            notes_skipped += 1;
            continue;
        }
        if mode == "merge" {
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM notes WHERE LOWER(title) = LOWER(?) AND deleted_at IS NULL LIMIT 1",
            )
            .bind(&title)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();
            if existing.is_some() {
                notes_skipped += 1;
                continue;
            }
        }
        let content = n.content.clone().unwrap_or_default();
        let status = n.status.clone().unwrap_or_else(|| "backlog".to_string());
        let priority = n.priority.clone().unwrap_or_else(|| "medium".to_string());
        let date = n
            .date
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| chrono::Local::now().format("%d.%m.%Y").to_string());
        let due_date = n.due_date.clone().filter(|s| !s.trim().is_empty());
        let department = n
            .department
            .clone()
            .map(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            })
            .flatten();
        let departments = n.departments.clone().filter(|s| !s.trim().is_empty());
        let appointments = n.appointments.clone().filter(|s| !s.trim().is_empty());
        let repeat_rule = n
            .repeat_rule
            .clone()
            .map(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            })
            .flatten();

        let max_sort: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(sort_order) FROM notes WHERE status = ? AND deleted_at IS NULL",
        )
        .bind(&status)
        .fetch_one(&state.pool)
        .await
        .ok()
        .flatten();
        let sort_order = max_sort.unwrap_or(0) + 1;

        let res = sqlx::query(
            r#"
            INSERT INTO notes (title, content, status, priority, date, due_date, department, departments, appointments, sort_order, repeat_rule, pinned)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)
            "#,
        )
        .bind(&title)
        .bind(content)
        .bind(status)
        .bind(priority)
        .bind(date)
        .bind(due_date)
        .bind(department)
        .bind(departments)
        .bind(appointments)
        .bind(sort_order)
        .bind(repeat_rule)
        .execute(&state.pool)
        .await;
        if res.is_ok() {
            notes_imported += 1;
        } else {
            notes_skipped += 1;
        }
    }

    let mut contacts_imported = 0usize;
    for c in &pcontacts {
        let name = c.name.trim().to_string();
        if name.is_empty() {
            contacts_skipped += 1;
            continue;
        }
        if mode == "merge" {
            let existing: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM contacts WHERE LOWER(name) = LOWER(?) AND deleted_at IS NULL LIMIT 1",
            )
            .bind(&name)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
            if existing.is_some() {
                contacts_skipped += 1;
                continue;
            }
        }
        let department = c
            .department
            .clone()
            .map(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            })
            .flatten();
        let phone = c
            .phone
            .clone()
            .map(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            })
            .flatten();
        let email = c
            .email
            .clone()
            .map(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            })
            .flatten();
        let description = c
            .description
            .clone()
            .map(|s| {
                let t = s.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            })
            .flatten();

        let res = sqlx::query(
            "INSERT INTO contacts (name, department, phone, email, description) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&name)
        .bind(department)
        .bind(phone)
        .bind(email)
        .bind(description)
        .execute(&state.db)
        .await;
        if res.is_ok() {
            contacts_imported += 1;
        } else {
            contacts_skipped += 1;
        }
    }

    Json(serde_json::json!({
        "notes_imported": notes_imported,
        "notes_skipped": notes_skipped,
        "contacts_imported": contacts_imported,
        "contacts_skipped": contacts_skipped
    }))
    .into_response()
}

async fn duplicate_note(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let existing = match sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(note)) => note,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let new_title = format!("{} (Kopie)", existing.title);
    let date = chrono::Local::now().format("%d.%m.%Y").to_string();

    let max_sort: Result<Option<i64>, _> =
        sqlx::query_scalar("SELECT MAX(sort_order) FROM notes WHERE status = ? AND deleted_at IS NULL")
            .bind(&existing.status)
            .fetch_one(&state.pool)
            .await;
    let sort_order = max_sort.unwrap_or(None).unwrap_or(0) + 1;

    let result = sqlx::query(
        r#"
        INSERT INTO notes (title, content, status, priority, date, due_date, department, departments, appointments, sort_order, repeat_rule, pinned)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)
        "#,
    )
    .bind(new_title)
    .bind(existing.content)
    .bind(existing.status)
    .bind(existing.priority)
    .bind(date)
    .bind(existing.due_date)
    .bind(existing.department)
    .bind(existing.departments)
    .bind(existing.appointments)
    .bind(sort_order)
    .bind(existing.repeat_rule)
    .execute(&state.pool)
    .await;

    match result {
        Ok(res) => {
            let new_id = res.last_insert_rowid();
            if let Ok(note) = sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ?")
                .bind(new_id)
                .fetch_one(&state.pool)
                .await
            {
                return (StatusCode::CREATED, Json(note)).into_response();
            }
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn get_contacts(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Contact>(
        "SELECT id, name, department, phone, email, description FROM contacts WHERE deleted_at IS NULL ORDER BY LOWER(name) ASC, id ASC",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(contacts) => Json(contacts).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn create_contact(
    State(state): State<AppState>,
    Json(payload): Json<CreateContact>,
) -> impl IntoResponse {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let department = payload
        .department
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let phone = payload
        .phone
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let email = payload
        .email
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let description = payload
        .description
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let result = sqlx::query(
        "INSERT INTO contacts (name, department, phone, email, description) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(name)
    .bind(department)
    .bind(phone)
    .bind(email)
    .bind(description)
    .execute(&state.db)
    .await;

    match result {
        Ok(res) => {
            let id = res.last_insert_rowid();
            if let Ok(contact) =
                sqlx::query_as::<_, Contact>("SELECT * FROM contacts WHERE id = ?")
                    .bind(id)
                    .fetch_one(&state.db)
                    .await
            {
                return (StatusCode::CREATED, Json(contact)).into_response();
            }
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn update_contact(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Json(payload): Json<UpdateContact>,
) -> impl IntoResponse {
    let existing = match sqlx::query_as::<_, Contact>("SELECT * FROM contacts WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let name = match payload.name {
        Some(s) => {
            let t = s.trim().to_string();
            if t.is_empty() {
                return StatusCode::BAD_REQUEST.into_response();
            }
            t
        }
        None => existing.name,
    };
    let department = payload
        .department
        .map(|s| {
            let t = s.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        })
        .unwrap_or(existing.department);
    let phone = payload
        .phone
        .map(|s| {
            let t = s.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        })
        .unwrap_or(existing.phone);
    let email = payload
        .email
        .map(|s| {
            let t = s.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        })
        .unwrap_or(existing.email);
    let description = payload
        .description
        .map(|s| {
            let t = s.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        })
        .unwrap_or(existing.description);

    let result = sqlx::query(
        "UPDATE contacts SET name = ?, department = ?, phone = ?, email = ?, description = ? WHERE id = ?",
    )
    .bind(name)
    .bind(department)
    .bind(phone)
    .bind(email)
    .bind(description)
    .bind(id)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
            if let Ok(contact) =
                sqlx::query_as::<_, Contact>("SELECT * FROM contacts WHERE id = ?")
                    .bind(id)
                    .fetch_one(&state.db)
                    .await
            {
                return Json(contact).into_response();
            }
            StatusCode::OK.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn delete_contact(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    match sqlx::query("UPDATE contacts SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL")
        .bind(now)
        .bind(id)
        .execute(&state.db)
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn restore_contact(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query("UPDATE contacts SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL")
        .bind(id)
        .execute(&state.db)
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn force_delete_contact(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match sqlx::query("DELETE FROM contacts WHERE id = ? AND deleted_at IS NOT NULL")
        .bind(id)
        .execute(&state.db)
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn search_contacts(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let q = params.q.unwrap_or_default().trim().to_lowercase();
    if q.is_empty() {
        return Json(Vec::<serde_json::Value>::new()).into_response();
    }
    let pattern = format!("%{}%", q);
    match sqlx::query_as::<_, (i64, String)>(
        "SELECT id, name FROM contacts WHERE LOWER(name) LIKE ? AND deleted_at IS NULL ORDER BY LOWER(name) ASC LIMIT 8",
    )
    .bind(pattern)
    .fetch_all(&state.db)
    .await
    {
        Ok(results) => {
            let json: Vec<serde_json::Value> = results
                .into_iter()
                .map(|(id, name)| serde_json::json!({ "id": id, "name": name }))
                .collect();
            Json(json).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn delete_department(
    State(state): State<AppState>,
    Json(payload): Json<DeleteDepartment>,
) -> impl IntoResponse {
    let name = payload.name.trim().to_lowercase();
    if name.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    // Update the notes pool: remove the department from every note's departments JSON array,
    // and clear the legacy single-value department column where it matches.
    let notes_result = sqlx::query(
        r#"
        UPDATE notes
        SET departments = (
            SELECT COALESCE(json_group_array(e.value), '[]')
            FROM json_each(COALESCE(notes.departments, '[]')) AS e
            WHERE LOWER(TRIM(e.value)) != LOWER(?)
              AND TRIM(e.value) != ''
        ),
        department = CASE WHEN LOWER(TRIM(department)) = LOWER(?) THEN NULL ELSE department END
        "#,
    )
    .bind(&name)
    .bind(&name)
    .execute(&state.pool)
    .await;

    // Update the contacts db: clear the department field where it matches.
    let contacts_result = sqlx::query(
        "UPDATE contacts SET department = NULL WHERE LOWER(TRIM(department)) = LOWER(?)",
    )
    .bind(&name)
    .execute(&state.db)
    .await;

    if notes_result.is_err() || contacts_result.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Return the freshly aggregated department list
    match sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT dep.value AS name, COUNT(*) AS cnt
        FROM notes,
             json_each(COALESCE(notes.departments, '[]')) AS dep
        WHERE json_type(COALESCE(notes.departments, '[]')) IS NOT NULL
          AND notes.deleted_at IS NULL
        GROUP BY LOWER(dep.value)
        ORDER BY cnt DESC, LOWER(dep.value) ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(results) => {
            let json: Vec<serde_json::Value> = results
                .into_iter()
                .filter(|(dep, _)| !dep.is_empty())
                .map(|(dep, count)| serde_json::json!({ "name": dep, "count": count }))
                .collect();
            Json(json).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

const FRONTEND_HTML: &str = r#"<!DOCTYPE html>
<html lang="de" class="dark">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Notice - Pro Kanban Notes</title>
    <script>
        (function () {
            try {
                if (localStorage.getItem('notice-theme') === 'light') document.documentElement.classList.remove('dark')
            } catch (e) {}
            var origWarn = console.warn.bind(console)
            console.warn = function () {
                var s = Array.prototype.map.call(arguments, String).join(' ')
                if (s.indexOf('cdn.tailwindcss.com should not be used in production') !== -1) return
                origWarn.apply(console, arguments)
            }
        })()
    </script>
    <script>__TAILWIND_JS__</script>
    <script>__VUE_JS__</script>
    <script>__MARKED_JS__</script>
    <script>
        tailwind.config = {
            darkMode: 'class',
            theme: {
                extend: {
                    borderRadius: { none: '0px', DEFAULT: '0px', sm: '0px', md: '0px', lg: '0px', xl: '0px', '2xl': '0px', '3xl': '0px', full: '0px' },
                    colors: {
                        zinc: {
                            50: '#dcdce0',
                            100: '#d0d0d5',
                            200: '#bfc0c6',
                            300: '#adaeb5',
                            400: '#8b8b93',
                            500: '#63646b',
                            600: '#45464d',
                            700: '#34353b',
                            800: '#27272a',
                            900: '#18181b',
                            950: '#09090b'
                        }
                    }
                }
            }
        }
    </script>
    <style>
        :root {
            --bg-page: #d0d0d5;
            --bg-panel: #dcdce0;
            --bg-card: #18181b;
            --border: #bfc0c6;
            --border-hover: #71727a;
            --text: #17171a;
            --text-muted: #36373d;
            --text-faint: #4e4f56;
            --code-bg: #dcdce0;
            --green: #047857;
            --green-bright: #047857;
            --green-hover: #065f46;
            --blue: #1e40af;
            --blue-bright: #1e40af;
            --red-bright: #b91c1c;
            --violet-bright: #6d28d9;
            --apt-bg: #dcdce0;
        }
        html.dark {
            --bg-page: #0d0d0f;
            --bg-panel: #18181b;
            --bg-card: #0d0d0f;
            --border: #27272a;
            --border-hover: #3f3f46;
            --text: #d4d4d8;
            --text-muted: #a1a1aa;
            --text-faint: #71717a;
            --code-bg: #27272a;
            --green: #34d399;
            --green-bright: #6ee7b7;
            --green-hover: #6ee7b7;
            --blue: #60a5fa;
            --blue-bright: #93c5fd;
            --red-bright: #fca5a5;
            --violet-bright: #a78bfa;
            --apt-bg: #18181b;
        }
        html, body {
            height: 100vh;
            margin: 0;
            overflow: hidden;
            background-color: var(--bg-page);
            color: var(--text);
            font-family: 'Courier New', Courier, Lucida Console, Monaco, monospace;
        }
        .markdown-body h1 { font-size: 1.15rem; font-weight: bold; margin-bottom: 0.4rem; }
        .markdown-body h2 { font-size: 1.05rem; font-weight: bold; margin-bottom: 0.3rem; }
        .markdown-body p { margin-bottom: 0.4rem; }
        .markdown-body ul { list-style-type: disc; padding-left: 1.1rem; margin-bottom: 0.4rem; }
        .markdown-body ol { list-style-type: decimal; padding-left: 1.1rem; margin-bottom: 0.4rem; }
        .markdown-body code { background: var(--code-bg); color: var(--text); padding: 0.1rem 0.2rem; font-size: 0.8em; }
        /* User-defined colors for markdown HTML output */
        .markdown-body span[style*="color"] { opacity: 0.9; }
        .note-link { color: var(--green); text-decoration: underline; cursor: pointer; }
        .address-link { color: var(--violet-bright, #8b5cf6); text-decoration: underline; cursor: pointer; }
        .address-link:hover { background: rgba(139, 92, 246, 0.12); }
        .note-link:hover { color: var(--green-hover); background: rgba(52, 211, 153, 0.1); }
        .autocomplete-dropdown { position: absolute; z-index: 70; min-width: 220px; max-width: 320px; }
        .autocomplete-item { cursor: pointer; }
        .autocomplete-item.active { background: rgba(52, 211, 153, 0.15); }
        .archive-expand-btn {
            writing-mode: vertical-rl;
            text-orientation: mixed;
            letter-spacing: 0.2em;
            font-size: 9px;
            font-weight: bold;
            text-transform: uppercase;
        }
        .dept-tag {
            display: inline-flex;
            align-items: center;
            gap: 4px;
            padding: 1px 6px;
            font-size: 9px;
            font-family: 'Courier New', Courier, Lucida Console, Monaco, monospace;
            background: rgba(14, 165, 233, 0.1);
            border: 1px solid rgba(14, 165, 233, 0.3);
            color: rgb(14, 165, 233);
        }
        .dept-tag-remove {
            cursor: pointer;
            font-size: 10px;
            line-height: 1;
            opacity: 0.6;
        }
        .dept-tag-remove:hover { opacity: 1; color: rgb(248, 113, 113); }
        .dept-tag-lg { font-size: 10px; padding: 2px 8px; }
        .dept-tag-sm { font-size: 8px; padding: 0px 4px; }
        .cal-grid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 1px; }
        .cal-header { font-size: 10px; text-transform: uppercase; color: var(--text-faint); text-align: center; padding: 4px 0; border-bottom: 1px solid var(--border); }
        .cal-day { min-height: 72px; padding: 3px 4px; border: 1px solid var(--border); font-size: 10px; position: relative; cursor: pointer; min-width: 0; overflow: hidden; }
        .cal-day:hover { background: rgba(113, 113, 122, 0.15); }
        .cal-day-num { font-weight: bold; margin-bottom: 2px; }
        .cal-day-num.today { color: var(--green); }
        .cal-day-num.other-month { color: var(--text-faint); }
        .cal-dot { width: 5px; height: 5px; border-radius: 50%; display: inline-block; margin: 1px 1px 1px 0; }
        .cal-event-item { font-size: 9px; padding: 2px 4px; margin-top: 1px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; cursor: pointer; }
        .cal-event-due { background: rgba(37, 99, 235, 0.12); border-left: 2px solid #3b82f6; color: var(--blue-bright); }
        .cal-event-apt { background: rgba(5, 150, 105, 0.12); border-left: 2px solid #10b981; color: var(--green-bright); }
        .cal-event-overdue { background: rgba(239, 68, 68, 0.12); border-left: 2px solid #ef4444; color: var(--red-bright); }
        .cal-week-grid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 1px; }
        .cal-week-col { border: 1px solid var(--border); min-height: 400px; padding: 0; overflow: hidden; }
        .cal-week-header { text-align: center; font-size: 10px; font-weight: bold; padding: 4px; border-bottom: 1px solid var(--border); }
        .cal-week-header.today { color: var(--green); }
        .cal-timeline { --pix-per-hour: 36px; position: relative; }
        .cal-timeline-lane { position: relative; }
        .cal-timeline-hour { position: absolute; left: 0; right: 0; height: 36px; border-bottom: 1px solid var(--border); pointer-events: none; }
        .cal-timeline-time { position: absolute; left: 2px; z-index: 3; font-size: 8px; color: var(--text-faint); pointer-events: none; transform: translateY(-50%); }
        .cal-timeline-block { position: absolute; z-index: 2; overflow-y: auto; font-size: 9px; line-height: 1.2; padding: 1px 2px; border-left: 2px solid; white-space: normal; overflow-wrap: break-word; cursor: pointer; box-sizing: border-box; }
        .cal-timeline-allday { font-size: 9px; }
        .cal-timeline-block.cal-event-apt { background: rgba(5, 150, 105, 0.18); border-left-color: #10b981; color: var(--green-bright); }
        .cal-timeline-block.cal-event-overdue { background: rgba(239, 68, 68, 0.18); border-left-color: #ef4444; color: var(--red-bright); }
        .cal-timeline-block.cal-event-due { background: rgba(37, 99, 235, 0.18); border-left-color: #3b82f6; color: var(--blue-bright); }
        .apt-item { display: flex; align-items: center; gap: 6px; padding: 4px 6px; background: var(--apt-bg); border: 1px solid var(--border); font-size: 11px; min-width: 0; overflow: hidden; }
        .apt-item:hover { border-color: var(--border-hover); }

        /* Print: clean board output */
        @media print {
            html, body { height: auto !important; overflow: visible !important; background: #fff !important; color: #000 !important; }
            header, .fixed { display: none !important; }
            main { overflow: visible !important; padding: 0 !important; background: #fff !important; }
            main > .flex-1, main .overflow-y-auto { overflow: visible !important; }
            .cal-day, .cal-week-col, [draggable="true"] { break-inside: avoid; page-break-inside: avoid; }
            * { box-shadow: none !important; }
        }
    </style>
</head>
<body>
    <div id="app" class="h-screen flex flex-col" @click="closeContextMenu">
        <header class="bg-zinc-50 dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-800 px-3 py-2 flex justify-between items-center shrink-0 gap-4">
            <h1 class="text-xs font-bold tracking-widest text-emerald-600 dark:text-emerald-400 flex items-center gap-2 shrink-0">
                <span class="inline-block w-2 h-2 bg-emerald-500"></span> NOTICE_V1.4
            </h1>

            <div class="flex gap-1 shrink-0 flex-wrap justify-center">
                <button 
                    @click="activeView = 'dashboard'"
                    :class="activeView === 'dashboard' ? 'bg-zinc-300 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 border-zinc-400 dark:border-zinc-600' : 'bg-zinc-50 dark:bg-zinc-900 text-zinc-600 dark:text-zinc-500 border-zinc-200 dark:border-zinc-800 hover:text-zinc-700 dark:text-zinc-300'"
                    class="px-3 py-1 text-[11px] font-bold tracking-wider border cursor-pointer transition-colors">
                    DASHBOARD
                </button>
                <button 
                    @click="activeView = 'board'"
                    :class="activeView === 'board' ? 'bg-zinc-300 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 border-zinc-400 dark:border-zinc-600' : 'bg-zinc-50 dark:bg-zinc-900 text-zinc-600 dark:text-zinc-500 border-zinc-200 dark:border-zinc-800 hover:text-zinc-700 dark:text-zinc-300'"
                    class="px-3 py-1 text-[11px] font-bold tracking-wider border cursor-pointer transition-colors">
                    BOARD
                </button>
                <button 
                    @click="activeView = 'calendar'"
                    :class="activeView === 'calendar' ? 'bg-zinc-300 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 border-zinc-400 dark:border-zinc-600' : 'bg-zinc-50 dark:bg-zinc-900 text-zinc-600 dark:text-zinc-500 border-zinc-200 dark:border-zinc-800 hover:text-zinc-700 dark:text-zinc-300'"
                    class="px-3 py-1 text-[11px] font-bold tracking-wider border cursor-pointer transition-colors">
                    KALENDER
                </button>
                <button 
                    @click="activeView = 'addressbook'"
                    :class="activeView === 'addressbook' ? 'bg-zinc-300 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 border-zinc-400 dark:border-zinc-600' : 'bg-zinc-50 dark:bg-zinc-900 text-zinc-600 dark:text-zinc-500 border-zinc-200 dark:border-zinc-800 hover:text-zinc-700 dark:text-zinc-300'"
                    class="px-3 py-1 text-[11px] font-bold tracking-wider border cursor-pointer transition-colors">
                    ADRESSBUCH
                </button>
                <button 
                    @click="activeView = 'data'"
                    :class="activeView === 'data' ? 'bg-zinc-300 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 border-zinc-400 dark:border-zinc-600' : 'bg-zinc-50 dark:bg-zinc-900 text-zinc-600 dark:text-zinc-500 border-zinc-200 dark:border-zinc-800 hover:text-zinc-700 dark:text-zinc-300'"
                    class="px-3 py-1 text-[11px] font-bold tracking-wider border cursor-pointer transition-colors">
                    DATEN
                </button>
                <button 
                    @click="toggleTheme" 
                    :title="isDark ? 'Zum hellen Modus wechseln' : 'Zum dunklen Modus wechseln'"
                    class="px-3 py-1 text-[11px] font-bold tracking-wider border cursor-pointer transition-colors bg-zinc-50 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300 border-zinc-200 dark:border-zinc-700 hover:text-emerald-600 dark:hover:text-emerald-400">
                    {{ isDark ? '☀' : '☾' }}
                </button>
                <button 
                    @click="isSettingsOpen = !isSettingsOpen"
                    title="Einstellungen"
                    class="px-3 py-1 text-[11px] font-bold tracking-wider border cursor-pointer transition-colors bg-zinc-50 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300 border-zinc-200 dark:border-zinc-700 hover:text-emerald-600 dark:hover:text-emerald-400">
                    ⚙
                </button>
            </div>

            <div class="flex-1 flex gap-1.5 items-center min-w-0">
                <input 
                    ref="searchInputRef"
                    type="text" 
                    v-model="searchQuery" 
                    placeholder="Suchen (Strg+K)..." 
                    class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2.5 py-1 text-xs text-zinc-900 dark:text-zinc-100 focus:outline-none focus:border-emerald-500 flex-1 min-w-0"
                >
                <div class="relative shrink-0">
                    <button 
                        @click.stop="timerOpen = !timerOpen"
                        :title="timerRunning || timerRemaining > 0 ? 'Countdown läuft – ' + fmtTimer() : 'Countdown-Timer'"
                        :class="timerRunning ? 'bg-emerald-700 hover:bg-emerald-600 text-white dark:text-zinc-100 border-emerald-600' : (timerRemaining > 0 ? 'bg-amber-200 dark:bg-amber-900 text-zinc-800 dark:text-amber-100 border-amber-300 dark:border-amber-700' : 'bg-zinc-200 dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300 border-zinc-300 dark:border-zinc-700 hover:bg-zinc-300 dark:bg-zinc-700')"
                        class="px-2.5 py-1 text-xs tabular-nums border cursor-pointer transition-colors">
                        ⏱ {{ fmtTimer() }}
                    </button>
                    <div 
                        v-if="timerOpen"
                        class="absolute right-0 top-full mt-1 bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 shadow-2xl py-1 text-xs z-50 min-w-[150px]"
                    >
                        <div class="px-2.5 py-1 text-[10px] text-zinc-600 dark:text-zinc-500 uppercase tracking-wider border-b border-zinc-200 dark:border-zinc-800 mb-1 flex justify-between items-center">
                            <span>Timer</span>
                            <span class="text-zinc-400 dark:text-zinc-600">⏱ {{ fmtTimer() }}</span>
                        </div>
                        <button v-for="m in [5, 10, 15, 25, 30]" :key="m"
                            @click="timerSet(m)"
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 cursor-pointer"
                            :class="{ 'font-bold': timerTotal === m * 60 }">
                            {{ m }} Minuten
                        </button>
                        <div class="border-t border-zinc-200 dark:border-zinc-700 my-1"></div>
                        <button 
                            @click="timerToggle()"
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 cursor-pointer">
                            {{ timerRunning ? 'Pause' : 'Start' }}
                        </button>
                        <button 
                            @click="timerReset()"
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 cursor-pointer">
                            Reset
                        </button>
                    </div>
                </div>
                <button 
                    @click="openNewNote" 
                    class="bg-emerald-700 hover:bg-emerald-600 text-white dark:text-zinc-100 px-3 py-1 text-xs border border-emerald-600 font-semibold cursor-pointer transition-colors shrink-0">
                    + NEU
                </button>
                <div class="relative shrink-0">
                    <button 
                        @click.stop="exportMenuOpen = !exportMenuOpen" 
                        :disabled="notes.length === 0 && contacts.length === 0"
                        :class="notes.length === 0 && contacts.length === 0 ? 'opacity-40 cursor-not-allowed bg-zinc-50 dark:bg-zinc-900 border-zinc-200 dark:border-zinc-800 text-zinc-400 dark:text-zinc-600' : 'bg-zinc-200 dark:bg-zinc-800 hover:bg-zinc-300 dark:bg-zinc-700 text-zinc-700 dark:text-zinc-300 border-zinc-300 dark:border-zinc-700 cursor-pointer'"
                        class="px-2.5 py-1 text-xs border">
                        Daten ▾
                    </button>
                    <div 
                        v-if="exportMenuOpen"
                        class="absolute right-0 top-full mt-1 bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 shadow-2xl py-1 text-xs font-mono z-50 min-w-[180px]"
                    >
                        <button 
                            @click="exportJson(); exportMenuOpen = false" 
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 cursor-pointer">
                            Als JSON
                        </button>
                        <button 
                            @click="exportCsv(); exportMenuOpen = false" 
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 cursor-pointer">
                            Als CSV
                        </button>
                        <div class="border-t border-zinc-200 dark:border-zinc-700 my-1"></div>
                        <button 
                            @click="exportFullBackup(); exportMenuOpen = false" 
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 cursor-pointer">
                            Backup (alles)
                        </button>
                        <div class="border-t border-zinc-200 dark:border-zinc-700 my-1"></div>
                        <button 
                            @click="startImport('merge'); exportMenuOpen = false" 
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 cursor-pointer">
                            Import (zusammenführen)
                        </button>
                        <button 
                            @click="startImport('replace'); exportMenuOpen = false" 
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 cursor-pointer">
                            Import (ersetzen)
                        </button>
                    </div>
                    <input ref="importFileInputRef" type="file" accept=".json,application/json" class="hidden" @change="onImportFile">
                </div>
            </div>
        </header>

        <main class="flex-1 flex gap-1.5 p-2 overflow-hidden bg-zinc-100 dark:bg-zinc-950">

            <!-- BOARD VIEW -->
            <template v-if="activeView === 'board'">
            <div class="flex-1 flex flex-col gap-1.5 min-w-0 h-full">

            <!-- Board filter bar -->
            <div class="bg-zinc-100/70 dark:bg-zinc-900/70 border border-zinc-200 dark:border-zinc-800 px-2 py-1 flex items-center gap-1.5 shrink-0">
                <span class="text-[10px] font-bold text-zinc-600 dark:text-zinc-500 uppercase tracking-wider">Filter:</span>
                <button 
                    v-for="f in [{id:'all',label:'Alle'},{id:'overdue',label:'Überfällig'},{id:'due',label:'Mit Datum'},{id:'nodate',label:'Ohne Datum'}]" 
                    :key="f.id"
                    @click="boardVisibility = f.id"
                    class="px-2 py-0.5 text-[10px] border cursor-pointer transition-colors"
                    :class="boardVisibility === f.id ? 'bg-zinc-300 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 border-zinc-400 dark:border-zinc-600' : 'bg-zinc-50 dark:bg-zinc-900 text-zinc-600 dark:text-zinc-500 border-zinc-200 dark:border-zinc-800 hover:text-zinc-700 dark:text-zinc-300'">
                    {{ f.label }}
                </button>
                <span class="w-px h-4 bg-zinc-300 dark:bg-zinc-700 mx-1"></span>
                <select 
                    v-model="boardDept"
                    class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-700 dark:text-zinc-300 cursor-pointer max-w-[140px]">
                    <option value="">Alle Abteilungen</option>
                    <option v-for="d in departments" :key="d" :value="d">{{ d }}</option>
                </select>
            </div>

            <div class="flex-1 flex gap-1.5 min-h-0">

            <!-- Left: vertical tab stack for minimized columns (archive-like background) -->
            <div v-if="collapsedStatuses.length > 0" class="bg-zinc-100/70 dark:bg-zinc-900/70 border border-zinc-200 dark:border-zinc-800 flex flex-col items-center h-full overflow-y-auto shrink-0" style="width:40px">
                <div 
                    v-for="status in collapsedStatuses" 
                    :key="status.id"
                    class="flex flex-col items-center py-2 w-full shrink-0 border-b border-zinc-200 dark:border-zinc-800"
                >
                    <button 
                        @click="expandColumn(status.id)"
                        class="archive-expand-btn text-zinc-600 dark:text-zinc-500 hover:text-zinc-800 dark:text-zinc-200 flex items-center px-1 py-2 cursor-pointer transition-colors"
                        :title="status.title + ' öffnen'"
                    >
                        {{ status.title }}
                    </button>
                    <span class="text-zinc-400 dark:text-zinc-600 text-[10px] mt-1 leading-none">{{ getNotesByColumn(status.id).length }}</span>
                </div>
            </div>

            <!-- Center: expanded columns as a grid -->
            <div 
                class="flex-1 grid gap-1.5 overflow-hidden transition-all duration-300 min-w-0"
                :style="{ gridTemplateColumns: gridColsStyle }"
            >
                <div 
                    v-for="column in expandedColumns" 
                    :key="column.id"
                    class="bg-zinc-100/70 dark:bg-zinc-900/70 border border-zinc-200 dark:border-zinc-800 flex flex-col h-full overflow-hidden"
                    @dragover.prevent
                    @drop="onDrop(column.id)"
                >
                    <div class="bg-zinc-50 dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-800 px-2.5 py-1.5 flex justify-between items-center shrink-0">
                        <button @click="toggleCollapse(column.id)" class="text-[11px] font-bold tracking-wider text-zinc-700 dark:text-zinc-300 uppercase flex items-center gap-1.5 cursor-pointer">
                            <span class="w-1.5 h-1.5" :class="{
                                'bg-zinc-400 dark:bg-zinc-500': column.id === 'backlog',
                                'bg-blue-500': column.id === 'in_progress',
                                'bg-amber-500': column.id === 'review',
                                'bg-emerald-500': column.id === 'done',
                                'bg-zinc-300 dark:bg-zinc-700': column.id === 'archived'
                            }"></span>
                            {{ column.title }}
                        </button>
                        <span class="bg-zinc-100 dark:bg-zinc-950 text-zinc-600 dark:text-zinc-400 text-[10px] px-1.5 py-0.5 border border-zinc-200 dark:border-zinc-800">
                            {{ getNotesByColumn(column.id).length }}
                        </span>
                    </div>

                    <div class="flex-1 p-1.5 overflow-y-auto space-y-1.5">
                        <div v-if="getNotesByColumn(column.id).length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic text-center py-4">
                            {{ column.id === 'archived' ? 'Keine archivierten Notizen.' : 'Keine Notizen.' }}
                        </div>
                        <div 
                            v-for="note in getNotesByColumn(column.id)" 
                            :key="note.id"
                            draggable="true"
                            @dragstart="startDrag(note)"
                            @dragover.prevent="onDragOver(note, $event)"
                            @drop.prevent.stop="onDrop(column.id, note)"
                            @dblclick="openModal(note)"
                            class="bg-zinc-50 dark:bg-zinc-900 border p-2.5 cursor-pointer hover:border-zinc-400 dark:border-zinc-500 transition-colors shadow-sm group relative"
                            :class="column.id === 'archived' ? 'opacity-70 border-zinc-200 dark:border-zinc-800' : {
                                'bg-red-950/10': note.priority === 'high' && note.status !== 'done',
                                'bg-amber-950/10': note.priority === 'medium' && note.status !== 'done',
                                'bg-zinc-50 dark:bg-zinc-900': note.priority === 'low' && note.status !== 'done',
                                'bg-emerald-950/15': note.status === 'done'
                            }"
                        >
                            <div class="flex justify-between items-start gap-1 mb-1.5">
                                <h3 class="font-bold text-xs text-zinc-900 dark:text-zinc-100 break-all leading-snug pr-1" :class="{ 'text-zinc-600 dark:text-zinc-400': column.id === 'archived' }">{{ note.title }}</h3>
                                <div class="flex gap-1 shrink-0">
                                    <button @click.stop="togglePin(note)" :title="note.pinned ? 'Aus der Pin-Liste entfernen' : 'Pinnen'" class="text-[10px] px-1 font-mono" :class="note.pinned ? 'text-amber-500 dark:text-amber-400' : 'text-zinc-400 dark:text-zinc-600 hover:text-amber-500 dark:hover:text-amber-400'">📌</button>
                                    <button @click.stop="duplicateNote(note)" title="Duplizieren" class="text-zinc-400 dark:text-zinc-600 hover:text-emerald-600 dark:text-emerald-400 text-[10px] px-1 font-mono">[+]</button>
                                    <button v-if="column.id === 'archived'" @click.stop="unarchiveNote(note)" title="Wiederherstellen" class="text-zinc-400 dark:text-zinc-600 hover:text-emerald-600 dark:text-emerald-400 text-[10px] px-1 font-mono shrink-0">[R]</button>
                                    <button v-else @click.stop="archiveNote(note)" title="Archivieren" class="text-zinc-400 dark:text-zinc-600 hover:text-zinc-700 dark:text-zinc-300 text-[10px] px-1 font-mono">[A]</button>
                                    <button @click.stop="deleteNote(note.id)" class="text-zinc-400 dark:text-zinc-600 hover:text-red-600 dark:text-red-400 text-[10px] px-1 font-mono shrink-0">[X]</button>
                                </div>
                            </div>

                            <div class="flex justify-between items-center text-[10px] text-zinc-600 dark:text-zinc-400 pt-1 border-t border-zinc-200/60 dark:border-zinc-800/60">
                                <template v-if="column.id === 'archived'">
                                    <span v-if="note.due_date" class="text-orange-600 dark:text-orange-400/70 font-mono">Fällig: {{ fmtDate(note.due_date) }}</span>
                                    <span v-else class="text-zinc-400 dark:text-zinc-600 font-mono">Kein Datum</span>
                                </template>
                                <template v-else>
                                <span v-if="isOverdue(note)" class="text-red-600 dark:text-red-400/90 font-mono flex items-center gap-1">
                                    <span class="inline-block w-1.5 h-1.5 bg-red-500 rounded-full animate-pulse"></span>
                                    ÜBERFÄLLIG ({{ fmtDate(note.due_date) }})
                                </span>
                                <span v-else-if="isDueSoon(note)" class="text-orange-600 dark:text-orange-400/90 font-mono flex items-center gap-1">
                                    <span class="inline-block w-1.5 h-1.5 bg-orange-400 rounded-full animate-pulse"></span>
                                    FÄLLIG ({{ fmtDate(note.due_date) }})
                                </span>
                                <span v-else-if="note.due_date" class="text-zinc-600 dark:text-zinc-500 font-mono">Fällig: {{ fmtDate(note.due_date) }}</span>
                                <span v-else class="text-zinc-400 dark:text-zinc-600 font-mono">Kein Datum</span>
                                </template>
                                <span class="uppercase tracking-widest text-[9px] font-bold px-1 bg-zinc-100 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800" :class="{
                                    'text-emerald-600 dark:text-emerald-400 border-emerald-500/60 dark:border-emerald-400/50': note.status === 'done',
                                    'text-red-600 dark:text-red-400 border-red-500/60 dark:border-red-500/50': column.id !== 'archived' && note.status !== 'done' && note.priority === 'high',
                                    'text-amber-600 dark:text-amber-400 border-amber-500/60 dark:border-amber-500/50': column.id !== 'archived' && note.status !== 'done' && note.priority === 'medium',
                                    'text-zinc-600 dark:text-zinc-400 border-zinc-300 dark:border-zinc-600': column.id !== 'archived' && note.status !== 'done' && note.priority === 'low',
                                    'text-zinc-600 dark:text-zinc-500 border-zinc-300 dark:border-zinc-700': column.id === 'archived'
                                }">
                                    {{ column.id === 'archived' ? 'ARCH' : (note.status === 'done' ? 'DONE' : note.priority) }}
                                </span>
                            </div>
                            <div class="mt-1 flex flex-col gap-0.5 items-start">
                                <span v-if="getChecklistProgress(note)" class="text-[9px] text-emerald-600 dark:text-emerald-400/90 font-mono" :class="{ 'text-emerald-500': getChecklistProgress(note).done === getChecklistProgress(note).total }">
                                    ✓ {{ getChecklistProgress(note).done }}/{{ getChecklistProgress(note).total }}
                                </span>
                                <span v-if="getAppointments(note).length > 0" class="text-[9px] text-violet-600 dark:text-violet-400/90 font-mono">Termine: {{ getAppointments(note).length }}</span>
                                <span v-if="note.repeat_rule" class="text-[9px] text-sky-600 dark:text-sky-400/90 font-mono" :title="'Wiederholt ' + ({daily: 'täglich', weekly: 'wöchentlich', monthly: 'monatlich'}[note.repeat_rule] || note.repeat_rule)">↻ {{ {daily: 'täglich', weekly: 'wöchentlich', monthly: 'monatlich'}[note.repeat_rule] || note.repeat_rule }}</span>
                                <div v-if="getDepartments(note).length > 0" class="flex flex-wrap gap-1">
                                    <span 
                                        v-for="(dept, di) in getDepartments(note)" 
                                        :key="di"
                                        class="dept-tag dept-tag-sm">
                                        {{ dept }}
                                    </span>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            </div>
            </div>
            </template>

            <!-- CALENDAR VIEW -->
            <template v-if="activeView === 'calendar'">
                <div class="flex-1 flex flex-col overflow-hidden">
                    <!-- Calendar Header Controls -->
                    <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 px-3 py-2 flex items-center justify-between mb-1.5 shrink-0">
                        <div class="flex items-center gap-2">
                            <button @click="calNavigate(-1)" class="text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:text-zinc-100 px-2 py-1 text-xs border border-zinc-300 dark:border-zinc-700 bg-zinc-200 dark:bg-zinc-800 cursor-pointer">◀</button>
                            <button @click="calToday" class="text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:text-zinc-100 px-2 py-1 text-[10px] border border-zinc-300 dark:border-zinc-700 bg-zinc-200 dark:bg-zinc-800 cursor-pointer font-bold">HEUTE</button>
                            <button @click="calNavigate(1)" class="text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:text-zinc-100 px-2 py-1 text-xs border border-zinc-300 dark:border-zinc-700 bg-zinc-200 dark:bg-zinc-800 cursor-pointer">▶</button>
                            <span class="text-sm font-bold text-zinc-800 dark:text-zinc-200 ml-2">{{ calTitle }}</span>
                        </div>
                        <div class="flex gap-1">
                            <button 
                                @click="calViewMode = 'week'"
                                :class="calViewMode === 'week' ? 'bg-zinc-300 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100' : 'bg-zinc-200 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-500 hover:text-zinc-700 dark:text-zinc-300'"
                                class="px-2 py-0.5 text-[10px] font-bold border border-zinc-300 dark:border-zinc-700 cursor-pointer">Woche</button>
                            <button 
                                @click="calViewMode = 'month'"
                                :class="calViewMode === 'month' ? 'bg-zinc-300 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100' : 'bg-zinc-200 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-500 hover:text-zinc-700 dark:text-zinc-300'"
                                class="px-2 py-0.5 text-[10px] font-bold border border-zinc-300 dark:border-zinc-700 cursor-pointer">Monat</button>
                            <button 
                                @click="calViewMode = 'year'"
                                :class="calViewMode === 'year' ? 'bg-zinc-300 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100' : 'bg-zinc-200 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-500 hover:text-zinc-700 dark:text-zinc-300'"
                                class="px-2 py-0.5 text-[10px] font-bold border border-zinc-300 dark:border-zinc-700 cursor-pointer">Jahr</button>
                        </div>
                    </div>

                    <!-- Month View -->
                    <div v-if="calViewMode === 'month'" class="flex-1 overflow-auto">
                        <div class="cal-grid">
                            <div v-for="d in ['Mo','Di','Mi','Do','Fr','Sa','So']" :key="d" class="cal-header">{{ d }}</div>
                            <div 
                                v-for="(day, idx) in calMonthDays" 
                                :key="idx"
                                class="cal-day"
                                :class="{ 'bg-zinc-100/50 dark:bg-zinc-900/50': day.currentMonth, 'bg-zinc-100 dark:bg-zinc-950': !day.currentMonth }"
                                @click="calDayClick(day)"
                                @dragover.prevent
                                @drop.stop="calDayDrop(day)">
                                <div class="cal-day-num" :class="{ 'today': day.isToday, 'other-month': !day.currentMonth }">{{ day.date.getDate() }}</div>
                                <div class="flex flex-wrap">
                                    <span v-for="(dot, di) in day.dots.slice(0, 4)" :key="di" class="cal-dot" :class="dot.cls"></span>
                                </div>
                                <div class="mt-0.5 space-y-0.5 max-h-16 overflow-hidden">
                                    <div v-for="(ev, ei) in day.events.slice(0, 3)" :key="ei" class="cal-event-item cursor-pointer" :class="ev.cls" :title="ev.title" draggable="true" @dragstart="calEventDragStart(ev, $event)" @click.stop="openModal(notesById[ev.noteId])">
                                        <span class="truncate">{{ ev.title }}</span>
                                        <span v-if="ev.departments && ev.departments.length" class="flex flex-wrap gap-0.5 mt-0.5">
                                            <span v-for="(dept, di) in ev.departments" :key="di" class="dept-tag dept-tag-sm">{{ dept }}</span>
                                        </span>
                                    </div>
                                    <div v-if="day.events.length > 3" class="text-[8px] text-zinc-600 dark:text-zinc-500 px-1">+{{ day.events.length - 3 }} weitere</div>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Week View -->
                    <div v-if="calViewMode === 'week'" ref="calWeekScrollRef" class="flex-1 overflow-auto" @vue:mounted="scrollWeekToNow">
                        <div class="flex" style="--pix-per-hour:36px">
                            <div class="shrink-0" style="width:32px">
                                <div class="cal-week-header" style="border-bottom:0">&nbsp;<br>&nbsp;</div>
                                <div class="relative" style="height:864px;margin-top:2px">
                                    <div v-for="h in calHours.slice(0, 24)" :key="h" class="cal-timeline-time" :style="{ top: (h * 36) + 'px' }">{{ h }}:00</div>
                                </div>
                            </div>
                            <div class="cal-week-grid flex-1">
                                <div 
                                    v-for="(day, idx) in calWeekDays" 
                                    :key="idx"
                                    class="cal-week-col flex flex-col" style="min-height:0"
                                    @click="calDayClick(day)">
                                    <div class="cal-week-header" :class="{ 'today': day.isToday }">
                                        {{ day.names }}<br>{{ day.date.getDate() }}.
                                    </div>
                                    <div class="cal-timeline-lane flex-1 relative" style="height:864px">
                                        <div v-for="h in calHours.slice(0, 23)" :key="'h'+h" class="cal-timeline-hour" :style="{ top: h * 36 + 'px' }"></div>
                                        <div v-if="day.allday.length" class="absolute top-0 left-0 right-0 bg-zinc-50/90 dark:bg-zinc-900/90 border-b border-zinc-200 dark:border-zinc-800 px-0.5 py-0.5" style="z-index:3">
                                            <div v-for="(ev, ei) in day.allday" :key="'a'+ei" class="cal-event-item" :class="ev.cls" :title="ev.title" @click.stop="openModal(notesById[ev.noteId])">
                                                {{ ev.title }}
                                            </div>
                                        </div>
                                        <div 
                                            v-for="(b, bi) in day.blocks" 
                                            :key="'b'+bi"
                                            class="cal-timeline-block" :class="b.cls" :title="b.title"
                                            :style="{ top: (b.startMin / 60) * 36 + 'px', height: (b.heightMin / 60) * 36 + 'px', left: 'calc(' + b.leftPct + '% + 1px)', width: 'calc(' + b.widthPct + '% - 1px)' }"
                                            @click.stop="openModal(notesById[b.noteId])">
                                            <span v-if="b.time" class="font-bold">{{ b.time }}</span>
                                            <span class="block leading-tight">{{ b.aptTitle }}</span>
                                            <span class="block text-zinc-500 dark:text-zinc-400 leading-tight">{{ b.noteTitle }}</span>
                                            <span v-if="b.departments && b.departments.length" class="flex flex-wrap gap-0.5 mt-0.5">
                                                <span v-for="(dept, di) in b.departments" :key="di" class="dept-tag dept-tag-sm">{{ dept }}</span>
                                            </span>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Year View -->
                    <div v-if="calViewMode === 'year'" class="flex-1 overflow-auto p-2">
                        <div class="grid grid-cols-3 md:grid-cols-4 gap-3">
                            <div v-for="(month, mi) in calYearMonths" :key="mi" class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2 overflow-hidden">
                                <div class="text-xs font-bold text-zinc-700 dark:text-zinc-300 mb-1.5">{{ month.name }}</div>
                                <div class="cal-grid" style="gap:0">
                                    <div v-for="d in ['M','D','M','D','F','S','S']" :key="d" class="text-[8px] text-zinc-400 dark:text-zinc-600 text-center">{{ d }}</div>
                                    <div 
                                        v-for="(day, idx) in month.days" 
                                        :key="idx"
                                        class="text-center py-0.5 text-[9px] relative cursor-pointer hover:bg-zinc-200 dark:bg-zinc-800"
                                        @click="calDayClick(day)">
                                        <span 
                                            class="inline-block px-0.5"
                                            :class="day.frameCls + ' ' + (day.isToday ? 'text-emerald-600 dark:text-emerald-400 font-bold' : (!day.currentMonth ? 'text-zinc-400 dark:text-zinc-600' : 'text-zinc-700 dark:text-zinc-300'))">
                                            {{ day.date.getDate() }}
                                        </span>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Selected Day Detail Panel -->
                    <div v-if="calSelectedDay" class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 px-3 py-2 mt-1.5 shrink-0 max-h-40 overflow-y-auto">
                        <div class="flex justify-between items-center mb-1.5">
                            <span class="text-xs font-bold text-zinc-700 dark:text-zinc-300">{{ calSelectedDayTitle }}</span>
                            <button @click="calSelectedDay = null" class="text-zinc-600 dark:text-zinc-500 hover:text-zinc-700 dark:text-zinc-300 text-[10px] cursor-pointer">[x]</button>
                        </div>
                        <div v-if="calSelectedDayEvents.length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic">Keine Termine an diesem Tag.</div>
                        <div class="space-y-1">
                            <div v-for="(ev, ei) in calSelectedDayEvents" :key="ei" class="apt-item cursor-pointer hover:border-zinc-400 dark:hover:border-zinc-500" :class="ev.cls" @click="openModal(notesById[ev.noteId])">
                                <span class="text-zinc-600 dark:text-zinc-400 font-mono text-[10px] shrink-0 whitespace-nowrap">{{ fmtAptRange(ev) }}</span>
                                <span class="flex-1 min-w-0 truncate">{{ ev.title }}</span>
                                <span class="text-[9px] text-zinc-400 dark:text-zinc-600 shrink-0">{{ ev.source }}</span>
                            </div>
                        </div>
                    </div>
                </div>
            </template>

            <!-- DASHBOARD VIEW -->
            <template v-if="activeView === 'dashboard'">
                <div class="flex-1 flex flex-col overflow-hidden">
                    <div class="flex items-center justify-between mb-1.5 shrink-0">
                        <span class="text-sm font-bold tracking-widest text-zinc-700 dark:text-zinc-300 uppercase">Übersicht</span>
                    </div>
                    <div class="flex-1 overflow-y-auto space-y-3 min-h-0">
                        <!-- Today's tasks -->
                        <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                            <div class="flex items-center justify-between mb-2">
                                <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">Heutige Aufgaben</span>
                                <span v-if="dashboardTodayEvents.length" class="text-[10px] text-zinc-500 dark:text-zinc-400">{{ dashboardTodayEvents.length }} × heute</span>
                            </div>
                            <div v-if="dashboardTodayEvents.length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic">Keine Termine oder Fälligkeiten für heute.</div>
                            <div class="space-y-1">
                                <div v-for="(ev, i) in dashboardTodayEvents" :key="'t'+i"
                                    class="flex items-center gap-2 text-[11px] px-2 py-1.5 border border-zinc-200 dark:border-zinc-800 cursor-pointer hover:border-zinc-400 dark:hover:border-zinc-500"
                                    :class="ev.cls === 'cal-event-overdue' ? 'bg-red-950/10' : (ev.cls === 'cal-event-apt' ? 'bg-emerald-950/10' : 'bg-blue-950/10')"
                                    @click="openModal(notesById[ev.noteId])">
                                    <span class="w-1.5 h-1.5 rounded-full shrink-0" :class="ev.cls === 'cal-event-overdue' ? 'bg-red-500' : (ev.cls === 'cal-event-apt' ? 'bg-emerald-500' : 'bg-blue-500')"></span>
                                    <span v-if="ev.time" class="font-mono text-zinc-500 dark:text-zinc-400 shrink-0">{{ ev.time }}</span>
                                    <span class="min-w-0 flex-1 truncate">{{ ev.aptTitle !== 'Fällig' ? ev.title : ('Fällig: ' + ev.noteTitle) }}</span>
                                    <span v-if="getDepartments(notesById[ev.noteId]).length" class="flex flex-wrap gap-0.5 shrink-0">
                                        <span v-for="(dept, di) in getDepartments(notesById[ev.noteId])" :key="di" class="dept-tag dept-tag-sm">{{ dept }}</span>
                                    </span>
                                </div>
                            </div>
                        </div>

                        <!-- Overdue -->
                        <div v-if="overdueNotes.length" class="bg-zinc-50 dark:bg-zinc-900 border border-red-800/40 dark:border-red-900/40 p-2.5">
                            <span class="text-[11px] font-bold text-red-600 dark:text-red-400 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">Überfällig ({{ overdueNotes.length }})</span>
                            <div class="space-y-1 mt-1.5">
                                <div v-for="(n, i) in overdueNotes" :key="'o'+i"
                                    class="flex items-center gap-2 text-[11px] px-2 py-1.5 border border-zinc-200 dark:border-zinc-800 cursor-pointer hover:border-zinc-400 dark:hover:border-zinc-500 bg-red-950/5"
                                    @click="openModal(n)">
                                    <span class="w-1.5 h-1.5 bg-red-500 rounded-full animate-pulse shrink-0"></span>
                                    <span class="min-w-0 flex-1 truncate">{{ n.title }}</span>
                                    <span class="text-red-600 dark:text-red-400/90 font-mono text-[10px] shrink-0">Fällig: {{ fmtDate(n.due_date) }}</span>
                                </div>
                            </div>
                        </div>

                        <!-- Upcoming appointments -->
                        <div v-if="dashboardUpcoming.length" class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                            <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">Kommende Termine</span>
                            <div class="space-y-1 mt-1.5">
                                <div v-for="(u, i) in dashboardUpcoming" :key="'u'+i"
                                    class="flex items-center gap-2 text-[11px] px-2 py-1.5 border border-zinc-200 dark:border-zinc-800 cursor-pointer hover:border-zinc-400 dark:hover:border-zinc-500"
                                    @click="openModal(notesById[u.noteId])">
                                    <span class="font-mono text-zinc-500 dark:text-zinc-400 shrink-0">{{ u.start === dateStr(new Date()) ? 'Heute' : fmtDate(u.start) }}<template v-if="u.time"> {{ u.time }}</template></span>
                                    <span class="min-w-0 flex-1 truncate">{{ u.title }}</span>
                                </div>
                            </div>
                        </div>

                        <!-- Stats grid -->
                        <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                            <!-- Status distribution -->
                            <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                                <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">Status</span>
                                <div class="space-y-1.5 mt-2">
                                    <div v-for="row in dashboardStatusRows" :key="row.id" class="flex items-center gap-2">
                                        <span class="w-24 shrink-0 text-[10px] text-zinc-500 dark:text-zinc-400 whitespace-nowrap truncate" :title="row.title.replace(/^\d+_/, '')">{{ row.title.replace(/^\d+_/, '') }}</span>
                                        <div class="flex-1 h-3 bg-zinc-100 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 overflow-hidden">
                                            <div class="h-full" :style="{ width: row.pct + '%' }" :class="row.color"></div>
                                        </div>
                                        <span class="w-6 shrink-0 text-[10px] text-right text-zinc-600 dark:text-zinc-400">{{ row.count }}</span>
                                    </div>
                                </div>
                            </div>

                            <!-- Priority distribution -->
                            <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                                <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">Priorität</span>
                                <div class="space-y-1.5 mt-2">
                                    <div v-for="row in dashboardPriorityRows" :key="row.label" class="flex items-center gap-2">
                                        <span class="w-16 shrink-0 text-[10px] text-zinc-500 dark:text-zinc-400 capitalize">{{ row.label }}</span>
                                        <div class="flex-1 h-3 bg-zinc-100 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 overflow-hidden">
                                            <div class="h-full" :style="{ width: row.pct + '%' }" :class="row.color"></div>
                                        </div>
                                        <span class="w-6 shrink-0 text-[10px] text-right text-zinc-600 dark:text-zinc-400">{{ row.count }}</span>
                                    </div>
                                </div>
                            </div>

                            <!-- Activity over time -->
                            <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                                <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">Fertiggestellt (letzte {{ dashboardActivityDays }} Tage)</span>
                                <div v-if="dashboardActivity.length" class="flex items-end gap-0.5 h-24 mt-2">
                                    <div v-for="(d, i) in dashboardActivity" :key="i" class="flex-1 flex flex-col items-center justify-end min-w-0" :title="d.label + ': ' + d.count">
                                        <div class="w-full max-w-[14px]" :style="{ height: d.height + 'px' }" :class="d.count ? 'bg-emerald-500' : 'bg-zinc-200 dark:bg-zinc-800'"></div>
                                    </div>
                                </div>
                                <div v-else class="text-[10px] text-zinc-400 dark:text-zinc-600 italic mt-2">Keine abgeschlossenen Notizen im Zeitraum.</div>
                            </div>

                            <!-- Todo progress -->
                            <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                                <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">To-do-Fortschritt</span>
                                <div v-if="dashboardTodo.total > 0" class="mt-2">
                                    <div class="flex items-center justify-between text-[11px] text-zinc-600 dark:text-zinc-400 mb-1">
                                        <span>{{ dashboardTodo.done }} von {{ dashboardTodo.total }} erledigt</span>
                                        <span class="font-mono">{{ dashboardTodo.pct }}%</span>
                                    </div>
                                    <div class="h-3 bg-zinc-100 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 overflow-hidden">
                                        <div class="h-full bg-emerald-500 transition-all" :style="{ width: dashboardTodo.pct + '%' }"></div>
                                    </div>
                                </div>
                                <div v-else class="text-[10px] text-zinc-400 dark:text-zinc-600 italic mt-2">Keine Checklisten-Einträge.</div>
                            </div>
                        </div>

                        <!-- Department distribution -->
                        <div v-if="dashboardDeptRows.length" class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                            <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">Aktivität nach Einrichtung</span>
                            <div class="space-y-1.5 mt-2">
                                <div v-for="row in dashboardDeptRows" :key="row.name" class="flex items-center gap-2">
                                    <span class="w-28 shrink-0 text-[10px] text-zinc-500 dark:text-zinc-400 truncate">{{ row.name }}</span>
                                    <div class="flex-1 h-3 bg-zinc-100 dark:bg-zinc-950 border border-zinc-200 dark:border-zinc-800 overflow-hidden">
                                        <div class="h-full bg-violet-500" :style="{ width: row.pct + '%' }" :title="row.name + ': ' + row.count + ' Notizen'"></div>
                                    </div>
                                    <span class="w-6 shrink-0 text-[10px] text-right text-zinc-600 dark:text-zinc-400">{{ row.count }}</span>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </template>

            <!-- ADDRESS BOOK VIEW -->
            <template v-if="activeView === 'addressbook'">
                <div class="flex-1 flex flex-col overflow-hidden">
                    <div class="flex items-center justify-between mb-1.5 shrink-0">
                        <span class="text-sm font-bold tracking-widest text-zinc-700 dark:text-zinc-300 uppercase">Adressbuch</span>
                        <div class="flex items-center gap-2">
                            <input v-model="contactFilter" type="text" placeholder="Suchen..." class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2.5 py-1 text-xs text-zinc-900 dark:text-zinc-100 focus:outline-none focus:border-emerald-500 w-48">
                            <button @click="openContactModal()" class="bg-emerald-700 hover:bg-emerald-600 text-white dark:text-zinc-100 px-3 py-1 text-xs border border-emerald-600 font-semibold cursor-pointer transition-colors">
                                + NEU
                            </button>
                        </div>
                    </div>
                    <div class="flex-1 overflow-y-auto min-h-0">
                        <div v-if="filteredContacts.length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic text-center py-4">Keine Kontakte gefunden.</div>
                        <table class="w-full text-left border-collapse">
                            <thead>
                                <tr class="text-[10px] text-zinc-500 dark:text-zinc-400 uppercase border-b border-zinc-200 dark:border-zinc-800">
                                    <th class="py-1.5 px-2 font-bold">Name</th>
                                    <th class="py-1.5 px-2 font-bold">Einrichtung</th>
                                    <th class="py-1.5 px-2 font-bold">Telefon</th>
                                    <th class="py-1.5 px-2 font-bold">E-Mail</th>
                                </tr>
                            </thead>
                            <tbody>
                                <tr v-for="c in filteredContacts" :key="c.id"
                                    class="border-b border-zinc-100 dark:border-zinc-900 cursor-pointer hover:bg-zinc-50 dark:hover:bg-zinc-900"
                                    :class="highlightContactId === c.id ? 'bg-emerald-100/70 dark:bg-emerald-900/30' : ''"
                                    @click="openContactModal(c)">
                                    <td class="py-1.5 px-2 text-xs text-zinc-800 dark:text-zinc-200">{{ c.name }}</td>
                                    <td class="py-1.5 px-2">
                                        <span v-if="c.department" class="dept-tag dept-tag-sm">{{ c.department }}</span>
                                        <span v-else class="text-zinc-400 dark:text-zinc-600 text-[10px]">–</span>
                                    </td>
                                    <td class="py-1.5 px-2 text-xs text-zinc-600 dark:text-zinc-400">{{ c.phone || '–' }}</td>
                                    <td class="py-1.5 px-2 text-xs text-zinc-600 dark:text-zinc-400">{{ c.email || '–' }}</td>
                                </tr>
                            </tbody>
                        </table>
                    </div>
                </div>
            </template>

            <!-- DATA OVERVIEW VIEW -->
            <template v-if="activeView === 'data'">
                <div class="flex-1 flex flex-col overflow-hidden">
                    <span class="text-sm font-bold tracking-widest text-zinc-700 dark:text-zinc-300 uppercase mb-1.5 shrink-0">Daten-Übersicht</span>
                    <div class="flex-1 overflow-y-auto space-y-3 min-h-0">
                        <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                            <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">Einrichtungen / Departments</span>
                            <div v-if="departments.length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic py-2">Keine Departments angelegt.</div>
                            <div class="space-y-1 mt-1.5">
                                <div v-for="d in departments" :key="d.name" class="flex items-center justify-between gap-2 px-2 py-1.5 border border-zinc-200 dark:border-zinc-800">
                                    <span class="text-[11px] text-zinc-800 dark:text-zinc-200">{{ d.name }}</span>
                                    <div class="flex items-center gap-2">
                                        <span class="text-[10px] text-zinc-500 dark:text-zinc-400">{{ d.count }} Note{{ d.count === 1 ? '' : 'n' }}</span>
                                        <button @click="deleteDepartment(d.name)" class="text-zinc-400 dark:text-zinc-600 hover:text-red-600 dark:hover:text-red-400 text-[10px] px-1 font-mono shrink-0">[X]</button>
                                    </div>
                                </div>
                            </div>
                        </div>

                        <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                            <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase border-b border-zinc-200 dark:border-zinc-800 pb-1">Kontakte (Adressbuch)</span>
                            <div v-if="contacts.length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic py-2">Keine Kontakte angelegt.</div>
                            <div class="space-y-1 mt-1.5">
                                <div v-for="c in contacts" :key="c.id" class="flex items-center justify-between gap-2 px-2 py-1.5 border border-zinc-200 dark:border-zinc-800 cursor-pointer hover:border-zinc-400 dark:hover:border-zinc-500" @click="openContactModal(c)">
                                    <div class="flex items-center gap-2 min-w-0">
                                        <span class="text-[11px] text-zinc-800 dark:text-zinc-200">{{ c.name }}</span>
                                        <span v-if="c.department" class="dept-tag dept-tag-sm">{{ c.department }}</span>
                                        <span v-if="c.phone" class="text-[10px] text-zinc-500 dark:text-zinc-400">{{ c.phone }}</span>
                                    </div>
                                    <button @click.stop="deleteContact(c)" class="text-zinc-400 dark:text-zinc-600 hover:text-red-600 dark:hover:text-red-400 text-[10px] px-1 font-mono shrink-0">[X]</button>
                                </div>
                            </div>
                        </div>

                        <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                            <div class="flex items-center justify-between border-b border-zinc-200 dark:border-zinc-800 pb-1">
                                <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase">Papierkorb</span>
                                <div class="flex items-center gap-2">
                                    <span class="text-[10px] text-zinc-500 dark:text-zinc-400">{{ trashNotes.length + trashContacts.length }} {{ trashNotes.length + trashContacts.length === 1 ? 'Element' : 'Elemente' }}</span>
                                    <button
                                        v-if="trashNotes.length + trashContacts.length > 0"
                                        @click="clearTrash"
                                        class="text-zinc-500 dark:text-zinc-400 hover:text-red-600 dark:hover:text-red-400 text-[10px] px-1 font-mono cursor-pointer">[Leeren]</button>
                                </div>
                            </div>
                            <div v-if="trashNotes.length === 0 && trashContacts.length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic py-2">Papierkorb ist leer.</div>
                            <div class="space-y-1 mt-1.5">
                                <div v-for="n in trashNotes" :key="'n' + n.id" class="flex items-center justify-between gap-2 px-2 py-1.5 border border-zinc-200 dark:border-zinc-800">
                                    <div class="flex items-center gap-2 min-w-0">
                                        <span class="text-[10px] text-zinc-500 dark:text-zinc-400 font-mono shrink-0">Note</span>
                                        <span class="text-[11px] text-zinc-800 dark:text-zinc-200 truncate">{{ n.title }}</span>
                                    </div>
                                    <div class="flex items-center gap-1.5 shrink-0">
                                        <button @click="restoreNote(n.id)" class="bg-zinc-200 dark:bg-zinc-800 hover:bg-emerald-600 text-zinc-600 dark:text-zinc-400 hover:text-white text-[10px] px-2 py-0.5 border border-zinc-300 dark:border-zinc-700 cursor-pointer">Wiederherstellen</button>
                                        <button @click="forceDeleteNote(n.id)" class="text-zinc-400 dark:text-zinc-600 hover:text-red-600 dark:hover:text-red-400 text-[10px] px-1 font-mono shrink-0" title="Endgültig löschen">[X]</button>
                                    </div>
                                </div>
                                <div v-for="c in trashContacts" :key="'c' + c.id" class="flex items-center justify-between gap-2 px-2 py-1.5 border border-zinc-200 dark:border-zinc-800">
                                    <div class="flex items-center gap-2 min-w-0">
                                        <span class="text-[10px] text-zinc-500 dark:text-zinc-400 font-mono shrink-0">Kontakt</span>
                                        <span class="text-[11px] text-zinc-800 dark:text-zinc-200 truncate">{{ c.name }}</span>
                                    </div>
                                    <div class="flex items-center gap-1.5 shrink-0">
                                        <button @click="restoreContact(c.id)" class="bg-zinc-200 dark:bg-zinc-800 hover:bg-emerald-600 text-zinc-600 dark:text-zinc-400 hover:text-white text-[10px] px-2 py-0.5 border border-zinc-300 dark:border-zinc-700 cursor-pointer">Wiederherstellen</button>
                                        <button @click="forceDeleteContact(c.id)" class="text-zinc-400 dark:text-zinc-600 hover:text-red-600 dark:hover:text-red-400 text-[10px] px-1 font-mono shrink-0" title="Endgültig löschen">[X]</button>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </template>

        </main>

        <!-- Settings modal -->
        <div v-if="isSettingsOpen" class="fixed inset-0 bg-black/80 flex items-center justify-center p-3 z-40" @click.stop="isSettingsOpen = false">
            <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 w-full max-w-md flex flex-col shadow-2xl" @click.stop>
                <div class="bg-zinc-50 dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-800 px-3 py-2 flex justify-between items-center shrink-0">
                    <span class="text-xs font-bold tracking-widest text-zinc-700 dark:text-zinc-300 uppercase">Einstellungen</span>
                    <button @click="isSettingsOpen = false" class="ml-3 text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:text-zinc-100 text-xs font-bold px-2 cursor-pointer">X</button>
                </div>
                <div class="p-3 flex flex-col gap-4 bg-zinc-100 dark:bg-zinc-950">
                    <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5 flex flex-col gap-2">
                        <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase mb-1 block border-b border-zinc-200 dark:border-zinc-800 pb-1">Auto-Archiv</span>
                        <label class="flex items-center gap-2 text-[11px] text-zinc-700 dark:text-zinc-300 cursor-pointer">
                            <input type="checkbox" v-model="autoArchiveEnabled" class="accent-emerald-600 cursor-pointer">
                            Abgeschlossene Notizen automatisch archivieren
                        </label>
                        <div class="flex items-center gap-2">
                            <span class="text-[11px] text-zinc-600 dark:text-zinc-400">Ab Tag des Monats:</span>
                            <input 
                                type="number" min="1" max="31"
                                v-model.number="autoArchiveDay"
                                :disabled="!autoArchiveEnabled"
                                class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-800 dark:text-zinc-200 w-16 focus:outline-none focus:border-emerald-500 disabled:opacity-40">
                        </div>
                        <p class="text-[9px] text-zinc-400 dark:text-zinc-600 leading-tight" v-if="autoArchiveEnabled">
                            Beim Start der App werden ab dem {{ autoArchiveDay }}. des Monats alle Notizen mit Status "Abgeschlossen" ins Archiv verschoben.
                        </p>
                    </div>
                </div>
            </div>
        </div>

        <!-- Contact modal -->
        <div v-if="contactModalOpen" class="fixed inset-0 bg-black/80 flex items-center justify-center p-3 z-40" @click.stop="contactModalOpen = false">
            <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 w-full max-w-md flex flex-col shadow-2xl" @click.stop>
                <div class="bg-zinc-50 dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-800 px-3 py-2 flex justify-between items-center shrink-0">
                    <span class="text-xs font-bold tracking-widest text-zinc-700 dark:text-zinc-300 uppercase">{{ contactForm.id ? 'Kontakt bearbeiten' : 'Neuer Kontakt' }}</span>
                    <button @click="contactModalOpen = false" class="ml-3 text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:text-zinc-100 text-xs font-bold px-2 cursor-pointer">X</button>
                </div>
                <div class="p-3 flex flex-col gap-3 bg-zinc-100 dark:bg-zinc-950 overflow-y-auto">
                    <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">
                        Name
                        <input v-model="contactForm.name" type="text" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-900 dark:text-zinc-100 focus:outline-none focus:border-emerald-500">
                    </label>
                    <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">
                        Einrichtung (Department)
                        <input v-model="contactForm.department" type="text" list="contact-dept-list" placeholder="Auswählen oder eingeben..."
                            class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-900 dark:text-zinc-100 focus:outline-none focus:border-emerald-500">
                        <datalist id="contact-dept-list">
                            <option v-for="d in departments" :key="d.name" :value="d.name"></option>
                        </datalist>
                    </label>
                    <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">
                        Telefon
                        <input v-model="contactForm.phone" type="text" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-900 dark:text-zinc-100 focus:outline-none focus:border-emerald-500">
                    </label>
                    <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">
                        E-Mail
                        <input v-model="contactForm.email" type="email" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-900 dark:text-zinc-100 focus:outline-none focus:border-emerald-500">
                    </label>
                    <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">
                        Beschreibung
                        <textarea v-model="contactForm.description" rows="3" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-900 dark:text-zinc-100 focus:outline-none focus:border-emerald-500 resize-y"></textarea>
                    </label>
                    <div class="flex justify-end gap-2 mt-1">
                        <button @click="contactModalOpen = false" class="px-3 py-1 text-xs border border-zinc-300 dark:border-zinc-700 text-zinc-600 dark:text-zinc-400 bg-zinc-50 dark:bg-zinc-900 cursor-pointer">Abbrechen</button>
                        <button @click="saveContact" :disabled="!contactForm.name.trim()"
                            :class="contactForm.name.trim() ? 'bg-emerald-700 hover:bg-emerald-600 cursor-pointer' : 'opacity-40 cursor-not-allowed'"
                            class="px-3 py-1 text-xs border border-emerald-600 text-white dark:text-zinc-100 font-semibold">Speichern</button>
                    </div>
                </div>
            </div>
        </div>

        <!-- Overdue appointment reminder (floating panel, z-40) -->
        <div v-if="reminderOpen" class="fixed bottom-3 right-3 w-full max-w-sm bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 flex flex-col shadow-2xl z-40">
            <div class="bg-zinc-50 dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-800 px-3 py-2 flex justify-between items-center shrink-0">
                <span class="text-xs font-bold tracking-widest text-red-600 dark:text-red-400 uppercase">Abgelaufene Termine</span>
                <button @click="reminderOpen = false" class="ml-3 text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:text-zinc-100 text-xs font-bold px-2 cursor-pointer">[x]</button>
            </div>
            <div class="p-3 flex flex-col gap-1.5 bg-zinc-100 dark:bg-zinc-950 max-h-60 overflow-y-auto">
                <p class="text-[10px] text-zinc-600 dark:text-zinc-400">
                    {{ overdueReminders.length }} {{ overdueReminders.length === 1 ? 'Termin/Fälligkeit ist' : 'Termine/Fälligkeiten sind' }} bereits vorbei.
                </p>
                <div v-for="(r, ri) in overdueReminders" :key="ri" class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-1.5 flex items-center gap-1.5">
                    <span class="flex-1 min-w-0">
                        <span class="block text-[10px] text-zinc-800 dark:text-zinc-200 truncate">{{ r.kind === 'apt' ? (r.note.title + ' – ' + (r.apt.title || 'Termin')) : ('Fällig: ' + r.note.title) }}</span>
                        <span class="block text-zinc-600 dark:text-zinc-400 font-mono text-[9px] whitespace-nowrap">{{ r.range }}</span>
                    </span>
                    <button @click="openNoteFromReminder(ri)" class="bg-zinc-200 dark:bg-zinc-800 hover:bg-emerald-600 text-zinc-700 dark:text-zinc-300 hover:text-white px-2 py-0.5 text-[9px] border border-zinc-300 dark:border-zinc-700 font-bold cursor-pointer shrink-0">Öffnen</button>
                </div>
            </div>
        </div>

        <!-- New note form -->
        <div v-if="isNewNoteOpen" class="fixed inset-0 bg-black/80 flex items-center justify-center p-3 z-40" @keydown.enter="handleNewNoteEnter">
            <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 w-full max-w-5xl max-h-[92vh] flex flex-col shadow-2xl" @click.stop>
                <div class="bg-zinc-50 dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-800 px-3 py-2 flex justify-between items-center shrink-0">
                    <span class="text-xs font-bold tracking-widest text-emerald-600 dark:text-emerald-400 uppercase">Neue Notiz</span>
                    <button @click="closeNewNote" class="ml-3 text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:text-zinc-100 text-xs font-bold px-2 cursor-pointer">X</button>
                </div>
                <div class="p-3 flex flex-col gap-3 bg-zinc-100 dark:bg-zinc-950 overflow-y-auto">
                    <div class="grid grid-cols-1 md:grid-cols-4 gap-3">
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400 md:col-span-4">
                            Titel
                            <input 
                                ref="newNoteTitleInputRef"
                                type="text" 
                                v-model="newNoteTitle" 
                                placeholder="Notiz-Titel..." 
                                class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-900 dark:text-zinc-100 focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
                            >
                        </label>
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">
                            Status
                            <select v-model="newNoteStatus" class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-800 dark:text-zinc-200 focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500">
                                <option value="backlog">Backlog</option>
                                <option value="in_progress">In Arbeit</option>
                                <option value="review">Review</option>
                                <option value="done">Abgeschlossen</option>
                            </select>
                        </label>
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">
                            Priorität
                            <select v-model="newNotePriority" class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-800 dark:text-zinc-200 focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500">
                                <option value="low">Niedrig</option>
                                <option value="medium">Mittel</option>
                                <option value="high">Hoch</option>
                            </select>
                        </label>
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">
                            Fällig
                            <input 
                                type="date" 
                                v-model="newNoteDueDate" 
                                class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-800 dark:text-zinc-200 focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
                            >
                        </label>
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400">
                            Wiederholung
                            <select v-model="newNoteRepeatRule" class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-800 dark:text-zinc-200 focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500">
                                <option value="">Keine</option>
                                <option value="daily">Täglich</option>
                                <option value="weekly">Wöchentlich</option>
                                <option value="monthly">Monatlich</option>
                            </select>
                        </label>
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-400 md:col-span-4">
                            Department
                            <div class="relative">
                                <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 px-2 py-1 flex flex-wrap gap-1 min-h-[28px] items-center focus-within:border-emerald-500 focus-within:ring-1 focus-within:ring-emerald-500">
                                    <span 
                                        v-for="(dept, di) in newNoteDepartments" 
                                        :key="di"
                                        class="dept-tag">
                                        {{ dept }}
                                        <span @click="removeNewNoteDepartment(di)" class="dept-tag-remove">✕</span>
                                    </span>
                                    <input 
                                        ref="newNoteDepartmentInputRef"
                                        type="text" 
                                        v-model="newNoteDeptInput"
                                        placeholder="Department hinzufügen..." 
                                        autocomplete="off"
                                        @focus="openDeptDropdown('new')"
                                        @input="onDeptInput"
                                        @keydown="handleDeptKeydown"
                                        @blur="closeDeptDropdown"
                                        class="bg-transparent text-xs text-zinc-900 dark:text-zinc-100 flex-1 min-w-[80px] outline-none"
                                    >
                                </div>
                                <div 
                                    v-if="deptDropdownOpen && deptDropdownSource === 'new'"
                                    class="absolute left-0 right-0 top-full mt-1 bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 shadow-2xl py-1 text-xs font-mono z-50 max-h-40 overflow-y-auto"
                                >
                                    <button 
                                        v-for="(dep, index) in filteredDepartments" 
                                        :key="dep.name"
                                        @mousedown.prevent="selectDepartment(dep.name)"
                                        class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 flex justify-between items-center gap-2 cursor-pointer"
                                        :class="{ 'bg-zinc-200 dark:bg-zinc-800': index === deptIndex }"
                                    >
                                        <span class="truncate">{{ dep.name }}</span>
                                        <span class="text-zinc-600 dark:text-zinc-500 text-[10px] shrink-0">{{ dep.count }}×</span>
                                    </button>
                                    <div v-if="filteredDepartments.length === 0 && newNoteDeptInput.trim()" class="px-3 py-1.5">
                                        <button @mousedown.prevent="addCustomDepartment('new')" class="w-full text-left text-emerald-600 dark:text-emerald-400 hover:text-emerald-600 dark:text-emerald-300 cursor-pointer text-[11px]">+ "{{ newNoteDeptInput.trim() }}" erstellen</button>
                                    </div>
                                    <div v-if="filteredDepartments.length === 0 && !newNoteDeptInput.trim()" class="px-3 py-2 text-zinc-600 dark:text-zinc-500">
                                        Keine Departments vorhanden.
                                    </div>
                                </div>
                            </div>
                        </label>
                    </div>

                    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                        <div class="flex flex-col gap-1 relative">
                            <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5 flex flex-col">
                                <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase mb-2 block border-b border-zinc-200 dark:border-zinc-800 pb-1">Inhalt (Markdown)</span>
                                <textarea 
                                ref="newNoteTextareaRef"
                                v-model="newNoteContent" 
                                placeholder="Inhalt schreiben... ([[ Für Notiz-Links)" 
                                @input="handleAutocomplete"
                                @keyup="updateAutocompletePos"
                                @click="updateAutocompletePos"
                                @keydown="handleAutocompleteKeydown"
                                class="w-full h-40 bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-900 dark:text-zinc-100 font-mono focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500 resize-none"
                                ></textarea>
                                <div ref="newNoteCaretMirrorRef" class="absolute invisible whitespace-pre break-all" style="font-family:'Courier New',Courier,Lucida Console,Monaco,monospace; font-size:12px; line-height:16px; padding:10px; border:1px solid transparent; left:0; top:0; z-index:-1; pointer-events:none;"></div>
                                <div 
                                v-if="showAutocomplete"
                                :style="{ top: autocompletePos.y + 'px', left: autocompletePos.x + 'px' }"
                                class="autocomplete-dropdown bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 shadow-2xl py-1 text-xs font-mono overflow-y-auto max-h-56"
                            >
                                <div class="px-2.5 py-1 text-[10px] text-zinc-600 dark:text-zinc-500 uppercase tracking-wider border-b border-zinc-200 dark:border-zinc-800 mb-1 flex justify-between items-center">
                                    <span>Notiz verlinken</span>
                                    <span class="text-zinc-400 dark:text-zinc-600">↑↓ Enter Esc</span>
                                </div>
                                <button 
                                    v-for="(result, index) in autocompleteResults" 
                                    :key="result.id"
                                    @click="selectAutocomplete(index)"
                                    @mousedown.prevent
                                    class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 flex justify-between items-center gap-2"
                                    :class="{ 'bg-zinc-200 dark:bg-zinc-800': index === autocompleteIndex }"
                                >
                                    <span class="truncate">{{ result.title }}</span>
                                    <span class="text-zinc-600 dark:text-zinc-500 text-[10px] shrink-0">#{{ result.id }}</span>
                                </button>
<div v-if="autocompleteResults.length === 0" class="px-3 py-2 text-zinc-600 dark:text-zinc-500">
                                    Keine Treffer.
                                </div>
                            </div>
                            </div>
                        </div>
                        <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5 flex flex-col">
                            <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase mb-2 border-b border-zinc-200 dark:border-zinc-800 pb-1">Checklist / Steps</span>
                            <div class="flex gap-1 mb-2">
                                <input 
                                ref="newNoteStepInputRef"
                                    type="text" 
                                    v-model="newNoteStepText" 
                                    @keyup.enter="addNewNoteStep"
                                    placeholder="Neuer Step..." 
                                    class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-900 dark:text-zinc-100 w-full focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
                                >
                                <button @click="addNewNoteStep" class="bg-zinc-200 dark:bg-zinc-800 hover:bg-zinc-300 dark:bg-zinc-700 text-zinc-800 dark:text-zinc-200 px-2 text-[11px] border border-zinc-300 dark:border-zinc-700 cursor-pointer shrink-0">+</button>
                            </div>
                            <div class="overflow-y-auto space-y-1.5 pr-1 h-28 shrink-0">
                                <div v-if="newNoteChecklist.length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic text-center py-2">Keine Zwischensteps.</div>
                                <div v-for="(step, index) in newNoteChecklist" :key="index" class="flex items-center justify-between bg-zinc-100 dark:bg-zinc-950 border border-zinc-200/80 dark:border-zinc-800/80 px-2 py-1 gap-2">
                                    <span class="text-[11px] text-zinc-700 dark:text-zinc-300 break-all">{{ step.text }}</span>
                                    <button @click="removeNewNoteStep(index)" class="text-zinc-400 dark:text-zinc-600 hover:text-red-600 dark:text-red-400 text-[10px] shrink-0 font-mono">[x]</button>
                                </div>
                            </div>
                        </div>
                    </div>

                    <!-- Appointments -->
                    <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5">
                        <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase mb-2 block border-b border-zinc-200 dark:border-zinc-800 pb-1">Termine</span>
                        <div class="grid grid-cols-1 md:grid-cols-4 gap-2 mb-2">
                            <input type="text" v-model="newAptTitle" placeholder="Termin..." @keyup.enter="addNewAppointment" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-900 dark:text-zinc-100 focus:outline-none focus:border-emerald-500 md:col-span-2">
                            <input type="date" v-model="newAptStart" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-800 dark:text-zinc-200 focus:outline-none focus:border-emerald-500">
                            <input type="text" inputmode="numeric" v-model="newAptTime" placeholder="14:30" pattern="([01][0-9]|2[0-3]):[0-5][0-9]" title="Uhrzeit im 24h-Format (z.B. 14:30)" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-800 dark:text-zinc-200 focus:outline-none focus:border-emerald-500">
                        </div>
                        <div class="flex gap-2 mb-2">
                            <label class="flex items-center gap-1 text-[10px] text-zinc-600 dark:text-zinc-500">
                                <input type="checkbox" v-model="newAptHasEnd" class="accent-emerald-600"> Bis-Datum
                            </label>
                            <input v-if="newAptHasEnd" type="date" v-model="newAptEnd" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-800 dark:text-zinc-200 focus:outline-none focus:border-emerald-500">
                            <input v-if="newAptHasEnd" type="text" inputmode="numeric" v-model="newAptEndTime" placeholder="16:00" pattern="([01][0-9]|2[0-3]):[0-5][0-9]" title="Endzeit im 24h-Format (z.B. 16:00)" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-1.5 py-1 text-[11px] text-zinc-800 dark:text-zinc-200 focus:outline-none focus:border-emerald-500">
                            <button @click="addNewAppointment" :disabled="!newAptTitle.trim() || !newAptStart" class="ml-auto bg-zinc-200 dark:bg-zinc-800 hover:bg-zinc-300 dark:bg-zinc-700 text-zinc-800 dark:text-zinc-200 px-3 text-[11px] border border-zinc-300 dark:border-zinc-700 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed">+ Hinzufügen</button>
                        </div>
                        <div v-if="newNoteAppointments.length > 0" class="space-y-1 max-h-24 overflow-y-auto">
                            <div v-for="(apt, ai) in newNoteAppointments" :key="ai" class="apt-item min-w-0 flex-wrap">
                                <span class="text-zinc-600 dark:text-zinc-400 font-mono text-[10px] shrink-0 whitespace-nowrap">{{ fmtDate(apt.start) }}{{ apt.time ? ' ' + apt.time : '' }}{{ apt.end ? ' – ' + fmtDate(apt.end) + (apt.endTime ? ' ' + apt.endTime : '') : '' }}</span>
                                <span class="flex-1 min-w-0 truncate text-zinc-800 dark:text-zinc-200">{{ apt.title }}</span>
                                <button @click="newNoteAppointments.splice(ai, 1)" class="text-zinc-400 dark:text-zinc-600 hover:text-red-600 dark:text-red-400 text-[10px] font-mono shrink-0">[x]</button>
                            </div>
                        </div>
                        <div v-else class="text-[10px] text-zinc-400 dark:text-zinc-600 italic">Keine Termine.</div>
                    </div>

                </div>
                <div class="bg-zinc-50 dark:bg-zinc-900 border-t border-zinc-200 dark:border-zinc-800 px-3 py-2 flex justify-end gap-2">
                    <button 
                        @click="createNote" 
                        class="bg-emerald-700 hover:bg-emerald-600 text-white dark:text-zinc-100 px-3 py-1 text-[11px] font-semibold border border-emerald-600 cursor-pointer">
                        ERSTELLEN & SCHLIESSEN
                    </button>
                </div>
            </div>
        </div>

        <!-- Detail Modal -->
        <div v-if="isModalOpen" class="fixed inset-0 bg-black/80 flex items-center justify-center p-3 z-50">
            <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 w-full max-w-5xl flex flex-col h-[88vh] shadow-2xl" @click.stop>
                <div class="bg-zinc-50 dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-800 px-3 py-2 flex justify-between items-center">
                    <input 
                        type="text" 
                        v-model="activeNote.title" 
                        class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-xs text-zinc-900 dark:text-zinc-100 w-full font-bold focus:outline-none focus:border-emerald-500"
                    >
                    <button @click="closeModal" class="ml-3 text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:text-zinc-100 text-xs font-bold px-2 cursor-pointer">X</button>
                </div>

                <div class="bg-zinc-100/50 dark:bg-zinc-900/50 border-b border-zinc-200 dark:border-zinc-800 px-3 py-1.5 flex justify-between items-center text-[11px]">
                    <div class="flex gap-3 items-center flex-wrap">
                        <label class="flex items-center gap-1 text-zinc-600 dark:text-zinc-400">
                            Status:
                            <select v-model="activeNote.status" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 text-zinc-800 dark:text-zinc-200 px-1 py-0.5 focus:outline-none">
                                <option value="backlog">Backlog</option>
                                <option value="in_progress">In Arbeit</option>
                                <option value="review">Review</option>
                                <option value="done">Abgeschlossen</option>
                                <option value="archived">Archiv</option>
                            </select>
                        </label>
                        <label class="flex items-center gap-1 text-zinc-600 dark:text-zinc-400">
                            Prio:
                            <select v-model="activeNote.priority" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 text-zinc-800 dark:text-zinc-200 px-1 py-0.5 focus:outline-none">
                                <option value="low">Niedrig</option>
                                <option value="medium">Mittel</option>
                                <option value="high">Hoch</option>
                            </select>
                        </label>
                        <label class="flex items-center gap-1 text-zinc-600 dark:text-zinc-400">
                            Fällig:
                            <input type="date" v-model="activeNote.due_date" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 text-zinc-800 dark:text-zinc-200 px-1 py-0.5 focus:outline-none">
                        </label>
                        <label class="flex items-center gap-1 text-zinc-600 dark:text-zinc-400">
                            Wiederholung:
                            <select v-model="activeNote.repeat_rule" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 text-zinc-800 dark:text-zinc-200 px-1 py-0.5 focus:outline-none">
                                <option value="">Keine</option>
                                <option value="daily">Täglich</option>
                                <option value="weekly">Wöchentlich</option>
                                <option value="monthly">Monatlich</option>
                            </select>
                        </label>
                        <label class="flex items-center gap-1 text-zinc-600 dark:text-zinc-400 relative">
                            Department:
                            <div class="relative">
                                <div class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-1 py-0.5 flex flex-wrap gap-1 min-h-[22px] items-center focus-within:border-emerald-500 w-48">
                                    <span 
                                        v-for="(dept, di) in activeNoteDepartments" 
                                        :key="di"
                                        class="dept-tag dept-tag-sm">
                                        {{ dept }}
                                        <span @click="removeActiveNoteDepartment(di)" class="dept-tag-remove text-[8px]">✕</span>
                                    </span>
                                    <input 
                                        ref="modalDepartmentInputRef"
                                        type="text" 
                                        v-model="modalDeptInput"
                                        placeholder="Hinzufügen..." 
                                        autocomplete="off"
                                        @focus="openDeptDropdown('modal')"
                                        @input="onDeptInput"
                                        @keydown="handleDeptKeydown"
                                        @blur="closeDeptDropdown"
                                        class="bg-transparent text-[11px] text-zinc-800 dark:text-zinc-200 flex-1 min-w-[60px] outline-none"
                                    >
                                </div>
                                <div 
                                    v-if="deptDropdownOpen && deptDropdownSource === 'modal'"
                                    class="absolute left-0 top-full mt-1 bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 shadow-2xl py-1 text-xs font-mono z-50 max-h-40 overflow-y-auto w-48"
                                >
                                    <button 
                                        v-for="(dep, index) in filteredDepartments" 
                                        :key="dep.name"
                                        @mousedown.prevent="selectDepartment(dep.name)"
                                        class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 flex justify-between items-center gap-2 cursor-pointer"
                                        :class="{ 'bg-zinc-200 dark:bg-zinc-800': index === deptIndex }"
                                    >
                                        <span class="truncate">{{ dep.name }}</span>
                                        <span class="text-zinc-600 dark:text-zinc-500 text-[10px] shrink-0">{{ dep.count }}×</span>
                                    </button>
                                    <div v-if="filteredDepartments.length === 0 && modalDeptInput.trim()" class="px-3 py-1.5">
                                        <button @mousedown.prevent="addCustomDepartment('modal')" class="w-full text-left text-emerald-600 dark:text-emerald-400 hover:text-emerald-600 dark:text-emerald-300 cursor-pointer text-[11px]">+ "{{ modalDeptInput.trim() }}" erstellen</button>
                                    </div>
                                    <div v-if="filteredDepartments.length === 0 && !modalDeptInput.trim()" class="px-3 py-2 text-zinc-600 dark:text-zinc-500">
                                        Keine Departments vorhanden.
                                    </div>
                                </div>
                            </div>
                        </label>
                    </div>
                    <div>
                        <button 
                            @click="isPreviewMode = !isPreviewMode" 
                            class="bg-zinc-200 dark:bg-zinc-800 border border-zinc-300 dark:border-zinc-700 hover:bg-zinc-300 dark:bg-zinc-700 text-zinc-800 dark:text-zinc-200 px-2.5 py-0.5 font-semibold transition-colors text-[11px] cursor-pointer">
                            {{ isPreviewMode ? 'Bearbeiten' : 'Vorschau' }}
                        </button>
                    </div>
                </div>

                <!-- Content & checklist area -->
                <div class="flex-1 grid grid-cols-1 md:grid-cols-3 p-3 gap-3 overflow-hidden bg-zinc-100 dark:bg-zinc-950 relative">
                    <div class="md:col-span-2 h-full flex flex-col relative">
                        <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5 flex flex-col flex-1 min-h-0">
                            <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase mb-2 block border-b border-zinc-200 dark:border-zinc-800 pb-1">Inhalt (Markdown)</span>
                            <textarea 
                                ref="textareaRef"
                                v-if="!isPreviewMode"
                                v-model="activeNote.content" 
                                @contextmenu.prevent="openContextMenu"
                                @input="handleAutocomplete"
                                @keyup="updateAutocompletePos"
                                @click="updateAutocompletePos"
                                @keydown="handleAutocompleteKeydown"
                                placeholder="Inhalt mit Markdown schreiben... (Rechtsklick für Text-Formatierung)"
                                class="w-full flex-1 min-h-0 bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 text-zinc-800 dark:text-zinc-200 p-2.5 text-xs font-mono resize-none focus:outline-none focus:border-zinc-400 dark:border-zinc-600"
                            ></textarea>
                            <!-- Invisible caret mirror for computing cursor coordinates -->
                            <div ref="caretMirrorRef" class="absolute invisible whitespace-pre break-all" style="font-family:'Courier New',Courier,Lucida Console,Monaco,monospace; font-size:12px; line-height:16px; padding:10px; border:1px solid transparent; left:0; top:0; z-index:-1; pointer-events:none;"></div>
                            <div 
                                 v-if="isPreviewMode" 
                                 class="markdown-body w-full flex-1 min-h-0 bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 p-2.5 text-zinc-800 dark:text-zinc-200 overflow-y-auto text-xs"
                                 v-html="renderedMarkdown"
                                 @click.prevent="onPreviewClick"
                                 @mousemove="showContactPopover"
                                 @mouseleave="hideContactPopover">
                            </div>

                        <!-- Autocomplete dropdown for [[ links -->
                        <div 
                            v-if="showAutocomplete"
                            :style="{ top: autocompletePos.y + 'px', left: autocompletePos.x + 'px' }"
                            class="autocomplete-dropdown bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 shadow-2xl py-1 text-xs font-mono overflow-y-auto max-h-56"
                        >
                            <div class="px-2.5 py-1 text-[10px] text-zinc-600 dark:text-zinc-500 uppercase tracking-wider border-b border-zinc-200 dark:border-zinc-800 mb-1 flex justify-between items-center">
                                <span>Notiz verlinken</span>
                                <span class="text-zinc-400 dark:text-zinc-600">↑↓ Enter Esc</span>
                            </div>
                            <button 
                                v-for="(result, index) in autocompleteResults" 
                                :key="result.id"
                                @click="selectAutocomplete(index)"
                                @mousedown.prevent
                                class="w-full text-left px-3 py-1.5 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 flex justify-between items-center gap-2"
                                :class="{ 'bg-zinc-200 dark:bg-zinc-800': index === autocompleteIndex }"
                            >
                                <span class="truncate">{{ result.title }}</span>
                                <span class="text-zinc-600 dark:text-zinc-500 text-[10px] shrink-0">#{{ result.id }}</span>
                            </button>
                            <div v-if="autocompleteResults.length === 0" class="px-3 py-2 text-zinc-600 dark:text-zinc-500">
                                Keine Treffer.
                            </div>
                        </div>
                        </div>

                        <!-- Context menu (right-click) -->
                        <div 
                            v-if="contextMenu.show" 
                            :style="{ top: contextMenu.y + 'px', left: contextMenu.x + 'px' }"
                            class="absolute z-50 bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 shadow-2xl py-1 text-xs w-52 font-mono select-none"
                        >
                            <div class="px-2.5 py-1 text-[10px] text-zinc-600 dark:text-zinc-500 uppercase tracking-wider border-b border-zinc-200 dark:border-zinc-800 mb-1">Markdown Format</div>
                            <button @click="applyFormat('**')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 flex justify-between"><span>Fett</span><span class="text-zinc-600 dark:text-zinc-500">**text**</span></button>
                            <button @click="applyFormat('*')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 flex justify-between"><span>Kursiv</span><span class="text-zinc-600 dark:text-zinc-500">*text*</span></button>
                            <button @click="applyFormat('~~')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 flex justify-between"><span>Durchgestrichen</span><span class="text-zinc-600 dark:text-zinc-500">~~text~~</span></button>
                            <button @click="applyFormat('`')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 flex justify-between border-b border-zinc-200 dark:border-zinc-800 pb-1.5 mb-1"><span>Code-Snippet</span><span class="text-zinc-600 dark:text-zinc-500">`text`</span></button>
                            <div class="px-2.5 py-1 text-[10px] text-zinc-600 dark:text-zinc-500 uppercase tracking-wider mb-1">Farben (HTML)</div>
                            <button @click="applyHtmlColor('#34d399')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-emerald-600 dark:text-emerald-400 flex items-center gap-2"><span class="w-2 h-2 bg-emerald-400 inline-block"></span> Smaragdgrün</button>
                            <button @click="applyHtmlColor('#f87171')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-red-600 dark:text-red-400 flex items-center gap-2"><span class="w-2 h-2 bg-red-400 inline-block"></span> Rot</button>
                            <button @click="applyHtmlColor('#fb923c')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-orange-600 dark:text-orange-400 flex items-center gap-2"><span class="w-2 h-2 bg-orange-400 inline-block"></span> Orange</button>
                            <button @click="applyHtmlColor('#facc15')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-yellow-600 dark:text-yellow-400 flex items-center gap-2"><span class="w-2 h-2 bg-yellow-400 inline-block"></span> Gelb</button>
                            <button @click="applyHtmlColor('#60a5fa')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-blue-600 dark:text-blue-400 flex items-center gap-2"><span class="w-2 h-2 bg-blue-400 inline-block"></span> Blau</button>
                            <button @click="applyHtmlColor('#a78bfa')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-violet-600 dark:text-violet-400 flex items-center gap-2"><span class="w-2 h-2 bg-violet-400 inline-block"></span> Lila</button>
                            <button @click="applyHtmlColor('#f472b6')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-pink-600 dark:text-pink-400 flex items-center gap-2"><span class="w-2 h-2 bg-pink-400 inline-block"></span> Rosa</button>
                            <button @click="applyHtmlColor('#a1a1aa')" class="w-full text-left px-3 py-1 hover:bg-zinc-200 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 flex items-center gap-2"><span class="w-2 h-2 bg-zinc-400 dark:bg-zinc-500 inline-block"></span> Grau</button>
                        </div>

                        <!-- Contact hover popover -->
                        <div 
                            v-if="contactPopover.show && contactPopover.contact"
                            :style="{ top: contactPopover.y + 'px', left: contactPopover.x + 'px' }"
                            class="absolute z-[60] bg-zinc-50 dark:bg-zinc-900 border border-zinc-300 dark:border-zinc-700 shadow-2xl py-1.5 px-2.5 text-[11px] w-56 pointer-events-none"
                        >
                            <div class="flex items-center gap-1.5 mb-1">
                                <span class="font-bold text-zinc-900 dark:text-zinc-100 break-all">{{ contactPopover.contact.name }}</span>
                                <span v-if="contactPopover.contact.department" class="dept-tag dept-tag-sm">{{ contactPopover.contact.department }}</span>
                            </div>
                            <div class="text-[9px] text-zinc-400 dark:text-zinc-600 capitalize mb-1">Kontakt</div>
                            <div class="space-y-0.5 text-zinc-700 dark:text-zinc-300">
                                <div v-if="contactPopover.contact.phone" class="flex gap-1"><span class="text-zinc-500 dark:text-zinc-400 shrink-0">Tel:</span><span class="min-w-0">{{ contactPopover.contact.phone }}</span></div>
                                <div v-if="contactPopover.contact.email" class="flex gap-1"><span class="text-zinc-500 dark:text-zinc-400 shrink-0">E-Mail:</span><span class="min-w-0 break-all">{{ contactPopover.contact.email }}</span></div>
                                <div v-if="contactPopover.contact.description" class="text-[10px] italic break-all mt-0.5">{{ contactPopover.contact.description }}</div>
                            </div>
                        </div>
                    </div>

                    <!-- Steps / checklist + appointments -->
                    <div class="flex flex-col gap-3 overflow-y-auto pr-1">
                        <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5 flex flex-col">
                            <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase mb-2 block border-b border-zinc-200 dark:border-zinc-800 pb-1">Checklist / Steps</span>
                            <div class="flex gap-1 mb-2">
                                <input 
                                    type="text" 
                                    v-model="newStepText" 
                                    @keyup.enter="addStep"
                                    placeholder="Neuer Step..." 
                                    class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-900 dark:text-zinc-100 w-full focus:outline-none focus:border-emerald-500"
                                >
                                <button @click="addStep" class="bg-zinc-200 dark:bg-zinc-800 hover:bg-zinc-300 dark:bg-zinc-700 text-zinc-800 dark:text-zinc-200 px-2 text-[11px] border border-zinc-300 dark:border-zinc-700 cursor-pointer">+</button>
                            </div>
                            <div class="space-y-1.5 pr-1 max-h-40 overflow-y-auto">
                                <div v-if="activeChecklist.length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic text-center py-4">Keine Zwischensteps vorhanden.</div>
                                <div v-for="(step, index) in activeChecklist" :key="index" class="flex items-center justify-between bg-zinc-100 dark:bg-zinc-950 border border-zinc-200/80 dark:border-zinc-800/80 px-2 py-1 gap-2">
                                    <label class="flex items-center gap-2 text-[11px] text-zinc-700 dark:text-zinc-300 cursor-pointer overflow-hidden">
                                        <input type="checkbox" v-model="step.done" class="accent-emerald-600 cursor-pointer">
                                        <span :class="{'line-through text-zinc-400 dark:text-zinc-600': step.done}" class="break-all">{{ step.text }}</span>
                                    </label>
                                    <button @click="removeStep(index)" class="text-zinc-400 dark:text-zinc-600 hover:text-red-600 dark:text-red-400 text-[10px] shrink-0 font-mono">[x]</button>
                                </div>
                            </div>
                        </div>

                        <!-- Appointments in modal -->
                        <div class="bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 p-2.5 flex flex-col">
                            <span class="text-[11px] font-bold text-zinc-700 dark:text-zinc-300 uppercase mb-2 block border-b border-zinc-200 dark:border-zinc-800 pb-1">Termine</span>
                            <div class="flex flex-col gap-1 mb-2">
                                <input type="text" v-model="modalAptTitle" placeholder="Termin..." @keyup.enter="addModalAppointment" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-900 dark:text-zinc-100 w-full focus:outline-none focus:border-emerald-500">
                                <div class="flex gap-1">
                                    <input type="date" v-model="modalAptStart" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-800 dark:text-zinc-200 flex-1 focus:outline-none focus:border-emerald-500">
                                    <input type="text" inputmode="numeric" v-model="modalAptTime" placeholder="14:30" pattern="([01][0-9]|2[0-3]):[0-5][0-9]" title="Uhrzeit im 24h-Format (z.B. 14:30)" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-1 text-[11px] text-zinc-800 dark:text-zinc-200 w-20 focus:outline-none focus:border-emerald-500">
                                </div>
                                <label class="flex items-center gap-1 text-[10px] text-zinc-600 dark:text-zinc-500">
                                    <input type="checkbox" v-model="modalAptHasEnd" class="accent-emerald-600"> Bis-Datum
                                </label>
                                <div v-if="modalAptHasEnd" class="flex gap-1">
                                    <input type="date" v-model="modalAptEnd" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-0.5 text-[11px] text-zinc-800 dark:text-zinc-200 focus:outline-none focus:border-emerald-500 flex-1">
                                    <input type="text" inputmode="numeric" v-model="modalAptEndTime" placeholder="16:00" pattern="([01][0-9]|2[0-3]):[0-5][0-9]" title="Endzeit im 24h-Format (z.B. 16:00)" class="bg-zinc-100 dark:bg-zinc-950 border border-zinc-300 dark:border-zinc-700 px-2 py-0.5 text-[11px] text-zinc-800 dark:text-zinc-200 w-20 focus:outline-none focus:border-emerald-500">
                                </div>
                                <button @click="addModalAppointment" :disabled="!modalAptTitle.trim() || !modalAptStart" class="bg-zinc-200 dark:bg-zinc-800 hover:bg-zinc-300 dark:bg-zinc-700 text-zinc-800 dark:text-zinc-200 px-2 py-1 text-[11px] border border-zinc-300 dark:border-zinc-700 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed">+ Hinzufügen</button>
                            </div>
                            <div class="space-y-1 pr-1">
                                <div v-if="activeNoteAppointments.length === 0" class="text-[10px] text-zinc-400 dark:text-zinc-600 italic text-center py-2">Keine Termine.</div>
                                <div v-for="(apt, ai) in activeNoteAppointments" :key="ai" class="apt-item min-w-0 flex-wrap">
                                <span class="text-zinc-600 dark:text-zinc-400 font-mono text-[10px] shrink-0 whitespace-nowrap">{{ fmtDate(apt.start) }}{{ apt.time ? ' ' + apt.time : '' }}{{ apt.end ? ' – ' + fmtDate(apt.end) + (apt.endTime ? ' ' + apt.endTime : '') : '' }}</span>
                                    <span class="flex-1 min-w-0 truncate text-zinc-800 dark:text-zinc-200">{{ apt.title }}</span>
                                    <button @click="activeNoteAppointments.splice(ai, 1)" class="text-zinc-400 dark:text-zinc-600 hover:text-red-600 dark:text-red-400 text-[10px] font-mono shrink-0">[x]</button>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>

                <div class="bg-zinc-50 dark:bg-zinc-900 border-t border-zinc-200 dark:border-zinc-800 px-3 py-2 flex justify-end gap-2">
                    <button 
                        @click="saveActiveNote" 
                        class="bg-emerald-700 hover:bg-emerald-600 text-white dark:text-zinc-100 px-3 py-1 text-[11px] font-semibold border border-emerald-600 cursor-pointer">
                        SPEICHERN & SCHLIESSEN
                    </button>
                </div>
            </div>
        </div>
    </div>

    <script>
        const { createApp, ref, computed, watch, nextTick, onMounted, onUnmounted } = Vue

        createApp({
            setup() {
                const newNoteTitle = ref('')
                const newNotePriority = ref('medium')
                const newNoteDueDate = ref('')
                const newNoteRepeatRule = ref('')
                const newNoteStatus = ref('backlog')
                const newNoteContent = ref('')
                const newNoteChecklist = ref([])
                const newNoteStepText = ref('')
                const newNoteDepartment = ref('')
                const newNoteDepartments = ref([])
                const newNoteDeptInput = ref('')
                const newNoteAppointments = ref([])
                const newAptTitle = ref('')
                const newAptStart = ref('')
                const newAptEnd = ref('')
                const newAptTime = ref('')
                const newAptEndTime = ref('')
                const newAptHasEnd = ref(false)
                const isNewNoteOpen = ref(false)
                const newNoteTitleInputRef = ref(null)
                const newNoteTextareaRef = ref(null)
                const newNoteCaretMirrorRef = ref(null)
                const newNoteStepInputRef = ref(null)
                const draggedNote = ref(null)
                const isBacklogCollapsed = ref(true)
                const isInProgressCollapsed = ref(false)
                const isReviewCollapsed = ref(false)
                const isDoneCollapsed = ref(false)
                const isArchiveCollapsed = ref(true)
                const searchQuery = ref('')
                const searchInputRef = ref(null)
                const textareaRef = ref(null)
                const caretMirrorRef = ref(null)

                const isModalOpen = ref(false)
                const isPreviewMode = ref(false)
                const activeNote = ref(null)
                const activeChecklist = ref([])
                const newStepText = ref('')
                const activeNoteAppointments = ref([])
                const activeNoteDepartments = ref([])
                const modalDeptInput = ref('')
                const modalAptTitle = ref('')
                const modalAptStart = ref('')
                const modalAptEnd = ref('')
                const modalAptTime = ref('')
                const modalAptEndTime = ref('')
                const modalAptHasEnd = ref(false)

                // Views / navigation
                const activeView = ref(localStorage.getItem('notice-view') || 'dashboard')
                watch(activeView, (v) => localStorage.setItem('notice-view', v))
                const calViewMode = ref('week')
                const calCursor = ref(new Date())
                const calSelectedDay = ref(null)

                // Autocomplete state for [[ note links and {{ address links
                const showAutocomplete = ref(false)
                const autocompleteType = ref('note')
                const autocompleteResults = ref([])
                const autocompleteIndex = ref(0)
                const autocompleteQuery = ref('')
                const autocompleteStart = ref(0)
                const autocompletePos = ref({ x: 0, y: 0, flip: false })
                let autocompleteTimer = null
                const autocompleteContainerRef = ref(null)
                const exportMenuOpen = ref(false)
                const importFileInputRef = ref(null)
                const importMode = ref('merge')
                const trashNotes = ref([])
                const trashContacts = ref([])

                // Theme (dark/light) toggle
                const isDark = ref(localStorage.getItem('notice-theme') !== 'light')
                const toggleTheme = () => {
                    isDark.value = !isDark.value
                    document.documentElement.classList.toggle('dark', isDark.value)
                    localStorage.setItem('notice-theme', isDark.value ? 'dark' : 'light')
                }

                // Settings (auto-archive)
                const isSettingsOpen = ref(false)
                const autoArchiveEnabled = ref(localStorage.getItem('notice-autoarchive-enabled') === '1')
                const autoArchiveDay = ref(parseInt(localStorage.getItem('notice-autoarchive-day') || '1', 10) || 1)
                watch([autoArchiveEnabled, autoArchiveDay], () => {
                    localStorage.setItem('notice-autoarchive-enabled', autoArchiveEnabled.value ? '1' : '0')
                    localStorage.setItem('notice-autoarchive-day', String(autoArchiveDay.value || 1))
                })

                // Department suggestions (for new note form & detail modal)
                const departments = ref([])
                const deptDropdownOpen = ref(false)
                const deptDropdownSource = ref('')
                const deptIndex = ref(0)
                const newNoteDepartmentInputRef = ref(null)
                const modalDepartmentInputRef = ref(null)

                const contextMenu = ref({
                    show: false,
                    x: 0,
                    y: 0,
                    selectionStart: 0,
                    selectionEnd: 0
                })

                const contactPopover = ref({
                    show: false,
                    x: 0,
                    y: 0,
                    contact: null
                })

                const columns = [
                    { id: 'backlog', title: '01_Backlog' },
                    { id: 'in_progress', title: '02_In Arbeit' },
                    { id: 'review', title: '03_Review' },
                    { id: 'done', title: '04_Abgeschlossen' },
                    { id: 'archived', title: 'Archiv' }
                ]

                const collapsedMap = {
                    backlog: isBacklogCollapsed,
                    in_progress: isInProgressCollapsed,
                    review: isReviewCollapsed,
                    done: isDoneCollapsed,
                    archived: isArchiveCollapsed
                }

                const isCollapsed = (colId) => collapsedMap[colId] ? collapsedMap[colId].value : false

                const collapseColumn = (colId) => {
                    if (collapsedMap[colId]) collapsedMap[colId].value = true
                }
                const expandColumn = (colId) => {
                    if (collapsedMap[colId]) collapsedMap[colId].value = false
                }
                const toggleCollapse = (colId) => {
                    if (isCollapsed(colId)) expandColumn(colId)
                    else collapseColumn(colId)
                }

                const collapsedStatuses = computed(() => columns.filter(c => isCollapsed(c.id)))
                const expandedColumns = computed(() => columns.filter(c => !isCollapsed(c.id)))

                const gridColsStyle = computed(() => {
                    const parts = expandedColumns.value.map(() => 'minmax(200px, 1fr)')
                    return parts.join(' ')
                })

                const notes = ref([])
                const notesById = computed(() => Object.fromEntries(notes.value.map(n => [n.id, n])))

                const fetchNotes = async () => {
                    try {
                        const res = await fetch('/api/notes')
                        if (res.ok) {
                            notes.value = await res.json()
                            runStartupChecks()
                        }
                    } catch (e) {
                        console.error('Fehler beim Laden der Notizen', e)
                    }
                }

                const fetchDepartments = async () => {
                    try {
                        const res = await fetch('/api/departments')
                        if (res.ok) {
                            departments.value = await res.json()
                        }
                    } catch (e) {
                        console.error('Fehler beim Laden der Departments', e)
                    }
                }

                // --- Address book (contacts) ---
                const contacts = ref([])
                const contactFilter = ref('')
                const contactModalOpen = ref(false)
                const contactForm = ref({ id: null, name: '', department: '', phone: '', email: '', description: '' })
                const highlightContactId = ref(null)

                const filteredContacts = computed(() => {
                    const q = contactFilter.value.trim().toLowerCase()
                    if (!q) return contacts.value
                    return contacts.value.filter(c =>
                        (c.name || '').toLowerCase().includes(q) ||
                        (c.department || '').toLowerCase().includes(q) ||
                        (c.phone || '').toLowerCase().includes(q) ||
                        (c.email || '').toLowerCase().includes(q)
                    )
                })

                const fetchContacts = async () => {
                    try {
                        const res = await fetch('/api/contacts')
                        if (res.ok) contacts.value = await res.json()
                    } catch (e) {
                        console.error('Fehler beim Laden der Kontakte', e)
                    }
                }

                const openContactModal = (contact) => {
                    if (contact && contact.id) {
                        contactForm.value = { ...contact }
                    } else {
                        contactForm.value = { id: null, name: '', department: '', phone: '', email: '', description: '' }
                    }
                    contactModalOpen.value = true
                }

                const saveContact = async () => {
                    if (!contactForm.value.name.trim()) return
                    const f = contactForm.value
                    const url = f.id ? `/api/contacts/${f.id}` : '/api/contacts'
                    const method = f.id ? 'PUT' : 'POST'
                    try {
                        const res = await fetch(url, {
                            method,
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                name: f.name,
                                department: f.department || null,
                                phone: f.phone || null,
                                email: f.email || null,
                                description: f.description || null
                            })
                        })
                        if (res.ok) {
                            contactModalOpen.value = false
                            await fetchContacts()
                        }
                    } catch (e) {
                        console.error('Fehler beim Speichern des Kontakts', e)
                    }
                }

                const deleteContact = async (contact) => {
                    if (!confirm(`Kontakt "${contact.name}" wirklich löschen?`)) return
                    try {
                        const res = await fetch(`/api/contacts/${contact.id}`, { method: 'DELETE' })
                        if (res.ok) {
                            if (highlightContactId.value === contact.id) highlightContactId.value = null
                            await fetchContacts()
                            await fetchTrash()
                        }
                    } catch (e) {
                        console.error('Fehler beim Löschen des Kontakts', e)
                    }
                }

                const deleteDepartment = async (name) => {
                    if (!confirm(`Department "${name}" wirklich löschen? Es wird aus allen Notizen und Kontakten entfernt.`)) return
                    try {
                        const res = await fetch('/api/departments/delete', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ name })
                        })
                        if (res.ok) {
                            departments.value = await res.json()
                            await fetchNotes()
                            await fetchContacts()
                        }
                    } catch (e) {
                        console.error('Fehler beim Löschen des Departments', e)
                    }
                }

                // --- Dashboard computeds ---
                const dashboardActivityDays = 30
                const todayDS = () => dateStr(new Date())

                const dashboardTodayEvents = computed(() => calEventsForDate(new Date()))

                const overdueNotes = computed(() => notes.value.filter(n => isOverdue(n)))

                const dashboardUpcoming = computed(() => {
                    const today = todayDS()
                    const list = []
                    notes.value.forEach(note => {
                        if (note.status === 'archived') return
                        getAppointments(note).forEach(apt => {
                            if (!apt.start) return
                            if (apt.start >= today) {
                                const isOverdueApt = apt.start < today
                                list.push({
                                    title: `${note.title} – ${apt.title || 'Termin'}`,
                                    noteId: note.id,
                                    start: apt.start,
                                    time: apt.time || ''
                                })
                            }
                        })
                    })
                    list.sort((a, b) => (a.start + ' ' + a.time).localeCompare(b.start + ' ' + b.time))
                    return list.slice(0, 10)
                })

                const dashboardStatusRows = computed(() => {
                    const total = Math.max(notes.value.length, 1)
                    return columns.map(col => {
                        const count = notes.value.filter(n => n.status === col.id).length
                        const color = {
                            backlog: 'bg-zinc-400 dark:bg-zinc-500',
                            in_progress: 'bg-blue-500',
                            review: 'bg-amber-500',
                            done: 'bg-emerald-500',
                            archived: 'bg-zinc-300 dark:bg-zinc-700'
                        }[col.id]
                        return { id: col.id, title: col.title, count, pct: Math.round((count / total) * 100), color }
                    })
                })

                const dashboardPriorityRows = computed(() => {
                    const active = notes.value.filter(n => n.status !== 'done' && n.status !== 'archived')
                    const total = Math.max(active.length, 1)
                    return [
                        { label: 'hoch', count: active.filter(n => n.priority === 'high').length, color: 'bg-red-500' },
                        { label: 'mittel', count: active.filter(n => n.priority === 'medium').length, color: 'bg-amber-500' },
                        { label: 'niedrig', count: active.filter(n => n.priority === 'low').length, color: 'bg-zinc-400 dark:bg-zinc-500' }
                    ].map(r => ({ ...r, pct: Math.round((r.count / total) * 100) }))
                })

                const dashboardTodo = computed(() => {
                    let done = 0, total = 0
                    notes.value.forEach(note => {
                        if (note.status === 'archived') return
                        const p = getChecklistProgress(note)
                        if (p) { done += p.done; total += p.total }
                    })
                    return { done, total, pct: total ? Math.round((done / total) * 100) : 0 }
                })

                const dashboardDeptRows = computed(() => {
                    const map = {}
                    notes.value.forEach(note => {
                        if (note.status === 'archived') return
                        getDepartments(note).forEach(dep => {
                            const key = dep.toLowerCase()
                            map[key] = map[key] || { name: dep, count: 0 }
                            map[key].count++
                        })
                    })
                    const rows = Object.values(map)
                    const total = Math.max(rows.reduce((s, r) => s + r.count, 0), 1)
                    return rows
                        .sort((a, b) => b.count - a.count)
                        .map(r => ({ ...r, pct: Math.round((r.count / total) * 100) }))
                })

                const dashboardActivity = computed(() => {
                    const days = dashboardActivityDays
                    const counts = new Array(days).fill(0)
                    const labels = []
                    const today = new Date()
                    for (let i = days - 1; i >= 0; i--) {
                        const d = new Date(today)
                        d.setDate(today.getDate() - i)
                        labels.push(dateStr(d))
                    }
                    const indexOf = (s) => labels.indexOf(s)
                    notes.value.forEach(note => {
                        if (note.status !== 'done' && note.status !== 'archived') return
                        let ds = note.completed_at
                        if (!ds) ds = note.date ? convertDateToISO(note.date) : null
                        if (!ds) return
                        const idx = indexOf(ds)
                        if (idx >= 0) counts[idx]++
                    })
                    const max = Math.max(1, ...counts)
                    return counts.map((c, i) => ({
                        label: labels[i],
                        count: c,
                        height: Math.round((c / max) * 60) + (c ? 4 : 2)
                    }))
                })

                const deptContext = computed(() => {
                    if (deptDropdownSource.value === 'modal') {
                        return {
                            val: () => modalDeptInput.value,
                            input: () => modalDeptInput.value
                        }
                    }
                    return {
                        val: () => newNoteDeptInput.value,
                        input: () => newNoteDeptInput.value
                    }
                })

                const filteredDepartments = computed(() => {
                    const q = deptContext.value.val().trim().toLowerCase()
                    if (!q) return departments.value.slice(0, 8)
                    return departments.value
                        .filter(dep => dep.name.toLowerCase().includes(q))
                        .slice(0, 8)
                })

                const openDeptDropdown = (source) => {
                    deptDropdownSource.value = source
                    deptIndex.value = 0
                    deptDropdownOpen.value = true
                }

                const closeDeptDropdown = () => {
                    deptDropdownOpen.value = false
                }

                const selectDepartment = (name) => {
                    if (deptDropdownSource.value === 'modal') {
                        if (activeNoteDepartments.value.some(d => d.toLowerCase() === name.toLowerCase())) {
                            modalDeptInput.value = ''
                            closeDeptDropdown()
                            return
                        }
                        activeNoteDepartments.value.push(name)
                    } else {
                        if (newNoteDepartments.value.some(d => d.toLowerCase() === name.toLowerCase())) {
                            newNoteDeptInput.value = ''
                            closeDeptDropdown()
                            return
                        }
                        newNoteDepartments.value.push(name)
                    }
                    if (deptDropdownSource.value === 'modal') modalDeptInput.value = ''
                    else newNoteDeptInput.value = ''
                    closeDeptDropdown()
                }

                const addCustomDepartment = (source) => {
                    const input = source === 'modal' ? modalDeptInput.value : newNoteDeptInput.value
                    const name = input.trim()
                    if (!name) return
                    const target = source === 'modal' ? activeNoteDepartments.value : newNoteDepartments.value
                    if (!target.some(d => d.toLowerCase() === name.toLowerCase())) {
                        target.push(name)
                    }
                    if (source === 'modal') modalDeptInput.value = ''
                    else newNoteDeptInput.value = ''
                    if (source === 'modal') {
                        if (modalDepartmentInputRef.value) modalDepartmentInputRef.value.focus()
                    } else {
                        if (newNoteDepartmentInputRef.value) newNoteDepartmentInputRef.value.focus()
                    }
                }

                const removeNewNoteDepartment = (index) => {
                    newNoteDepartments.value.splice(index, 1)
                }

                const removeActiveNoteDepartment = (index) => {
                    activeNoteDepartments.value.splice(index, 1)
                }

                const onDeptInput = () => {
                    deptIndex.value = 0
                }

                const handleDeptKeydown = (e) => {
                    const items = filteredDepartments.value
                    if (e.key === 'ArrowDown') {
                        if (!deptDropdownOpen.value) return
                        e.preventDefault()
                        if (items.length === 0) return
                        deptIndex.value = (deptIndex.value + 1) % items.length
                    } else if (e.key === 'ArrowUp') {
                        if (!deptDropdownOpen.value) return
                        e.preventDefault()
                        if (items.length === 0) return
                        deptIndex.value = (deptIndex.value - 1 + items.length) % items.length
                    } else if (e.key === 'Enter') {
                        e.preventDefault()
                        e.stopPropagation()
                        if (!deptDropdownOpen.value) {
                            addCustomDepartment(deptDropdownSource.value)
                        } else if (items.length > 0) {
                            if (deptIndex.value >= items.length) deptIndex.value = items.length - 1
                            selectDepartment(items[deptIndex.value].name)
                        } else {
                            addCustomDepartment(deptDropdownSource.value)
                        }
                    } else if (e.key === 'Escape') {
                        if (deptDropdownOpen.value) e.preventDefault()
                        e.stopPropagation()
                        closeDeptDropdown()
                    }
                }

                const focusSearchInput = () => {
                    if (searchInputRef.value) searchInputRef.value.focus()
                }

                const handleGlobalKeydown = (e) => {
                    const isCtrlOrMeta = e.ctrlKey || e.metaKey;
                    const isAlt = e.altKey;

                    if (isAlt && (e.key === 'n' || e.key === 'N' || e.code === 'KeyN')) {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        openNewNote()
                    } else if (isCtrlOrMeta && (e.key === 'k' || e.key === 'K' || e.code === 'KeyK')) {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        focusSearchInput()
                    } else if (isCtrlOrMeta && (e.key === 's' || e.key === 'S')) {
                        if (isModalOpen.value) {
                            e.preventDefault()
                            e.stopPropagation()
                            e.stopImmediatePropagation()
                            saveActiveNote()
                        }
                    } else if (e.key === 'Escape') {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        if (showAutocomplete.value) {
                            cancelAutocomplete()
                        } else if (contextMenu.value.show) {
                            closeContextMenu()
                        } else if (deptDropdownOpen.value) {
                            closeDeptDropdown()
                        } else if (contactModalOpen.value) {
                            contactModalOpen.value = false
                        } else if (isSettingsOpen.value) {
                            isSettingsOpen.value = false
                        } else if (exportMenuOpen.value) {
                            exportMenuOpen.value = false
                        } else if (reminderOpen.value) {
                            reminderOpen.value = false
                        } else if (isModalOpen.value) {
                            closeModal()
                        } else if (isNewNoteOpen.value) {
                            closeNewNote()
                        }
                    }
                }

                onMounted(() => {
                    document.documentElement.classList.toggle('dark', isDark.value)
                    fetchNotes()
                    fetchDepartments()
                    fetchContacts()
                    fetchTrash()
                    window.addEventListener('keydown', handleGlobalKeydown, { capture: true })
                    startReminderLoop()
                })

                onUnmounted(() => {
                    window.removeEventListener('keydown', handleGlobalKeydown, { capture: true })
                    clearInterval(reminderInterval)
                    clearInterval(timerInterval)
                })

                const dueStatus = (note) => {
                    if (note.status === 'done' || note.status === 'archived' || !note.due_date) return 'none'
                    const now = new Date()
                    const todayStr = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`
                    if (note.due_date < todayStr) return 'overdue'
                    if (note.due_date === todayStr) return 'due'
                    const due = new Date(note.due_date + 'T00:00:00')
                    const today = new Date(todayStr + 'T00:00:00')
                    if (Math.round((due - today) / 86400000) <= 3) return 'due'
                    return 'none'
                }

                const isOverdue = (note) => dueStatus(note) === 'overdue'
                const isDueSoon = (note) => dueStatus(note) === 'due'

                const openNewNote = () => {
                    isNewNoteOpen.value = true
                    requestAnimationFrame(() => {
                        if (newNoteTitleInputRef.value) {
                            newNoteTitleInputRef.value.focus()
                        }
                    })
                }

                const closeNewNote = () => {
                    isNewNoteOpen.value = false
                    newNoteTitle.value = ''
                    newNotePriority.value = 'medium'
                    newNoteDueDate.value = ''
                    newNoteRepeatRule.value = ''
                    newNoteStatus.value = 'backlog'
                    newNoteContent.value = ''
                    newNoteChecklist.value = []
                    newNoteStepText.value = ''
                    newNoteDepartment.value = ''
                    newNoteDepartments.value = []
                    newNoteDeptInput.value = ''
                    newNoteAppointments.value = []
                    newAptTitle.value = ''
                    newAptStart.value = ''
                    newAptEnd.value = ''
                    newAptTime.value = ''
                    newAptEndTime.value = ''
                    newAptHasEnd.value = false
                    closeDeptDropdown()
                }

                const addNewNoteStep = () => {
                    if (!newNoteStepText.value.trim()) return
                    newNoteChecklist.value.push({ text: newNoteStepText.value.trim(), done: false })
                    newNoteStepText.value = ''
                }

                const removeNewNoteStep = (index) => {
                    newNoteChecklist.value.splice(index, 1)
                }

                const addNewAppointment = () => {
                    if (!newAptTitle.value.trim() || !newAptStart.value) return
                    const apt = { title: newAptTitle.value.trim(), start: newAptStart.value }
                    if (newAptTime.value) apt.time = newAptTime.value
                    if (newAptHasEnd.value && newAptEnd.value && newAptEnd.value >= newAptStart.value) apt.end = newAptEnd.value
                    if (apt.end && newAptEndTime.value) apt.endTime = newAptEndTime.value
                    newNoteAppointments.value.push(apt)
                    newAptTitle.value = ''
                    newAptStart.value = ''
                    newAptEnd.value = ''
                    newAptTime.value = ''
                    newAptEndTime.value = ''
                    newAptHasEnd.value = false
                }

                const addModalAppointment = () => {
                    if (!modalAptTitle.value.trim() || !modalAptStart.value) return
                    const apt = { title: modalAptTitle.value.trim(), start: modalAptStart.value }
                    if (modalAptTime.value) apt.time = modalAptTime.value
                    if (modalAptHasEnd.value && modalAptEnd.value && modalAptEnd.value >= modalAptStart.value) apt.end = modalAptEnd.value
                    if (apt.end && modalAptEndTime.value) apt.endTime = modalAptEndTime.value
                    activeNoteAppointments.value.push(apt)
                    modalAptTitle.value = ''
                    modalAptStart.value = ''
                    modalAptEnd.value = ''
                    modalAptTime.value = ''
                    modalAptEndTime.value = ''
                    modalAptHasEnd.value = false
                }

                const handleNewNoteEnter = (e) => {
                    if (deptDropdownOpen.value) return
                    if (e.target && (e.target.tagName === 'SELECT' || e.target.tagName === 'BUTTON')) return
                    if (newNoteStepInputRef.value && e.target === newNoteStepInputRef.value) return
                    if (newNoteDepartmentInputRef.value && e.target === newNoteDepartmentInputRef.value) return
                    if (newNoteTextareaRef.value && e.target === newNoteTextareaRef.value) return
                    e.preventDefault()
                    createNote()
                }

                const createNote = async () => {
                    if (!newNoteTitle.value.trim()) return

                    const storagePayload = {
                        text: newNoteContent.value,
                        checklist: newNoteChecklist.value
                    }

                    try {
                        const res = await fetch('/api/notes', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: newNoteTitle.value.trim(),
                                content: JSON.stringify(storagePayload),
                                status: newNoteStatus.value,
                                priority: newNotePriority.value,
                                due_date: newNoteDueDate.value || null,
                                departments: JSON.stringify(newNoteDepartments.value),
                                appointments: JSON.stringify(newNoteAppointments.value),
                                repeat_rule: newNoteRepeatRule.value || null
                            })
                        })
                        if (res.ok) {
                            const createdNote = await res.json()
                            notes.value.push(createdNote)
                            fetchDepartments()
                            closeNewNote()
                        }
                    } catch (e) {
                        console.error('Fehler beim Erstellen', e)
                    }
                }

                const exportJson = () => {
                    if (notes.value.length === 0) return
                    const dataStr = "data:text/json;charset=utf-8," + encodeURIComponent(JSON.stringify(notes.value, null, 2))
                    const downloadAnchor = document.createElement('a')
                    downloadAnchor.setAttribute("href", dataStr)
                    downloadAnchor.setAttribute("download", "kanban_notes_export.json")
                    document.body.appendChild(downloadAnchor)
                    downloadAnchor.click()
                    downloadAnchor.remove()
                }

                const exportCsv = () => {
                    if (notes.value.length === 0) return
                    let csvContent = "data:text/csv;charset=utf-8,ID,Title,Status,Priority,Date,DueDate,Departments,Appointments\r\n";
                    notes.value.forEach(note => {
                        const depts = getDepartments(note).join('; ')
                        const appts = getAppointments(note).map(a => `${a.title} (${a.start})`).join('; ')
                        let row = [note.id, `"${note.title.replace(/"/g, '""')}"`, note.status, note.priority, note.date, note.due_date || '', `"${depts.replace(/"/g, '""')}"`, `"${appts.replace(/"/g, '""')}"`];
                        csvContent += row.join(",") + "\r\n";
                    });
                    const encodedUri = encodeURI(csvContent);
                    const downloadAnchor = document.createElement('a');
                    downloadAnchor.setAttribute("href", encodedUri);
                    downloadAnchor.setAttribute("download", "kanban_notes_export.csv");
                    document.body.appendChild(downloadAnchor);
                    downloadAnchor.click();
                    downloadAnchor.remove();
                }

                const exportFullBackup = () => {
                    const data = {
                        app: 'notice',
                        version: '1.5',
                        exported: new Date().toISOString(),
                        notes: notes.value,
                        contacts: contacts.value
                    }
                    const dataStr = "data:text/json;charset=utf-8," + encodeURIComponent(JSON.stringify(data, null, 2))
                    const downloadAnchor = document.createElement('a')
                    downloadAnchor.setAttribute("href", dataStr)
                    downloadAnchor.setAttribute("download", `notice_backup_${new Date().toISOString().slice(0, 10)}.json`)
                    document.body.appendChild(downloadAnchor)
                    downloadAnchor.click()
                    downloadAnchor.remove()
                }

                const startImport = (mode) => {
                    if (mode === 'replace' && !confirm('Beim Ersetzen werden ALLE bestehenden Notizen und Kontakte gelöscht und durch die Import-Datei ersetzt. Fortfahren?')) return
                    importMode.value = mode
                    if (importFileInputRef.value) importFileInputRef.value.click()
                }

                const onImportFile = async (e) => {
                    const file = e.target.files && e.target.files[0]
                    e.target.value = ''
                    if (!file) return
                    let data
                    try {
                        data = JSON.parse(await file.text())
                    } catch {
                        alert('Die Datei ist kein gültiges JSON.')
                        return
                    }
                    if (!data || typeof data !== 'object') {
                        alert('Ungültiges JSON.')
                        return
                    }
                    const pnotes = Array.isArray(data) ? data : (Array.isArray(data.notes) ? data.notes : null)
                    const pcontacts = (!Array.isArray(data) && Array.isArray(data.contacts)) ? data.contacts : []
                    if (!pnotes) {
                        alert('Keine Notizen in der Datei gefunden.')
                        return
                    }
                    try {
                        const res = await fetch('/api/import', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ mode: importMode.value, notes: pnotes, contacts: pcontacts })
                        })
                        if (res.ok) {
                            const r = await res.json()
                            alert(`Import abgeschlossen:\n${r.notes_imported} Notizen importiert, ${r.notes_skipped} übersprungen.\n${r.contacts_imported} Kontakte importiert, ${r.contacts_skipped} übersprungen.`)
                            await fetchNotes()
                            await fetchContacts()
                            await fetchDepartments()
                            await fetchTrash()
                        } else {
                            alert('Import fehlgeschlagen.')
                        }
                    } catch (err) {
                        alert('Import fehlgeschlagen.')
                    }
                }

                // --- Papierkorb (soft-deleted notes & contacts) ---
                const fetchTrash = async () => {
                    try {
                        const res = await fetch('/api/trash')
                        if (res.ok) {
                            const data = await res.json()
                            trashNotes.value = data.notes || []
                            trashContacts.value = data.contacts || []
                        }
                    } catch (e) {
                        console.error('Fehler beim Laden des Papierkorbs', e)
                    }
                }

                const restoreNote = async (id) => {
                    const res = await fetch(`/api/notes/${id}/restore`, { method: 'POST' })
                    if (res.ok) {
                        await fetchTrash()
                        await fetchNotes()
                    }
                }

                const restoreContact = async (id) => {
                    const res = await fetch(`/api/contacts/${id}/restore`, { method: 'POST' })
                    if (res.ok) {
                        await fetchTrash()
                        await fetchContacts()
                    }
                }

                const forceDeleteNote = async (id) => {
                    if (!confirm('Notiz endgültig löschen? Dies kann nicht rückgängig gemacht werden.')) return
                    const res = await fetch(`/api/notes/${id}/force`, { method: 'DELETE' })
                    if (res.ok) await fetchTrash()
                }

                const forceDeleteContact = async (id) => {
                    if (!confirm('Kontakt endgültig löschen? Dies kann nicht rückgängig gemacht werden.')) return
                    const res = await fetch(`/api/contacts/${id}/force`, { method: 'DELETE' })
                    if (res.ok) await fetchTrash()
                }

                const clearTrash = async () => {
                    if (!confirm('Papierkorb wirklich leeren? Alle Elemente werden endgültig gelöscht.')) return
                    const res = await fetch('/api/trash/clear', { method: 'POST' })
                    if (res.ok) await fetchTrash()
                }

                const openModal = (note) => {
                    activeNote.value = { ...note }
                    try {
                        const parsed = JSON.parse(note.content);
                        if (parsed && typeof parsed === 'object' && parsed.text !== undefined) {
                            activeNote.value.content = parsed.text || '';
                            activeChecklist.value = parsed.checklist || [];
                        } else {
                            activeChecklist.value = [];
                        }
                    } catch {
                        activeChecklist.value = [];
                    }
                    activeNoteDepartments.value = getDepartments(note)
                    activeNoteAppointments.value = getAppointments(note)
                    modalDeptInput.value = ''
                    isPreviewMode.value = true
                    isModalOpen.value = true
                    closeContextMenu()
                }

                const closeModal = () => {
                    isModalOpen.value = false
                    activeNote.value = null
                    activeChecklist.value = []
                    newStepText.value = ''
                    activeNoteAppointments.value = []
                    activeNoteDepartments.value = []
                    modalDeptInput.value = ''
                    closeContextMenu()
                    showAutocomplete.value = false
                    clearTimeout(autocompleteTimer)
                    collectOverdueReminders()
                }

                // Context menu logic
                const openContextMenu = (e) => {
                    if (!textareaRef.value) return
                    const rect = e.target.getBoundingClientRect()
                    // Offset result to the menu's containing block (nearest positioned ancestor)
                    let cont = textareaRef.value.parentElement
                    while (cont && getComputedStyle(cont).position === 'static') cont = cont.parentElement
                    const contRect = (cont || document.body).getBoundingClientRect()
                    contextMenu.value = {
                        show: true,
                        x: (e.clientX - rect.left) + (rect.left - contRect.left),
                        y: (e.clientY - rect.top) + (rect.top - contRect.top),
                        selectionStart: textareaRef.value.selectionStart,
                        selectionEnd: textareaRef.value.selectionEnd
                    }
                }

                const closeContextMenu = () => {
                    contextMenu.value.show = false
                    exportMenuOpen.value = false
                }

                // --- Autocomplete for [[ note links ---
                const searchAutocomplete = async (q) => {
                    try {
                        const url = autocompleteType.value === 'address'
                            ? `/api/contacts/search?q=${encodeURIComponent(q)}`
                            : `/api/notes/search?q=${encodeURIComponent(q)}`
                        const res = await fetch(url)
                        if (res.ok) {
                            let data = await res.json()
                            if (autocompleteType.value === 'address') {
                                data = data.map(c => ({ id: c.id, title: c.name }))
                            }
                            autocompleteResults.value = data
                            autocompleteIndex.value = 0
                            showAutocomplete.value = true
                            updateAutocompletePos()
                        } else {
                            showAutocomplete.value = false
                        }
                    } catch (e) {
                        showAutocomplete.value = false
                    }
                }

                const acTextarea = () => isNewNoteOpen.value ? newNoteTextareaRef.value : textareaRef.value
                const acMirror = () => isNewNoteOpen.value ? newNoteCaretMirrorRef.value : caretMirrorRef.value
                const acGetContent = () => isNewNoteOpen.value ? (newNoteContent.value || '') : (activeNote.value ? activeNote.value.content : '')
                const acTextareaValue = () => {
                    const ta = acTextarea()
                    return ta ? ta.value : acGetContent()
                }
                const acSetContent = (str) => {
                    if (isNewNoteOpen.value) newNoteContent.value = str
                    else if (activeNote.value) activeNote.value.content = str
                }

                const updateAutocompletePos = () => {
                    const ta = acTextarea()
                    const mirror = acMirror()
                    if (!ta || !mirror) return
                    const cursor = ta.selectionStart
                    const text = acTextareaValue()
                    const c = Math.min(cursor, text.length)
                    const before = text.substring(0, c)
                    const line = before.split('\n').length - 1
                    const colLines = before.split('\n')
                    const col = colLines[colLines.length - 1].length

                    // Update the caret mirror: text with a marking span at the cursor position
                    let html = ''
                    for (let i = 0; i < colLines.length; i++) {
                        const chunk = colLines[i]
                        if (i < colLines.length - 1) {
                            html += chunk.replace(/</g, '&lt;') + '\n'
                        } else {
                            // last line
                            const colonIdx = chunk.lastIndexOf(':')
                            if (colonIdx >= col) {
                                // cursor on this line, place marker after the prefix
                                html += chunk.substring(0, col).replace(/</g, '&lt;')
                                html += '<span id="caret-marker">a</span>'
                            } else {
                                html += chunk.substring(0, col).replace(/</g, '&lt;')
                                html += '<span id="caret-marker">a</span>'
                            }
                        }
                    }
                    mirror.innerHTML = html + '<br>'

                    const marker = mirror.querySelector('#caret-marker')
                    if (!marker) return
                    const taRect = ta.getBoundingClientRect()
                    const markRect = marker.getBoundingClientRect()
                    // Offset result to the dropdown's containing block (nearest positioned ancestor)
                    let cont = ta.parentElement
                    while (cont && getComputedStyle(cont).position === 'static') cont = cont.parentElement
                    const contRect = (cont || document.body).getBoundingClientRect()
                    const x = (markRect.left - taRect.left) + (taRect.left - contRect.left)
                    const y = (markRect.top - taRect.top) + (taRect.top - contRect.top)

                    // Flip: if there is enough space above / not below, show the dropdown upward
                    const container = ta.closest('.md\\:col-span-2') || ta.parentElement
                    const containerHeight = container ? container.clientHeight : 300
                    const estHeight = Math.min(224, (autocompleteResults.value.length * 26) + 30)
                    const flip = (y + 24 + estHeight) > containerHeight && y > estHeight
                    autocompletePos.value = { x, y: flip ? (y - estHeight) : (y + 24), flip }
                }

                const cancelAutocomplete = () => {
                    clearTimeout(autocompleteTimer)
                    const ta = acTextarea()
                    if (ta) {
                        const text = acTextareaValue()
                        const cursor = Math.min(ta.selectionStart, text.length)
                        const start = Math.min(autocompleteStart.value, cursor)
                        const removeFrom = Math.max(0, start - 2)
                        const before = text.substring(removeFrom, start)
                        if (before === '[[' || before === '{{') {
                            acSetContent(text.substring(0, removeFrom) + text.substring(cursor))
                            const target = removeFrom
                            requestAnimationFrame(() => {
                                if (ta) {
                                    ta.selectionStart = ta.selectionEnd = target
                                    ta.focus()
                                }
                            })
                        }
                    }
                    showAutocomplete.value = false
                    autocompleteResults.value = []
                }

                const handleAutocomplete = () => {
                    const ta = acTextarea()
                    if (!ta) return
                    // Prefer the DOM value (source of truth) since v-model may not be in sync yet.
                    const text = acTextareaValue()
                    const cursor = ta.selectionStart
                    const effectiveCursor = Math.min(cursor, text.length)

                    // Detect the nearest active opener ([[ or {{) before the cursor
                    const openNote = text.lastIndexOf('[[', effectiveCursor)
                    const openAddr = text.lastIndexOf('{{', effectiveCursor)
                    let opener = -1
                    let kind = null
                    if (openNote >= 0 && openNote >= openAddr) {
                        opener = openNote
                        kind = 'note'
                    } else if (openAddr >= 0) {
                        opener = openAddr
                        kind = 'address'
                    } else {
                        opener = -1
                        kind = null
                    }

                    if (opener === -1 || !kind) {
                        showAutocomplete.value = false
                        autocompleteType.value = 'note'
                        return
                    }
                    const between = text.substring(opener + 2, effectiveCursor)
                    // Close if the closer already lies between the opener and cursor
                    const endMarker = kind === 'note' ? ']]' : '}}'
                    if (between.includes(endMarker)) {
                        showAutocomplete.value = false
                        autocompleteType.value = 'note'
                        return
                    }
                    // Stop after "|" (display alias is being typed)
                    if (between.includes('|')) {
                        showAutocomplete.value = false
                        autocompleteType.value = 'note'
                        return
                    }

                    autocompleteType.value = kind
                    autocompleteStart.value = opener + 2
                    autocompleteQuery.value = between
                    updateAutocompletePos()
                    clearTimeout(autocompleteTimer)
                    autocompleteTimer = setTimeout(() => {
                        searchAutocomplete(between)
                    }, 200)
                }

                const selectAutocomplete = (index) => {
                    const ta = acTextarea()
                    if (!ta) return
                    const selected = autocompleteResults.value[index]
                    if (!selected) return
                    const closer = autocompleteType.value === 'address' ? '}}' : ']]'
                    const text = acTextareaValue()
                    const cursor = Math.min(ta.selectionStart, text.length)
                    const start = Math.min(autocompleteStart.value, cursor)
                    // Replace opener+partial with opener+Full title+closer
                    const newText = text.substring(0, start) + selected.title + closer + text.substring(cursor)
                    acSetContent(newText)
                    showAutocomplete.value = false
                    // Move the cursor after the closer
                    requestAnimationFrame(() => {
                        if (ta) {
                            const newCursor = start + selected.title.length + 2
                            ta.selectionStart = ta.selectionEnd = newCursor
                            ta.focus()
                        }
                    })
                }


                const handleAutocompleteKeydown = (e) => {
                    if (!showAutocomplete.value) return
                    if (e.key === 'ArrowDown') {
                        e.preventDefault()
                        autocompleteIndex.value = (autocompleteIndex.value + 1) % autocompleteResults.value.length
                    } else if (e.key === 'ArrowUp') {
                        e.preventDefault()
                        autocompleteIndex.value = (autocompleteIndex.value - 1 + autocompleteResults.value.length) % autocompleteResults.value.length
                    } else if (e.key === 'Enter') {
                        e.preventDefault()
                        selectAutocomplete(autocompleteIndex.value)
                    }
                }

                // --- Note links in preview: [[Title|Display-Name]] ---
                const processNoteLinks = (html) => {
                    return html.replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (match, title, display) => {
                        const safeTitle = title.replace(/"/g, '&quot;')
                        const safeDisplay = (display || title).replace(/"/g, '&quot;')
                        return `<a class="note-link" data-note-title="${safeTitle}">${safeDisplay}</a>`
                    })
                }

                const handleNoteLinkClick = (e) => {
                    const addrLink = e.target.closest('.address-link')
                    if (addrLink) {
                        handleAddressLinkClick(e)
                        return
                    }
                    const link = e.target.closest('.note-link')
                    if (!link) return
                    e.preventDefault()
                    const title = link.getAttribute('data-note-title')
                    const note = notes.value.find(n => n.title === title)
                    if (note) {
                        openModal(note)
                    } else {
                        alert(`Notiz "${title}" nicht gefunden.`)
                    }
                }

                // --- Address links in preview: {{Name|display}} ---
                const processAddressLinks = (html) => {
                    return html.replace(/\{\{([^}|]+)(?:\|([^}]+))?\}\}/g, (match, name, display) => {
                        const safeName = name.replace(/"/g, '&quot;')
                        const safeDisplay = (display || name).replace(/"/g, '&quot;')
                        return `<a class="note-link address-link" data-address-name="${safeName}">${safeDisplay}</a>`
                    })
                }

                const hideContactPopover = () => {
                    contactPopover.value.show = false
                    contactPopover.value.contact = null
                }

                const showContactPopover = (e) => {
                    const link = e.target.closest('.address-link')
                    if (!link) {
                        hideContactPopover()
                        return
                    }
                    const name = link.getAttribute('data-address-name')
                    const contact = contacts.value.find(c => (c.name || '') === name)
                    const rect = link.getBoundingClientRect()
                    let cont = link
                    while (cont && getComputedStyle(cont).position === 'static') cont = cont.parentElement
                    const contRect = (cont || document.body).getBoundingClientRect()
                    contactPopover.value = {
                        show: true,
                        x: Math.min(rect.right + 6 - contRect.left, 280),
                        y: rect.bottom + 4 - contRect.top,
                        contact: contact || { name, department: null, phone: null, email: null, description: null }
                    }
                }

                const handleAddressLinkClick = (e) => {
                    const link = e.target.closest('.address-link')
                    if (!link) return
                    e.preventDefault()
                    hideContactPopover()
                    if (isModalOpen.value) closeModal()
                    const name = link.getAttribute('data-address-name')
                    const contact = contacts.value.find(c => (c.name || '') === name)
                    if (contact) {
                        highlightContactId.value = contact.id
                        activeView.value = 'addressbook'
                    } else {
                        highlightContactId.value = null
                        activeView.value = 'addressbook'
                        contactFilter.value = name
                    }
                }

                // --- Duplicate note ---
                const duplicateNote = async (note) => {
                    try {
                        const res = await fetch(`/api/notes/${note.id}/duplicate`, { method: 'POST' })
                        if (res.ok) {
                            const newNote = await res.json()
                            notes.value.push(newNote)
                            fetchDepartments()
                        }
                    } catch (e) {
                        console.error('Fehler beim Duplizieren', e)
                    }
                }

                // --- Archive / restore ---
                const archiveNote = async (note) => {
                    await updateNoteStatus(note, 'archived')
                }

                const unarchiveNote = async (note) => {
                    await updateNoteStatus(note, 'backlog')
                }

                const updateNoteStatus = async (note, newStatus) => {
                    try {
                        const res = await fetch(`/api/notes/${note.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: note.title,
                                content: note.content,
                                status: newStatus,
                                priority: note.priority,
                                due_date: note.due_date,
                                sort_order: -1
                            })
                        })
                        if (res.ok) {
                            note.status = newStatus
                            note.sort_order = -1
                        }
                    } catch (e) {
                        console.error('Fehler beim Status-Update', e)
                    }
                }

                // --- Auto-archive (settings) ---
                const autoArchiveDone = async () => {
                    if (!autoArchiveEnabled.value) return
                    const now = new Date()
                    if (now.getDate() < (autoArchiveDay.value || 1)) return
                    const candidates = notes.value.filter(n => n.status === 'done')
                    for (const note of candidates) {
                        await archiveNote(note)
                    }
                }

                // --- Overdue appointment reminder (Feature 3) ---
                const reminderOpen = ref(false)
                const overdueReminders = ref([])
                const isAppointmentOverdue = (apt) => {
                    if (!apt.start) return false
                    const todayStr = dateStr(new Date())
                    const nowH = new Date()
                    const nowMin = nowH.getHours() * 60 + nowH.getMinutes()
                    if (apt.end) {
                        if (apt.end < todayStr) return true
                        if (apt.end > todayStr) return false
                        if (apt.endTime) {
                            const m = apt.endTime.match(/^(\d{1,2}):(\d{2})/)
                            if (m) return nowMin > (parseInt(m[1], 10) * 60 + parseInt(m[2], 10))
                        }
                        return false
                    }
                    if (apt.start < todayStr) return true
                    if (apt.start > todayStr) return false
                    if (apt.time) {
                        const m = apt.time.match(/^(\d{1,2}):(\d{2})/)
                        if (m) return nowMin > (parseInt(m[1], 10) * 60 + parseInt(m[2], 10))
                    }
                    return false
                }

                // --- Browser notifications for overdue items ---
                const notify = (title, body) => {
                    if (typeof Notification !== 'undefined' && Notification.permission === 'granted') {
                        try { new Notification(title, { body }) } catch (e) {}
                    }
                }

                let notifiedKeys = new Set()
                try {
                    const saved = JSON.parse(localStorage.getItem('notice-notified') || '[]')
                    if (Array.isArray(saved)) notifiedKeys = new Set(saved)
                } catch (e) {}

                const collectOverdueReminders = (opts = {}) => {
                    const list = []
                    notes.value.forEach(note => {
                        if (note.status === 'archived' || note.status === 'done') return
                        const appts = getAppointments(note)
                        appts.forEach(apt => {
                            if (isAppointmentOverdue(apt)) {
                                const ev = {
                                    title: `${note.title} – ${apt.title || 'Termin'}`,
                                    time: apt.time || '',
                                    start: apt.start,
                                    end: apt.end || '',
                                    endTime: apt.endTime || ''
                                }
                                list.push({
                                    note,
                                    apt,
                                    kind: 'apt',
                                    range: fmtAptRange(ev)
                                })
                            }
                        })
                        if (isOverdue(note)) {
                            list.push({
                                note,
                                apt: { title: 'Fällig', start: note.due_date },
                                kind: 'due',
                                range: 'Fällig: ' + fmtDate(note.due_date)
                            })
                        }
                    })
                    const currentKeys = new Set()
                    list.forEach(r => {
                        const key = (r.kind === 'apt' ? 'apt' : 'due') + '_' + r.note.id + '_' + r.apt.start
                        currentKeys.add(key)
                        if (!notifiedKeys.has(key) && !opts.skipNotify) {
                            notifiedKeys.add(key)
                            notify('Notice: überfällig', `${r.range} – ${r.note.title}`)
                        }
                    })
                    notifiedKeys = new Set([...notifiedKeys].filter(k => currentKeys.has(k)))
                    try { localStorage.setItem('notice-notified', JSON.stringify([...notifiedKeys])) } catch (e) {}
                    overdueReminders.value = list
                    reminderOpen.value = list.length > 0
                }

                watch(overdueReminders, (list) => {
                    document.title = list.length > 0 ? `(${list.length}) Notice – Pro Kanban Notes` : 'Notice – Pro Kanban Notes'
                })

                const openNoteFromReminder = (ri) => {
                    const r = overdueReminders.value[ri]
                    if (!r) return
                    openModal(r.note)
                    overdueReminders.value.splice(ri, 1)
                    if (overdueReminders.value.length === 0) reminderOpen.value = false
                }

                let startupChecksDone = false
                const runStartupChecks = async () => {
                    if (startupChecksDone) return
                    startupChecksDone = true
                    await autoArchiveDone()
                    collectOverdueReminders({ skipNotify: true })
                }

                // --- Countdown timer (header) ---
                const timerOpen = ref(false)
                const timerTotal = ref(0)
                const timerRemaining = ref(0)
                const timerRunning = ref(false)
                let timerInterval = null

                const fmtTimer = () => {
                    const s = Math.max(0, Math.round(timerRemaining.value))
                    return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`
                }

                const timerSet = (min) => {
                    timerTotal.value = min * 60
                    timerRemaining.value = min * 60
                    timerRunning.value = false
                    clearInterval(timerInterval)
                }

                const timerToggle = () => {
                    if (timerRemaining.value <= 0) {
                        if (timerTotal.value <= 0) timerTotal.value = 600
                        timerRemaining.value = timerTotal.value
                    }
                    timerRunning.value = !timerRunning.value
                    clearInterval(timerInterval)
                    if (timerRunning.value) {
                        timerInterval = setInterval(() => {
                            timerRemaining.value -= 1
                            if (timerRemaining.value <= 0) {
                                timerRemaining.value = 0
                                clearInterval(timerInterval)
                                timerRunning.value = false
                                timerOpen.value = false
                                notify('Notice: Timer abgelaufen', 'Der Countdown ist abgelaufen.')
                            }
                        }, 1000)
                    }
                }

                const timerReset = () => {
                    timerRunning.value = false
                    clearInterval(timerInterval)
                    timerRemaining.value = timerTotal.value
                }

                // Periodic overdue check (every 5 minutes) + notification permission
                let reminderInterval = null
                const startReminderLoop = () => {
                    reminderInterval = setInterval(() => collectOverdueReminders(), 300000)
                    if (typeof Notification !== 'undefined' && Notification.permission === 'default') {
                        Notification.requestPermission().catch(() => {})
                    }
                }

                // --- Drag & drop sorting ---
                let dragOverNote = null

                const onDragOver = (note, e) => {
                    dragOverNote = note
                }

                const reorderColumn = async (columnId, dragged, target) => {
                    let colNotes = notes.value
                        .filter(n => n.status === columnId)
                        .sort((a, b) => (a.sort_order || 0) - (b.sort_order || 0) || a.id - b.id)

                    // Remove the dragged element
                    const fromIndex = colNotes.findIndex(n => n.id === dragged.id)
                    if (fromIndex !== -1) colNotes.splice(fromIndex, 1)

                    // Find the target index
                    let toIndex = colNotes.findIndex(n => n.id === target.id)
                    if (toIndex === -1) toIndex = colNotes.length

                    // Insert
                    colNotes.splice(toIndex, 0, dragged)

                    // Assign new sort_order values
                    const updates = colNotes.map((n, i) => {
                        if ((n.sort_order || 0) !== i || n.id === dragged.id) {
                            return { id: n.id, sort_order: i }
                        }
                        return null
                    }).filter(Boolean)

                    // Update locally
                    colNotes.forEach((n, i) => { n.sort_order = i })
                    dragged.status = columnId

                    // Update the API
                    await Promise.all(updates.map(async (u) => {
                        const n = notes.value.find(x => x.id === u.id)
                        if (!n) return
                        try {
                            await fetch(`/api/notes/${u.id}`, {
                                method: 'PUT',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({
                                    title: n.title,
                                    content: n.content,
                                    status: n.status,
                                    priority: n.priority,
                                    due_date: n.due_date,
                                    sort_order: u.sort_order
                                })
                            })
                        } catch (e) {
                            console.error('Fehler beim Sortieren', e)
                        }
                    }))
                }

                const applyFormat = (syntax) => {
                    if (!activeNote.value) return
                    const start = contextMenu.value.selectionStart
                    const end = contextMenu.value.selectionEnd
                    const text = activeNote.value.content
                    
                    const selectedText = text.substring(start, end)
                    const replacement = `${syntax}${selectedText || 'Text'}${syntax}`
                    
                    activeNote.value.content = text.substring(0, start) + replacement + text.substring(end)
                    closeContextMenu()
                }

                const applyHtmlColor = (colorHex) => {
                    if (!activeNote.value) return
                    const start = contextMenu.value.selectionStart
                    const end = contextMenu.value.selectionEnd
                    const text = activeNote.value.content
                    
                    const selectedText = text.substring(start, end)
                    const replacement = `<span style="color: ${colorHex}">${selectedText || 'Text'}</span>`
                    
                    activeNote.value.content = text.substring(0, start) + replacement + text.substring(end)
                    closeContextMenu()
                }

                const addStep = () => {
                    if (!newStepText.value.trim()) return
                    activeChecklist.value.push({ text: newStepText.value.trim(), done: false })
                    newStepText.value = ''
                }

                const removeStep = (index) => {
                    activeChecklist.value.splice(index, 1)
                }

                const renderedMarkdown = computed(() => {
                    if (!activeNote.value || !activeNote.value.content) return '<p class="text-zinc-600 dark:text-zinc-500">Kein Inhalt vorhanden.</p>'
                    let html = marked.parse(activeNote.value.content)
                    html = processAddressLinks(processNoteLinks(html))
                    // Make markdown checkboxes in the preview clickable, mapped
                    // positionally to the checklist entries.
                    let idx = 0
                    html = html.replace(/<input[^>]*disabled[^>]*type="checkbox"[^>]*>/g, () => {
                        const cur = idx++
                        const done = activeChecklist.value[cur] ? activeChecklist.value[cur].done : false
                        return `<input type="checkbox" data-cb-i="${cur}"${done ? ' checked' : ''}>`
                    })
                    return html
                })

                const onPreviewClick = (e) => {
                    const cb = e.target.closest('input[data-cb-i]')
                    if (cb) {
                        const i = parseInt(cb.getAttribute('data-cb-i'), 10)
                        if (activeChecklist.value[i]) {
                            activeChecklist.value[i].done = !activeChecklist.value[i].done
                        }
                        return
                    }
                    handleNoteLinkClick(e)
                }

                const saveActiveNote = async () => {
                    if (!activeNote.value) return

                    const storagePayload = {
                        text: activeNote.value.content,
                        checklist: activeChecklist.value
                    };
                    const serializedContent = JSON.stringify(storagePayload);

                    try {
                        const res = await fetch(`/api/notes/${activeNote.value.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: activeNote.value.title,
                                content: serializedContent,
                                status: activeNote.value.status,
                                priority: activeNote.value.priority,
                                due_date: activeNote.value.due_date || null,
                                departments: JSON.stringify(activeNoteDepartments.value),
                                appointments: JSON.stringify(activeNoteAppointments.value),
                                repeat_rule: activeNote.value.repeat_rule || null
                            })
                        })
                        if (res.ok) {
                            const index = notes.value.findIndex(n => n.id === activeNote.value.id)
                            if (index !== -1) {
                                notes.value[index] = { ...activeNote.value, content: serializedContent, departments: JSON.stringify(activeNoteDepartments.value), appointments: JSON.stringify(activeNoteAppointments.value) }
                            }
                            fetchDepartments()
                            closeModal()
                        }
                    } catch (e) {
                        console.error('Fehler beim Aktualisieren', e)
                    }
                }

                const deleteNote = async (id) => {
                    try {
                        const res = await fetch(`/api/notes/${id}`, { method: 'DELETE' })
                        if (res.ok) {
                            notes.value = notes.value.filter(note => note.id !== id)
                            fetchTrash()
                        }
                    } catch (e) {
                        console.error('Fehler beim Löschen', e)
                    }
                }

                // --- Search syntax: key:value parser ---
                const STATUS_ALIASES = {
                    'backlog': 'backlog',
                    'in_progress': 'in_progress',
                    'inprogress': 'in_progress',
                    'in-progress': 'in_progress',
                    'in arbeit': 'in_progress',
                    'review': 'review',
                    'done': 'done',
                    'abgeschlossen': 'done',
                    'archived': 'archived',
                    'archiv': 'archived'
                }

                const PRIO_ALIASES = {
                    'high': 'high',
                    'h': 'high',
                    'hoch': 'high',
                    'medium': 'medium',
                    'm': 'medium',
                    'mittel': 'medium',
                    'low': 'low',
                    'l': 'low',
                    'niedrig': 'low'
                }

                const parseFlexDate = (str) => {
                    // YYYY-MM-DD
                    let m = str.match(/^(\d{4})-(\d{2})-(\d{2})$/)
                    if (m) return new Date(+m[1], +m[2] - 1, +m[3]).getTime()
                    // DD.MM.YYYY
                    m = str.match(/^(\d{2})\.(\d{2})\.(\d{4})$/)
                    if (m) return new Date(+m[3], +m[2] - 1, +m[1]).getTime()
                    // YYYY-MM-DD tolerant to leading zeros being omitted
                    m = str.match(/^(\d{4})-(\d{1,2})-(\d{1,2})$/)
                    if (m) return new Date(+m[1], +m[2] - 1, +m[3]).getTime()
                    return null
                }

                const startOfDay = (t) => new Date(new Date(t).setHours(0, 0, 0, 0)).getTime()
                const endOfDay = (t) => new Date(new Date(t).setHours(23, 59, 59, 999)).getTime()

                const parseSearchQuery = (raw) => {
                    const result = {
                        freeText: [],
                        titel: [],
                        inhalt: [],
                        status: [],
                        prio: [],
                        datum: [],
                        faellig: [],
                        department: []
                    }
                    // Detect key:value tokens (value may contain ".." or date characters, no whitespace)
                    const tokens = raw.match(/(\w+):([^\s]+)/g) || []
                    let rest = raw
                    tokens.forEach(tok => {
                        const idx = rest.indexOf(tok)
                        const valStart = tok.indexOf(':') + 1
                        const val = tok.substring(valStart)
                        const key = tok.substring(0, tok.indexOf(':')).toLowerCase()
                        // Strip the matched token from the remainder for free-text matching
                        rest = rest.replace(tok, ' ')
                        if (key === 'titel' || key === 'title') result.titel.push(val)
                        else if (key === 'inhalt' || key === 'content') result.inhalt.push(val)
                        else if (key === 'status') result.status.push(val)
                        else if (key === 'prio' || key === 'priority') result.prio.push(val)
                        else if (key === 'datum' || key === 'date') result.datum.push(val)
                        else if (key === 'faellig' || key === 'due' || key === 'due_date' || key === 'fällig') result.faellig.push(val)
                        else if (key === 'dep' || key === 'department') result.department.push(val)
                        else result.freeText.push(tok)
                    })
                    // Remaining free-text tokens
                    const freeTokens = rest.split(/\s+/).filter(t => t.trim() !== '')
                    result.freeText.push(...freeTokens)
                    return result
                }

                const evalDateSpec = (fieldVal, spec) => {
                    const today = startOfDay(Date.now())
                    const endToday = endOfDay(Date.now())
                    if (spec === 'none') return !fieldVal || fieldVal.trim() === ''
                    if (spec === 'any') return !!fieldVal && fieldVal.trim() !== ''
                    if (!fieldVal || fieldVal.trim() === '') return false

                    const valTime = parseFlexDate(fieldVal)
                    if (valTime === null) return false

                    if (spec === 'today') return valTime >= today && valTime <= endToday
                    if (spec === 'overdue') return valTime < today
                    if (spec === 'this_week' || spec === 'thisweek' || spec === 'woche') {
                        const now = new Date()
                        const day = (now.getDay() + 6) % 7
                        const monday = startOfDay(new Date(now).setDate(now.getDate() - day))
                        const sunday = endOfDay(new Date(now).setDate(now.getDate() - day + 6))
                        return valTime >= monday && valTime <= sunday
                    }
                    if (spec.startsWith('>=')) {
                        const t = parseFlexDate(spec.substring(2))
                        return t !== null && valTime >= startOfDay(t)
                    }
                    if (spec.startsWith('<=')) {
                        const t = parseFlexDate(spec.substring(2))
                        return t !== null && valTime <= endOfDay(t)
                    }
                    if (spec.startsWith('>')) {
                        const t = parseFlexDate(spec.substring(1))
                        return t !== null && valTime > endOfDay(t)
                    }
                    if (spec.startsWith('<')) {
                        const t = parseFlexDate(spec.substring(1))
                        return t !== null && valTime < startOfDay(t)
                    }
                    if (spec.includes('..')) {
                        const [a, b] = spec.split('..')
                        const tA = parseFlexDate(a)
                        const tB = parseFlexDate(b)
                        if (tA === null || tB === null) return false
                        return valTime >= startOfDay(tA) && valTime <= endOfDay(tB)
                    }
                    const t = parseFlexDate(spec)
                    if (t === null) return false
                    return valTime >= startOfDay(t) && valTime <= endOfDay(t)
                }

                const notePlainContent = (note) => {
                    try {
                        const parsed = JSON.parse(note.content)
                        if (parsed && typeof parsed === 'object' && parsed.text !== undefined) return String(parsed.text)
                    } catch {}
                    return note.content || ''
                }

                const getDepartments = (note) => {
                    if (!note) return []
                    try {
                        const raw = note.departments
                        if (!raw) {
                            if (note.department && note.department.trim()) return [note.department]
                            return []
                        }
                        const parsed = JSON.parse(raw)
                        if (Array.isArray(parsed)) return parsed.filter(d => d && d.trim() !== '')
                        return []
                    } catch {
                        if (note.department && note.department.trim()) return [note.department]
                        return []
                    }
                }

                const getAppointments = (note) => {
                    if (!note) return []
                    try {
                        const raw = note.appointments
                        if (!raw) return []
                        const parsed = JSON.parse(raw)
                        if (Array.isArray(parsed)) return parsed
                        return []
                    } catch {
                        return []
                    }
                }

                const getChecklistProgress = (note) => {
                    if (!note) return null
                    try {
                        const parsed = JSON.parse(note.content)
                        const checklist = (parsed && parsed.checklist) ? parsed.checklist : []
                        if (!Array.isArray(checklist) || checklist.length === 0) return null
                        const done = checklist.filter(s => s.done).length
                        return { done, total: checklist.length }
                    } catch {
                        return null
                    }
                }

                const matchesNote = (note, q) => {
                    if (q.freeText.length) {
                        const depts = getDepartments(note).join(' ')
                        const haystack = (note.title + ' ' + depts + ' ' + notePlainContent(note)).toLowerCase()
                        if (!q.freeText.every(t => haystack.includes(t.toLowerCase()))) return false
                    }
                    if (q.titel.length) {
                        const title = note.title.toLowerCase()
                        if (!q.titel.every(t => title.includes(t.toLowerCase()))) return false
                    }
                    if (q.inhalt.length) {
                        const content = notePlainContent(note).toLowerCase()
                        if (!q.inhalt.every(t => content.includes(t.toLowerCase()))) return false
                    }
                    if (q.department.length) {
                        const depts = getDepartments(note).join(' ').toLowerCase()
                        if (!q.department.every(t => depts.includes(t.toLowerCase()))) return false
                    }
                    if (q.status.length) {
                        const resolved = q.status.map(s => STATUS_ALIASES[s.trim().toLowerCase()]).filter(Boolean)
                        if (resolved.length && !resolved.includes(note.status)) return false
                    }
                    if (q.prio.length) {
                        const resolved = q.prio.map(p => PRIO_ALIASES[p.trim().toLowerCase()]).filter(Boolean)
                        if (resolved.length && !resolved.includes(note.priority)) return false
                    }
                    if (q.datum.length) {
                        if (!q.datum.every(d => evalDateSpec(note.date, d))) return false
                    }
                    if (q.faellig.length) {
                        if (!q.faellig.every(d => evalDateSpec(note.due_date, d))) return false
                    }
                    return true
                }

                // --- Calendar logic ---
                const dateStr = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`

                const convertDateToISO = (v) => {
                    if (!v) return null
                    const m = String(v).match(/^(\d{1,2})\.(\d{1,2})\.(\d{4})$/)
                    if (m) return `${m[3]}-${String(m[2]).padStart(2, '0')}-${String(m[1]).padStart(2, '0')}`
                    if (/^\d{4}-\d{2}-\d{2}$/.test(String(v))) return String(v)
                    return null
                }

                const fmtDate = (v) => {
                    if (!v) return ''
                    const m = String(v).match(/^(\d{4})-(\d{2})-(\d{2})/)
                    return m ? `${m[3]}.${m[2]}.${m[1]}` : String(v)
                }

                const fmtAptRange = (ev) => {
                    let s = fmtDate(ev.start) + (ev.time ? ' ' + ev.time : '')
                    if (ev.end) s += ' – ' + fmtDate(ev.end) + (ev.endTime ? ' ' + ev.endTime : '')
                    return s
                }

                const calEventsForDate = (d) => {
                    const ds = dateStr(d)
                    const events = []
                    notes.value.forEach(note => {
                        if (note.status === 'archived') return
                        const appts = getAppointments(note)
                        appts.forEach(apt => {
                            if (!apt.start) return
                            if (apt.start === ds || (apt.end >= ds && apt.start <= ds)) {
                                const overdue = apt.start < ds && !apt.end
                                events.push({
                                    title: `${note.title} – ${apt.title || 'Termin'}`,
                                    aptTitle: apt.title || 'Termin',
                                    noteTitle: note.title,
                                    time: apt.time || '',
                                    cls: overdue ? 'cal-event-overdue' : 'cal-event-apt',
                                    source: note.title,
                                    noteId: note.id,
                                    departments: getDepartments(note),
                                    start: apt.start,
                                    end: apt.end || '',
                                    endTime: apt.endTime || ''
                                })
                            }
                        })
                        if (note.due_date && note.due_date === ds) {
                            const overdue = note.due_date < dateStr(new Date())
                            events.push({
                                title: `Fällig: ${note.title}`,
                                aptTitle: 'Fällig',
                                noteTitle: note.title,
                                time: '',
                                cls: overdue ? 'cal-event-overdue' : 'cal-event-due',
                                source: note.title,
                                noteId: note.id,
                                departments: getDepartments(note)
                            })
                        }
                    })
                    events.sort((a, b) => (a.time || '').localeCompare(b.time || ''))
                    return events
                }

                const calDayStruct = (date, currentMonth) => {
                    const events = calEventsForDate(date)
                    const ds = dateStr(date)
                    const d = new Date(date)
                    const todayStr = dateStr(new Date())
                    const overdueStr = { cls: 'bg-red-500 rounded-full' }
                    const aptStr = { cls: 'bg-emerald-500 rounded-full' }
                    const dueStr = { cls: 'bg-blue-500 rounded-full' }
                    const dots = events.map(ev => {
                        if (ev.cls === 'cal-event-overdue') return overdueStr
                        if (ev.cls === 'cal-event-apt') return aptStr
                        return dueStr
                    })
                    let frameCls = ''
                    if (events.length > 0) {
                        let prio = 0
                        for (const ev of events) {
                            if (ev.cls === 'cal-event-overdue') prio = Math.max(prio, 3)
                            else if (ev.cls === 'cal-event-apt') prio = Math.max(prio, 2)
                            else prio = Math.max(prio, 1)
                        }
                        frameCls = prio === 3 ? 'ring-1 ring-red-400 dark:ring-red-500' :
                            prio === 2 ? 'ring-1 ring-emerald-400 dark:ring-emerald-500' :
                            'ring-1 ring-blue-400 dark:ring-blue-500'
                    }
                    return {
                        date: d,
                        currentMonth,
                        isToday: ds === todayStr,
                        dots,
                        events,
                        frameCls
                    }
                }

                const calMonthDays = computed(() => {
                    const now = calCursor.value
                    const year = now.getFullYear()
                    const month = now.getMonth()
                    const first = new Date(year, month, 1)
                    const startDay = (first.getDay() + 6) % 7
                    const daysInMonth = new Date(year, month + 1, 0).getDate()
                    const start = new Date(year, month, 1 - startDay)
                    const result = []
                    for (let i = 0; i < 42; i++) {
                        const d = new Date(start)
                        d.setDate(start.getDate() + i)
                        result.push(calDayStruct(d, d.getMonth() === month))
                    }
                    return result
                })

                const calHours = Array.from({ length: 25 }, (_, h) => h)

                const calWeekBlocks = (events, ds) => {
                    const allday = []
                    const timed = []
                    events.forEach(ev => {
                        let startMin = -1
                        let endMin = -1
                        const stime = ev.time || ''
                        if (/^\d{1,2}:\d{2}$/.test(stime)) {
                            const isMulti = ev.start && ev.end && ev.end > ev.start
                            const isStartDay = ds === ev.start
                            const isEndDay = isMulti && ds === ev.end
                            if (isStartDay) {
                                const hm = stime.split(':')
                                startMin = parseInt(hm[0], 10) * 60 + parseInt(hm[1], 10)
                                if (isMulti) {
                                    endMin = 1440
                                } else if (ev.endTime && /^\d{1,2}:\d{2}$/.test(ev.endTime)) {
                                    const ehm = ev.endTime.split(':')
                                    endMin = parseInt(ehm[0], 10) * 60 + parseInt(ehm[1], 10)
                                } else {
                                    endMin = Math.min(1440, startMin + 60)
                                }
                            } else if (isEndDay) {
                                startMin = 0
                                if (ev.endTime && /^\d{1,2}:\d{2}$/.test(ev.endTime)) {
                                    const ehm = ev.endTime.split(':')
                                    endMin = parseInt(ehm[0], 10) * 60 + parseInt(ehm[1], 10)
                                } else {
                                    endMin = 1440
                                }
                            } else if (isMulti) {
                                startMin = 0
                                endMin = 1440
                            } else {
                                const hm = stime.split(':')
                                startMin = parseInt(hm[0], 10) * 60 + parseInt(hm[1], 10)
                                if (ev.endTime && /^\d{1,2}:\d{2}$/.test(ev.endTime)) {
                                    const ehm = ev.endTime.split(':')
                                    endMin = parseInt(ehm[0], 10) * 60 + parseInt(ehm[1], 10)
                                } else {
                                    endMin = Math.min(1440, startMin + 60)
                                }
                            }
                        }
                        if (startMin < 0) {
                            allday.push(ev)
                        } else {
                            timed.push({ ev, startMin, endMin: Math.max(endMin, startMin + 15) })
                        }
                    })
                    // Overlap nesting: assign lanes greedily by start time.
                    timed.sort((a, b) => a.startMin - b.startMin || b.endMin - a.endMin)
                    const lanes = []  // lanes[i] = { endMin }
                    let laneCount = 0
                    for (const t of timed) {
                        let placed = lanes.findIndex(l => t.startMin >= l.endMin)
                        if (placed < 0) {
                            placed = lanes.length
                            lanes.push({ endMin: 0 })
                            laneCount = Math.max(laneCount, placed + 1)
                        }
                        lanes[placed] = { endMin: t.endMin }
                        t.lane = placed
                    }
                    const blocks = timed.map(t => {
                        const widthPct = laneCount > 0 ? (100 / laneCount) : 100
                        return {
                            ev: t.ev,
                            cls: t.ev.cls,
                            title: t.ev.title,
                            aptTitle: t.ev.aptTitle,
                            noteTitle: t.ev.noteTitle,
                            time: (t.startMin > 0 || (t.ev.end && t.ev.end === ds)) ? t.ev.time : '',
                            noteId: t.ev.noteId,
                            departments: t.ev.departments || [],
                            startMin: t.startMin,
                            heightMin: t.endMin - t.startMin,
                            lane: t.lane,
                            laneCount,
                            leftPct: (t.lane / laneCount) * 100,
                            widthPct
                        }
                    })
                    return { allday, blocks, laneCount }
                }

                const calWeekDays = computed(() => {
                    const now = calCursor.value
                    const dayOfWeek = (now.getDay() + 6) % 7
                    const start = new Date(now)
                    start.setDate(now.getDate() - dayOfWeek)
                    const names = ['Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa', 'So']
                    const result = []
                    for (let i = 0; i < 7; i++) {
                        const d = new Date(start)
                        d.setDate(start.getDate() + i)
                        const ds = dateStr(d)
                        const events = calEventsForDate(d)
                        const { allday, blocks, laneCount } = calWeekBlocks(events, ds)
                        result.push({
                            date: d,
                            isToday: ds === dateStr(new Date()),
                            names: names[i],
                            events,
                            allday,
                            blocks,
                            laneCount,
                            dateStr: ds
                        })
                    }
                    return result
                })

                // Week view: auto-scroll the timeline to the current hour on (re)mount of the view
                const calWeekScrollRef = ref(null)
                const scrollWeekToNow = () => {
                    const apply = () => {
                        if (calWeekScrollRef.value) {
                            const h = new Date().getHours() + 1
                            calWeekScrollRef.value.scrollTop = Math.max(0, h * 36 - 100)
                        }
                    }
                    nextTick(apply)
                    requestAnimationFrame(apply)
                    setTimeout(apply, 250)
                }

                // Month view: drag & drop to move appointments between days
                const calDragEv = ref(null)
                const addDaysToISO = (iso, delta) => {
                    const d = new Date(iso + 'T00:00:00')
                    d.setDate(d.getDate() + delta)
                    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
                }
                const calEventDragStart = (ev, e) => {
                    if (!ev.start) return
                    calDragEv.value = { noteId: ev.noteId, aptTitle: ev.aptTitle, start: ev.start, end: ev.end || '' }
                    try { e.dataTransfer.setData('text/plain', ev.title || '') } catch (e2) {}
                }
                const calDayDrop = async (day) => {
                    const drag = calDragEv.value
                    calDragEv.value = null
                    if (!drag) return
                    const targetStr = dateStr(day.date)
                    if (drag.start === targetStr) return
                    const delta = Math.round((new Date(targetStr + 'T00:00:00') - new Date(drag.start + 'T00:00:00')) / 86400000)
                    if (delta === 0) return
                    const note = notes.value.find(n => n.id === drag.noteId)
                    if (!note) return
                    const appts = getAppointments(note)
                    const apt = appts.find(a => (a.title || '') === drag.aptTitle && (a.start || '') === drag.start)
                    if (!apt) return
                    apt.start = addDaysToISO(apt.start, delta)
                    if (apt.end) apt.end = addDaysToISO(apt.end, delta)
                    const res = await fetch(`/api/notes/${note.id}`, {
                        method: 'PUT',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ appointments: JSON.stringify(appts) })
                    })
                    if (res.ok) {
                        note.appointments = JSON.stringify(appts)
                        collectOverdueReminders()
                    }
                }

                const calYearMonths = computed(() => {
                    const year = calCursor.value.getFullYear()
                    const months = []
                    for (let m = 0; m < 12; m++) {
                        const first = new Date(year, m, 1)
                        const startDay = (first.getDay() + 6) % 7
                        const daysInMonth = new Date(year, m + 1, 0).getDate()
                        const start = new Date(year, m, 1 - startDay)
                        const days = []
                        for (let i = 0; i < 42; i++) {
                            const d = new Date(start)
                            d.setDate(start.getDate() + i)
                            days.push(calDayStruct(d, d.getMonth() === m))
                        }
                        months.push({ name: first.toLocaleDateString('de-DE', { month: 'long' }), days })
                    }
                    return months
                })

                const calTitle = computed(() => {
                    if (calViewMode.value === 'month') {
                        return calCursor.value.toLocaleDateString('de-DE', { month: 'long', year: 'numeric' })
                    } else if (calViewMode.value === 'week') {
                        const start = calWeekDays.value[0]
                        const end = calWeekDays.value[6]
                        if (start.date.getMonth() === end.date.getMonth()) {
                            return `${start.date.getDate()}. – ${end.date.getDate()}. ${end.date.toLocaleDateString('de-DE', { month: 'long', year: 'numeric' })}`
                        }
                        return `${start.date.getDate()}. ${start.date.toLocaleDateString('de-DE', { month: 'short' })} – ${end.date.getDate()}. ${end.date.toLocaleDateString('de-DE', { month: 'short', year: 'numeric' })}`
                    }
                    return String(calCursor.value.getFullYear())
                })

                const calToday = () => { calCursor.value = new Date() }

                const calNavigate = (dir) => {
                    const c = new Date(calCursor.value)
                    if (calViewMode.value === 'month') c.setMonth(c.getMonth() + dir)
                    else if (calViewMode.value === 'week') c.setDate(c.getDate() + (7 * dir))
                    else if (calViewMode.value === 'year') c.setFullYear(c.getFullYear() + dir)
                    calCursor.value = c
                }

                const calDayClick = (day) => {
                    calSelectedDay.value = day.date
                }

                const calSelectedDayTitle = computed(() => {
                    if (!calSelectedDay.value) return ''
                    return calSelectedDay.value.toLocaleDateString('de-DE', { weekday: 'long', day: '2-digit', month: 'long', year: 'numeric' })
                })

                const calSelectedDayEvents = computed(() => {
                    if (!calSelectedDay.value) return []
                    return calEventsForDate(calSelectedDay.value)
                })

                const boardVisibility = ref('all')
                const boardDept = ref('')

                const getNotesByColumn = (status) => {
                    let filtered = notes.value.filter(note => note.status === status)
                    if (boardVisibility.value === 'overdue') filtered = filtered.filter(isOverdue)
                    else if (boardVisibility.value === 'due') filtered = filtered.filter(note => note.due_date)
                    else if (boardVisibility.value === 'nodate') filtered = filtered.filter(note => !note.due_date)
                    if (boardDept.value) filtered = filtered.filter(note => getDepartments(note).includes(boardDept.value))
                    filtered.sort((a, b) => (b.pinned || 0) - (a.pinned || 0) || (a.sort_order || 0) - (b.sort_order || 0) || a.id - b.id)
                    if (searchQuery.value.trim() !== '') {
                        const q = parseSearchQuery(searchQuery.value)
                        filtered = filtered.filter(note => matchesNote(note, q))
                    }
                    return filtered
                }

                const togglePin = async (note) => {
                    const newVal = !note.pinned
                    note.pinned = newVal ? 1 : 0
                    try {
                        const res = await fetch(`/api/notes/${note.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ pinned: newVal })
                        })
                        if (!res.ok) {
                            note.pinned = newVal ? 0 : 1
                        }
                    } catch (e) {
                        note.pinned = newVal ? 0 : 1
                        console.error('Fehler beim Pinnen', e)
                    }
                }

                const startDrag = (note) => {
                    draggedNote.value = note
                }

                const onDrop = async (columnId, targetNote) => {
                    if (!draggedNote.value) return
                    const dragged = draggedNote.value

                    if (targetNote && dragged.id === targetNote.id) {
                        draggedNote.value = null
                        return
                    }

                    if (dragged.status === columnId && targetNote) {
                        // Reorder within the same column
                        await reorderColumn(columnId, dragged, targetNote)
                    } else if (dragged.status !== columnId) {
                        // Cross-column move, append to end
                        dragged.status = columnId
                        const maxSort = notes.value
                            .filter(n => n.status === columnId && n.id !== dragged.id)
                            .reduce((max, n) => Math.max(max, n.sort_order || 0), 0)
                        dragged.sort_order = maxSort + 1
                        await fetch(`/api/notes/${dragged.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: dragged.title,
                                content: dragged.content,
                                status: dragged.status,
                                priority: dragged.priority,
                                due_date: dragged.due_date,
                                sort_order: dragged.sort_order
                            })
                        })
                        if (targetNote && columnId !== 'archived') {
                            await reorderColumn(columnId, dragged, targetNote)
                        }
                    }
                    draggedNote.value = null
                    collectOverdueReminders()
                }

                return {
                    activeView,
                    isDark,
                    toggleTheme,
                    isSettingsOpen,
                    autoArchiveEnabled,
                    autoArchiveDay,
                    reminderOpen,
                    overdueReminders,
                    openNoteFromReminder,
                    timerOpen,
                    timerTotal,
                    timerRemaining,
                    timerRunning,
                    fmtTimer,
                    timerSet,
                    timerToggle,
                    timerReset,
                    newNoteTitle,
                    newNotePriority,
                    newNoteDueDate,
                    newNoteRepeatRule,
                    newNoteStatus,
                    newNoteContent,
                    newNoteChecklist,
                    newNoteStepText,
                    newNoteDepartment,
                    newNoteDepartments,
                    newNoteDeptInput,
                    newNoteDepartmentInputRef,
                    modalDepartmentInputRef,
                    newNoteAppointments,
                    newAptTitle,
                    newAptStart,
                    newAptEnd,
                    newAptTime,
                    newAptEndTime,
                    newAptHasEnd,
                    addNewAppointment,
                    addModalAppointment,
                    activeNoteAppointments,
                    activeNoteDepartments,
                    modalDeptInput,
                    modalAptTitle,
                    modalAptStart,
                    modalAptEnd,
                    modalAptTime,
                    modalAptEndTime,
                    modalAptHasEnd,
                    removeNewNoteDepartment,
                    removeActiveNoteDepartment,
                    addCustomDepartment,
                    getDepartments,
                    getAppointments,
                    getChecklistProgress,
                    isNewNoteOpen,
                    newNoteTitleInputRef,
                    newNoteTextareaRef,
                    newNoteCaretMirrorRef,
                    newNoteStepInputRef,
                    columns,
                    notes,
                    notesById,
                    isModalOpen,
                    isPreviewMode,
                    activeNote,
                    activeChecklist,
                    newStepText,
                    isBacklogCollapsed,
                    isArchiveCollapsed,
                    collapsedStatuses,
                    expandedColumns,
                    isCollapsed,
                    toggleCollapse,
                    expandColumn,
                    collapseColumn,
                    gridColsStyle,
                    searchQuery,
                    searchInputRef,
                    textareaRef,
                    caretMirrorRef,
                    isOverdue,
                    isDueSoon,
                    renderedMarkdown,
                    onPreviewClick,
                    contextMenu,
                    exportMenuOpen,
                    openContextMenu,
                    closeContextMenu,
                    applyFormat,
                    applyHtmlColor,
                    openNewNote,
                    closeNewNote,
                    createNote,
                    addNewNoteStep,
                    removeNewNoteStep,
                    handleNewNoteEnter,
                    cancelAutocomplete,
                    exportJson,
                    exportCsv,
                    exportFullBackup,
                    startImport,
                    onImportFile,
                    importFileInputRef,
                    trashNotes,
                    trashContacts,
                    fetchTrash,
                    restoreNote,
                    restoreContact,
                    forceDeleteNote,
                    forceDeleteContact,
                    clearTrash,
                    openModal,
                    closeModal,
                    addStep,
                    removeStep,
                    saveActiveNote,
                    deleteNote,
                    duplicateNote,
                    archiveNote,
                    unarchiveNote,
                    getNotesByColumn,
                    startDrag,
                    onDragOver,
                    onDrop,
                    boardVisibility,
                    boardDept,
                    togglePin,
                    handleAutocomplete,
                    handleAutocompleteKeydown,
                    selectAutocomplete,
                    showAutocomplete,
                    autocompleteResults,
                    autocompleteIndex,
                    autocompletePos,
                    updateAutocompletePos,
                    handleNoteLinkClick,
                    focusSearchInput,
                    processNoteLinks,
                    departments,
                    filteredDepartments,
                    deptDropdownOpen,
                    deptDropdownSource,
                    deptIndex,
                    openDeptDropdown,
                    closeDeptDropdown,
                    selectDepartment,
                    onDeptInput,
                    handleDeptKeydown,
                    calViewMode,
                    calCursor,
                    calSelectedDay,
                    calMonthDays,
                    calWeekDays,
                    calYearMonths,
                    calHours,
                    calTitle,
                    calToday,
                    calNavigate,
                    calDayClick,
                    calSelectedDayTitle,
                    calSelectedDayEvents,
                    calWeekScrollRef,
                    scrollWeekToNow,
                    calEventDragStart,
                    calDayDrop,
                    fmtDate,
                    fmtAptRange,
                    dateStr,
                    convertDateToISO,
                    processAddressLinks,
                    handleAddressLinkClick,
                    contactPopover,
                    showContactPopover,
                    hideContactPopover,
                    contacts,
                    contactFilter,
                    contactModalOpen,
                    contactForm,
                    highlightContactId,
                    filteredContacts,
                    fetchContacts,
                    openContactModal,
                    saveContact,
                    deleteContact,
                    deleteDepartment,
                    dashboardTodayEvents,
                    overdueNotes,
                    dashboardUpcoming,
                    dashboardStatusRows,
                    dashboardPriorityRows,
                    dashboardActivity,
                    dashboardActivityDays,
                    dashboardTodo,
                    dashboardDeptRows
                }
            }
        }).mount('#app')
    </script>
</body>
</html>
"#;


const TAILWIND_JS: &str = r##"(()=>{var qv=Object.create;var Hi=Object.defineProperty;var $v=Object.getOwnPropertyDescriptor;var Lv=Object.getOwnPropertyNames;var Mv=Object.getPrototypeOf,Nv=Object.prototype.hasOwnProperty;var df=r=>Hi(r,"__esModule",{value:!0});var hf=r=>{if(typeof require!="undefined")return require(r);throw new Error('Dynamic require of "'+r+'" is not supported')};var P=(r,e)=>()=>(r&&(e=r(r=0)),e);var x=(r,e)=>()=>(e||r((e={exports:{}}).exports,e),e.exports),Ge=(r,e)=>{df(r);for(var t in e)Hi(r,t,{get:e[t],enumerable:!0})},Bv=(r,e,t)=>{if(e&&typeof e=="object"||typeof e=="function")for(let i of Lv(e))!Nv.call(r,i)&&i!=="default"&&Hi(r,i,{get:()=>e[i],enumerable:!(t=$v(e,i))||t.enumerable});return r},pe=r=>Bv(df(Hi(r!=null?qv(Mv(r)):{},"default",r&&r.__esModule&&"default"in r?{get:()=>r.default,enumerable:!0}:{value:r,enumerable:!0})),r);var m,u=P(()=>{m={platform:"",env:{},versions:{node:"14.17.6"}}});var Fv,be,ft=P(()=>{u();Fv=0,be={readFileSync:r=>self[r]||"",statSync:()=>({mtimeMs:Fv++}),promises:{readFile:r=>Promise.resolve(self[r]||"")}}});var Fs=x((oP,gf)=>{u();"use strict";var mf=class{constructor(e={}){if(!(e.maxSize&&e.maxSize>0))throw new TypeError("`maxSize` must be a number greater than 0");if(typeof e.maxAge=="number"&&e.maxAge===0)throw new TypeError("`maxAge` must be a number greater than 0");this.maxSize=e.maxSize,this.maxAge=e.maxAge||1/0,this.onEviction=e.onEviction,this.cache=new Map,this.oldCache=new Map,this._size=0}_emitEvictions(e){if(typeof this.onEviction=="function")for(let[t,i]of e)this.onEviction(t,i.value)}_deleteIfExpired(e,t){return typeof t.expiry=="number"&&t.expiry<=Date.now()?(typeof this.onEviction=="function"&&this.onEviction(e,t.value),this.delete(e)):!1}_getOrDeleteIfExpired(e,t){if(this._deleteIfExpired(e,t)===!1)return t.value}_getItemValue(e,t){return t.expiry?this._getOrDeleteIfExpired(e,t):t.value}_peek(e,t){let i=t.get(e);return this._getItemValue(e,i)}_set(e,t){this.cache.set(e,t),this._size++,this._size>=this.maxSize&&(this._size=0,this._emitEvictions(this.oldCache),this.oldCache=this.cache,this.cache=new Map)}_moveToRecent(e,t){this.oldCache.delete(e),this._set(e,t)}*_entriesAscending(){for(let e of this.oldCache){let[t,i]=e;this.cache.has(t)||this._deleteIfExpired(t,i)===!1&&(yield e)}for(let e of this.cache){let[t,i]=e;this._deleteIfExpired(t,i)===!1&&(yield e)}}get(e){if(this.cache.has(e)){let t=this.cache.get(e);return this._getItemValue(e,t)}if(this.oldCache.has(e)){let t=this.oldCache.get(e);if(this._deleteIfExpired(e,t)===!1)return this._moveToRecent(e,t),t.value}}set(e,t,{maxAge:i=this.maxAge===1/0?void 0:Date.now()+this.maxAge}={}){this.cache.has(e)?this.cache.set(e,{value:t,maxAge:i}):this._set(e,{value:t,expiry:i})}has(e){return this.cache.has(e)?!this._deleteIfExpired(e,this.cache.get(e)):this.oldCache.has(e)?!this._deleteIfExpired(e,this.oldCache.get(e)):!1}peek(e){if(this.cache.has(e))return this._peek(e,this.cache);if(this.oldCache.has(e))return this._peek(e,this.oldCache)}delete(e){let t=this.cache.delete(e);return t&&this._size--,this.oldCache.delete(e)||t}clear(){this.cache.clear(),this.oldCache.clear(),this._size=0}resize(e){if(!(e&&e>0))throw new TypeError("`maxSize` must be a number greater than 0");let t=[...this._entriesAscending()],i=t.length-e;i<0?(this.cache=new Map(t),this.oldCache=new Map,this._size=t.length):(i>0&&this._emitEvictions(t.slice(0,i)),this.oldCache=new Map(t.slice(i)),this.cache=new Map,this._size=0),this.maxSize=e}*keys(){for(let[e]of this)yield e}*values(){for(let[,e]of this)yield e}*[Symbol.iterator](){for(let e of this.cache){let[t,i]=e;this._deleteIfExpired(t,i)===!1&&(yield[t,i.value])}for(let e of this.oldCache){let[t,i]=e;this.cache.has(t)||this._deleteIfExpired(t,i)===!1&&(yield[t,i.value])}}*entriesDescending(){let e=[...this.cache];for(let t=e.length-1;t>=0;--t){let i=e[t],[n,s]=i;this._deleteIfExpired(n,s)===!1&&(yield[n,s.value])}e=[...this.oldCache];for(let t=e.length-1;t>=0;--t){let i=e[t],[n,s]=i;this.cache.has(n)||this._deleteIfExpired(n,s)===!1&&(yield[n,s.value])}}*entriesAscending(){for(let[e,t]of this._entriesAscending())yield[e,t.value]}get size(){if(!this._size)return this.oldCache.size;let e=0;for(let t of this.oldCache.keys())this.cache.has(t)||e++;return Math.min(this._size+e,this.maxSize)}};gf.exports=mf});var yf,bf=P(()=>{u();yf=r=>r&&r._hash});function Wi(r){return yf(r,{ignoreUnknown:!0})}var wf=P(()=>{u();bf()});function xt(r){if(r=`${r}`,r==="0")return"0";if(/^[+-]?(\d+|\d*\.\d+)(e[+-]?\d+)?(%|\w+)?$/.test(r))return r.replace(/^[+-]?/,t=>t==="-"?"":"-");let e=["var","calc","min","max","clamp"];for(let t of e)if(r.includes(`${t}(`))return`calc(${r} * -1)`}var Gi=P(()=>{u()});var vf,xf=P(()=>{u();vf=["preflight","container","accessibility","pointerEvents","visibility","position","inset","isolation","zIndex","order","gridColumn","gridColumnStart","gridColumnEnd","gridRow","gridRowStart","gridRowEnd","float","clear","margin","boxSizing","lineClamp","display","aspectRatio","size","height","maxHeight","minHeight","width","minWidth","maxWidth","flex","flexShrink","flexGrow","flexBasis","tableLayout","captionSide","borderCollapse","borderSpacing","transformOrigin","translate","rotate","skew","scale","transform","animation","cursor","touchAction","userSelect","resize","scrollSnapType","scrollSnapAlign","scrollSnapStop","scrollMargin","scrollPadding","listStylePosition","listStyleType","listStyleImage","appearance","columns","breakBefore","breakInside","breakAfter","gridAutoColumns","gridAutoFlow","gridAutoRows","gridTemplateColumns","gridTemplateRows","flexDirection","flexWrap","placeContent","placeItems","alignContent","alignItems","justifyContent","justifyItems","gap","space","divideWidth","divideStyle","divideColor","divideOpacity","placeSelf","alignSelf","justifySelf","overflow","overscrollBehavior","scrollBehavior","textOverflow","hyphens","whitespace","textWrap","wordBreak","borderRadius","borderWidth","borderStyle","borderColor","borderOpacity","backgroundColor","backgroundOpacity","backgroundImage","gradientColorStops","boxDecorationBreak","backgroundSize","backgroundAttachment","backgroundClip","backgroundPosition","backgroundRepeat","backgroundOrigin","fill","stroke","strokeWidth","objectFit","objectPosition","padding","textAlign","textIndent","verticalAlign","fontFamily","fontSize","fontWeight","textTransform","fontStyle","fontVariantNumeric","lineHeight","letterSpacing","textColor","textOpacity","textDecoration","textDecorationColor","textDecorationStyle","textDecorationThickness","textUnderlineOffset","fontSmoothing","placeholderColor","placeholderOpacity","caretColor","accentColor","opacity","backgroundBlendMode","mixBlendMode","boxShadow","boxShadowColor","outlineStyle","outlineWidth","outlineOffset","outlineColor","ringWidth","ringColor","ringOpacity","ringOffsetWidth","ringOffsetColor","blur","brightness","contrast","dropShadow","grayscale","hueRotate","invert","saturate","sepia","filter","backdropBlur","backdropBrightness","backdropContrast","backdropGrayscale","backdropHueRotate","backdropInvert","backdropOpacity","backdropSaturate","backdropSepia","backdropFilter","transitionProperty","transitionDelay","transitionDuration","transitionTimingFunction","willChange","contain","content","forcedColorAdjust"]});function kf(r,e){return r===void 0?e:Array.isArray(r)?r:[...new Set(e.filter(i=>r!==!1&&r[i]!==!1).concat(Object.keys(r).filter(i=>r[i]!==!1)))]}var Sf=P(()=>{u()});var Af={};Ge(Af,{default:()=>Qe});var Qe,Qi=P(()=>{u();Qe=new Proxy({},{get:()=>String})});function js(r,e,t){typeof m!="undefined"&&m.env.JEST_WORKER_ID||t&&Cf.has(t)||(t&&Cf.add(t),console.warn(""),e.forEach(i=>console.warn(r,"-",i)))}function zs(r){return Qe.dim(r)}var Cf,G,Be=P(()=>{u();Qi();Cf=new Set;G={info(r,e){js(Qe.bold(Qe.cyan("info")),...Array.isArray(r)?[r]:[e,r])},warn(r,e){["content-problems"].includes(r)||js(Qe.bold(Qe.yellow("warn")),...Array.isArray(r)?[r]:[e,r])},risk(r,e){js(Qe.bold(Qe.magenta("risk")),...Array.isArray(r)?[r]:[e,r])}}});var _f={};Ge(_f,{default:()=>Us});function qr({version:r,from:e,to:t}){G.warn(`${e}-color-renamed`,[`As of Tailwind CSS ${r}, \`${e}\` has been renamed to \`${t}\`.`,"Update your configuration file to silence this warning."])}var Us,Vs=P(()=>{u();Be();Us={inherit:"inherit",current:"currentColor",transparent:"transparent",black:"#000",white:"#fff",slate:{50:"#f8fafc",100:"#f1f5f9",200:"#e2e8f0",300:"#cbd5e1",400:"#94a3b8",500:"#64748b",600:"#475569",700:"#334155",800:"#1e293b",900:"#0f172a",950:"#020617"},gray:{50:"#f9fafb",100:"#f3f4f6",200:"#e5e7eb",300:"#d1d5db",400:"#9ca3af",500:"#6b7280",600:"#4b5563",700:"#374151",800:"#1f2937",900:"#111827",950:"#030712"},zinc:{50:"#fafafa",100:"#f4f4f5",200:"#e4e4e7",300:"#d4d4d8",400:"#a1a1aa",500:"#71717a",600:"#52525b",700:"#3f3f46",800:"#27272a",900:"#18181b",950:"#09090b"},neutral:{50:"#fafafa",100:"#f5f5f5",200:"#e5e5e5",300:"#d4d4d4",400:"#a3a3a3",500:"#737373",600:"#525252",700:"#404040",800:"#262626",900:"#171717",950:"#0a0a0a"},stone:{50:"#fafaf9",100:"#f5f5f4",200:"#e7e5e4",300:"#d6d3d1",400:"#a8a29e",500:"#78716c",600:"#57534e",700:"#44403c",800:"#292524",900:"#1c1917",950:"#0c0a09"},red:{50:"#fef2f2",100:"#fee2e2",200:"#fecaca",300:"#fca5a5",400:"#f87171",500:"#ef4444",600:"#dc2626",700:"#b91c1c",800:"#991b1b",900:"#7f1d1d",950:"#450a0a"},orange:{50:"#fff7ed",100:"#ffedd5",200:"#fed7aa",300:"#fdba74",400:"#fb923c",500:"#f97316",600:"#ea580c",700:"#c2410c",800:"#9a3412",900:"#7c2d12",950:"#431407"},amber:{50:"#fffbeb",100:"#fef3c7",200:"#fde68a",300:"#fcd34d",400:"#fbbf24",500:"#f59e0b",600:"#d97706",700:"#b45309",800:"#92400e",900:"#78350f",950:"#451a03"},yellow:{50:"#fefce8",100:"#fef9c3",200:"#fef08a",300:"#fde047",400:"#facc15",500:"#eab308",600:"#ca8a04",700:"#a16207",800:"#854d0e",900:"#713f12",950:"#422006"},lime:{50:"#f7fee7",100:"#ecfccb",200:"#d9f99d",300:"#bef264",400:"#a3e635",500:"#84cc16",600:"#65a30d",700:"#4d7c0f",800:"#3f6212",900:"#365314",950:"#1a2e05"},green:{50:"#f0fdf4",100:"#dcfce7",200:"#bbf7d0",300:"#86efac",400:"#4ade80",500:"#22c55e",600:"#16a34a",700:"#15803d",800:"#166534",900:"#14532d",950:"#052e16"},emerald:{50:"#ecfdf5",100:"#d1fae5",200:"#a7f3d0",300:"#6ee7b7",400:"#34d399",500:"#10b981",600:"#059669",700:"#047857",800:"#065f46",900:"#064e3b",950:"#022c22"},teal:{50:"#f0fdfa",100:"#ccfbf1",200:"#99f6e4",300:"#5eead4",400:"#2dd4bf",500:"#14b8a6",600:"#0d9488",700:"#0f766e",800:"#115e59",900:"#134e4a",950:"#042f2e"},cyan:{50:"#ecfeff",100:"#cffafe",200:"#a5f3fc",300:"#67e8f9",400:"#22d3ee",500:"#06b6d4",600:"#0891b2",700:"#0e7490",800:"#155e75",900:"#164e63",950:"#083344"},sky:{50:"#f0f9ff",100:"#e0f2fe",200:"#bae6fd",300:"#7dd3fc",400:"#38bdf8",500:"#0ea5e9",600:"#0284c7",700:"#0369a1",800:"#075985",900:"#0c4a6e",950:"#082f49"},blue:{50:"#eff6ff",100:"#dbeafe",200:"#bfdbfe",300:"#93c5fd",400:"#60a5fa",500:"#3b82f6",600:"#2563eb",700:"#1d4ed8",800:"#1e40af",900:"#1e3a8a",950:"#172554"},indigo:{50:"#eef2ff",100:"#e0e7ff",200:"#c7d2fe",300:"#a5b4fc",400:"#818cf8",500:"#6366f1",600:"#4f46e5",700:"#4338ca",800:"#3730a3",900:"#312e81",950:"#1e1b4b"},violet:{50:"#f5f3ff",100:"#ede9fe",200:"#ddd6fe",300:"#c4b5fd",400:"#a78bfa",500:"#8b5cf6",600:"#7c3aed",700:"#6d28d9",800:"#5b21b6",900:"#4c1d95",950:"#2e1065"},purple:{50:"#faf5ff",100:"#f3e8ff",200:"#e9d5ff",300:"#d8b4fe",400:"#c084fc",500:"#a855f7",600:"#9333ea",700:"#7e22ce",800:"#6b21a8",900:"#581c87",950:"#3b0764"},fuchsia:{50:"#fdf4ff",100:"#fae8ff",200:"#f5d0fe",300:"#f0abfc",400:"#e879f9",500:"#d946ef",600:"#c026d3",700:"#a21caf",800:"#86198f",900:"#701a75",950:"#4a044e"},pink:{50:"#fdf2f8",100:"#fce7f3",200:"#fbcfe8",300:"#f9a8d4",400:"#f472b6",500:"#ec4899",600:"#db2777",700:"#be185d",800:"#9d174d",900:"#831843",950:"#500724"},rose:{50:"#fff1f2",100:"#ffe4e6",200:"#fecdd3",300:"#fda4af",400:"#fb7185",500:"#f43f5e",600:"#e11d48",700:"#be123c",800:"#9f1239",900:"#881337",950:"#4c0519"},get lightBlue(){return qr({version:"v2.2",from:"lightBlue",to:"sky"}),this.sky},get warmGray(){return qr({version:"v3.0",from:"warmGray",to:"stone"}),this.stone},get trueGray(){return qr({version:"v3.0",from:"trueGray",to:"neutral"}),this.neutral},get coolGray(){return qr({version:"v3.0",from:"coolGray",to:"gray"}),this.gray},get blueGray(){return qr({version:"v3.0",from:"blueGray",to:"slate"}),this.slate}}});function Hs(r,...e){for(let t of e){for(let i in t)r?.hasOwnProperty?.(i)||(r[i]=t[i]);for(let i of Object.getOwnPropertySymbols(t))r?.hasOwnProperty?.(i)||(r[i]=t[i])}return r}var Ef=P(()=>{u()});function kt(r){if(Array.isArray(r))return r;let e=r.split("[").length-1,t=r.split("]").length-1;if(e!==t)throw new Error(`Path is invalid. Has unbalanced brackets: ${r}`);return r.split(/\.(?![^\[]*\])|[\[\]]/g).filter(Boolean)}var Yi=P(()=>{u()});function we(r,e){return Ki.future.includes(e)?r.future==="all"||(r?.future?.[e]??Of[e]??!1):Ki.experimental.includes(e)?r.experimental==="all"||(r?.experimental?.[e]??Of[e]??!1):!1}function Tf(r){return r.experimental==="all"?Ki.experimental:Object.keys(r?.experimental??{}).filter(e=>Ki.experimental.includes(e)&&r.experimental[e])}function Rf(r){if(m.env.JEST_WORKER_ID===void 0&&Tf(r).length>0){let e=Tf(r).map(t=>Qe.yellow(t)).join(", ");G.warn("experimental-flags-enabled",[`You have enabled experimental features: ${e}`,"Experimental features in Tailwind CSS are not covered by semver, may introduce breaking changes, and can change at any time."])}}var Of,Ki,ct=P(()=>{u();Qi();Be();Of={optimizeUniversalDefaults:!1,generalizedModifiers:!0,disableColorOpacityUtilitiesByDefault:!1,relativeContentPathsByDefault:!1},Ki={future:["hoverOnlyWhenSupported","respectDefaultRingColorOpacity","disableColorOpacityUtilitiesByDefault","relativeContentPathsByDefault"],experimental:["optimizeUniversalDefaults","generalizedModifiers"]}});function Pf(r){(()=>{if(r.purge||!r.content||!Array.isArray(r.content)&&!(typeof r.content=="object"&&r.content!==null))return!1;if(Array.isArray(r.content))return r.content.every(t=>typeof t=="string"?!0:!(typeof t?.raw!="string"||t?.extension&&typeof t?.extension!="string"));if(typeof r.content=="object"&&r.content!==null){if(Object.keys(r.content).some(t=>!["files","relative","extract","transform"].includes(t)))return!1;if(Array.isArray(r.content.files)){if(!r.content.files.every(t=>typeof t=="string"?!0:!(typeof t?.raw!="string"||t?.extension&&typeof t?.extension!="string")))return!1;if(typeof r.content.extract=="object"){for(let t of Object.values(r.content.extract))if(typeof t!="function")return!1}else if(!(r.content.extract===void 0||typeof r.content.extract=="function"))return!1;if(typeof r.content.transform=="object"){for(let t of Object.values(r.content.transform))if(typeof t!="function")return!1}else if(!(r.content.transform===void 0||typeof r.content.transform=="function"))return!1;if(typeof r.content.relative!="boolean"&&typeof r.content.relative!="undefined")return!1}return!0}return!1})()||G.warn("purge-deprecation",["The `purge`/`content` options have changed in Tailwind CSS v3.0.","Update your configuration file to eliminate this warning.","https://tailwindcss.com/docs/upgrade-guide#configure-content-sources"]),r.safelist=(()=>{let{content:t,purge:i,safelist:n}=r;return Array.isArray(n)?n:Array.isArray(t?.safelist)?t.safelist:Array.isArray(i?.safelist)?i.safelist:Array.isArray(i?.options?.safelist)?i.options.safelist:[]})(),r.blocklist=(()=>{let{blocklist:t}=r;if(Array.isArray(t)){if(t.every(i=>typeof i=="string"))return t;G.warn("blocklist-invalid",["The `blocklist` option must be an array of strings.","https://tailwindcss.com/docs/content-configuration#discarding-classes"])}return[]})(),typeof r.prefix=="function"?(G.warn("prefix-function",["As of Tailwind CSS v3.0, `prefix` cannot be a function.","Update `prefix` in your configuration to be a string to eliminate this warning.","https://tailwindcss.com/docs/upgrade-guide#prefix-cannot-be-a-function"]),r.prefix=""):r.prefix=r.prefix??"",r.content={relative:(()=>{let{content:t}=r;return t?.relative?t.relative:we(r,"relativeContentPathsByDefault")})(),files:(()=>{let{content:t,purge:i}=r;return Array.isArray(i)?i:Array.isArray(i?.content)?i.content:Array.isArray(t)?t:Array.isArray(t?.content)?t.content:Array.isArray(t?.files)?t.files:[]})(),extract:(()=>{let t=(()=>r.purge?.extract?r.purge.extract:r.content?.extract?r.content.extract:r.purge?.extract?.DEFAULT?r.purge.extract.DEFAULT:r.content?.extract?.DEFAULT?r.content.extract.DEFAULT:r.purge?.options?.extractors?r.purge.options.extractors:r.content?.options?.extractors?r.content.options.extractors:{})(),i={},n=(()=>{if(r.purge?.options?.defaultExtractor)return r.purge.options.defaultExtractor;if(r.content?.options?.defaultExtractor)return r.content.options.defaultExtractor})();if(n!==void 0&&(i.DEFAULT=n),typeof t=="function")i.DEFAULT=t;else if(Array.isArray(t))for(let{extensions:s,extractor:a}of t??[])for(let o of s)i[o]=a;else typeof t=="object"&&t!==null&&Object.assign(i,t);return i})(),transform:(()=>{let t=(()=>r.purge?.transform?r.purge.transform:r.content?.transform?r.content.transform:r.purge?.transform?.DEFAULT?r.purge.transform.DEFAULT:r.content?.transform?.DEFAULT?r.content.transform.DEFAULT:{})(),i={};return typeof t=="function"?i.DEFAULT=t:typeof t=="object"&&t!==null&&Object.assign(i,t),i})()};for(let t of r.content.files)if(typeof t=="string"&&/{([^,]*?)}/g.test(t)){G.warn("invalid-glob-braces",[`The glob pattern ${zs(t)} in your Tailwind CSS configuration is invalid.`,`Update it to ${zs(t.replace(/{([^,]*?)}/g,"$1"))} to silence this warning.`]);break}return r}var If=P(()=>{u();ct();Be()});function ke(r){if(Object.prototype.toString.call(r)!=="[object Object]")return!1;let e=Object.getPrototypeOf(r);return e===null||Object.getPrototypeOf(e)===null}var Kt=P(()=>{u()});function St(r){return Array.isArray(r)?r.map(e=>St(e)):typeof r=="object"&&r!==null?Object.fromEntries(Object.entries(r).map(([e,t])=>[e,St(t)])):r}var Xi=P(()=>{u()});function jt(r){return r.replace(/\\,/g,"\\2c ")}var Zi=P(()=>{u()});var Ws,Df=P(()=>{u();Ws={aliceblue:[240,248,255],antiquewhite:[250,235,215],aqua:[0,255,255],aquamarine:[127,255,212],azure:[240,255,255],beige:[245,245,220],bisque:[255,228,196],black:[0,0,0],blanchedalmond:[255,235,205],blue:[0,0,255],blueviolet:[138,43,226],brown:[165,42,42],burlywood:[222,184,135],cadetblue:[95,158,160],chartreuse:[127,255,0],chocolate:[210,105,30],coral:[255,127,80],cornflowerblue:[100,149,237],cornsilk:[255,248,220],crimson:[220,20,60],cyan:[0,255,255],darkblue:[0,0,139],darkcyan:[0,139,139],darkgoldenrod:[184,134,11],darkgray:[169,169,169],darkgreen:[0,100,0],darkgrey:[169,169,169],darkkhaki:[189,183,107],darkmagenta:[139,0,139],darkolivegreen:[85,107,47],darkorange:[255,140,0],darkorchid:[153,50,204],darkred:[139,0,0],darksalmon:[233,150,122],darkseagreen:[143,188,143],darkslateblue:[72,61,139],darkslategray:[47,79,79],darkslategrey:[47,79,79],darkturquoise:[0,206,209],darkviolet:[148,0,211],deeppink:[255,20,147],deepskyblue:[0,191,255],dimgray:[105,105,105],dimgrey:[105,105,105],dodgerblue:[30,144,255],firebrick:[178,34,34],floralwhite:[255,250,240],forestgreen:[34,139,34],fuchsia:[255,0,255],gainsboro:[220,220,220],ghostwhite:[248,248,255],gold:[255,215,0],goldenrod:[218,165,32],gray:[128,128,128],green:[0,128,0],greenyellow:[173,255,47],grey:[128,128,128],honeydew:[240,255,240],hotpink:[255,105,180],indianred:[205,92,92],indigo:[75,0,130],ivory:[255,255,240],khaki:[240,230,140],lavender:[230,230,250],lavenderblush:[255,240,245],lawngreen:[124,252,0],lemonchiffon:[255,250,205],lightblue:[173,216,230],lightcoral:[240,128,128],lightcyan:[224,255,255],lightgoldenrodyellow:[250,250,210],lightgray:[211,211,211],lightgreen:[144,238,144],lightgrey:[211,211,211],lightpink:[255,182,193],lightsalmon:[255,160,122],lightseagreen:[32,178,170],lightskyblue:[135,206,250],lightslategray:[119,136,153],lightslategrey:[119,136,153],lightsteelblue:[176,196,222],lightyellow:[255,255,224],lime:[0,255,0],limegreen:[50,205,50],linen:[250,240,230],magenta:[255,0,255],maroon:[128,0,0],mediumaquamarine:[102,205,170],mediumblue:[0,0,205],mediumorchid:[186,85,211],mediumpurple:[147,112,219],mediumseagreen:[60,179,113],mediumslateblue:[123,104,238],mediumspringgreen:[0,250,154],mediumturquoise:[72,209,204],mediumvioletred:[199,21,133],midnightblue:[25,25,112],mintcream:[245,255,250],mistyrose:[255,228,225],moccasin:[255,228,181],navajowhite:[255,222,173],navy:[0,0,128],oldlace:[253,245,230],olive:[128,128,0],olivedrab:[107,142,35],orange:[255,165,0],orangered:[255,69,0],orchid:[218,112,214],palegoldenrod:[238,232,170],palegreen:[152,251,152],paleturquoise:[175,238,238],palevioletred:[219,112,147],papayawhip:[255,239,213],peachpuff:[255,218,185],peru:[205,133,63],pink:[255,192,203],plum:[221,160,221],powderblue:[176,224,230],purple:[128,0,128],rebeccapurple:[102,51,153],red:[255,0,0],rosybrown:[188,143,143],royalblue:[65,105,225],saddlebrown:[139,69,19],salmon:[250,128,114],sandybrown:[244,164,96],seagreen:[46,139,87],seashell:[255,245,238],sienna:[160,82,45],silver:[192,192,192],skyblue:[135,206,235],slateblue:[106,90,205],slategray:[112,128,144],slategrey:[112,128,144],snow:[255,250,250],springgreen:[0,255,127],steelblue:[70,130,180],tan:[210,180,140],teal:[0,128,128],thistle:[216,191,216],tomato:[255,99,71],turquoise:[64,224,208],violet:[238,130,238],wheat:[245,222,179],white:[255,255,255],whitesmoke:[245,245,245],yellow:[255,255,0],yellowgreen:[154,205,50]}});function $r(r,{loose:e=!1}={}){if(typeof r!="string")return null;if(r=r.trim(),r==="transparent")return{mode:"rgb",color:["0","0","0"],alpha:"0"};if(r in Ws)return{mode:"rgb",color:Ws[r].map(s=>s.toString())};let t=r.replace(zv,(s,a,o,l,c)=>["#",a,a,o,o,l,l,c?c+c:""].join("")).match(jv);if(t!==null)return{mode:"rgb",color:[parseInt(t[1],16),parseInt(t[2],16),parseInt(t[3],16)].map(s=>s.toString()),alpha:t[4]?(parseInt(t[4],16)/255).toString():void 0};let i=r.match(Uv)??r.match(Vv);if(i===null)return null;let n=[i[2],i[3],i[4]].filter(Boolean).map(s=>s.toString());return n.length===2&&n[0].startsWith("var(")?{mode:i[1],color:[n[0]],alpha:n[1]}:!e&&n.length!==3||n.length<3&&!n.some(s=>/^var\(.*?\)$/.test(s))?null:{mode:i[1],color:n,alpha:i[5]?.toString?.()}}function Gs({mode:r,color:e,alpha:t}){let i=t!==void 0;return r==="rgba"||r==="hsla"?`${r}(${e.join(", ")}${i?`, ${t}`:""})`:`${r}(${e.join(" ")}${i?` / ${t}`:""})`}var jv,zv,At,Ji,qf,Ct,Uv,Vv,Qs=P(()=>{u();Df();jv=/^#([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})?$/i,zv=/^#([a-f\d])([a-f\d])([a-f\d])([a-f\d])?$/i,At=/(?:\d+|\d*\.\d+)%?/,Ji=/(?:\s*,\s*|\s+)/,qf=/\s*[,/]\s*/,Ct=/var\(--(?:[^ )]*?)(?:,(?:[^ )]*?|var\(--[^ )]*?\)))?\)/,Uv=new RegExp(`^(rgba?)\\(\\s*(${At.source}|${Ct.source})(?:${Ji.source}(${At.source}|${Ct.source}))?(?:${Ji.source}(${At.source}|${Ct.source}))?(?:${qf.source}(${At.source}|${Ct.source}))?\\s*\\)$`),Vv=new RegExp(`^(hsla?)\\(\\s*((?:${At.source})(?:deg|rad|grad|turn)?|${Ct.source})(?:${Ji.source}(${At.source}|${Ct.source}))?(?:${Ji.source}(${At.source}|${Ct.source}))?(?:${qf.source}(${At.source}|${Ct.source}))?\\s*\\)$`)});function Je(r,e,t){if(typeof r=="function")return r({opacityValue:e});let i=$r(r,{loose:!0});return i===null?t:Gs({...i,alpha:e})}function Ae({color:r,property:e,variable:t}){let i=[].concat(e);if(typeof r=="function")return{[t]:"1",...Object.fromEntries(i.map(s=>[s,r({opacityVariable:t,opacityValue:`var(${t}, 1)`})]))};let n=$r(r);return n===null?Object.fromEntries(i.map(s=>[s,r])):n.alpha!==void 0?Object.fromEntries(i.map(s=>[s,r])):{[t]:"1",...Object.fromEntries(i.map(s=>[s,Gs({...n,alpha:`var(${t}, 1)`})]))}}var Lr=P(()=>{u();Qs()});function ve(r,e){let t=[],i=[],n=0,s=!1;for(let a=0;a<r.length;a++){let o=r[a];t.length===0&&o===e[0]&&!s&&(e.length===1||r.slice(a,a+e.length)===e)&&(i.push(r.slice(n,a)),n=a+e.length),s=s?!1:o==="\\",o==="("||o==="["||o==="{"?t.push(o):(o===")"&&t[t.length-1]==="("||o==="]"&&t[t.length-1]==="["||o==="}"&&t[t.length-1]==="{")&&t.pop()}return i.push(r.slice(n)),i}var zt=P(()=>{u()});function en(r){return ve(r,",").map(t=>{let i=t.trim(),n={raw:i},s=i.split(Wv),a=new Set;for(let o of s)$f.lastIndex=0,!a.has("KEYWORD")&&Hv.has(o)?(n.keyword=o,a.add("KEYWORD")):$f.test(o)?a.has("X")?a.has("Y")?a.has("BLUR")?a.has("SPREAD")||(n.spread=o,a.add("SPREAD")):(n.blur=o,a.add("BLUR")):(n.y=o,a.add("Y")):(n.x=o,a.add("X")):n.color?(n.unknown||(n.unknown=[]),n.unknown.push(o)):n.color=o;return n.valid=n.x!==void 0&&n.y!==void 0,n})}function Lf(r){return r.map(e=>e.valid?[e.keyword,e.x,e.y,e.blur,e.spread,e.color].filter(Boolean).join(" "):e.raw).join(", ")}var Hv,Wv,$f,Ys=P(()=>{u();zt();Hv=new Set(["inset","inherit","initial","revert","unset"]),Wv=/\ +(?![^(]*\))/g,$f=/^-?(\d+|\.\d+)(.*?)$/g});function Ks(r){return Gv.some(e=>new RegExp(`^${e}\\(.*\\)`).test(r))}function K(r,e=null,t=!0){let i=e&&Qv.has(e.property);return r.startsWith("--")&&!i?`var(${r})`:r.includes("url(")?r.split(/(url\(.*?\))/g).filter(Boolean).map(n=>/^url\(.*?\)$/.test(n)?n:K(n,e,!1)).join(""):(r=r.replace(/([^\\])_+/g,(n,s)=>s+" ".repeat(n.length-1)).replace(/^_/g," ").replace(/\\_/g,"_"),t&&(r=r.trim()),r=Yv(r),r)}function Ye(r){return r.includes("=")&&(r=r.replace(/(=.*)/g,(e,t)=>{if(t[1]==="'"||t[1]==='"')return t;if(t.length>2){let i=t[t.length-1];if(t[t.length-2]===" "&&(i==="i"||i==="I"||i==="s"||i==="S"))return`="${t.slice(1,-2)}" ${t[t.length-1]}`}return`="${t.slice(1)}"`})),r}function Yv(r){let e=["theme"],t=["min-content","max-content","fit-content","safe-area-inset-top","safe-area-inset-right","safe-area-inset-bottom","safe-area-inset-left","titlebar-area-x","titlebar-area-y","titlebar-area-width","titlebar-area-height","keyboard-inset-top","keyboard-inset-right","keyboard-inset-bottom","keyboard-inset-left","keyboard-inset-width","keyboard-inset-height","radial-gradient","linear-gradient","conic-gradient","repeating-radial-gradient","repeating-linear-gradient","repeating-conic-gradient","anchor-size"];return r.replace(/(calc|min|max|clamp)\(.+\)/g,i=>{let n="";function s(){let a=n.trimEnd();return a[a.length-1]}for(let a=0;a<i.length;a++){let o=function(f){return f.split("").every((d,p)=>i[a+p]===d)},l=function(f){let d=1/0;for(let h of f){let b=i.indexOf(h,a);b!==-1&&b<d&&(d=b)}let p=i.slice(a,d);return a+=p.length-1,p},c=i[a];if(o("var"))n+=l([")",","]);else if(t.some(f=>o(f))){let f=t.find(d=>o(d));n+=f,a+=f.length-1}else e.some(f=>o(f))?n+=l([")"]):o("[")?n+=l(["]"]):["+","-","*","/"].includes(c)&&!["(","+","-","*","/",","].includes(s())?n+=` ${c} `:n+=c}return n.replace(/\s+/g," ")})}function Xs(r){return r.startsWith("url(")}function Zs(r){return!isNaN(Number(r))||Ks(r)}function Mr(r){return r.endsWith("%")&&Zs(r.slice(0,-1))||Ks(r)}function Nr(r){return r==="0"||new RegExp(`^[+-]?[0-9]*.?[0-9]+(?:[eE][+-]?[0-9]+)?${Xv}$`).test(r)||Ks(r)}function Mf(r){return Zv.has(r)}function Nf(r){let e=en(K(r));for(let t of e)if(!t.valid)return!1;return!0}function Bf(r){let e=0;return ve(r,"_").every(i=>(i=K(i),i.startsWith("var(")?!0:$r(i,{loose:!0})!==null?(e++,!0):!1))?e>0:!1}function Ff(r){let e=0;return ve(r,",").every(i=>(i=K(i),i.startsWith("var(")?!0:Xs(i)||ex(i)||["element(","image(","cross-fade(","image-set("].some(n=>i.startsWith(n))?(e++,!0):!1))?e>0:!1}function ex(r){r=K(r);for(let e of Jv)if(r.startsWith(`${e}(`))return!0;return!1}function jf(r){let e=0;return ve(r,"_").every(i=>(i=K(i),i.startsWith("var(")?!0:tx.has(i)||Nr(i)||Mr(i)?(e++,!0):!1))?e>0:!1}function zf(r){let e=0;return ve(r,",").every(i=>(i=K(i),i.startsWith("var(")?!0:i.includes(" ")&&!/(['"])([^"']+)\1/g.test(i)||/^\d/g.test(i)?!1:(e++,!0)))?e>0:!1}function Uf(r){return rx.has(r)}function Vf(r){return ix.has(r)}function Hf(r){return nx.has(r)}var Gv,Qv,Kv,Xv,Zv,Jv,tx,rx,ix,nx,Br=P(()=>{u();Qs();Ys();zt();Gv=["min","max","clamp","calc"];Qv=new Set(["scroll-timeline-name","timeline-scope","view-timeline-name","font-palette","anchor-name","anchor-scope","position-anchor","position-try-options","scroll-timeline","animation-timeline","view-timeline","position-try"]);Kv=["cm","mm","Q","in","pc","pt","px","em","ex","ch","rem","lh","rlh","vw","vh","vmin","vmax","vb","vi","svw","svh","lvw","lvh","dvw","dvh","cqw","cqh","cqi","cqb","cqmin","cqmax"],Xv=`(?:${Kv.join("|")})`;Zv=new Set(["thin","medium","thick"]);Jv=new Set(["conic-gradient","linear-gradient","radial-gradient","repeating-conic-gradient","repeating-linear-gradient","repeating-radial-gradient"]);tx=new Set(["center","top","right","bottom","left"]);rx=new Set(["serif","sans-serif","monospace","cursive","fantasy","system-ui","ui-serif","ui-sans-serif","ui-monospace","ui-rounded","math","emoji","fangsong"]);ix=new Set(["xx-small","x-small","small","medium","large","x-large","xx-large","xxx-large"]);nx=new Set(["larger","smaller"])});function Wf(r){let e=["cover","contain"];return ve(r,",").every(t=>{let i=ve(t,"_").filter(Boolean);return i.length===1&&e.includes(i[0])?!0:i.length!==1&&i.length!==2?!1:i.every(n=>Nr(n)||Mr(n)||n==="auto")})}var Gf=P(()=>{u();Br();zt()});function Qf(r,e){r.walkClasses(t=>{t.value=e(t.value),t.raws&&t.raws.value&&(t.raws.value=jt(t.raws.value))})}function Yf(r,e){if(!_t(r))return;let t=r.slice(1,-1);if(!!e(t))return K(t)}function sx(r,e={},t){let i=e[r];if(i!==void 0)return xt(i);if(_t(r)){let n=Yf(r,t);return n===void 0?void 0:xt(n)}}function tn(r,e={},{validate:t=()=>!0}={}){let i=e.values?.[r];return i!==void 0?i:e.supportsNegativeValues&&r.startsWith("-")?sx(r.slice(1),e.values,t):Yf(r,t)}function _t(r){return r.startsWith("[")&&r.endsWith("]")}function Kf(r){let e=r.lastIndexOf("/"),t=r.lastIndexOf("[",e),i=r.indexOf("]",e);return r[e-1]==="]"||r[e+1]==="["||t!==-1&&i!==-1&&t<e&&e<i&&(e=r.lastIndexOf("/",t)),e===-1||e===r.length-1?[r,void 0]:_t(r)&&!r.includes("]/[")?[r,void 0]:[r.slice(0,e),r.slice(e+1)]}function Xt(r){if(typeof r=="string"&&r.includes("<alpha-value>")){let e=r;return({opacityValue:t=1})=>e.replace(/<alpha-value>/g,t)}return r}function Xf(r){return K(r.slice(1,-1))}function ax(r,e={},{tailwindConfig:t={}}={}){if(e.values?.[r]!==void 0)return Xt(e.values?.[r]);let[i,n]=Kf(r);if(n!==void 0){let s=e.values?.[i]??(_t(i)?i.slice(1,-1):void 0);return s===void 0?void 0:(s=Xt(s),_t(n)?Je(s,Xf(n)):t.theme?.opacity?.[n]===void 0?void 0:Je(s,t.theme.opacity[n]))}return tn(r,e,{validate:Bf})}function ox(r,e={}){return e.values?.[r]}function qe(r){return(e,t)=>tn(e,t,{validate:r})}function lx(r,e){let t=r.indexOf(e);return t===-1?[void 0,r]:[r.slice(0,t),r.slice(t+1)]}function ea(r,e,t,i){if(t.values&&e in t.values)for(let{type:s}of r??[]){let a=Js[s](e,t,{tailwindConfig:i});if(a!==void 0)return[a,s,null]}if(_t(e)){let s=e.slice(1,-1),[a,o]=lx(s,":");if(!/^[\w-_]+$/g.test(a))o=s;else if(a!==void 0&&!Zf.includes(a))return[];if(o.length>0&&Zf.includes(a))return[tn(`[${o}]`,t),a,null]}let n=ta(r,e,t,i);for(let s of n)return s;return[]}function*ta(r,e,t,i){let n=we(i,"generalizedModifiers"),[s,a]=Kf(e);if(n&&t.modifiers!=null&&(t.modifiers==="any"||typeof t.modifiers=="object"&&(a&&_t(a)||a in t.modifiers))||(s=e,a=void 0),a!==void 0&&s===""&&(s="DEFAULT"),a!==void 0&&typeof t.modifiers=="object"){let l=t.modifiers?.[a]??null;l!==null?a=l:_t(a)&&(a=Xf(a))}for(let{type:l}of r??[]){let c=Js[l](s,t,{tailwindConfig:i});c!==void 0&&(yield[c,l,a??null])}}var Js,Zf,Fr=P(()=>{u();Zi();Lr();Br();Gi();Gf();ct();Js={any:tn,color:ax,url:qe(Xs),image:qe(Ff),length:qe(Nr),percentage:qe(Mr),position:qe(jf),lookup:ox,"generic-name":qe(Uf),"family-name":qe(zf),number:qe(Zs),"line-width":qe(Mf),"absolute-size":qe(Vf),"relative-size":qe(Hf),shadow:qe(Nf),size:qe(Wf)},Zf=Object.keys(Js)});function X(r){return typeof r=="function"?r({}):r}var ra=P(()=>{u()});function Zt(r){return typeof r=="function"}function jr(r,...e){let t=e.pop();for(let i of e)for(let n in i){let s=t(r[n],i[n]);s===void 0?ke(r[n])&&ke(i[n])?r[n]=jr({},r[n],i[n],t):r[n]=i[n]:r[n]=s}return r}function ux(r,...e){return Zt(r)?r(...e):r}function fx(r){return r.reduce((e,{extend:t})=>jr(e,t,(i,n)=>i===void 0?[n]:Array.isArray(i)?[n,...i]:[n,i]),{})}function cx(r){return{...r.reduce((e,t)=>Hs(e,t),{}),extend:fx(r)}}function Jf(r,e){if(Array.isArray(r)&&ke(r[0]))return r.concat(e);if(Array.isArray(e)&&ke(e[0])&&ke(r))return[r,...e];if(Array.isArray(e))return e}function px({extend:r,...e}){return jr(e,r,(t,i)=>!Zt(t)&&!i.some(Zt)?jr({},t,...i,Jf):(n,s)=>jr({},...[t,...i].map(a=>ux(a,n,s)),Jf))}function*dx(r){let e=kt(r);if(e.length===0||(yield e,Array.isArray(r)))return;let t=/^(.*?)\s*\/\s*([^/]+)$/,i=r.match(t);if(i!==null){let[,n,s]=i,a=kt(n);a.alpha=s,yield a}}function hx(r){let e=(t,i)=>{for(let n of dx(t)){let s=0,a=r;for(;a!=null&&s<n.length;)a=a[n[s++]],a=Zt(a)&&(n.alpha===void 0||s<=n.length-1)?a(e,ia):a;if(a!==void 0){if(n.alpha!==void 0){let o=Xt(a);return Je(o,n.alpha,X(o))}return ke(a)?St(a):a}}return i};return Object.assign(e,{theme:e,...ia}),Object.keys(r).reduce((t,i)=>(t[i]=Zt(r[i])?r[i](e,ia):r[i],t),{})}function ec(r){let e=[];return r.forEach(t=>{e=[...e,t];let i=t?.plugins??[];i.length!==0&&i.forEach(n=>{n.__isOptionsFunction&&(n=n()),e=[...e,...ec([n?.config??{}])]})}),e}function mx(r){return[...r].reduceRight((t,i)=>Zt(i)?i({corePlugins:t}):kf(i,t),vf)}function gx(r){return[...r].reduceRight((t,i)=>[...t,...i],[])}function na(r){let e=[...ec(r),{prefix:"",important:!1,separator:":"}];return Pf(Hs({theme:hx(px(cx(e.map(t=>t?.theme??{})))),corePlugins:mx(e.map(t=>t.corePlugins)),plugins:gx(r.map(t=>t?.plugins??[]))},...e))}var ia,tc=P(()=>{u();Gi();xf();Sf();Vs();Ef();Yi();If();Kt();Xi();Fr();Lr();ra();ia={colors:Us,negative(r){return Object.keys(r).filter(e=>r[e]!=="0").reduce((e,t)=>{let i=xt(r[t]);return i!==void 0&&(e[`-${t}`]=i),e},{})},breakpoints(r){return Object.keys(r).filter(e=>typeof r[e]=="string").reduce((e,t)=>({...e,[`screen-${t}`]:r[t]}),{})}}});var rn=x((f3,rc)=>{u();rc.exports={content:[],presets:[],darkMode:"media",theme:{accentColor:({theme:r})=>({...r("colors"),auto:"auto"}),animation:{none:"none",spin:"spin 1s linear infinite",ping:"ping 1s cubic-bezier(0, 0, 0.2, 1) infinite",pulse:"pulse 2s cubic-bezier(0.4, 0, 0.6, 1) infinite",bounce:"bounce 1s infinite"},aria:{busy:'busy="true"',checked:'checked="true"',disabled:'disabled="true"',expanded:'expanded="true"',hidden:'hidden="true"',pressed:'pressed="true"',readonly:'readonly="true"',required:'required="true"',selected:'selected="true"'},aspectRatio:{auto:"auto",square:"1 / 1",video:"16 / 9"},backdropBlur:({theme:r})=>r("blur"),backdropBrightness:({theme:r})=>r("brightness"),backdropContrast:({theme:r})=>r("contrast"),backdropGrayscale:({theme:r})=>r("grayscale"),backdropHueRotate:({theme:r})=>r("hueRotate"),backdropInvert:({theme:r})=>r("invert"),backdropOpacity:({theme:r})=>r("opacity"),backdropSaturate:({theme:r})=>r("saturate"),backdropSepia:({theme:r})=>r("sepia"),backgroundColor:({theme:r})=>r("colors"),backgroundImage:{none:"none","gradient-to-t":"linear-gradient(to top, var(--tw-gradient-stops))","gradient-to-tr":"linear-gradient(to top right, var(--tw-gradient-stops))","gradient-to-r":"linear-gradient(to right, var(--tw-gradient-stops))","gradient-to-br":"linear-gradient(to bottom right, var(--tw-gradient-stops))","gradient-to-b":"linear-gradient(to bottom, var(--tw-gradient-stops))","gradient-to-bl":"linear-gradient(to bottom left, var(--tw-gradient-stops))","gradient-to-l":"linear-gradient(to left, var(--tw-gradient-stops))","gradient-to-tl":"linear-gradient(to top left, var(--tw-gradient-stops))"},backgroundOpacity:({theme:r})=>r("opacity"),backgroundPosition:{bottom:"bottom",center:"center",left:"left","left-bottom":"left bottom","left-top":"left top",right:"right","right-bottom":"right bottom","right-top":"right top",top:"top"},backgroundSize:{auto:"auto",cover:"cover",contain:"contain"},blur:{0:"0",none:"",sm:"4px",DEFAULT:"8px",md:"12px",lg:"16px",xl:"24px","2xl":"40px","3xl":"64px"},borderColor:({theme:r})=>({...r("colors"),DEFAULT:r("colors.gray.200","currentColor")}),borderOpacity:({theme:r})=>r("opacity"),borderRadius:{none:"0px",sm:"0.125rem",DEFAULT:"0.25rem",md:"0.375rem",lg:"0.5rem",xl:"0.75rem","2xl":"1rem","3xl":"1.5rem",full:"9999px"},borderSpacing:({theme:r})=>({...r("spacing")}),borderWidth:{DEFAULT:"1px",0:"0px",2:"2px",4:"4px",8:"8px"},boxShadow:{sm:"0 1px 2px 0 rgb(0 0 0 / 0.05)",DEFAULT:"0 1px 3px 0 rgb(0 0 0 / 0.1), 0 1px 2px -1px rgb(0 0 0 / 0.1)",md:"0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1)",lg:"0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1)",xl:"0 20px 25px -5px rgb(0 0 0 / 0.1), 0 8px 10px -6px rgb(0 0 0 / 0.1)","2xl":"0 25px 50px -12px rgb(0 0 0 / 0.25)",inner:"inset 0 2px 4px 0 rgb(0 0 0 / 0.05)",none:"none"},boxShadowColor:({theme:r})=>r("colors"),brightness:{0:"0",50:".5",75:".75",90:".9",95:".95",100:"1",105:"1.05",110:"1.1",125:"1.25",150:"1.5",200:"2"},caretColor:({theme:r})=>r("colors"),colors:({colors:r})=>({inherit:r.inherit,current:r.current,transparent:r.transparent,black:r.black,white:r.white,slate:r.slate,gray:r.gray,zinc:r.zinc,neutral:r.neutral,stone:r.stone,red:r.red,orange:r.orange,amber:r.amber,yellow:r.yellow,lime:r.lime,green:r.green,emerald:r.emerald,teal:r.teal,cyan:r.cyan,sky:r.sky,blue:r.blue,indigo:r.indigo,violet:r.violet,purple:r.purple,fuchsia:r.fuchsia,pink:r.pink,rose:r.rose}),columns:{auto:"auto",1:"1",2:"2",3:"3",4:"4",5:"5",6:"6",7:"7",8:"8",9:"9",10:"10",11:"11",12:"12","3xs":"16rem","2xs":"18rem",xs:"20rem",sm:"24rem",md:"28rem",lg:"32rem",xl:"36rem","2xl":"42rem","3xl":"48rem","4xl":"56rem","5xl":"64rem","6xl":"72rem","7xl":"80rem"},container:{},content:{none:"none"},contrast:{0:"0",50:".5",75:".75",100:"1",125:"1.25",150:"1.5",200:"2"},cursor:{auto:"auto",default:"default",pointer:"pointer",wait:"wait",text:"text",move:"move",help:"help","not-allowed":"not-allowed",none:"none","context-menu":"context-menu",progress:"progress",cell:"cell",crosshair:"crosshair","vertical-text":"vertical-text",alias:"alias",copy:"copy","no-drop":"no-drop",grab:"grab",grabbing:"grabbing","all-scroll":"all-scroll","col-resize":"col-resize","row-resize":"row-resize","n-resize":"n-resize","e-resize":"e-resize","s-resize":"s-resize","w-resize":"w-resize","ne-resize":"ne-resize","nw-resize":"nw-resize","se-resize":"se-resize","sw-resize":"sw-resize","ew-resize":"ew-resize","ns-resize":"ns-resize","nesw-resize":"nesw-resize","nwse-resize":"nwse-resize","zoom-in":"zoom-in","zoom-out":"zoom-out"},divideColor:({theme:r})=>r("borderColor"),divideOpacity:({theme:r})=>r("borderOpacity"),divideWidth:({theme:r})=>r("borderWidth"),dropShadow:{sm:"0 1px 1px rgb(0 0 0 / 0.05)",DEFAULT:["0 1px 2px rgb(0 0 0 / 0.1)","0 1px 1px rgb(0 0 0 / 0.06)"],md:["0 4px 3px rgb(0 0 0 / 0.07)","0 2px 2px rgb(0 0 0 / 0.06)"],lg:["0 10px 8px rgb(0 0 0 / 0.04)","0 4px 3px rgb(0 0 0 / 0.1)"],xl:["0 20px 13px rgb(0 0 0 / 0.03)","0 8px 5px rgb(0 0 0 / 0.08)"],"2xl":"0 25px 25px rgb(0 0 0 / 0.15)",none:"0 0 #0000"},fill:({theme:r})=>({none:"none",...r("colors")}),flex:{1:"1 1 0%",auto:"1 1 auto",initial:"0 1 auto",none:"none"},flexBasis:({theme:r})=>({auto:"auto",...r("spacing"),"1/2":"50%","1/3":"33.333333%","2/3":"66.666667%","1/4":"25%","2/4":"50%","3/4":"75%","1/5":"20%","2/5":"40%","3/5":"60%","4/5":"80%","1/6":"16.666667%","2/6":"33.333333%","3/6":"50%","4/6":"66.666667%","5/6":"83.333333%","1/12":"8.333333%","2/12":"16.666667%","3/12":"25%","4/12":"33.333333%","5/12":"41.666667%","6/12":"50%","7/12":"58.333333%","8/12":"66.666667%","9/12":"75%","10/12":"83.333333%","11/12":"91.666667%",full:"100%"}),flexGrow:{0:"0",DEFAULT:"1"},flexShrink:{0:"0",DEFAULT:"1"},fontFamily:{sans:["ui-sans-serif","system-ui","sans-serif",'"Apple Color Emoji"','"Segoe UI Emoji"','"Segoe UI Symbol"','"Noto Color Emoji"'],serif:["ui-serif","Georgia","Cambria",'"Times New Roman"',"Times","serif"],mono:["ui-monospace","SFMono-Regular","Menlo","Monaco","Consolas",'"Liberation Mono"','"Courier New"',"monospace"]},fontSize:{xs:["0.75rem",{lineHeight:"1rem"}],sm:["0.875rem",{lineHeight:"1.25rem"}],base:["1rem",{lineHeight:"1.5rem"}],lg:["1.125rem",{lineHeight:"1.75rem"}],xl:["1.25rem",{lineHeight:"1.75rem"}],"2xl":["1.5rem",{lineHeight:"2rem"}],"3xl":["1.875rem",{lineHeight:"2.25rem"}],"4xl":["2.25rem",{lineHeight:"2.5rem"}],"5xl":["3rem",{lineHeight:"1"}],"6xl":["3.75rem",{lineHeight:"1"}],"7xl":["4.5rem",{lineHeight:"1"}],"8xl":["6rem",{lineHeight:"1"}],"9xl":["8rem",{lineHeight:"1"}]},fontWeight:{thin:"100",extralight:"200",light:"300",normal:"400",medium:"500",semibold:"600",bold:"700",extrabold:"800",black:"900"},gap:({theme:r})=>r("spacing"),gradientColorStops:({theme:r})=>r("colors"),gradientColorStopPositions:{"0%":"0%","5%":"5%","10%":"10%","15%":"15%","20%":"20%","25%":"25%","30%":"30%","35%":"35%","40%":"40%","45%":"45%","50%":"50%","55%":"55%","60%":"60%","65%":"65%","70%":"70%","75%":"75%","80%":"80%","85%":"85%","90%":"90%","95%":"95%","100%":"100%"},grayscale:{0:"0",DEFAULT:"100%"},gridAutoColumns:{auto:"auto",min:"min-content",max:"max-content",fr:"minmax(0, 1fr)"},gridAutoRows:{auto:"auto",min:"min-content",max:"max-content",fr:"minmax(0, 1fr)"},gridColumn:{auto:"auto","span-1":"span 1 / span 1","span-2":"span 2 / span 2","span-3":"span 3 / span 3","span-4":"span 4 / span 4","span-5":"span 5 / span 5","span-6":"span 6 / span 6","span-7":"span 7 / span 7","span-8":"span 8 / span 8","span-9":"span 9 / span 9","span-10":"span 10 / span 10","span-11":"span 11 / span 11","span-12":"span 12 / span 12","span-full":"1 / -1"},gridColumnEnd:{auto:"auto",1:"1",2:"2",3:"3",4:"4",5:"5",6:"6",7:"7",8:"8",9:"9",10:"10",11:"11",12:"12",13:"13"},gridColumnStart:{auto:"auto",1:"1",2:"2",3:"3",4:"4",5:"5",6:"6",7:"7",8:"8",9:"9",10:"10",11:"11",12:"12",13:"13"},gridRow:{auto:"auto","span-1":"span 1 / span 1","span-2":"span 2 / span 2","span-3":"span 3 / span 3","span-4":"span 4 / span 4","span-5":"span 5 / span 5","span-6":"span 6 / span 6","span-7":"span 7 / span 7","span-8":"span 8 / span 8","span-9":"span 9 / span 9","span-10":"span 10 / span 10","span-11":"span 11 / span 11","span-12":"span 12 / span 12","span-full":"1 / -1"},gridRowEnd:{auto:"auto",1:"1",2:"2",3:"3",4:"4",5:"5",6:"6",7:"7",8:"8",9:"9",10:"10",11:"11",12:"12",13:"13"},gridRowStart:{auto:"auto",1:"1",2:"2",3:"3",4:"4",5:"5",6:"6",7:"7",8:"8",9:"9",10:"10",11:"11",12:"12",13:"13"},gridTemplateColumns:{none:"none",subgrid:"subgrid",1:"repeat(1, minmax(0, 1fr))",2:"repeat(2, minmax(0, 1fr))",3:"repeat(3, minmax(0, 1fr))",4:"repeat(4, minmax(0, 1fr))",5:"repeat(5, minmax(0, 1fr))",6:"repeat(6, minmax(0, 1fr))",7:"repeat(7, minmax(0, 1fr))",8:"repeat(8, minmax(0, 1fr))",9:"repeat(9, minmax(0, 1fr))",10:"repeat(10, minmax(0, 1fr))",11:"repeat(11, minmax(0, 1fr))",12:"repeat(12, minmax(0, 1fr))"},gridTemplateRows:{none:"none",subgrid:"subgrid",1:"repeat(1, minmax(0, 1fr))",2:"repeat(2, minmax(0, 1fr))",3:"repeat(3, minmax(0, 1fr))",4:"repeat(4, minmax(0, 1fr))",5:"repeat(5, minmax(0, 1fr))",6:"repeat(6, minmax(0, 1fr))",7:"repeat(7, minmax(0, 1fr))",8:"repeat(8, minmax(0, 1fr))",9:"repeat(9, minmax(0, 1fr))",10:"repeat(10, minmax(0, 1fr))",11:"repeat(11, minmax(0, 1fr))",12:"repeat(12, minmax(0, 1fr))"},height:({theme:r})=>({auto:"auto",...r("spacing"),"1/2":"50%","1/3":"33.333333%","2/3":"66.666667%","1/4":"25%","2/4":"50%","3/4":"75%","1/5":"20%","2/5":"40%","3/5":"60%","4/5":"80%","1/6":"16.666667%","2/6":"33.333333%","3/6":"50%","4/6":"66.666667%","5/6":"83.333333%",full:"100%",screen:"100vh",svh:"100svh",lvh:"100lvh",dvh:"100dvh",min:"min-content",max:"max-content",fit:"fit-content"}),hueRotate:{0:"0deg",15:"15deg",30:"30deg",60:"60deg",90:"90deg",180:"180deg"},inset:({theme:r})=>({auto:"auto",...r("spacing"),"1/2":"50%","1/3":"33.333333%","2/3":"66.666667%","1/4":"25%","2/4":"50%","3/4":"75%",full:"100%"}),invert:{0:"0",DEFAULT:"100%"},keyframes:{spin:{to:{transform:"rotate(360deg)"}},ping:{"75%, 100%":{transform:"scale(2)",opacity:"0"}},pulse:{"50%":{opacity:".5"}},bounce:{"0%, 100%":{transform:"translateY(-25%)",animationTimingFunction:"cubic-bezier(0.8,0,1,1)"},"50%":{transform:"none",animationTimingFunction:"cubic-bezier(0,0,0.2,1)"}}},letterSpacing:{tighter:"-0.05em",tight:"-0.025em",normal:"0em",wide:"0.025em",wider:"0.05em",widest:"0.1em"},lineHeight:{none:"1",tight:"1.25",snug:"1.375",normal:"1.5",relaxed:"1.625",loose:"2",3:".75rem",4:"1rem",5:"1.25rem",6:"1.5rem",7:"1.75rem",8:"2rem",9:"2.25rem",10:"2.5rem"},listStyleType:{none:"none",disc:"disc",decimal:"decimal"},listStyleImage:{none:"none"},margin:({theme:r})=>({auto:"auto",...r("spacing")}),lineClamp:{1:"1",2:"2",3:"3",4:"4",5:"5",6:"6"},maxHeight:({theme:r})=>({...r("spacing"),none:"none",full:"100%",screen:"100vh",svh:"100svh",lvh:"100lvh",dvh:"100dvh",min:"min-content",max:"max-content",fit:"fit-content"}),maxWidth:({theme:r,breakpoints:e})=>({...r("spacing"),none:"none",xs:"20rem",sm:"24rem",md:"28rem",lg:"32rem",xl:"36rem","2xl":"42rem","3xl":"48rem","4xl":"56rem","5xl":"64rem","6xl":"72rem","7xl":"80rem",full:"100%",min:"min-content",max:"max-content",fit:"fit-content",prose:"65ch",...e(r("screens"))}),minHeight:({theme:r})=>({...r("spacing"),full:"100%",screen:"100vh",svh:"100svh",lvh:"100lvh",dvh:"100dvh",min:"min-content",max:"max-content",fit:"fit-content"}),minWidth:({theme:r})=>({...r("spacing"),full:"100%",min:"min-content",max:"max-content",fit:"fit-content"}),objectPosition:{bottom:"bottom",center:"center",left:"left","left-bottom":"left bottom","left-top":"left top",right:"right","right-bottom":"right bottom","right-top":"right top",top:"top"},opacity:{0:"0",5:"0.05",10:"0.1",15:"0.15",20:"0.2",25:"0.25",30:"0.3",35:"0.35",40:"0.4",45:"0.45",50:"0.5",55:"0.55",60:"0.6",65:"0.65",70:"0.7",75:"0.75",80:"0.8",85:"0.85",90:"0.9",95:"0.95",100:"1"},order:{first:"-9999",last:"9999",none:"0",1:"1",2:"2",3:"3",4:"4",5:"5",6:"6",7:"7",8:"8",9:"9",10:"10",11:"11",12:"12"},outlineColor:({theme:r})=>r("colors"),outlineOffset:{0:"0px",1:"1px",2:"2px",4:"4px",8:"8px"},outlineWidth:{0:"0px",1:"1px",2:"2px",4:"4px",8:"8px"},padding:({theme:r})=>r("spacing"),placeholderColor:({theme:r})=>r("colors"),placeholderOpacity:({theme:r})=>r("opacity"),ringColor:({theme:r})=>({DEFAULT:r("colors.blue.500","#3b82f6"),...r("colors")}),ringOffsetColor:({theme:r})=>r("colors"),ringOffsetWidth:{0:"0px",1:"1px",2:"2px",4:"4px",8:"8px"},ringOpacity:({theme:r})=>({DEFAULT:"0.5",...r("opacity")}),ringWidth:{DEFAULT:"3px",0:"0px",1:"1px",2:"2px",4:"4px",8:"8px"},rotate:{0:"0deg",1:"1deg",2:"2deg",3:"3deg",6:"6deg",12:"12deg",45:"45deg",90:"90deg",180:"180deg"},saturate:{0:"0",50:".5",100:"1",150:"1.5",200:"2"},scale:{0:"0",50:".5",75:".75",90:".9",95:".95",100:"1",105:"1.05",110:"1.1",125:"1.25",150:"1.5"},screens:{sm:"640px",md:"768px",lg:"1024px",xl:"1280px","2xl":"1536px"},scrollMargin:({theme:r})=>({...r("spacing")}),scrollPadding:({theme:r})=>r("spacing"),sepia:{0:"0",DEFAULT:"100%"},skew:{0:"0deg",1:"1deg",2:"2deg",3:"3deg",6:"6deg",12:"12deg"},space:({theme:r})=>({...r("spacing")}),spacing:{px:"1px",0:"0px",.5:"0.125rem",1:"0.25rem",1.5:"0.375rem",2:"0.5rem",2.5:"0.625rem",3:"0.75rem",3.5:"0.875rem",4:"1rem",5:"1.25rem",6:"1.5rem",7:"1.75rem",8:"2rem",9:"2.25rem",10:"2.5rem",11:"2.75rem",12:"3rem",14:"3.5rem",16:"4rem",20:"5rem",24:"6rem",28:"7rem",32:"8rem",36:"9rem",40:"10rem",44:"11rem",48:"12rem",52:"13rem",56:"14rem",60:"15rem",64:"16rem",72:"18rem",80:"20rem",96:"24rem"},stroke:({theme:r})=>({none:"none",...r("colors")}),strokeWidth:{0:"0",1:"1",2:"2"},supports:{},data:{},textColor:({theme:r})=>r("colors"),textDecorationColor:({theme:r})=>r("colors"),textDecorationThickness:{auto:"auto","from-font":"from-font",0:"0px",1:"1px",2:"2px",4:"4px",8:"8px"},textIndent:({theme:r})=>({...r("spacing")}),textOpacity:({theme:r})=>r("opacity"),textUnderlineOffset:{auto:"auto",0:"0px",1:"1px",2:"2px",4:"4px",8:"8px"},transformOrigin:{center:"center",top:"top","top-right":"top right",right:"right","bottom-right":"bottom right",bottom:"bottom","bottom-left":"bottom left",left:"left","top-left":"top left"},transitionDelay:{0:"0s",75:"75ms",100:"100ms",150:"150ms",200:"200ms",300:"300ms",500:"500ms",700:"700ms",1e3:"1000ms"},transitionDuration:{DEFAULT:"150ms",0:"0s",75:"75ms",100:"100ms",150:"150ms",200:"200ms",300:"300ms",500:"500ms",700:"700ms",1e3:"1000ms"},transitionProperty:{none:"none",all:"all",DEFAULT:"color, background-color, border-color, text-decoration-color, fill, stroke, opacity, box-shadow, transform, filter, backdrop-filter",colors:"color, background-color, border-color, text-decoration-color, fill, stroke",opacity:"opacity",shadow:"box-shadow",transform:"transform"},transitionTimingFunction:{DEFAULT:"cubic-bezier(0.4, 0, 0.2, 1)",linear:"linear",in:"cubic-bezier(0.4, 0, 1, 1)",out:"cubic-bezier(0, 0, 0.2, 1)","in-out":"cubic-bezier(0.4, 0, 0.2, 1)"},translate:({theme:r})=>({...r("spacing"),"1/2":"50%","1/3":"33.333333%","2/3":"66.666667%","1/4":"25%","2/4":"50%","3/4":"75%",full:"100%"}),size:({theme:r})=>({auto:"auto",...r("spacing"),"1/2":"50%","1/3":"33.333333%","2/3":"66.666667%","1/4":"25%","2/4":"50%","3/4":"75%","1/5":"20%","2/5":"40%","3/5":"60%","4/5":"80%","1/6":"16.666667%","2/6":"33.333333%","3/6":"50%","4/6":"66.666667%","5/6":"83.333333%","1/12":"8.333333%","2/12":"16.666667%","3/12":"25%","4/12":"33.333333%","5/12":"41.666667%","6/12":"50%","7/12":"58.333333%","8/12":"66.666667%","9/12":"75%","10/12":"83.333333%","11/12":"91.666667%",full:"100%",min:"min-content",max:"max-content",fit:"fit-content"}),width:({theme:r})=>({auto:"auto",...r("spacing"),"1/2":"50%","1/3":"33.333333%","2/3":"66.666667%","1/4":"25%","2/4":"50%","3/4":"75%","1/5":"20%","2/5":"40%","3/5":"60%","4/5":"80%","1/6":"16.666667%","2/6":"33.333333%","3/6":"50%","4/6":"66.666667%","5/6":"83.333333%","1/12":"8.333333%","2/12":"16.666667%","3/12":"25%","4/12":"33.333333%","5/12":"41.666667%","6/12":"50%","7/12":"58.333333%","8/12":"66.666667%","9/12":"75%","10/12":"83.333333%","11/12":"91.666667%",full:"100%",screen:"100vw",svw:"100svw",lvw:"100lvw",dvw:"100dvw",min:"min-content",max:"max-content",fit:"fit-content"}),willChange:{auto:"auto",scroll:"scroll-position",contents:"contents",transform:"transform"},zIndex:{auto:"auto",0:"0",10:"10",20:"20",30:"30",40:"40",50:"50"}},plugins:[]}});function nn(r){let e=(r?.presets??[ic.default]).slice().reverse().flatMap(n=>nn(n instanceof Function?n():n)),t={respectDefaultRingColorOpacity:{theme:{ringColor:({theme:n})=>({DEFAULT:"#3b82f67f",...n("colors")})}},disableColorOpacityUtilitiesByDefault:{corePlugins:{backgroundOpacity:!1,borderOpacity:!1,divideOpacity:!1,placeholderOpacity:!1,ringOpacity:!1,textOpacity:!1}}},i=Object.keys(t).filter(n=>we(r,n)).map(n=>t[n]);return[r,...i,...e]}var ic,nc=P(()=>{u();ic=pe(rn());ct()});var sc={};Ge(sc,{default:()=>zr});function zr(...r){let[,...e]=nn(r[0]);return na([...r,...e])}var sa=P(()=>{u();tc();nc()});var Ur={};Ge(Ur,{default:()=>me});var me,et=P(()=>{u();me={resolve:r=>r,extname:r=>"."+r.split(".").pop()}});function sn(r){return typeof r=="object"&&r!==null}function bx(r){return Object.keys(r).length===0}function ac(r){return typeof r=="string"||r instanceof String}function aa(r){return sn(r)&&r.config===void 0&&!bx(r)?null:sn(r)&&r.config!==void 0&&ac(r.config)?me.resolve(r.config):sn(r)&&r.config!==void 0&&sn(r.config)?null:ac(r)?me.resolve(r):wx()}function wx(){for(let r of yx)try{let e=me.resolve(r);return be.accessSync(e),e}catch(e){}return null}var yx,oc=P(()=>{u();ft();et();yx=["./tailwind.config.js","./tailwind.config.cjs","./tailwind.config.mjs","./tailwind.config.ts","./tailwind.config.cts","./tailwind.config.mts"]});var lc={};Ge(lc,{default:()=>oa});var oa,la=P(()=>{u();oa={parse:r=>({href:r})}});var ua=x(()=>{u()});var an=x((v3,cc)=>{u();"use strict";var uc=(Qi(),Af),fc=ua(),Jt=class extends Error{constructor(e,t,i,n,s,a){super(e);this.name="CssSyntaxError",this.reason=e,s&&(this.file=s),n&&(this.source=n),a&&(this.plugin=a),typeof t!="undefined"&&typeof i!="undefined"&&(typeof t=="number"?(this.line=t,this.column=i):(this.line=t.line,this.column=t.column,this.endLine=i.line,this.endColumn=i.column)),this.setMessage(),Error.captureStackTrace&&Error.captureStackTrace(this,Jt)}setMessage(){this.message=this.plugin?this.plugin+": ":"",this.message+=this.file?this.file:"<css input>",typeof this.line!="undefined"&&(this.message+=":"+this.line+":"+this.column),this.message+=": "+this.reason}showSourceCode(e){if(!this.source)return"";let t=this.source;e==null&&(e=uc.isColorSupported);let i=f=>f,n=f=>f,s=f=>f;if(e){let{bold:f,gray:d,red:p}=uc.createColors(!0);n=h=>f(p(h)),i=h=>d(h),fc&&(s=h=>fc(h))}let a=t.split(/\r?\n/),o=Math.max(this.line-3,0),l=Math.min(this.line+2,a.length),c=String(l).length;return a.slice(o,l).map((f,d)=>{let p=o+1+d,h=" "+(" "+p).slice(-c)+" | ";if(p===this.line){if(f.length>160){let v=20,y=Math.max(0,this.column-v),w=Math.max(this.column+v,this.endColumn+v),k=f.slice(y,w),S=i(h.replace(/\d/g," "))+f.slice(0,Math.min(this.column-1,v-1)).replace(/[^\t]/g," ");return n(">")+i(h)+s(k)+`
 `+S+n("^")}let b=i(h.replace(/\d/g," "))+f.slice(0,this.column-1).replace(/[^\t]/g," ");return n(">")+i(h)+s(f)+`
 `+b+n("^")}return" "+i(h)+s(f)}).join(`
`)}toString(){let e=this.showSourceCode();return e&&(e=`

`+e+`
`),this.name+": "+this.message+e}};cc.exports=Jt;Jt.default=Jt});var fa=x((x3,dc)=>{u();"use strict";var pc={after:`
`,beforeClose:`
`,beforeComment:`
`,beforeDecl:`
`,beforeOpen:" ",beforeRule:`
`,colon:": ",commentLeft:" ",commentRight:" ",emptyBody:"",indent:"    ",semicolon:!1};function vx(r){return r[0].toUpperCase()+r.slice(1)}var on=class{constructor(e){this.builder=e}atrule(e,t){let i="@"+e.name,n=e.params?this.rawValue(e,"params"):"";if(typeof e.raws.afterName!="undefined"?i+=e.raws.afterName:n&&(i+=" "),e.nodes)this.block(e,i+n);else{let s=(e.raws.between||"")+(t?";":"");this.builder(i+n+s,e)}}beforeAfter(e,t){let i;e.type==="decl"?i=this.raw(e,null,"beforeDecl"):e.type==="comment"?i=this.raw(e,null,"beforeComment"):t==="before"?i=this.raw(e,null,"beforeRule"):i=this.raw(e,null,"beforeClose");let n=e.parent,s=0;for(;n&&n.type!=="root";)s+=1,n=n.parent;if(i.includes(`
`)){let a=this.raw(e,null,"indent");if(a.length)for(let o=0;o<s;o++)i+=a}return i}block(e,t){let i=this.raw(e,"between","beforeOpen");this.builder(t+i+"{",e,"start");let n;e.nodes&&e.nodes.length?(this.body(e),n=this.raw(e,"after")):n=this.raw(e,"after","emptyBody"),n&&this.builder(n),this.builder("}",e,"end")}body(e){let t=e.nodes.length-1;for(;t>0&&e.nodes[t].type==="comment";)t-=1;let i=this.raw(e,"semicolon");for(let n=0;n<e.nodes.length;n++){let s=e.nodes[n],a=this.raw(s,"before");a&&this.builder(a),this.stringify(s,t!==n||i)}}comment(e){let t=this.raw(e,"left","commentLeft"),i=this.raw(e,"right","commentRight");this.builder("/*"+t+e.text+i+"*/",e)}decl(e,t){let i=this.raw(e,"between","colon"),n=e.prop+i+this.rawValue(e,"value");e.important&&(n+=e.raws.important||" !important"),t&&(n+=";"),this.builder(n,e)}document(e){this.body(e)}raw(e,t,i){let n;if(i||(i=t),t&&(n=e.raws[t],typeof n!="undefined"))return n;let s=e.parent;if(i==="before"&&(!s||s.type==="root"&&s.first===e||s&&s.type==="document"))return"";if(!s)return pc[i];let a=e.root();if(a.rawCache||(a.rawCache={}),typeof a.rawCache[i]!="undefined")return a.rawCache[i];if(i==="before"||i==="after")return this.beforeAfter(e,i);{let o="raw"+vx(i);this[o]?n=this[o](a,e):a.walk(l=>{if(n=l.raws[t],typeof n!="undefined")return!1})}return typeof n=="undefined"&&(n=pc[i]),a.rawCache[i]=n,n}rawBeforeClose(e){let t;return e.walk(i=>{if(i.nodes&&i.nodes.length>0&&typeof i.raws.after!="undefined")return t=i.raws.after,t.includes(`
`)&&(t=t.replace(/[^\n]+$/,"")),!1}),t&&(t=t.replace(/\S/g,"")),t}rawBeforeComment(e,t){let i;return e.walkComments(n=>{if(typeof n.raws.before!="undefined")return i=n.raws.before,i.includes(`
`)&&(i=i.replace(/[^\n]+$/,"")),!1}),typeof i=="undefined"?i=this.raw(t,null,"beforeDecl"):i&&(i=i.replace(/\S/g,"")),i}rawBeforeDecl(e,t){let i;return e.walkDecls(n=>{if(typeof n.raws.before!="undefined")return i=n.raws.before,i.includes(`
`)&&(i=i.replace(/[^\n]+$/,"")),!1}),typeof i=="undefined"?i=this.raw(t,null,"beforeRule"):i&&(i=i.replace(/\S/g,"")),i}rawBeforeOpen(e){let t;return e.walk(i=>{if(i.type!=="decl"&&(t=i.raws.between,typeof t!="undefined"))return!1}),t}rawBeforeRule(e){let t;return e.walk(i=>{if(i.nodes&&(i.parent!==e||e.first!==i)&&typeof i.raws.before!="undefined")return t=i.raws.before,t.includes(`
`)&&(t=t.replace(/[^\n]+$/,"")),!1}),t&&(t=t.replace(/\S/g,"")),t}rawColon(e){let t;return e.walkDecls(i=>{if(typeof i.raws.between!="undefined")return t=i.raws.between.replace(/[^\s:]/g,""),!1}),t}rawEmptyBody(e){let t;return e.walk(i=>{if(i.nodes&&i.nodes.length===0&&(t=i.raws.after,typeof t!="undefined"))return!1}),t}rawIndent(e){if(e.raws.indent)return e.raws.indent;let t;return e.walk(i=>{let n=i.parent;if(n&&n!==e&&n.parent&&n.parent===e&&typeof i.raws.before!="undefined"){let s=i.raws.before.split(`
`);return t=s[s.length-1],t=t.replace(/\S/g,""),!1}}),t}rawSemicolon(e){let t;return e.walk(i=>{if(i.nodes&&i.nodes.length&&i.last.type==="decl"&&(t=i.raws.semicolon,typeof t!="undefined"))return!1}),t}rawValue(e,t){let i=e[t],n=e.raws[t];return n&&n.value===i?n.raw:i}root(e){this.body(e),e.raws.after&&this.builder(e.raws.after)}rule(e){this.block(e,this.rawValue(e,"selector")),e.raws.ownSemicolon&&this.builder(e.raws.ownSemicolon,e,"end")}stringify(e,t){if(!this[e.type])throw new Error("Unknown AST node type "+e.type+". Maybe you need to change PostCSS stringifier.");this[e.type](e,t)}};dc.exports=on;on.default=on});var Vr=x((k3,hc)=>{u();"use strict";var xx=fa();function ca(r,e){new xx(e).stringify(r)}hc.exports=ca;ca.default=ca});var ln=x((S3,pa)=>{u();"use strict";pa.exports.isClean=Symbol("isClean");pa.exports.my=Symbol("my")});var Gr=x((A3,mc)=>{u();"use strict";var kx=an(),Sx=fa(),Ax=Vr(),{isClean:Hr,my:Cx}=ln();function da(r,e){let t=new r.constructor;for(let i in r){if(!Object.prototype.hasOwnProperty.call(r,i)||i==="proxyCache")continue;let n=r[i],s=typeof n;i==="parent"&&s==="object"?e&&(t[i]=e):i==="source"?t[i]=n:Array.isArray(n)?t[i]=n.map(a=>da(a,t)):(s==="object"&&n!==null&&(n=da(n)),t[i]=n)}return t}function Wr(r,e){if(e&&typeof e.offset!="undefined")return e.offset;let t=1,i=1,n=0;for(let s=0;s<r.length;s++){if(i===e.line&&t===e.column){n=s;break}r[s]===`
`?(t=1,i+=1):t+=1}return n}var un=class{constructor(e={}){this.raws={},this[Hr]=!1,this[Cx]=!0;for(let t in e)if(t==="nodes"){this.nodes=[];for(let i of e[t])typeof i.clone=="function"?this.append(i.clone()):this.append(i)}else this[t]=e[t]}addToError(e){if(e.postcssNode=this,e.stack&&this.source&&/\n\s{4}at /.test(e.stack)){let t=this.source;e.stack=e.stack.replace(/\n\s{4}at /,`$&${t.input.from}:${t.start.line}:${t.start.column}$&`)}return e}after(e){return this.parent.insertAfter(this,e),this}assign(e={}){for(let t in e)this[t]=e[t];return this}before(e){return this.parent.insertBefore(this,e),this}cleanRaws(e){delete this.raws.before,delete this.raws.after,e||delete this.raws.between}clone(e={}){let t=da(this);for(let i in e)t[i]=e[i];return t}cloneAfter(e={}){let t=this.clone(e);return this.parent.insertAfter(this,t),t}cloneBefore(e={}){let t=this.clone(e);return this.parent.insertBefore(this,t),t}error(e,t={}){if(this.source){let{end:i,start:n}=this.rangeBy(t);return this.source.input.error(e,{column:n.column,line:n.line},{column:i.column,line:i.line},t)}return new kx(e)}getProxyProcessor(){return{get(e,t){return t==="proxyOf"?e:t==="root"?()=>e.root().toProxy():e[t]},set(e,t,i){return e[t]===i||(e[t]=i,(t==="prop"||t==="value"||t==="name"||t==="params"||t==="important"||t==="text")&&e.markDirty()),!0}}}markClean(){this[Hr]=!0}markDirty(){if(this[Hr]){this[Hr]=!1;let e=this;for(;e=e.parent;)e[Hr]=!1}}next(){if(!this.parent)return;let e=this.parent.index(this);return this.parent.nodes[e+1]}positionBy(e){let t=this.source.start;if(e.index)t=this.positionInside(e.index);else if(e.word){let n=this.source.input.css.slice(Wr(this.source.input.css,this.source.start),Wr(this.source.input.css,this.source.end)).indexOf(e.word);n!==-1&&(t=this.positionInside(n))}return t}positionInside(e){let t=this.source.start.column,i=this.source.start.line,n=Wr(this.source.input.css,this.source.start),s=n+e;for(let a=n;a<s;a++)this.source.input.css[a]===`
`?(t=1,i+=1):t+=1;return{column:t,line:i}}prev(){if(!this.parent)return;let e=this.parent.index(this);return this.parent.nodes[e-1]}rangeBy(e){let t={column:this.source.start.column,line:this.source.start.line},i=this.source.end?{column:this.source.end.column+1,line:this.source.end.line}:{column:t.column+1,line:t.line};if(e.word){let s=this.source.input.css.slice(Wr(this.source.input.css,this.source.start),Wr(this.source.input.css,this.source.end)).indexOf(e.word);s!==-1&&(t=this.positionInside(s),i=this.positionInside(s+e.word.length))}else e.start?t={column:e.start.column,line:e.start.line}:e.index&&(t=this.positionInside(e.index)),e.end?i={column:e.end.column,line:e.end.line}:typeof e.endIndex=="number"?i=this.positionInside(e.endIndex):e.index&&(i=this.positionInside(e.index+1));return(i.line<t.line||i.line===t.line&&i.column<=t.column)&&(i={column:t.column+1,line:t.line}),{end:i,start:t}}raw(e,t){return new Sx().raw(this,e,t)}remove(){return this.parent&&this.parent.removeChild(this),this.parent=void 0,this}replaceWith(...e){if(this.parent){let t=this,i=!1;for(let n of e)n===this?i=!0:i?(this.parent.insertAfter(t,n),t=n):this.parent.insertBefore(t,n);i||this.remove()}return this}root(){let e=this;for(;e.parent&&e.parent.type!=="document";)e=e.parent;return e}toJSON(e,t){let i={},n=t==null;t=t||new Map;let s=0;for(let a in this){if(!Object.prototype.hasOwnProperty.call(this,a)||a==="parent"||a==="proxyCache")continue;let o=this[a];if(Array.isArray(o))i[a]=o.map(l=>typeof l=="object"&&l.toJSON?l.toJSON(null,t):l);else if(typeof o=="object"&&o.toJSON)i[a]=o.toJSON(null,t);else if(a==="source"){let l=t.get(o.input);l==null&&(l=s,t.set(o.input,s),s++),i[a]={end:o.end,inputId:l,start:o.start}}else i[a]=o}return n&&(i.inputs=[...t.keys()].map(a=>a.toJSON())),i}toProxy(){return this.proxyCache||(this.proxyCache=new Proxy(this,this.getProxyProcessor())),this.proxyCache}toString(e=Ax){e.stringify&&(e=e.stringify);let t="";return e(this,i=>{t+=i}),t}warn(e,t,i){let n={node:this};for(let s in i)n[s]=i[s];return e.warn(t,n)}get proxyOf(){return this}};mc.exports=un;un.default=un});var Qr=x((C3,gc)=>{u();"use strict";var _x=Gr(),fn=class extends _x{constructor(e){super(e);this.type="comment"}};gc.exports=fn;fn.default=fn});var Yr=x((_3,yc)=>{u();"use strict";var Ex=Gr(),cn=class extends Ex{constructor(e){e&&typeof e.value!="undefined"&&typeof e.value!="string"&&(e={...e,value:String(e.value)});super(e);this.type="decl"}get variable(){return this.prop.startsWith("--")||this.prop[0]==="$"}};yc.exports=cn;cn.default=cn});var Et=x((E3,_c)=>{u();"use strict";var bc=Qr(),wc=Yr(),Ox=Gr(),{isClean:vc,my:xc}=ln(),ha,kc,Sc,ma;function Ac(r){return r.map(e=>(e.nodes&&(e.nodes=Ac(e.nodes)),delete e.source,e))}function Cc(r){if(r[vc]=!1,r.proxyOf.nodes)for(let e of r.proxyOf.nodes)Cc(e)}var Fe=class extends Ox{append(...e){for(let t of e){let i=this.normalize(t,this.last);for(let n of i)this.proxyOf.nodes.push(n)}return this.markDirty(),this}cleanRaws(e){if(super.cleanRaws(e),this.nodes)for(let t of this.nodes)t.cleanRaws(e)}each(e){if(!this.proxyOf.nodes)return;let t=this.getIterator(),i,n;for(;this.indexes[t]<this.proxyOf.nodes.length&&(i=this.indexes[t],n=e(this.proxyOf.nodes[i],i),n!==!1);)this.indexes[t]+=1;return delete this.indexes[t],n}every(e){return this.nodes.every(e)}getIterator(){this.lastEach||(this.lastEach=0),this.indexes||(this.indexes={}),this.lastEach+=1;let e=this.lastEach;return this.indexes[e]=0,e}getProxyProcessor(){return{get(e,t){return t==="proxyOf"?e:e[t]?t==="each"||typeof t=="string"&&t.startsWith("walk")?(...i)=>e[t](...i.map(n=>typeof n=="function"?(s,a)=>n(s.toProxy(),a):n)):t==="every"||t==="some"?i=>e[t]((n,...s)=>i(n.toProxy(),...s)):t==="root"?()=>e.root().toProxy():t==="nodes"?e.nodes.map(i=>i.toProxy()):t==="first"||t==="last"?e[t].toProxy():e[t]:e[t]},set(e,t,i){return e[t]===i||(e[t]=i,(t==="name"||t==="params"||t==="selector")&&e.markDirty()),!0}}}index(e){return typeof e=="number"?e:(e.proxyOf&&(e=e.proxyOf),this.proxyOf.nodes.indexOf(e))}insertAfter(e,t){let i=this.index(e),n=this.normalize(t,this.proxyOf.nodes[i]).reverse();i=this.index(e);for(let a of n)this.proxyOf.nodes.splice(i+1,0,a);let s;for(let a in this.indexes)s=this.indexes[a],i<s&&(this.indexes[a]=s+n.length);return this.markDirty(),this}insertBefore(e,t){let i=this.index(e),n=i===0?"prepend":!1,s=this.normalize(t,this.proxyOf.nodes[i],n).reverse();i=this.index(e);for(let o of s)this.proxyOf.nodes.splice(i,0,o);let a;for(let o in this.indexes)a=this.indexes[o],i<=a&&(this.indexes[o]=a+s.length);return this.markDirty(),this}normalize(e,t){if(typeof e=="string")e=Ac(kc(e).nodes);else if(typeof e=="undefined")e=[];else if(Array.isArray(e)){e=e.slice(0);for(let n of e)n.parent&&n.parent.removeChild(n,"ignore")}else if(e.type==="root"&&this.type!=="document"){e=e.nodes.slice(0);for(let n of e)n.parent&&n.parent.removeChild(n,"ignore")}else if(e.type)e=[e];else if(e.prop){if(typeof e.value=="undefined")throw new Error("Value field is missed in node creation");typeof e.value!="string"&&(e.value=String(e.value)),e=[new wc(e)]}else if(e.selector||e.selectors)e=[new ma(e)];else if(e.name)e=[new ha(e)];else if(e.text)e=[new bc(e)];else throw new Error("Unknown node type in node creation");return e.map(n=>(n[xc]||Fe.rebuild(n),n=n.proxyOf,n.parent&&n.parent.removeChild(n),n[vc]&&Cc(n),n.raws||(n.raws={}),typeof n.raws.before=="undefined"&&t&&typeof t.raws.before!="undefined"&&(n.raws.before=t.raws.before.replace(/\S/g,"")),n.parent=this.proxyOf,n))}prepend(...e){e=e.reverse();for(let t of e){let i=this.normalize(t,this.first,"prepend").reverse();for(let n of i)this.proxyOf.nodes.unshift(n);for(let n in this.indexes)this.indexes[n]=this.indexes[n]+i.length}return this.markDirty(),this}push(e){return e.parent=this,this.proxyOf.nodes.push(e),this}removeAll(){for(let e of this.proxyOf.nodes)e.parent=void 0;return this.proxyOf.nodes=[],this.markDirty(),this}removeChild(e){e=this.index(e),this.proxyOf.nodes[e].parent=void 0,this.proxyOf.nodes.splice(e,1);let t;for(let i in this.indexes)t=this.indexes[i],t>=e&&(this.indexes[i]=t-1);return this.markDirty(),this}replaceValues(e,t,i){return i||(i=t,t={}),this.walkDecls(n=>{t.props&&!t.props.includes(n.prop)||t.fast&&!n.value.includes(t.fast)||(n.value=n.value.replace(e,i))}),this.markDirty(),this}some(e){return this.nodes.some(e)}walk(e){return this.each((t,i)=>{let n;try{n=e(t,i)}catch(s){throw t.addToError(s)}return n!==!1&&t.walk&&(n=t.walk(e)),n})}walkAtRules(e,t){return t?e instanceof RegExp?this.walk((i,n)=>{if(i.type==="atrule"&&e.test(i.name))return t(i,n)}):this.walk((i,n)=>{if(i.type==="atrule"&&i.name===e)return t(i,n)}):(t=e,this.walk((i,n)=>{if(i.type==="atrule")return t(i,n)}))}walkComments(e){return this.walk((t,i)=>{if(t.type==="comment")return e(t,i)})}walkDecls(e,t){return t?e instanceof RegExp?this.walk((i,n)=>{if(i.type==="decl"&&e.test(i.prop))return t(i,n)}):this.walk((i,n)=>{if(i.type==="decl"&&i.prop===e)return t(i,n)}):(t=e,this.walk((i,n)=>{if(i.type==="decl")return t(i,n)}))}walkRules(e,t){return t?e instanceof RegExp?this.walk((i,n)=>{if(i.type==="rule"&&e.test(i.selector))return t(i,n)}):this.walk((i,n)=>{if(i.type==="rule"&&i.selector===e)return t(i,n)}):(t=e,this.walk((i,n)=>{if(i.type==="rule")return t(i,n)}))}get first(){if(!!this.proxyOf.nodes)return this.proxyOf.nodes[0]}get last(){if(!!this.proxyOf.nodes)return this.proxyOf.nodes[this.proxyOf.nodes.length-1]}};Fe.registerParse=r=>{kc=r};Fe.registerRule=r=>{ma=r};Fe.registerAtRule=r=>{ha=r};Fe.registerRoot=r=>{Sc=r};_c.exports=Fe;Fe.default=Fe;Fe.rebuild=r=>{r.type==="atrule"?Object.setPrototypeOf(r,ha.prototype):r.type==="rule"?Object.setPrototypeOf(r,ma.prototype):r.type==="decl"?Object.setPrototypeOf(r,wc.prototype):r.type==="comment"?Object.setPrototypeOf(r,bc.prototype):r.type==="root"&&Object.setPrototypeOf(r,Sc.prototype),r[xc]=!0,r.nodes&&r.nodes.forEach(e=>{Fe.rebuild(e)})}});var pn=x((O3,Oc)=>{u();"use strict";var Ec=Et(),Kr=class extends Ec{constructor(e){super(e);this.type="atrule"}append(...e){return this.proxyOf.nodes||(this.nodes=[]),super.append(...e)}prepend(...e){return this.proxyOf.nodes||(this.nodes=[]),super.prepend(...e)}};Oc.exports=Kr;Kr.default=Kr;Ec.registerAtRule(Kr)});var dn=x((T3,Pc)=>{u();"use strict";var Tx=Et(),Tc,Rc,er=class extends Tx{constructor(e){super({type:"document",...e});this.nodes||(this.nodes=[])}toResult(e={}){return new Tc(new Rc,this,e).stringify()}};er.registerLazyResult=r=>{Tc=r};er.registerProcessor=r=>{Rc=r};Pc.exports=er;er.default=er});var Dc=x((R3,Ic)=>{u();var Rx="useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict",Px=(r,e=21)=>(t=e)=>{let i="",n=t;for(;n--;)i+=r[Math.random()*r.length|0];return i},Ix=(r=21)=>{let e="",t=r;for(;t--;)e+=Rx[Math.random()*64|0];return e};Ic.exports={nanoid:Ix,customAlphabet:Px}});var qc=x(()=>{u()});var ga=x((D3,$c)=>{u();$c.exports={}});var mn=x((q3,Bc)=>{u();"use strict";var{nanoid:Dx}=Dc(),{isAbsolute:ya,resolve:ba}=(et(),Ur),{SourceMapConsumer:qx,SourceMapGenerator:$x}=qc(),{fileURLToPath:Lc,pathToFileURL:hn}=(la(),lc),Mc=an(),Lx=ga(),wa=ua(),va=Symbol("fromOffsetCache"),Mx=Boolean(qx&&$x),Nc=Boolean(ba&&ya),Xr=class{constructor(e,t={}){if(e===null||typeof e=="undefined"||typeof e=="object"&&!e.toString)throw new Error(`PostCSS received ${e} instead of CSS string`);if(this.css=e.toString(),this.css[0]==="\uFEFF"||this.css[0]==="\uFFFE"?(this.hasBOM=!0,this.css=this.css.slice(1)):this.hasBOM=!1,t.from&&(!Nc||/^\w+:\/\//.test(t.from)||ya(t.from)?this.file=t.from:this.file=ba(t.from)),Nc&&Mx){let i=new Lx(this.css,t);if(i.text){this.map=i;let n=i.consumer().file;!this.file&&n&&(this.file=this.mapResolve(n))}}this.file||(this.id="<input css "+Dx(6)+">"),this.map&&(this.map.file=this.from)}error(e,t,i,n={}){let s,a,o;if(t&&typeof t=="object"){let c=t,f=i;if(typeof c.offset=="number"){let d=this.fromOffset(c.offset);t=d.line,i=d.col}else t=c.line,i=c.column;if(typeof f.offset=="number"){let d=this.fromOffset(f.offset);a=d.line,s=d.col}else a=f.line,s=f.column}else if(!i){let c=this.fromOffset(t);t=c.line,i=c.col}let l=this.origin(t,i,a,s);return l?o=new Mc(e,l.endLine===void 0?l.line:{column:l.column,line:l.line},l.endLine===void 0?l.column:{column:l.endColumn,line:l.endLine},l.source,l.file,n.plugin):o=new Mc(e,a===void 0?t:{column:i,line:t},a===void 0?i:{column:s,line:a},this.css,this.file,n.plugin),o.input={column:i,endColumn:s,endLine:a,line:t,source:this.css},this.file&&(hn&&(o.input.url=hn(this.file).toString()),o.input.file=this.file),o}fromOffset(e){let t,i;if(this[va])i=this[va];else{let s=this.css.split(`
`);i=new Array(s.length);let a=0;for(let o=0,l=s.length;o<l;o++)i[o]=a,a+=s[o].length+1;this[va]=i}t=i[i.length-1];let n=0;if(e>=t)n=i.length-1;else{let s=i.length-2,a;for(;n<s;)if(a=n+(s-n>>1),e<i[a])s=a-1;else if(e>=i[a+1])n=a+1;else{n=a;break}}return{col:e-i[n]+1,line:n+1}}mapResolve(e){return/^\w+:\/\//.test(e)?e:ba(this.map.consumer().sourceRoot||this.map.root||".",e)}origin(e,t,i,n){if(!this.map)return!1;let s=this.map.consumer(),a=s.originalPositionFor({column:t,line:e});if(!a.source)return!1;let o;typeof i=="number"&&(o=s.originalPositionFor({column:n,line:i}));let l;ya(a.source)?l=hn(a.source):l=new URL(a.source,this.map.consumer().sourceRoot||hn(this.map.mapFile));let c={column:a.column,endColumn:o&&o.column,endLine:o&&o.line,line:a.line,url:l.toString()};if(l.protocol==="file:")if(Lc)c.file=Lc(l);else throw new Error("file: protocol is not available in this PostCSS build");let f=s.sourceContentFor(a.source);return f&&(c.source=f),c}toJSON(){let e={};for(let t of["hasBOM","css","file","id"])this[t]!=null&&(e[t]=this[t]);return this.map&&(e.map={...this.map},e.map.consumerCache&&(e.map.consumerCache=void 0)),e}get from(){return this.file||this.id}};Bc.exports=Xr;Xr.default=Xr;wa&&wa.registerInput&&wa.registerInput(Xr)});var tr=x(($3,Uc)=>{u();"use strict";var Fc=Et(),jc,zc,Ut=class extends Fc{constructor(e){super(e);this.type="root",this.nodes||(this.nodes=[])}normalize(e,t,i){let n=super.normalize(e);if(t){if(i==="prepend")this.nodes.length>1?t.raws.before=this.nodes[1].raws.before:delete t.raws.before;else if(this.first!==t)for(let s of n)s.raws.before=t.raws.before}return n}removeChild(e,t){let i=this.index(e);return!t&&i===0&&this.nodes.length>1&&(this.nodes[1].raws.before=this.nodes[i].raws.before),super.removeChild(e)}toResult(e={}){return new jc(new zc,this,e).stringify()}};Ut.registerLazyResult=r=>{jc=r};Ut.registerProcessor=r=>{zc=r};Uc.exports=Ut;Ut.default=Ut;Fc.registerRoot(Ut)});var xa=x((L3,Vc)=>{u();"use strict";var Zr={comma(r){return Zr.split(r,[","],!0)},space(r){let e=[" ",`
`,"	"];return Zr.split(r,e)},split(r,e,t){let i=[],n="",s=!1,a=0,o=!1,l="",c=!1;for(let f of r)c?c=!1:f==="\\"?c=!0:o?f===l&&(o=!1):f==='"'||f==="'"?(o=!0,l=f):f==="("?a+=1:f===")"?a>0&&(a-=1):a===0&&e.includes(f)&&(s=!0),s?(n!==""&&i.push(n.trim()),n="",s=!1):n+=f;return(t||n!=="")&&i.push(n.trim()),i}};Vc.exports=Zr;Zr.default=Zr});var gn=x((M3,Wc)=>{u();"use strict";var Hc=Et(),Nx=xa(),Jr=class extends Hc{constructor(e){super(e);this.type="rule",this.nodes||(this.nodes=[])}get selectors(){return Nx.comma(this.selector)}set selectors(e){let t=this.selector?this.selector.match(/,\s*/):null,i=t?t[0]:","+this.raw("between","beforeOpen");this.selector=e.join(i)}};Wc.exports=Jr;Jr.default=Jr;Hc.registerRule(Jr)});var Qc=x((N3,Gc)=>{u();"use strict";var Bx=pn(),Fx=Qr(),jx=Yr(),zx=mn(),Ux=ga(),Vx=tr(),Hx=gn();function ei(r,e){if(Array.isArray(r))return r.map(n=>ei(n));let{inputs:t,...i}=r;if(t){e=[];for(let n of t){let s={...n,__proto__:zx.prototype};s.map&&(s.map={...s.map,__proto__:Ux.prototype}),e.push(s)}}if(i.nodes&&(i.nodes=r.nodes.map(n=>ei(n,e))),i.source){let{inputId:n,...s}=i.source;i.source=s,n!=null&&(i.source.input=e[n])}if(i.type==="root")return new Vx(i);if(i.type==="decl")return new jx(i);if(i.type==="rule")return new Hx(i);if(i.type==="comment")return new Fx(i);if(i.type==="atrule")return new Bx(i);throw new Error("Unknown node type: "+r.type)}Gc.exports=ei;ei.default=ei});var ka=x((B3,Yc)=>{u();Yc.exports=function(r,e){return{generate:()=>{let t="";return r(e,i=>{t+=i}),[t]}}}});var ep=x((F3,Jc)=>{u();"use strict";var Sa="'".charCodeAt(0),Kc='"'.charCodeAt(0),yn="\\".charCodeAt(0),Xc="/".charCodeAt(0),bn=`
`.charCodeAt(0),ti=" ".charCodeAt(0),wn="\f".charCodeAt(0),vn="	".charCodeAt(0),xn="\r".charCodeAt(0),Wx="[".charCodeAt(0),Gx="]".charCodeAt(0),Qx="(".charCodeAt(0),Yx=")".charCodeAt(0),Kx="{".charCodeAt(0),Xx="}".charCodeAt(0),Zx=";".charCodeAt(0),Jx="*".charCodeAt(0),e1=":".charCodeAt(0),t1="@".charCodeAt(0),kn=/[\t\n\f\r "#'()/;[\\\]{}]/g,Sn=/[\t\n\f\r !"#'():;@[\\\]{}]|\/(?=\*)/g,r1=/.[\r\n"'(/\\]/,Zc=/[\da-f]/i;Jc.exports=function(e,t={}){let i=e.css.valueOf(),n=t.ignoreErrors,s,a,o,l,c,f,d,p,h,b,v=i.length,y=0,w=[],k=[];function S(){return y}function E(R){throw e.error("Unclosed "+R,y)}function T(){return k.length===0&&y>=v}function B(R){if(k.length)return k.pop();if(y>=v)return;let F=R?R.ignoreUnclosed:!1;switch(s=i.charCodeAt(y),s){case bn:case ti:case vn:case xn:case wn:{l=y;do l+=1,s=i.charCodeAt(l);while(s===ti||s===bn||s===vn||s===xn||s===wn);f=["space",i.slice(y,l)],y=l-1;break}case Wx:case Gx:case Kx:case Xx:case e1:case Zx:case Yx:{let Y=String.fromCharCode(s);f=[Y,Y,y];break}case Qx:{if(b=w.length?w.pop()[1]:"",h=i.charCodeAt(y+1),b==="url"&&h!==Sa&&h!==Kc&&h!==ti&&h!==bn&&h!==vn&&h!==wn&&h!==xn){l=y;do{if(d=!1,l=i.indexOf(")",l+1),l===-1)if(n||F){l=y;break}else E("bracket");for(p=l;i.charCodeAt(p-1)===yn;)p-=1,d=!d}while(d);f=["brackets",i.slice(y,l+1),y,l],y=l}else l=i.indexOf(")",y+1),a=i.slice(y,l+1),l===-1||r1.test(a)?f=["(","(",y]:(f=["brackets",a,y,l],y=l);break}case Sa:case Kc:{c=s===Sa?"'":'"',l=y;do{if(d=!1,l=i.indexOf(c,l+1),l===-1)if(n||F){l=y+1;break}else E("string");for(p=l;i.charCodeAt(p-1)===yn;)p-=1,d=!d}while(d);f=["string",i.slice(y,l+1),y,l],y=l;break}case t1:{kn.lastIndex=y+1,kn.test(i),kn.lastIndex===0?l=i.length-1:l=kn.lastIndex-2,f=["at-word",i.slice(y,l+1),y,l],y=l;break}case yn:{for(l=y,o=!0;i.charCodeAt(l+1)===yn;)l+=1,o=!o;if(s=i.charCodeAt(l+1),o&&s!==Xc&&s!==ti&&s!==bn&&s!==vn&&s!==xn&&s!==wn&&(l+=1,Zc.test(i.charAt(l)))){for(;Zc.test(i.charAt(l+1));)l+=1;i.charCodeAt(l+1)===ti&&(l+=1)}f=["word",i.slice(y,l+1),y,l],y=l;break}default:{s===Xc&&i.charCodeAt(y+1)===Jx?(l=i.indexOf("*/",y+2)+1,l===0&&(n||F?l=i.length:E("comment")),f=["comment",i.slice(y,l+1),y,l],y=l):(Sn.lastIndex=y+1,Sn.test(i),Sn.lastIndex===0?l=i.length-1:l=Sn.lastIndex-2,f=["word",i.slice(y,l+1),y,l],w.push(f),y=l);break}}return y++,f}function N(R){k.push(R)}return{back:N,endOfFile:T,nextToken:B,position:S}}});var sp=x((j3,np)=>{u();"use strict";var i1=pn(),n1=Qr(),s1=Yr(),a1=tr(),tp=gn(),o1=ep(),rp={empty:!0,space:!0};function l1(r){for(let e=r.length-1;e>=0;e--){let t=r[e],i=t[3]||t[2];if(i)return i}}var ip=class{constructor(e){this.input=e,this.root=new a1,this.current=this.root,this.spaces="",this.semicolon=!1,this.createTokenizer(),this.root.source={input:e,start:{column:1,line:1,offset:0}}}atrule(e){let t=new i1;t.name=e[1].slice(1),t.name===""&&this.unnamedAtrule(t,e),this.init(t,e[2]);let i,n,s,a=!1,o=!1,l=[],c=[];for(;!this.tokenizer.endOfFile();){if(e=this.tokenizer.nextToken(),i=e[0],i==="("||i==="["?c.push(i==="("?")":"]"):i==="{"&&c.length>0?c.push("}"):i===c[c.length-1]&&c.pop(),c.length===0)if(i===";"){t.source.end=this.getPosition(e[2]),t.source.end.offset++,this.semicolon=!0;break}else if(i==="{"){o=!0;break}else if(i==="}"){if(l.length>0){for(s=l.length-1,n=l[s];n&&n[0]==="space";)n=l[--s];n&&(t.source.end=this.getPosition(n[3]||n[2]),t.source.end.offset++)}this.end(e);break}else l.push(e);else l.push(e);if(this.tokenizer.endOfFile()){a=!0;break}}t.raws.between=this.spacesAndCommentsFromEnd(l),l.length?(t.raws.afterName=this.spacesAndCommentsFromStart(l),this.raw(t,"params",l),a&&(e=l[l.length-1],t.source.end=this.getPosition(e[3]||e[2]),t.source.end.offset++,this.spaces=t.raws.between,t.raws.between="")):(t.raws.afterName="",t.params=""),o&&(t.nodes=[],this.current=t)}checkMissedSemicolon(e){let t=this.colon(e);if(t===!1)return;let i=0,n;for(let s=t-1;s>=0&&(n=e[s],!(n[0]!=="space"&&(i+=1,i===2)));s--);throw this.input.error("Missed semicolon",n[0]==="word"?n[3]+1:n[2])}colon(e){let t=0,i,n,s;for(let[a,o]of e.entries()){if(n=o,s=n[0],s==="("&&(t+=1),s===")"&&(t-=1),t===0&&s===":")if(!i)this.doubleColon(n);else{if(i[0]==="word"&&i[1]==="progid")continue;return a}i=n}return!1}comment(e){let t=new n1;this.init(t,e[2]),t.source.end=this.getPosition(e[3]||e[2]),t.source.end.offset++;let i=e[1].slice(2,-2);if(/^\s*$/.test(i))t.text="",t.raws.left=i,t.raws.right="";else{let n=i.match(/^(\s*)([^]*\S)(\s*)$/);t.text=n[2],t.raws.left=n[1],t.raws.right=n[3]}}createTokenizer(){this.tokenizer=o1(this.input)}decl(e,t){let i=new s1;this.init(i,e[0][2]);let n=e[e.length-1];for(n[0]===";"&&(this.semicolon=!0,e.pop()),i.source.end=this.getPosition(n[3]||n[2]||l1(e)),i.source.end.offset++;e[0][0]!=="word";)e.length===1&&this.unknownWord(e),i.raws.before+=e.shift()[1];for(i.source.start=this.getPosition(e[0][2]),i.prop="";e.length;){let c=e[0][0];if(c===":"||c==="space"||c==="comment")break;i.prop+=e.shift()[1]}i.raws.between="";let s;for(;e.length;)if(s=e.shift(),s[0]===":"){i.raws.between+=s[1];break}else s[0]==="word"&&/\w/.test(s[1])&&this.unknownWord([s]),i.raws.between+=s[1];(i.prop[0]==="_"||i.prop[0]==="*")&&(i.raws.before+=i.prop[0],i.prop=i.prop.slice(1));let a=[],o;for(;e.length&&(o=e[0][0],!(o!=="space"&&o!=="comment"));)a.push(e.shift());this.precheckMissedSemicolon(e);for(let c=e.length-1;c>=0;c--){if(s=e[c],s[1].toLowerCase()==="!important"){i.important=!0;let f=this.stringFrom(e,c);f=this.spacesFromEnd(e)+f,f!==" !important"&&(i.raws.important=f);break}else if(s[1].toLowerCase()==="important"){let f=e.slice(0),d="";for(let p=c;p>0;p--){let h=f[p][0];if(d.trim().startsWith("!")&&h!=="space")break;d=f.pop()[1]+d}d.trim().startsWith("!")&&(i.important=!0,i.raws.important=d,e=f)}if(s[0]!=="space"&&s[0]!=="comment")break}e.some(c=>c[0]!=="space"&&c[0]!=="comment")&&(i.raws.between+=a.map(c=>c[1]).join(""),a=[]),this.raw(i,"value",a.concat(e),t),i.value.includes(":")&&!t&&this.checkMissedSemicolon(e)}doubleColon(e){throw this.input.error("Double colon",{offset:e[2]},{offset:e[2]+e[1].length})}emptyRule(e){let t=new tp;this.init(t,e[2]),t.selector="",t.raws.between="",this.current=t}end(e){this.current.nodes&&this.current.nodes.length&&(this.current.raws.semicolon=this.semicolon),this.semicolon=!1,this.current.raws.after=(this.current.raws.after||"")+this.spaces,this.spaces="",this.current.parent?(this.current.source.end=this.getPosition(e[2]),this.current.source.end.offset++,this.current=this.current.parent):this.unexpectedClose(e)}endFile(){this.current.parent&&this.unclosedBlock(),this.current.nodes&&this.current.nodes.length&&(this.current.raws.semicolon=this.semicolon),this.current.raws.after=(this.current.raws.after||"")+this.spaces,this.root.source.end=this.getPosition(this.tokenizer.position())}freeSemicolon(e){if(this.spaces+=e[1],this.current.nodes){let t=this.current.nodes[this.current.nodes.length-1];t&&t.type==="rule"&&!t.raws.ownSemicolon&&(t.raws.ownSemicolon=this.spaces,this.spaces="")}}getPosition(e){let t=this.input.fromOffset(e);return{column:t.col,line:t.line,offset:e}}init(e,t){this.current.push(e),e.source={input:this.input,start:this.getPosition(t)},e.raws.before=this.spaces,this.spaces="",e.type!=="comment"&&(this.semicolon=!1)}other(e){let t=!1,i=null,n=!1,s=null,a=[],o=e[1].startsWith("--"),l=[],c=e;for(;c;){if(i=c[0],l.push(c),i==="("||i==="[")s||(s=c),a.push(i==="("?")":"]");else if(o&&n&&i==="{")s||(s=c),a.push("}");else if(a.length===0)if(i===";")if(n){this.decl(l,o);return}else break;else if(i==="{"){this.rule(l);return}else if(i==="}"){this.tokenizer.back(l.pop()),t=!0;break}else i===":"&&(n=!0);else i===a[a.length-1]&&(a.pop(),a.length===0&&(s=null));c=this.tokenizer.nextToken()}if(this.tokenizer.endOfFile()&&(t=!0),a.length>0&&this.unclosedBracket(s),t&&n){if(!o)for(;l.length&&(c=l[l.length-1][0],!(c!=="space"&&c!=="comment"));)this.tokenizer.back(l.pop());this.decl(l,o)}else this.unknownWord(l)}parse(){let e;for(;!this.tokenizer.endOfFile();)switch(e=this.tokenizer.nextToken(),e[0]){case"space":this.spaces+=e[1];break;case";":this.freeSemicolon(e);break;case"}":this.end(e);break;case"comment":this.comment(e);break;case"at-word":this.atrule(e);break;case"{":this.emptyRule(e);break;default:this.other(e);break}this.endFile()}precheckMissedSemicolon(){}raw(e,t,i,n){let s,a,o=i.length,l="",c=!0,f,d;for(let p=0;p<o;p+=1)s=i[p],a=s[0],a==="space"&&p===o-1&&!n?c=!1:a==="comment"?(d=i[p-1]?i[p-1][0]:"empty",f=i[p+1]?i[p+1][0]:"empty",!rp[d]&&!rp[f]?l.slice(-1)===","?c=!1:l+=s[1]:c=!1):l+=s[1];if(!c){let p=i.reduce((h,b)=>h+b[1],"");e.raws[t]={raw:p,value:l}}e[t]=l}rule(e){e.pop();let t=new tp;this.init(t,e[0][2]),t.raws.between=this.spacesAndCommentsFromEnd(e),this.raw(t,"selector",e),this.current=t}spacesAndCommentsFromEnd(e){let t,i="";for(;e.length&&(t=e[e.length-1][0],!(t!=="space"&&t!=="comment"));)i=e.pop()[1]+i;return i}spacesAndCommentsFromStart(e){let t,i="";for(;e.length&&(t=e[0][0],!(t!=="space"&&t!=="comment"));)i+=e.shift()[1];return i}spacesFromEnd(e){let t,i="";for(;e.length&&(t=e[e.length-1][0],t==="space");)i=e.pop()[1]+i;return i}stringFrom(e,t){let i="";for(let n=t;n<e.length;n++)i+=e[n][1];return e.splice(t,e.length-t),i}unclosedBlock(){let e=this.current.source.start;throw this.input.error("Unclosed block",e.line,e.column)}unclosedBracket(e){throw this.input.error("Unclosed bracket",{offset:e[2]},{offset:e[2]+1})}unexpectedClose(e){throw this.input.error("Unexpected }",{offset:e[2]},{offset:e[2]+1})}unknownWord(e){throw this.input.error("Unknown word",{offset:e[0][2]},{offset:e[0][2]+e[0][1].length})}unnamedAtrule(e,t){throw this.input.error("At-rule without name",{offset:t[2]},{offset:t[2]+t[1].length})}};np.exports=ip});var Cn=x((z3,ap)=>{u();"use strict";var u1=Et(),f1=mn(),c1=sp();function An(r,e){let t=new f1(r,e),i=new c1(t);try{i.parse()}catch(n){throw n}return i.root}ap.exports=An;An.default=An;u1.registerParse(An)});var Aa=x((U3,op)=>{u();"use strict";var _n=class{constructor(e,t={}){if(this.type="warning",this.text=e,t.node&&t.node.source){let i=t.node.rangeBy(t);this.line=i.start.line,this.column=i.start.column,this.endLine=i.end.line,this.endColumn=i.end.column}for(let i in t)this[i]=t[i]}toString(){return this.node?this.node.error(this.text,{index:this.index,plugin:this.plugin,word:this.word}).message:this.plugin?this.plugin+": "+this.text:this.text}};op.exports=_n;_n.default=_n});var On=x((V3,lp)=>{u();"use strict";var p1=Aa(),En=class{constructor(e,t,i){this.processor=e,this.messages=[],this.root=t,this.opts=i,this.css=void 0,this.map=void 0}toString(){return this.css}warn(e,t={}){t.plugin||this.lastPlugin&&this.lastPlugin.postcssPlugin&&(t.plugin=this.lastPlugin.postcssPlugin);let i=new p1(e,t);return this.messages.push(i),i}warnings(){return this.messages.filter(e=>e.type==="warning")}get content(){return this.css}};lp.exports=En;En.default=En});var Ca=x((H3,fp)=>{u();"use strict";var up={};fp.exports=function(e){up[e]||(up[e]=!0,typeof console!="undefined"&&console.warn&&console.warn(e))}});var Oa=x((G3,hp)=>{u();"use strict";var d1=Et(),h1=dn(),m1=ka(),g1=Cn(),cp=On(),y1=tr(),b1=Vr(),{isClean:tt,my:w1}=ln(),W3=Ca(),v1={atrule:"AtRule",comment:"Comment",decl:"Declaration",document:"Document",root:"Root",rule:"Rule"},x1={AtRule:!0,AtRuleExit:!0,Comment:!0,CommentExit:!0,Declaration:!0,DeclarationExit:!0,Document:!0,DocumentExit:!0,Once:!0,OnceExit:!0,postcssPlugin:!0,prepare:!0,Root:!0,RootExit:!0,Rule:!0,RuleExit:!0},k1={Once:!0,postcssPlugin:!0,prepare:!0},rr=0;function ri(r){return typeof r=="object"&&typeof r.then=="function"}function pp(r){let e=!1,t=v1[r.type];return r.type==="decl"?e=r.prop.toLowerCase():r.type==="atrule"&&(e=r.name.toLowerCase()),e&&r.append?[t,t+"-"+e,rr,t+"Exit",t+"Exit-"+e]:e?[t,t+"-"+e,t+"Exit",t+"Exit-"+e]:r.append?[t,rr,t+"Exit"]:[t,t+"Exit"]}function dp(r){let e;return r.type==="document"?e=["Document",rr,"DocumentExit"]:r.type==="root"?e=["Root",rr,"RootExit"]:e=pp(r),{eventIndex:0,events:e,iterator:0,node:r,visitorIndex:0,visitors:[]}}function _a(r){return r[tt]=!1,r.nodes&&r.nodes.forEach(e=>_a(e)),r}var Ea={},pt=class{constructor(e,t,i){this.stringified=!1,this.processed=!1;let n;if(typeof t=="object"&&t!==null&&(t.type==="root"||t.type==="document"))n=_a(t);else if(t instanceof pt||t instanceof cp)n=_a(t.root),t.map&&(typeof i.map=="undefined"&&(i.map={}),i.map.inline||(i.map.inline=!1),i.map.prev=t.map);else{let s=g1;i.syntax&&(s=i.syntax.parse),i.parser&&(s=i.parser),s.parse&&(s=s.parse);try{n=s(t,i)}catch(a){this.processed=!0,this.error=a}n&&!n[w1]&&d1.rebuild(n)}this.result=new cp(e,n,i),this.helpers={...Ea,postcss:Ea,result:this.result},this.plugins=this.processor.plugins.map(s=>typeof s=="object"&&s.prepare?{...s,...s.prepare(this.result)}:s)}async(){return this.error?Promise.reject(this.error):this.processed?Promise.resolve(this.result):(this.processing||(this.processing=this.runAsync()),this.processing)}catch(e){return this.async().catch(e)}finally(e){return this.async().then(e,e)}getAsyncError(){throw new Error("Use process(css).then(cb) to work with async plugins")}handleError(e,t){let i=this.result.lastPlugin;try{t&&t.addToError(e),this.error=e,e.name==="CssSyntaxError"&&!e.plugin?(e.plugin=i.postcssPlugin,e.setMessage()):i.postcssVersion}catch(n){console&&console.error&&console.error(n)}return e}prepareVisitors(){this.listeners={};let e=(t,i,n)=>{this.listeners[i]||(this.listeners[i]=[]),this.listeners[i].push([t,n])};for(let t of this.plugins)if(typeof t=="object")for(let i in t){if(!x1[i]&&/^[A-Z]/.test(i))throw new Error(`Unknown event ${i} in ${t.postcssPlugin}. Try to update PostCSS (${this.processor.version} now).`);if(!k1[i])if(typeof t[i]=="object")for(let n in t[i])n==="*"?e(t,i,t[i][n]):e(t,i+"-"+n.toLowerCase(),t[i][n]);else typeof t[i]=="function"&&e(t,i,t[i])}this.hasListener=Object.keys(this.listeners).length>0}async runAsync(){this.plugin=0;for(let e=0;e<this.plugins.length;e++){let t=this.plugins[e],i=this.runOnRoot(t);if(ri(i))try{await i}catch(n){throw this.handleError(n)}}if(this.prepareVisitors(),this.hasListener){let e=this.result.root;for(;!e[tt];){e[tt]=!0;let t=[dp(e)];for(;t.length>0;){let i=this.visitTick(t);if(ri(i))try{await i}catch(n){let s=t[t.length-1].node;throw this.handleError(n,s)}}}if(this.listeners.OnceExit)for(let[t,i]of this.listeners.OnceExit){this.result.lastPlugin=t;try{if(e.type==="document"){let n=e.nodes.map(s=>i(s,this.helpers));await Promise.all(n)}else await i(e,this.helpers)}catch(n){throw this.handleError(n)}}}return this.processed=!0,this.stringify()}runOnRoot(e){this.result.lastPlugin=e;try{if(typeof e=="object"&&e.Once){if(this.result.root.type==="document"){let t=this.result.root.nodes.map(i=>e.Once(i,this.helpers));return ri(t[0])?Promise.all(t):t}return e.Once(this.result.root,this.helpers)}else if(typeof e=="function")return e(this.result.root,this.result)}catch(t){throw this.handleError(t)}}stringify(){if(this.error)throw this.error;if(this.stringified)return this.result;this.stringified=!0,this.sync();let e=this.result.opts,t=b1;e.syntax&&(t=e.syntax.stringify),e.stringifier&&(t=e.stringifier),t.stringify&&(t=t.stringify);let n=new m1(t,this.result.root,this.result.opts).generate();return this.result.css=n[0],this.result.map=n[1],this.result}sync(){if(this.error)throw this.error;if(this.processed)return this.result;if(this.processed=!0,this.processing)throw this.getAsyncError();for(let e of this.plugins){let t=this.runOnRoot(e);if(ri(t))throw this.getAsyncError()}if(this.prepareVisitors(),this.hasListener){let e=this.result.root;for(;!e[tt];)e[tt]=!0,this.walkSync(e);if(this.listeners.OnceExit)if(e.type==="document")for(let t of e.nodes)this.visitSync(this.listeners.OnceExit,t);else this.visitSync(this.listeners.OnceExit,e)}return this.result}then(e,t){return this.async().then(e,t)}toString(){return this.css}visitSync(e,t){for(let[i,n]of e){this.result.lastPlugin=i;let s;try{s=n(t,this.helpers)}catch(a){throw this.handleError(a,t.proxyOf)}if(t.type!=="root"&&t.type!=="document"&&!t.parent)return!0;if(ri(s))throw this.getAsyncError()}}visitTick(e){let t=e[e.length-1],{node:i,visitors:n}=t;if(i.type!=="root"&&i.type!=="document"&&!i.parent){e.pop();return}if(n.length>0&&t.visitorIndex<n.length){let[a,o]=n[t.visitorIndex];t.visitorIndex+=1,t.visitorIndex===n.length&&(t.visitors=[],t.visitorIndex=0),this.result.lastPlugin=a;try{return o(i.toProxy(),this.helpers)}catch(l){throw this.handleError(l,i)}}if(t.iterator!==0){let a=t.iterator,o;for(;o=i.nodes[i.indexes[a]];)if(i.indexes[a]+=1,!o[tt]){o[tt]=!0,e.push(dp(o));return}t.iterator=0,delete i.indexes[a]}let s=t.events;for(;t.eventIndex<s.length;){let a=s[t.eventIndex];if(t.eventIndex+=1,a===rr){i.nodes&&i.nodes.length&&(i[tt]=!0,t.iterator=i.getIterator());return}else if(this.listeners[a]){t.visitors=this.listeners[a];return}}e.pop()}walkSync(e){e[tt]=!0;let t=pp(e);for(let i of t)if(i===rr)e.nodes&&e.each(n=>{n[tt]||this.walkSync(n)});else{let n=this.listeners[i];if(n&&this.visitSync(n,e.toProxy()))return}}warnings(){return this.sync().warnings()}get content(){return this.stringify().content}get css(){return this.stringify().css}get map(){return this.stringify().map}get messages(){return this.sync().messages}get opts(){return this.result.opts}get processor(){return this.result.processor}get root(){return this.sync().root}get[Symbol.toStringTag](){return"LazyResult"}};pt.registerPostcss=r=>{Ea=r};hp.exports=pt;pt.default=pt;y1.registerLazyResult(pt);h1.registerLazyResult(pt)});var gp=x((Y3,mp)=>{u();"use strict";var S1=ka(),A1=Cn(),C1=On(),_1=Vr(),Q3=Ca(),Tn=class{constructor(e,t,i){t=t.toString(),this.stringified=!1,this._processor=e,this._css=t,this._opts=i,this._map=void 0;let n,s=_1;this.result=new C1(this._processor,n,this._opts),this.result.css=t;let a=this;Object.defineProperty(this.result,"root",{get(){return a.root}});let o=new S1(s,n,this._opts,t);if(o.isMap()){let[l,c]=o.generate();l&&(this.result.css=l),c&&(this.result.map=c)}else o.clearAnnotation(),this.result.css=o.css}async(){return this.error?Promise.reject(this.error):Promise.resolve(this.result)}catch(e){return this.async().catch(e)}finally(e){return this.async().then(e,e)}sync(){if(this.error)throw this.error;return this.result}then(e,t){return this.async().then(e,t)}toString(){return this._css}warnings(){return[]}get content(){return this.result.css}get css(){return this.result.css}get map(){return this.result.map}get messages(){return[]}get opts(){return this.result.opts}get processor(){return this.result.processor}get root(){if(this._root)return this._root;let e,t=A1;try{e=t(this._css,this._opts)}catch(i){this.error=i}if(this.error)throw this.error;return this._root=e,e}get[Symbol.toStringTag](){return"NoWorkResult"}};mp.exports=Tn;Tn.default=Tn});var bp=x((K3,yp)=>{u();"use strict";var E1=dn(),O1=Oa(),T1=gp(),R1=tr(),ir=class{constructor(e=[]){this.version="8.4.49",this.plugins=this.normalize(e)}normalize(e){let t=[];for(let i of e)if(i.postcss===!0?i=i():i.postcss&&(i=i.postcss),typeof i=="object"&&Array.isArray(i.plugins))t=t.concat(i.plugins);else if(typeof i=="object"&&i.postcssPlugin)t.push(i);else if(typeof i=="function")t.push(i);else if(!(typeof i=="object"&&(i.parse||i.stringify)))throw new Error(i+" is not a PostCSS plugin");return t}process(e,t={}){return!this.plugins.length&&!t.parser&&!t.stringifier&&!t.syntax?new T1(this,e,t):new O1(this,e,t)}use(e){return this.plugins=this.plugins.concat(this.normalize([e])),this}};yp.exports=ir;ir.default=ir;R1.registerProcessor(ir);E1.registerProcessor(ir)});var $e=x((X3,Cp)=>{u();"use strict";var wp=pn(),vp=Qr(),P1=Et(),I1=an(),xp=Yr(),kp=dn(),D1=Qc(),q1=mn(),$1=Oa(),L1=xa(),M1=Gr(),N1=Cn(),Ta=bp(),B1=On(),Sp=tr(),Ap=gn(),F1=Vr(),j1=Aa();function J(...r){return r.length===1&&Array.isArray(r[0])&&(r=r[0]),new Ta(r)}J.plugin=function(e,t){let i=!1;function n(...a){console&&console.warn&&!i&&(i=!0,console.warn(e+`: postcss.plugin was deprecated. Migration guide:
https://evilmartians.com/chronicles/postcss-8-plugin-migration`),m.env.LANG&&m.env.LANG.startsWith("cn")&&console.warn(e+`: \u91CC\u9762 postcss.plugin \u88AB\u5F03\u7528. \u8FC1\u79FB\u6307\u5357:
https://www.w3ctech.com/topic/2226`));let o=t(...a);return o.postcssPlugin=e,o.postcssVersion=new Ta().version,o}let s;return Object.defineProperty(n,"postcss",{get(){return s||(s=n()),s}}),n.process=function(a,o,l){return J([n(l)]).process(a,o)},n};J.stringify=F1;J.parse=N1;J.fromJSON=D1;J.list=L1;J.comment=r=>new vp(r);J.atRule=r=>new wp(r);J.decl=r=>new xp(r);J.rule=r=>new Ap(r);J.root=r=>new Sp(r);J.document=r=>new kp(r);J.CssSyntaxError=I1;J.Declaration=xp;J.Container=P1;J.Processor=Ta;J.Document=kp;J.Comment=vp;J.Warning=j1;J.AtRule=wp;J.Result=B1;J.Input=q1;J.Rule=Ap;J.Root=Sp;J.Node=M1;$1.registerPostcss(J);Cp.exports=J;J.default=J});var re,ee,Z3,J3,eI,tI,rI,iI,nI,sI,aI,oI,lI,uI,fI,cI,pI,dI,hI,mI,gI,yI,bI,wI,vI,xI,Ot=P(()=>{u();re=pe($e()),ee=re.default,Z3=re.default.stringify,J3=re.default.fromJSON,eI=re.default.plugin,tI=re.default.parse,rI=re.default.list,iI=re.default.document,nI=re.default.comment,sI=re.default.atRule,aI=re.default.rule,oI=re.default.decl,lI=re.default.root,uI=re.default.CssSyntaxError,fI=re.default.Declaration,cI=re.default.Container,pI=re.default.Processor,dI=re.default.Document,hI=re.default.Comment,mI=re.default.Warning,gI=re.default.AtRule,yI=re.default.Result,bI=re.default.Input,wI=re.default.Rule,vI=re.default.Root,xI=re.default.Node});var Ra=x((SI,_p)=>{u();_p.exports=function(r,e,t,i,n){for(e=e.split?e.split("."):e,i=0;i<e.length;i++)r=r?r[e[i]]:n;return r===n?t:r}});var Pn=x((Rn,Ep)=>{u();"use strict";Rn.__esModule=!0;Rn.default=V1;function z1(r){for(var e=r.toLowerCase(),t="",i=!1,n=0;n<6&&e[n]!==void 0;n++){var s=e.charCodeAt(n),a=s>=97&&s<=102||s>=48&&s<=57;if(i=s===32,!a)break;t+=e[n]}if(t.length!==0){var o=parseInt(t,16),l=o>=55296&&o<=57343;return l||o===0||o>1114111?["\uFFFD",t.length+(i?1:0)]:[String.fromCodePoint(o),t.length+(i?1:0)]}}var U1=/\\/;function V1(r){var e=U1.test(r);if(!e)return r;for(var t="",i=0;i<r.length;i++){if(r[i]==="\\"){var n=z1(r.slice(i+1,i+7));if(n!==void 0){t+=n[0],i+=n[1];continue}if(r[i+1]==="\\"){t+="\\",i++;continue}r.length===i+1&&(t+=r[i]);continue}t+=r[i]}return t}Ep.exports=Rn.default});var Tp=x((In,Op)=>{u();"use strict";In.__esModule=!0;In.default=H1;function H1(r){for(var e=arguments.length,t=new Array(e>1?e-1:0),i=1;i<e;i++)t[i-1]=arguments[i];for(;t.length>0;){var n=t.shift();if(!r[n])return;r=r[n]}return r}Op.exports=In.default});var Pp=x((Dn,Rp)=>{u();"use strict";Dn.__esModule=!0;Dn.default=W1;function W1(r){for(var e=arguments.length,t=new Array(e>1?e-1:0),i=1;i<e;i++)t[i-1]=arguments[i];for(;t.length>0;){var n=t.shift();r[n]||(r[n]={}),r=r[n]}}Rp.exports=Dn.default});var Dp=x((qn,Ip)=>{u();"use strict";qn.__esModule=!0;qn.default=G1;function G1(r){for(var e="",t=r.indexOf("/*"),i=0;t>=0;){e=e+r.slice(i,t);var n=r.indexOf("*/",t+2);if(n<0)return e;i=n+2,t=r.indexOf("/*",i)}return e=e+r.slice(i),e}Ip.exports=qn.default});var ii=x(rt=>{u();"use strict";rt.__esModule=!0;rt.unesc=rt.stripComments=rt.getProp=rt.ensureObject=void 0;var Q1=$n(Pn());rt.unesc=Q1.default;var Y1=$n(Tp());rt.getProp=Y1.default;var K1=$n(Pp());rt.ensureObject=K1.default;var X1=$n(Dp());rt.stripComments=X1.default;function $n(r){return r&&r.__esModule?r:{default:r}}});var dt=x((ni,Lp)=>{u();"use strict";ni.__esModule=!0;ni.default=void 0;var qp=ii();function $p(r,e){for(var t=0;t<e.length;t++){var i=e[t];i.enumerable=i.enumerable||!1,i.configurable=!0,"value"in i&&(i.writable=!0),Object.defineProperty(r,i.key,i)}}function Z1(r,e,t){return e&&$p(r.prototype,e),t&&$p(r,t),Object.defineProperty(r,"prototype",{writable:!1}),r}var J1=function r(e,t){if(typeof e!="object"||e===null)return e;var i=new e.constructor;for(var n in e)if(!!e.hasOwnProperty(n)){var s=e[n],a=typeof s;n==="parent"&&a==="object"?t&&(i[n]=t):s instanceof Array?i[n]=s.map(function(o){return r(o,i)}):i[n]=r(s,i)}return i},ek=function(){function r(t){t===void 0&&(t={}),Object.assign(this,t),this.spaces=this.spaces||{},this.spaces.before=this.spaces.before||"",this.spaces.after=this.spaces.after||""}var e=r.prototype;return e.remove=function(){return this.parent&&this.parent.removeChild(this),this.parent=void 0,this},e.replaceWith=function(){if(this.parent){for(var i in arguments)this.parent.insertBefore(this,arguments[i]);this.remove()}return this},e.next=function(){return this.parent.at(this.parent.index(this)+1)},e.prev=function(){return this.parent.at(this.parent.index(this)-1)},e.clone=function(i){i===void 0&&(i={});var n=J1(this);for(var s in i)n[s]=i[s];return n},e.appendToPropertyAndEscape=function(i,n,s){this.raws||(this.raws={});var a=this[i],o=this.raws[i];this[i]=a+n,o||s!==n?this.raws[i]=(o||a)+s:delete this.raws[i]},e.setPropertyAndEscape=function(i,n,s){this.raws||(this.raws={}),this[i]=n,this.raws[i]=s},e.setPropertyWithoutEscape=function(i,n){this[i]=n,this.raws&&delete this.raws[i]},e.isAtPosition=function(i,n){if(this.source&&this.source.start&&this.source.end)return!(this.source.start.line>i||this.source.end.line<i||this.source.start.line===i&&this.source.start.column>n||this.source.end.line===i&&this.source.end.column<n)},e.stringifyProperty=function(i){return this.raws&&this.raws[i]||this[i]},e.valueToString=function(){return String(this.stringifyProperty("value"))},e.toString=function(){return[this.rawSpaceBefore,this.valueToString(),this.rawSpaceAfter].join("")},Z1(r,[{key:"rawSpaceBefore",get:function(){var i=this.raws&&this.raws.spaces&&this.raws.spaces.before;return i===void 0&&(i=this.spaces&&this.spaces.before),i||""},set:function(i){(0,qp.ensureObject)(this,"raws","spaces"),this.raws.spaces.before=i}},{key:"rawSpaceAfter",get:function(){var i=this.raws&&this.raws.spaces&&this.raws.spaces.after;return i===void 0&&(i=this.spaces.after),i||""},set:function(i){(0,qp.ensureObject)(this,"raws","spaces"),this.raws.spaces.after=i}}]),r}();ni.default=ek;Lp.exports=ni.default});var Se=x(ie=>{u();"use strict";ie.__esModule=!0;ie.UNIVERSAL=ie.TAG=ie.STRING=ie.SELECTOR=ie.ROOT=ie.PSEUDO=ie.NESTING=ie.ID=ie.COMMENT=ie.COMBINATOR=ie.CLASS=ie.ATTRIBUTE=void 0;var tk="tag";ie.TAG=tk;var rk="string";ie.STRING=rk;var ik="selector";ie.SELECTOR=ik;var nk="root";ie.ROOT=nk;var sk="pseudo";ie.PSEUDO=sk;var ak="nesting";ie.NESTING=ak;var ok="id";ie.ID=ok;var lk="comment";ie.COMMENT=lk;var uk="combinator";ie.COMBINATOR=uk;var fk="class";ie.CLASS=fk;var ck="attribute";ie.ATTRIBUTE=ck;var pk="universal";ie.UNIVERSAL=pk});var Ln=x((si,Fp)=>{u();"use strict";si.__esModule=!0;si.default=void 0;var dk=mk(dt()),ht=hk(Se());function Mp(r){if(typeof WeakMap!="function")return null;var e=new WeakMap,t=new WeakMap;return(Mp=function(n){return n?t:e})(r)}function hk(r,e){if(!e&&r&&r.__esModule)return r;if(r===null||typeof r!="object"&&typeof r!="function")return{default:r};var t=Mp(e);if(t&&t.has(r))return t.get(r);var i={},n=Object.defineProperty&&Object.getOwnPropertyDescriptor;for(var s in r)if(s!=="default"&&Object.prototype.hasOwnProperty.call(r,s)){var a=n?Object.getOwnPropertyDescriptor(r,s):null;a&&(a.get||a.set)?Object.defineProperty(i,s,a):i[s]=r[s]}return i.default=r,t&&t.set(r,i),i}function mk(r){return r&&r.__esModule?r:{default:r}}function gk(r,e){var t=typeof Symbol!="undefined"&&r[Symbol.iterator]||r["@@iterator"];if(t)return(t=t.call(r)).next.bind(t);if(Array.isArray(r)||(t=yk(r))||e&&r&&typeof r.length=="number"){t&&(r=t);var i=0;return function(){return i>=r.length?{done:!0}:{done:!1,value:r[i++]}}}throw new TypeError(`Invalid attempt to iterate non-iterable instance.
In order to be iterable, non-array objects must have a [Symbol.iterator]() method.`)}function yk(r,e){if(!!r){if(typeof r=="string")return Np(r,e);var t=Object.prototype.toString.call(r).slice(8,-1);if(t==="Object"&&r.constructor&&(t=r.constructor.name),t==="Map"||t==="Set")return Array.from(r);if(t==="Arguments"||/^(?:Ui|I)nt(?:8|16|32)(?:Clamped)?Array$/.test(t))return Np(r,e)}}function Np(r,e){(e==null||e>r.length)&&(e=r.length);for(var t=0,i=new Array(e);t<e;t++)i[t]=r[t];return i}function Bp(r,e){for(var t=0;t<e.length;t++){var i=e[t];i.enumerable=i.enumerable||!1,i.configurable=!0,"value"in i&&(i.writable=!0),Object.defineProperty(r,i.key,i)}}function bk(r,e,t){return e&&Bp(r.prototype,e),t&&Bp(r,t),Object.defineProperty(r,"prototype",{writable:!1}),r}function wk(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,Pa(r,e)}function Pa(r,e){return Pa=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},Pa(r,e)}var vk=function(r){wk(e,r);function e(i){var n;return n=r.call(this,i)||this,n.nodes||(n.nodes=[]),n}var t=e.prototype;return t.append=function(n){return n.parent=this,this.nodes.push(n),this},t.prepend=function(n){return n.parent=this,this.nodes.unshift(n),this},t.at=function(n){return this.nodes[n]},t.index=function(n){return typeof n=="number"?n:this.nodes.indexOf(n)},t.removeChild=function(n){n=this.index(n),this.at(n).parent=void 0,this.nodes.splice(n,1);var s;for(var a in this.indexes)s=this.indexes[a],s>=n&&(this.indexes[a]=s-1);return this},t.removeAll=function(){for(var n=gk(this.nodes),s;!(s=n()).done;){var a=s.value;a.parent=void 0}return this.nodes=[],this},t.empty=function(){return this.removeAll()},t.insertAfter=function(n,s){s.parent=this;var a=this.index(n);this.nodes.splice(a+1,0,s),s.parent=this;var o;for(var l in this.indexes)o=this.indexes[l],a<=o&&(this.indexes[l]=o+1);return this},t.insertBefore=function(n,s){s.parent=this;var a=this.index(n);this.nodes.splice(a,0,s),s.parent=this;var o;for(var l in this.indexes)o=this.indexes[l],o<=a&&(this.indexes[l]=o+1);return this},t._findChildAtPosition=function(n,s){var a=void 0;return this.each(function(o){if(o.atPosition){var l=o.atPosition(n,s);if(l)return a=l,!1}else if(o.isAtPosition(n,s))return a=o,!1}),a},t.atPosition=function(n,s){if(this.isAtPosition(n,s))return this._findChildAtPosition(n,s)||this},t._inferEndPosition=function(){this.last&&this.last.source&&this.last.source.end&&(this.source=this.source||{},this.source.end=this.source.end||{},Object.assign(this.source.end,this.last.source.end))},t.each=function(n){this.lastEach||(this.lastEach=0),this.indexes||(this.indexes={}),this.lastEach++;var s=this.lastEach;if(this.indexes[s]=0,!!this.length){for(var a,o;this.indexes[s]<this.length&&(a=this.indexes[s],o=n(this.at(a),a),o!==!1);)this.indexes[s]+=1;if(delete this.indexes[s],o===!1)return!1}},t.walk=function(n){return this.each(function(s,a){var o=n(s,a);if(o!==!1&&s.length&&(o=s.walk(n)),o===!1)return!1})},t.walkAttributes=function(n){var s=this;return this.walk(function(a){if(a.type===ht.ATTRIBUTE)return n.call(s,a)})},t.walkClasses=function(n){var s=this;return this.walk(function(a){if(a.type===ht.CLASS)return n.call(s,a)})},t.walkCombinators=function(n){var s=this;return this.walk(function(a){if(a.type===ht.COMBINATOR)return n.call(s,a)})},t.walkComments=function(n){var s=this;return this.walk(function(a){if(a.type===ht.COMMENT)return n.call(s,a)})},t.walkIds=function(n){var s=this;return this.walk(function(a){if(a.type===ht.ID)return n.call(s,a)})},t.walkNesting=function(n){var s=this;return this.walk(function(a){if(a.type===ht.NESTING)return n.call(s,a)})},t.walkPseudos=function(n){var s=this;return this.walk(function(a){if(a.type===ht.PSEUDO)return n.call(s,a)})},t.walkTags=function(n){var s=this;return this.walk(function(a){if(a.type===ht.TAG)return n.call(s,a)})},t.walkUniversals=function(n){var s=this;return this.walk(function(a){if(a.type===ht.UNIVERSAL)return n.call(s,a)})},t.split=function(n){var s=this,a=[];return this.reduce(function(o,l,c){var f=n.call(s,l);return a.push(l),f?(o.push(a),a=[]):c===s.length-1&&o.push(a),o},[])},t.map=function(n){return this.nodes.map(n)},t.reduce=function(n,s){return this.nodes.reduce(n,s)},t.every=function(n){return this.nodes.every(n)},t.some=function(n){return this.nodes.some(n)},t.filter=function(n){return this.nodes.filter(n)},t.sort=function(n){return this.nodes.sort(n)},t.toString=function(){return this.map(String).join("")},bk(e,[{key:"first",get:function(){return this.at(0)}},{key:"last",get:function(){return this.at(this.length-1)}},{key:"length",get:function(){return this.nodes.length}}]),e}(dk.default);si.default=vk;Fp.exports=si.default});var Da=x((ai,zp)=>{u();"use strict";ai.__esModule=!0;ai.default=void 0;var xk=Sk(Ln()),kk=Se();function Sk(r){return r&&r.__esModule?r:{default:r}}function jp(r,e){for(var t=0;t<e.length;t++){var i=e[t];i.enumerable=i.enumerable||!1,i.configurable=!0,"value"in i&&(i.writable=!0),Object.defineProperty(r,i.key,i)}}function Ak(r,e,t){return e&&jp(r.prototype,e),t&&jp(r,t),Object.defineProperty(r,"prototype",{writable:!1}),r}function Ck(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,Ia(r,e)}function Ia(r,e){return Ia=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},Ia(r,e)}var _k=function(r){Ck(e,r);function e(i){var n;return n=r.call(this,i)||this,n.type=kk.ROOT,n}var t=e.prototype;return t.toString=function(){var n=this.reduce(function(s,a){return s.push(String(a)),s},[]).join(",");return this.trailingComma?n+",":n},t.error=function(n,s){return this._error?this._error(n,s):new Error(n)},Ak(e,[{key:"errorGenerator",set:function(n){this._error=n}}]),e}(xk.default);ai.default=_k;zp.exports=ai.default});var $a=x((oi,Up)=>{u();"use strict";oi.__esModule=!0;oi.default=void 0;var Ek=Tk(Ln()),Ok=Se();function Tk(r){return r&&r.__esModule?r:{default:r}}function Rk(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,qa(r,e)}function qa(r,e){return qa=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},qa(r,e)}var Pk=function(r){Rk(e,r);function e(t){var i;return i=r.call(this,t)||this,i.type=Ok.SELECTOR,i}return e}(Ek.default);oi.default=Pk;Up.exports=oi.default});var Mn=x((_I,Vp)=>{u();"use strict";var Ik={},Dk=Ik.hasOwnProperty,qk=function(e,t){if(!e)return t;var i={};for(var n in t)i[n]=Dk.call(e,n)?e[n]:t[n];return i},$k=/[ -,\.\/:-@\[-\^`\{-~]/,Lk=/[ -,\.\/:-@\[\]\^`\{-~]/,Mk=/(^|\\+)?(\\[A-F0-9]{1,6})\x20(?![a-fA-F0-9\x20])/g,La=function r(e,t){t=qk(t,r.options),t.quotes!="single"&&t.quotes!="double"&&(t.quotes="single");for(var i=t.quotes=="double"?'"':"'",n=t.isIdentifier,s=e.charAt(0),a="",o=0,l=e.length;o<l;){var c=e.charAt(o++),f=c.charCodeAt(),d=void 0;if(f<32||f>126){if(f>=55296&&f<=56319&&o<l){var p=e.charCodeAt(o++);(p&64512)==56320?f=((f&1023)<<10)+(p&1023)+65536:o--}d="\\"+f.toString(16).toUpperCase()+" "}else t.escapeEverything?$k.test(c)?d="\\"+c:d="\\"+f.toString(16).toUpperCase()+" ":/[\t\n\f\r\x0B]/.test(c)?d="\\"+f.toString(16).toUpperCase()+" ":c=="\\"||!n&&(c=='"'&&i==c||c=="'"&&i==c)||n&&Lk.test(c)?d="\\"+c:d=c;a+=d}return n&&(/^-[-\d]/.test(a)?a="\\-"+a.slice(1):/\d/.test(s)&&(a="\\3"+s+" "+a.slice(1))),a=a.replace(Mk,function(h,b,v){return b&&b.length%2?h:(b||"")+v}),!n&&t.wrap?i+a+i:a};La.options={escapeEverything:!1,isIdentifier:!1,quotes:"single",wrap:!1};La.version="3.0.0";Vp.exports=La});var Na=x((li,Gp)=>{u();"use strict";li.__esModule=!0;li.default=void 0;var Nk=Hp(Mn()),Bk=ii(),Fk=Hp(dt()),jk=Se();function Hp(r){return r&&r.__esModule?r:{default:r}}function Wp(r,e){for(var t=0;t<e.length;t++){var i=e[t];i.enumerable=i.enumerable||!1,i.configurable=!0,"value"in i&&(i.writable=!0),Object.defineProperty(r,i.key,i)}}function zk(r,e,t){return e&&Wp(r.prototype,e),t&&Wp(r,t),Object.defineProperty(r,"prototype",{writable:!1}),r}function Uk(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,Ma(r,e)}function Ma(r,e){return Ma=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},Ma(r,e)}var Vk=function(r){Uk(e,r);function e(i){var n;return n=r.call(this,i)||this,n.type=jk.CLASS,n._constructed=!0,n}var t=e.prototype;return t.valueToString=function(){return"."+r.prototype.valueToString.call(this)},zk(e,[{key:"value",get:function(){return this._value},set:function(n){if(this._constructed){var s=(0,Nk.default)(n,{isIdentifier:!0});s!==n?((0,Bk.ensureObject)(this,"raws"),this.raws.value=s):this.raws&&delete this.raws.value}this._value=n}}]),e}(Fk.default);li.default=Vk;Gp.exports=li.default});var Fa=x((ui,Qp)=>{u();"use strict";ui.__esModule=!0;ui.default=void 0;var Hk=Gk(dt()),Wk=Se();function Gk(r){return r&&r.__esModule?r:{default:r}}function Qk(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,Ba(r,e)}function Ba(r,e){return Ba=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},Ba(r,e)}var Yk=function(r){Qk(e,r);function e(t){var i;return i=r.call(this,t)||this,i.type=Wk.COMMENT,i}return e}(Hk.default);ui.default=Yk;Qp.exports=ui.default});var za=x((fi,Yp)=>{u();"use strict";fi.__esModule=!0;fi.default=void 0;var Kk=Zk(dt()),Xk=Se();function Zk(r){return r&&r.__esModule?r:{default:r}}function Jk(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,ja(r,e)}function ja(r,e){return ja=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},ja(r,e)}var eS=function(r){Jk(e,r);function e(i){var n;return n=r.call(this,i)||this,n.type=Xk.ID,n}var t=e.prototype;return t.valueToString=function(){return"#"+r.prototype.valueToString.call(this)},e}(Kk.default);fi.default=eS;Yp.exports=fi.default});var Nn=x((ci,Zp)=>{u();"use strict";ci.__esModule=!0;ci.default=void 0;var tS=Kp(Mn()),rS=ii(),iS=Kp(dt());function Kp(r){return r&&r.__esModule?r:{default:r}}function Xp(r,e){for(var t=0;t<e.length;t++){var i=e[t];i.enumerable=i.enumerable||!1,i.configurable=!0,"value"in i&&(i.writable=!0),Object.defineProperty(r,i.key,i)}}function nS(r,e,t){return e&&Xp(r.prototype,e),t&&Xp(r,t),Object.defineProperty(r,"prototype",{writable:!1}),r}function sS(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,Ua(r,e)}function Ua(r,e){return Ua=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},Ua(r,e)}var aS=function(r){sS(e,r);function e(){return r.apply(this,arguments)||this}var t=e.prototype;return t.qualifiedName=function(n){return this.namespace?this.namespaceString+"|"+n:n},t.valueToString=function(){return this.qualifiedName(r.prototype.valueToString.call(this))},nS(e,[{key:"namespace",get:function(){return this._namespace},set:function(n){if(n===!0||n==="*"||n==="&"){this._namespace=n,this.raws&&delete this.raws.namespace;return}var s=(0,tS.default)(n,{isIdentifier:!0});this._namespace=n,s!==n?((0,rS.ensureObject)(this,"raws"),this.raws.namespace=s):this.raws&&delete this.raws.namespace}},{key:"ns",get:function(){return this._namespace},set:function(n){this.namespace=n}},{key:"namespaceString",get:function(){if(this.namespace){var n=this.stringifyProperty("namespace");return n===!0?"":n}else return""}}]),e}(iS.default);ci.default=aS;Zp.exports=ci.default});var Ha=x((pi,Jp)=>{u();"use strict";pi.__esModule=!0;pi.default=void 0;var oS=uS(Nn()),lS=Se();function uS(r){return r&&r.__esModule?r:{default:r}}function fS(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,Va(r,e)}function Va(r,e){return Va=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},Va(r,e)}var cS=function(r){fS(e,r);function e(t){var i;return i=r.call(this,t)||this,i.type=lS.TAG,i}return e}(oS.default);pi.default=cS;Jp.exports=pi.default});var Ga=x((di,ed)=>{u();"use strict";di.__esModule=!0;di.default=void 0;var pS=hS(dt()),dS=Se();function hS(r){return r&&r.__esModule?r:{default:r}}function mS(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,Wa(r,e)}function Wa(r,e){return Wa=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},Wa(r,e)}var gS=function(r){mS(e,r);function e(t){var i;return i=r.call(this,t)||this,i.type=dS.STRING,i}return e}(pS.default);di.default=gS;ed.exports=di.default});var Ya=x((hi,td)=>{u();"use strict";hi.__esModule=!0;hi.default=void 0;var yS=wS(Ln()),bS=Se();function wS(r){return r&&r.__esModule?r:{default:r}}function vS(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,Qa(r,e)}function Qa(r,e){return Qa=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},Qa(r,e)}var xS=function(r){vS(e,r);function e(i){var n;return n=r.call(this,i)||this,n.type=bS.PSEUDO,n}var t=e.prototype;return t.toString=function(){var n=this.length?"("+this.map(String).join(",")+")":"";return[this.rawSpaceBefore,this.stringifyProperty("value"),n,this.rawSpaceAfter].join("")},e}(yS.default);hi.default=xS;td.exports=hi.default});var Bn={};Ge(Bn,{deprecate:()=>kS});function kS(r){return r}var Fn=P(()=>{u()});var id=x((EI,rd)=>{u();rd.exports=(Fn(),Bn).deprecate});var to=x(yi=>{u();"use strict";yi.__esModule=!0;yi.default=void 0;yi.unescapeValue=Ja;var mi=Xa(Mn()),SS=Xa(Pn()),AS=Xa(Nn()),CS=Se(),Ka;function Xa(r){return r&&r.__esModule?r:{default:r}}function nd(r,e){for(var t=0;t<e.length;t++){var i=e[t];i.enumerable=i.enumerable||!1,i.configurable=!0,"value"in i&&(i.writable=!0),Object.defineProperty(r,i.key,i)}}function _S(r,e,t){return e&&nd(r.prototype,e),t&&nd(r,t),Object.defineProperty(r,"prototype",{writable:!1}),r}function ES(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,Za(r,e)}function Za(r,e){return Za=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},Za(r,e)}var gi=id(),OS=/^('|")([^]*)\1$/,TS=gi(function(){},"Assigning an attribute a value containing characters that might need to be escaped is deprecated. Call attribute.setValue() instead."),RS=gi(function(){},"Assigning attr.quoted is deprecated and has no effect. Assign to attr.quoteMark instead."),PS=gi(function(){},"Constructing an Attribute selector with a value without specifying quoteMark is deprecated. Note: The value should be unescaped now.");function Ja(r){var e=!1,t=null,i=r,n=i.match(OS);return n&&(t=n[1],i=n[2]),i=(0,SS.default)(i),i!==r&&(e=!0),{deprecatedUsage:e,unescaped:i,quoteMark:t}}function IS(r){if(r.quoteMark!==void 0||r.value===void 0)return r;PS();var e=Ja(r.value),t=e.quoteMark,i=e.unescaped;return r.raws||(r.raws={}),r.raws.value===void 0&&(r.raws.value=r.value),r.value=i,r.quoteMark=t,r}var jn=function(r){ES(e,r);function e(i){var n;return i===void 0&&(i={}),n=r.call(this,IS(i))||this,n.type=CS.ATTRIBUTE,n.raws=n.raws||{},Object.defineProperty(n.raws,"unquoted",{get:gi(function(){return n.value},"attr.raws.unquoted is deprecated. Call attr.value instead."),set:gi(function(){return n.value},"Setting attr.raws.unquoted is deprecated and has no effect. attr.value is unescaped by default now.")}),n._constructed=!0,n}var t=e.prototype;return t.getQuotedValue=function(n){n===void 0&&(n={});var s=this._determineQuoteMark(n),a=eo[s],o=(0,mi.default)(this._value,a);return o},t._determineQuoteMark=function(n){return n.smart?this.smartQuoteMark(n):this.preferredQuoteMark(n)},t.setValue=function(n,s){s===void 0&&(s={}),this._value=n,this._quoteMark=this._determineQuoteMark(s),this._syncRawValue()},t.smartQuoteMark=function(n){var s=this.value,a=s.replace(/[^']/g,"").length,o=s.replace(/[^"]/g,"").length;if(a+o===0){var l=(0,mi.default)(s,{isIdentifier:!0});if(l===s)return e.NO_QUOTE;var c=this.preferredQuoteMark(n);if(c===e.NO_QUOTE){var f=this.quoteMark||n.quoteMark||e.DOUBLE_QUOTE,d=eo[f],p=(0,mi.default)(s,d);if(p.length<l.length)return f}return c}else return o===a?this.preferredQuoteMark(n):o<a?e.DOUBLE_QUOTE:e.SINGLE_QUOTE},t.preferredQuoteMark=function(n){var s=n.preferCurrentQuoteMark?this.quoteMark:n.quoteMark;return s===void 0&&(s=n.preferCurrentQuoteMark?n.quoteMark:this.quoteMark),s===void 0&&(s=e.DOUBLE_QUOTE),s},t._syncRawValue=function(){var n=(0,mi.default)(this._value,eo[this.quoteMark]);n===this._value?this.raws&&delete this.raws.value:this.raws.value=n},t._handleEscapes=function(n,s){if(this._constructed){var a=(0,mi.default)(s,{isIdentifier:!0});a!==s?this.raws[n]=a:delete this.raws[n]}},t._spacesFor=function(n){var s={before:"",after:""},a=this.spaces[n]||{},o=this.raws.spaces&&this.raws.spaces[n]||{};return Object.assign(s,a,o)},t._stringFor=function(n,s,a){s===void 0&&(s=n),a===void 0&&(a=sd);var o=this._spacesFor(s);return a(this.stringifyProperty(n),o)},t.offsetOf=function(n){var s=1,a=this._spacesFor("attribute");if(s+=a.before.length,n==="namespace"||n==="ns")return this.namespace?s:-1;if(n==="attributeNS"||(s+=this.namespaceString.length,this.namespace&&(s+=1),n==="attribute"))return s;s+=this.stringifyProperty("attribute").length,s+=a.after.length;var o=this._spacesFor("operator");s+=o.before.length;var l=this.stringifyProperty("operator");if(n==="operator")return l?s:-1;s+=l.length,s+=o.after.length;var c=this._spacesFor("value");s+=c.before.length;var f=this.stringifyProperty("value");if(n==="value")return f?s:-1;s+=f.length,s+=c.after.length;var d=this._spacesFor("insensitive");return s+=d.before.length,n==="insensitive"&&this.insensitive?s:-1},t.toString=function(){var n=this,s=[this.rawSpaceBefore,"["];return s.push(this._stringFor("qualifiedAttribute","attribute")),this.operator&&(this.value||this.value==="")&&(s.push(this._stringFor("operator")),s.push(this._stringFor("value")),s.push(this._stringFor("insensitiveFlag","insensitive",function(a,o){return a.length>0&&!n.quoted&&o.before.length===0&&!(n.spaces.value&&n.spaces.value.after)&&(o.before=" "),sd(a,o)}))),s.push("]"),s.push(this.rawSpaceAfter),s.join("")},_S(e,[{key:"quoted",get:function(){var n=this.quoteMark;return n==="'"||n==='"'},set:function(n){RS()}},{key:"quoteMark",get:function(){return this._quoteMark},set:function(n){if(!this._constructed){this._quoteMark=n;return}this._quoteMark!==n&&(this._quoteMark=n,this._syncRawValue())}},{key:"qualifiedAttribute",get:function(){return this.qualifiedName(this.raws.attribute||this.attribute)}},{key:"insensitiveFlag",get:function(){return this.insensitive?"i":""}},{key:"value",get:function(){return this._value},set:function(n){if(this._constructed){var s=Ja(n),a=s.deprecatedUsage,o=s.unescaped,l=s.quoteMark;if(a&&TS(),o===this._value&&l===this._quoteMark)return;this._value=o,this._quoteMark=l,this._syncRawValue()}else this._value=n}},{key:"insensitive",get:function(){return this._insensitive},set:function(n){n||(this._insensitive=!1,this.raws&&(this.raws.insensitiveFlag==="I"||this.raws.insensitiveFlag==="i")&&(this.raws.insensitiveFlag=void 0)),this._insensitive=n}},{key:"attribute",get:function(){return this._attribute},set:function(n){this._handleEscapes("attribute",n),this._attribute=n}}]),e}(AS.default);yi.default=jn;jn.NO_QUOTE=null;jn.SINGLE_QUOTE="'";jn.DOUBLE_QUOTE='"';var eo=(Ka={"'":{quotes:"single",wrap:!0},'"':{quotes:"double",wrap:!0}},Ka[null]={isIdentifier:!0},Ka);function sd(r,e){return""+e.before+r+e.after}});var io=x((bi,ad)=>{u();"use strict";bi.__esModule=!0;bi.default=void 0;var DS=$S(Nn()),qS=Se();function $S(r){return r&&r.__esModule?r:{default:r}}function LS(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,ro(r,e)}function ro(r,e){return ro=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},ro(r,e)}var MS=function(r){LS(e,r);function e(t){var i;return i=r.call(this,t)||this,i.type=qS.UNIVERSAL,i.value="*",i}return e}(DS.default);bi.default=MS;ad.exports=bi.default});var so=x((wi,od)=>{u();"use strict";wi.__esModule=!0;wi.default=void 0;var NS=FS(dt()),BS=Se();function FS(r){return r&&r.__esModule?r:{default:r}}function jS(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,no(r,e)}function no(r,e){return no=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},no(r,e)}var zS=function(r){jS(e,r);function e(t){var i;return i=r.call(this,t)||this,i.type=BS.COMBINATOR,i}return e}(NS.default);wi.default=zS;od.exports=wi.default});var oo=x((vi,ld)=>{u();"use strict";vi.__esModule=!0;vi.default=void 0;var US=HS(dt()),VS=Se();function HS(r){return r&&r.__esModule?r:{default:r}}function WS(r,e){r.prototype=Object.create(e.prototype),r.prototype.constructor=r,ao(r,e)}function ao(r,e){return ao=Object.setPrototypeOf?Object.setPrototypeOf.bind():function(i,n){return i.__proto__=n,i},ao(r,e)}var GS=function(r){WS(e,r);function e(t){var i;return i=r.call(this,t)||this,i.type=VS.NESTING,i.value="&",i}return e}(US.default);vi.default=GS;ld.exports=vi.default});var fd=x((zn,ud)=>{u();"use strict";zn.__esModule=!0;zn.default=QS;function QS(r){return r.sort(function(e,t){return e-t})}ud.exports=zn.default});var lo=x(M=>{u();"use strict";M.__esModule=!0;M.word=M.tilde=M.tab=M.str=M.space=M.slash=M.singleQuote=M.semicolon=M.plus=M.pipe=M.openSquare=M.openParenthesis=M.newline=M.greaterThan=M.feed=M.equals=M.doubleQuote=M.dollar=M.cr=M.comment=M.comma=M.combinator=M.colon=M.closeSquare=M.closeParenthesis=M.caret=M.bang=M.backslash=M.at=M.asterisk=M.ampersand=void 0;var YS=38;M.ampersand=YS;var KS=42;M.asterisk=KS;var XS=64;M.at=XS;var ZS=44;M.comma=ZS;var JS=58;M.colon=JS;var eA=59;M.semicolon=eA;var tA=40;M.openParenthesis=tA;var rA=41;M.closeParenthesis=rA;var iA=91;M.openSquare=iA;var nA=93;M.closeSquare=nA;var sA=36;M.dollar=sA;var aA=126;M.tilde=aA;var oA=94;M.caret=oA;var lA=43;M.plus=lA;var uA=61;M.equals=uA;var fA=124;M.pipe=fA;var cA=62;M.greaterThan=cA;var pA=32;M.space=pA;var cd=39;M.singleQuote=cd;var dA=34;M.doubleQuote=dA;var hA=47;M.slash=hA;var mA=33;M.bang=mA;var gA=92;M.backslash=gA;var yA=13;M.cr=yA;var bA=12;M.feed=bA;var wA=10;M.newline=wA;var vA=9;M.tab=vA;var xA=cd;M.str=xA;var kA=-1;M.comment=kA;var SA=-2;M.word=SA;var AA=-3;M.combinator=AA});var hd=x(xi=>{u();"use strict";xi.__esModule=!0;xi.FIELDS=void 0;xi.default=PA;var D=CA(lo()),nr,te;function pd(r){if(typeof WeakMap!="function")return null;var e=new WeakMap,t=new WeakMap;return(pd=function(n){return n?t:e})(r)}function CA(r,e){if(!e&&r&&r.__esModule)return r;if(r===null||typeof r!="object"&&typeof r!="function")return{default:r};var t=pd(e);if(t&&t.has(r))return t.get(r);var i={},n=Object.defineProperty&&Object.getOwnPropertyDescriptor;for(var s in r)if(s!=="default"&&Object.prototype.hasOwnProperty.call(r,s)){var a=n?Object.getOwnPropertyDescriptor(r,s):null;a&&(a.get||a.set)?Object.defineProperty(i,s,a):i[s]=r[s]}return i.default=r,t&&t.set(r,i),i}var _A=(nr={},nr[D.tab]=!0,nr[D.newline]=!0,nr[D.cr]=!0,nr[D.feed]=!0,nr),EA=(te={},te[D.space]=!0,te[D.tab]=!0,te[D.newline]=!0,te[D.cr]=!0,te[D.feed]=!0,te[D.ampersand]=!0,te[D.asterisk]=!0,te[D.bang]=!0,te[D.comma]=!0,te[D.colon]=!0,te[D.semicolon]=!0,te[D.openParenthesis]=!0,te[D.closeParenthesis]=!0,te[D.openSquare]=!0,te[D.closeSquare]=!0,te[D.singleQuote]=!0,te[D.doubleQuote]=!0,te[D.plus]=!0,te[D.pipe]=!0,te[D.tilde]=!0,te[D.greaterThan]=!0,te[D.equals]=!0,te[D.dollar]=!0,te[D.caret]=!0,te[D.slash]=!0,te),uo={},dd="0123456789abcdefABCDEF";for(Un=0;Un<dd.length;Un++)uo[dd.charCodeAt(Un)]=!0;var Un;function OA(r,e){var t=e,i;do{if(i=r.charCodeAt(t),EA[i])return t-1;i===D.backslash?t=TA(r,t)+1:t++}while(t<r.length);return t-1}function TA(r,e){var t=e,i=r.charCodeAt(t+1);if(!_A[i])if(uo[i]){var n=0;do t++,n++,i=r.charCodeAt(t+1);while(uo[i]&&n<6);n<6&&i===D.space&&t++}else t++;return t}var RA={TYPE:0,START_LINE:1,START_COL:2,END_LINE:3,END_COL:4,START_POS:5,END_POS:6};xi.FIELDS=RA;function PA(r){var e=[],t=r.css.valueOf(),i=t,n=i.length,s=-1,a=1,o=0,l=0,c,f,d,p,h,b,v,y,w,k,S,E,T;function B(N,R){if(r.safe)t+=R,w=t.length-1;else throw r.error("Unclosed "+N,a,o-s,o)}for(;o<n;){switch(c=t.charCodeAt(o),c===D.newline&&(s=o,a+=1),c){case D.space:case D.tab:case D.newline:case D.cr:case D.feed:w=o;do w+=1,c=t.charCodeAt(w),c===D.newline&&(s=w,a+=1);while(c===D.space||c===D.newline||c===D.tab||c===D.cr||c===D.feed);T=D.space,p=a,d=w-s-1,l=w;break;case D.plus:case D.greaterThan:case D.tilde:case D.pipe:w=o;do w+=1,c=t.charCodeAt(w);while(c===D.plus||c===D.greaterThan||c===D.tilde||c===D.pipe);T=D.combinator,p=a,d=o-s,l=w;break;case D.asterisk:case D.ampersand:case D.bang:case D.comma:case D.equals:case D.dollar:case D.caret:case D.openSquare:case D.closeSquare:case D.colon:case D.semicolon:case D.openParenthesis:case D.closeParenthesis:w=o,T=c,p=a,d=o-s,l=w+1;break;case D.singleQuote:case D.doubleQuote:E=c===D.singleQuote?"'":'"',w=o;do for(h=!1,w=t.indexOf(E,w+1),w===-1&&B("quote",E),b=w;t.charCodeAt(b-1)===D.backslash;)b-=1,h=!h;while(h);T=D.str,p=a,d=o-s,l=w+1;break;default:c===D.slash&&t.charCodeAt(o+1)===D.asterisk?(w=t.indexOf("*/",o+2)+1,w===0&&B("comment","*/"),f=t.slice(o,w+1),y=f.split(`
`),v=y.length-1,v>0?(k=a+v,S=w-y[v].length):(k=a,S=s),T=D.comment,a=k,p=k,d=w-S):c===D.slash?(w=o,T=c,p=a,d=o-s,l=w+1):(w=OA(t,o),T=D.word,p=a,d=w-s),l=w+1;break}e.push([T,a,o-s,p,d,o,l]),S&&(s=S,S=null),o=l}return e}});var kd=x((ki,xd)=>{u();"use strict";ki.__esModule=!0;ki.default=void 0;var IA=je(Da()),fo=je($a()),DA=je(Na()),md=je(Fa()),qA=je(za()),$A=je(Ha()),co=je(Ga()),LA=je(Ya()),gd=Vn(to()),MA=je(io()),po=je(so()),NA=je(oo()),BA=je(fd()),O=Vn(hd()),q=Vn(lo()),FA=Vn(Se()),ue=ii(),Vt,ho;function yd(r){if(typeof WeakMap!="function")return null;var e=new WeakMap,t=new WeakMap;return(yd=function(n){return n?t:e})(r)}function Vn(r,e){if(!e&&r&&r.__esModule)return r;if(r===null||typeof r!="object"&&typeof r!="function")return{default:r};var t=yd(e);if(t&&t.has(r))return t.get(r);var i={},n=Object.defineProperty&&Object.getOwnPropertyDescriptor;for(var s in r)if(s!=="default"&&Object.prototype.hasOwnProperty.call(r,s)){var a=n?Object.getOwnPropertyDescriptor(r,s):null;a&&(a.get||a.set)?Object.defineProperty(i,s,a):i[s]=r[s]}return i.default=r,t&&t.set(r,i),i}function je(r){return r&&r.__esModule?r:{default:r}}function bd(r,e){for(var t=0;t<e.length;t++){var i=e[t];i.enumerable=i.enumerable||!1,i.configurable=!0,"value"in i&&(i.writable=!0),Object.defineProperty(r,i.key,i)}}function jA(r,e,t){return e&&bd(r.prototype,e),t&&bd(r,t),Object.defineProperty(r,"prototype",{writable:!1}),r}var mo=(Vt={},Vt[q.space]=!0,Vt[q.cr]=!0,Vt[q.feed]=!0,Vt[q.newline]=!0,Vt[q.tab]=!0,Vt),zA=Object.assign({},mo,(ho={},ho[q.comment]=!0,ho));function wd(r){return{line:r[O.FIELDS.START_LINE],column:r[O.FIELDS.START_COL]}}function vd(r){return{line:r[O.FIELDS.END_LINE],column:r[O.FIELDS.END_COL]}}function Ht(r,e,t,i){return{start:{line:r,column:e},end:{line:t,column:i}}}function sr(r){return Ht(r[O.FIELDS.START_LINE],r[O.FIELDS.START_COL],r[O.FIELDS.END_LINE],r[O.FIELDS.END_COL])}function go(r,e){if(!!r)return Ht(r[O.FIELDS.START_LINE],r[O.FIELDS.START_COL],e[O.FIELDS.END_LINE],e[O.FIELDS.END_COL])}function ar(r,e){var t=r[e];if(typeof t=="string")return t.indexOf("\\")!==-1&&((0,ue.ensureObject)(r,"raws"),r[e]=(0,ue.unesc)(t),r.raws[e]===void 0&&(r.raws[e]=t)),r}function yo(r,e){for(var t=-1,i=[];(t=r.indexOf(e,t+1))!==-1;)i.push(t);return i}function UA(){var r=Array.prototype.concat.apply([],arguments);return r.filter(function(e,t){return t===r.indexOf(e)})}var VA=function(){function r(t,i){i===void 0&&(i={}),this.rule=t,this.options=Object.assign({lossy:!1,safe:!1},i),this.position=0,this.css=typeof this.rule=="string"?this.rule:this.rule.selector,this.tokens=(0,O.default)({css:this.css,error:this._errorGenerator(),safe:this.options.safe});var n=go(this.tokens[0],this.tokens[this.tokens.length-1]);this.root=new IA.default({source:n}),this.root.errorGenerator=this._errorGenerator();var s=new fo.default({source:{start:{line:1,column:1}},sourceIndex:0});this.root.append(s),this.current=s,this.loop()}var e=r.prototype;return e._errorGenerator=function(){var i=this;return function(n,s){return typeof i.rule=="string"?new Error(n):i.rule.error(n,s)}},e.attribute=function(){var i=[],n=this.currToken;for(this.position++;this.position<this.tokens.length&&this.currToken[O.FIELDS.TYPE]!==q.closeSquare;)i.push(this.currToken),this.position++;if(this.currToken[O.FIELDS.TYPE]!==q.closeSquare)return this.expected("closing square bracket",this.currToken[O.FIELDS.START_POS]);var s=i.length,a={source:Ht(n[1],n[2],this.currToken[3],this.currToken[4]),sourceIndex:n[O.FIELDS.START_POS]};if(s===1&&!~[q.word].indexOf(i[0][O.FIELDS.TYPE]))return this.expected("attribute",i[0][O.FIELDS.START_POS]);for(var o=0,l="",c="",f=null,d=!1;o<s;){var p=i[o],h=this.content(p),b=i[o+1];switch(p[O.FIELDS.TYPE]){case q.space:if(d=!0,this.options.lossy)break;if(f){(0,ue.ensureObject)(a,"spaces",f);var v=a.spaces[f].after||"";a.spaces[f].after=v+h;var y=(0,ue.getProp)(a,"raws","spaces",f,"after")||null;y&&(a.raws.spaces[f].after=y+h)}else l=l+h,c=c+h;break;case q.asterisk:if(b[O.FIELDS.TYPE]===q.equals)a.operator=h,f="operator";else if((!a.namespace||f==="namespace"&&!d)&&b){l&&((0,ue.ensureObject)(a,"spaces","attribute"),a.spaces.attribute.before=l,l=""),c&&((0,ue.ensureObject)(a,"raws","spaces","attribute"),a.raws.spaces.attribute.before=l,c=""),a.namespace=(a.namespace||"")+h;var w=(0,ue.getProp)(a,"raws","namespace")||null;w&&(a.raws.namespace+=h),f="namespace"}d=!1;break;case q.dollar:if(f==="value"){var k=(0,ue.getProp)(a,"raws","value");a.value+="$",k&&(a.raws.value=k+"$");break}case q.caret:b[O.FIELDS.TYPE]===q.equals&&(a.operator=h,f="operator"),d=!1;break;case q.combinator:if(h==="~"&&b[O.FIELDS.TYPE]===q.equals&&(a.operator=h,f="operator"),h!=="|"){d=!1;break}b[O.FIELDS.TYPE]===q.equals?(a.operator=h,f="operator"):!a.namespace&&!a.attribute&&(a.namespace=!0),d=!1;break;case q.word:if(b&&this.content(b)==="|"&&i[o+2]&&i[o+2][O.FIELDS.TYPE]!==q.equals&&!a.operator&&!a.namespace)a.namespace=h,f="namespace";else if(!a.attribute||f==="attribute"&&!d){l&&((0,ue.ensureObject)(a,"spaces","attribute"),a.spaces.attribute.before=l,l=""),c&&((0,ue.ensureObject)(a,"raws","spaces","attribute"),a.raws.spaces.attribute.before=c,c=""),a.attribute=(a.attribute||"")+h;var S=(0,ue.getProp)(a,"raws","attribute")||null;S&&(a.raws.attribute+=h),f="attribute"}else if(!a.value&&a.value!==""||f==="value"&&!(d||a.quoteMark)){var E=(0,ue.unesc)(h),T=(0,ue.getProp)(a,"raws","value")||"",B=a.value||"";a.value=B+E,a.quoteMark=null,(E!==h||T)&&((0,ue.ensureObject)(a,"raws"),a.raws.value=(T||B)+h),f="value"}else{var N=h==="i"||h==="I";(a.value||a.value==="")&&(a.quoteMark||d)?(a.insensitive=N,(!N||h==="I")&&((0,ue.ensureObject)(a,"raws"),a.raws.insensitiveFlag=h),f="insensitive",l&&((0,ue.ensureObject)(a,"spaces","insensitive"),a.spaces.insensitive.before=l,l=""),c&&((0,ue.ensureObject)(a,"raws","spaces","insensitive"),a.raws.spaces.insensitive.before=c,c="")):(a.value||a.value==="")&&(f="value",a.value+=h,a.raws.value&&(a.raws.value+=h))}d=!1;break;case q.str:if(!a.attribute||!a.operator)return this.error("Expected an attribute followed by an operator preceding the string.",{index:p[O.FIELDS.START_POS]});var R=(0,gd.unescapeValue)(h),F=R.unescaped,Y=R.quoteMark;a.value=F,a.quoteMark=Y,f="value",(0,ue.ensureObject)(a,"raws"),a.raws.value=h,d=!1;break;case q.equals:if(!a.attribute)return this.expected("attribute",p[O.FIELDS.START_POS],h);if(a.value)return this.error('Unexpected "=" found; an operator was already defined.',{index:p[O.FIELDS.START_POS]});a.operator=a.operator?a.operator+h:h,f="operator",d=!1;break;case q.comment:if(f)if(d||b&&b[O.FIELDS.TYPE]===q.space||f==="insensitive"){var _=(0,ue.getProp)(a,"spaces",f,"after")||"",Q=(0,ue.getProp)(a,"raws","spaces",f,"after")||_;(0,ue.ensureObject)(a,"raws","spaces",f),a.raws.spaces[f].after=Q+h}else{var U=a[f]||"",le=(0,ue.getProp)(a,"raws",f)||U;(0,ue.ensureObject)(a,"raws"),a.raws[f]=le+h}else c=c+h;break;default:return this.error('Unexpected "'+h+'" found.',{index:p[O.FIELDS.START_POS]})}o++}ar(a,"attribute"),ar(a,"namespace"),this.newNode(new gd.default(a)),this.position++},e.parseWhitespaceEquivalentTokens=function(i){i<0&&(i=this.tokens.length);var n=this.position,s=[],a="",o=void 0;do if(mo[this.currToken[O.FIELDS.TYPE]])this.options.lossy||(a+=this.content());else if(this.currToken[O.FIELDS.TYPE]===q.comment){var l={};a&&(l.before=a,a=""),o=new md.default({value:this.content(),source:sr(this.currToken),sourceIndex:this.currToken[O.FIELDS.START_POS],spaces:l}),s.push(o)}while(++this.position<i);if(a){if(o)o.spaces.after=a;else if(!this.options.lossy){var c=this.tokens[n],f=this.tokens[this.position-1];s.push(new co.default({value:"",source:Ht(c[O.FIELDS.START_LINE],c[O.FIELDS.START_COL],f[O.FIELDS.END_LINE],f[O.FIELDS.END_COL]),sourceIndex:c[O.FIELDS.START_POS],spaces:{before:a,after:""}}))}}return s},e.convertWhitespaceNodesToSpace=function(i,n){var s=this;n===void 0&&(n=!1);var a="",o="";i.forEach(function(c){var f=s.lossySpace(c.spaces.before,n),d=s.lossySpace(c.rawSpaceBefore,n);a+=f+s.lossySpace(c.spaces.after,n&&f.length===0),o+=f+c.value+s.lossySpace(c.rawSpaceAfter,n&&d.length===0)}),o===a&&(o=void 0);var l={space:a,rawSpace:o};return l},e.isNamedCombinator=function(i){return i===void 0&&(i=this.position),this.tokens[i+0]&&this.tokens[i+0][O.FIELDS.TYPE]===q.slash&&this.tokens[i+1]&&this.tokens[i+1][O.FIELDS.TYPE]===q.word&&this.tokens[i+2]&&this.tokens[i+2][O.FIELDS.TYPE]===q.slash},e.namedCombinator=function(){if(this.isNamedCombinator()){var i=this.content(this.tokens[this.position+1]),n=(0,ue.unesc)(i).toLowerCase(),s={};n!==i&&(s.value="/"+i+"/");var a=new po.default({value:"/"+n+"/",source:Ht(this.currToken[O.FIELDS.START_LINE],this.currToken[O.FIELDS.START_COL],this.tokens[this.position+2][O.FIELDS.END_LINE],this.tokens[this.position+2][O.FIELDS.END_COL]),sourceIndex:this.currToken[O.FIELDS.START_POS],raws:s});return this.position=this.position+3,a}else this.unexpected()},e.combinator=function(){var i=this;if(this.content()==="|")return this.namespace();var n=this.locateNextMeaningfulToken(this.position);if(n<0||this.tokens[n][O.FIELDS.TYPE]===q.comma||this.tokens[n][O.FIELDS.TYPE]===q.closeParenthesis){var s=this.parseWhitespaceEquivalentTokens(n);if(s.length>0){var a=this.current.last;if(a){var o=this.convertWhitespaceNodesToSpace(s),l=o.space,c=o.rawSpace;c!==void 0&&(a.rawSpaceAfter+=c),a.spaces.after+=l}else s.forEach(function(T){return i.newNode(T)})}return}var f=this.currToken,d=void 0;n>this.position&&(d=this.parseWhitespaceEquivalentTokens(n));var p;if(this.isNamedCombinator()?p=this.namedCombinator():this.currToken[O.FIELDS.TYPE]===q.combinator?(p=new po.default({value:this.content(),source:sr(this.currToken),sourceIndex:this.currToken[O.FIELDS.START_POS]}),this.position++):mo[this.currToken[O.FIELDS.TYPE]]||d||this.unexpected(),p){if(d){var h=this.convertWhitespaceNodesToSpace(d),b=h.space,v=h.rawSpace;p.spaces.before=b,p.rawSpaceBefore=v}}else{var y=this.convertWhitespaceNodesToSpace(d,!0),w=y.space,k=y.rawSpace;k||(k=w);var S={},E={spaces:{}};w.endsWith(" ")&&k.endsWith(" ")?(S.before=w.slice(0,w.length-1),E.spaces.before=k.slice(0,k.length-1)):w.startsWith(" ")&&k.startsWith(" ")?(S.after=w.slice(1),E.spaces.after=k.slice(1)):E.value=k,p=new po.default({value:" ",source:go(f,this.tokens[this.position-1]),sourceIndex:f[O.FIELDS.START_POS],spaces:S,raws:E})}return this.currToken&&this.currToken[O.FIELDS.TYPE]===q.space&&(p.spaces.after=this.optionalSpace(this.content()),this.position++),this.newNode(p)},e.comma=function(){if(this.position===this.tokens.length-1){this.root.trailingComma=!0,this.position++;return}this.current._inferEndPosition();var i=new fo.default({source:{start:wd(this.tokens[this.position+1])},sourceIndex:this.tokens[this.position+1][O.FIELDS.START_POS]});this.current.parent.append(i),this.current=i,this.position++},e.comment=function(){var i=this.currToken;this.newNode(new md.default({value:this.content(),source:sr(i),sourceIndex:i[O.FIELDS.START_POS]})),this.position++},e.error=function(i,n){throw this.root.error(i,n)},e.missingBackslash=function(){return this.error("Expected a backslash preceding the semicolon.",{index:this.currToken[O.FIELDS.START_POS]})},e.missingParenthesis=function(){return this.expected("opening parenthesis",this.currToken[O.FIELDS.START_POS])},e.missingSquareBracket=function(){return this.expected("opening square bracket",this.currToken[O.FIELDS.START_POS])},e.unexpected=function(){return this.error("Unexpected '"+this.content()+"'. Escaping special characters with \\ may help.",this.currToken[O.FIELDS.START_POS])},e.unexpectedPipe=function(){return this.error("Unexpected '|'.",this.currToken[O.FIELDS.START_POS])},e.namespace=function(){var i=this.prevToken&&this.content(this.prevToken)||!0;if(this.nextToken[O.FIELDS.TYPE]===q.word)return this.position++,this.word(i);if(this.nextToken[O.FIELDS.TYPE]===q.asterisk)return this.position++,this.universal(i);this.unexpectedPipe()},e.nesting=function(){if(this.nextToken){var i=this.content(this.nextToken);if(i==="|"){this.position++;return}}var n=this.currToken;this.newNode(new NA.default({value:this.content(),source:sr(n),sourceIndex:n[O.FIELDS.START_POS]})),this.position++},e.parentheses=function(){var i=this.current.last,n=1;if(this.position++,i&&i.type===FA.PSEUDO){var s=new fo.default({source:{start:wd(this.tokens[this.position])},sourceIndex:this.tokens[this.position][O.FIELDS.START_POS]}),a=this.current;for(i.append(s),this.current=s;this.position<this.tokens.length&&n;)this.currToken[O.FIELDS.TYPE]===q.openParenthesis&&n++,this.currToken[O.FIELDS.TYPE]===q.closeParenthesis&&n--,n?this.parse():(this.current.source.end=vd(this.currToken),this.current.parent.source.end=vd(this.currToken),this.position++);this.current=a}else{for(var o=this.currToken,l="(",c;this.position<this.tokens.length&&n;)this.currToken[O.FIELDS.TYPE]===q.openParenthesis&&n++,this.currToken[O.FIELDS.TYPE]===q.closeParenthesis&&n--,c=this.currToken,l+=this.parseParenthesisToken(this.currToken),this.position++;i?i.appendToPropertyAndEscape("value",l,l):this.newNode(new co.default({value:l,source:Ht(o[O.FIELDS.START_LINE],o[O.FIELDS.START_COL],c[O.FIELDS.END_LINE],c[O.FIELDS.END_COL]),sourceIndex:o[O.FIELDS.START_POS]}))}if(n)return this.expected("closing parenthesis",this.currToken[O.FIELDS.START_POS])},e.pseudo=function(){for(var i=this,n="",s=this.currToken;this.currToken&&this.currToken[O.FIELDS.TYPE]===q.colon;)n+=this.content(),this.position++;if(!this.currToken)return this.expected(["pseudo-class","pseudo-element"],this.position-1);if(this.currToken[O.FIELDS.TYPE]===q.word)this.splitWord(!1,function(a,o){n+=a,i.newNode(new LA.default({value:n,source:go(s,i.currToken),sourceIndex:s[O.FIELDS.START_POS]})),o>1&&i.nextToken&&i.nextToken[O.FIELDS.TYPE]===q.openParenthesis&&i.error("Misplaced parenthesis.",{index:i.nextToken[O.FIELDS.START_POS]})});else return this.expected(["pseudo-class","pseudo-element"],this.currToken[O.FIELDS.START_POS])},e.space=function(){var i=this.content();this.position===0||this.prevToken[O.FIELDS.TYPE]===q.comma||this.prevToken[O.FIELDS.TYPE]===q.openParenthesis||this.current.nodes.every(function(n){return n.type==="comment"})?(this.spaces=this.optionalSpace(i),this.position++):this.position===this.tokens.length-1||this.nextToken[O.FIELDS.TYPE]===q.comma||this.nextToken[O.FIELDS.TYPE]===q.closeParenthesis?(this.current.last.spaces.after=this.optionalSpace(i),this.position++):this.combinator()},e.string=function(){var i=this.currToken;this.newNode(new co.default({value:this.content(),source:sr(i),sourceIndex:i[O.FIELDS.START_POS]})),this.position++},e.universal=function(i){var n=this.nextToken;if(n&&this.content(n)==="|")return this.position++,this.namespace();var s=this.currToken;this.newNode(new MA.default({value:this.content(),source:sr(s),sourceIndex:s[O.FIELDS.START_POS]}),i),this.position++},e.splitWord=function(i,n){for(var s=this,a=this.nextToken,o=this.content();a&&~[q.dollar,q.caret,q.equals,q.word].indexOf(a[O.FIELDS.TYPE]);){this.position++;var l=this.content();if(o+=l,l.lastIndexOf("\\")===l.length-1){var c=this.nextToken;c&&c[O.FIELDS.TYPE]===q.space&&(o+=this.requiredSpace(this.content(c)),this.position++)}a=this.nextToken}var f=yo(o,".").filter(function(b){var v=o[b-1]==="\\",y=/^\d+\.\d+%$/.test(o);return!v&&!y}),d=yo(o,"#").filter(function(b){return o[b-1]!=="\\"}),p=yo(o,"#{");p.length&&(d=d.filter(function(b){return!~p.indexOf(b)}));var h=(0,BA.default)(UA([0].concat(f,d)));h.forEach(function(b,v){var y=h[v+1]||o.length,w=o.slice(b,y);if(v===0&&n)return n.call(s,w,h.length);var k,S=s.currToken,E=S[O.FIELDS.START_POS]+h[v],T=Ht(S[1],S[2]+b,S[3],S[2]+(y-1));if(~f.indexOf(b)){var B={value:w.slice(1),source:T,sourceIndex:E};k=new DA.default(ar(B,"value"))}else if(~d.indexOf(b)){var N={value:w.slice(1),source:T,sourceIndex:E};k=new qA.default(ar(N,"value"))}else{var R={value:w,source:T,sourceIndex:E};ar(R,"value"),k=new $A.default(R)}s.newNode(k,i),i=null}),this.position++},e.word=function(i){var n=this.nextToken;return n&&this.content(n)==="|"?(this.position++,this.namespace()):this.splitWord(i)},e.loop=function(){for(;this.position<this.tokens.length;)this.parse(!0);return this.current._inferEndPosition(),this.root},e.parse=function(i){switch(this.currToken[O.FIELDS.TYPE]){case q.space:this.space();break;case q.comment:this.comment();break;case q.openParenthesis:this.parentheses();break;case q.closeParenthesis:i&&this.missingParenthesis();break;case q.openSquare:this.attribute();break;case q.dollar:case q.caret:case q.equals:case q.word:this.word();break;case q.colon:this.pseudo();break;case q.comma:this.comma();break;case q.asterisk:this.universal();break;case q.ampersand:this.nesting();break;case q.slash:case q.combinator:this.combinator();break;case q.str:this.string();break;case q.closeSquare:this.missingSquareBracket();case q.semicolon:this.missingBackslash();default:this.unexpected()}},e.expected=function(i,n,s){if(Array.isArray(i)){var a=i.pop();i=i.join(", ")+" or "+a}var o=/^[aeiou]/.test(i[0])?"an":"a";return s?this.error("Expected "+o+" "+i+', found "'+s+'" instead.',{index:n}):this.error("Expected "+o+" "+i+".",{index:n})},e.requiredSpace=function(i){return this.options.lossy?" ":i},e.optionalSpace=function(i){return this.options.lossy?"":i},e.lossySpace=function(i,n){return this.options.lossy?n?" ":"":i},e.parseParenthesisToken=function(i){var n=this.content(i);return i[O.FIELDS.TYPE]===q.space?this.requiredSpace(n):n},e.newNode=function(i,n){return n&&(/^ +$/.test(n)&&(this.options.lossy||(this.spaces=(this.spaces||"")+n),n=!0),i.namespace=n,ar(i,"namespace")),this.spaces&&(i.spaces.before=this.spaces,this.spaces=""),this.current.append(i)},e.content=function(i){return i===void 0&&(i=this.currToken),this.css.slice(i[O.FIELDS.START_POS],i[O.FIELDS.END_POS])},e.locateNextMeaningfulToken=function(i){i===void 0&&(i=this.position+1);for(var n=i;n<this.tokens.length;)if(zA[this.tokens[n][O.FIELDS.TYPE]]){n++;continue}else return n;return-1},jA(r,[{key:"currToken",get:function(){return this.tokens[this.position]}},{key:"nextToken",get:function(){return this.tokens[this.position+1]}},{key:"prevToken",get:function(){return this.tokens[this.position-1]}}]),r}();ki.default=VA;xd.exports=ki.default});var Ad=x((Si,Sd)=>{u();"use strict";Si.__esModule=!0;Si.default=void 0;var HA=WA(kd());function WA(r){return r&&r.__esModule?r:{default:r}}var GA=function(){function r(t,i){this.func=t||function(){},this.funcRes=null,this.options=i}var e=r.prototype;return e._shouldUpdateSelector=function(i,n){n===void 0&&(n={});var s=Object.assign({},this.options,n);return s.updateSelector===!1?!1:typeof i!="string"},e._isLossy=function(i){i===void 0&&(i={});var n=Object.assign({},this.options,i);return n.lossless===!1},e._root=function(i,n){n===void 0&&(n={});var s=new HA.default(i,this._parseOptions(n));return s.root},e._parseOptions=function(i){return{lossy:this._isLossy(i)}},e._run=function(i,n){var s=this;return n===void 0&&(n={}),new Promise(function(a,o){try{var l=s._root(i,n);Promise.resolve(s.func(l)).then(function(c){var f=void 0;return s._shouldUpdateSelector(i,n)&&(f=l.toString(),i.selector=f),{transform:c,root:l,string:f}}).then(a,o)}catch(c){o(c);return}})},e._runSync=function(i,n){n===void 0&&(n={});var s=this._root(i,n),a=this.func(s);if(a&&typeof a.then=="function")throw new Error("Selector processor returned a promise to a synchronous call.");var o=void 0;return n.updateSelector&&typeof i!="string"&&(o=s.toString(),i.selector=o),{transform:a,root:s,string:o}},e.ast=function(i,n){return this._run(i,n).then(function(s){return s.root})},e.astSync=function(i,n){return this._runSync(i,n).root},e.transform=function(i,n){return this._run(i,n).then(function(s){return s.transform})},e.transformSync=function(i,n){return this._runSync(i,n).transform},e.process=function(i,n){return this._run(i,n).then(function(s){return s.string||s.root.toString()})},e.processSync=function(i,n){var s=this._runSync(i,n);return s.string||s.root.toString()},r}();Si.default=GA;Sd.exports=Si.default});var Cd=x(ne=>{u();"use strict";ne.__esModule=!0;ne.universal=ne.tag=ne.string=ne.selector=ne.root=ne.pseudo=ne.nesting=ne.id=ne.comment=ne.combinator=ne.className=ne.attribute=void 0;var QA=ze(to()),YA=ze(Na()),KA=ze(so()),XA=ze(Fa()),ZA=ze(za()),JA=ze(oo()),eC=ze(Ya()),tC=ze(Da()),rC=ze($a()),iC=ze(Ga()),nC=ze(Ha()),sC=ze(io());function ze(r){return r&&r.__esModule?r:{default:r}}var aC=function(e){return new QA.default(e)};ne.attribute=aC;var oC=function(e){return new YA.default(e)};ne.className=oC;var lC=function(e){return new KA.default(e)};ne.combinator=lC;var uC=function(e){return new XA.default(e)};ne.comment=uC;var fC=function(e){return new ZA.default(e)};ne.id=fC;var cC=function(e){return new JA.default(e)};ne.nesting=cC;var pC=function(e){return new eC.default(e)};ne.pseudo=pC;var dC=function(e){return new tC.default(e)};ne.root=dC;var hC=function(e){return new rC.default(e)};ne.selector=hC;var mC=function(e){return new iC.default(e)};ne.string=mC;var gC=function(e){return new nC.default(e)};ne.tag=gC;var yC=function(e){return new sC.default(e)};ne.universal=yC});var Td=x(Z=>{u();"use strict";Z.__esModule=!0;Z.isComment=Z.isCombinator=Z.isClassName=Z.isAttribute=void 0;Z.isContainer=TC;Z.isIdentifier=void 0;Z.isNamespace=RC;Z.isNesting=void 0;Z.isNode=bo;Z.isPseudo=void 0;Z.isPseudoClass=OC;Z.isPseudoElement=Od;Z.isUniversal=Z.isTag=Z.isString=Z.isSelector=Z.isRoot=void 0;var fe=Se(),Oe,bC=(Oe={},Oe[fe.ATTRIBUTE]=!0,Oe[fe.CLASS]=!0,Oe[fe.COMBINATOR]=!0,Oe[fe.COMMENT]=!0,Oe[fe.ID]=!0,Oe[fe.NESTING]=!0,Oe[fe.PSEUDO]=!0,Oe[fe.ROOT]=!0,Oe[fe.SELECTOR]=!0,Oe[fe.STRING]=!0,Oe[fe.TAG]=!0,Oe[fe.UNIVERSAL]=!0,Oe);function bo(r){return typeof r=="object"&&bC[r.type]}function Ue(r,e){return bo(e)&&e.type===r}var _d=Ue.bind(null,fe.ATTRIBUTE);Z.isAttribute=_d;var wC=Ue.bind(null,fe.CLASS);Z.isClassName=wC;var vC=Ue.bind(null,fe.COMBINATOR);Z.isCombinator=vC;var xC=Ue.bind(null,fe.COMMENT);Z.isComment=xC;var kC=Ue.bind(null,fe.ID);Z.isIdentifier=kC;var SC=Ue.bind(null,fe.NESTING);Z.isNesting=SC;var wo=Ue.bind(null,fe.PSEUDO);Z.isPseudo=wo;var AC=Ue.bind(null,fe.ROOT);Z.isRoot=AC;var CC=Ue.bind(null,fe.SELECTOR);Z.isSelector=CC;var _C=Ue.bind(null,fe.STRING);Z.isString=_C;var Ed=Ue.bind(null,fe.TAG);Z.isTag=Ed;var EC=Ue.bind(null,fe.UNIVERSAL);Z.isUniversal=EC;function Od(r){return wo(r)&&r.value&&(r.value.startsWith("::")||r.value.toLowerCase()===":before"||r.value.toLowerCase()===":after"||r.value.toLowerCase()===":first-letter"||r.value.toLowerCase()===":first-line")}function OC(r){return wo(r)&&!Od(r)}function TC(r){return!!(bo(r)&&r.walk)}function RC(r){return _d(r)||Ed(r)}});var Rd=x(Ke=>{u();"use strict";Ke.__esModule=!0;var vo=Se();Object.keys(vo).forEach(function(r){r==="default"||r==="__esModule"||r in Ke&&Ke[r]===vo[r]||(Ke[r]=vo[r])});var xo=Cd();Object.keys(xo).forEach(function(r){r==="default"||r==="__esModule"||r in Ke&&Ke[r]===xo[r]||(Ke[r]=xo[r])});var ko=Td();Object.keys(ko).forEach(function(r){r==="default"||r==="__esModule"||r in Ke&&Ke[r]===ko[r]||(Ke[r]=ko[r])})});var it=x((Ai,Id)=>{u();"use strict";Ai.__esModule=!0;Ai.default=void 0;var PC=qC(Ad()),IC=DC(Rd());function Pd(r){if(typeof WeakMap!="function")return null;var e=new WeakMap,t=new WeakMap;return(Pd=function(n){return n?t:e})(r)}function DC(r,e){if(!e&&r&&r.__esModule)return r;if(r===null||typeof r!="object"&&typeof r!="function")return{default:r};var t=Pd(e);if(t&&t.has(r))return t.get(r);var i={},n=Object.defineProperty&&Object.getOwnPropertyDescriptor;for(var s in r)if(s!=="default"&&Object.prototype.hasOwnProperty.call(r,s)){var a=n?Object.getOwnPropertyDescriptor(r,s):null;a&&(a.get||a.set)?Object.defineProperty(i,s,a):i[s]=r[s]}return i.default=r,t&&t.set(r,i),i}function qC(r){return r&&r.__esModule?r:{default:r}}var So=function(e){return new PC.default(e)};Object.assign(So,IC);delete So.__esModule;var $C=So;Ai.default=$C;Id.exports=Ai.default});function mt(r){return["fontSize","outline"].includes(r)?e=>(typeof e=="function"&&(e=e({})),Array.isArray(e)&&(e=e[0]),e):r==="fontFamily"?e=>{typeof e=="function"&&(e=e({}));let t=Array.isArray(e)&&ke(e[1])?e[0]:e;return Array.isArray(t)?t.join(", "):t}:["boxShadow","transitionProperty","transitionDuration","transitionDelay","transitionTimingFunction","backgroundImage","backgroundSize","backgroundColor","cursor","animation"].includes(r)?e=>(typeof e=="function"&&(e=e({})),Array.isArray(e)&&(e=e.join(", ")),e):["gridTemplateColumns","gridTemplateRows","objectPosition"].includes(r)?e=>(typeof e=="function"&&(e=e({})),typeof e=="string"&&(e=ee.list.comma(e).join(" ")),e):(e,t={})=>(typeof e=="function"&&(e=e(t)),e)}var Ci=P(()=>{u();Ot();Kt()});var Bd=x((MI,Oo)=>{u();var{AtRule:LC,Rule:Dd}=$e(),qd=it();function Ao(r,e){let t;try{qd(i=>{t=i}).processSync(r)}catch(i){throw r.includes(":")?e?e.error("Missed semicolon"):i:e?e.error(i.message):i}return t.at(0)}function $d(r,e){let t=!1;return r.each(i=>{if(i.type==="nesting"){let n=e.clone({});i.value!=="&"?i.replaceWith(Ao(i.value.replace("&",n.toString()))):i.replaceWith(n),t=!0}else"nodes"in i&&i.nodes&&$d(i,e)&&(t=!0)}),t}function Ld(r,e){let t=[];return r.selectors.forEach(i=>{let n=Ao(i,r);e.selectors.forEach(s=>{if(!s)return;let a=Ao(s,e);$d(a,n)||(a.prepend(qd.combinator({value:" "})),a.prepend(n.clone({}))),t.push(a.toString())})}),t}function Hn(r,e){let t=r.prev();for(e.after(r);t&&t.type==="comment";){let i=t.prev();e.after(t),t=i}return r}function MC(r){return function e(t,i,n,s=n){let a=[];if(i.each(o=>{o.type==="rule"&&n?s&&(o.selectors=Ld(t,o)):o.type==="atrule"&&o.nodes?r[o.name]?e(t,o,s):i[_o]!==!1&&a.push(o):a.push(o)}),n&&a.length){let o=t.clone({nodes:[]});for(let l of a)o.append(l);i.prepend(o)}}}function Co(r,e,t){let i=new Dd({nodes:[],selector:r});return i.append(e),t.after(i),i}function Md(r,e){let t={};for(let i of r)t[i]=!0;if(e)for(let i of e)t[i.replace(/^@/,"")]=!0;return t}function NC(r){r=r.trim();let e=r.match(/^\((.*)\)$/);if(!e)return{selector:r,type:"basic"};let t=e[1].match(/^(with(?:out)?):(.+)$/);if(t){let i=t[1]==="with",n=Object.fromEntries(t[2].trim().split(/\s+/).map(a=>[a,!0]));if(i&&n.all)return{type:"noop"};let s=a=>!!n[a];return n.all?s=()=>!0:i&&(s=a=>a==="all"?!1:!n[a]),{escapes:s,type:"withrules"}}return{type:"unknown"}}function BC(r){let e=[],t=r.parent;for(;t&&t instanceof LC;)e.push(t),t=t.parent;return e}function FC(r){let e=r[Nd];if(!e)r.after(r.nodes);else{let t=r.nodes,i,n=-1,s,a,o,l=BC(r);if(l.forEach((c,f)=>{if(e(c.name))i=c,n=f,a=o;else{let d=o;o=c.clone({nodes:[]}),d&&o.append(d),s=s||o}}),i?a?(s.append(t),i.after(a)):i.after(t):r.after(t),r.next()&&i){let c;l.slice(0,n+1).forEach((f,d,p)=>{let h=c;c=f.clone({nodes:[]}),h&&c.append(h);let b=[],y=(p[d-1]||r).next();for(;y;)b.push(y),y=y.next();c.append(b)}),c&&(a||t[t.length-1]).after(c)}}r.remove()}var _o=Symbol("rootRuleMergeSel"),Nd=Symbol("rootRuleEscapes");function jC(r){let{params:e}=r,{escapes:t,selector:i,type:n}=NC(e);if(n==="unknown")throw r.error(`Unknown @${r.name} parameter ${JSON.stringify(e)}`);if(n==="basic"&&i){let s=new Dd({nodes:r.nodes,selector:i});r.removeAll(),r.append(s)}r[Nd]=t,r[_o]=t?!t("all"):n==="noop"}var Eo=Symbol("hasRootRule");Oo.exports=(r={})=>{let e=Md(["media","supports","layer","container","starting-style"],r.bubble),t=MC(e),i=Md(["document","font-face","keyframes","-webkit-keyframes","-moz-keyframes"],r.unwrap),n=(r.rootRuleName||"at-root").replace(/^@/,""),s=r.preserveEmpty;return{Once(a){a.walkAtRules(n,o=>{jC(o),a[Eo]=!0})},postcssPlugin:"postcss-nested",RootExit(a){a[Eo]&&(a.walkAtRules(n,FC),a[Eo]=!1)},Rule(a){let o=!1,l=a,c=!1,f=[];a.each(d=>{d.type==="rule"?(f.length&&(l=Co(a.selector,f,l),f=[]),c=!0,o=!0,d.selectors=Ld(a,d),l=Hn(d,l)):d.type==="atrule"?(f.length&&(l=Co(a.selector,f,l),f=[]),d.name===n?(o=!0,t(a,d,!0,d[_o]),l=Hn(d,l)):e[d.name]?(c=!0,o=!0,t(a,d,!0),l=Hn(d,l)):i[d.name]?(c=!0,o=!0,t(a,d,!1),l=Hn(d,l)):c&&f.push(d)):d.type==="decl"&&c&&f.push(d)}),f.length&&(l=Co(a.selector,f,l)),o&&s!==!0&&(a.raws.semicolon=!0,a.nodes.length===0&&a.remove())}}};Oo.exports.postcss=!0});var Ud=x((NI,zd)=>{u();"use strict";var Fd=/-(\w|$)/g,jd=(r,e)=>e.toUpperCase(),zC=r=>(r=r.toLowerCase(),r==="float"?"cssFloat":r.startsWith("-ms-")?r.substr(1).replace(Fd,jd):r.replace(Fd,jd));zd.exports=zC});var Po=x((BI,Vd)=>{u();var UC=Ud(),VC={boxFlex:!0,boxFlexGroup:!0,columnCount:!0,flex:!0,flexGrow:!0,flexPositive:!0,flexShrink:!0,flexNegative:!0,fontWeight:!0,lineClamp:!0,lineHeight:!0,opacity:!0,order:!0,orphans:!0,tabSize:!0,widows:!0,zIndex:!0,zoom:!0,fillOpacity:!0,strokeDashoffset:!0,strokeOpacity:!0,strokeWidth:!0};function To(r){return typeof r.nodes=="undefined"?!0:Ro(r)}function Ro(r){let e,t={};return r.each(i=>{if(i.type==="atrule")e="@"+i.name,i.params&&(e+=" "+i.params),typeof t[e]=="undefined"?t[e]=To(i):Array.isArray(t[e])?t[e].push(To(i)):t[e]=[t[e],To(i)];else if(i.type==="rule"){let n=Ro(i);if(t[i.selector])for(let s in n)t[i.selector][s]=n[s];else t[i.selector]=n}else if(i.type==="decl"){i.prop[0]==="-"&&i.prop[1]==="-"||i.parent&&i.parent.selector===":export"?e=i.prop:e=UC(i.prop);let n=i.value;!isNaN(i.value)&&VC[e]&&(n=parseFloat(i.value)),i.important&&(n+=" !important"),typeof t[e]=="undefined"?t[e]=n:Array.isArray(t[e])?t[e].push(n):t[e]=[t[e],n]}}),t}Vd.exports=Ro});var Wn=x((FI,Qd)=>{u();var _i=$e(),Hd=/\s*!important\s*$/i,HC={"box-flex":!0,"box-flex-group":!0,"column-count":!0,flex:!0,"flex-grow":!0,"flex-positive":!0,"flex-shrink":!0,"flex-negative":!0,"font-weight":!0,"line-clamp":!0,"line-height":!0,opacity:!0,order:!0,orphans:!0,"tab-size":!0,widows:!0,"z-index":!0,zoom:!0,"fill-opacity":!0,"stroke-dashoffset":!0,"stroke-opacity":!0,"stroke-width":!0};function WC(r){return r.replace(/([A-Z])/g,"-$1").replace(/^ms-/,"-ms-").toLowerCase()}function Wd(r,e,t){t===!1||t===null||(e.startsWith("--")||(e=WC(e)),typeof t=="number"&&(t===0||HC[e]?t=t.toString():t+="px"),e==="css-float"&&(e="float"),Hd.test(t)?(t=t.replace(Hd,""),r.push(_i.decl({prop:e,value:t,important:!0}))):r.push(_i.decl({prop:e,value:t})))}function Gd(r,e,t){let i=_i.atRule({name:e[1],params:e[3]||""});typeof t=="object"&&(i.nodes=[],Io(t,i)),r.push(i)}function Io(r,e){let t,i,n;for(t in r)if(i=r[t],!(i===null||typeof i=="undefined"))if(t[0]==="@"){let s=t.match(/@(\S+)(\s+([\W\w]*)\s*)?/);if(Array.isArray(i))for(let a of i)Gd(e,s,a);else Gd(e,s,i)}else if(Array.isArray(i))for(let s of i)Wd(e,t,s);else typeof i=="object"?(n=_i.rule({selector:t}),Io(i,n),e.push(n)):Wd(e,t,i)}Qd.exports=function(r){let e=_i.root();return Io(r,e),e}});var Do=x((jI,Yd)=>{u();var GC=Po();Yd.exports=function(e){return console&&console.warn&&e.warnings().forEach(t=>{let i=t.plugin||"PostCSS";console.warn(i+": "+t.text)}),GC(e.root)}});var Xd=x((zI,Kd)=>{u();var QC=$e(),YC=Do(),KC=Wn();Kd.exports=function(e){let t=QC(e);return async i=>{let n=await t.process(i,{parser:KC,from:void 0});return YC(n)}}});var Jd=x((UI,Zd)=>{u();var XC=$e(),ZC=Do(),JC=Wn();Zd.exports=function(r){let e=XC(r);return t=>{let i=e.process(t,{parser:JC,from:void 0});return ZC(i)}}});var th=x((VI,eh)=>{u();var e_=Po(),t_=Wn(),r_=Xd(),i_=Jd();eh.exports={objectify:e_,parse:t_,async:r_,sync:i_}});var or,rh,HI,WI,GI,QI,ih=P(()=>{u();or=pe(th()),rh=or.default,HI=or.default.objectify,WI=or.default.parse,GI=or.default.async,QI=or.default.sync});function lr(r){return Array.isArray(r)?r.flatMap(e=>ee([(0,nh.default)({bubble:["screen"]})]).process(e,{parser:rh}).root.nodes):lr([r])}var nh,qo=P(()=>{u();Ot();nh=pe(Bd());ih()});function ur(r,e,t=!1){if(r==="")return e;let i=typeof e=="string"?(0,sh.default)().astSync(e):e;return i.walkClasses(n=>{let s=n.value,a=t&&s.startsWith("-");n.value=a?`-${r}${s.slice(1)}`:`${r}${s}`}),typeof e=="string"?i.toString():i}var sh,Gn=P(()=>{u();sh=pe(it())});function Te(r){let e=ah.default.className();return e.value=r,jt(e?.raws?.value??e.value)}var ah,fr=P(()=>{u();ah=pe(it());Zi()});function $o(r){return jt(`.${Te(r)}`)}function Qn(r,e){return $o(Ei(r,e))}function Ei(r,e){return e==="DEFAULT"?r:e==="-"||e==="-DEFAULT"?`-${r}`:e.startsWith("-")?`-${r}${e}`:e.startsWith("/")?`${r}${e}`:`${r}-${e}`}var Lo=P(()=>{u();fr();Zi()});function L(r,e=[[r,[r]]],{filterDefault:t=!1,...i}={}){let n=mt(r);return function({matchUtilities:s,theme:a}){for(let o of e){let l=Array.isArray(o[0])?o:[o];s(l.reduce((c,[f,d])=>Object.assign(c,{[f]:p=>d.reduce((h,b)=>Array.isArray(b)?Object.assign(h,{[b[0]]:b[1]}):Object.assign(h,{[b]:n(p)}),{})}),{}),{...i,values:t?Object.fromEntries(Object.entries(a(r)??{}).filter(([c])=>c!=="DEFAULT")):a(r)})}}}var oh=P(()=>{u();Ci()});function Tt(r){return r=Array.isArray(r)?r:[r],r.map(e=>{let t=e.values.map(i=>i.raw!==void 0?i.raw:[i.min&&`(min-width: ${i.min})`,i.max&&`(max-width: ${i.max})`].filter(Boolean).join(" and "));return e.not?`not all and ${t}`:t}).join(", ")}var Yn=P(()=>{u()});function Mo(r){return r.split(f_).map(t=>{let i=t.trim(),n={value:i},s=i.split(c_),a=new Set;for(let o of s)!a.has("DIRECTIONS")&&n_.has(o)?(n.direction=o,a.add("DIRECTIONS")):!a.has("PLAY_STATES")&&s_.has(o)?(n.playState=o,a.add("PLAY_STATES")):!a.has("FILL_MODES")&&a_.has(o)?(n.fillMode=o,a.add("FILL_MODES")):!a.has("ITERATION_COUNTS")&&(o_.has(o)||p_.test(o))?(n.iterationCount=o,a.add("ITERATION_COUNTS")):!a.has("TIMING_FUNCTION")&&l_.has(o)||!a.has("TIMING_FUNCTION")&&u_.some(l=>o.startsWith(`${l}(`))?(n.timingFunction=o,a.add("TIMING_FUNCTION")):!a.has("DURATION")&&lh.test(o)?(n.duration=o,a.add("DURATION")):!a.has("DELAY")&&lh.test(o)?(n.delay=o,a.add("DELAY")):a.has("NAME")?(n.unknown||(n.unknown=[]),n.unknown.push(o)):(n.name=o,a.add("NAME"));return n})}var n_,s_,a_,o_,l_,u_,f_,c_,lh,p_,uh=P(()=>{u();n_=new Set(["normal","reverse","alternate","alternate-reverse"]),s_=new Set(["running","paused"]),a_=new Set(["none","forwards","backwards","both"]),o_=new Set(["infinite"]),l_=new Set(["linear","ease","ease-in","ease-out","ease-in-out","step-start","step-end"]),u_=["cubic-bezier","steps"],f_=/\,(?![^(]*\))/g,c_=/\ +(?![^(]*\))/g,lh=/^(-?[\d.]+m?s)$/,p_=/^(\d+)$/});var fh,xe,ch=P(()=>{u();fh=r=>Object.assign({},...Object.entries(r??{}).flatMap(([e,t])=>typeof t=="object"?Object.entries(fh(t)).map(([i,n])=>({[e+(i==="DEFAULT"?"":`-${i}`)]:n})):[{[`${e}`]:t}])),xe=fh});var dh,ph=P(()=>{dh="3.4.17"});function Rt(r,e=!0){return Array.isArray(r)?r.map(t=>{if(e&&Array.isArray(t))throw new Error("The tuple syntax is not supported for `screens`.");if(typeof t=="string")return{name:t.toString(),not:!1,values:[{min:t,max:void 0}]};let[i,n]=t;return i=i.toString(),typeof n=="string"?{name:i,not:!1,values:[{min:n,max:void 0}]}:Array.isArray(n)?{name:i,not:!1,values:n.map(s=>mh(s))}:{name:i,not:!1,values:[mh(n)]}}):Rt(Object.entries(r??{}),!1)}function Kn(r){return r.values.length!==1?{result:!1,reason:"multiple-values"}:r.values[0].raw!==void 0?{result:!1,reason:"raw-values"}:r.values[0].min!==void 0&&r.values[0].max!==void 0?{result:!1,reason:"min-and-max"}:{result:!0,reason:null}}function hh(r,e,t){let i=Xn(e,r),n=Xn(t,r),s=Kn(i),a=Kn(n);if(s.reason==="multiple-values"||a.reason==="multiple-values")throw new Error("Attempted to sort a screen with multiple values. This should never happen. Please open a bug report.");if(s.reason==="raw-values"||a.reason==="raw-values")throw new Error("Attempted to sort a screen with raw values. This should never happen. Please open a bug report.");if(s.reason==="min-and-max"||a.reason==="min-and-max")throw new Error("Attempted to sort a screen with both min and max values. This should never happen. Please open a bug report.");let{min:o,max:l}=i.values[0],{min:c,max:f}=n.values[0];e.not&&([o,l]=[l,o]),t.not&&([c,f]=[f,c]),o=o===void 0?o:parseFloat(o),l=l===void 0?l:parseFloat(l),c=c===void 0?c:parseFloat(c),f=f===void 0?f:parseFloat(f);let[d,p]=r==="min"?[o,c]:[f,l];return d-p}function Xn(r,e){return typeof r=="object"?r:{name:"arbitrary-screen",values:[{[e]:r}]}}function mh({"min-width":r,min:e=r,max:t,raw:i}={}){return{min:e,max:t,raw:i}}var Zn=P(()=>{u()});function Jn(r,e){r.walkDecls(t=>{if(e.includes(t.prop)){t.remove();return}for(let i of e)t.value.includes(`/ var(${i})`)?t.value=t.value.replace(`/ var(${i})`,""):t.value.includes(`/ var(${i}, 1)`)&&(t.value=t.value.replace(`/ var(${i}, 1)`,""))})}var gh=P(()=>{u()});var se,Xe,nt,ge,yh,bh=P(()=>{u();ft();et();Ot();oh();Yn();fr();uh();ch();Lr();ra();Kt();Ci();ph();Be();Zn();Ys();gh();ct();Br();Oi();se={childVariant:({addVariant:r})=>{r("*","& > *")},pseudoElementVariants:({addVariant:r})=>{r("first-letter","&::first-letter"),r("first-line","&::first-line"),r("marker",[({container:e})=>(Jn(e,["--tw-text-opacity"]),"& *::marker"),({container:e})=>(Jn(e,["--tw-text-opacity"]),"&::marker")]),r("selection",["& *::selection","&::selection"]),r("file","&::file-selector-button"),r("placeholder","&::placeholder"),r("backdrop","&::backdrop"),r("before",({container:e})=>(e.walkRules(t=>{let i=!1;t.walkDecls("content",()=>{i=!0}),i||t.prepend(ee.decl({prop:"content",value:"var(--tw-content)"}))}),"&::before")),r("after",({container:e})=>(e.walkRules(t=>{let i=!1;t.walkDecls("content",()=>{i=!0}),i||t.prepend(ee.decl({prop:"content",value:"var(--tw-content)"}))}),"&::after"))},pseudoClassVariants:({addVariant:r,matchVariant:e,config:t,prefix:i})=>{let n=[["first","&:first-child"],["last","&:last-child"],["only","&:only-child"],["odd","&:nth-child(odd)"],["even","&:nth-child(even)"],"first-of-type","last-of-type","only-of-type",["visited",({container:a})=>(Jn(a,["--tw-text-opacity","--tw-border-opacity","--tw-bg-opacity"]),"&:visited")],"target",["open","&[open]"],"default","checked","indeterminate","placeholder-shown","autofill","optional","required","valid","invalid","in-range","out-of-range","read-only","empty","focus-within",["hover",we(t(),"hoverOnlyWhenSupported")?"@media (hover: hover) and (pointer: fine) { &:hover }":"&:hover"],"focus","focus-visible","active","enabled","disabled"].map(a=>Array.isArray(a)?a:[a,`&:${a}`]);for(let[a,o]of n)r(a,l=>typeof o=="function"?o(l):o);let s={group:(a,{modifier:o})=>o?[`:merge(${i(".group")}\\/${Te(o)})`," &"]:[`:merge(${i(".group")})`," &"],peer:(a,{modifier:o})=>o?[`:merge(${i(".peer")}\\/${Te(o)})`," ~ &"]:[`:merge(${i(".peer")})`," ~ &"]};for(let[a,o]of Object.entries(s))e(a,(l="",c)=>{let f=K(typeof l=="function"?l(c):l);f.includes("&")||(f="&"+f);let[d,p]=o("",c),h=null,b=null,v=0;for(let y=0;y<f.length;++y){let w=f[y];w==="&"?h=y:w==="'"||w==='"'?v+=1:h!==null&&w===" "&&!v&&(b=y)}return h!==null&&b===null&&(b=f.length),f.slice(0,h)+d+f.slice(h+1,b)+p+f.slice(b)},{values:Object.fromEntries(n),[Pt]:{respectPrefix:!1}})},directionVariants:({addVariant:r})=>{r("ltr",'&:where([dir="ltr"], [dir="ltr"] *)'),r("rtl",'&:where([dir="rtl"], [dir="rtl"] *)')},reducedMotionVariants:({addVariant:r})=>{r("motion-safe","@media (prefers-reduced-motion: no-preference)"),r("motion-reduce","@media (prefers-reduced-motion: reduce)")},darkVariants:({config:r,addVariant:e})=>{let[t,i=".dark"]=[].concat(r("darkMode","media"));if(t===!1&&(t="media",G.warn("darkmode-false",["The `darkMode` option in your Tailwind CSS configuration is set to `false`, which now behaves the same as `media`.","Change `darkMode` to `media` or remove it entirely.","https://tailwindcss.com/docs/upgrade-guide#remove-dark-mode-configuration"])),t==="variant"){let n;if(Array.isArray(i)||typeof i=="function"?n=i:typeof i=="string"&&(n=[i]),Array.isArray(n))for(let s of n)s===".dark"?(t=!1,G.warn("darkmode-variant-without-selector",["When using `variant` for `darkMode`, you must provide a selector.",'Example: `darkMode: ["variant", ".your-selector &"]`'])):s.includes("&")||(t=!1,G.warn("darkmode-variant-without-ampersand",["When using `variant` for `darkMode`, your selector must contain `&`.",'Example `darkMode: ["variant", ".your-selector &"]`']));i=n}t==="selector"?e("dark",`&:where(${i}, ${i} *)`):t==="media"?e("dark","@media (prefers-color-scheme: dark)"):t==="variant"?e("dark",i):t==="class"&&e("dark",`&:is(${i} *)`)},printVariant:({addVariant:r})=>{r("print","@media print")},screenVariants:({theme:r,addVariant:e,matchVariant:t})=>{let i=r("screens")??{},n=Object.values(i).every(w=>typeof w=="string"),s=Rt(r("screens")),a=new Set([]);function o(w){return w.match(/(\D+)$/)?.[1]??"(none)"}function l(w){w!==void 0&&a.add(o(w))}function c(w){return l(w),a.size===1}for(let w of s)for(let k of w.values)l(k.min),l(k.max);let f=a.size<=1;function d(w){return Object.fromEntries(s.filter(k=>Kn(k).result).map(k=>{let{min:S,max:E}=k.values[0];if(w==="min"&&S!==void 0)return k;if(w==="min"&&E!==void 0)return{...k,not:!k.not};if(w==="max"&&E!==void 0)return k;if(w==="max"&&S!==void 0)return{...k,not:!k.not}}).map(k=>[k.name,k]))}function p(w){return(k,S)=>hh(w,k.value,S.value)}let h=p("max"),b=p("min");function v(w){return k=>{if(n)if(f){if(typeof k=="string"&&!c(k))return G.warn("minmax-have-mixed-units",["The `min-*` and `max-*` variants are not supported with a `screens` configuration containing mixed units."]),[]}else return G.warn("mixed-screen-units",["The `min-*` and `max-*` variants are not supported with a `screens` configuration containing mixed units."]),[];else return G.warn("complex-screen-config",["The `min-*` and `max-*` variants are not supported with a `screens` configuration containing objects."]),[];return[`@media ${Tt(Xn(k,w))}`]}}t("max",v("max"),{sort:h,values:n?d("max"):{}});let y="min-screens";for(let w of s)e(w.name,`@media ${Tt(w)}`,{id:y,sort:n&&f?b:void 0,value:w});t("min",v("min"),{id:y,sort:b})},supportsVariants:({matchVariant:r,theme:e})=>{r("supports",(t="")=>{let i=K(t),n=/^\w*\s*\(/.test(i);return i=n?i.replace(/\b(and|or|not)\b/g," $1 "):i,n?`@supports ${i}`:(i.includes(":")||(i=`${i}: var(--tw)`),i.startsWith("(")&&i.endsWith(")")||(i=`(${i})`),`@supports ${i}`)},{values:e("supports")??{}})},hasVariants:({matchVariant:r,prefix:e})=>{r("has",t=>`&:has(${K(t)})`,{values:{},[Pt]:{respectPrefix:!1}}),r("group-has",(t,{modifier:i})=>i?`:merge(${e(".group")}\\/${i}):has(${K(t)}) &`:`:merge(${e(".group")}):has(${K(t)}) &`,{values:{},[Pt]:{respectPrefix:!1}}),r("peer-has",(t,{modifier:i})=>i?`:merge(${e(".peer")}\\/${i}):has(${K(t)}) ~ &`:`:merge(${e(".peer")}):has(${K(t)}) ~ &`,{values:{},[Pt]:{respectPrefix:!1}})},ariaVariants:({matchVariant:r,theme:e})=>{r("aria",t=>`&[aria-${Ye(K(t))}]`,{values:e("aria")??{}}),r("group-aria",(t,{modifier:i})=>i?`:merge(.group\\/${i})[aria-${Ye(K(t))}] &`:`:merge(.group)[aria-${Ye(K(t))}] &`,{values:e("aria")??{}}),r("peer-aria",(t,{modifier:i})=>i?`:merge(.peer\\/${i})[aria-${Ye(K(t))}] ~ &`:`:merge(.peer)[aria-${Ye(K(t))}] ~ &`,{values:e("aria")??{}})},dataVariants:({matchVariant:r,theme:e})=>{r("data",t=>`&[data-${Ye(K(t))}]`,{values:e("data")??{}}),r("group-data",(t,{modifier:i})=>i?`:merge(.group\\/${i})[data-${Ye(K(t))}] &`:`:merge(.group)[data-${Ye(K(t))}] &`,{values:e("data")??{}}),r("peer-data",(t,{modifier:i})=>i?`:merge(.peer\\/${i})[data-${Ye(K(t))}] ~ &`:`:merge(.peer)[data-${Ye(K(t))}] ~ &`,{values:e("data")??{}})},orientationVariants:({addVariant:r})=>{r("portrait","@media (orientation: portrait)"),r("landscape","@media (orientation: landscape)")},prefersContrastVariants:({addVariant:r})=>{r("contrast-more","@media (prefers-contrast: more)"),r("contrast-less","@media (prefers-contrast: less)")},forcedColorsVariants:({addVariant:r})=>{r("forced-colors","@media (forced-colors: active)")}},Xe=["translate(var(--tw-translate-x), var(--tw-translate-y))","rotate(var(--tw-rotate))","skewX(var(--tw-skew-x))","skewY(var(--tw-skew-y))","scaleX(var(--tw-scale-x))","scaleY(var(--tw-scale-y))"].join(" "),nt=["var(--tw-blur)","var(--tw-brightness)","var(--tw-contrast)","var(--tw-grayscale)","var(--tw-hue-rotate)","var(--tw-invert)","var(--tw-saturate)","var(--tw-sepia)","var(--tw-drop-shadow)"].join(" "),ge=["var(--tw-backdrop-blur)","var(--tw-backdrop-brightness)","var(--tw-backdrop-contrast)","var(--tw-backdrop-grayscale)","var(--tw-backdrop-hue-rotate)","var(--tw-backdrop-invert)","var(--tw-backdrop-opacity)","var(--tw-backdrop-saturate)","var(--tw-backdrop-sepia)"].join(" "),yh={preflight:({addBase:r})=>{let e=ee.parse(`*,::after,::before{box-sizing:border-box;border-width:0;border-style:solid;border-color:theme('borderColor.DEFAULT', currentColor)}::after,::before{--tw-content:''}:host,html{line-height:1.5;-webkit-text-size-adjust:100%;-moz-tab-size:4;tab-size:4;font-family:theme('fontFamily.sans', ui-sans-serif, system-ui, sans-serif, "Apple Color Emoji", "Segoe UI Emoji", "Segoe UI Symbol", "Noto Color Emoji");font-feature-settings:theme('fontFamily.sans[1].fontFeatureSettings', normal);font-variation-settings:theme('fontFamily.sans[1].fontVariationSettings', normal);-webkit-tap-highlight-color:transparent}body{margin:0;line-height:inherit}hr{height:0;color:inherit;border-top-width:1px}abbr:where([title]){text-decoration:underline dotted}h1,h2,h3,h4,h5,h6{font-size:inherit;font-weight:inherit}a{color:inherit;text-decoration:inherit}b,strong{font-weight:bolder}code,kbd,pre,samp{font-family:theme('fontFamily.mono', ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace);font-feature-settings:theme('fontFamily.mono[1].fontFeatureSettings', normal);font-variation-settings:theme('fontFamily.mono[1].fontVariationSettings', normal);font-size:1em}small{font-size:80%}sub,sup{font-size:75%;line-height:0;position:relative;vertical-align:baseline}sub{bottom:-.25em}sup{top:-.5em}table{text-indent:0;border-color:inherit;border-collapse:collapse}button,input,optgroup,select,textarea{font-family:inherit;font-feature-settings:inherit;font-variation-settings:inherit;font-size:100%;font-weight:inherit;line-height:inherit;letter-spacing:inherit;color:inherit;margin:0;padding:0}button,select{text-transform:none}button,input:where([type=button]),input:where([type=reset]),input:where([type=submit]){-webkit-appearance:button;background-color:transparent;background-image:none}:-moz-focusring{outline:auto}:-moz-ui-invalid{box-shadow:none}progress{vertical-align:baseline}::-webkit-inner-spin-button,::-webkit-outer-spin-button{height:auto}[type=search]{-webkit-appearance:textfield;outline-offset:-2px}::-webkit-search-decoration{-webkit-appearance:none}::-webkit-file-upload-button{-webkit-appearance:button;font:inherit}summary{display:list-item}blockquote,dd,dl,figure,h1,h2,h3,h4,h5,h6,hr,p,pre{margin:0}fieldset{margin:0;padding:0}legend{padding:0}menu,ol,ul{list-style:none;margin:0;padding:0}dialog{padding:0}textarea{resize:vertical}input::placeholder,textarea::placeholder{opacity:1;color:theme('colors.gray.4', #9ca3af)}[role=button],button{cursor:pointer}:disabled{cursor:default}audio,canvas,embed,iframe,img,object,svg,video{display:block;vertical-align:middle}img,video{max-width:100%;height:auto}[hidden]:where(:not([hidden=until-found])){display:none}`);r([ee.comment({text:`! tailwindcss v${dh} | MIT License | https://tailwindcss.com`}),...e.nodes])},container:(()=>{function r(t=[]){return t.flatMap(i=>i.values.map(n=>n.min)).filter(i=>i!==void 0)}function e(t,i,n){if(typeof n=="undefined")return[];if(!(typeof n=="object"&&n!==null))return[{screen:"DEFAULT",minWidth:0,padding:n}];let s=[];n.DEFAULT&&s.push({screen:"DEFAULT",minWidth:0,padding:n.DEFAULT});for(let a of t)for(let o of i)for(let{min:l}of o.values)l===a&&s.push({minWidth:a,padding:n[o.name]});return s}return function({addComponents:t,theme:i}){let n=Rt(i("container.screens",i("screens"))),s=r(n),a=e(s,n,i("container.padding")),o=c=>{let f=a.find(d=>d.minWidth===c);return f?{paddingRight:f.padding,paddingLeft:f.padding}:{}},l=Array.from(new Set(s.slice().sort((c,f)=>parseInt(c)-parseInt(f)))).map(c=>({[`@media (min-width: ${c})`]:{".container":{"max-width":c,...o(c)}}}));t([{".container":Object.assign({width:"100%"},i("container.center",!1)?{marginRight:"auto",marginLeft:"auto"}:{},o(0))},...l])}})(),accessibility:({addUtilities:r})=>{r({".sr-only":{position:"absolute",width:"1px",height:"1px",padding:"0",margin:"-1px",overflow:"hidden",clip:"rect(0, 0, 0, 0)",whiteSpace:"nowrap",borderWidth:"0"},".not-sr-only":{position:"static",width:"auto",height:"auto",padding:"0",margin:"0",overflow:"visible",clip:"auto",whiteSpace:"normal"}})},pointerEvents:({addUtilities:r})=>{r({".pointer-events-none":{"pointer-events":"none"},".pointer-events-auto":{"pointer-events":"auto"}})},visibility:({addUtilities:r})=>{r({".visible":{visibility:"visible"},".invisible":{visibility:"hidden"},".collapse":{visibility:"collapse"}})},position:({addUtilities:r})=>{r({".static":{position:"static"},".fixed":{position:"fixed"},".absolute":{position:"absolute"},".relative":{position:"relative"},".sticky":{position:"sticky"}})},inset:L("inset",[["inset",["inset"]],[["inset-x",["left","right"]],["inset-y",["top","bottom"]]],[["start",["inset-inline-start"]],["end",["inset-inline-end"]],["top",["top"]],["right",["right"]],["bottom",["bottom"]],["left",["left"]]]],{supportsNegativeValues:!0}),isolation:({addUtilities:r})=>{r({".isolate":{isolation:"isolate"},".isolation-auto":{isolation:"auto"}})},zIndex:L("zIndex",[["z",["zIndex"]]],{supportsNegativeValues:!0}),order:L("order",void 0,{supportsNegativeValues:!0}),gridColumn:L("gridColumn",[["col",["gridColumn"]]]),gridColumnStart:L("gridColumnStart",[["col-start",["gridColumnStart"]]],{supportsNegativeValues:!0}),gridColumnEnd:L("gridColumnEnd",[["col-end",["gridColumnEnd"]]],{supportsNegativeValues:!0}),gridRow:L("gridRow",[["row",["gridRow"]]]),gridRowStart:L("gridRowStart",[["row-start",["gridRowStart"]]],{supportsNegativeValues:!0}),gridRowEnd:L("gridRowEnd",[["row-end",["gridRowEnd"]]],{supportsNegativeValues:!0}),float:({addUtilities:r})=>{r({".float-start":{float:"inline-start"},".float-end":{float:"inline-end"},".float-right":{float:"right"},".float-left":{float:"left"},".float-none":{float:"none"}})},clear:({addUtilities:r})=>{r({".clear-start":{clear:"inline-start"},".clear-end":{clear:"inline-end"},".clear-left":{clear:"left"},".clear-right":{clear:"right"},".clear-both":{clear:"both"},".clear-none":{clear:"none"}})},margin:L("margin",[["m",["margin"]],[["mx",["margin-left","margin-right"]],["my",["margin-top","margin-bottom"]]],[["ms",["margin-inline-start"]],["me",["margin-inline-end"]],["mt",["margin-top"]],["mr",["margin-right"]],["mb",["margin-bottom"]],["ml",["margin-left"]]]],{supportsNegativeValues:!0}),boxSizing:({addUtilities:r})=>{r({".box-border":{"box-sizing":"border-box"},".box-content":{"box-sizing":"content-box"}})},lineClamp:({matchUtilities:r,addUtilities:e,theme:t})=>{r({"line-clamp":i=>({overflow:"hidden",display:"-webkit-box","-webkit-box-orient":"vertical","-webkit-line-clamp":`${i}`})},{values:t("lineClamp")}),e({".line-clamp-none":{overflow:"visible",display:"block","-webkit-box-orient":"horizontal","-webkit-line-clamp":"none"}})},display:({addUtilities:r})=>{r({".block":{display:"block"},".inline-block":{display:"inline-block"},".inline":{display:"inline"},".flex":{display:"flex"},".inline-flex":{display:"inline-flex"},".table":{display:"table"},".inline-table":{display:"inline-table"},".table-caption":{display:"table-caption"},".table-cell":{display:"table-cell"},".table-column":{display:"table-column"},".table-column-group":{display:"table-column-group"},".table-footer-group":{display:"table-footer-group"},".table-header-group":{display:"table-header-group"},".table-row-group":{display:"table-row-group"},".table-row":{display:"table-row"},".flow-root":{display:"flow-root"},".grid":{display:"grid"},".inline-grid":{display:"inline-grid"},".contents":{display:"contents"},".list-item":{display:"list-item"},".hidden":{display:"none"}})},aspectRatio:L("aspectRatio",[["aspect",["aspect-ratio"]]]),size:L("size",[["size",["width","height"]]]),height:L("height",[["h",["height"]]]),maxHeight:L("maxHeight",[["max-h",["maxHeight"]]]),minHeight:L("minHeight",[["min-h",["minHeight"]]]),width:L("width",[["w",["width"]]]),minWidth:L("minWidth",[["min-w",["minWidth"]]]),maxWidth:L("maxWidth",[["max-w",["maxWidth"]]]),flex:L("flex"),flexShrink:L("flexShrink",[["flex-shrink",["flex-shrink"]],["shrink",["flex-shrink"]]]),flexGrow:L("flexGrow",[["flex-grow",["flex-grow"]],["grow",["flex-grow"]]]),flexBasis:L("flexBasis",[["basis",["flex-basis"]]]),tableLayout:({addUtilities:r})=>{r({".table-auto":{"table-layout":"auto"},".table-fixed":{"table-layout":"fixed"}})},captionSide:({addUtilities:r})=>{r({".caption-top":{"caption-side":"top"},".caption-bottom":{"caption-side":"bottom"}})},borderCollapse:({addUtilities:r})=>{r({".border-collapse":{"border-collapse":"collapse"},".border-separate":{"border-collapse":"separate"}})},borderSpacing:({addDefaults:r,matchUtilities:e,theme:t})=>{r("border-spacing",{"--tw-border-spacing-x":0,"--tw-border-spacing-y":0}),e({"border-spacing":i=>({"--tw-border-spacing-x":i,"--tw-border-spacing-y":i,"@defaults border-spacing":{},"border-spacing":"var(--tw-border-spacing-x) var(--tw-border-spacing-y)"}),"border-spacing-x":i=>({"--tw-border-spacing-x":i,"@defaults border-spacing":{},"border-spacing":"var(--tw-border-spacing-x) var(--tw-border-spacing-y)"}),"border-spacing-y":i=>({"--tw-border-spacing-y":i,"@defaults border-spacing":{},"border-spacing":"var(--tw-border-spacing-x) var(--tw-border-spacing-y)"})},{values:t("borderSpacing")})},transformOrigin:L("transformOrigin",[["origin",["transformOrigin"]]]),translate:L("translate",[[["translate-x",[["@defaults transform",{}],"--tw-translate-x",["transform",Xe]]],["translate-y",[["@defaults transform",{}],"--tw-translate-y",["transform",Xe]]]]],{supportsNegativeValues:!0}),rotate:L("rotate",[["rotate",[["@defaults transform",{}],"--tw-rotate",["transform",Xe]]]],{supportsNegativeValues:!0}),skew:L("skew",[[["skew-x",[["@defaults transform",{}],"--tw-skew-x",["transform",Xe]]],["skew-y",[["@defaults transform",{}],"--tw-skew-y",["transform",Xe]]]]],{supportsNegativeValues:!0}),scale:L("scale",[["scale",[["@defaults transform",{}],"--tw-scale-x","--tw-scale-y",["transform",Xe]]],[["scale-x",[["@defaults transform",{}],"--tw-scale-x",["transform",Xe]]],["scale-y",[["@defaults transform",{}],"--tw-scale-y",["transform",Xe]]]]],{supportsNegativeValues:!0}),transform:({addDefaults:r,addUtilities:e})=>{r("transform",{"--tw-translate-x":"0","--tw-translate-y":"0","--tw-rotate":"0","--tw-skew-x":"0","--tw-skew-y":"0","--tw-scale-x":"1","--tw-scale-y":"1"}),e({".transform":{"@defaults transform":{},transform:Xe},".transform-cpu":{transform:Xe},".transform-gpu":{transform:Xe.replace("translate(var(--tw-translate-x), var(--tw-translate-y))","translate3d(var(--tw-translate-x), var(--tw-translate-y), 0)")},".transform-none":{transform:"none"}})},animation:({matchUtilities:r,theme:e,config:t})=>{let i=s=>Te(t("prefix")+s),n=Object.fromEntries(Object.entries(e("keyframes")??{}).map(([s,a])=>[s,{[`@keyframes ${i(s)}`]:a}]));r({animate:s=>{let a=Mo(s);return[...a.flatMap(o=>n[o.name]),{animation:a.map(({name:o,value:l})=>o===void 0||n[o]===void 0?l:l.replace(o,i(o))).join(", ")}]}},{values:e("animation")})},cursor:L("cursor"),touchAction:({addDefaults:r,addUtilities:e})=>{r("touch-action",{"--tw-pan-x":" ","--tw-pan-y":" ","--tw-pinch-zoom":" "});let t="var(--tw-pan-x) var(--tw-pan-y) var(--tw-pinch-zoom)";e({".touch-auto":{"touch-action":"auto"},".touch-none":{"touch-action":"none"},".touch-pan-x":{"@defaults touch-action":{},"--tw-pan-x":"pan-x","touch-action":t},".touch-pan-left":{"@defaults touch-action":{},"--tw-pan-x":"pan-left","touch-action":t},".touch-pan-right":{"@defaults touch-action":{},"--tw-pan-x":"pan-right","touch-action":t},".touch-pan-y":{"@defaults touch-action":{},"--tw-pan-y":"pan-y","touch-action":t},".touch-pan-up":{"@defaults touch-action":{},"--tw-pan-y":"pan-up","touch-action":t},".touch-pan-down":{"@defaults touch-action":{},"--tw-pan-y":"pan-down","touch-action":t},".touch-pinch-zoom":{"@defaults touch-action":{},"--tw-pinch-zoom":"pinch-zoom","touch-action":t},".touch-manipulation":{"touch-action":"manipulation"}})},userSelect:({addUtilities:r})=>{r({".select-none":{"user-select":"none"},".select-text":{"user-select":"text"},".select-all":{"user-select":"all"},".select-auto":{"user-select":"auto"}})},resize:({addUtilities:r})=>{r({".resize-none":{resize:"none"},".resize-y":{resize:"vertical"},".resize-x":{resize:"horizontal"},".resize":{resize:"both"}})},scrollSnapType:({addDefaults:r,addUtilities:e})=>{r("scroll-snap-type",{"--tw-scroll-snap-strictness":"proximity"}),e({".snap-none":{"scroll-snap-type":"none"},".snap-x":{"@defaults scroll-snap-type":{},"scroll-snap-type":"x var(--tw-scroll-snap-strictness)"},".snap-y":{"@defaults scroll-snap-type":{},"scroll-snap-type":"y var(--tw-scroll-snap-strictness)"},".snap-both":{"@defaults scroll-snap-type":{},"scroll-snap-type":"both var(--tw-scroll-snap-strictness)"},".snap-mandatory":{"--tw-scroll-snap-strictness":"mandatory"},".snap-proximity":{"--tw-scroll-snap-strictness":"proximity"}})},scrollSnapAlign:({addUtilities:r})=>{r({".snap-start":{"scroll-snap-align":"start"},".snap-end":{"scroll-snap-align":"end"},".snap-center":{"scroll-snap-align":"center"},".snap-align-none":{"scroll-snap-align":"none"}})},scrollSnapStop:({addUtilities:r})=>{r({".snap-normal":{"scroll-snap-stop":"normal"},".snap-always":{"scroll-snap-stop":"always"}})},scrollMargin:L("scrollMargin",[["scroll-m",["scroll-margin"]],[["scroll-mx",["scroll-margin-left","scroll-margin-right"]],["scroll-my",["scroll-margin-top","scroll-margin-bottom"]]],[["scroll-ms",["scroll-margin-inline-start"]],["scroll-me",["scroll-margin-inline-end"]],["scroll-mt",["scroll-margin-top"]],["scroll-mr",["scroll-margin-right"]],["scroll-mb",["scroll-margin-bottom"]],["scroll-ml",["scroll-margin-left"]]]],{supportsNegativeValues:!0}),scrollPadding:L("scrollPadding",[["scroll-p",["scroll-padding"]],[["scroll-px",["scroll-padding-left","scroll-padding-right"]],["scroll-py",["scroll-padding-top","scroll-padding-bottom"]]],[["scroll-ps",["scroll-padding-inline-start"]],["scroll-pe",["scroll-padding-inline-end"]],["scroll-pt",["scroll-padding-top"]],["scroll-pr",["scroll-padding-right"]],["scroll-pb",["scroll-padding-bottom"]],["scroll-pl",["scroll-padding-left"]]]]),listStylePosition:({addUtilities:r})=>{r({".list-inside":{"list-style-position":"inside"},".list-outside":{"list-style-position":"outside"}})},listStyleType:L("listStyleType",[["list",["listStyleType"]]]),listStyleImage:L("listStyleImage",[["list-image",["listStyleImage"]]]),appearance:({addUtilities:r})=>{r({".appearance-none":{appearance:"none"},".appearance-auto":{appearance:"auto"}})},columns:L("columns",[["columns",["columns"]]]),breakBefore:({addUtilities:r})=>{r({".break-before-auto":{"break-before":"auto"},".break-before-avoid":{"break-before":"avoid"},".break-before-all":{"break-before":"all"},".break-before-avoid-page":{"break-before":"avoid-page"},".break-before-page":{"break-before":"page"},".break-before-left":{"break-before":"left"},".break-before-right":{"break-before":"right"},".break-before-column":{"break-before":"column"}})},breakInside:({addUtilities:r})=>{r({".break-inside-auto":{"break-inside":"auto"},".break-inside-avoid":{"break-inside":"avoid"},".break-inside-avoid-page":{"break-inside":"avoid-page"},".break-inside-avoid-column":{"break-inside":"avoid-column"}})},breakAfter:({addUtilities:r})=>{r({".break-after-auto":{"break-after":"auto"},".break-after-avoid":{"break-after":"avoid"},".break-after-all":{"break-after":"all"},".break-after-avoid-page":{"break-after":"avoid-page"},".break-after-page":{"break-after":"page"},".break-after-left":{"break-after":"left"},".break-after-right":{"break-after":"right"},".break-after-column":{"break-after":"column"}})},gridAutoColumns:L("gridAutoColumns",[["auto-cols",["gridAutoColumns"]]]),gridAutoFlow:({addUtilities:r})=>{r({".grid-flow-row":{gridAutoFlow:"row"},".grid-flow-col":{gridAutoFlow:"column"},".grid-flow-dense":{gridAutoFlow:"dense"},".grid-flow-row-dense":{gridAutoFlow:"row dense"},".grid-flow-col-dense":{gridAutoFlow:"column dense"}})},gridAutoRows:L("gridAutoRows",[["auto-rows",["gridAutoRows"]]]),gridTemplateColumns:L("gridTemplateColumns",[["grid-cols",["gridTemplateColumns"]]]),gridTemplateRows:L("gridTemplateRows",[["grid-rows",["gridTemplateRows"]]]),flexDirection:({addUtilities:r})=>{r({".flex-row":{"flex-direction":"row"},".flex-row-reverse":{"flex-direction":"row-reverse"},".flex-col":{"flex-direction":"column"},".flex-col-reverse":{"flex-direction":"column-reverse"}})},flexWrap:({addUtilities:r})=>{r({".flex-wrap":{"flex-wrap":"wrap"},".flex-wrap-reverse":{"flex-wrap":"wrap-reverse"},".flex-nowrap":{"flex-wrap":"nowrap"}})},placeContent:({addUtilities:r})=>{r({".place-content-center":{"place-content":"center"},".place-content-start":{"place-content":"start"},".place-content-end":{"place-content":"end"},".place-content-between":{"place-content":"space-between"},".place-content-around":{"place-content":"space-around"},".place-content-evenly":{"place-content":"space-evenly"},".place-content-baseline":{"place-content":"baseline"},".place-content-stretch":{"place-content":"stretch"}})},placeItems:({addUtilities:r})=>{r({".place-items-start":{"place-items":"start"},".place-items-end":{"place-items":"end"},".place-items-center":{"place-items":"center"},".place-items-baseline":{"place-items":"baseline"},".place-items-stretch":{"place-items":"stretch"}})},alignContent:({addUtilities:r})=>{r({".content-normal":{"align-content":"normal"},".content-center":{"align-content":"center"},".content-start":{"align-content":"flex-start"},".content-end":{"align-content":"flex-end"},".content-between":{"align-content":"space-between"},".content-around":{"align-content":"space-around"},".content-evenly":{"align-content":"space-evenly"},".content-baseline":{"align-content":"baseline"},".content-stretch":{"align-content":"stretch"}})},alignItems:({addUtilities:r})=>{r({".items-start":{"align-items":"flex-start"},".items-end":{"align-items":"flex-end"},".items-center":{"align-items":"center"},".items-baseline":{"align-items":"baseline"},".items-stretch":{"align-items":"stretch"}})},justifyContent:({addUtilities:r})=>{r({".justify-normal":{"justify-content":"normal"},".justify-start":{"justify-content":"flex-start"},".justify-end":{"justify-content":"flex-end"},".justify-center":{"justify-content":"center"},".justify-between":{"justify-content":"space-between"},".justify-around":{"justify-content":"space-around"},".justify-evenly":{"justify-content":"space-evenly"},".justify-stretch":{"justify-content":"stretch"}})},justifyItems:({addUtilities:r})=>{r({".justify-items-start":{"justify-items":"start"},".justify-items-end":{"justify-items":"end"},".justify-items-center":{"justify-items":"center"},".justify-items-stretch":{"justify-items":"stretch"}})},gap:L("gap",[["gap",["gap"]],[["gap-x",["columnGap"]],["gap-y",["rowGap"]]]]),space:({matchUtilities:r,addUtilities:e,theme:t})=>{r({"space-x":i=>(i=i==="0"?"0px":i,{"& > :not([hidden]) ~ :not([hidden])":{"--tw-space-x-reverse":"0","margin-right":`calc(${i} * var(--tw-space-x-reverse))`,"margin-left":`calc(${i} * calc(1 - var(--tw-space-x-reverse)))`}}),"space-y":i=>(i=i==="0"?"0px":i,{"& > :not([hidden]) ~ :not([hidden])":{"--tw-space-y-reverse":"0","margin-top":`calc(${i} * calc(1 - var(--tw-space-y-reverse)))`,"margin-bottom":`calc(${i} * var(--tw-space-y-reverse))`}})},{values:t("space"),supportsNegativeValues:!0}),e({".space-y-reverse > :not([hidden]) ~ :not([hidden])":{"--tw-space-y-reverse":"1"},".space-x-reverse > :not([hidden]) ~ :not([hidden])":{"--tw-space-x-reverse":"1"}})},divideWidth:({matchUtilities:r,addUtilities:e,theme:t})=>{r({"divide-x":i=>(i=i==="0"?"0px":i,{"& > :not([hidden]) ~ :not([hidden])":{"@defaults border-width":{},"--tw-divide-x-reverse":"0","border-right-width":`calc(${i} * var(--tw-divide-x-reverse))`,"border-left-width":`calc(${i} * calc(1 - var(--tw-divide-x-reverse)))`}}),"divide-y":i=>(i=i==="0"?"0px":i,{"& > :not([hidden]) ~ :not([hidden])":{"@defaults border-width":{},"--tw-divide-y-reverse":"0","border-top-width":`calc(${i} * calc(1 - var(--tw-divide-y-reverse)))`,"border-bottom-width":`calc(${i} * var(--tw-divide-y-reverse))`}})},{values:t("divideWidth"),type:["line-width","length","any"]}),e({".divide-y-reverse > :not([hidden]) ~ :not([hidden])":{"@defaults border-width":{},"--tw-divide-y-reverse":"1"},".divide-x-reverse > :not([hidden]) ~ :not([hidden])":{"@defaults border-width":{},"--tw-divide-x-reverse":"1"}})},divideStyle:({addUtilities:r})=>{r({".divide-solid > :not([hidden]) ~ :not([hidden])":{"border-style":"solid"},".divide-dashed > :not([hidden]) ~ :not([hidden])":{"border-style":"dashed"},".divide-dotted > :not([hidden]) ~ :not([hidden])":{"border-style":"dotted"},".divide-double > :not([hidden]) ~ :not([hidden])":{"border-style":"double"},".divide-none > :not([hidden]) ~ :not([hidden])":{"border-style":"none"}})},divideColor:({matchUtilities:r,theme:e,corePlugins:t})=>{r({divide:i=>t("divideOpacity")?{["& > :not([hidden]) ~ :not([hidden])"]:Ae({color:i,property:"border-color",variable:"--tw-divide-opacity"})}:{["& > :not([hidden]) ~ :not([hidden])"]:{"border-color":X(i)}}},{values:(({DEFAULT:i,...n})=>n)(xe(e("divideColor"))),type:["color","any"]})},divideOpacity:({matchUtilities:r,theme:e})=>{r({"divide-opacity":t=>({["& > :not([hidden]) ~ :not([hidden])"]:{"--tw-divide-opacity":t}})},{values:e("divideOpacity")})},placeSelf:({addUtilities:r})=>{r({".place-self-auto":{"place-self":"auto"},".place-self-start":{"place-self":"start"},".place-self-end":{"place-self":"end"},".place-self-center":{"place-self":"center"},".place-self-stretch":{"place-self":"stretch"}})},alignSelf:({addUtilities:r})=>{r({".self-auto":{"align-self":"auto"},".self-start":{"align-self":"flex-start"},".self-end":{"align-self":"flex-end"},".self-center":{"align-self":"center"},".self-stretch":{"align-self":"stretch"},".self-baseline":{"align-self":"baseline"}})},justifySelf:({addUtilities:r})=>{r({".justify-self-auto":{"justify-self":"auto"},".justify-self-start":{"justify-self":"start"},".justify-self-end":{"justify-self":"end"},".justify-self-center":{"justify-self":"center"},".justify-self-stretch":{"justify-self":"stretch"}})},overflow:({addUtilities:r})=>{r({".overflow-auto":{overflow:"auto"},".overflow-hidden":{overflow:"hidden"},".overflow-clip":{overflow:"clip"},".overflow-visible":{overflow:"visible"},".overflow-scroll":{overflow:"scroll"},".overflow-x-auto":{"overflow-x":"auto"},".overflow-y-auto":{"overflow-y":"auto"},".overflow-x-hidden":{"overflow-x":"hidden"},".overflow-y-hidden":{"overflow-y":"hidden"},".overflow-x-clip":{"overflow-x":"clip"},".overflow-y-clip":{"overflow-y":"clip"},".overflow-x-visible":{"overflow-x":"visible"},".overflow-y-visible":{"overflow-y":"visible"},".overflow-x-scroll":{"overflow-x":"scroll"},".overflow-y-scroll":{"overflow-y":"scroll"}})},overscrollBehavior:({addUtilities:r})=>{r({".overscroll-auto":{"overscroll-behavior":"auto"},".overscroll-contain":{"overscroll-behavior":"contain"},".overscroll-none":{"overscroll-behavior":"none"},".overscroll-y-auto":{"overscroll-behavior-y":"auto"},".overscroll-y-contain":{"overscroll-behavior-y":"contain"},".overscroll-y-none":{"overscroll-behavior-y":"none"},".overscroll-x-auto":{"overscroll-behavior-x":"auto"},".overscroll-x-contain":{"overscroll-behavior-x":"contain"},".overscroll-x-none":{"overscroll-behavior-x":"none"}})},scrollBehavior:({addUtilities:r})=>{r({".scroll-auto":{"scroll-behavior":"auto"},".scroll-smooth":{"scroll-behavior":"smooth"}})},textOverflow:({addUtilities:r})=>{r({".truncate":{overflow:"hidden","text-overflow":"ellipsis","white-space":"nowrap"},".overflow-ellipsis":{"text-overflow":"ellipsis"},".text-ellipsis":{"text-overflow":"ellipsis"},".text-clip":{"text-overflow":"clip"}})},hyphens:({addUtilities:r})=>{r({".hyphens-none":{hyphens:"none"},".hyphens-manual":{hyphens:"manual"},".hyphens-auto":{hyphens:"auto"}})},whitespace:({addUtilities:r})=>{r({".whitespace-normal":{"white-space":"normal"},".whitespace-nowrap":{"white-space":"nowrap"},".whitespace-pre":{"white-space":"pre"},".whitespace-pre-line":{"white-space":"pre-line"},".whitespace-pre-wrap":{"white-space":"pre-wrap"},".whitespace-break-spaces":{"white-space":"break-spaces"}})},textWrap:({addUtilities:r})=>{r({".text-wrap":{"text-wrap":"wrap"},".text-nowrap":{"text-wrap":"nowrap"},".text-balance":{"text-wrap":"balance"},".text-pretty":{"text-wrap":"pretty"}})},wordBreak:({addUtilities:r})=>{r({".break-normal":{"overflow-wrap":"normal","word-break":"normal"},".break-words":{"overflow-wrap":"break-word"},".break-all":{"word-break":"break-all"},".break-keep":{"word-break":"keep-all"}})},borderRadius:L("borderRadius",[["rounded",["border-radius"]],[["rounded-s",["border-start-start-radius","border-end-start-radius"]],["rounded-e",["border-start-end-radius","border-end-end-radius"]],["rounded-t",["border-top-left-radius","border-top-right-radius"]],["rounded-r",["border-top-right-radius","border-bottom-right-radius"]],["rounded-b",["border-bottom-right-radius","border-bottom-left-radius"]],["rounded-l",["border-top-left-radius","border-bottom-left-radius"]]],[["rounded-ss",["border-start-start-radius"]],["rounded-se",["border-start-end-radius"]],["rounded-ee",["border-end-end-radius"]],["rounded-es",["border-end-start-radius"]],["rounded-tl",["border-top-left-radius"]],["rounded-tr",["border-top-right-radius"]],["rounded-br",["border-bottom-right-radius"]],["rounded-bl",["border-bottom-left-radius"]]]]),borderWidth:L("borderWidth",[["border",[["@defaults border-width",{}],"border-width"]],[["border-x",[["@defaults border-width",{}],"border-left-width","border-right-width"]],["border-y",[["@defaults border-width",{}],"border-top-width","border-bottom-width"]]],[["border-s",[["@defaults border-width",{}],"border-inline-start-width"]],["border-e",[["@defaults border-width",{}],"border-inline-end-width"]],["border-t",[["@defaults border-width",{}],"border-top-width"]],["border-r",[["@defaults border-width",{}],"border-right-width"]],["border-b",[["@defaults border-width",{}],"border-bottom-width"]],["border-l",[["@defaults border-width",{}],"border-left-width"]]]],{type:["line-width","length"]}),borderStyle:({addUtilities:r})=>{r({".border-solid":{"border-style":"solid"},".border-dashed":{"border-style":"dashed"},".border-dotted":{"border-style":"dotted"},".border-double":{"border-style":"double"},".border-hidden":{"border-style":"hidden"},".border-none":{"border-style":"none"}})},borderColor:({matchUtilities:r,theme:e,corePlugins:t})=>{r({border:i=>t("borderOpacity")?Ae({color:i,property:"border-color",variable:"--tw-border-opacity"}):{"border-color":X(i)}},{values:(({DEFAULT:i,...n})=>n)(xe(e("borderColor"))),type:["color","any"]}),r({"border-x":i=>t("borderOpacity")?Ae({color:i,property:["border-left-color","border-right-color"],variable:"--tw-border-opacity"}):{"border-left-color":X(i),"border-right-color":X(i)},"border-y":i=>t("borderOpacity")?Ae({color:i,property:["border-top-color","border-bottom-color"],variable:"--tw-border-opacity"}):{"border-top-color":X(i),"border-bottom-color":X(i)}},{values:(({DEFAULT:i,...n})=>n)(xe(e("borderColor"))),type:["color","any"]}),r({"border-s":i=>t("borderOpacity")?Ae({color:i,property:"border-inline-start-color",variable:"--tw-border-opacity"}):{"border-inline-start-color":X(i)},"border-e":i=>t("borderOpacity")?Ae({color:i,property:"border-inline-end-color",variable:"--tw-border-opacity"}):{"border-inline-end-color":X(i)},"border-t":i=>t("borderOpacity")?Ae({color:i,property:"border-top-color",variable:"--tw-border-opacity"}):{"border-top-color":X(i)},"border-r":i=>t("borderOpacity")?Ae({color:i,property:"border-right-color",variable:"--tw-border-opacity"}):{"border-right-color":X(i)},"border-b":i=>t("borderOpacity")?Ae({color:i,property:"border-bottom-color",variable:"--tw-border-opacity"}):{"border-bottom-color":X(i)},"border-l":i=>t("borderOpacity")?Ae({color:i,property:"border-left-color",variable:"--tw-border-opacity"}):{"border-left-color":X(i)}},{values:(({DEFAULT:i,...n})=>n)(xe(e("borderColor"))),type:["color","any"]})},borderOpacity:L("borderOpacity",[["border-opacity",["--tw-border-opacity"]]]),backgroundColor:({matchUtilities:r,theme:e,corePlugins:t})=>{r({bg:i=>t("backgroundOpacity")?Ae({color:i,property:"background-color",variable:"--tw-bg-opacity"}):{"background-color":X(i)}},{values:xe(e("backgroundColor")),type:["color","any"]})},backgroundOpacity:L("backgroundOpacity",[["bg-opacity",["--tw-bg-opacity"]]]),backgroundImage:L("backgroundImage",[["bg",["background-image"]]],{type:["lookup","image","url"]}),gradientColorStops:(()=>{function r(e){return Je(e,0,"rgb(255 255 255 / 0)")}return function({matchUtilities:e,theme:t,addDefaults:i}){i("gradient-color-stops",{"--tw-gradient-from-position":" ","--tw-gradient-via-position":" ","--tw-gradient-to-position":" "});let n={values:xe(t("gradientColorStops")),type:["color","any"]},s={values:t("gradientColorStopPositions"),type:["length","percentage"]};e({from:a=>{let o=r(a);return{"@defaults gradient-color-stops":{},"--tw-gradient-from":`${X(a)} var(--tw-gradient-from-position)`,"--tw-gradient-to":`${o} var(--tw-gradient-to-position)`,"--tw-gradient-stops":"var(--tw-gradient-from), var(--tw-gradient-to)"}}},n),e({from:a=>({"--tw-gradient-from-position":a})},s),e({via:a=>{let o=r(a);return{"@defaults gradient-color-stops":{},"--tw-gradient-to":`${o}  var(--tw-gradient-to-position)`,"--tw-gradient-stops":`var(--tw-gradient-from), ${X(a)} var(--tw-gradient-via-position), var(--tw-gradient-to)`}}},n),e({via:a=>({"--tw-gradient-via-position":a})},s),e({to:a=>({"@defaults gradient-color-stops":{},"--tw-gradient-to":`${X(a)} var(--tw-gradient-to-position)`})},n),e({to:a=>({"--tw-gradient-to-position":a})},s)}})(),boxDecorationBreak:({addUtilities:r})=>{r({".decoration-slice":{"box-decoration-break":"slice"},".decoration-clone":{"box-decoration-break":"clone"},".box-decoration-slice":{"box-decoration-break":"slice"},".box-decoration-clone":{"box-decoration-break":"clone"}})},backgroundSize:L("backgroundSize",[["bg",["background-size"]]],{type:["lookup","length","percentage","size"]}),backgroundAttachment:({addUtilities:r})=>{r({".bg-fixed":{"background-attachment":"fixed"},".bg-local":{"background-attachment":"local"},".bg-scroll":{"background-attachment":"scroll"}})},backgroundClip:({addUtilities:r})=>{r({".bg-clip-border":{"background-clip":"border-box"},".bg-clip-padding":{"background-clip":"padding-box"},".bg-clip-content":{"background-clip":"content-box"},".bg-clip-text":{"background-clip":"text"}})},backgroundPosition:L("backgroundPosition",[["bg",["background-position"]]],{type:["lookup",["position",{preferOnConflict:!0}]]}),backgroundRepeat:({addUtilities:r})=>{r({".bg-repeat":{"background-repeat":"repeat"},".bg-no-repeat":{"background-repeat":"no-repeat"},".bg-repeat-x":{"background-repeat":"repeat-x"},".bg-repeat-y":{"background-repeat":"repeat-y"},".bg-repeat-round":{"background-repeat":"round"},".bg-repeat-space":{"background-repeat":"space"}})},backgroundOrigin:({addUtilities:r})=>{r({".bg-origin-border":{"background-origin":"border-box"},".bg-origin-padding":{"background-origin":"padding-box"},".bg-origin-content":{"background-origin":"content-box"}})},fill:({matchUtilities:r,theme:e})=>{r({fill:t=>({fill:X(t)})},{values:xe(e("fill")),type:["color","any"]})},stroke:({matchUtilities:r,theme:e})=>{r({stroke:t=>({stroke:X(t)})},{values:xe(e("stroke")),type:["color","url","any"]})},strokeWidth:L("strokeWidth",[["stroke",["stroke-width"]]],{type:["length","number","percentage"]}),objectFit:({addUtilities:r})=>{r({".object-contain":{"object-fit":"contain"},".object-cover":{"object-fit":"cover"},".object-fill":{"object-fit":"fill"},".object-none":{"object-fit":"none"},".object-scale-down":{"object-fit":"scale-down"}})},objectPosition:L("objectPosition",[["object",["object-position"]]]),padding:L("padding",[["p",["padding"]],[["px",["padding-left","padding-right"]],["py",["padding-top","padding-bottom"]]],[["ps",["padding-inline-start"]],["pe",["padding-inline-end"]],["pt",["padding-top"]],["pr",["padding-right"]],["pb",["padding-bottom"]],["pl",["padding-left"]]]]),textAlign:({addUtilities:r})=>{r({".text-left":{"text-align":"left"},".text-center":{"text-align":"center"},".text-right":{"text-align":"right"},".text-justify":{"text-align":"justify"},".text-start":{"text-align":"start"},".text-end":{"text-align":"end"}})},textIndent:L("textIndent",[["indent",["text-indent"]]],{supportsNegativeValues:!0}),verticalAlign:({addUtilities:r,matchUtilities:e})=>{r({".align-baseline":{"vertical-align":"baseline"},".align-top":{"vertical-align":"top"},".align-middle":{"vertical-align":"middle"},".align-bottom":{"vertical-align":"bottom"},".align-text-top":{"vertical-align":"text-top"},".align-text-bottom":{"vertical-align":"text-bottom"},".align-sub":{"vertical-align":"sub"},".align-super":{"vertical-align":"super"}}),e({align:t=>({"vertical-align":t})})},fontFamily:({matchUtilities:r,theme:e})=>{r({font:t=>{let[i,n={}]=Array.isArray(t)&&ke(t[1])?t:[t],{fontFeatureSettings:s,fontVariationSettings:a}=n;return{"font-family":Array.isArray(i)?i.join(", "):i,...s===void 0?{}:{"font-feature-settings":s},...a===void 0?{}:{"font-variation-settings":a}}}},{values:e("fontFamily"),type:["lookup","generic-name","family-name"]})},fontSize:({matchUtilities:r,theme:e})=>{r({text:(t,{modifier:i})=>{let[n,s]=Array.isArray(t)?t:[t];if(i)return{"font-size":n,"line-height":i};let{lineHeight:a,letterSpacing:o,fontWeight:l}=ke(s)?s:{lineHeight:s};return{"font-size":n,...a===void 0?{}:{"line-height":a},...o===void 0?{}:{"letter-spacing":o},...l===void 0?{}:{"font-weight":l}}}},{values:e("fontSize"),modifiers:e("lineHeight"),type:["absolute-size","relative-size","length","percentage"]})},fontWeight:L("fontWeight",[["font",["fontWeight"]]],{type:["lookup","number","any"]}),textTransform:({addUtilities:r})=>{r({".uppercase":{"text-transform":"uppercase"},".lowercase":{"text-transform":"lowercase"},".capitalize":{"text-transform":"capitalize"},".normal-case":{"text-transform":"none"}})},fontStyle:({addUtilities:r})=>{r({".italic":{"font-style":"italic"},".not-italic":{"font-style":"normal"}})},fontVariantNumeric:({addDefaults:r,addUtilities:e})=>{let t="var(--tw-ordinal) var(--tw-slashed-zero) var(--tw-numeric-figure) var(--tw-numeric-spacing) var(--tw-numeric-fraction)";r("font-variant-numeric",{"--tw-ordinal":" ","--tw-slashed-zero":" ","--tw-numeric-figure":" ","--tw-numeric-spacing":" ","--tw-numeric-fraction":" "}),e({".normal-nums":{"font-variant-numeric":"normal"},".ordinal":{"@defaults font-variant-numeric":{},"--tw-ordinal":"ordinal","font-variant-numeric":t},".slashed-zero":{"@defaults font-variant-numeric":{},"--tw-slashed-zero":"slashed-zero","font-variant-numeric":t},".lining-nums":{"@defaults font-variant-numeric":{},"--tw-numeric-figure":"lining-nums","font-variant-numeric":t},".oldstyle-nums":{"@defaults font-variant-numeric":{},"--tw-numeric-figure":"oldstyle-nums","font-variant-numeric":t},".proportional-nums":{"@defaults font-variant-numeric":{},"--tw-numeric-spacing":"proportional-nums","font-variant-numeric":t},".tabular-nums":{"@defaults font-variant-numeric":{},"--tw-numeric-spacing":"tabular-nums","font-variant-numeric":t},".diagonal-fractions":{"@defaults font-variant-numeric":{},"--tw-numeric-fraction":"diagonal-fractions","font-variant-numeric":t},".stacked-fractions":{"@defaults font-variant-numeric":{},"--tw-numeric-fraction":"stacked-fractions","font-variant-numeric":t}})},lineHeight:L("lineHeight",[["leading",["lineHeight"]]]),letterSpacing:L("letterSpacing",[["tracking",["letterSpacing"]]],{supportsNegativeValues:!0}),textColor:({matchUtilities:r,theme:e,corePlugins:t})=>{r({text:i=>t("textOpacity")?Ae({color:i,property:"color",variable:"--tw-text-opacity"}):{color:X(i)}},{values:xe(e("textColor")),type:["color","any"]})},textOpacity:L("textOpacity",[["text-opacity",["--tw-text-opacity"]]]),textDecoration:({addUtilities:r})=>{r({".underline":{"text-decoration-line":"underline"},".overline":{"text-decoration-line":"overline"},".line-through":{"text-decoration-line":"line-through"},".no-underline":{"text-decoration-line":"none"}})},textDecorationColor:({matchUtilities:r,theme:e})=>{r({decoration:t=>({"text-decoration-color":X(t)})},{values:xe(e("textDecorationColor")),type:["color","any"]})},textDecorationStyle:({addUtilities:r})=>{r({".decoration-solid":{"text-decoration-style":"solid"},".decoration-double":{"text-decoration-style":"double"},".decoration-dotted":{"text-decoration-style":"dotted"},".decoration-dashed":{"text-decoration-style":"dashed"},".decoration-wavy":{"text-decoration-style":"wavy"}})},textDecorationThickness:L("textDecorationThickness",[["decoration",["text-decoration-thickness"]]],{type:["length","percentage"]}),textUnderlineOffset:L("textUnderlineOffset",[["underline-offset",["text-underline-offset"]]],{type:["length","percentage","any"]}),fontSmoothing:({addUtilities:r})=>{r({".antialiased":{"-webkit-font-smoothing":"antialiased","-moz-osx-font-smoothing":"grayscale"},".subpixel-antialiased":{"-webkit-font-smoothing":"auto","-moz-osx-font-smoothing":"auto"}})},placeholderColor:({matchUtilities:r,theme:e,corePlugins:t})=>{r({placeholder:i=>t("placeholderOpacity")?{"&::placeholder":Ae({color:i,property:"color",variable:"--tw-placeholder-opacity"})}:{"&::placeholder":{color:X(i)}}},{values:xe(e("placeholderColor")),type:["color","any"]})},placeholderOpacity:({matchUtilities:r,theme:e})=>{r({"placeholder-opacity":t=>({["&::placeholder"]:{"--tw-placeholder-opacity":t}})},{values:e("placeholderOpacity")})},caretColor:({matchUtilities:r,theme:e})=>{r({caret:t=>({"caret-color":X(t)})},{values:xe(e("caretColor")),type:["color","any"]})},accentColor:({matchUtilities:r,theme:e})=>{r({accent:t=>({"accent-color":X(t)})},{values:xe(e("accentColor")),type:["color","any"]})},opacity:L("opacity",[["opacity",["opacity"]]]),backgroundBlendMode:({addUtilities:r})=>{r({".bg-blend-normal":{"background-blend-mode":"normal"},".bg-blend-multiply":{"background-blend-mode":"multiply"},".bg-blend-screen":{"background-blend-mode":"screen"},".bg-blend-overlay":{"background-blend-mode":"overlay"},".bg-blend-darken":{"background-blend-mode":"darken"},".bg-blend-lighten":{"background-blend-mode":"lighten"},".bg-blend-color-dodge":{"background-blend-mode":"color-dodge"},".bg-blend-color-burn":{"background-blend-mode":"color-burn"},".bg-blend-hard-light":{"background-blend-mode":"hard-light"},".bg-blend-soft-light":{"background-blend-mode":"soft-light"},".bg-blend-difference":{"background-blend-mode":"difference"},".bg-blend-exclusion":{"background-blend-mode":"exclusion"},".bg-blend-hue":{"background-blend-mode":"hue"},".bg-blend-saturation":{"background-blend-mode":"saturation"},".bg-blend-color":{"background-blend-mode":"color"},".bg-blend-luminosity":{"background-blend-mode":"luminosity"}})},mixBlendMode:({addUtilities:r})=>{r({".mix-blend-normal":{"mix-blend-mode":"normal"},".mix-blend-multiply":{"mix-blend-mode":"multiply"},".mix-blend-screen":{"mix-blend-mode":"screen"},".mix-blend-overlay":{"mix-blend-mode":"overlay"},".mix-blend-darken":{"mix-blend-mode":"darken"},".mix-blend-lighten":{"mix-blend-mode":"lighten"},".mix-blend-color-dodge":{"mix-blend-mode":"color-dodge"},".mix-blend-color-burn":{"mix-blend-mode":"color-burn"},".mix-blend-hard-light":{"mix-blend-mode":"hard-light"},".mix-blend-soft-light":{"mix-blend-mode":"soft-light"},".mix-blend-difference":{"mix-blend-mode":"difference"},".mix-blend-exclusion":{"mix-blend-mode":"exclusion"},".mix-blend-hue":{"mix-blend-mode":"hue"},".mix-blend-saturation":{"mix-blend-mode":"saturation"},".mix-blend-color":{"mix-blend-mode":"color"},".mix-blend-luminosity":{"mix-blend-mode":"luminosity"},".mix-blend-plus-darker":{"mix-blend-mode":"plus-darker"},".mix-blend-plus-lighter":{"mix-blend-mode":"plus-lighter"}})},boxShadow:(()=>{let r=mt("boxShadow"),e=["var(--tw-ring-offset-shadow, 0 0 #0000)","var(--tw-ring-shadow, 0 0 #0000)","var(--tw-shadow)"].join(", ");return function({matchUtilities:t,addDefaults:i,theme:n}){i("box-shadow",{"--tw-ring-offset-shadow":"0 0 #0000","--tw-ring-shadow":"0 0 #0000","--tw-shadow":"0 0 #0000","--tw-shadow-colored":"0 0 #0000"}),t({shadow:s=>{s=r(s);let a=en(s);for(let o of a)!o.valid||(o.color="var(--tw-shadow-color)");return{"@defaults box-shadow":{},"--tw-shadow":s==="none"?"0 0 #0000":s,"--tw-shadow-colored":s==="none"?"0 0 #0000":Lf(a),"box-shadow":e}}},{values:n("boxShadow"),type:["shadow"]})}})(),boxShadowColor:({matchUtilities:r,theme:e})=>{r({shadow:t=>({"--tw-shadow-color":X(t),"--tw-shadow":"var(--tw-shadow-colored)"})},{values:xe(e("boxShadowColor")),type:["color","any"]})},outlineStyle:({addUtilities:r})=>{r({".outline-none":{outline:"2px solid transparent","outline-offset":"2px"},".outline":{"outline-style":"solid"},".outline-dashed":{"outline-style":"dashed"},".outline-dotted":{"outline-style":"dotted"},".outline-double":{"outline-style":"double"}})},outlineWidth:L("outlineWidth",[["outline",["outline-width"]]],{type:["length","number","percentage"]}),outlineOffset:L("outlineOffset",[["outline-offset",["outline-offset"]]],{type:["length","number","percentage","any"],supportsNegativeValues:!0}),outlineColor:({matchUtilities:r,theme:e})=>{r({outline:t=>({"outline-color":X(t)})},{values:xe(e("outlineColor")),type:["color","any"]})},ringWidth:({matchUtilities:r,addDefaults:e,addUtilities:t,theme:i,config:n})=>{let s=(()=>{if(we(n(),"respectDefaultRingColorOpacity"))return i("ringColor.DEFAULT");let a=i("ringOpacity.DEFAULT","0.5");return i("ringColor")?.DEFAULT?Je(i("ringColor")?.DEFAULT,a,`rgb(147 197 253 / ${a})`):`rgb(147 197 253 / ${a})`})();e("ring-width",{"--tw-ring-inset":" ","--tw-ring-offset-width":i("ringOffsetWidth.DEFAULT","0px"),"--tw-ring-offset-color":i("ringOffsetColor.DEFAULT","#fff"),"--tw-ring-color":s,"--tw-ring-offset-shadow":"0 0 #0000","--tw-ring-shadow":"0 0 #0000","--tw-shadow":"0 0 #0000","--tw-shadow-colored":"0 0 #0000"}),r({ring:a=>({"@defaults ring-width":{},"--tw-ring-offset-shadow":"var(--tw-ring-inset) 0 0 0 var(--tw-ring-offset-width) var(--tw-ring-offset-color)","--tw-ring-shadow":`var(--tw-ring-inset) 0 0 0 calc(${a} + var(--tw-ring-offset-width)) var(--tw-ring-color)`,"box-shadow":["var(--tw-ring-offset-shadow)","var(--tw-ring-shadow)","var(--tw-shadow, 0 0 #0000)"].join(", ")})},{values:i("ringWidth"),type:"length"}),t({".ring-inset":{"@defaults ring-width":{},"--tw-ring-inset":"inset"}})},ringColor:({matchUtilities:r,theme:e,corePlugins:t})=>{r({ring:i=>t("ringOpacity")?Ae({color:i,property:"--tw-ring-color",variable:"--tw-ring-opacity"}):{"--tw-ring-color":X(i)}},{values:Object.fromEntries(Object.entries(xe(e("ringColor"))).filter(([i])=>i!=="DEFAULT")),type:["color","any"]})},ringOpacity:r=>{let{config:e}=r;return L("ringOpacity",[["ring-opacity",["--tw-ring-opacity"]]],{filterDefault:!we(e(),"respectDefaultRingColorOpacity")})(r)},ringOffsetWidth:L("ringOffsetWidth",[["ring-offset",["--tw-ring-offset-width"]]],{type:"length"}),ringOffsetColor:({matchUtilities:r,theme:e})=>{r({"ring-offset":t=>({"--tw-ring-offset-color":X(t)})},{values:xe(e("ringOffsetColor")),type:["color","any"]})},blur:({matchUtilities:r,theme:e})=>{r({blur:t=>({"--tw-blur":t.trim()===""?" ":`blur(${t})`,"@defaults filter":{},filter:nt})},{values:e("blur")})},brightness:({matchUtilities:r,theme:e})=>{r({brightness:t=>({"--tw-brightness":`brightness(${t})`,"@defaults filter":{},filter:nt})},{values:e("brightness")})},contrast:({matchUtilities:r,theme:e})=>{r({contrast:t=>({"--tw-contrast":`contrast(${t})`,"@defaults filter":{},filter:nt})},{values:e("contrast")})},dropShadow:({matchUtilities:r,theme:e})=>{r({"drop-shadow":t=>({"--tw-drop-shadow":Array.isArray(t)?t.map(i=>`drop-shadow(${i})`).join(" "):`drop-shadow(${t})`,"@defaults filter":{},filter:nt})},{values:e("dropShadow")})},grayscale:({matchUtilities:r,theme:e})=>{r({grayscale:t=>({"--tw-grayscale":`grayscale(${t})`,"@defaults filter":{},filter:nt})},{values:e("grayscale")})},hueRotate:({matchUtilities:r,theme:e})=>{r({"hue-rotate":t=>({"--tw-hue-rotate":`hue-rotate(${t})`,"@defaults filter":{},filter:nt})},{values:e("hueRotate"),supportsNegativeValues:!0})},invert:({matchUtilities:r,theme:e})=>{r({invert:t=>({"--tw-invert":`invert(${t})`,"@defaults filter":{},filter:nt})},{values:e("invert")})},saturate:({matchUtilities:r,theme:e})=>{r({saturate:t=>({"--tw-saturate":`saturate(${t})`,"@defaults filter":{},filter:nt})},{values:e("saturate")})},sepia:({matchUtilities:r,theme:e})=>{r({sepia:t=>({"--tw-sepia":`sepia(${t})`,"@defaults filter":{},filter:nt})},{values:e("sepia")})},filter:({addDefaults:r,addUtilities:e})=>{r("filter",{"--tw-blur":" ","--tw-brightness":" ","--tw-contrast":" ","--tw-grayscale":" ","--tw-hue-rotate":" ","--tw-invert":" ","--tw-saturate":" ","--tw-sepia":" ","--tw-drop-shadow":" "}),e({".filter":{"@defaults filter":{},filter:nt},".filter-none":{filter:"none"}})},backdropBlur:({matchUtilities:r,theme:e})=>{r({"backdrop-blur":t=>({"--tw-backdrop-blur":t.trim()===""?" ":`blur(${t})`,"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge})},{values:e("backdropBlur")})},backdropBrightness:({matchUtilities:r,theme:e})=>{r({"backdrop-brightness":t=>({"--tw-backdrop-brightness":`brightness(${t})`,"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge})},{values:e("backdropBrightness")})},backdropContrast:({matchUtilities:r,theme:e})=>{r({"backdrop-contrast":t=>({"--tw-backdrop-contrast":`contrast(${t})`,"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge})},{values:e("backdropContrast")})},backdropGrayscale:({matchUtilities:r,theme:e})=>{r({"backdrop-grayscale":t=>({"--tw-backdrop-grayscale":`grayscale(${t})`,"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge})},{values:e("backdropGrayscale")})},backdropHueRotate:({matchUtilities:r,theme:e})=>{r({"backdrop-hue-rotate":t=>({"--tw-backdrop-hue-rotate":`hue-rotate(${t})`,"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge})},{values:e("backdropHueRotate"),supportsNegativeValues:!0})},backdropInvert:({matchUtilities:r,theme:e})=>{r({"backdrop-invert":t=>({"--tw-backdrop-invert":`invert(${t})`,"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge})},{values:e("backdropInvert")})},backdropOpacity:({matchUtilities:r,theme:e})=>{r({"backdrop-opacity":t=>({"--tw-backdrop-opacity":`opacity(${t})`,"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge})},{values:e("backdropOpacity")})},backdropSaturate:({matchUtilities:r,theme:e})=>{r({"backdrop-saturate":t=>({"--tw-backdrop-saturate":`saturate(${t})`,"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge})},{values:e("backdropSaturate")})},backdropSepia:({matchUtilities:r,theme:e})=>{r({"backdrop-sepia":t=>({"--tw-backdrop-sepia":`sepia(${t})`,"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge})},{values:e("backdropSepia")})},backdropFilter:({addDefaults:r,addUtilities:e})=>{r("backdrop-filter",{"--tw-backdrop-blur":" ","--tw-backdrop-brightness":" ","--tw-backdrop-contrast":" ","--tw-backdrop-grayscale":" ","--tw-backdrop-hue-rotate":" ","--tw-backdrop-invert":" ","--tw-backdrop-opacity":" ","--tw-backdrop-saturate":" ","--tw-backdrop-sepia":" "}),e({".backdrop-filter":{"@defaults backdrop-filter":{},"-webkit-backdrop-filter":ge,"backdrop-filter":ge},".backdrop-filter-none":{"-webkit-backdrop-filter":"none","backdrop-filter":"none"}})},transitionProperty:({matchUtilities:r,theme:e})=>{let t=e("transitionTimingFunction.DEFAULT"),i=e("transitionDuration.DEFAULT");r({transition:n=>({"transition-property":n,...n==="none"?{}:{"transition-timing-function":t,"transition-duration":i}})},{values:e("transitionProperty")})},transitionDelay:L("transitionDelay",[["delay",["transitionDelay"]]]),transitionDuration:L("transitionDuration",[["duration",["transitionDuration"]]],{filterDefault:!0}),transitionTimingFunction:L("transitionTimingFunction",[["ease",["transitionTimingFunction"]]],{filterDefault:!0}),willChange:L("willChange",[["will-change",["will-change"]]]),contain:({addDefaults:r,addUtilities:e})=>{let t="var(--tw-contain-size) var(--tw-contain-layout) var(--tw-contain-paint) var(--tw-contain-style)";r("contain",{"--tw-contain-size":" ","--tw-contain-layout":" ","--tw-contain-paint":" ","--tw-contain-style":" "}),e({".contain-none":{contain:"none"},".contain-content":{contain:"content"},".contain-strict":{contain:"strict"},".contain-size":{"@defaults contain":{},"--tw-contain-size":"size",contain:t},".contain-inline-size":{"@defaults contain":{},"--tw-contain-size":"inline-size",contain:t},".contain-layout":{"@defaults contain":{},"--tw-contain-layout":"layout",contain:t},".contain-paint":{"@defaults contain":{},"--tw-contain-paint":"paint",contain:t},".contain-style":{"@defaults contain":{},"--tw-contain-style":"style",contain:t}})},content:L("content",[["content",["--tw-content",["content","var(--tw-content)"]]]]),forcedColorAdjust:({addUtilities:r})=>{r({".forced-color-adjust-auto":{"forced-color-adjust":"auto"},".forced-color-adjust-none":{"forced-color-adjust":"none"}})}}});function h_(r){if(r===void 0)return!1;if(r==="true"||r==="1")return!0;if(r==="false"||r==="0")return!1;if(r==="*")return!0;let e=r.split(",").map(t=>t.split(":")[0]);return e.includes("-tailwindcss")?!1:!!e.includes("tailwindcss")}var Ze,wh,vh,es,No,gt,Ti,It=P(()=>{u();Ze=typeof m!="undefined"?{NODE_ENV:"production",DEBUG:h_(m.env.DEBUG)}:{NODE_ENV:"production",DEBUG:!1},wh=new Map,vh=new Map,es=new Map,No=new Map,gt=new String("*"),Ti=Symbol("__NONE__")});function cr(r){let e=[],t=!1;for(let i=0;i<r.length;i++){let n=r[i];if(n===":"&&!t&&e.length===0)return!1;if(m_.has(n)&&r[i-1]!=="\\"&&(t=!t),!t&&r[i-1]!=="\\"){if(xh.has(n))e.push(n);else if(kh.has(n)){let s=kh.get(n);if(e.length<=0||e.pop()!==s)return!1}}}return!(e.length>0)}var xh,kh,m_,Bo=P(()=>{u();xh=new Map([["{","}"],["[","]"],["(",")"]]),kh=new Map(Array.from(xh.entries()).map(([r,e])=>[e,r])),m_=new Set(['"',"'","`"])});function pr(r){let[e]=Sh(r);return e.forEach(([t,i])=>t.removeChild(i)),r.nodes.push(...e.map(([,t])=>t)),r}function Sh(r){let e=[],t=null;for(let i of r.nodes)if(i.type==="combinator")e=e.filter(([,n])=>jo(n).includes("jumpable")),t=null;else if(i.type==="pseudo"){g_(i)?(t=i,e.push([r,i,null])):t&&y_(i,t)?e.push([r,i,t]):t=null;for(let n of i.nodes??[]){let[s,a]=Sh(n);t=a||t,e.push(...s)}}return[e,t]}function Ah(r){return r.value.startsWith("::")||Fo[r.value]!==void 0}function g_(r){return Ah(r)&&jo(r).includes("terminal")}function y_(r,e){return r.type!=="pseudo"||Ah(r)?!1:jo(e).includes("actionable")}function jo(r){return Fo[r.value]??Fo.__default__}var Fo,ts=P(()=>{u();Fo={"::after":["terminal","jumpable"],"::backdrop":["terminal","jumpable"],"::before":["terminal","jumpable"],"::cue":["terminal"],"::cue-region":["terminal"],"::first-letter":["terminal","jumpable"],"::first-line":["terminal","jumpable"],"::grammar-error":["terminal"],"::marker":["terminal","jumpable"],"::part":["terminal","actionable"],"::placeholder":["terminal","jumpable"],"::selection":["terminal","jumpable"],"::slotted":["terminal"],"::spelling-error":["terminal"],"::target-text":["terminal"],"::file-selector-button":["terminal","actionable"],"::deep":["actionable"],"::v-deep":["actionable"],"::ng-deep":["actionable"],":after":["terminal","jumpable"],":before":["terminal","jumpable"],":first-letter":["terminal","jumpable"],":first-line":["terminal","jumpable"],":where":[],":is":[],":has":[],__default__:["terminal","actionable"]}});function dr(r,{context:e,candidate:t}){let i=e?.tailwindConfig.prefix??"",n=r.map(a=>{let o=(0,st.default)().astSync(a.format);return{...a,ast:a.respectPrefix?ur(i,o):o}}),s=st.default.root({nodes:[st.default.selector({nodes:[st.default.className({value:Te(t)})]})]});for(let{ast:a}of n)[s,a]=w_(s,a),a.walkNesting(o=>o.replaceWith(...s.nodes[0].nodes)),s=a;return s}function _h(r){let e=[];for(;r.prev()&&r.prev().type!=="combinator";)r=r.prev();for(;r&&r.type!=="combinator";)e.push(r),r=r.next();return e}function b_(r){return r.sort((e,t)=>e.type==="tag"&&t.type==="class"?-1:e.type==="class"&&t.type==="tag"?1:e.type==="class"&&t.type==="pseudo"&&t.value.startsWith("::")?-1:e.type==="pseudo"&&e.value.startsWith("::")&&t.type==="class"?1:r.index(e)-r.index(t)),r}function Uo(r,e){let t=!1;r.walk(i=>{if(i.type==="class"&&i.value===e)return t=!0,!1}),t||r.remove()}function rs(r,e,{context:t,candidate:i,base:n}){let s=t?.tailwindConfig?.separator??":";n=n??ve(i,s).pop();let a=(0,st.default)().astSync(r);if(a.walkClasses(f=>{f.raws&&f.value.includes(n)&&(f.raws.value=Te((0,Ch.default)(f.raws.value)))}),a.each(f=>Uo(f,n)),a.length===0)return null;let o=Array.isArray(e)?dr(e,{context:t,candidate:i}):e;if(o===null)return a.toString();let l=st.default.comment({value:"/*__simple__*/"}),c=st.default.comment({value:"/*__simple__*/"});return a.walkClasses(f=>{if(f.value!==n)return;let d=f.parent,p=o.nodes[0].nodes;if(d.nodes.length===1){f.replaceWith(...p);return}let h=_h(f);d.insertBefore(h[0],l),d.insertAfter(h[h.length-1],c);for(let v of p)d.insertBefore(h[0],v.clone());f.remove(),h=_h(l);let b=d.index(l);d.nodes.splice(b,h.length,...b_(st.default.selector({nodes:h})).nodes),l.remove(),c.remove()}),a.walkPseudos(f=>{f.value===zo&&f.replaceWith(f.nodes)}),a.each(f=>pr(f)),a.toString()}function w_(r,e){let t=[];return r.walkPseudos(i=>{i.value===zo&&t.push({pseudo:i,value:i.nodes[0].toString()})}),e.walkPseudos(i=>{if(i.value!==zo)return;let n=i.nodes[0].toString(),s=t.find(c=>c.value===n);if(!s)return;let a=[],o=i.next();for(;o&&o.type!=="combinator";)a.push(o),o=o.next();let l=o;s.pseudo.parent.insertAfter(s.pseudo,st.default.selector({nodes:a.map(c=>c.clone())})),i.remove(),a.forEach(c=>c.remove()),l&&l.type==="combinator"&&l.remove()}),[r,e]}var st,Ch,zo,Vo=P(()=>{u();st=pe(it()),Ch=pe(Pn());fr();Gn();ts();zt();zo=":merge"});function is(r,e){let t=(0,Ho.default)().astSync(r);return t.each(i=>{i.nodes.some(s=>s.type==="combinator")&&(i.nodes=[Ho.default.pseudo({value:":is",nodes:[i.clone()]})]),pr(i)}),`${e} ${t.toString()}`}var Ho,Wo=P(()=>{u();Ho=pe(it());ts()});function Go(r){return v_.transformSync(r)}function*x_(r){let e=1/0;for(;e>=0;){let t,i=!1;if(e===1/0&&r.endsWith("]")){let a=r.indexOf("[");r[a-1]==="-"?t=a-1:r[a-1]==="/"?(t=a-1,i=!0):t=-1}else e===1/0&&r.includes("/")?(t=r.lastIndexOf("/"),i=!0):t=r.lastIndexOf("-",e);if(t<0)break;let n=r.slice(0,t),s=r.slice(i?t:t+1);e=t-1,!(n===""||s==="/")&&(yield[n,s])}}function k_(r,e){if(r.length===0||e.tailwindConfig.prefix==="")return r;for(let t of r){let[i]=t;if(i.options.respectPrefix){let n=ee.root({nodes:[t[1].clone()]}),s=t[1].raws.tailwind.classCandidate;n.walkRules(a=>{let o=s.startsWith("-");a.selector=ur(e.tailwindConfig.prefix,a.selector,o)}),t[1]=n.nodes[0]}}return r}function S_(r,e){if(r.length===0)return r;let t=[];function i(n){return n.parent&&n.parent.type==="atrule"&&n.parent.name==="keyframes"}for(let[n,s]of r){let a=ee.root({nodes:[s.clone()]});a.walkRules(o=>{if(i(o))return;let l=(0,ns.default)().astSync(o.selector);l.each(c=>Uo(c,e)),Qf(l,c=>c===e?`!${c}`:c),o.selector=l.toString(),o.walkDecls(c=>c.important=!0)}),t.push([{...n,important:!0},a.nodes[0]])}return t}function A_(r,e,t){if(e.length===0)return e;let i={modifier:null,value:Ti};{let[n,...s]=ve(r,"/");if(s.length>1&&(n=n+"/"+s.slice(0,-1).join("/"),s=s.slice(-1)),s.length&&!t.variantMap.has(r)&&(r=n,i.modifier=s[0],!we(t.tailwindConfig,"generalizedModifiers")))return[]}if(r.endsWith("]")&&!r.startsWith("[")){let n=/(.)(-?)\[(.*)\]/g.exec(r);if(n){let[,s,a,o]=n;if(s==="@"&&a==="-")return[];if(s!=="@"&&a==="")return[];r=r.replace(`${a}[${o}]`,""),i.value=o}}if(Ko(r)&&!t.variantMap.has(r)){let n=t.offsets.recordVariant(r),s=K(r.slice(1,-1)),a=ve(s,",");if(a.length>1)return[];if(!a.every(ls))return[];let o=a.map((l,c)=>[t.offsets.applyParallelOffset(n,c),Ri(l.trim())]);t.variantMap.set(r,o)}if(t.variantMap.has(r)){let n=Ko(r),s=t.variantOptions.get(r)?.[Pt]??{},a=t.variantMap.get(r).slice(),o=[],l=(()=>!(n||s.respectPrefix===!1))();for(let[c,f]of e){if(c.layer==="user")continue;let d=ee.root({nodes:[f.clone()]});for(let[p,h,b]of a){let w=function(){v.raws.neededBackup||(v.raws.neededBackup=!0,v.walkRules(T=>T.raws.originalSelector=T.selector))},k=function(T){return w(),v.each(B=>{B.type==="rule"&&(B.selectors=B.selectors.map(N=>T({get className(){return Go(N)},selector:N})))}),v},v=(b??d).clone(),y=[],S=h({get container(){return w(),v},separator:t.tailwindConfig.separator,modifySelectors:k,wrap(T){let B=v.nodes;v.removeAll(),T.append(B),v.append(T)},format(T){y.push({format:T,respectPrefix:l})},args:i});if(Array.isArray(S)){for(let[T,B]of S.entries())a.push([t.offsets.applyParallelOffset(p,T),B,v.clone()]);continue}if(typeof S=="string"&&y.push({format:S,respectPrefix:l}),S===null)continue;v.raws.neededBackup&&(delete v.raws.neededBackup,v.walkRules(T=>{let B=T.raws.originalSelector;if(!B||(delete T.raws.originalSelector,B===T.selector))return;let N=T.selector,R=(0,ns.default)(F=>{F.walkClasses(Y=>{Y.value=`${r}${t.tailwindConfig.separator}${Y.value}`})}).processSync(B);y.push({format:N.replace(R,"&"),respectPrefix:l}),T.selector=B})),v.nodes[0].raws.tailwind={...v.nodes[0].raws.tailwind,parentLayer:c.layer};let E=[{...c,sort:t.offsets.applyVariantOffset(c.sort,p,Object.assign(i,t.variantOptions.get(r))),collectedFormats:(c.collectedFormats??[]).concat(y)},v.nodes[0]];o.push(E)}}return o}return[]}function Qo(r,e,t={}){return!ke(r)&&!Array.isArray(r)?[[r],t]:Array.isArray(r)?Qo(r[0],e,r[1]):(e.has(r)||e.set(r,lr(r)),[e.get(r),t])}function __(r){return C_.test(r)}function E_(r){if(!r.includes("://"))return!1;try{let e=new URL(r);return e.scheme!==""&&e.host!==""}catch(e){return!1}}function Eh(r){let e=!0;return r.walkDecls(t=>{if(!Oh(t.prop,t.value))return e=!1,!1}),e}function Oh(r,e){if(E_(`${r}:${e}`))return!1;try{return ee.parse(`a{${r}:${e}}`).toResult(),!0}catch(t){return!1}}function O_(r,e){let[,t,i]=r.match(/^\[([a-zA-Z0-9-_]+):(\S+)\]$/)??[];if(i===void 0||!__(t)||!cr(i))return null;let n=K(i,{property:t});return Oh(t,n)?[[{sort:e.offsets.arbitraryProperty(r),layer:"utilities",options:{respectImportant:!0}},()=>({[$o(r)]:{[t]:n}})]]:null}function*T_(r,e){e.candidateRuleMap.has(r)&&(yield[e.candidateRuleMap.get(r),"DEFAULT"]),yield*function*(o){o!==null&&(yield[o,"DEFAULT"])}(O_(r,e));let t=r,i=!1,n=e.tailwindConfig.prefix,s=n.length,a=t.startsWith(n)||t.startsWith(`-${n}`);t[s]==="-"&&a&&(i=!0,t=n+t.slice(s+1)),i&&e.candidateRuleMap.has(t)&&(yield[e.candidateRuleMap.get(t),"-DEFAULT"]);for(let[o,l]of x_(t))e.candidateRuleMap.has(o)&&(yield[e.candidateRuleMap.get(o),i?`-${l}`:l])}function R_(r,e){return r===gt?[gt]:ve(r,e)}function*P_(r,e){for(let t of r)t[1].raws.tailwind={...t[1].raws.tailwind,classCandidate:e,preserveSource:t[0].options?.preserveSource??!1},yield t}function*Yo(r,e){let t=e.tailwindConfig.separator,[i,...n]=R_(r,t).reverse(),s=!1;i.startsWith("!")&&(s=!0,i=i.slice(1));for(let a of T_(i,e)){let o=[],l=new Map,[c,f]=a,d=c.length===1;for(let[p,h]of c){let b=[];if(typeof h=="function")for(let v of[].concat(h(f,{isOnlyPlugin:d}))){let[y,w]=Qo(v,e.postCssNodeCache);for(let k of y)b.push([{...p,options:{...p.options,...w}},k])}else if(f==="DEFAULT"||f==="-DEFAULT"){let v=h,[y,w]=Qo(v,e.postCssNodeCache);for(let k of y)b.push([{...p,options:{...p.options,...w}},k])}if(b.length>0){let v=Array.from(ta(p.options?.types??[],f,p.options??{},e.tailwindConfig)).map(([y,w])=>w);v.length>0&&l.set(b,v),o.push(b)}}if(Ko(f)){if(o.length>1){let b=function(y){return y.length===1?y[0]:y.find(w=>{let k=l.get(w);return w.some(([{options:S},E])=>Eh(E)?S.types.some(({type:T,preferOnConflict:B})=>k.includes(T)&&B):!1)})},[p,h]=o.reduce((y,w)=>(w.some(([{options:S}])=>S.types.some(({type:E})=>E==="any"))?y[0].push(w):y[1].push(w),y),[[],[]]),v=b(h)??b(p);if(v)o=[v];else{let y=o.map(k=>new Set([...l.get(k)??[]]));for(let k of y)for(let S of k){let E=!1;for(let T of y)k!==T&&T.has(S)&&(T.delete(S),E=!0);E&&k.delete(S)}let w=[];for(let[k,S]of y.entries())for(let E of S){let T=o[k].map(([,B])=>B).flat().map(B=>B.toString().split(`
`).slice(1,-1).map(N=>N.trim()).map(N=>`      ${N}`).join(`
`)).join(`

`);w.push(`  Use \`${r.replace("[",`[${E}:`)}\` for \`${T.trim()}\``);break}G.warn([`The class \`${r}\` is ambiguous and matches multiple utilities.`,...w,`If this is content and not a class, replace it with \`${r.replace("[","&lsqb;").replace("]","&rsqb;")}\` to silence this warning.`]);continue}}o=o.map(p=>p.filter(h=>Eh(h[1])))}o=o.flat(),o=Array.from(P_(o,i)),o=k_(o,e),s&&(o=S_(o,i));for(let p of n)o=A_(p,o,e);for(let p of o)p[1].raws.tailwind={...p[1].raws.tailwind,candidate:r},p=I_(p,{context:e,candidate:r}),p!==null&&(yield p)}}function I_(r,{context:e,candidate:t}){if(!r[0].collectedFormats)return r;let i=!0,n;try{n=dr(r[0].collectedFormats,{context:e,candidate:t})}catch{return null}let s=ee.root({nodes:[r[1].clone()]});return s.walkRules(a=>{if(!ss(a))try{let o=rs(a.selector,n,{candidate:t,context:e});if(o===null){a.remove();return}a.selector=o}catch{return i=!1,!1}}),!i||s.nodes.length===0?null:(r[1]=s.nodes[0],r)}function ss(r){return r.parent&&r.parent.type==="atrule"&&r.parent.name==="keyframes"}function D_(r){if(r===!0)return e=>{ss(e)||e.walkDecls(t=>{t.parent.type==="rule"&&!ss(t.parent)&&(t.important=!0)})};if(typeof r=="string")return e=>{ss(e)||(e.selectors=e.selectors.map(t=>is(t,r)))}}function as(r,e,t=!1){let i=[],n=D_(e.tailwindConfig.important);for(let s of r){if(e.notClassCache.has(s))continue;if(e.candidateRuleCache.has(s)){i=i.concat(Array.from(e.candidateRuleCache.get(s)));continue}let a=Array.from(Yo(s,e));if(a.length===0){e.notClassCache.add(s);continue}e.classCache.set(s,a);let o=e.candidateRuleCache.get(s)??new Set;e.candidateRuleCache.set(s,o);for(let l of a){let[{sort:c,options:f},d]=l;if(f.respectImportant&&n){let h=ee.root({nodes:[d.clone()]});h.walkRules(n),d=h.nodes[0]}let p=[c,t?d.clone():d];o.add(p),e.ruleCache.add(p),i.push(p)}}return i}function Ko(r){return r.startsWith("[")&&r.endsWith("]")}var ns,v_,C_,os=P(()=>{u();Ot();ns=pe(it());qo();Kt();Gn();Fr();Be();It();Vo();Lo();Br();Oi();Bo();zt();ct();Wo();v_=(0,ns.default)(r=>r.first.filter(({type:e})=>e==="class").pop().value);C_=/^[a-z_-]/});var Th,Rh=P(()=>{u();Th={}});function q_(r){try{return Th.createHash("md5").update(r,"utf-8").digest("binary")}catch(e){return""}}function Ph(r,e){let t=e.toString();if(!t.includes("@tailwind"))return!1;let i=No.get(r),n=q_(t),s=i!==n;return No.set(r,n),s}var Ih=P(()=>{u();Rh();It()});function us(r){return(r>0n)-(r<0n)}var Dh=P(()=>{u()});function qh(r,e){let t=0n,i=0n;for(let[n,s]of e)r&n&&(t=t|n,i=i|s);return r&~t|i}var $h=P(()=>{u()});function Lh(r){let e=null;for(let t of r)e=e??t,e=e>t?e:t;return e}function $_(r,e){let t=r.length,i=e.length,n=t<i?t:i;for(let s=0;s<n;s++){let a=r.charCodeAt(s)-e.charCodeAt(s);if(a!==0)return a}return t-i}var Xo,Mh=P(()=>{u();Dh();$h();Xo=class{constructor(){this.offsets={defaults:0n,base:0n,components:0n,utilities:0n,variants:0n,user:0n},this.layerPositions={defaults:0n,base:1n,components:2n,utilities:3n,user:4n,variants:5n},this.reservedVariantBits=0n,this.variantOffsets=new Map}create(e){return{layer:e,parentLayer:e,arbitrary:0n,variants:0n,parallelIndex:0n,index:this.offsets[e]++,propertyOffset:0n,property:"",options:[]}}arbitraryProperty(e){return{...this.create("utilities"),arbitrary:1n,property:e}}forVariant(e,t=0){let i=this.variantOffsets.get(e);if(i===void 0)throw new Error(`Cannot find offset for unknown variant ${e}`);return{...this.create("variants"),variants:i<<BigInt(t)}}applyVariantOffset(e,t,i){return i.variant=t.variants,{...e,layer:"variants",parentLayer:e.layer==="variants"?e.parentLayer:e.layer,variants:e.variants|t.variants,options:i.sort?[].concat(i,e.options):e.options,parallelIndex:Lh([e.parallelIndex,t.parallelIndex])}}applyParallelOffset(e,t){return{...e,parallelIndex:BigInt(t)}}recordVariants(e,t){for(let i of e)this.recordVariant(i,t(i))}recordVariant(e,t=1){return this.variantOffsets.set(e,1n<<this.reservedVariantBits),this.reservedVariantBits+=BigInt(t),{...this.create("variants"),variants:this.variantOffsets.get(e)}}compare(e,t){if(e.layer!==t.layer)return this.layerPositions[e.layer]-this.layerPositions[t.layer];if(e.parentLayer!==t.parentLayer)return this.layerPositions[e.parentLayer]-this.layerPositions[t.parentLayer];for(let i of e.options)for(let n of t.options){if(i.id!==n.id||!i.sort||!n.sort)continue;let s=Lh([i.variant,n.variant])??0n,a=~(s|s-1n),o=e.variants&a,l=t.variants&a;if(o!==l)continue;let c=i.sort({value:i.value,modifier:i.modifier},{value:n.value,modifier:n.modifier});if(c!==0)return c}return e.variants!==t.variants?e.variants-t.variants:e.parallelIndex!==t.parallelIndex?e.parallelIndex-t.parallelIndex:e.arbitrary!==t.arbitrary?e.arbitrary-t.arbitrary:e.propertyOffset!==t.propertyOffset?e.propertyOffset-t.propertyOffset:e.index-t.index}recalculateVariantOffsets(){let e=Array.from(this.variantOffsets.entries()).filter(([n])=>n.startsWith("[")).sort(([n],[s])=>$_(n,s)),t=e.map(([,n])=>n).sort((n,s)=>us(n-s));return e.map(([,n],s)=>[n,t[s]]).filter(([n,s])=>n!==s)}remapArbitraryVariantOffsets(e){let t=this.recalculateVariantOffsets();return t.length===0?e:e.map(i=>{let[n,s]=i;return n={...n,variants:qh(n.variants,t)},[n,s]})}sortArbitraryProperties(e){let t=new Set;for(let[a]of e)a.arbitrary===1n&&t.add(a.property);if(t.size===0)return e;let i=Array.from(t).sort(),n=new Map,s=1n;for(let a of i)n.set(a,s++);return e.map(a=>{let[o,l]=a;return o={...o,propertyOffset:n.get(o.property)??0n},[o,l]})}sort(e){return e=this.remapArbitraryVariantOffsets(e),e=this.sortArbitraryProperties(e),e.sort(([t],[i])=>us(this.compare(t,i)))}}});function tl(r,e){let t=r.tailwindConfig.prefix;return typeof t=="function"?t(e):t+e}function Bh({type:r="any",...e}){let t=[].concat(r);return{...e,types:t.map(i=>Array.isArray(i)?{type:i[0],...i[1]}:{type:i,preferOnConflict:!1})}}function L_(r){let e=[],t="",i=0;for(let n=0;n<r.length;n++){let s=r[n];if(s==="\\")t+="\\"+r[++n];else if(s==="{")++i,e.push(t.trim()),t="";else if(s==="}"){if(--i<0)throw new Error("Your { and } are unbalanced.");e.push(t.trim()),t=""}else t+=s}return t.length>0&&e.push(t.trim()),e=e.filter(n=>n!==""),e}function M_(r,e,{before:t=[]}={}){if(t=[].concat(t),t.length<=0){r.push(e);return}let i=r.length-1;for(let n of t){let s=r.indexOf(n);s!==-1&&(i=Math.min(i,s))}r.splice(i,0,e)}function Fh(r){return Array.isArray(r)?r.flatMap(e=>!Array.isArray(e)&&!ke(e)?e:lr(e)):Fh([r])}function N_(r,e){return(0,Zo.default)(i=>{let n=[];return e&&e(i),i.walkClasses(s=>{n.push(s.value)}),n}).transformSync(r)}function B_(r){r.walkPseudos(e=>{e.value===":not"&&e.remove()})}function F_(r,e={containsNonOnDemandable:!1},t=0){let i=[],n=[];r.type==="rule"?n.push(...r.selectors):r.type==="atrule"&&r.walkRules(s=>n.push(...s.selectors));for(let s of n){let a=N_(s,B_);a.length===0&&(e.containsNonOnDemandable=!0);for(let o of a)i.push(o)}return t===0?[e.containsNonOnDemandable||i.length===0,i]:i}function fs(r){return Fh(r).flatMap(e=>{let t=new Map,[i,n]=F_(e);return i&&n.unshift(gt),n.map(s=>(t.has(e)||t.set(e,e),[s,t.get(e)]))})}function ls(r){return r.startsWith("@")||r.includes("&")}function Ri(r){r=r.replace(/\n+/g,"").replace(/\s{1,}/g," ").trim();let e=L_(r).map(t=>{if(!t.startsWith("@"))return({format:s})=>s(t);let[,i,n]=/@(\S*)( .+|[({].*)?/g.exec(t);return({wrap:s})=>s(ee.atRule({name:i,params:n?.trim()??""}))}).reverse();return t=>{for(let i of e)i(t)}}function j_(r,e,{variantList:t,variantMap:i,offsets:n,classList:s}){function a(p,h){return p?(0,Nh.default)(r,p,h):r}function o(p){return ur(r.prefix,p)}function l(p,h){return p===gt?gt:h.respectPrefix?e.tailwindConfig.prefix+p:p}function c(p,h,b={}){let v=kt(p),y=a(["theme",...v],h);return mt(v[0])(y,b)}let f=0,d={postcss:ee,prefix:o,e:Te,config:a,theme:c,corePlugins:p=>Array.isArray(r.corePlugins)?r.corePlugins.includes(p):a(["corePlugins",p],!0),variants:()=>[],addBase(p){for(let[h,b]of fs(p)){let v=l(h,{}),y=n.create("base");e.candidateRuleMap.has(v)||e.candidateRuleMap.set(v,[]),e.candidateRuleMap.get(v).push([{sort:y,layer:"base"},b])}},addDefaults(p,h){let b={[`@defaults ${p}`]:h};for(let[v,y]of fs(b)){let w=l(v,{});e.candidateRuleMap.has(w)||e.candidateRuleMap.set(w,[]),e.candidateRuleMap.get(w).push([{sort:n.create("defaults"),layer:"defaults"},y])}},addComponents(p,h){h=Object.assign({},{preserveSource:!1,respectPrefix:!0,respectImportant:!1},Array.isArray(h)?{}:h);for(let[v,y]of fs(p)){let w=l(v,h);s.add(w),e.candidateRuleMap.has(w)||e.candidateRuleMap.set(w,[]),e.candidateRuleMap.get(w).push([{sort:n.create("components"),layer:"components",options:h},y])}},addUtilities(p,h){h=Object.assign({},{preserveSource:!1,respectPrefix:!0,respectImportant:!0},Array.isArray(h)?{}:h);for(let[v,y]of fs(p)){let w=l(v,h);s.add(w),e.candidateRuleMap.has(w)||e.candidateRuleMap.set(w,[]),e.candidateRuleMap.get(w).push([{sort:n.create("utilities"),layer:"utilities",options:h},y])}},matchUtilities:function(p,h){h=Bh({...{respectPrefix:!0,respectImportant:!0,modifiers:!1},...h});let v=n.create("utilities");for(let y in p){let S=function(T,{isOnlyPlugin:B}){let[N,R,F]=ea(h.types,T,h,r);if(N===void 0)return[];if(!h.types.some(({type:U})=>U===R))if(B)G.warn([`Unnecessary typehint \`${R}\` in \`${y}-${T}\`.`,`You can safely update it to \`${y}-${T.replace(R+":","")}\`.`]);else return[];if(!cr(N))return[];let Y={get modifier(){return h.modifiers||G.warn(`modifier-used-without-options-for-${y}`,["Your plugin must set `modifiers: true` in its options to support modifiers."]),F}},_=we(r,"generalizedModifiers");return[].concat(_?k(N,Y):k(N)).filter(Boolean).map(U=>({[Qn(y,T)]:U}))},w=l(y,h),k=p[y];s.add([w,h]);let E=[{sort:v,layer:"utilities",options:h},S];e.candidateRuleMap.has(w)||e.candidateRuleMap.set(w,[]),e.candidateRuleMap.get(w).push(E)}},matchComponents:function(p,h){h=Bh({...{respectPrefix:!0,respectImportant:!1,modifiers:!1},...h});let v=n.create("components");for(let y in p){let S=function(T,{isOnlyPlugin:B}){let[N,R,F]=ea(h.types,T,h,r);if(N===void 0)return[];if(!h.types.some(({type:U})=>U===R))if(B)G.warn([`Unnecessary typehint \`${R}\` in \`${y}-${T}\`.`,`You can safely update it to \`${y}-${T.replace(R+":","")}\`.`]);else return[];if(!cr(N))return[];let Y={get modifier(){return h.modifiers||G.warn(`modifier-used-without-options-for-${y}`,["Your plugin must set `modifiers: true` in its options to support modifiers."]),F}},_=we(r,"generalizedModifiers");return[].concat(_?k(N,Y):k(N)).filter(Boolean).map(U=>({[Qn(y,T)]:U}))},w=l(y,h),k=p[y];s.add([w,h]);let E=[{sort:v,layer:"components",options:h},S];e.candidateRuleMap.has(w)||e.candidateRuleMap.set(w,[]),e.candidateRuleMap.get(w).push(E)}},addVariant(p,h,b={}){h=[].concat(h).map(v=>{if(typeof v!="string")return(y={})=>{let{args:w,modifySelectors:k,container:S,separator:E,wrap:T,format:B}=y,N=v(Object.assign({modifySelectors:k,container:S,separator:E},b.type===Jo.MatchVariant&&{args:w,wrap:T,format:B}));if(typeof N=="string"&&!ls(N))throw new Error(`Your custom variant \`${p}\` has an invalid format string. Make sure it's an at-rule or contains a \`&\` placeholder.`);return Array.isArray(N)?N.filter(R=>typeof R=="string").map(R=>Ri(R)):N&&typeof N=="string"&&Ri(N)(y)};if(!ls(v))throw new Error(`Your custom variant \`${p}\` has an invalid format string. Make sure it's an at-rule or contains a \`&\` placeholder.`);return Ri(v)}),M_(t,p,b),i.set(p,h),e.variantOptions.set(p,b)},matchVariant(p,h,b){let v=b?.id??++f,y=p==="@",w=we(r,"generalizedModifiers");for(let[S,E]of Object.entries(b?.values??{}))S!=="DEFAULT"&&d.addVariant(y?`${p}${S}`:`${p}-${S}`,({args:T,container:B})=>h(E,w?{modifier:T?.modifier,container:B}:{container:B}),{...b,value:E,id:v,type:Jo.MatchVariant,variantInfo:el.Base});let k="DEFAULT"in(b?.values??{});d.addVariant(p,({args:S,container:E})=>S?.value===Ti&&!k?null:h(S?.value===Ti?b.values.DEFAULT:S?.value??(typeof S=="string"?S:""),w?{modifier:S?.modifier,container:E}:{container:E}),{...b,id:v,type:Jo.MatchVariant,variantInfo:el.Dynamic})}};return d}function cs(r){return rl.has(r)||rl.set(r,new Map),rl.get(r)}function jh(r,e){let t=!1,i=new Map;for(let n of r){if(!n)continue;let s=oa.parse(n),a=s.hash?s.href.replace(s.hash,""):s.href;a=s.search?a.replace(s.search,""):a;let o=be.statSync(decodeURIComponent(a),{throwIfNoEntry:!1})?.mtimeMs;!o||((!e.has(n)||o>e.get(n))&&(t=!0),i.set(n,o))}return[t,i]}function zh(r){r.walkAtRules(e=>{["responsive","variants"].includes(e.name)&&(zh(e),e.before(e.nodes),e.remove())})}function z_(r){let e=[];return r.each(t=>{t.type==="atrule"&&["responsive","variants"].includes(t.name)&&(t.name="layer",t.params="utilities")}),r.walkAtRules("layer",t=>{if(zh(t),t.params==="base"){for(let i of t.nodes)e.push(function({addBase:n}){n(i,{respectPrefix:!1})});t.remove()}else if(t.params==="components"){for(let i of t.nodes)e.push(function({addComponents:n}){n(i,{respectPrefix:!1,preserveSource:!0})});t.remove()}else if(t.params==="utilities"){for(let i of t.nodes)e.push(function({addUtilities:n}){n(i,{respectPrefix:!1,preserveSource:!0})});t.remove()}}),e}function U_(r,e){let t=Object.entries({...se,...yh}).map(([l,c])=>r.tailwindConfig.corePlugins.includes(l)?c:null).filter(Boolean),i=r.tailwindConfig.plugins.map(l=>(l.__isOptionsFunction&&(l=l()),typeof l=="function"?l:l.handler)),n=z_(e),s=[se.childVariant,se.pseudoElementVariants,se.pseudoClassVariants,se.hasVariants,se.ariaVariants,se.dataVariants],a=[se.supportsVariants,se.reducedMotionVariants,se.prefersContrastVariants,se.screenVariants,se.orientationVariants,se.directionVariants,se.darkVariants,se.forcedColorsVariants,se.printVariant];return(r.tailwindConfig.darkMode==="class"||Array.isArray(r.tailwindConfig.darkMode)&&r.tailwindConfig.darkMode[0]==="class")&&(a=[se.supportsVariants,se.reducedMotionVariants,se.prefersContrastVariants,se.darkVariants,se.screenVariants,se.orientationVariants,se.directionVariants,se.forcedColorsVariants,se.printVariant]),[...t,...s,...i,...a,...n]}function V_(r,e){let t=[],i=new Map;e.variantMap=i;let n=new Xo;e.offsets=n;let s=new Set,a=j_(e.tailwindConfig,e,{variantList:t,variantMap:i,offsets:n,classList:s});for(let f of r)if(Array.isArray(f))for(let d of f)d(a);else f?.(a);n.recordVariants(t,f=>i.get(f).length);for(let[f,d]of i.entries())e.variantMap.set(f,d.map((p,h)=>[n.forVariant(f,h),p]));let o=(e.tailwindConfig.safelist??[]).filter(Boolean);if(o.length>0){let f=[];for(let d of o){if(typeof d=="string"){e.changedContent.push({content:d,extension:"html"});continue}if(d instanceof RegExp){G.warn("root-regex",["Regular expressions in `safelist` work differently in Tailwind CSS v3.0.","Update your `safelist` configuration to eliminate this warning.","https://tailwindcss.com/docs/content-configuration#safelisting-classes"]);continue}f.push(d)}if(f.length>0){let d=new Map,p=e.tailwindConfig.prefix.length,h=f.some(b=>b.pattern.source.includes("!"));for(let b of s){let v=Array.isArray(b)?(()=>{let[y,w]=b,S=Object.keys(w?.values??{}).map(E=>Ei(y,E));return w?.supportsNegativeValues&&(S=[...S,...S.map(E=>"-"+E)],S=[...S,...S.map(E=>E.slice(0,p)+"-"+E.slice(p))]),w.types.some(({type:E})=>E==="color")&&(S=[...S,...S.flatMap(E=>Object.keys(e.tailwindConfig.theme.opacity).map(T=>`${E}/${T}`))]),h&&w?.respectImportant&&(S=[...S,...S.map(E=>"!"+E)]),S})():[b];for(let y of v)for(let{pattern:w,variants:k=[]}of f)if(w.lastIndex=0,d.has(w)||d.set(w,0),!!w.test(y)){d.set(w,d.get(w)+1),e.changedContent.push({content:y,extension:"html"});for(let S of k)e.changedContent.push({content:S+e.tailwindConfig.separator+y,extension:"html"})}}for(let[b,v]of d.entries())v===0&&G.warn([`The safelist pattern \`${b}\` doesn't match any Tailwind CSS classes.`,"Fix this pattern or remove it from your `safelist` configuration.","https://tailwindcss.com/docs/content-configuration#safelisting-classes"])}}let l=[].concat(e.tailwindConfig.darkMode??"media")[1]??"dark",c=[tl(e,l),tl(e,"group"),tl(e,"peer")];e.getClassOrder=function(d){let p=[...d].sort((y,w)=>y===w?0:y<w?-1:1),h=new Map(p.map(y=>[y,null])),b=as(new Set(p),e,!0);b=e.offsets.sort(b);let v=BigInt(c.length);for(let[,y]of b){let w=y.raws.tailwind.candidate;h.set(w,h.get(w)??v++)}return d.map(y=>{let w=h.get(y)??null,k=c.indexOf(y);return w===null&&k!==-1&&(w=BigInt(k)),[y,w]})},e.getClassList=function(d={}){let p=[];for(let h of s)if(Array.isArray(h)){let[b,v]=h,y=[],w=Object.keys(v?.modifiers??{});v?.types?.some(({type:E})=>E==="color")&&w.push(...Object.keys(e.tailwindConfig.theme.opacity??{}));let k={modifiers:w},S=d.includeMetadata&&w.length>0;for(let[E,T]of Object.entries(v?.values??{})){if(T==null)continue;let B=Ei(b,E);if(p.push(S?[B,k]:B),v?.supportsNegativeValues&&xt(T)){let N=Ei(b,`-${E}`);y.push(S?[N,k]:N)}}p.push(...y)}else p.push(h);return p},e.getVariants=function(){let d=Math.random().toString(36).substring(7).toUpperCase(),p=[];for(let[h,b]of e.variantOptions.entries())b.variantInfo!==el.Base&&p.push({name:h,isArbitrary:b.type===Symbol.for("MATCH_VARIANT"),values:Object.keys(b.values??{}),hasDash:h!=="@",selectors({modifier:v,value:y}={}){let w=`TAILWINDPLACEHOLDER${d}`,k=ee.rule({selector:`.${w}`}),S=ee.root({nodes:[k.clone()]}),E=S.toString(),T=(e.variantMap.get(h)??[]).flatMap(([le,A])=>A),B=[];for(let le of T){let A=[],C={args:{modifier:v,value:b.values?.[y]??y},separator:e.tailwindConfig.separator,modifySelectors(V){return S.each(Ee=>{Ee.type==="rule"&&(Ee.selectors=Ee.selectors.map(Ie=>V({get className(){return Go(Ie)},selector:Ie})))}),S},format(V){A.push(V)},wrap(V){A.push(`@${V.name} ${V.params} { & }`)},container:S},he=le(C);if(A.length>0&&B.push(A),Array.isArray(he))for(let V of he)A=[],V(C),B.push(A)}let N=[],R=S.toString();E!==R&&(S.walkRules(le=>{let A=le.selector,C=(0,Zo.default)(he=>{he.walkClasses(V=>{V.value=`${h}${e.tailwindConfig.separator}${V.value}`})}).processSync(A);N.push(A.replace(C,"&").replace(w,"&"))}),S.walkAtRules(le=>{N.push(`@${le.name} (${le.params}) { & }`)}));let F=!(y in(b.values??{})),Y=b[Pt]??{},_=(()=>!(F||Y.respectPrefix===!1))();B=B.map(le=>le.map(A=>({format:A,respectPrefix:_}))),N=N.map(le=>({format:le,respectPrefix:_}));let Q={candidate:w,context:e},U=B.map(le=>rs(`.${w}`,dr(le,Q),Q).replace(`.${w}`,"&").replace("{ & }","").trim());return N.length>0&&U.push(dr(N,Q).toString().replace(`.${w}`,"&")),U}});return p}}function Uh(r,e){!r.classCache.has(e)||(r.notClassCache.add(e),r.classCache.delete(e),r.applyClassCache.delete(e),r.candidateRuleMap.delete(e),r.candidateRuleCache.delete(e),r.stylesheetCache=null)}function H_(r,e){let t=e.raws.tailwind.candidate;if(!!t){for(let i of r.ruleCache)i[1].raws.tailwind.candidate===t&&r.ruleCache.delete(i);Uh(r,t)}}function il(r,e=[],t=ee.root()){let i={disposables:[],ruleCache:new Set,candidateRuleCache:new Map,classCache:new Map,applyClassCache:new Map,notClassCache:new Set(r.blocklist??[]),postCssNodeCache:new Map,candidateRuleMap:new Map,tailwindConfig:r,changedContent:e,variantMap:new Map,stylesheetCache:null,variantOptions:new Map,markInvalidUtilityCandidate:s=>Uh(i,s),markInvalidUtilityNode:s=>H_(i,s)},n=U_(i,t);return V_(n,i),i}function Vh(r,e,t,i,n,s){let a=e.opts.from,o=i!==null;Ze.DEBUG&&console.log("Source path:",a);let l;if(o&&hr.has(a))l=hr.get(a);else if(Pi.has(n)){let p=Pi.get(n);Dt.get(p).add(a),hr.set(a,p),l=p}let c=Ph(a,r);if(l){let[p,h]=jh([...s],cs(l));if(!p&&!c)return[l,!1,h]}if(hr.has(a)){let p=hr.get(a);if(Dt.has(p)&&(Dt.get(p).delete(a),Dt.get(p).size===0)){Dt.delete(p);for(let[h,b]of Pi)b===p&&Pi.delete(h);for(let h of p.disposables.splice(0))h(p)}}Ze.DEBUG&&console.log("Setting up new context...");let f=il(t,[],r);Object.assign(f,{userConfigPath:i});let[,d]=jh([...s],cs(f));return Pi.set(n,f),hr.set(a,f),Dt.has(f)||Dt.set(f,new Set),Dt.get(f).add(a),[f,!0,d]}var Nh,Zo,Pt,Jo,el,rl,hr,Pi,Dt,Oi=P(()=>{u();ft();la();Ot();Nh=pe(Ra()),Zo=pe(it());Ci();qo();Gn();Kt();fr();Lo();Fr();bh();It();It();Yi();Be();Gi();Bo();os();Ih();Mh();ct();Vo();Pt=Symbol(),Jo={AddVariant:Symbol.for("ADD_VARIANT"),MatchVariant:Symbol.for("MATCH_VARIANT")},el={Base:1<<0,Dynamic:1<<1};rl=new WeakMap;hr=wh,Pi=vh,Dt=es});function nl(r){return r.ignore?[]:r.glob?m.env.ROLLUP_WATCH==="true"?[{type:"dependency",file:r.base}]:[{type:"dir-dependency",dir:r.base,glob:r.glob}]:[{type:"dependency",file:r.base}]}var Hh=P(()=>{u()});function Wh(r,e){return{handler:r,config:e}}var Gh,Qh=P(()=>{u();Wh.withOptions=function(r,e=()=>({})){let t=function(i){return{__options:i,handler:r(i),config:e(i)}};return t.__isOptionsFunction=!0,t.__pluginFunction=r,t.__configFunction=e,t};Gh=Wh});var sl={};Ge(sl,{default:()=>W_});var W_,al=P(()=>{u();Qh();W_=Gh});var Kh=x((z4,Yh)=>{u();var G_=(al(),sl).default,Q_={overflow:"hidden",display:"-webkit-box","-webkit-box-orient":"vertical"},Y_=G_(function({matchUtilities:r,addUtilities:e,theme:t,variants:i}){let n=t("lineClamp");r({"line-clamp":s=>({...Q_,"-webkit-line-clamp":`${s}`})},{values:n}),e([{".line-clamp-none":{"-webkit-line-clamp":"unset"}}],i("lineClamp"))},{theme:{lineClamp:{1:"1",2:"2",3:"3",4:"4",5:"5",6:"6"}},variants:{lineClamp:["responsive"]}});Yh.exports=Y_});function ol(r){r.content.files.length===0&&G.warn("content-problems",["The `content` option in your Tailwind CSS configuration is missing or empty.","Configure your content sources or your generated CSS will be missing styles.","https://tailwindcss.com/docs/content-configuration"]);try{let e=Kh();r.plugins.includes(e)&&(G.warn("line-clamp-in-core",["As of Tailwind CSS v3.3, the `@tailwindcss/line-clamp` plugin is now included by default.","Remove it from the `plugins` array in your configuration to eliminate this warning."]),r.plugins=r.plugins.filter(t=>t!==e))}catch{}return r}var Xh=P(()=>{u();Be()});var Zh,Jh=P(()=>{u();Zh=()=>!1});var ps,em=P(()=>{u();ps={sync:r=>[].concat(r),generateTasks:r=>[{dynamic:!1,base:".",negative:[],positive:[].concat(r),patterns:[].concat(r)}],escapePath:r=>r}});var ll,tm=P(()=>{u();ll=r=>r});var rm,im=P(()=>{u();rm=()=>""});function nm(r){let e=r,t=rm(r);return t!=="."&&(e=r.substr(t.length),e.charAt(0)==="/"&&(e=e.substr(1))),e.substr(0,2)==="./"?e=e.substr(2):e.charAt(0)==="/"&&(e=e.substr(1)),{base:t,glob:e}}var sm=P(()=>{u();im()});var ds=x(Ve=>{u();"use strict";Ve.isInteger=r=>typeof r=="number"?Number.isInteger(r):typeof r=="string"&&r.trim()!==""?Number.isInteger(Number(r)):!1;Ve.find=(r,e)=>r.nodes.find(t=>t.type===e);Ve.exceedsLimit=(r,e,t=1,i)=>i===!1||!Ve.isInteger(r)||!Ve.isInteger(e)?!1:(Number(e)-Number(r))/Number(t)>=i;Ve.escapeNode=(r,e=0,t)=>{let i=r.nodes[e];!i||(t&&i.type===t||i.type==="open"||i.type==="close")&&i.escaped!==!0&&(i.value="\\"+i.value,i.escaped=!0)};Ve.encloseBrace=r=>r.type!=="brace"?!1:r.commas>>0+r.ranges>>0==0?(r.invalid=!0,!0):!1;Ve.isInvalidBrace=r=>r.type!=="brace"?!1:r.invalid===!0||r.dollar?!0:r.commas>>0+r.ranges>>0==0||r.open!==!0||r.close!==!0?(r.invalid=!0,!0):!1;Ve.isOpenOrClose=r=>r.type==="open"||r.type==="close"?!0:r.open===!0||r.close===!0;Ve.reduce=r=>r.reduce((e,t)=>(t.type==="text"&&e.push(t.value),t.type==="range"&&(t.type="text"),e),[]);Ve.flatten=(...r)=>{let e=[],t=i=>{for(let n=0;n<i.length;n++){let s=i[n];if(Array.isArray(s)){t(s);continue}s!==void 0&&e.push(s)}return e};return t(r),e}});var hs=x((Z4,om)=>{u();"use strict";var am=ds();om.exports=(r,e={})=>{let t=(i,n={})=>{let s=e.escapeInvalid&&am.isInvalidBrace(n),a=i.invalid===!0&&e.escapeInvalid===!0,o="";if(i.value)return(s||a)&&am.isOpenOrClose(i)?"\\"+i.value:i.value;if(i.value)return i.value;if(i.nodes)for(let l of i.nodes)o+=t(l);return o};return t(r)}});var um=x((J4,lm)=>{u();"use strict";lm.exports=function(r){return typeof r=="number"?r-r==0:typeof r=="string"&&r.trim()!==""?Number.isFinite?Number.isFinite(+r):isFinite(+r):!1}});var bm=x((e6,ym)=>{u();"use strict";var fm=um(),Wt=(r,e,t)=>{if(fm(r)===!1)throw new TypeError("toRegexRange: expected the first argument to be a number");if(e===void 0||r===e)return String(r);if(fm(e)===!1)throw new TypeError("toRegexRange: expected the second argument to be a number.");let i={relaxZeros:!0,...t};typeof i.strictZeros=="boolean"&&(i.relaxZeros=i.strictZeros===!1);let n=String(i.relaxZeros),s=String(i.shorthand),a=String(i.capture),o=String(i.wrap),l=r+":"+e+"="+n+s+a+o;if(Wt.cache.hasOwnProperty(l))return Wt.cache[l].result;let c=Math.min(r,e),f=Math.max(r,e);if(Math.abs(c-f)===1){let v=r+"|"+e;return i.capture?`(${v})`:i.wrap===!1?v:`(?:${v})`}let d=gm(r)||gm(e),p={min:r,max:e,a:c,b:f},h=[],b=[];if(d&&(p.isPadded=d,p.maxLen=String(p.max).length),c<0){let v=f<0?Math.abs(f):1;b=cm(v,Math.abs(c),p,i),c=p.a=0}return f>=0&&(h=cm(c,f,p,i)),p.negatives=b,p.positives=h,p.result=K_(b,h,i),i.capture===!0?p.result=`(${p.result})`:i.wrap!==!1&&h.length+b.length>1&&(p.result=`(?:${p.result})`),Wt.cache[l]=p,p.result};function K_(r,e,t){let i=ul(r,e,"-",!1,t)||[],n=ul(e,r,"",!1,t)||[],s=ul(r,e,"-?",!0,t)||[];return i.concat(s).concat(n).join("|")}function X_(r,e){let t=1,i=1,n=dm(r,t),s=new Set([e]);for(;r<=n&&n<=e;)s.add(n),t+=1,n=dm(r,t);for(n=hm(e+1,i)-1;r<n&&n<=e;)s.add(n),i+=1,n=hm(e+1,i)-1;return s=[...s],s.sort(eE),s}function Z_(r,e,t){if(r===e)return{pattern:r,count:[],digits:0};let i=J_(r,e),n=i.length,s="",a=0;for(let o=0;o<n;o++){let[l,c]=i[o];l===c?s+=l:l!=="0"||c!=="9"?s+=tE(l,c,t):a++}return a&&(s+=t.shorthand===!0?"\\d":"[0-9]"),{pattern:s,count:[a],digits:n}}function cm(r,e,t,i){let n=X_(r,e),s=[],a=r,o;for(let l=0;l<n.length;l++){let c=n[l],f=Z_(String(a),String(c),i),d="";if(!t.isPadded&&o&&o.pattern===f.pattern){o.count.length>1&&o.count.pop(),o.count.push(f.count[0]),o.string=o.pattern+mm(o.count),a=c+1;continue}t.isPadded&&(d=rE(c,t,i)),f.string=d+f.pattern+mm(f.count),s.push(f),a=c+1,o=f}return s}function ul(r,e,t,i,n){let s=[];for(let a of r){let{string:o}=a;!i&&!pm(e,"string",o)&&s.push(t+o),i&&pm(e,"string",o)&&s.push(t+o)}return s}function J_(r,e){let t=[];for(let i=0;i<r.length;i++)t.push([r[i],e[i]]);return t}function eE(r,e){return r>e?1:e>r?-1:0}function pm(r,e,t){return r.some(i=>i[e]===t)}function dm(r,e){return Number(String(r).slice(0,-e)+"9".repeat(e))}function hm(r,e){return r-r%Math.pow(10,e)}function mm(r){let[e=0,t=""]=r;return t||e>1?`{${e+(t?","+t:"")}}`:""}function tE(r,e,t){return`[${r}${e-r==1?"":"-"}${e}]`}function gm(r){return/^-?(0+)\d/.test(r)}function rE(r,e,t){if(!e.isPadded)return r;let i=Math.abs(e.maxLen-String(r).length),n=t.relaxZeros!==!1;switch(i){case 0:return"";case 1:return n?"0?":"0";case 2:return n?"0{0,2}":"00";default:return n?`0{0,${i}}`:`0{${i}}`}}Wt.cache={};Wt.clearCache=()=>Wt.cache={};ym.exports=Wt});var pl=x((t6,Cm)=>{u();"use strict";var iE=(Fn(),Bn),wm=bm(),vm=r=>r!==null&&typeof r=="object"&&!Array.isArray(r),nE=r=>e=>r===!0?Number(e):String(e),fl=r=>typeof r=="number"||typeof r=="string"&&r!=="",Ii=r=>Number.isInteger(+r),cl=r=>{let e=`${r}`,t=-1;if(e[0]==="-"&&(e=e.slice(1)),e==="0")return!1;for(;e[++t]==="0";);return t>0},sE=(r,e,t)=>typeof r=="string"||typeof e=="string"?!0:t.stringify===!0,aE=(r,e,t)=>{if(e>0){let i=r[0]==="-"?"-":"";i&&(r=r.slice(1)),r=i+r.padStart(i?e-1:e,"0")}return t===!1?String(r):r},ms=(r,e)=>{let t=r[0]==="-"?"-":"";for(t&&(r=r.slice(1),e--);r.length<e;)r="0"+r;return t?"-"+r:r},oE=(r,e,t)=>{r.negatives.sort((o,l)=>o<l?-1:o>l?1:0),r.positives.sort((o,l)=>o<l?-1:o>l?1:0);let i=e.capture?"":"?:",n="",s="",a;return r.positives.length&&(n=r.positives.map(o=>ms(String(o),t)).join("|")),r.negatives.length&&(s=`-(${i}${r.negatives.map(o=>ms(String(o),t)).join("|")})`),n&&s?a=`${n}|${s}`:a=n||s,e.wrap?`(${i}${a})`:a},xm=(r,e,t,i)=>{if(t)return wm(r,e,{wrap:!1,...i});let n=String.fromCharCode(r);if(r===e)return n;let s=String.fromCharCode(e);return`[${n}-${s}]`},km=(r,e,t)=>{if(Array.isArray(r)){let i=t.wrap===!0,n=t.capture?"":"?:";return i?`(${n}${r.join("|")})`:r.join("|")}return wm(r,e,t)},Sm=(...r)=>new RangeError("Invalid range arguments: "+iE.inspect(...r)),Am=(r,e,t)=>{if(t.strictRanges===!0)throw Sm([r,e]);return[]},lE=(r,e)=>{if(e.strictRanges===!0)throw new TypeError(`Expected step "${r}" to be a number`);return[]},uE=(r,e,t=1,i={})=>{let n=Number(r),s=Number(e);if(!Number.isInteger(n)||!Number.isInteger(s)){if(i.strictRanges===!0)throw Sm([r,e]);return[]}n===0&&(n=0),s===0&&(s=0);let a=n>s,o=String(r),l=String(e),c=String(t);t=Math.max(Math.abs(t),1);let f=cl(o)||cl(l)||cl(c),d=f?Math.max(o.length,l.length,c.length):0,p=f===!1&&sE(r,e,i)===!1,h=i.transform||nE(p);if(i.toRegex&&t===1)return xm(ms(r,d),ms(e,d),!0,i);let b={negatives:[],positives:[]},v=k=>b[k<0?"negatives":"positives"].push(Math.abs(k)),y=[],w=0;for(;a?n>=s:n<=s;)i.toRegex===!0&&t>1?v(n):y.push(aE(h(n,w),d,p)),n=a?n-t:n+t,w++;return i.toRegex===!0?t>1?oE(b,i,d):km(y,null,{wrap:!1,...i}):y},fE=(r,e,t=1,i={})=>{if(!Ii(r)&&r.length>1||!Ii(e)&&e.length>1)return Am(r,e,i);let n=i.transform||(p=>String.fromCharCode(p)),s=`${r}`.charCodeAt(0),a=`${e}`.charCodeAt(0),o=s>a,l=Math.min(s,a),c=Math.max(s,a);if(i.toRegex&&t===1)return xm(l,c,!1,i);let f=[],d=0;for(;o?s>=a:s<=a;)f.push(n(s,d)),s=o?s-t:s+t,d++;return i.toRegex===!0?km(f,null,{wrap:!1,options:i}):f},gs=(r,e,t,i={})=>{if(e==null&&fl(r))return[r];if(!fl(r)||!fl(e))return Am(r,e,i);if(typeof t=="function")return gs(r,e,1,{transform:t});if(vm(t))return gs(r,e,0,t);let n={...i};return n.capture===!0&&(n.wrap=!0),t=t||n.step||1,Ii(t)?Ii(r)&&Ii(e)?uE(r,e,t,n):fE(r,e,Math.max(Math.abs(t),1),n):t!=null&&!vm(t)?lE(t,n):gs(r,e,1,t)};Cm.exports=gs});var Om=x((r6,Em)=>{u();"use strict";var cE=pl(),_m=ds(),pE=(r,e={})=>{let t=(i,n={})=>{let s=_m.isInvalidBrace(n),a=i.invalid===!0&&e.escapeInvalid===!0,o=s===!0||a===!0,l=e.escapeInvalid===!0?"\\":"",c="";if(i.isOpen===!0)return l+i.value;if(i.isClose===!0)return console.log("node.isClose",l,i.value),l+i.value;if(i.type==="open")return o?l+i.value:"(";if(i.type==="close")return o?l+i.value:")";if(i.type==="comma")return i.prev.type==="comma"?"":o?i.value:"|";if(i.value)return i.value;if(i.nodes&&i.ranges>0){let f=_m.reduce(i.nodes),d=cE(...f,{...e,wrap:!1,toRegex:!0,strictZeros:!0});if(d.length!==0)return f.length>1&&d.length>1?`(${d})`:d}if(i.nodes)for(let f of i.nodes)c+=t(f,i);return c};return t(r)};Em.exports=pE});var Pm=x((i6,Rm)=>{u();"use strict";var dE=pl(),Tm=hs(),mr=ds(),Gt=(r="",e="",t=!1)=>{let i=[];if(r=[].concat(r),e=[].concat(e),!e.length)return r;if(!r.length)return t?mr.flatten(e).map(n=>`{${n}}`):e;for(let n of r)if(Array.isArray(n))for(let s of n)i.push(Gt(s,e,t));else for(let s of e)t===!0&&typeof s=="string"&&(s=`{${s}}`),i.push(Array.isArray(s)?Gt(n,s,t):n+s);return mr.flatten(i)},hE=(r,e={})=>{let t=e.rangeLimit===void 0?1e3:e.rangeLimit,i=(n,s={})=>{n.queue=[];let a=s,o=s.queue;for(;a.type!=="brace"&&a.type!=="root"&&a.parent;)a=a.parent,o=a.queue;if(n.invalid||n.dollar){o.push(Gt(o.pop(),Tm(n,e)));return}if(n.type==="brace"&&n.invalid!==!0&&n.nodes.length===2){o.push(Gt(o.pop(),["{}"]));return}if(n.nodes&&n.ranges>0){let d=mr.reduce(n.nodes);if(mr.exceedsLimit(...d,e.step,t))throw new RangeError("expanded array length exceeds range limit. Use options.rangeLimit to increase or disable the limit.");let p=dE(...d,e);p.length===0&&(p=Tm(n,e)),o.push(Gt(o.pop(),p)),n.nodes=[];return}let l=mr.encloseBrace(n),c=n.queue,f=n;for(;f.type!=="brace"&&f.type!=="root"&&f.parent;)f=f.parent,c=f.queue;for(let d=0;d<n.nodes.length;d++){let p=n.nodes[d];if(p.type==="comma"&&n.type==="brace"){d===1&&c.push(""),c.push("");continue}if(p.type==="close"){o.push(Gt(o.pop(),c,l));continue}if(p.value&&p.type!=="open"){c.push(Gt(c.pop(),p.value));continue}p.nodes&&i(p,n)}return c};return mr.flatten(i(r))};Rm.exports=hE});var Dm=x((n6,Im)=>{u();"use strict";Im.exports={MAX_LENGTH:1e4,CHAR_0:"0",CHAR_9:"9",CHAR_UPPERCASE_A:"A",CHAR_LOWERCASE_A:"a",CHAR_UPPERCASE_Z:"Z",CHAR_LOWERCASE_Z:"z",CHAR_LEFT_PARENTHESES:"(",CHAR_RIGHT_PARENTHESES:")",CHAR_ASTERISK:"*",CHAR_AMPERSAND:"&",CHAR_AT:"@",CHAR_BACKSLASH:"\\",CHAR_BACKTICK:"`",CHAR_CARRIAGE_RETURN:"\r",CHAR_CIRCUMFLEX_ACCENT:"^",CHAR_COLON:":",CHAR_COMMA:",",CHAR_DOLLAR:"$",CHAR_DOT:".",CHAR_DOUBLE_QUOTE:'"',CHAR_EQUAL:"=",CHAR_EXCLAMATION_MARK:"!",CHAR_FORM_FEED:"\f",CHAR_FORWARD_SLASH:"/",CHAR_HASH:"#",CHAR_HYPHEN_MINUS:"-",CHAR_LEFT_ANGLE_BRACKET:"<",CHAR_LEFT_CURLY_BRACE:"{",CHAR_LEFT_SQUARE_BRACKET:"[",CHAR_LINE_FEED:`
`,CHAR_NO_BREAK_SPACE:"\xA0",CHAR_PERCENT:"%",CHAR_PLUS:"+",CHAR_QUESTION_MARK:"?",CHAR_RIGHT_ANGLE_BRACKET:">",CHAR_RIGHT_CURLY_BRACE:"}",CHAR_RIGHT_SQUARE_BRACKET:"]",CHAR_SEMICOLON:";",CHAR_SINGLE_QUOTE:"'",CHAR_SPACE:" ",CHAR_TAB:"	",CHAR_UNDERSCORE:"_",CHAR_VERTICAL_LINE:"|",CHAR_ZERO_WIDTH_NOBREAK_SPACE:"\uFEFF"}});var Nm=x((s6,Mm)=>{u();"use strict";var mE=hs(),{MAX_LENGTH:qm,CHAR_BACKSLASH:dl,CHAR_BACKTICK:gE,CHAR_COMMA:yE,CHAR_DOT:bE,CHAR_LEFT_PARENTHESES:wE,CHAR_RIGHT_PARENTHESES:vE,CHAR_LEFT_CURLY_BRACE:xE,CHAR_RIGHT_CURLY_BRACE:kE,CHAR_LEFT_SQUARE_BRACKET:$m,CHAR_RIGHT_SQUARE_BRACKET:Lm,CHAR_DOUBLE_QUOTE:SE,CHAR_SINGLE_QUOTE:AE,CHAR_NO_BREAK_SPACE:CE,CHAR_ZERO_WIDTH_NOBREAK_SPACE:_E}=Dm(),EE=(r,e={})=>{if(typeof r!="string")throw new TypeError("Expected a string");let t=e||{},i=typeof t.maxLength=="number"?Math.min(qm,t.maxLength):qm;if(r.length>i)throw new SyntaxError(`Input length (${r.length}), exceeds max characters (${i})`);let n={type:"root",input:r,nodes:[]},s=[n],a=n,o=n,l=0,c=r.length,f=0,d=0,p,h=()=>r[f++],b=v=>{if(v.type==="text"&&o.type==="dot"&&(o.type="text"),o&&o.type==="text"&&v.type==="text"){o.value+=v.value;return}return a.nodes.push(v),v.parent=a,v.prev=o,o=v,v};for(b({type:"bos"});f<c;)if(a=s[s.length-1],p=h(),!(p===_E||p===CE)){if(p===dl){b({type:"text",value:(e.keepEscaping?p:"")+h()});continue}if(p===Lm){b({type:"text",value:"\\"+p});continue}if(p===$m){l++;let v;for(;f<c&&(v=h());){if(p+=v,v===$m){l++;continue}if(v===dl){p+=h();continue}if(v===Lm&&(l--,l===0))break}b({type:"text",value:p});continue}if(p===wE){a=b({type:"paren",nodes:[]}),s.push(a),b({type:"text",value:p});continue}if(p===vE){if(a.type!=="paren"){b({type:"text",value:p});continue}a=s.pop(),b({type:"text",value:p}),a=s[s.length-1];continue}if(p===SE||p===AE||p===gE){let v=p,y;for(e.keepQuotes!==!0&&(p="");f<c&&(y=h());){if(y===dl){p+=y+h();continue}if(y===v){e.keepQuotes===!0&&(p+=y);break}p+=y}b({type:"text",value:p});continue}if(p===xE){d++;let v=o.value&&o.value.slice(-1)==="$"||a.dollar===!0;a=b({type:"brace",open:!0,close:!1,dollar:v,depth:d,commas:0,ranges:0,nodes:[]}),s.push(a),b({type:"open",value:p});continue}if(p===kE){if(a.type!=="brace"){b({type:"text",value:p});continue}let v="close";a=s.pop(),a.close=!0,b({type:v,value:p}),d--,a=s[s.length-1];continue}if(p===yE&&d>0){if(a.ranges>0){a.ranges=0;let v=a.nodes.shift();a.nodes=[v,{type:"text",value:mE(a)}]}b({type:"comma",value:p}),a.commas++;continue}if(p===bE&&d>0&&a.commas===0){let v=a.nodes;if(d===0||v.length===0){b({type:"text",value:p});continue}if(o.type==="dot"){if(a.range=[],o.value+=p,o.type="range",a.nodes.length!==3&&a.nodes.length!==5){a.invalid=!0,a.ranges=0,o.type="text";continue}a.ranges++,a.args=[];continue}if(o.type==="range"){v.pop();let y=v[v.length-1];y.value+=o.value+p,o=y,a.ranges--;continue}b({type:"dot",value:p});continue}b({type:"text",value:p})}do if(a=s.pop(),a.type!=="root"){a.nodes.forEach(w=>{w.nodes||(w.type==="open"&&(w.isOpen=!0),w.type==="close"&&(w.isClose=!0),w.nodes||(w.type="text"),w.invalid=!0)});let v=s[s.length-1],y=v.nodes.indexOf(a);v.nodes.splice(y,1,...a.nodes)}while(s.length>0);return b({type:"eos"}),n};Mm.exports=EE});var jm=x((a6,Fm)=>{u();"use strict";var Bm=hs(),OE=Om(),TE=Pm(),RE=Nm(),Le=(r,e={})=>{let t=[];if(Array.isArray(r))for(let i of r){let n=Le.create(i,e);Array.isArray(n)?t.push(...n):t.push(n)}else t=[].concat(Le.create(r,e));return e&&e.expand===!0&&e.nodupes===!0&&(t=[...new Set(t)]),t};Le.parse=(r,e={})=>RE(r,e);Le.stringify=(r,e={})=>typeof r=="string"?Bm(Le.parse(r,e),e):Bm(r,e);Le.compile=(r,e={})=>(typeof r=="string"&&(r=Le.parse(r,e)),OE(r,e));Le.expand=(r,e={})=>{typeof r=="string"&&(r=Le.parse(r,e));let t=TE(r,e);return e.noempty===!0&&(t=t.filter(Boolean)),e.nodupes===!0&&(t=[...new Set(t)]),t};Le.create=(r,e={})=>r===""||r.length<3?[r]:e.expand!==!0?Le.compile(r,e):Le.expand(r,e);Fm.exports=Le});var Di=x((o6,Wm)=>{u();"use strict";var PE=(et(),Ur),at="\\\\/",zm=`[^${at}]`,yt="\\.",IE="\\+",DE="\\?",ys="\\/",qE="(?=.)",Um="[^/]",hl=`(?:${ys}|$)`,Vm=`(?:^|${ys})`,ml=`${yt}{1,2}${hl}`,$E=`(?!${yt})`,LE=`(?!${Vm}${ml})`,ME=`(?!${yt}{0,1}${hl})`,NE=`(?!${ml})`,BE=`[^.${ys}]`,FE=`${Um}*?`,Hm={DOT_LITERAL:yt,PLUS_LITERAL:IE,QMARK_LITERAL:DE,SLASH_LITERAL:ys,ONE_CHAR:qE,QMARK:Um,END_ANCHOR:hl,DOTS_SLASH:ml,NO_DOT:$E,NO_DOTS:LE,NO_DOT_SLASH:ME,NO_DOTS_SLASH:NE,QMARK_NO_DOT:BE,STAR:FE,START_ANCHOR:Vm},jE={...Hm,SLASH_LITERAL:`[${at}]`,QMARK:zm,STAR:`${zm}*?`,DOTS_SLASH:`${yt}{1,2}(?:[${at}]|$)`,NO_DOT:`(?!${yt})`,NO_DOTS:`(?!(?:^|[${at}])${yt}{1,2}(?:[${at}]|$))`,NO_DOT_SLASH:`(?!${yt}{0,1}(?:[${at}]|$))`,NO_DOTS_SLASH:`(?!${yt}{1,2}(?:[${at}]|$))`,QMARK_NO_DOT:`[^.${at}]`,START_ANCHOR:`(?:^|[${at}])`,END_ANCHOR:`(?:[${at}]|$)`},zE={alnum:"a-zA-Z0-9",alpha:"a-zA-Z",ascii:"\\x00-\\x7F",blank:" \\t",cntrl:"\\x00-\\x1F\\x7F",digit:"0-9",graph:"\\x21-\\x7E",lower:"a-z",print:"\\x20-\\x7E ",punct:"\\-!\"#$%&'()\\*+,./:;<=>?@[\\]^_`{|}~",space:" \\t\\r\\n\\v\\f",upper:"A-Z",word:"A-Za-z0-9_",xdigit:"A-Fa-f0-9"};Wm.exports={MAX_LENGTH:1024*64,POSIX_REGEX_SOURCE:zE,REGEX_BACKSLASH:/\\(?![*+?^${}(|)[\]])/g,REGEX_NON_SPECIAL_CHARS:/^[^@![\].,$*+?^{}()|\\/]+/,REGEX_SPECIAL_CHARS:/[-*+?.^${}(|)[\]]/,REGEX_SPECIAL_CHARS_BACKREF:/(\\?)((\W)(\3*))/g,REGEX_SPECIAL_CHARS_GLOBAL:/([-*+?.^${}(|)[\]])/g,REGEX_REMOVE_BACKSLASH:/(?:\[.*?[^\\]\]|\\(?=.))/g,REPLACEMENTS:{"***":"*","**/**":"**","**/**/**":"**"},CHAR_0:48,CHAR_9:57,CHAR_UPPERCASE_A:65,CHAR_LOWERCASE_A:97,CHAR_UPPERCASE_Z:90,CHAR_LOWERCASE_Z:122,CHAR_LEFT_PARENTHESES:40,CHAR_RIGHT_PARENTHESES:41,CHAR_ASTERISK:42,CHAR_AMPERSAND:38,CHAR_AT:64,CHAR_BACKWARD_SLASH:92,CHAR_CARRIAGE_RETURN:13,CHAR_CIRCUMFLEX_ACCENT:94,CHAR_COLON:58,CHAR_COMMA:44,CHAR_DOT:46,CHAR_DOUBLE_QUOTE:34,CHAR_EQUAL:61,CHAR_EXCLAMATION_MARK:33,CHAR_FORM_FEED:12,CHAR_FORWARD_SLASH:47,CHAR_GRAVE_ACCENT:96,CHAR_HASH:35,CHAR_HYPHEN_MINUS:45,CHAR_LEFT_ANGLE_BRACKET:60,CHAR_LEFT_CURLY_BRACE:123,CHAR_LEFT_SQUARE_BRACKET:91,CHAR_LINE_FEED:10,CHAR_NO_BREAK_SPACE:160,CHAR_PERCENT:37,CHAR_PLUS:43,CHAR_QUESTION_MARK:63,CHAR_RIGHT_ANGLE_BRACKET:62,CHAR_RIGHT_CURLY_BRACE:125,CHAR_RIGHT_SQUARE_BRACKET:93,CHAR_SEMICOLON:59,CHAR_SINGLE_QUOTE:39,CHAR_SPACE:32,CHAR_TAB:9,CHAR_UNDERSCORE:95,CHAR_VERTICAL_LINE:124,CHAR_ZERO_WIDTH_NOBREAK_SPACE:65279,SEP:PE.sep,extglobChars(r){return{"!":{type:"negate",open:"(?:(?!(?:",close:`))${r.STAR})`},"?":{type:"qmark",open:"(?:",close:")?"},"+":{type:"plus",open:"(?:",close:")+"},"*":{type:"star",open:"(?:",close:")*"},"@":{type:"at",open:"(?:",close:")"}}},globChars(r){return r===!0?jE:Hm}}});var qi=x(Re=>{u();"use strict";var UE=(et(),Ur),VE=m.platform==="win32",{REGEX_BACKSLASH:HE,REGEX_REMOVE_BACKSLASH:WE,REGEX_SPECIAL_CHARS:GE,REGEX_SPECIAL_CHARS_GLOBAL:QE}=Di();Re.isObject=r=>r!==null&&typeof r=="object"&&!Array.isArray(r);Re.hasRegexChars=r=>GE.test(r);Re.isRegexChar=r=>r.length===1&&Re.hasRegexChars(r);Re.escapeRegex=r=>r.replace(QE,"\\$1");Re.toPosixSlashes=r=>r.replace(HE,"/");Re.removeBackslashes=r=>r.replace(WE,e=>e==="\\"?"":e);Re.supportsLookbehinds=()=>{let r=m.version.slice(1).split(".").map(Number);return r.length===3&&r[0]>=9||r[0]===8&&r[1]>=10};Re.isWindows=r=>r&&typeof r.windows=="boolean"?r.windows:VE===!0||UE.sep==="\\";Re.escapeLast=(r,e,t)=>{let i=r.lastIndexOf(e,t);return i===-1?r:r[i-1]==="\\"?Re.escapeLast(r,e,i-1):`${r.slice(0,i)}\\${r.slice(i)}`};Re.removePrefix=(r,e={})=>{let t=r;return t.startsWith("./")&&(t=t.slice(2),e.prefix="./"),t};Re.wrapOutput=(r,e={},t={})=>{let i=t.contains?"":"^",n=t.contains?"":"$",s=`${i}(?:${r})${n}`;return e.negated===!0&&(s=`(?:^(?!${s}).*$)`),s}});var eg=x((u6,Jm)=>{u();"use strict";var Gm=qi(),{CHAR_ASTERISK:gl,CHAR_AT:YE,CHAR_BACKWARD_SLASH:$i,CHAR_COMMA:KE,CHAR_DOT:yl,CHAR_EXCLAMATION_MARK:bl,CHAR_FORWARD_SLASH:Qm,CHAR_LEFT_CURLY_BRACE:wl,CHAR_LEFT_PARENTHESES:vl,CHAR_LEFT_SQUARE_BRACKET:XE,CHAR_PLUS:ZE,CHAR_QUESTION_MARK:Ym,CHAR_RIGHT_CURLY_BRACE:JE,CHAR_RIGHT_PARENTHESES:Km,CHAR_RIGHT_SQUARE_BRACKET:e2}=Di(),Xm=r=>r===Qm||r===$i,Zm=r=>{r.isPrefix!==!0&&(r.depth=r.isGlobstar?1/0:1)},t2=(r,e)=>{let t=e||{},i=r.length-1,n=t.parts===!0||t.scanToEnd===!0,s=[],a=[],o=[],l=r,c=-1,f=0,d=0,p=!1,h=!1,b=!1,v=!1,y=!1,w=!1,k=!1,S=!1,E=!1,T=!1,B=0,N,R,F={value:"",depth:0,isGlob:!1},Y=()=>c>=i,_=()=>l.charCodeAt(c+1),Q=()=>(N=R,l.charCodeAt(++c));for(;c<i;){R=Q();let he;if(R===$i){k=F.backslashes=!0,R=Q(),R===wl&&(w=!0);continue}if(w===!0||R===wl){for(B++;Y()!==!0&&(R=Q());){if(R===$i){k=F.backslashes=!0,Q();continue}if(R===wl){B++;continue}if(w!==!0&&R===yl&&(R=Q())===yl){if(p=F.isBrace=!0,b=F.isGlob=!0,T=!0,n===!0)continue;break}if(w!==!0&&R===KE){if(p=F.isBrace=!0,b=F.isGlob=!0,T=!0,n===!0)continue;break}if(R===JE&&(B--,B===0)){w=!1,p=F.isBrace=!0,T=!0;break}}if(n===!0)continue;break}if(R===Qm){if(s.push(c),a.push(F),F={value:"",depth:0,isGlob:!1},T===!0)continue;if(N===yl&&c===f+1){f+=2;continue}d=c+1;continue}if(t.noext!==!0&&(R===ZE||R===YE||R===gl||R===Ym||R===bl)===!0&&_()===vl){if(b=F.isGlob=!0,v=F.isExtglob=!0,T=!0,R===bl&&c===f&&(E=!0),n===!0){for(;Y()!==!0&&(R=Q());){if(R===$i){k=F.backslashes=!0,R=Q();continue}if(R===Km){b=F.isGlob=!0,T=!0;break}}continue}break}if(R===gl){if(N===gl&&(y=F.isGlobstar=!0),b=F.isGlob=!0,T=!0,n===!0)continue;break}if(R===Ym){if(b=F.isGlob=!0,T=!0,n===!0)continue;break}if(R===XE){for(;Y()!==!0&&(he=Q());){if(he===$i){k=F.backslashes=!0,Q();continue}if(he===e2){h=F.isBracket=!0,b=F.isGlob=!0,T=!0;break}}if(n===!0)continue;break}if(t.nonegate!==!0&&R===bl&&c===f){S=F.negated=!0,f++;continue}if(t.noparen!==!0&&R===vl){if(b=F.isGlob=!0,n===!0){for(;Y()!==!0&&(R=Q());){if(R===vl){k=F.backslashes=!0,R=Q();continue}if(R===Km){T=!0;break}}continue}break}if(b===!0){if(T=!0,n===!0)continue;break}}t.noext===!0&&(v=!1,b=!1);let U=l,le="",A="";f>0&&(le=l.slice(0,f),l=l.slice(f),d-=f),U&&b===!0&&d>0?(U=l.slice(0,d),A=l.slice(d)):b===!0?(U="",A=l):U=l,U&&U!==""&&U!=="/"&&U!==l&&Xm(U.charCodeAt(U.length-1))&&(U=U.slice(0,-1)),t.unescape===!0&&(A&&(A=Gm.removeBackslashes(A)),U&&k===!0&&(U=Gm.removeBackslashes(U)));let C={prefix:le,input:r,start:f,base:U,glob:A,isBrace:p,isBracket:h,isGlob:b,isExtglob:v,isGlobstar:y,negated:S,negatedExtglob:E};if(t.tokens===!0&&(C.maxDepth=0,Xm(R)||a.push(F),C.tokens=a),t.parts===!0||t.tokens===!0){let he;for(let V=0;V<s.length;V++){let Ee=he?he+1:f,Ie=s[V],De=r.slice(Ee,Ie);t.tokens&&(V===0&&f!==0?(a[V].isPrefix=!0,a[V].value=le):a[V].value=De,Zm(a[V]),C.maxDepth+=a[V].depth),(V!==0||De!=="")&&o.push(De),he=Ie}if(he&&he+1<r.length){let V=r.slice(he+1);o.push(V),t.tokens&&(a[a.length-1].value=V,Zm(a[a.length-1]),C.maxDepth+=a[a.length-1].depth)}C.slashes=s,C.parts=o}return C};Jm.exports=t2});var ig=x((f6,rg)=>{u();"use strict";var bs=Di(),Me=qi(),{MAX_LENGTH:ws,POSIX_REGEX_SOURCE:r2,REGEX_NON_SPECIAL_CHARS:i2,REGEX_SPECIAL_CHARS_BACKREF:n2,REPLACEMENTS:tg}=bs,s2=(r,e)=>{if(typeof e.expandRange=="function")return e.expandRange(...r,e);r.sort();let t=`[${r.join("-")}]`;try{new RegExp(t)}catch(i){return r.map(n=>Me.escapeRegex(n)).join("..")}return t},gr=(r,e)=>`Missing ${r}: "${e}" - use "\\\\${e}" to match literal characters`,xl=(r,e)=>{if(typeof r!="string")throw new TypeError("Expected a string");r=tg[r]||r;let t={...e},i=typeof t.maxLength=="number"?Math.min(ws,t.maxLength):ws,n=r.length;if(n>i)throw new SyntaxError(`Input length: ${n}, exceeds maximum allowed length: ${i}`);let s={type:"bos",value:"",output:t.prepend||""},a=[s],o=t.capture?"":"?:",l=Me.isWindows(e),c=bs.globChars(l),f=bs.extglobChars(c),{DOT_LITERAL:d,PLUS_LITERAL:p,SLASH_LITERAL:h,ONE_CHAR:b,DOTS_SLASH:v,NO_DOT:y,NO_DOT_SLASH:w,NO_DOTS_SLASH:k,QMARK:S,QMARK_NO_DOT:E,STAR:T,START_ANCHOR:B}=c,N=$=>`(${o}(?:(?!${B}${$.dot?v:d}).)*?)`,R=t.dot?"":y,F=t.dot?S:E,Y=t.bash===!0?N(t):T;t.capture&&(Y=`(${Y})`),typeof t.noext=="boolean"&&(t.noextglob=t.noext);let _={input:r,index:-1,start:0,dot:t.dot===!0,consumed:"",output:"",prefix:"",backtrack:!1,negated:!1,brackets:0,braces:0,parens:0,quotes:0,globstar:!1,tokens:a};r=Me.removePrefix(r,_),n=r.length;let Q=[],U=[],le=[],A=s,C,he=()=>_.index===n-1,V=_.peek=($=1)=>r[_.index+$],Ee=_.advance=()=>r[++_.index]||"",Ie=()=>r.slice(_.index+1),De=($="",ae=0)=>{_.consumed+=$,_.index+=ae},ji=$=>{_.output+=$.output!=null?$.output:$.value,De($.value)},Iv=()=>{let $=1;for(;V()==="!"&&(V(2)!=="("||V(3)==="?");)Ee(),_.start++,$++;return $%2==0?!1:(_.negated=!0,_.start++,!0)},zi=$=>{_[$]++,le.push($)},Ft=$=>{_[$]--,le.pop()},W=$=>{if(A.type==="globstar"){let ae=_.braces>0&&($.type==="comma"||$.type==="brace"),I=$.extglob===!0||Q.length&&($.type==="pipe"||$.type==="paren");$.type!=="slash"&&$.type!=="paren"&&!ae&&!I&&(_.output=_.output.slice(0,-A.output.length),A.type="star",A.value="*",A.output=Y,_.output+=A.output)}if(Q.length&&$.type!=="paren"&&(Q[Q.length-1].inner+=$.value),($.value||$.output)&&ji($),A&&A.type==="text"&&$.type==="text"){A.value+=$.value,A.output=(A.output||"")+$.value;return}$.prev=A,a.push($),A=$},Ui=($,ae)=>{let I={...f[ae],conditions:1,inner:""};I.prev=A,I.parens=_.parens,I.output=_.output;let H=(t.capture?"(":"")+I.open;zi("parens"),W({type:$,value:ae,output:_.output?"":b}),W({type:"paren",extglob:!0,value:Ee(),output:H}),Q.push(I)},Dv=$=>{let ae=$.close+(t.capture?")":""),I;if($.type==="negate"){let H=Y;if($.inner&&$.inner.length>1&&$.inner.includes("/")&&(H=N(t)),(H!==Y||he()||/^\)+$/.test(Ie()))&&(ae=$.close=`)$))${H}`),$.inner.includes("*")&&(I=Ie())&&/^\.[^\\/.]+$/.test(I)){let ce=xl(I,{...e,fastpaths:!1}).output;ae=$.close=`)${ce})${H})`}$.prev.type==="bos"&&(_.negatedExtglob=!0)}W({type:"paren",extglob:!0,value:C,output:ae}),Ft("parens")};if(t.fastpaths!==!1&&!/(^[*!]|[/()[\]{}"])/.test(r)){let $=!1,ae=r.replace(n2,(I,H,ce,Ce,ye,Bs)=>Ce==="\\"?($=!0,I):Ce==="?"?H?H+Ce+(ye?S.repeat(ye.length):""):Bs===0?F+(ye?S.repeat(ye.length):""):S.repeat(ce.length):Ce==="."?d.repeat(ce.length):Ce==="*"?H?H+Ce+(ye?Y:""):Y:H?I:`\\${I}`);return $===!0&&(t.unescape===!0?ae=ae.replace(/\\/g,""):ae=ae.replace(/\\+/g,I=>I.length%2==0?"\\\\":I?"\\":"")),ae===r&&t.contains===!0?(_.output=r,_):(_.output=Me.wrapOutput(ae,_,e),_)}for(;!he();){if(C=Ee(),C==="\0")continue;if(C==="\\"){let I=V();if(I==="/"&&t.bash!==!0||I==="."||I===";")continue;if(!I){C+="\\",W({type:"text",value:C});continue}let H=/^\\+/.exec(Ie()),ce=0;if(H&&H[0].length>2&&(ce=H[0].length,_.index+=ce,ce%2!=0&&(C+="\\")),t.unescape===!0?C=Ee():C+=Ee(),_.brackets===0){W({type:"text",value:C});continue}}if(_.brackets>0&&(C!=="]"||A.value==="["||A.value==="[^")){if(t.posix!==!1&&C===":"){let I=A.value.slice(1);if(I.includes("[")&&(A.posix=!0,I.includes(":"))){let H=A.value.lastIndexOf("["),ce=A.value.slice(0,H),Ce=A.value.slice(H+2),ye=r2[Ce];if(ye){A.value=ce+ye,_.backtrack=!0,Ee(),!s.output&&a.indexOf(A)===1&&(s.output=b);continue}}}(C==="["&&V()!==":"||C==="-"&&V()==="]")&&(C=`\\${C}`),C==="]"&&(A.value==="["||A.value==="[^")&&(C=`\\${C}`),t.posix===!0&&C==="!"&&A.value==="["&&(C="^"),A.value+=C,ji({value:C});continue}if(_.quotes===1&&C!=='"'){C=Me.escapeRegex(C),A.value+=C,ji({value:C});continue}if(C==='"'){_.quotes=_.quotes===1?0:1,t.keepQuotes===!0&&W({type:"text",value:C});continue}if(C==="("){zi("parens"),W({type:"paren",value:C});continue}if(C===")"){if(_.parens===0&&t.strictBrackets===!0)throw new SyntaxError(gr("opening","("));let I=Q[Q.length-1];if(I&&_.parens===I.parens+1){Dv(Q.pop());continue}W({type:"paren",value:C,output:_.parens?")":"\\)"}),Ft("parens");continue}if(C==="["){if(t.nobracket===!0||!Ie().includes("]")){if(t.nobracket!==!0&&t.strictBrackets===!0)throw new SyntaxError(gr("closing","]"));C=`\\${C}`}else zi("brackets");W({type:"bracket",value:C});continue}if(C==="]"){if(t.nobracket===!0||A&&A.type==="bracket"&&A.value.length===1){W({type:"text",value:C,output:`\\${C}`});continue}if(_.brackets===0){if(t.strictBrackets===!0)throw new SyntaxError(gr("opening","["));W({type:"text",value:C,output:`\\${C}`});continue}Ft("brackets");let I=A.value.slice(1);if(A.posix!==!0&&I[0]==="^"&&!I.includes("/")&&(C=`/${C}`),A.value+=C,ji({value:C}),t.literalBrackets===!1||Me.hasRegexChars(I))continue;let H=Me.escapeRegex(A.value);if(_.output=_.output.slice(0,-A.value.length),t.literalBrackets===!0){_.output+=H,A.value=H;continue}A.value=`(${o}${H}|${A.value})`,_.output+=A.value;continue}if(C==="{"&&t.nobrace!==!0){zi("braces");let I={type:"brace",value:C,output:"(",outputIndex:_.output.length,tokensIndex:_.tokens.length};U.push(I),W(I);continue}if(C==="}"){let I=U[U.length-1];if(t.nobrace===!0||!I){W({type:"text",value:C,output:C});continue}let H=")";if(I.dots===!0){let ce=a.slice(),Ce=[];for(let ye=ce.length-1;ye>=0&&(a.pop(),ce[ye].type!=="brace");ye--)ce[ye].type!=="dots"&&Ce.unshift(ce[ye].value);H=s2(Ce,t),_.backtrack=!0}if(I.comma!==!0&&I.dots!==!0){let ce=_.output.slice(0,I.outputIndex),Ce=_.tokens.slice(I.tokensIndex);I.value=I.output="\\{",C=H="\\}",_.output=ce;for(let ye of Ce)_.output+=ye.output||ye.value}W({type:"brace",value:C,output:H}),Ft("braces"),U.pop();continue}if(C==="|"){Q.length>0&&Q[Q.length-1].conditions++,W({type:"text",value:C});continue}if(C===","){let I=C,H=U[U.length-1];H&&le[le.length-1]==="braces"&&(H.comma=!0,I="|"),W({type:"comma",value:C,output:I});continue}if(C==="/"){if(A.type==="dot"&&_.index===_.start+1){_.start=_.index+1,_.consumed="",_.output="",a.pop(),A=s;continue}W({type:"slash",value:C,output:h});continue}if(C==="."){if(_.braces>0&&A.type==="dot"){A.value==="."&&(A.output=d);let I=U[U.length-1];A.type="dots",A.output+=C,A.value+=C,I.dots=!0;continue}if(_.braces+_.parens===0&&A.type!=="bos"&&A.type!=="slash"){W({type:"text",value:C,output:d});continue}W({type:"dot",value:C,output:d});continue}if(C==="?"){if(!(A&&A.value==="(")&&t.noextglob!==!0&&V()==="("&&V(2)!=="?"){Ui("qmark",C);continue}if(A&&A.type==="paren"){let H=V(),ce=C;if(H==="<"&&!Me.supportsLookbehinds())throw new Error("Node.js v10 or higher is required for regex lookbehinds");(A.value==="("&&!/[!=<:]/.test(H)||H==="<"&&!/<([!=]|\w+>)/.test(Ie()))&&(ce=`\\${C}`),W({type:"text",value:C,output:ce});continue}if(t.dot!==!0&&(A.type==="slash"||A.type==="bos")){W({type:"qmark",value:C,output:E});continue}W({type:"qmark",value:C,output:S});continue}if(C==="!"){if(t.noextglob!==!0&&V()==="("&&(V(2)!=="?"||!/[!=<:]/.test(V(3)))){Ui("negate",C);continue}if(t.nonegate!==!0&&_.index===0){Iv();continue}}if(C==="+"){if(t.noextglob!==!0&&V()==="("&&V(2)!=="?"){Ui("plus",C);continue}if(A&&A.value==="("||t.regex===!1){W({type:"plus",value:C,output:p});continue}if(A&&(A.type==="bracket"||A.type==="paren"||A.type==="brace")||_.parens>0){W({type:"plus",value:C});continue}W({type:"plus",value:p});continue}if(C==="@"){if(t.noextglob!==!0&&V()==="("&&V(2)!=="?"){W({type:"at",extglob:!0,value:C,output:""});continue}W({type:"text",value:C});continue}if(C!=="*"){(C==="$"||C==="^")&&(C=`\\${C}`);let I=i2.exec(Ie());I&&(C+=I[0],_.index+=I[0].length),W({type:"text",value:C});continue}if(A&&(A.type==="globstar"||A.star===!0)){A.type="star",A.star=!0,A.value+=C,A.output=Y,_.backtrack=!0,_.globstar=!0,De(C);continue}let $=Ie();if(t.noextglob!==!0&&/^\([^?]/.test($)){Ui("star",C);continue}if(A.type==="star"){if(t.noglobstar===!0){De(C);continue}let I=A.prev,H=I.prev,ce=I.type==="slash"||I.type==="bos",Ce=H&&(H.type==="star"||H.type==="globstar");if(t.bash===!0&&(!ce||$[0]&&$[0]!=="/")){W({type:"star",value:C,output:""});continue}let ye=_.braces>0&&(I.type==="comma"||I.type==="brace"),Bs=Q.length&&(I.type==="pipe"||I.type==="paren");if(!ce&&I.type!=="paren"&&!ye&&!Bs){W({type:"star",value:C,output:""});continue}for(;$.slice(0,3)==="/**";){let Vi=r[_.index+4];if(Vi&&Vi!=="/")break;$=$.slice(3),De("/**",3)}if(I.type==="bos"&&he()){A.type="globstar",A.value+=C,A.output=N(t),_.output=A.output,_.globstar=!0,De(C);continue}if(I.type==="slash"&&I.prev.type!=="bos"&&!Ce&&he()){_.output=_.output.slice(0,-(I.output+A.output).length),I.output=`(?:${I.output}`,A.type="globstar",A.output=N(t)+(t.strictSlashes?")":"|$)"),A.value+=C,_.globstar=!0,_.output+=I.output+A.output,De(C);continue}if(I.type==="slash"&&I.prev.type!=="bos"&&$[0]==="/"){let Vi=$[1]!==void 0?"|$":"";_.output=_.output.slice(0,-(I.output+A.output).length),I.output=`(?:${I.output}`,A.type="globstar",A.output=`${N(t)}${h}|${h}${Vi})`,A.value+=C,_.output+=I.output+A.output,_.globstar=!0,De(C+Ee()),W({type:"slash",value:"/",output:""});continue}if(I.type==="bos"&&$[0]==="/"){A.type="globstar",A.value+=C,A.output=`(?:^|${h}|${N(t)}${h})`,_.output=A.output,_.globstar=!0,De(C+Ee()),W({type:"slash",value:"/",output:""});continue}_.output=_.output.slice(0,-A.output.length),A.type="globstar",A.output=N(t),A.value+=C,_.output+=A.output,_.globstar=!0,De(C);continue}let ae={type:"star",value:C,output:Y};if(t.bash===!0){ae.output=".*?",(A.type==="bos"||A.type==="slash")&&(ae.output=R+ae.output),W(ae);continue}if(A&&(A.type==="bracket"||A.type==="paren")&&t.regex===!0){ae.output=C,W(ae);continue}(_.index===_.start||A.type==="slash"||A.type==="dot")&&(A.type==="dot"?(_.output+=w,A.output+=w):t.dot===!0?(_.output+=k,A.output+=k):(_.output+=R,A.output+=R),V()!=="*"&&(_.output+=b,A.output+=b)),W(ae)}for(;_.brackets>0;){if(t.strictBrackets===!0)throw new SyntaxError(gr("closing","]"));_.output=Me.escapeLast(_.output,"["),Ft("brackets")}for(;_.parens>0;){if(t.strictBrackets===!0)throw new SyntaxError(gr("closing",")"));_.output=Me.escapeLast(_.output,"("),Ft("parens")}for(;_.braces>0;){if(t.strictBrackets===!0)throw new SyntaxError(gr("closing","}"));_.output=Me.escapeLast(_.output,"{"),Ft("braces")}if(t.strictSlashes!==!0&&(A.type==="star"||A.type==="bracket")&&W({type:"maybe_slash",value:"",output:`${h}?`}),_.backtrack===!0){_.output="";for(let $ of _.tokens)_.output+=$.output!=null?$.output:$.value,$.suffix&&(_.output+=$.suffix)}return _};xl.fastpaths=(r,e)=>{let t={...e},i=typeof t.maxLength=="number"?Math.min(ws,t.maxLength):ws,n=r.length;if(n>i)throw new SyntaxError(`Input length: ${n}, exceeds maximum allowed length: ${i}`);r=tg[r]||r;let s=Me.isWindows(e),{DOT_LITERAL:a,SLASH_LITERAL:o,ONE_CHAR:l,DOTS_SLASH:c,NO_DOT:f,NO_DOTS:d,NO_DOTS_SLASH:p,STAR:h,START_ANCHOR:b}=bs.globChars(s),v=t.dot?d:f,y=t.dot?p:f,w=t.capture?"":"?:",k={negated:!1,prefix:""},S=t.bash===!0?".*?":h;t.capture&&(S=`(${S})`);let E=R=>R.noglobstar===!0?S:`(${w}(?:(?!${b}${R.dot?c:a}).)*?)`,T=R=>{switch(R){case"*":return`${v}${l}${S}`;case".*":return`${a}${l}${S}`;case"*.*":return`${v}${S}${a}${l}${S}`;case"*/*":return`${v}${S}${o}${l}${y}${S}`;case"**":return v+E(t);case"**/*":return`(?:${v}${E(t)}${o})?${y}${l}${S}`;case"**/*.*":return`(?:${v}${E(t)}${o})?${y}${S}${a}${l}${S}`;case"**/.*":return`(?:${v}${E(t)}${o})?${a}${l}${S}`;default:{let F=/^(.*?)\.(\w+)$/.exec(R);if(!F)return;let Y=T(F[1]);return Y?Y+a+F[2]:void 0}}},B=Me.removePrefix(r,k),N=T(B);return N&&t.strictSlashes!==!0&&(N+=`${o}?`),N};rg.exports=xl});var sg=x((c6,ng)=>{u();"use strict";var a2=(et(),Ur),o2=eg(),kl=ig(),Sl=qi(),l2=Di(),u2=r=>r&&typeof r=="object"&&!Array.isArray(r),de=(r,e,t=!1)=>{if(Array.isArray(r)){let f=r.map(p=>de(p,e,t));return p=>{for(let h of f){let b=h(p);if(b)return b}return!1}}let i=u2(r)&&r.tokens&&r.input;if(r===""||typeof r!="string"&&!i)throw new TypeError("Expected pattern to be a non-empty string");let n=e||{},s=Sl.isWindows(e),a=i?de.compileRe(r,e):de.makeRe(r,e,!1,!0),o=a.state;delete a.state;let l=()=>!1;if(n.ignore){let f={...e,ignore:null,onMatch:null,onResult:null};l=de(n.ignore,f,t)}let c=(f,d=!1)=>{let{isMatch:p,match:h,output:b}=de.test(f,a,e,{glob:r,posix:s}),v={glob:r,state:o,regex:a,posix:s,input:f,output:b,match:h,isMatch:p};return typeof n.onResult=="function"&&n.onResult(v),p===!1?(v.isMatch=!1,d?v:!1):l(f)?(typeof n.onIgnore=="function"&&n.onIgnore(v),v.isMatch=!1,d?v:!1):(typeof n.onMatch=="function"&&n.onMatch(v),d?v:!0)};return t&&(c.state=o),c};de.test=(r,e,t,{glob:i,posix:n}={})=>{if(typeof r!="string")throw new TypeError("Expected input to be a string");if(r==="")return{isMatch:!1,output:""};let s=t||{},a=s.format||(n?Sl.toPosixSlashes:null),o=r===i,l=o&&a?a(r):r;return o===!1&&(l=a?a(r):r,o=l===i),(o===!1||s.capture===!0)&&(s.matchBase===!0||s.basename===!0?o=de.matchBase(r,e,t,n):o=e.exec(l)),{isMatch:Boolean(o),match:o,output:l}};de.matchBase=(r,e,t,i=Sl.isWindows(t))=>(e instanceof RegExp?e:de.makeRe(e,t)).test(a2.basename(r));de.isMatch=(r,e,t)=>de(e,t)(r);de.parse=(r,e)=>Array.isArray(r)?r.map(t=>de.parse(t,e)):kl(r,{...e,fastpaths:!1});de.scan=(r,e)=>o2(r,e);de.compileRe=(r,e,t=!1,i=!1)=>{if(t===!0)return r.output;let n=e||{},s=n.contains?"":"^",a=n.contains?"":"$",o=`${s}(?:${r.output})${a}`;r&&r.negated===!0&&(o=`^(?!${o}).*$`);let l=de.toRegex(o,e);return i===!0&&(l.state=r),l};de.makeRe=(r,e={},t=!1,i=!1)=>{if(!r||typeof r!="string")throw new TypeError("Expected a non-empty string");let n={negated:!1,fastpaths:!0};return e.fastpaths!==!1&&(r[0]==="."||r[0]==="*")&&(n.output=kl.fastpaths(r,e)),n.output||(n=kl(r,e)),de.compileRe(n,e,t,i)};de.toRegex=(r,e)=>{try{let t=e||{};return new RegExp(r,t.flags||(t.nocase?"i":""))}catch(t){if(e&&e.debug===!0)throw t;return/$^/}};de.constants=l2;ng.exports=de});var og=x((p6,ag)=>{u();"use strict";ag.exports=sg()});var dg=x((d6,pg)=>{u();"use strict";var lg=(Fn(),Bn),ug=jm(),ot=og(),Al=qi(),fg=r=>r===""||r==="./",cg=r=>{let e=r.indexOf("{");return e>-1&&r.indexOf("}",e)>-1},oe=(r,e,t)=>{e=[].concat(e),r=[].concat(r);let i=new Set,n=new Set,s=new Set,a=0,o=f=>{s.add(f.output),t&&t.onResult&&t.onResult(f)};for(let f=0;f<e.length;f++){let d=ot(String(e[f]),{...t,onResult:o},!0),p=d.state.negated||d.state.negatedExtglob;p&&a++;for(let h of r){let b=d(h,!0);!(p?!b.isMatch:b.isMatch)||(p?i.add(b.output):(i.delete(b.output),n.add(b.output)))}}let c=(a===e.length?[...s]:[...n]).filter(f=>!i.has(f));if(t&&c.length===0){if(t.failglob===!0)throw new Error(`No matches found for "${e.join(", ")}"`);if(t.nonull===!0||t.nullglob===!0)return t.unescape?e.map(f=>f.replace(/\\/g,"")):e}return c};oe.match=oe;oe.matcher=(r,e)=>ot(r,e);oe.isMatch=(r,e,t)=>ot(e,t)(r);oe.any=oe.isMatch;oe.not=(r,e,t={})=>{e=[].concat(e).map(String);let i=new Set,n=[],s=o=>{t.onResult&&t.onResult(o),n.push(o.output)},a=new Set(oe(r,e,{...t,onResult:s}));for(let o of n)a.has(o)||i.add(o);return[...i]};oe.contains=(r,e,t)=>{if(typeof r!="string")throw new TypeError(`Expected a string: "${lg.inspect(r)}"`);if(Array.isArray(e))return e.some(i=>oe.contains(r,i,t));if(typeof e=="string"){if(fg(r)||fg(e))return!1;if(r.includes(e)||r.startsWith("./")&&r.slice(2).includes(e))return!0}return oe.isMatch(r,e,{...t,contains:!0})};oe.matchKeys=(r,e,t)=>{if(!Al.isObject(r))throw new TypeError("Expected the first argument to be an object");let i=oe(Object.keys(r),e,t),n={};for(let s of i)n[s]=r[s];return n};oe.some=(r,e,t)=>{let i=[].concat(r);for(let n of[].concat(e)){let s=ot(String(n),t);if(i.some(a=>s(a)))return!0}return!1};oe.every=(r,e,t)=>{let i=[].concat(r);for(let n of[].concat(e)){let s=ot(String(n),t);if(!i.every(a=>s(a)))return!1}return!0};oe.all=(r,e,t)=>{if(typeof r!="string")throw new TypeError(`Expected a string: "${lg.inspect(r)}"`);return[].concat(e).every(i=>ot(i,t)(r))};oe.capture=(r,e,t)=>{let i=Al.isWindows(t),s=ot.makeRe(String(r),{...t,capture:!0}).exec(i?Al.toPosixSlashes(e):e);if(s)return s.slice(1).map(a=>a===void 0?"":a)};oe.makeRe=(...r)=>ot.makeRe(...r);oe.scan=(...r)=>ot.scan(...r);oe.parse=(r,e)=>{let t=[];for(let i of[].concat(r||[]))for(let n of ug(String(i),e))t.push(ot.parse(n,e));return t};oe.braces=(r,e)=>{if(typeof r!="string")throw new TypeError("Expected a string");return e&&e.nobrace===!0||!cg(r)?[r]:ug(r,e)};oe.braceExpand=(r,e)=>{if(typeof r!="string")throw new TypeError("Expected a string");return oe.braces(r,{...e,expand:!0})};oe.hasBraces=cg;pg.exports=oe});function mg(r,e){let t=e.content.files;t=t.filter(o=>typeof o=="string"),t=t.map(ll);let i=ps.generateTasks(t),n=[],s=[];for(let o of i)n.push(...o.positive.map(l=>gg(l,!1))),s.push(...o.negative.map(l=>gg(l,!0)));let a=[...n,...s];return a=c2(r,a),a=a.flatMap(p2),a=a.map(f2),a}function gg(r,e){let t={original:r,base:r,ignore:e,pattern:r,glob:null};return Zh(r)&&Object.assign(t,nm(r)),t}function f2(r){let e=ll(r.base);return e=ps.escapePath(e),r.pattern=r.glob?`${e}/${r.glob}`:e,r.pattern=r.ignore?`!${r.pattern}`:r.pattern,r}function c2(r,e){let t=[];return r.userConfigPath&&r.tailwindConfig.content.relative&&(t=[me.dirname(r.userConfigPath)]),e.map(i=>(i.base=me.resolve(...t,i.base),i))}function p2(r){let e=[r];try{let t=be.realpathSync(r.base);t!==r.base&&e.push({...r,base:t})}catch{}return e}function yg(r,e,t){let i=r.tailwindConfig.content.files.filter(a=>typeof a.raw=="string").map(({raw:a,extension:o="html"})=>({content:a,extension:o})),[n,s]=h2(e,t);for(let a of n){let o=me.extname(a).slice(1);i.push({file:a,extension:o})}return[i,s]}function d2(r){if(!r.some(s=>s.includes("**")&&!wg.test(s)))return()=>{};let t=[],i=[];for(let s of r){let a=hg.default.matcher(s);wg.test(s)&&i.push(a),t.push(a)}let n=!1;return s=>{if(n||i.some(f=>f(s)))return;let a=t.findIndex(f=>f(s));if(a===-1)return;let o=r[a],l=me.relative(m.cwd(),o);l[0]!=="."&&(l=`./${l}`);let c=bg.find(f=>s.includes(f));c&&(n=!0,G.warn("broad-content-glob-pattern",[`Your \`content\` configuration includes a pattern which looks like it's accidentally matching all of \`${c}\` and can cause serious performance issues.`,`Pattern: \`${l}\``,"See our documentation for recommendations:","https://tailwindcss.com/docs/content-configuration#pattern-recommendations"]))}}function h2(r,e){let t=r.map(o=>o.pattern),i=new Map,n=d2(t),s=new Set;Ze.DEBUG&&console.time("Finding changed files");let a=ps.sync(t,{absolute:!0});for(let o of a){n(o);let l=e.get(o)||-1/0,c=be.statSync(o).mtimeMs;c>l&&(s.add(o),i.set(o,c))}return Ze.DEBUG&&console.timeEnd("Finding changed files"),[s,i]}var hg,bg,wg,vg=P(()=>{u();ft();et();Jh();em();tm();sm();It();Be();hg=pe(dg());bg=["node_modules"],wg=new RegExp(`(${bg.map(r=>String.raw`\b${r}\b`).join("|")})`)});function xg(){}var kg=P(()=>{u()});function b2(r,e){for(let t of e){let i=`${r}${t}`;if(be.existsSync(i)&&be.statSync(i).isFile())return i}for(let t of e){let i=`${r}/index${t}`;if(be.existsSync(i))return i}return null}function*Sg(r,e,t,i=me.extname(r)){let n=b2(me.resolve(e,r),m2.includes(i)?g2:y2);if(n===null||t.has(n))return;t.add(n),yield n,e=me.dirname(n),i=me.extname(n);let s=be.readFileSync(n,"utf-8");for(let a of[...s.matchAll(/import[\s\S]*?['"](.{3,}?)['"]/gi),...s.matchAll(/import[\s\S]*from[\s\S]*?['"](.{3,}?)['"]/gi),...s.matchAll(/require\(['"`](.+)['"`]\)/gi)])!a[1].startsWith(".")||(yield*Sg(a[1],e,t,i))}function Cl(r){return r===null?new Set:new Set(Sg(r,me.dirname(r),new Set))}var m2,g2,y2,Ag=P(()=>{u();ft();et();m2=[".js",".cjs",".mjs"],g2=["",".js",".cjs",".mjs",".ts",".cts",".mts",".jsx",".tsx"],y2=["",".ts",".cts",".mts",".tsx",".js",".cjs",".mjs",".jsx"]});function w2(r,e){if(_l.has(r))return _l.get(r);let t=mg(r,e);return _l.set(r,t).get(r)}function v2(r){let e=aa(r);if(e!==null){let[i,n,s,a]=_g.get(e)||[],o=Cl(e),l=!1,c=new Map;for(let p of o){let h=be.statSync(p).mtimeMs;c.set(p,h),(!a||!a.has(p)||h>a.get(p))&&(l=!0)}if(!l)return[i,e,n,s];for(let p of o)delete hf.cache[p];let f=ol(zr(xg(e))),d=Wi(f);return _g.set(e,[f,d,o,c]),[f,e,d,o]}let t=zr(r?.config??r??{});return t=ol(t),[t,null,Wi(t),[]]}function El(r){return({tailwindDirectives:e,registerDependency:t})=>(i,n)=>{let[s,a,o,l]=v2(r),c=new Set(l);if(e.size>0){c.add(n.opts.from);for(let b of n.messages)b.type==="dependency"&&c.add(b.file)}let[f,,d]=Vh(i,n,s,a,o,c),p=cs(f),h=w2(f,s);if(e.size>0){for(let y of h)for(let w of nl(y))t(w);let[b,v]=yg(f,h,p);for(let y of b)f.changedContent.push(y);for(let[y,w]of v.entries())d.set(y,w)}for(let b of l)t({type:"dependency",file:b});for(let[b,v]of d.entries())p.set(b,v);return f}}var Cg,_g,_l,Eg=P(()=>{u();ft();Cg=pe(Fs());wf();sa();oc();Oi();Hh();Xh();vg();kg();Ag();_g=new Cg.default({maxSize:100}),_l=new WeakMap});function Ol(r){let e=new Set,t=new Set,i=new Set;if(r.walkAtRules(n=>{n.name==="apply"&&i.add(n),n.name==="import"&&(n.params==='"tailwindcss/base"'||n.params==="'tailwindcss/base'"?(n.name="tailwind",n.params="base"):n.params==='"tailwindcss/components"'||n.params==="'tailwindcss/components'"?(n.name="tailwind",n.params="components"):n.params==='"tailwindcss/utilities"'||n.params==="'tailwindcss/utilities'"?(n.name="tailwind",n.params="utilities"):(n.params==='"tailwindcss/screens"'||n.params==="'tailwindcss/screens'"||n.params==='"tailwindcss/variants"'||n.params==="'tailwindcss/variants'")&&(n.name="tailwind",n.params="variants")),n.name==="tailwind"&&(n.params==="screens"&&(n.params="variants"),e.add(n.params)),["layer","responsive","variants"].includes(n.name)&&(["responsive","variants"].includes(n.name)&&G.warn(`${n.name}-at-rule-deprecated`,[`The \`@${n.name}\` directive has been deprecated in Tailwind CSS v3.0.`,"Use `@layer utilities` or `@layer components` instead.","https://tailwindcss.com/docs/upgrade-guide#replace-variants-with-layer"]),t.add(n))}),!e.has("base")||!e.has("components")||!e.has("utilities")){for(let n of t)if(n.name==="layer"&&["base","components","utilities"].includes(n.params)){if(!e.has(n.params))throw n.error(`\`@layer ${n.params}\` is used but no matching \`@tailwind ${n.params}\` directive is present.`)}else if(n.name==="responsive"){if(!e.has("utilities"))throw n.error("`@responsive` is used but `@tailwind utilities` is missing.")}else if(n.name==="variants"&&!e.has("utilities"))throw n.error("`@variants` is used but `@tailwind utilities` is missing.")}return{tailwindDirectives:e,applyDirectives:i}}var Og=P(()=>{u();Be()});function Qt(r,e=void 0,t=void 0){return r.map(i=>{let n=i.clone();return t!==void 0&&(n.raws.tailwind={...n.raws.tailwind,...t}),e!==void 0&&Tg(n,s=>{if(s.raws.tailwind?.preserveSource===!0&&s.source)return!1;s.source=e}),n})}function Tg(r,e){e(r)!==!1&&r.each?.(t=>Tg(t,e))}var Rg=P(()=>{u()});function Tl(r){return r=Array.isArray(r)?r:[r],r=r.map(e=>e instanceof RegExp?e.source:e),r.join("")}function Ne(r){return new RegExp(Tl(r),"g")}function qt(r){return`(?:${r.map(Tl).join("|")})`}function Rl(r){return`(?:${Tl(r)})?`}function Ig(r){return r&&x2.test(r)?r.replace(Pg,"\\$&"):r||""}var Pg,x2,Dg=P(()=>{u();Pg=/[\\^$.*+?()[\]{}|]/g,x2=RegExp(Pg.source)});function qg(r){let e=Array.from(k2(r));return t=>{let i=[];for(let n of e)for(let s of t.match(n)??[])i.push(C2(s));for(let n of i.slice()){let s=ve(n,".");for(let a=0;a<s.length;a++){let o=s[a];if(a>=s.length-1){i.push(o);continue}let l=Number(s[a+1]);isNaN(l)?i.push(o):a++}}return i}}function*k2(r){let e=r.tailwindConfig.separator,t=r.tailwindConfig.prefix!==""?Rl(Ne([/-?/,Ig(r.tailwindConfig.prefix)])):"",i=qt([/\[[^\s:'"`]+:[^\s\[\]]+\]/,/\[[^\s:'"`\]]+:[^\s]+?\[[^\s]+\][^\s]+?\]/,Ne([qt([/-?(?:\w+)/,/@(?:\w+)/]),Rl(qt([Ne([qt([/-(?:\w+-)*\['[^\s]+'\]/,/-(?:\w+-)*\["[^\s]+"\]/,/-(?:\w+-)*\[`[^\s]+`\]/,/-(?:\w+-)*\[(?:[^\s\[\]]+\[[^\s\[\]]+\])*[^\s:\[\]]+\]/]),/(?![{([]])/,/(?:\/[^\s'"`\\><$]*)?/]),Ne([qt([/-(?:\w+-)*\['[^\s]+'\]/,/-(?:\w+-)*\["[^\s]+"\]/,/-(?:\w+-)*\[`[^\s]+`\]/,/-(?:\w+-)*\[(?:[^\s\[\]]+\[[^\s\[\]]+\])*[^\s\[\]]+\]/]),/(?![{([]])/,/(?:\/[^\s'"`\\$]*)?/]),/[-\/][^\s'"`\\$={><]*/]))])]),n=[qt([Ne([/@\[[^\s"'`]+\](\/[^\s"'`]+)?/,e]),Ne([/([^\s"'`\[\\]+-)?\[[^\s"'`]+\]\/[\w_-]+/,e]),Ne([/([^\s"'`\[\\]+-)?\[[^\s"'`]+\]/,e]),Ne([/[^\s"'`\[\\]+/,e])]),qt([Ne([/([^\s"'`\[\\]+-)?\[[^\s`]+\]\/[\w_-]+/,e]),Ne([/([^\s"'`\[\\]+-)?\[[^\s`]+\]/,e]),Ne([/[^\s`\[\\]+/,e])])];for(let s of n)yield Ne(["((?=((",s,")+))\\2)?",/!?/,t,i]);yield/[^<>"'`\s.(){}[\]#=%$][^<>"'`\s(){}[\]#=%$]*[^<>"'`\s.(){}[\]#=%:$]/g}function C2(r){if(!r.includes("-["))return r;let e=0,t=[],i=r.matchAll(S2);i=Array.from(i).flatMap(n=>{let[,...s]=n;return s.map((a,o)=>Object.assign([],n,{index:n.index+o,0:a}))});for(let n of i){let s=n[0],a=t[t.length-1];if(s===a?t.pop():(s==="'"||s==='"'||s==="`")&&t.push(s),!a){if(s==="["){e++;continue}else if(s==="]"){e--;continue}if(e<0)return r.substring(0,n.index-1);if(e===0&&!A2.test(s))return r.substring(0,n.index)}}return r}var S2,A2,$g=P(()=>{u();Dg();zt();S2=/([\[\]'"`])([^\[\]'"`])?/g,A2=/[^"'`\s<>\]]+/});function _2(r,e){let t=r.tailwindConfig.content.extract;return t[e]||t.DEFAULT||Mg[e]||Mg.DEFAULT(r)}function E2(r,e){let t=r.content.transform;return t[e]||t.DEFAULT||Ng[e]||Ng.DEFAULT}function O2(r,e,t,i){Li.has(e)||Li.set(e,new Lg.default({maxSize:25e3}));for(let n of r.split(`
`))if(n=n.trim(),!i.has(n))if(i.add(n),Li.get(e).has(n))for(let s of Li.get(e).get(n))t.add(s);else{let s=e(n).filter(o=>o!=="!*"),a=new Set(s);for(let o of a)t.add(o);Li.get(e).set(n,a)}}function T2(r,e){let t=e.offsets.sort(r),i={base:new Set,defaults:new Set,components:new Set,utilities:new Set,variants:new Set};for(let[n,s]of t)i[n.layer].add(s);return i}function Pl(r){return async e=>{let t={base:null,components:null,utilities:null,variants:null};if(e.walkAtRules(y=>{y.name==="tailwind"&&Object.keys(t).includes(y.params)&&(t[y.params]=y)}),Object.values(t).every(y=>y===null))return e;let i=new Set([...r.candidates??[],gt]),n=new Set;bt.DEBUG&&console.time("Reading changed files");let s=[];for(let y of r.changedContent){let w=E2(r.tailwindConfig,y.extension),k=_2(r,y.extension);s.push([y,{transformer:w,extractor:k}])}let a=500;for(let y=0;y<s.length;y+=a){let w=s.slice(y,y+a);await Promise.all(w.map(async([{file:k,content:S},{transformer:E,extractor:T}])=>{S=k?await be.promises.readFile(k,"utf8"):S,O2(E(S),T,i,n)}))}bt.DEBUG&&console.timeEnd("Reading changed files");let o=r.classCache.size;bt.DEBUG&&console.time("Generate rules"),bt.DEBUG&&console.time("Sorting candidates");let l=new Set([...i].sort((y,w)=>y===w?0:y<w?-1:1));bt.DEBUG&&console.timeEnd("Sorting candidates"),as(l,r),bt.DEBUG&&console.timeEnd("Generate rules"),bt.DEBUG&&console.time("Build stylesheet"),(r.stylesheetCache===null||r.classCache.size!==o)&&(r.stylesheetCache=T2([...r.ruleCache],r)),bt.DEBUG&&console.timeEnd("Build stylesheet");let{defaults:c,base:f,components:d,utilities:p,variants:h}=r.stylesheetCache;t.base&&(t.base.before(Qt([...c,...f],t.base.source,{layer:"base"})),t.base.remove()),t.components&&(t.components.before(Qt([...d],t.components.source,{layer:"components"})),t.components.remove()),t.utilities&&(t.utilities.before(Qt([...p],t.utilities.source,{layer:"utilities"})),t.utilities.remove());let b=Array.from(h).filter(y=>{let w=y.raws.tailwind?.parentLayer;return w==="components"?t.components!==null:w==="utilities"?t.utilities!==null:!0});t.variants?(t.variants.before(Qt(b,t.variants.source,{layer:"variants"})),t.variants.remove()):b.length>0&&e.append(Qt(b,e.source,{layer:"variants"})),e.source.end=e.source.end??e.source.start;let v=b.some(y=>y.raws.tailwind?.parentLayer==="utilities");t.utilities&&p.size===0&&!v&&G.warn("content-problems",["No utility classes were detected in your source files. If this is unexpected, double-check the `content` option in your Tailwind CSS configuration.","https://tailwindcss.com/docs/content-configuration"]),bt.DEBUG&&(console.log("Potential classes: ",i.size),console.log("Active contexts: ",es.size)),r.changedContent=[],e.walkAtRules("layer",y=>{Object.keys(t).includes(y.params)&&y.remove()})}}var Lg,bt,Mg,Ng,Li,Bg=P(()=>{u();ft();Lg=pe(Fs());It();os();Be();Rg();$g();bt=Ze,Mg={DEFAULT:qg},Ng={DEFAULT:r=>r,svelte:r=>r.replace(/(?:^|\s)class:/g," ")};Li=new WeakMap});function xs(r){let e=new Map;ee.root({nodes:[r.clone()]}).walkRules(s=>{(0,vs.default)(a=>{a.walkClasses(o=>{let l=o.parent.toString(),c=e.get(l);c||e.set(l,c=new Set),c.add(o.value)})}).processSync(s.selector)});let i=Array.from(e.values(),s=>Array.from(s)),n=i.flat();return Object.assign(n,{groups:i})}function Il(r){return R2.astSync(r)}function Fg(r,e){let t=new Set;for(let i of r)t.add(i.split(e).pop());return Array.from(t)}function jg(r,e){let t=r.tailwindConfig.prefix;return typeof t=="function"?t(e):t+e}function*zg(r){for(yield r;r.parent;)yield r.parent,r=r.parent}function P2(r,e={}){let t=r.nodes;r.nodes=[];let i=r.clone(e);return r.nodes=t,i}function I2(r){for(let e of zg(r))if(r!==e){if(e.type==="root")break;r=P2(e,{nodes:[r]})}return r}function D2(r,e){let t=new Map;return r.walkRules(i=>{for(let a of zg(i))if(a.raws.tailwind?.layer!==void 0)return;let n=I2(i),s=e.offsets.create("user");for(let a of xs(i)){let o=t.get(a)||[];t.set(a,o),o.push([{layer:"user",sort:s,important:!1},n])}}),t}function q2(r,e){for(let t of r){if(e.notClassCache.has(t)||e.applyClassCache.has(t))continue;if(e.classCache.has(t)){e.applyClassCache.set(t,e.classCache.get(t).map(([n,s])=>[n,s.clone()]));continue}let i=Array.from(Yo(t,e));if(i.length===0){e.notClassCache.add(t);continue}e.applyClassCache.set(t,i)}return e.applyClassCache}function $2(r){let e=null;return{get:t=>(e=e||r(),e.get(t)),has:t=>(e=e||r(),e.has(t))}}function L2(r){return{get:e=>r.flatMap(t=>t.get(e)||[]),has:e=>r.some(t=>t.has(e))}}function Ug(r){let e=r.split(/[\s\t\n]+/g);return e[e.length-1]==="!important"?[e.slice(0,-1),!0]:[e,!1]}function Vg(r,e,t){let i=new Set,n=[];if(r.walkAtRules("apply",l=>{let[c]=Ug(l.params);for(let f of c)i.add(f);n.push(l)}),n.length===0)return;let s=L2([t,q2(i,e)]);function a(l,c,f){let d=Il(l),p=Il(c),b=Il(`.${Te(f)}`).nodes[0].nodes[0];return d.each(v=>{let y=new Set;p.each(w=>{let k=!1;w=w.clone(),w.walkClasses(S=>{S.value===b.value&&(k||(S.replaceWith(...v.nodes.map(E=>E.clone())),y.add(w),k=!0))})});for(let w of y){let k=[[]];for(let S of w.nodes)S.type==="combinator"?(k.push(S),k.push([])):k[k.length-1].push(S);w.nodes=[];for(let S of k)Array.isArray(S)&&S.sort((E,T)=>E.type==="tag"&&T.type==="class"?-1:E.type==="class"&&T.type==="tag"?1:E.type==="class"&&T.type==="pseudo"&&T.value.startsWith("::")?-1:E.type==="pseudo"&&E.value.startsWith("::")&&T.type==="class"?1:0),w.nodes=w.nodes.concat(S)}v.replaceWith(...y)}),d.toString()}let o=new Map;for(let l of n){let[c]=o.get(l.parent)||[[],l.source];o.set(l.parent,[c,l.source]);let[f,d]=Ug(l.params);if(l.parent.type==="atrule"){if(l.parent.name==="screen"){let p=l.parent.params;throw l.error(`@apply is not supported within nested at-rules like @screen. We suggest you write this as @apply ${f.map(h=>`${p}:${h}`).join(" ")} instead.`)}throw l.error(`@apply is not supported within nested at-rules like @${l.parent.name}. You can fix this by un-nesting @${l.parent.name}.`)}for(let p of f){if([jg(e,"group"),jg(e,"peer")].includes(p))throw l.error(`@apply should not be used with the '${p}' utility`);if(!s.has(p))throw l.error(`The \`${p}\` class does not exist. If \`${p}\` is a custom class, make sure it is defined within a \`@layer\` directive.`);let h=s.get(p);for(let[,b]of h)b.type!=="atrule"&&b.walkRules(()=>{throw l.error([`The \`${p}\` class cannot be used with \`@apply\` because \`@apply\` does not currently support nested CSS.`,"Rewrite the selector without nesting or configure the `tailwindcss/nesting` plugin:","https://tailwindcss.com/docs/using-with-preprocessors#nesting"].join(`
`))});c.push([p,d,h])}}for(let[l,[c,f]]of o){let d=[];for(let[h,b,v]of c){let y=[h,...Fg([h],e.tailwindConfig.separator)];for(let[w,k]of v){let S=xs(l),E=xs(k);if(E=E.groups.filter(R=>R.some(F=>y.includes(F))).flat(),E=E.concat(Fg(E,e.tailwindConfig.separator)),S.some(R=>E.includes(R)))throw k.error(`You cannot \`@apply\` the \`${h}\` utility here because it creates a circular dependency.`);let B=ee.root({nodes:[k.clone()]});B.walk(R=>{R.source=f}),(k.type!=="atrule"||k.type==="atrule"&&k.name!=="keyframes")&&B.walkRules(R=>{if(!xs(R).some(U=>U===h)){R.remove();return}let F=typeof e.tailwindConfig.important=="string"?e.tailwindConfig.important:null,_=l.raws.tailwind!==void 0&&F&&l.selector.indexOf(F)===0?l.selector.slice(F.length):l.selector;_===""&&(_=l.selector),R.selector=a(_,R.selector,h),F&&_!==l.selector&&(R.selector=is(R.selector,F)),R.walkDecls(U=>{U.important=w.important||b});let Q=(0,vs.default)().astSync(R.selector);Q.each(U=>pr(U)),R.selector=Q.toString()}),!!B.nodes[0]&&d.push([w.sort,B.nodes[0]])}}let p=e.offsets.sort(d).map(h=>h[1]);l.after(p)}for(let l of n)l.parent.nodes.length>1?l.remove():l.parent.remove();Vg(r,e,t)}function Dl(r){return e=>{let t=$2(()=>D2(e,r));Vg(e,r,t)}}var vs,R2,Hg=P(()=>{u();Ot();vs=pe(it());os();fr();Wo();ts();R2=(0,vs.default)()});var Wg=x((nq,ks)=>{u();(function(){"use strict";function r(i,n,s){if(!i)return null;r.caseSensitive||(i=i.toLowerCase());var a=r.threshold===null?null:r.threshold*i.length,o=r.thresholdAbsolute,l;a!==null&&o!==null?l=Math.min(a,o):a!==null?l=a:o!==null?l=o:l=null;var c,f,d,p,h,b=n.length;for(h=0;h<b;h++)if(f=n[h],s&&(f=f[s]),!!f&&(r.caseSensitive?d=f:d=f.toLowerCase(),p=t(i,d,l),(l===null||p<l)&&(l=p,s&&r.returnWinningObject?c=n[h]:c=f,r.returnFirstMatch)))return c;return c||r.nullResultValue}r.threshold=.4,r.thresholdAbsolute=20,r.caseSensitive=!1,r.nullResultValue=null,r.returnWinningObject=null,r.returnFirstMatch=!1,typeof ks!="undefined"&&ks.exports?ks.exports=r:window.didYouMean=r;var e=Math.pow(2,32)-1;function t(i,n,s){s=s||s===0?s:e;var a=i.length,o=n.length;if(a===0)return Math.min(s+1,o);if(o===0)return Math.min(s+1,a);if(Math.abs(a-o)>s)return s+1;var l=[],c,f,d,p,h;for(c=0;c<=o;c++)l[c]=[c];for(f=0;f<=a;f++)l[0][f]=f;for(c=1;c<=o;c++){for(d=e,p=1,c>s&&(p=c-s),h=o+1,h>s+c&&(h=s+c),f=1;f<=a;f++)f<p||f>h?l[c][f]=s+1:n.charAt(c-1)===i.charAt(f-1)?l[c][f]=l[c-1][f-1]:l[c][f]=Math.min(l[c-1][f-1]+1,Math.min(l[c][f-1]+1,l[c-1][f]+1)),l[c][f]<d&&(d=l[c][f]);if(d>s)return s+1}return l[o][a]}})()});var Qg=x((sq,Gg)=>{u();var ql="(".charCodeAt(0),$l=")".charCodeAt(0),Ss="'".charCodeAt(0),Ll='"'.charCodeAt(0),Ml="\\".charCodeAt(0),yr="/".charCodeAt(0),Nl=",".charCodeAt(0),Bl=":".charCodeAt(0),As="*".charCodeAt(0),M2="u".charCodeAt(0),N2="U".charCodeAt(0),B2="+".charCodeAt(0),F2=/^[a-f0-9?-]+$/i;Gg.exports=function(r){for(var e=[],t=r,i,n,s,a,o,l,c,f,d=0,p=t.charCodeAt(d),h=t.length,b=[{nodes:e}],v=0,y,w="",k="",S="";d<h;)if(p<=32){i=d;do i+=1,p=t.charCodeAt(i);while(p<=32);a=t.slice(d,i),s=e[e.length-1],p===$l&&v?S=a:s&&s.type==="div"?(s.after=a,s.sourceEndIndex+=a.length):p===Nl||p===Bl||p===yr&&t.charCodeAt(i+1)!==As&&(!y||y&&y.type==="function"&&!1)?k=a:e.push({type:"space",sourceIndex:d,sourceEndIndex:i,value:a}),d=i}else if(p===Ss||p===Ll){i=d,n=p===Ss?"'":'"',a={type:"string",sourceIndex:d,quote:n};do if(o=!1,i=t.indexOf(n,i+1),~i)for(l=i;t.charCodeAt(l-1)===Ml;)l-=1,o=!o;else t+=n,i=t.length-1,a.unclosed=!0;while(o);a.value=t.slice(d+1,i),a.sourceEndIndex=a.unclosed?i:i+1,e.push(a),d=i+1,p=t.charCodeAt(d)}else if(p===yr&&t.charCodeAt(d+1)===As)i=t.indexOf("*/",d),a={type:"comment",sourceIndex:d,sourceEndIndex:i+2},i===-1&&(a.unclosed=!0,i=t.length,a.sourceEndIndex=i),a.value=t.slice(d+2,i),e.push(a),d=i+2,p=t.charCodeAt(d);else if((p===yr||p===As)&&y&&y.type==="function")a=t[d],e.push({type:"word",sourceIndex:d-k.length,sourceEndIndex:d+a.length,value:a}),d+=1,p=t.charCodeAt(d);else if(p===yr||p===Nl||p===Bl)a=t[d],e.push({type:"div",sourceIndex:d-k.length,sourceEndIndex:d+a.length,value:a,before:k,after:""}),k="",d+=1,p=t.charCodeAt(d);else if(ql===p){i=d;do i+=1,p=t.charCodeAt(i);while(p<=32);if(f=d,a={type:"function",sourceIndex:d-w.length,value:w,before:t.slice(f+1,i)},d=i,w==="url"&&p!==Ss&&p!==Ll){i-=1;do if(o=!1,i=t.indexOf(")",i+1),~i)for(l=i;t.charCodeAt(l-1)===Ml;)l-=1,o=!o;else t+=")",i=t.length-1,a.unclosed=!0;while(o);c=i;do c-=1,p=t.charCodeAt(c);while(p<=32);f<c?(d!==c+1?a.nodes=[{type:"word",sourceIndex:d,sourceEndIndex:c+1,value:t.slice(d,c+1)}]:a.nodes=[],a.unclosed&&c+1!==i?(a.after="",a.nodes.push({type:"space",sourceIndex:c+1,sourceEndIndex:i,value:t.slice(c+1,i)})):(a.after=t.slice(c+1,i),a.sourceEndIndex=i)):(a.after="",a.nodes=[]),d=i+1,a.sourceEndIndex=a.unclosed?i:d,p=t.charCodeAt(d),e.push(a)}else v+=1,a.after="",a.sourceEndIndex=d+1,e.push(a),b.push(a),e=a.nodes=[],y=a;w=""}else if($l===p&&v)d+=1,p=t.charCodeAt(d),y.after=S,y.sourceEndIndex+=S.length,S="",v-=1,b[b.length-1].sourceEndIndex=d,b.pop(),y=b[v],e=y.nodes;else{i=d;do p===Ml&&(i+=1),i+=1,p=t.charCodeAt(i);while(i<h&&!(p<=32||p===Ss||p===Ll||p===Nl||p===Bl||p===yr||p===ql||p===As&&y&&y.type==="function"&&!0||p===yr&&y.type==="function"&&!0||p===$l&&v));a=t.slice(d,i),ql===p?w=a:(M2===a.charCodeAt(0)||N2===a.charCodeAt(0))&&B2===a.charCodeAt(1)&&F2.test(a.slice(2))?e.push({type:"unicode-range",sourceIndex:d,sourceEndIndex:i,value:a}):e.push({type:"word",sourceIndex:d,sourceEndIndex:i,value:a}),d=i}for(d=b.length-1;d;d-=1)b[d].unclosed=!0,b[d].sourceEndIndex=t.length;return b[0].nodes}});var Kg=x((aq,Yg)=>{u();Yg.exports=function r(e,t,i){var n,s,a,o;for(n=0,s=e.length;n<s;n+=1)a=e[n],i||(o=t(a,n,e)),o!==!1&&a.type==="function"&&Array.isArray(a.nodes)&&r(a.nodes,t,i),i&&t(a,n,e)}});var ey=x((oq,Jg)=>{u();function Xg(r,e){var t=r.type,i=r.value,n,s;return e&&(s=e(r))!==void 0?s:t==="word"||t==="space"?i:t==="string"?(n=r.quote||"",n+i+(r.unclosed?"":n)):t==="comment"?"/*"+i+(r.unclosed?"":"*/"):t==="div"?(r.before||"")+i+(r.after||""):Array.isArray(r.nodes)?(n=Zg(r.nodes,e),t!=="function"?n:i+"("+(r.before||"")+n+(r.after||"")+(r.unclosed?"":")")):i}function Zg(r,e){var t,i;if(Array.isArray(r)){for(t="",i=r.length-1;~i;i-=1)t=Xg(r[i],e)+t;return t}return Xg(r,e)}Jg.exports=Zg});var ry=x((lq,ty)=>{u();var Cs="-".charCodeAt(0),_s="+".charCodeAt(0),Fl=".".charCodeAt(0),j2="e".charCodeAt(0),z2="E".charCodeAt(0);function U2(r){var e=r.charCodeAt(0),t;if(e===_s||e===Cs){if(t=r.charCodeAt(1),t>=48&&t<=57)return!0;var i=r.charCodeAt(2);return t===Fl&&i>=48&&i<=57}return e===Fl?(t=r.charCodeAt(1),t>=48&&t<=57):e>=48&&e<=57}ty.exports=function(r){var e=0,t=r.length,i,n,s;if(t===0||!U2(r))return!1;for(i=r.charCodeAt(e),(i===_s||i===Cs)&&e++;e<t&&(i=r.charCodeAt(e),!(i<48||i>57));)e+=1;if(i=r.charCodeAt(e),n=r.charCodeAt(e+1),i===Fl&&n>=48&&n<=57)for(e+=2;e<t&&(i=r.charCodeAt(e),!(i<48||i>57));)e+=1;if(i=r.charCodeAt(e),n=r.charCodeAt(e+1),s=r.charCodeAt(e+2),(i===j2||i===z2)&&(n>=48&&n<=57||(n===_s||n===Cs)&&s>=48&&s<=57))for(e+=n===_s||n===Cs?3:2;e<t&&(i=r.charCodeAt(e),!(i<48||i>57));)e+=1;return{number:r.slice(0,e),unit:r.slice(e)}}});var ay=x((uq,sy)=>{u();var V2=Qg(),iy=Kg(),ny=ey();function $t(r){return this instanceof $t?(this.nodes=V2(r),this):new $t(r)}$t.prototype.toString=function(){return Array.isArray(this.nodes)?ny(this.nodes):""};$t.prototype.walk=function(r,e){return iy(this.nodes,r,e),this};$t.unit=ry();$t.walk=iy;$t.stringify=ny;sy.exports=$t});function zl(r){return typeof r=="object"&&r!==null}function H2(r,e){let t=kt(e);do if(t.pop(),(0,Mi.default)(r,t)!==void 0)break;while(t.length);return t.length?t:void 0}function br(r){return typeof r=="string"?r:r.reduce((e,t,i)=>t.includes(".")?`${e}[${t}]`:i===0?t:`${e}.${t}`,"")}function ly(r){return r.map(e=>`'${e}'`).join(", ")}function uy(r){return ly(Object.keys(r))}function Ul(r,e,t,i={}){let n=Array.isArray(e)?br(e):e.replace(/^['"]+|['"]+$/g,""),s=Array.isArray(e)?e:kt(n),a=(0,Mi.default)(r.theme,s,t);if(a===void 0){let l=`'${n}' does not exist in your theme config.`,c=s.slice(0,-1),f=(0,Mi.default)(r.theme,c);if(zl(f)){let d=Object.keys(f).filter(h=>Ul(r,[...c,h]).isValid),p=(0,oy.default)(s[s.length-1],d);p?l+=` Did you mean '${br([...c,p])}'?`:d.length>0&&(l+=` '${br(c)}' has the following valid keys: ${ly(d)}`)}else{let d=H2(r.theme,n);if(d){let p=(0,Mi.default)(r.theme,d);zl(p)?l+=` '${br(d)}' has the following keys: ${uy(p)}`:l+=` '${br(d)}' is not an object.`}else l+=` Your theme has the following top-level keys: ${uy(r.theme)}`}return{isValid:!1,error:l}}if(!(typeof a=="string"||typeof a=="number"||typeof a=="function"||a instanceof String||a instanceof Number||Array.isArray(a))){let l=`'${n}' was found but does not resolve to a string.`;if(zl(a)){let c=Object.keys(a).filter(f=>Ul(r,[...s,f]).isValid);c.length&&(l+=` Did you mean something like '${br([...s,c[0]])}'?`)}return{isValid:!1,error:l}}let[o]=s;return{isValid:!0,value:mt(o)(a,i)}}function W2(r,e,t){e=e.map(n=>fy(r,n,t));let i=[""];for(let n of e)n.type==="div"&&n.value===","?i.push(""):i[i.length-1]+=jl.default.stringify(n);return i}function fy(r,e,t){if(e.type==="function"&&t[e.value]!==void 0){let i=W2(r,e.nodes,t);e.type="word",e.value=t[e.value](r,...i)}return e}function G2(r,e,t){return Object.keys(t).some(n=>e.includes(`${n}(`))?(0,jl.default)(e).walk(n=>{fy(r,n,t)}).toString():e}function*Y2(r){r=r.replace(/^['"]+|['"]+$/g,"");let e=r.match(/^([^\s]+)(?![^\[]*\])(?:\s*\/\s*([^\/\s]+))$/),t;yield[r,void 0],e&&(r=e[1],t=e[2],yield[r,t])}function K2(r,e,t){let i=Array.from(Y2(e)).map(([n,s])=>Object.assign(Ul(r,n,t,{opacityValue:s}),{resolvedPath:n,alpha:s}));return i.find(n=>n.isValid)??i[0]}function cy(r){let e=r.tailwindConfig,t={theme:(i,n,...s)=>{let{isValid:a,value:o,error:l,alpha:c}=K2(e,n,s.length?s:void 0);if(!a){let p=i.parent,h=p?.raws.tailwind?.candidate;if(p&&h!==void 0){r.markInvalidUtilityNode(p),p.remove(),G.warn("invalid-theme-key-in-class",[`The utility \`${h}\` contains an invalid theme value and was not generated.`]);return}throw i.error(l)}let f=Xt(o),d=f!==void 0&&typeof f=="function";return(c!==void 0||d)&&(c===void 0&&(c=1),o=Je(f,c,f)),o},screen:(i,n)=>{n=n.replace(/^['"]+/g,"").replace(/['"]+$/g,"");let a=Rt(e.theme.screens).find(({name:o})=>o===n);if(!a)throw i.error(`The '${n}' screen does not exist in your theme.`);return Tt(a)}};return i=>{i.walk(n=>{let s=Q2[n.type];s!==void 0&&(n[s]=G2(n,n[s],t))})}}var Mi,oy,jl,Q2,py=P(()=>{u();Mi=pe(Ra()),oy=pe(Wg());Ci();jl=pe(ay());Zn();Yn();Yi();Lr();Fr();Be();Q2={atrule:"params",decl:"value"}});function dy({tailwindConfig:{theme:r}}){return function(e){e.walkAtRules("screen",t=>{let i=t.params,s=Rt(r.screens).find(({name:a})=>a===i);if(!s)throw t.error(`No \`${i}\` screen found.`);t.name="media",t.params=Tt(s)})}}var hy=P(()=>{u();Zn();Yn()});function X2(r){let e=r.filter(o=>o.type!=="pseudo"||o.nodes.length>0?!0:o.value.startsWith("::")||[":before",":after",":first-line",":first-letter"].includes(o.value)).reverse(),t=new Set(["tag","class","id","attribute"]),i=e.findIndex(o=>t.has(o.type));if(i===-1)return e.reverse().join("").trim();let n=e[i],s=my[n.type]?my[n.type](n):n;e=e.slice(0,i);let a=e.findIndex(o=>o.type==="combinator"&&o.value===">");return a!==-1&&(e.splice(0,a),e.unshift(Es.default.universal())),[s,...e.reverse()].join("").trim()}function J2(r){return Vl.has(r)||Vl.set(r,Z2.transformSync(r)),Vl.get(r)}function Hl({tailwindConfig:r}){return e=>{let t=new Map,i=new Set;if(e.walkAtRules("defaults",n=>{if(n.nodes&&n.nodes.length>0){i.add(n);return}let s=n.params;t.has(s)||t.set(s,new Set),t.get(s).add(n.parent),n.remove()}),we(r,"optimizeUniversalDefaults"))for(let n of i){let s=new Map,a=t.get(n.params)??[];for(let o of a)for(let l of J2(o.selector)){let c=l.includes(":-")||l.includes("::-")||l.includes(":has")?l:"__DEFAULT__",f=s.get(c)??new Set;s.set(c,f),f.add(l)}if(s.size===0){n.remove();continue}for(let[,o]of s){let l=ee.rule({source:n.source});l.selectors=[...o],l.append(n.nodes.map(c=>c.clone())),n.before(l)}n.remove()}else if(i.size){let n=ee.rule({selectors:["*","::before","::after"]});for(let a of i)n.append(a.nodes),n.parent||a.before(n),n.source||(n.source=a.source),a.remove();let s=n.clone({selectors:["::backdrop"]});n.after(s)}}}var Es,my,Z2,Vl,gy=P(()=>{u();Ot();Es=pe(it());ct();my={id(r){return Es.default.attribute({attribute:"id",operator:"=",value:r.value,quoteMark:'"'})}};Z2=(0,Es.default)(r=>r.map(e=>{let t=e.split(i=>i.type==="combinator"&&i.value===" ").pop();return X2(t)})),Vl=new Map});function Wl(){function r(e){let t=null;e.each(i=>{if(!eO.has(i.type)){t=null;return}if(t===null){t=i;return}let n=yy[i.type];i.type==="atrule"&&i.name==="font-face"?t=i:n.every(s=>(i[s]??"").replace(/\s+/g," ")===(t[s]??"").replace(/\s+/g," "))?(i.nodes&&t.append(i.nodes),i.remove()):t=i}),e.each(i=>{i.type==="atrule"&&r(i)})}return e=>{r(e)}}var yy,eO,by=P(()=>{u();yy={atrule:["name","params"],rule:["selector"]},eO=new Set(Object.keys(yy))});function Gl(){return r=>{r.walkRules(e=>{let t=new Map,i=new Set([]),n=new Map;e.walkDecls(s=>{if(s.parent===e){if(t.has(s.prop)){if(t.get(s.prop).value===s.value){i.add(t.get(s.prop)),t.set(s.prop,s);return}n.has(s.prop)||n.set(s.prop,new Set),n.get(s.prop).add(t.get(s.prop)),n.get(s.prop).add(s)}t.set(s.prop,s)}});for(let s of i)s.remove();for(let s of n.values()){let a=new Map;for(let o of s){let l=rO(o.value);l!==null&&(a.has(l)||a.set(l,new Set),a.get(l).add(o))}for(let o of a.values()){let l=Array.from(o).slice(0,-1);for(let c of l)c.remove()}}})}}function rO(r){let e=/^-?\d*.?\d+([\w%]+)?$/g.exec(r);return e?e[1]??tO:null}var tO,wy=P(()=>{u();tO=Symbol("unitless-number")});function iO(r){if(!r.walkAtRules)return;let e=new Set;if(r.walkAtRules("apply",t=>{e.add(t.parent)}),e.size!==0)for(let t of e){let i=[],n=[];for(let s of t.nodes)s.type==="atrule"&&s.name==="apply"?(n.length>0&&(i.push(n),n=[]),i.push([s])):n.push(s);if(n.length>0&&i.push(n),i.length!==1){for(let s of[...i].reverse()){let a=t.clone({nodes:[]});a.append(s),t.after(a)}t.remove()}}}function Os(){return r=>{iO(r)}}var vy=P(()=>{u()});function Ts(r){return async function(e,t){let{tailwindDirectives:i,applyDirectives:n}=Ol(e);Os()(e,t);let s=r({tailwindDirectives:i,applyDirectives:n,registerDependency(a){t.messages.push({plugin:"tailwindcss",parent:t.opts.from,...a})},createContext(a,o){return il(a,o,e)}})(e,t);if(s.tailwindConfig.separator==="-")throw new Error("The '-' character cannot be used as a custom separator in JIT mode due to parsing ambiguity. Please use another character like '_' instead.");Rf(s.tailwindConfig),await Pl(s)(e,t),Os()(e,t),Dl(s)(e,t),cy(s)(e,t),dy(s)(e,t),Hl(s)(e,t),Wl(s)(e,t),Gl(s)(e,t)}}var xy=P(()=>{u();Og();Bg();Hg();py();hy();gy();by();wy();vy();Oi();ct()});function ky(r,e){let t=null,i=null;return r.walkAtRules("config",n=>{if(i=n.source?.input.file??e.opts.from??null,i===null)throw n.error("The `@config` directive cannot be used without setting `from` in your PostCSS config.");if(t)throw n.error("Only one `@config` directive is allowed per file.");let s=n.params.match(/(['"])(.*?)\1/);if(!s)throw n.error("A path is required when using the `@config` directive.");let a=s[2];if(me.isAbsolute(a))throw n.error("The `@config` directive cannot be used with an absolute path.");if(t=me.resolve(me.dirname(i),a),!be.existsSync(t))throw n.error(`The config file at "${a}" does not exist. Make sure the path is correct and the file exists.`);n.remove()}),t||null}var Sy=P(()=>{u();ft();et()});var Ay=x((Wq,Ql)=>{u();Eg();xy();It();Sy();Ql.exports=function(e){return{postcssPlugin:"tailwindcss",plugins:[Ze.DEBUG&&function(t){return console.log(`
`),console.time("JIT TOTAL"),t},async function(t,i){e=ky(t,i)??e;let n=El(e);if(t.type==="document"){let s=t.nodes.filter(a=>a.type==="root");for(let a of s)a.type==="root"&&await Ts(n)(a,i);return}await Ts(n)(t,i)},Ze.DEBUG&&function(t){return console.timeEnd("JIT TOTAL"),console.log(`
`),t}].filter(Boolean)}};Ql.exports.postcss=!0});var _y=x((Gq,Cy)=>{u();Cy.exports=Ay()});var Yl=x((Qq,Ey)=>{u();Ey.exports=()=>["and_chr 114","and_uc 15.5","chrome 114","chrome 113","chrome 109","edge 114","firefox 114","ios_saf 16.5","ios_saf 16.4","ios_saf 16.3","ios_saf 16.1","opera 99","safari 16.5","samsung 21"]});var Rs={};Ge(Rs,{agents:()=>nO,feature:()=>sO});function sO(){return{status:"cr",title:"CSS Feature Queries",stats:{ie:{"6":"n","7":"n","8":"n","9":"n","10":"n","11":"n","5.5":"n"},edge:{"12":"y","13":"y","14":"y","15":"y","16":"y","17":"y","18":"y","79":"y","80":"y","81":"y","83":"y","84":"y","85":"y","86":"y","87":"y","88":"y","89":"y","90":"y","91":"y","92":"y","93":"y","94":"y","95":"y","96":"y","97":"y","98":"y","99":"y","100":"y","101":"y","102":"y","103":"y","104":"y","105":"y","106":"y","107":"y","108":"y","109":"y","110":"y","111":"y","112":"y","113":"y","114":"y"},firefox:{"2":"n","3":"n","4":"n","5":"n","6":"n","7":"n","8":"n","9":"n","10":"n","11":"n","12":"n","13":"n","14":"n","15":"n","16":"n","17":"n","18":"n","19":"n","20":"n","21":"n","22":"y","23":"y","24":"y","25":"y","26":"y","27":"y","28":"y","29":"y","30":"y","31":"y","32":"y","33":"y","34":"y","35":"y","36":"y","37":"y","38":"y","39":"y","40":"y","41":"y","42":"y","43":"y","44":"y","45":"y","46":"y","47":"y","48":"y","49":"y","50":"y","51":"y","52":"y","53":"y","54":"y","55":"y","56":"y","57":"y","58":"y","59":"y","60":"y","61":"y","62":"y","63":"y","64":"y","65":"y","66":"y","67":"y","68":"y","69":"y","70":"y","71":"y","72":"y","73":"y","74":"y","75":"y","76":"y","77":"y","78":"y","79":"y","80":"y","81":"y","82":"y","83":"y","84":"y","85":"y","86":"y","87":"y","88":"y","89":"y","90":"y","91":"y","92":"y","93":"y","94":"y","95":"y","96":"y","97":"y","98":"y","99":"y","100":"y","101":"y","102":"y","103":"y","104":"y","105":"y","106":"y","107":"y","108":"y","109":"y","110":"y","111":"y","112":"y","113":"y","114":"y","115":"y","116":"y","117":"y","3.5":"n","3.6":"n"},chrome:{"4":"n","5":"n","6":"n","7":"n","8":"n","9":"n","10":"n","11":"n","12":"n","13":"n","14":"n","15":"n","16":"n","17":"n","18":"n","19":"n","20":"n","21":"n","22":"n","23":"n","24":"n","25":"n","26":"n","27":"n","28":"y","29":"y","30":"y","31":"y","32":"y","33":"y","34":"y","35":"y","36":"y","37":"y","38":"y","39":"y","40":"y","41":"y","42":"y","43":"y","44":"y","45":"y","46":"y","47":"y","48":"y","49":"y","50":"y","51":"y","52":"y","53":"y","54":"y","55":"y","56":"y","57":"y","58":"y","59":"y","60":"y","61":"y","62":"y","63":"y","64":"y","65":"y","66":"y","67":"y","68":"y","69":"y","70":"y","71":"y","72":"y","73":"y","74":"y","75":"y","76":"y","77":"y","78":"y","79":"y","80":"y","81":"y","83":"y","84":"y","85":"y","86":"y","87":"y","88":"y","89":"y","90":"y","91":"y","92":"y","93":"y","94":"y","95":"y","96":"y","97":"y","98":"y","99":"y","100":"y","101":"y","102":"y","103":"y","104":"y","105":"y","106":"y","107":"y","108":"y","109":"y","110":"y","111":"y","112":"y","113":"y","114":"y","115":"y","116":"y","117":"y"},safari:{"4":"n","5":"n","6":"n","7":"n","8":"n","9":"y","10":"y","11":"y","12":"y","13":"y","14":"y","15":"y","17":"y","9.1":"y","10.1":"y","11.1":"y","12.1":"y","13.1":"y","14.1":"y","15.1":"y","15.2-15.3":"y","15.4":"y","15.5":"y","15.6":"y","16.0":"y","16.1":"y","16.2":"y","16.3":"y","16.4":"y","16.5":"y","16.6":"y",TP:"y","3.1":"n","3.2":"n","5.1":"n","6.1":"n","7.1":"n"},opera:{"9":"n","11":"n","12":"n","15":"y","16":"y","17":"y","18":"y","19":"y","20":"y","21":"y","22":"y","23":"y","24":"y","25":"y","26":"y","27":"y","28":"y","29":"y","30":"y","31":"y","32":"y","33":"y","34":"y","35":"y","36":"y","37":"y","38":"y","39":"y","40":"y","41":"y","42":"y","43":"y","44":"y","45":"y","46":"y","47":"y","48":"y","49":"y","50":"y","51":"y","52":"y","53":"y","54":"y","55":"y","56":"y","57":"y","58":"y","60":"y","62":"y","63":"y","64":"y","65":"y","66":"y","67":"y","68":"y","69":"y","70":"y","71":"y","72":"y","73":"y","74":"y","75":"y","76":"y","77":"y","78":"y","79":"y","80":"y","81":"y","82":"y","83":"y","84":"y","85":"y","86":"y","87":"y","88":"y","89":"y","90":"y","91":"y","92":"y","93":"y","94":"y","95":"y","96":"y","97":"y","98":"y","99":"y","100":"y","12.1":"y","9.5-9.6":"n","10.0-10.1":"n","10.5":"n","10.6":"n","11.1":"n","11.5":"n","11.6":"n"},ios_saf:{"8":"n","17":"y","9.0-9.2":"y","9.3":"y","10.0-10.2":"y","10.3":"y","11.0-11.2":"y","11.3-11.4":"y","12.0-12.1":"y","12.2-12.5":"y","13.0-13.1":"y","13.2":"y","13.3":"y","13.4-13.7":"y","14.0-14.4":"y","14.5-14.8":"y","15.0-15.1":"y","15.2-15.3":"y","15.4":"y","15.5":"y","15.6":"y","16.0":"y","16.1":"y","16.2":"y","16.3":"y","16.4":"y","16.5":"y","16.6":"y","3.2":"n","4.0-4.1":"n","4.2-4.3":"n","5.0-5.1":"n","6.0-6.1":"n","7.0-7.1":"n","8.1-8.4":"n"},op_mini:{all:"y"},android:{"3":"n","4":"n","114":"y","4.4":"y","4.4.3-4.4.4":"y","2.1":"n","2.2":"n","2.3":"n","4.1":"n","4.2-4.3":"n"},bb:{"7":"n","10":"n"},op_mob:{"10":"n","11":"n","12":"n","73":"y","11.1":"n","11.5":"n","12.1":"n"},and_chr:{"114":"y"},and_ff:{"115":"y"},ie_mob:{"10":"n","11":"n"},and_uc:{"15.5":"y"},samsung:{"4":"y","20":"y","21":"y","5.0-5.4":"y","6.2-6.4":"y","7.2-7.4":"y","8.2":"y","9.2":"y","10.1":"y","11.1-11.2":"y","12.0":"y","13.0":"y","14.0":"y","15.0":"y","16.0":"y","17.0":"y","18.0":"y","19.0":"y"},and_qq:{"13.1":"y"},baidu:{"13.18":"y"},kaios:{"2.5":"y","3.0-3.1":"y"}}}}var nO,Ps=P(()=>{u();nO={ie:{prefix:"ms"},edge:{prefix:"webkit",prefix_exceptions:{"12":"ms","13":"ms","14":"ms","15":"ms","16":"ms","17":"ms","18":"ms"}},firefox:{prefix:"moz"},chrome:{prefix:"webkit"},safari:{prefix:"webkit"},opera:{prefix:"webkit",prefix_exceptions:{"9":"o","11":"o","12":"o","9.5-9.6":"o","10.0-10.1":"o","10.5":"o","10.6":"o","11.1":"o","11.5":"o","11.6":"o","12.1":"o"}},ios_saf:{prefix:"webkit"},op_mini:{prefix:"o"},android:{prefix:"webkit"},bb:{prefix:"webkit"},op_mob:{prefix:"o",prefix_exceptions:{"73":"webkit"}},and_chr:{prefix:"webkit"},and_ff:{prefix:"moz"},ie_mob:{prefix:"ms"},and_uc:{prefix:"webkit",prefix_exceptions:{"15.5":"webkit"}},samsung:{prefix:"webkit"},and_qq:{prefix:"webkit"},baidu:{prefix:"webkit"},kaios:{prefix:"moz"}}});var Oy=x(()=>{u()});var _e=x((Xq,Lt)=>{u();var{list:Kl}=$e();Lt.exports.error=function(r){let e=new Error(r);throw e.autoprefixer=!0,e};Lt.exports.uniq=function(r){return[...new Set(r)]};Lt.exports.removeNote=function(r){return r.includes(" ")?r.split(" ")[0]:r};Lt.exports.escapeRegexp=function(r){return r.replace(/[$()*+-.?[\\\]^{|}]/g,"\\$&")};Lt.exports.regexp=function(r,e=!0){return e&&(r=this.escapeRegexp(r)),new RegExp(`(^|[\\s,(])(${r}($|[\\s(,]))`,"gi")};Lt.exports.editList=function(r,e){let t=Kl.comma(r),i=e(t,[]);if(t===i)return r;let n=r.match(/,\s*/);return n=n?n[0]:", ",i.join(n)};Lt.exports.splitSelector=function(r){return Kl.comma(r).map(e=>Kl.space(e).map(t=>t.split(/(?=\.|#)/g)))}});var Mt=x((Zq,Py)=>{u();var aO=Yl(),Ty=(Ps(),Rs).agents,oO=_e(),Ry=class{static prefixes(){if(this.prefixesCache)return this.prefixesCache;this.prefixesCache=[];for(let e in Ty)this.prefixesCache.push(`-${Ty[e].prefix}-`);return this.prefixesCache=oO.uniq(this.prefixesCache).sort((e,t)=>t.length-e.length),this.prefixesCache}static withPrefix(e){return this.prefixesRegexp||(this.prefixesRegexp=new RegExp(this.prefixes().join("|"))),this.prefixesRegexp.test(e)}constructor(e,t,i,n){this.data=e,this.options=i||{},this.browserslistOpts=n||{},this.selected=this.parse(t)}parse(e){let t={};for(let i in this.browserslistOpts)t[i]=this.browserslistOpts[i];return t.path=this.options.from,aO(e,t)}prefix(e){let[t,i]=e.split(" "),n=this.data[t],s=n.prefix_exceptions&&n.prefix_exceptions[i];return s||(s=n.prefix),`-${s}-`}isSelected(e){return this.selected.includes(e)}};Py.exports=Ry});var Ni=x((Jq,Iy)=>{u();Iy.exports={prefix(r){let e=r.match(/^(-\w+-)/);return e?e[0]:""},unprefixed(r){return r.replace(/^-\w+-/,"")}}});var wr=x((e$,qy)=>{u();var lO=Mt(),Dy=Ni(),uO=_e();function Xl(r,e){let t=new r.constructor;for(let i of Object.keys(r||{})){let n=r[i];i==="parent"&&typeof n=="object"?e&&(t[i]=e):i==="source"||i===null?t[i]=n:Array.isArray(n)?t[i]=n.map(s=>Xl(s,t)):i!=="_autoprefixerPrefix"&&i!=="_autoprefixerValues"&&i!=="proxyCache"&&(typeof n=="object"&&n!==null&&(n=Xl(n,t)),t[i]=n)}return t}var Is=class{static hack(e){return this.hacks||(this.hacks={}),e.names.map(t=>(this.hacks[t]=e,this.hacks[t]))}static load(e,t,i){let n=this.hacks&&this.hacks[e];return n?new n(e,t,i):new this(e,t,i)}static clone(e,t){let i=Xl(e);for(let n in t)i[n]=t[n];return i}constructor(e,t,i){this.prefixes=t,this.name=e,this.all=i}parentPrefix(e){let t;return typeof e._autoprefixerPrefix!="undefined"?t=e._autoprefixerPrefix:e.type==="decl"&&e.prop[0]==="-"?t=Dy.prefix(e.prop):e.type==="root"?t=!1:e.type==="rule"&&e.selector.includes(":-")&&/:(-\w+-)/.test(e.selector)?t=e.selector.match(/:(-\w+-)/)[1]:e.type==="atrule"&&e.name[0]==="-"?t=Dy.prefix(e.name):t=this.parentPrefix(e.parent),lO.prefixes().includes(t)||(t=!1),e._autoprefixerPrefix=t,e._autoprefixerPrefix}process(e,t){if(!this.check(e))return;let i=this.parentPrefix(e),n=this.prefixes.filter(a=>!i||i===uO.removeNote(a)),s=[];for(let a of n)this.add(e,a,s.concat([a]),t)&&s.push(a);return s}clone(e,t){return Is.clone(e,t)}};qy.exports=Is});var j=x((t$,My)=>{u();var fO=wr(),cO=Mt(),$y=_e(),Ly=class extends fO{check(){return!0}prefixed(e,t){return t+e}normalize(e){return e}otherPrefixes(e,t){for(let i of cO.prefixes())if(i!==t&&e.includes(i))return!0;return!1}set(e,t){return e.prop=this.prefixed(e.prop,t),e}needCascade(e){return e._autoprefixerCascade||(e._autoprefixerCascade=this.all.options.cascade!==!1&&e.raw("before").includes(`
`)),e._autoprefixerCascade}maxPrefixed(e,t){if(t._autoprefixerMax)return t._autoprefixerMax;let i=0;for(let n of e)n=$y.removeNote(n),n.length>i&&(i=n.length);return t._autoprefixerMax=i,t._autoprefixerMax}calcBefore(e,t,i=""){let s=this.maxPrefixed(e,t)-$y.removeNote(i).length,a=t.raw("before");return s>0&&(a+=Array(s).fill(" ").join("")),a}restoreBefore(e){let t=e.raw("before").split(`
`),i=t[t.length-1];this.all.group(e).up(n=>{let s=n.raw("before").split(`
`),a=s[s.length-1];a.length<i.length&&(i=a)}),t[t.length-1]=i,e.raws.before=t.join(`
`)}insert(e,t,i){let n=this.set(this.clone(e),t);if(!(!n||e.parent.some(a=>a.prop===n.prop&&a.value===n.value)))return this.needCascade(e)&&(n.raws.before=this.calcBefore(i,e,t)),e.parent.insertBefore(e,n)}isAlready(e,t){let i=this.all.group(e).up(n=>n.prop===t);return i||(i=this.all.group(e).down(n=>n.prop===t)),i}add(e,t,i,n){let s=this.prefixed(e.prop,t);if(!(this.isAlready(e,s)||this.otherPrefixes(e.value,t)))return this.insert(e,t,i,n)}process(e,t){if(!this.needCascade(e)){super.process(e,t);return}let i=super.process(e,t);!i||!i.length||(this.restoreBefore(e),e.raws.before=this.calcBefore(i,e))}old(e,t){return[this.prefixed(e,t)]}};My.exports=Ly});var By=x((r$,Ny)=>{u();Ny.exports=function r(e){return{mul:t=>new r(e*t),div:t=>new r(e/t),simplify:()=>new r(e),toString:()=>e.toString()}}});var zy=x((i$,jy)=>{u();var pO=By(),dO=wr(),Zl=_e(),hO=/(min|max)-resolution\s*:\s*\d*\.?\d+(dppx|dpcm|dpi|x)/gi,mO=/(min|max)-resolution(\s*:\s*)(\d*\.?\d+)(dppx|dpcm|dpi|x)/i,Fy=class extends dO{prefixName(e,t){return e==="-moz-"?t+"--moz-device-pixel-ratio":e+t+"-device-pixel-ratio"}prefixQuery(e,t,i,n,s){return n=new pO(n),s==="dpi"?n=n.div(96):s==="dpcm"&&(n=n.mul(2.54).div(96)),n=n.simplify(),e==="-o-"&&(n=n.n+"/"+n.d),this.prefixName(e,t)+i+n}clean(e){if(!this.bad){this.bad=[];for(let t of this.prefixes)this.bad.push(this.prefixName(t,"min")),this.bad.push(this.prefixName(t,"max"))}e.params=Zl.editList(e.params,t=>t.filter(i=>this.bad.every(n=>!i.includes(n))))}process(e){let t=this.parentPrefix(e),i=t?[t]:this.prefixes;e.params=Zl.editList(e.params,(n,s)=>{for(let a of n){if(!a.includes("min-resolution")&&!a.includes("max-resolution")){s.push(a);continue}for(let o of i){let l=a.replace(hO,c=>{let f=c.match(mO);return this.prefixQuery(o,f[1],f[2],f[3],f[4])});s.push(l)}s.push(a)}return Zl.uniq(s)})}};jy.exports=Fy});var Vy=x((n$,Uy)=>{u();var Jl="(".charCodeAt(0),eu=")".charCodeAt(0),Ds="'".charCodeAt(0),tu='"'.charCodeAt(0),ru="\\".charCodeAt(0),vr="/".charCodeAt(0),iu=",".charCodeAt(0),nu=":".charCodeAt(0),qs="*".charCodeAt(0),gO="u".charCodeAt(0),yO="U".charCodeAt(0),bO="+".charCodeAt(0),wO=/^[a-f0-9?-]+$/i;Uy.exports=function(r){for(var e=[],t=r,i,n,s,a,o,l,c,f,d=0,p=t.charCodeAt(d),h=t.length,b=[{nodes:e}],v=0,y,w="",k="",S="";d<h;)if(p<=32){i=d;do i+=1,p=t.charCodeAt(i);while(p<=32);a=t.slice(d,i),s=e[e.length-1],p===eu&&v?S=a:s&&s.type==="div"?(s.after=a,s.sourceEndIndex+=a.length):p===iu||p===nu||p===vr&&t.charCodeAt(i+1)!==qs&&(!y||y&&y.type==="function"&&y.value!=="calc")?k=a:e.push({type:"space",sourceIndex:d,sourceEndIndex:i,value:a}),d=i}else if(p===Ds||p===tu){i=d,n=p===Ds?"'":'"',a={type:"string",sourceIndex:d,quote:n};do if(o=!1,i=t.indexOf(n,i+1),~i)for(l=i;t.charCodeAt(l-1)===ru;)l-=1,o=!o;else t+=n,i=t.length-1,a.unclosed=!0;while(o);a.value=t.slice(d+1,i),a.sourceEndIndex=a.unclosed?i:i+1,e.push(a),d=i+1,p=t.charCodeAt(d)}else if(p===vr&&t.charCodeAt(d+1)===qs)i=t.indexOf("*/",d),a={type:"comment",sourceIndex:d,sourceEndIndex:i+2},i===-1&&(a.unclosed=!0,i=t.length,a.sourceEndIndex=i),a.value=t.slice(d+2,i),e.push(a),d=i+2,p=t.charCodeAt(d);else if((p===vr||p===qs)&&y&&y.type==="function"&&y.value==="calc")a=t[d],e.push({type:"word",sourceIndex:d-k.length,sourceEndIndex:d+a.length,value:a}),d+=1,p=t.charCodeAt(d);else if(p===vr||p===iu||p===nu)a=t[d],e.push({type:"div",sourceIndex:d-k.length,sourceEndIndex:d+a.length,value:a,before:k,after:""}),k="",d+=1,p=t.charCodeAt(d);else if(Jl===p){i=d;do i+=1,p=t.charCodeAt(i);while(p<=32);if(f=d,a={type:"function",sourceIndex:d-w.length,value:w,before:t.slice(f+1,i)},d=i,w==="url"&&p!==Ds&&p!==tu){i-=1;do if(o=!1,i=t.indexOf(")",i+1),~i)for(l=i;t.charCodeAt(l-1)===ru;)l-=1,o=!o;else t+=")",i=t.length-1,a.unclosed=!0;while(o);c=i;do c-=1,p=t.charCodeAt(c);while(p<=32);f<c?(d!==c+1?a.nodes=[{type:"word",sourceIndex:d,sourceEndIndex:c+1,value:t.slice(d,c+1)}]:a.nodes=[],a.unclosed&&c+1!==i?(a.after="",a.nodes.push({type:"space",sourceIndex:c+1,sourceEndIndex:i,value:t.slice(c+1,i)})):(a.after=t.slice(c+1,i),a.sourceEndIndex=i)):(a.after="",a.nodes=[]),d=i+1,a.sourceEndIndex=a.unclosed?i:d,p=t.charCodeAt(d),e.push(a)}else v+=1,a.after="",a.sourceEndIndex=d+1,e.push(a),b.push(a),e=a.nodes=[],y=a;w=""}else if(eu===p&&v)d+=1,p=t.charCodeAt(d),y.after=S,y.sourceEndIndex+=S.length,S="",v-=1,b[b.length-1].sourceEndIndex=d,b.pop(),y=b[v],e=y.nodes;else{i=d;do p===ru&&(i+=1),i+=1,p=t.charCodeAt(i);while(i<h&&!(p<=32||p===Ds||p===tu||p===iu||p===nu||p===vr||p===Jl||p===qs&&y&&y.type==="function"&&y.value==="calc"||p===vr&&y.type==="function"&&y.value==="calc"||p===eu&&v));a=t.slice(d,i),Jl===p?w=a:(gO===a.charCodeAt(0)||yO===a.charCodeAt(0))&&bO===a.charCodeAt(1)&&wO.test(a.slice(2))?e.push({type:"unicode-range",sourceIndex:d,sourceEndIndex:i,value:a}):e.push({type:"word",sourceIndex:d,sourceEndIndex:i,value:a}),d=i}for(d=b.length-1;d;d-=1)b[d].unclosed=!0,b[d].sourceEndIndex=t.length;return b[0].nodes}});var Wy=x((s$,Hy)=>{u();Hy.exports=function r(e,t,i){var n,s,a,o;for(n=0,s=e.length;n<s;n+=1)a=e[n],i||(o=t(a,n,e)),o!==!1&&a.type==="function"&&Array.isArray(a.nodes)&&r(a.nodes,t,i),i&&t(a,n,e)}});var Ky=x((a$,Yy)=>{u();function Gy(r,e){var t=r.type,i=r.value,n,s;return e&&(s=e(r))!==void 0?s:t==="word"||t==="space"?i:t==="string"?(n=r.quote||"",n+i+(r.unclosed?"":n)):t==="comment"?"/*"+i+(r.unclosed?"":"*/"):t==="div"?(r.before||"")+i+(r.after||""):Array.isArray(r.nodes)?(n=Qy(r.nodes,e),t!=="function"?n:i+"("+(r.before||"")+n+(r.after||"")+(r.unclosed?"":")")):i}function Qy(r,e){var t,i;if(Array.isArray(r)){for(t="",i=r.length-1;~i;i-=1)t=Gy(r[i],e)+t;return t}return Gy(r,e)}Yy.exports=Qy});var Zy=x((o$,Xy)=>{u();var $s="-".charCodeAt(0),Ls="+".charCodeAt(0),su=".".charCodeAt(0),vO="e".charCodeAt(0),xO="E".charCodeAt(0);function kO(r){var e=r.charCodeAt(0),t;if(e===Ls||e===$s){if(t=r.charCodeAt(1),t>=48&&t<=57)return!0;var i=r.charCodeAt(2);return t===su&&i>=48&&i<=57}return e===su?(t=r.charCodeAt(1),t>=48&&t<=57):e>=48&&e<=57}Xy.exports=function(r){var e=0,t=r.length,i,n,s;if(t===0||!kO(r))return!1;for(i=r.charCodeAt(e),(i===Ls||i===$s)&&e++;e<t&&(i=r.charCodeAt(e),!(i<48||i>57));)e+=1;if(i=r.charCodeAt(e),n=r.charCodeAt(e+1),i===su&&n>=48&&n<=57)for(e+=2;e<t&&(i=r.charCodeAt(e),!(i<48||i>57));)e+=1;if(i=r.charCodeAt(e),n=r.charCodeAt(e+1),s=r.charCodeAt(e+2),(i===vO||i===xO)&&(n>=48&&n<=57||(n===Ls||n===$s)&&s>=48&&s<=57))for(e+=n===Ls||n===$s?3:2;e<t&&(i=r.charCodeAt(e),!(i<48||i>57));)e+=1;return{number:r.slice(0,e),unit:r.slice(e)}}});var Ms=x((l$,tb)=>{u();var SO=Vy(),Jy=Wy(),eb=Ky();function Nt(r){return this instanceof Nt?(this.nodes=SO(r),this):new Nt(r)}Nt.prototype.toString=function(){return Array.isArray(this.nodes)?eb(this.nodes):""};Nt.prototype.walk=function(r,e){return Jy(this.nodes,r,e),this};Nt.unit=Zy();Nt.walk=Jy;Nt.stringify=eb;tb.exports=Nt});var ab=x((u$,sb)=>{u();var{list:AO}=$e(),rb=Ms(),CO=Mt(),ib=Ni(),nb=class{constructor(e){this.props=["transition","transition-property"],this.prefixes=e}add(e,t){let i,n,s=this.prefixes.add[e.prop],a=this.ruleVendorPrefixes(e),o=a||s&&s.prefixes||[],l=this.parse(e.value),c=l.map(h=>this.findProp(h)),f=[];if(c.some(h=>h[0]==="-"))return;for(let h of l){if(n=this.findProp(h),n[0]==="-")continue;let b=this.prefixes.add[n];if(!(!b||!b.prefixes))for(i of b.prefixes){if(a&&!a.some(y=>i.includes(y)))continue;let v=this.prefixes.prefixed(n,i);v!=="-ms-transform"&&!c.includes(v)&&(this.disabled(n,i)||f.push(this.clone(n,v,h)))}}l=l.concat(f);let d=this.stringify(l),p=this.stringify(this.cleanFromUnprefixed(l,"-webkit-"));if(o.includes("-webkit-")&&this.cloneBefore(e,`-webkit-${e.prop}`,p),this.cloneBefore(e,e.prop,p),o.includes("-o-")){let h=this.stringify(this.cleanFromUnprefixed(l,"-o-"));this.cloneBefore(e,`-o-${e.prop}`,h)}for(i of o)if(i!=="-webkit-"&&i!=="-o-"){let h=this.stringify(this.cleanOtherPrefixes(l,i));this.cloneBefore(e,i+e.prop,h)}d!==e.value&&!this.already(e,e.prop,d)&&(this.checkForWarning(t,e),e.cloneBefore(),e.value=d)}findProp(e){let t=e[0].value;if(/^\d/.test(t)){for(let[i,n]of e.entries())if(i!==0&&n.type==="word")return n.value}return t}already(e,t,i){return e.parent.some(n=>n.prop===t&&n.value===i)}cloneBefore(e,t,i){this.already(e,t,i)||e.cloneBefore({prop:t,value:i})}checkForWarning(e,t){if(t.prop!=="transition-property")return;let i=!1,n=!1;t.parent.each(s=>{if(s.type!=="decl"||s.prop.indexOf("transition-")!==0)return;let a=AO.comma(s.value);if(s.prop==="transition-property"){a.forEach(o=>{let l=this.prefixes.add[o];l&&l.prefixes&&l.prefixes.length>0&&(i=!0)});return}return n=n||a.length>1,!1}),i&&n&&t.warn(e,"Replace transition-property to transition, because Autoprefixer could not support any cases of transition-property and other transition-*")}remove(e){let t=this.parse(e.value);t=t.filter(a=>{let o=this.prefixes.remove[this.findProp(a)];return!o||!o.remove});let i=this.stringify(t);if(e.value===i)return;if(t.length===0){e.remove();return}let n=e.parent.some(a=>a.prop===e.prop&&a.value===i),s=e.parent.some(a=>a!==e&&a.prop===e.prop&&a.value.length>i.length);if(n||s){e.remove();return}e.value=i}parse(e){let t=rb(e),i=[],n=[];for(let s of t.nodes)n.push(s),s.type==="div"&&s.value===","&&(i.push(n),n=[]);return i.push(n),i.filter(s=>s.length>0)}stringify(e){if(e.length===0)return"";let t=[];for(let i of e)i[i.length-1].type!=="div"&&i.push(this.div(e)),t=t.concat(i);return t[0].type==="div"&&(t=t.slice(1)),t[t.length-1].type==="div"&&(t=t.slice(0,-2+1||void 0)),rb.stringify({nodes:t})}clone(e,t,i){let n=[],s=!1;for(let a of i)!s&&a.type==="word"&&a.value===e?(n.push({type:"word",value:t}),s=!0):n.push(a);return n}div(e){for(let t of e)for(let i of t)if(i.type==="div"&&i.value===",")return i;return{type:"div",value:",",after:" "}}cleanOtherPrefixes(e,t){return e.filter(i=>{let n=ib.prefix(this.findProp(i));return n===""||n===t})}cleanFromUnprefixed(e,t){let i=e.map(s=>this.findProp(s)).filter(s=>s.slice(0,t.length)===t).map(s=>this.prefixes.unprefixed(s)),n=[];for(let s of e){let a=this.findProp(s),o=ib.prefix(a);!i.includes(a)&&(o===t||o==="")&&n.push(s)}return n}disabled(e,t){let i=["order","justify-content","align-self","align-content"];if(e.includes("flex")||i.includes(e)){if(this.prefixes.options.flexbox===!1)return!0;if(this.prefixes.options.flexbox==="no-2009")return t.includes("2009")}}ruleVendorPrefixes(e){let{parent:t}=e;if(t.type!=="rule")return!1;if(!t.selector.includes(":-"))return!1;let i=CO.prefixes().filter(n=>t.selector.includes(":"+n));return i.length>0?i:!1}};sb.exports=nb});var xr=x((f$,lb)=>{u();var _O=_e(),ob=class{constructor(e,t,i,n){this.unprefixed=e,this.prefixed=t,this.string=i||t,this.regexp=n||_O.regexp(t)}check(e){return e.includes(this.string)?!!e.match(this.regexp):!1}};lb.exports=ob});var He=x((c$,fb)=>{u();var EO=wr(),OO=xr(),TO=Ni(),RO=_e(),ub=class extends EO{static save(e,t){let i=t.prop,n=[];for(let s in t._autoprefixerValues){let a=t._autoprefixerValues[s];if(a===t.value)continue;let o,l=TO.prefix(i);if(l==="-pie-")continue;if(l===s){o=t.value=a,n.push(o);continue}let c=e.prefixed(i,s),f=t.parent;if(!f.every(b=>b.prop!==c)){n.push(o);continue}let d=a.replace(/\s+/," ");if(f.some(b=>b.prop===t.prop&&b.value.replace(/\s+/," ")===d)){n.push(o);continue}let h=this.clone(t,{value:a});o=t.parent.insertBefore(t,h),n.push(o)}return n}check(e){let t=e.value;return t.includes(this.name)?!!t.match(this.regexp()):!1}regexp(){return this.regexpCache||(this.regexpCache=RO.regexp(this.name))}replace(e,t){return e.replace(this.regexp(),`$1${t}$2`)}value(e){return e.raws.value&&e.raws.value.value===e.value?e.raws.value.raw:e.value}add(e,t){e._autoprefixerValues||(e._autoprefixerValues={});let i=e._autoprefixerValues[t]||this.value(e),n;do if(n=i,i=this.replace(i,t),i===!1)return;while(i!==n);e._autoprefixerValues[t]=i}old(e){return new OO(this.name,e+this.name)}};fb.exports=ub});var Bt=x((p$,cb)=>{u();cb.exports={}});var ou=x((d$,hb)=>{u();var pb=Ms(),PO=He(),IO=Bt().insertAreas,DO=/(^|[^-])linear-gradient\(\s*(top|left|right|bottom)/i,qO=/(^|[^-])radial-gradient\(\s*\d+(\w*|%)\s+\d+(\w*|%)\s*,/i,$O=/(!\s*)?autoprefixer:\s*ignore\s+next/i,LO=/(!\s*)?autoprefixer\s*grid:\s*(on|off|(no-)?autoplace)/i,MO=["width","height","min-width","max-width","min-height","max-height","inline-size","min-inline-size","max-inline-size","block-size","min-block-size","max-block-size"];function au(r){return r.parent.some(e=>e.prop==="grid-template"||e.prop==="grid-template-areas")}function NO(r){let e=r.parent.some(i=>i.prop==="grid-template-rows"),t=r.parent.some(i=>i.prop==="grid-template-columns");return e&&t}var db=class{constructor(e){this.prefixes=e}add(e,t){let i=this.prefixes.add["@resolution"],n=this.prefixes.add["@keyframes"],s=this.prefixes.add["@viewport"],a=this.prefixes.add["@supports"];e.walkAtRules(f=>{if(f.name==="keyframes"){if(!this.disabled(f,t))return n&&n.process(f)}else if(f.name==="viewport"){if(!this.disabled(f,t))return s&&s.process(f)}else if(f.name==="supports"){if(this.prefixes.options.supports!==!1&&!this.disabled(f,t))return a.process(f)}else if(f.name==="media"&&f.params.includes("-resolution")&&!this.disabled(f,t))return i&&i.process(f)}),e.walkRules(f=>{if(!this.disabled(f,t))return this.prefixes.add.selectors.map(d=>d.process(f,t))});function o(f){return f.parent.nodes.some(d=>{if(d.type!=="decl")return!1;let p=d.prop==="display"&&/(inline-)?grid/.test(d.value),h=d.prop.startsWith("grid-template"),b=/^grid-([A-z]+-)?gap/.test(d.prop);return p||h||b})}function l(f){return f.parent.some(d=>d.prop==="display"&&/(inline-)?flex/.test(d.value))}let c=this.gridStatus(e,t)&&this.prefixes.add["grid-area"]&&this.prefixes.add["grid-area"].prefixes;return e.walkDecls(f=>{if(this.disabledDecl(f,t))return;let d=f.parent,p=f.prop,h=f.value;if(p==="grid-row-span"){t.warn("grid-row-span is not part of final Grid Layout. Use grid-row.",{node:f});return}else if(p==="grid-column-span"){t.warn("grid-column-span is not part of final Grid Layout. Use grid-column.",{node:f});return}else if(p==="display"&&h==="box"){t.warn("You should write display: flex by final spec instead of display: box",{node:f});return}else if(p==="text-emphasis-position")(h==="under"||h==="over")&&t.warn("You should use 2 values for text-emphasis-position For example, `under left` instead of just `under`.",{node:f});else if(/^(align|justify|place)-(items|content)$/.test(p)&&l(f))(h==="start"||h==="end")&&t.warn(`${h} value has mixed support, consider using flex-${h} instead`,{node:f});else if(p==="text-decoration-skip"&&h==="ink")t.warn("Replace text-decoration-skip: ink to text-decoration-skip-ink: auto, because spec had been changed",{node:f});else{if(c&&this.gridStatus(f,t))if(f.value==="subgrid"&&t.warn("IE does not support subgrid",{node:f}),/^(align|justify|place)-items$/.test(p)&&o(f)){let v=p.replace("-items","-self");t.warn(`IE does not support ${p} on grid containers. Try using ${v} on child elements instead: ${f.parent.selector} > * { ${v}: ${f.value} }`,{node:f})}else if(/^(align|justify|place)-content$/.test(p)&&o(f))t.warn(`IE does not support ${f.prop} on grid containers`,{node:f});else if(p==="display"&&f.value==="contents"){t.warn("Please do not use display: contents; if you have grid setting enabled",{node:f});return}else if(f.prop==="grid-gap"){let v=this.gridStatus(f,t);v==="autoplace"&&!NO(f)&&!au(f)?t.warn("grid-gap only works if grid-template(-areas) is being used or both rows and columns have been declared and cells have not been manually placed inside the explicit grid",{node:f}):(v===!0||v==="no-autoplace")&&!au(f)&&t.warn("grid-gap only works if grid-template(-areas) is being used",{node:f})}else if(p==="grid-auto-columns"){t.warn("grid-auto-columns is not supported by IE",{node:f});return}else if(p==="grid-auto-rows"){t.warn("grid-auto-rows is not supported by IE",{node:f});return}else if(p==="grid-auto-flow"){let v=d.some(w=>w.prop==="grid-template-rows"),y=d.some(w=>w.prop==="grid-template-columns");au(f)?t.warn("grid-auto-flow is not supported by IE",{node:f}):h.includes("dense")?t.warn("grid-auto-flow: dense is not supported by IE",{node:f}):!v&&!y&&t.warn("grid-auto-flow works only if grid-template-rows and grid-template-columns are present in the same rule",{node:f});return}else if(h.includes("auto-fit")){t.warn("auto-fit value is not supported by IE",{node:f,word:"auto-fit"});return}else if(h.includes("auto-fill")){t.warn("auto-fill value is not supported by IE",{node:f,word:"auto-fill"});return}else p.startsWith("grid-template")&&h.includes("[")&&t.warn("Autoprefixer currently does not support line names. Try using grid-template-areas instead.",{node:f,word:"["});if(h.includes("radial-gradient"))if(qO.test(f.value))t.warn("Gradient has outdated direction syntax. New syntax is like `closest-side at 0 0` instead of `0 0, closest-side`.",{node:f});else{let v=pb(h);for(let y of v.nodes)if(y.type==="function"&&y.value==="radial-gradient")for(let w of y.nodes)w.type==="word"&&(w.value==="cover"?t.warn("Gradient has outdated direction syntax. Replace `cover` to `farthest-corner`.",{node:f}):w.value==="contain"&&t.warn("Gradient has outdated direction syntax. Replace `contain` to `closest-side`.",{node:f}))}h.includes("linear-gradient")&&DO.test(h)&&t.warn("Gradient has outdated direction syntax. New syntax is like `to left` instead of `right`.",{node:f})}MO.includes(f.prop)&&(f.value.includes("-fill-available")||(f.value.includes("fill-available")?t.warn("Replace fill-available to stretch, because spec had been changed",{node:f}):f.value.includes("fill")&&pb(h).nodes.some(y=>y.type==="word"&&y.value==="fill")&&t.warn("Replace fill to stretch, because spec had been changed",{node:f})));let b;if(f.prop==="transition"||f.prop==="transition-property")return this.prefixes.transition.add(f,t);if(f.prop==="align-self"){if(this.displayType(f)!=="grid"&&this.prefixes.options.flexbox!==!1&&(b=this.prefixes.add["align-self"],b&&b.prefixes&&b.process(f)),this.gridStatus(f,t)!==!1&&(b=this.prefixes.add["grid-row-align"],b&&b.prefixes))return b.process(f,t)}else if(f.prop==="justify-self"){if(this.gridStatus(f,t)!==!1&&(b=this.prefixes.add["grid-column-align"],b&&b.prefixes))return b.process(f,t)}else if(f.prop==="place-self"){if(b=this.prefixes.add["place-self"],b&&b.prefixes&&this.gridStatus(f,t)!==!1)return b.process(f,t)}else if(b=this.prefixes.add[f.prop],b&&b.prefixes)return b.process(f,t)}),this.gridStatus(e,t)&&IO(e,this.disabled),e.walkDecls(f=>{if(this.disabledValue(f,t))return;let d=this.prefixes.unprefixed(f.prop),p=this.prefixes.values("add",d);if(Array.isArray(p))for(let h of p)h.process&&h.process(f,t);PO.save(this.prefixes,f)})}remove(e,t){let i=this.prefixes.remove["@resolution"];e.walkAtRules((n,s)=>{this.prefixes.remove[`@${n.name}`]?this.disabled(n,t)||n.parent.removeChild(s):n.name==="media"&&n.params.includes("-resolution")&&i&&i.clean(n)});for(let n of this.prefixes.remove.selectors)e.walkRules((s,a)=>{n.check(s)&&(this.disabled(s,t)||s.parent.removeChild(a))});return e.walkDecls((n,s)=>{if(this.disabled(n,t))return;let a=n.parent,o=this.prefixes.unprefixed(n.prop);if((n.prop==="transition"||n.prop==="transition-property")&&this.prefixes.transition.remove(n),this.prefixes.remove[n.prop]&&this.prefixes.remove[n.prop].remove){let l=this.prefixes.group(n).down(c=>this.prefixes.normalize(c.prop)===o);if(o==="flex-flow"&&(l=!0),n.prop==="-webkit-box-orient"){let c={"flex-direction":!0,"flex-flow":!0};if(!n.parent.some(f=>c[f.prop]))return}if(l&&!this.withHackValue(n)){n.raw("before").includes(`
`)&&this.reduceSpaces(n),a.removeChild(s);return}}for(let l of this.prefixes.values("remove",o)){if(!l.check||!l.check(n.value))continue;if(o=l.unprefixed,this.prefixes.group(n).down(f=>f.value.includes(o))){a.removeChild(s);return}}})}withHackValue(e){return e.prop==="-webkit-background-clip"&&e.value==="text"}disabledValue(e,t){return this.gridStatus(e,t)===!1&&e.type==="decl"&&e.prop==="display"&&e.value.includes("grid")||this.prefixes.options.flexbox===!1&&e.type==="decl"&&e.prop==="display"&&e.value.includes("flex")||e.type==="decl"&&e.prop==="content"?!0:this.disabled(e,t)}disabledDecl(e,t){if(this.gridStatus(e,t)===!1&&e.type==="decl"&&(e.prop.includes("grid")||e.prop==="justify-items"))return!0;if(this.prefixes.options.flexbox===!1&&e.type==="decl"){let i=["order","justify-content","align-items","align-content"];if(e.prop.includes("flex")||i.includes(e.prop))return!0}return this.disabled(e,t)}disabled(e,t){if(!e)return!1;if(e._autoprefixerDisabled!==void 0)return e._autoprefixerDisabled;if(e.parent){let n=e.prev();if(n&&n.type==="comment"&&$O.test(n.text))return e._autoprefixerDisabled=!0,e._autoprefixerSelfDisabled=!0,!0}let i=null;if(e.nodes){let n;e.each(s=>{s.type==="comment"&&/(!\s*)?autoprefixer:\s*(off|on)/i.test(s.text)&&(typeof n!="undefined"?t.warn("Second Autoprefixer control comment was ignored. Autoprefixer applies control comment to whole block, not to next rules.",{node:s}):n=/on/i.test(s.text))}),n!==void 0&&(i=!n)}if(!e.nodes||i===null)if(e.parent){let n=this.disabled(e.parent,t);e.parent._autoprefixerSelfDisabled===!0?i=!1:i=n}else i=!1;return e._autoprefixerDisabled=i,i}reduceSpaces(e){let t=!1;if(this.prefixes.group(e).up(()=>(t=!0,!0)),t)return;let i=e.raw("before").split(`
`),n=i[i.length-1].length,s=!1;this.prefixes.group(e).down(a=>{i=a.raw("before").split(`
`);let o=i.length-1;i[o].length>n&&(s===!1&&(s=i[o].length-n),i[o]=i[o].slice(0,-s),a.raws.before=i.join(`
`))})}displayType(e){for(let t of e.parent.nodes)if(t.prop==="display"){if(t.value.includes("flex"))return"flex";if(t.value.includes("grid"))return"grid"}return!1}gridStatus(e,t){if(!e)return!1;if(e._autoprefixerGridStatus!==void 0)return e._autoprefixerGridStatus;let i=null;if(e.nodes){let n;e.each(s=>{if(s.type==="comment"&&LO.test(s.text)){let a=/:\s*autoplace/i.test(s.text),o=/no-autoplace/i.test(s.text);typeof n!="undefined"?t.warn("Second Autoprefixer grid control comment was ignored. Autoprefixer applies control comments to the whole block, not to the next rules.",{node:s}):a?n="autoplace":o?n=!0:n=/on/i.test(s.text)}}),n!==void 0&&(i=n)}if(e.type==="atrule"&&e.name==="supports"){let n=e.params;n.includes("grid")&&n.includes("auto")&&(i=!1)}if(!e.nodes||i===null)if(e.parent){let n=this.gridStatus(e.parent,t);e.parent._autoprefixerSelfDisabled===!0?i=!1:i=n}else typeof this.prefixes.options.grid!="undefined"?i=this.prefixes.options.grid:typeof m.env.AUTOPREFIXER_GRID!="undefined"?m.env.AUTOPREFIXER_GRID==="autoplace"?i="autoplace":i=!0:i=!1;return e._autoprefixerGridStatus=i,i}};hb.exports=db});var gb=x((h$,mb)=>{u();mb.exports={A:{A:{"2":"K E F G A B JC"},B:{"1":"C L M H N D O P Q R S T U V W X Y Z a b c d e f g h i j n o p q r s t u v w x y z I"},C:{"1":"2 3 4 5 6 7 8 9 AB BB CB DB EB FB GB HB IB JB KB LB MB NB OB PB QB RB SB TB UB VB WB XB YB ZB aB bB cB 0B dB 1B eB fB gB hB iB jB kB lB mB nB oB m pB qB rB sB tB P Q R 2B S T U V W X Y Z a b c d e f g h i j n o p q r s t u v w x y z I uB 3B 4B","2":"0 1 KC zB J K E F G A B C L M H N D O k l LC MC"},D:{"1":"8 9 AB BB CB DB EB FB GB HB IB JB KB LB MB NB OB PB QB RB SB TB UB VB WB XB YB ZB aB bB cB 0B dB 1B eB fB gB hB iB jB kB lB mB nB oB m pB qB rB sB tB P Q R S T U V W X Y Z a b c d e f g h i j n o p q r s t u v w x y z I uB 3B 4B","2":"0 1 2 3 4 5 6 7 J K E F G A B C L M H N D O k l"},E:{"1":"G A B C L M H D RC 6B vB wB 7B SC TC 8B 9B xB AC yB BC CC DC EC FC GC UC","2":"0 J K E F NC 5B OC PC QC"},F:{"1":"1 2 3 4 5 6 7 8 9 H N D O k l AB BB CB DB EB FB GB HB IB JB KB LB MB NB OB PB QB RB SB TB UB VB WB XB YB ZB aB bB cB dB eB fB gB hB iB jB kB lB mB nB oB m pB qB rB sB tB P Q R 2B S T U V W X Y Z a b c d e f g h i j wB","2":"G B C VC WC XC YC vB HC ZC"},G:{"1":"D fC gC hC iC jC kC lC mC nC oC pC qC rC sC tC 8B 9B xB AC yB BC CC DC EC FC GC","2":"F 5B aC IC bC cC dC eC"},H:{"1":"uC"},I:{"1":"I zC 0C","2":"zB J vC wC xC yC IC"},J:{"2":"E A"},K:{"1":"m","2":"A B C vB HC wB"},L:{"1":"I"},M:{"1":"uB"},N:{"2":"A B"},O:{"1":"xB"},P:{"1":"J k l 1C 2C 3C 4C 5C 6B 6C 7C 8C 9C AD yB BD CD DD"},Q:{"1":"7B"},R:{"1":"ED"},S:{"1":"FD GD"}},B:4,C:"CSS Feature Queries"}});var vb=x((m$,wb)=>{u();function yb(r){return r[r.length-1]}var bb={parse(r){let e=[""],t=[e];for(let i of r){if(i==="("){e=[""],yb(t).push(e),t.push(e);continue}if(i===")"){t.pop(),e=yb(t),e.push("");continue}e[e.length-1]+=i}return t[0]},stringify(r){let e="";for(let t of r){if(typeof t=="object"){e+=`(${bb.stringify(t)})`;continue}e+=t}return e}};wb.exports=bb});var Cb=x((g$,Ab)=>{u();var BO=gb(),{feature:FO}=(Ps(),Rs),{parse:jO}=$e(),zO=Mt(),lu=vb(),UO=He(),VO=_e(),xb=FO(BO),kb=[];for(let r in xb.stats){let e=xb.stats[r];for(let t in e){let i=e[t];/y/.test(i)&&kb.push(r+" "+t)}}var Sb=class{constructor(e,t){this.Prefixes=e,this.all=t}prefixer(){if(this.prefixerCache)return this.prefixerCache;let e=this.all.browsers.selected.filter(i=>kb.includes(i)),t=new zO(this.all.browsers.data,e,this.all.options);return this.prefixerCache=new this.Prefixes(this.all.data,t,this.all.options),this.prefixerCache}parse(e){let t=e.split(":"),i=t[0],n=t[1];return n||(n=""),[i.trim(),n.trim()]}virtual(e){let[t,i]=this.parse(e),n=jO("a{}").first;return n.append({prop:t,value:i,raws:{before:""}}),n}prefixed(e){let t=this.virtual(e);if(this.disabled(t.first))return t.nodes;let i={warn:()=>null},n=this.prefixer().add[t.first.prop];n&&n.process&&n.process(t.first,i);for(let s of t.nodes){for(let a of this.prefixer().values("add",t.first.prop))a.process(s);UO.save(this.all,s)}return t.nodes}isNot(e){return typeof e=="string"&&/not\s*/i.test(e)}isOr(e){return typeof e=="string"&&/\s*or\s*/i.test(e)}isProp(e){return typeof e=="object"&&e.length===1&&typeof e[0]=="string"}isHack(e,t){return!new RegExp(`(\\(|\\s)${VO.escapeRegexp(t)}:`).test(e)}toRemove(e,t){let[i,n]=this.parse(e),s=this.all.unprefixed(i),a=this.all.cleaner();if(a.remove[i]&&a.remove[i].remove&&!this.isHack(t,s))return!0;for(let o of a.values("remove",s))if(o.check(n))return!0;return!1}remove(e,t){let i=0;for(;i<e.length;){if(!this.isNot(e[i-1])&&this.isProp(e[i])&&this.isOr(e[i+1])){if(this.toRemove(e[i][0],t)){e.splice(i,2);continue}i+=2;continue}typeof e[i]=="object"&&(e[i]=this.remove(e[i],t)),i+=1}return e}cleanBrackets(e){return e.map(t=>typeof t!="object"?t:t.length===1&&typeof t[0]=="object"?this.cleanBrackets(t[0]):this.cleanBrackets(t))}convert(e){let t=[""];for(let i of e)t.push([`${i.prop}: ${i.value}`]),t.push(" or ");return t[t.length-1]="",t}normalize(e){if(typeof e!="object")return e;if(e=e.filter(t=>t!==""),typeof e[0]=="string"){let t=e[0].trim();if(t.includes(":")||t==="selector"||t==="not selector")return[lu.stringify(e)]}return e.map(t=>this.normalize(t))}add(e,t){return e.map(i=>{if(this.isProp(i)){let n=this.prefixed(i[0]);return n.length>1?this.convert(n):i}return typeof i=="object"?this.add(i,t):i})}process(e){let t=lu.parse(e.params);t=this.normalize(t),t=this.remove(t,e.params),t=this.add(t,e.params),t=this.cleanBrackets(t),e.params=lu.stringify(t)}disabled(e){if(!this.all.options.grid&&(e.prop==="display"&&e.value.includes("grid")||e.prop.includes("grid")||e.prop==="justify-items"))return!0;if(this.all.options.flexbox===!1){if(e.prop==="display"&&e.value.includes("flex"))return!0;let t=["order","justify-content","align-items","align-content"];if(e.prop.includes("flex")||t.includes(e.prop))return!0}return!1}};Ab.exports=Sb});var Ob=x((y$,Eb)=>{u();var _b=class{constructor(e,t){this.prefix=t,this.prefixed=e.prefixed(this.prefix),this.regexp=e.regexp(this.prefix),this.prefixeds=e.possible().map(i=>[e.prefixed(i),e.regexp(i)]),this.unprefixed=e.name,this.nameRegexp=e.regexp()}isHack(e){let t=e.parent.index(e)+1,i=e.parent.nodes;for(;t<i.length;){let n=i[t].selector;if(!n)return!0;if(n.includes(this.unprefixed)&&n.match(this.nameRegexp))return!1;let s=!1;for(let[a,o]of this.prefixeds)if(n.includes(a)&&n.match(o)){s=!0;break}if(!s)return!0;t+=1}return!0}check(e){return!(!e.selector.includes(this.prefixed)||!e.selector.match(this.regexp)||this.isHack(e))}};Eb.exports=_b});var kr=x((b$,Rb)=>{u();var{list:HO}=$e(),WO=Ob(),GO=wr(),QO=Mt(),YO=_e(),Tb=class extends GO{constructor(e,t,i){super(e,t,i);this.regexpCache=new Map}check(e){return e.selector.includes(this.name)?!!e.selector.match(this.regexp()):!1}prefixed(e){return this.name.replace(/^(\W*)/,`$1${e}`)}regexp(e){if(!this.regexpCache.has(e)){let t=e?this.prefixed(e):this.name;this.regexpCache.set(e,new RegExp(`(^|[^:"'=])${YO.escapeRegexp(t)}`,"gi"))}return this.regexpCache.get(e)}possible(){return QO.prefixes()}prefixeds(e){if(e._autoprefixerPrefixeds){if(e._autoprefixerPrefixeds[this.name])return e._autoprefixerPrefixeds}else e._autoprefixerPrefixeds={};let t={};if(e.selector.includes(",")){let n=HO.comma(e.selector).filter(s=>s.includes(this.name));for(let s of this.possible())t[s]=n.map(a=>this.replace(a,s)).join(", ")}else for(let i of this.possible())t[i]=this.replace(e.selector,i);return e._autoprefixerPrefixeds[this.name]=t,e._autoprefixerPrefixeds}already(e,t,i){let n=e.parent.index(e)-1;for(;n>=0;){let s=e.parent.nodes[n];if(s.type!=="rule")return!1;let a=!1;for(let o in t[this.name]){let l=t[this.name][o];if(s.selector===l){if(i===o)return!0;a=!0;break}}if(!a)return!1;n-=1}return!1}replace(e,t){return e.replace(this.regexp(),`$1${this.prefixed(t)}`)}add(e,t){let i=this.prefixeds(e);if(this.already(e,i,t))return;let n=this.clone(e,{selector:i[this.name][t]});e.parent.insertBefore(e,n)}old(e){return new WO(this,e)}};Rb.exports=Tb});var Db=x((w$,Ib)=>{u();var KO=wr(),Pb=class extends KO{add(e,t){let i=t+e.name;if(e.parent.some(a=>a.name===i&&a.params===e.params))return;let s=this.clone(e,{name:i});return e.parent.insertBefore(e,s)}process(e){let t=this.parentPrefix(e);for(let i of this.prefixes)(!t||t===i)&&this.add(e,i)}};Ib.exports=Pb});var $b=x((v$,qb)=>{u();var XO=kr(),uu=class extends XO{prefixed(e){return e==="-webkit-"?":-webkit-full-screen":e==="-moz-"?":-moz-full-screen":`:${e}fullscreen`}};uu.names=[":fullscreen"];qb.exports=uu});var Mb=x((x$,Lb)=>{u();var ZO=kr(),fu=class extends ZO{possible(){return super.possible().concat(["-moz- old","-ms- old"])}prefixed(e){return e==="-webkit-"?"::-webkit-input-placeholder":e==="-ms-"?"::-ms-input-placeholder":e==="-ms- old"?":-ms-input-placeholder":e==="-moz- old"?":-moz-placeholder":`::${e}placeholder`}};fu.names=["::placeholder"];Lb.exports=fu});var Bb=x((k$,Nb)=>{u();var JO=kr(),cu=class extends JO{prefixed(e){return e==="-ms-"?":-ms-input-placeholder":`:${e}placeholder-shown`}};cu.names=[":placeholder-shown"];Nb.exports=cu});var jb=x((S$,Fb)=>{u();var eT=kr(),tT=_e(),pu=class extends eT{constructor(e,t,i){super(e,t,i);this.prefixes&&(this.prefixes=tT.uniq(this.prefixes.map(n=>"-webkit-")))}prefixed(e){return e==="-webkit-"?"::-webkit-file-upload-button":`::${e}file-selector-button`}};pu.names=["::file-selector-button"];Fb.exports=pu});var Pe=x((A$,zb)=>{u();zb.exports=function(r){let e;return r==="-webkit- 2009"||r==="-moz-"?e=2009:r==="-ms-"?e=2012:r==="-webkit-"&&(e="final"),r==="-webkit- 2009"&&(r="-webkit-"),[e,r]}});var Wb=x((C$,Hb)=>{u();var Ub=$e().list,Vb=Pe(),rT=j(),Sr=class extends rT{prefixed(e,t){let i;return[i,t]=Vb(t),i===2009?t+"box-flex":super.prefixed(e,t)}normalize(){return"flex"}set(e,t){let i=Vb(t)[0];if(i===2009)return e.value=Ub.space(e.value)[0],e.value=Sr.oldValues[e.value]||e.value,super.set(e,t);if(i===2012){let n=Ub.space(e.value);n.length===3&&n[2]==="0"&&(e.value=n.slice(0,2).concat("0px").join(" "))}return super.set(e,t)}};Sr.names=["flex","box-flex"];Sr.oldValues={auto:"1",none:"0"};Hb.exports=Sr});var Yb=x((_$,Qb)=>{u();var Gb=Pe(),iT=j(),du=class extends iT{prefixed(e,t){let i;return[i,t]=Gb(t),i===2009?t+"box-ordinal-group":i===2012?t+"flex-order":super.prefixed(e,t)}normalize(){return"order"}set(e,t){return Gb(t)[0]===2009&&/\d/.test(e.value)?(e.value=(parseInt(e.value)+1).toString(),super.set(e,t)):super.set(e,t)}};du.names=["order","flex-order","box-ordinal-group"];Qb.exports=du});var Xb=x((E$,Kb)=>{u();var nT=j(),hu=class extends nT{check(e){let t=e.value;return!t.toLowerCase().includes("alpha(")&&!t.includes("DXImageTransform.Microsoft")&&!t.includes("data:image/svg+xml")}};hu.names=["filter"];Kb.exports=hu});var Jb=x((O$,Zb)=>{u();var sT=j(),mu=class extends sT{insert(e,t,i,n){if(t!=="-ms-")return super.insert(e,t,i);let s=this.clone(e),a=e.prop.replace(/end$/,"start"),o=t+e.prop.replace(/end$/,"span");if(!e.parent.some(l=>l.prop===o)){if(s.prop=o,e.value.includes("span"))s.value=e.value.replace(/span\s/i,"");else{let l;if(e.parent.walkDecls(a,c=>{l=c}),l){let c=Number(e.value)-Number(l.value)+"";s.value=c}else e.warn(n,`Can not prefix ${e.prop} (${a} is not found)`)}e.cloneBefore(s)}}};mu.names=["grid-row-end","grid-column-end"];Zb.exports=mu});var tw=x((T$,ew)=>{u();var aT=j(),gu=class extends aT{check(e){return!e.value.split(/\s+/).some(t=>{let i=t.toLowerCase();return i==="reverse"||i==="alternate-reverse"})}};gu.names=["animation","animation-direction"];ew.exports=gu});var iw=x((R$,rw)=>{u();var oT=Pe(),lT=j(),yu=class extends lT{insert(e,t,i){let n;if([n,t]=oT(t),n!==2009)return super.insert(e,t,i);let s=e.value.split(/\s+/).filter(d=>d!=="wrap"&&d!=="nowrap"&&"wrap-reverse");if(s.length===0||e.parent.some(d=>d.prop===t+"box-orient"||d.prop===t+"box-direction"))return;let o=s[0],l=o.includes("row")?"horizontal":"vertical",c=o.includes("reverse")?"reverse":"normal",f=this.clone(e);return f.prop=t+"box-orient",f.value=l,this.needCascade(e)&&(f.raws.before=this.calcBefore(i,e,t)),e.parent.insertBefore(e,f),f=this.clone(e),f.prop=t+"box-direction",f.value=c,this.needCascade(e)&&(f.raws.before=this.calcBefore(i,e,t)),e.parent.insertBefore(e,f)}};yu.names=["flex-flow","box-direction","box-orient"];rw.exports=yu});var sw=x((P$,nw)=>{u();var uT=Pe(),fT=j(),bu=class extends fT{normalize(){return"flex"}prefixed(e,t){let i;return[i,t]=uT(t),i===2009?t+"box-flex":i===2012?t+"flex-positive":super.prefixed(e,t)}};bu.names=["flex-grow","flex-positive"];nw.exports=bu});var ow=x((I$,aw)=>{u();var cT=Pe(),pT=j(),wu=class extends pT{set(e,t){if(cT(t)[0]!==2009)return super.set(e,t)}};wu.names=["flex-wrap"];aw.exports=wu});var uw=x((D$,lw)=>{u();var dT=j(),Ar=Bt(),vu=class extends dT{insert(e,t,i,n){if(t!=="-ms-")return super.insert(e,t,i);let s=Ar.parse(e),[a,o]=Ar.translate(s,0,2),[l,c]=Ar.translate(s,1,3);[["grid-row",a],["grid-row-span",o],["grid-column",l],["grid-column-span",c]].forEach(([f,d])=>{Ar.insertDecl(e,f,d)}),Ar.warnTemplateSelectorNotFound(e,n),Ar.warnIfGridRowColumnExists(e,n)}};vu.names=["grid-area"];lw.exports=vu});var cw=x((q$,fw)=>{u();var hT=j(),Bi=Bt(),xu=class extends hT{insert(e,t,i){if(t!=="-ms-")return super.insert(e,t,i);if(e.parent.some(a=>a.prop==="-ms-grid-row-align"))return;let[[n,s]]=Bi.parse(e);s?(Bi.insertDecl(e,"grid-row-align",n),Bi.insertDecl(e,"grid-column-align",s)):(Bi.insertDecl(e,"grid-row-align",n),Bi.insertDecl(e,"grid-column-align",n))}};xu.names=["place-self"];fw.exports=xu});var dw=x(($$,pw)=>{u();var mT=j(),ku=class extends mT{check(e){let t=e.value;return!t.includes("/")||t.includes("span")}normalize(e){return e.replace("-start","")}prefixed(e,t){let i=super.prefixed(e,t);return t==="-ms-"&&(i=i.replace("-start","")),i}};ku.names=["grid-row-start","grid-column-start"];pw.exports=ku});var gw=x((L$,mw)=>{u();var hw=Pe(),gT=j(),Cr=class extends gT{check(e){return e.parent&&!e.parent.some(t=>t.prop&&t.prop.startsWith("grid-"))}prefixed(e,t){let i;return[i,t]=hw(t),i===2012?t+"flex-item-align":super.prefixed(e,t)}normalize(){return"align-self"}set(e,t){let i=hw(t)[0];if(i===2012)return e.value=Cr.oldValues[e.value]||e.value,super.set(e,t);if(i==="final")return super.set(e,t)}};Cr.names=["align-self","flex-item-align"];Cr.oldValues={"flex-end":"end","flex-start":"start"};mw.exports=Cr});var bw=x((M$,yw)=>{u();var yT=j(),bT=_e(),Su=class extends yT{constructor(e,t,i){super(e,t,i);this.prefixes&&(this.prefixes=bT.uniq(this.prefixes.map(n=>n==="-ms-"?"-webkit-":n)))}};Su.names=["appearance"];yw.exports=Su});var xw=x((N$,vw)=>{u();var ww=Pe(),wT=j(),Au=class extends wT{normalize(){return"flex-basis"}prefixed(e,t){let i;return[i,t]=ww(t),i===2012?t+"flex-preferred-size":super.prefixed(e,t)}set(e,t){let i;if([i,t]=ww(t),i===2012||i==="final")return super.set(e,t)}};Au.names=["flex-basis","flex-preferred-size"];vw.exports=Au});var Sw=x((B$,kw)=>{u();var vT=j(),Cu=class extends vT{normalize(){return this.name.replace("box-image","border")}prefixed(e,t){let i=super.prefixed(e,t);return t==="-webkit-"&&(i=i.replace("border","box-image")),i}};Cu.names=["mask-border","mask-border-source","mask-border-slice","mask-border-width","mask-border-outset","mask-border-repeat","mask-box-image","mask-box-image-source","mask-box-image-slice","mask-box-image-width","mask-box-image-outset","mask-box-image-repeat"];kw.exports=Cu});var Cw=x((F$,Aw)=>{u();var xT=j(),lt=class extends xT{insert(e,t,i){let n=e.prop==="mask-composite",s;n?s=e.value.split(","):s=e.value.match(lt.regexp)||[],s=s.map(c=>c.trim()).filter(c=>c);let a=s.length,o;if(a&&(o=this.clone(e),o.value=s.map(c=>lt.oldValues[c]||c).join(", "),s.includes("intersect")&&(o.value+=", xor"),o.prop=t+"mask-composite"),n)return a?(this.needCascade(e)&&(o.raws.before=this.calcBefore(i,e,t)),e.parent.insertBefore(e,o)):void 0;let l=this.clone(e);return l.prop=t+l.prop,a&&(l.value=l.value.replace(lt.regexp,"")),this.needCascade(e)&&(l.raws.before=this.calcBefore(i,e,t)),e.parent.insertBefore(e,l),a?(this.needCascade(e)&&(o.raws.before=this.calcBefore(i,e,t)),e.parent.insertBefore(e,o)):e}};lt.names=["mask","mask-composite"];lt.oldValues={add:"source-over",subtract:"source-out",intersect:"source-in",exclude:"xor"};lt.regexp=new RegExp(`\\s+(${Object.keys(lt.oldValues).join("|")})\\b(?!\\))\\s*(?=[,])`,"ig");Aw.exports=lt});var Ow=x((j$,Ew)=>{u();var _w=Pe(),kT=j(),_r=class extends kT{prefixed(e,t){let i;return[i,t]=_w(t),i===2009?t+"box-align":i===2012?t+"flex-align":super.prefixed(e,t)}normalize(){return"align-items"}set(e,t){let i=_w(t)[0];return(i===2009||i===2012)&&(e.value=_r.oldValues[e.value]||e.value),super.set(e,t)}};_r.names=["align-items","flex-align","box-align"];_r.oldValues={"flex-end":"end","flex-start":"start"};Ew.exports=_r});var Rw=x((z$,Tw)=>{u();var ST=j(),_u=class extends ST{set(e,t){return t==="-ms-"&&e.value==="contain"&&(e.value="element"),super.set(e,t)}insert(e,t,i){if(!(e.value==="all"&&t==="-ms-"))return super.insert(e,t,i)}};_u.names=["user-select"];Tw.exports=_u});var Dw=x((U$,Iw)=>{u();var Pw=Pe(),AT=j(),Eu=class extends AT{normalize(){return"flex-shrink"}prefixed(e,t){let i;return[i,t]=Pw(t),i===2012?t+"flex-negative":super.prefixed(e,t)}set(e,t){let i;if([i,t]=Pw(t),i===2012||i==="final")return super.set(e,t)}};Eu.names=["flex-shrink","flex-negative"];Iw.exports=Eu});var $w=x((V$,qw)=>{u();var CT=j(),Ou=class extends CT{prefixed(e,t){return`${t}column-${e}`}normalize(e){return e.includes("inside")?"break-inside":e.includes("before")?"break-before":"break-after"}set(e,t){return(e.prop==="break-inside"&&e.value==="avoid-column"||e.value==="avoid-page")&&(e.value="avoid"),super.set(e,t)}insert(e,t,i){if(e.prop!=="break-inside")return super.insert(e,t,i);if(!(/region/i.test(e.value)||/page/i.test(e.value)))return super.insert(e,t,i)}};Ou.names=["break-inside","page-break-inside","column-break-inside","break-before","page-break-before","column-break-before","break-after","page-break-after","column-break-after"];qw.exports=Ou});var Mw=x((H$,Lw)=>{u();var _T=j(),Tu=class extends _T{prefixed(e,t){return t+"print-color-adjust"}normalize(){return"color-adjust"}};Tu.names=["color-adjust","print-color-adjust"];Lw.exports=Tu});var Bw=x((W$,Nw)=>{u();var ET=j(),Er=class extends ET{insert(e,t,i){if(t==="-ms-"){let n=this.set(this.clone(e),t);this.needCascade(e)&&(n.raws.before=this.calcBefore(i,e,t));let s="ltr";return e.parent.nodes.forEach(a=>{a.prop==="direction"&&(a.value==="rtl"||a.value==="ltr")&&(s=a.value)}),n.value=Er.msValues[s][e.value]||e.value,e.parent.insertBefore(e,n)}return super.insert(e,t,i)}};Er.names=["writing-mode"];Er.msValues={ltr:{"horizontal-tb":"lr-tb","vertical-rl":"tb-rl","vertical-lr":"tb-lr"},rtl:{"horizontal-tb":"rl-tb","vertical-rl":"bt-rl","vertical-lr":"bt-lr"}};Nw.exports=Er});var jw=x((G$,Fw)=>{u();var OT=j(),Ru=class extends OT{set(e,t){return e.value=e.value.replace(/\s+fill(\s)/,"$1"),super.set(e,t)}};Ru.names=["border-image"];Fw.exports=Ru});var Vw=x((Q$,Uw)=>{u();var zw=Pe(),TT=j(),Or=class extends TT{prefixed(e,t){let i;return[i,t]=zw(t),i===2012?t+"flex-line-pack":super.prefixed(e,t)}normalize(){return"align-content"}set(e,t){let i=zw(t)[0];if(i===2012)return e.value=Or.oldValues[e.value]||e.value,super.set(e,t);if(i==="final")return super.set(e,t)}};Or.names=["align-content","flex-line-pack"];Or.oldValues={"flex-end":"end","flex-start":"start","space-between":"justify","space-around":"distribute"};Uw.exports=Or});var Ww=x((Y$,Hw)=>{u();var RT=j(),We=class extends RT{prefixed(e,t){return t==="-moz-"?t+(We.toMozilla[e]||e):super.prefixed(e,t)}normalize(e){return We.toNormal[e]||e}};We.names=["border-radius"];We.toMozilla={};We.toNormal={};for(let r of["top","bottom"])for(let e of["left","right"]){let t=`border-${r}-${e}-radius`,i=`border-radius-${r}${e}`;We.names.push(t),We.names.push(i),We.toMozilla[t]=i,We.toNormal[i]=t}Hw.exports=We});var Qw=x((K$,Gw)=>{u();var PT=j(),Pu=class extends PT{prefixed(e,t){return e.includes("-start")?t+e.replace("-block-start","-before"):t+e.replace("-block-end","-after")}normalize(e){return e.includes("-before")?e.replace("-before","-block-start"):e.replace("-after","-block-end")}};Pu.names=["border-block-start","border-block-end","margin-block-start","margin-block-end","padding-block-start","padding-block-end","border-before","border-after","margin-before","margin-after","padding-before","padding-after"];Gw.exports=Pu});var Kw=x((X$,Yw)=>{u();var IT=j(),{parseTemplate:DT,warnMissedAreas:qT,getGridGap:$T,warnGridGap:LT,inheritGridGap:MT}=Bt(),Iu=class extends IT{insert(e,t,i,n){if(t!=="-ms-")return super.insert(e,t,i);if(e.parent.some(h=>h.prop==="-ms-grid-rows"))return;let s=$T(e),a=MT(e,s),{rows:o,columns:l,areas:c}=DT({decl:e,gap:a||s}),f=Object.keys(c).length>0,d=Boolean(o),p=Boolean(l);return LT({gap:s,hasColumns:p,decl:e,result:n}),qT(c,e,n),(d&&p||f)&&e.cloneBefore({prop:"-ms-grid-rows",value:o,raws:{}}),p&&e.cloneBefore({prop:"-ms-grid-columns",value:l,raws:{}}),e}};Iu.names=["grid-template"];Yw.exports=Iu});var Zw=x((Z$,Xw)=>{u();var NT=j(),Du=class extends NT{prefixed(e,t){return t+e.replace("-inline","")}normalize(e){return e.replace(/(margin|padding|border)-(start|end)/,"$1-inline-$2")}};Du.names=["border-inline-start","border-inline-end","margin-inline-start","margin-inline-end","padding-inline-start","padding-inline-end","border-start","border-end","margin-start","margin-end","padding-start","padding-end"];Xw.exports=Du});var e0=x((J$,Jw)=>{u();var BT=j(),qu=class extends BT{check(e){return!e.value.includes("flex-")&&e.value!=="baseline"}prefixed(e,t){return t+"grid-row-align"}normalize(){return"align-self"}};qu.names=["grid-row-align"];Jw.exports=qu});var r0=x((eL,t0)=>{u();var FT=j(),Tr=class extends FT{keyframeParents(e){let{parent:t}=e;for(;t;){if(t.type==="atrule"&&t.name==="keyframes")return!0;({parent:t}=t)}return!1}contain3d(e){if(e.prop==="transform-origin")return!1;for(let t of Tr.functions3d)if(e.value.includes(`${t}(`))return!0;return!1}set(e,t){return e=super.set(e,t),t==="-ms-"&&(e.value=e.value.replace(/rotatez/gi,"rotate")),e}insert(e,t,i){if(t==="-ms-"){if(!this.contain3d(e)&&!this.keyframeParents(e))return super.insert(e,t,i)}else if(t==="-o-"){if(!this.contain3d(e))return super.insert(e,t,i)}else return super.insert(e,t,i)}};Tr.names=["transform","transform-origin"];Tr.functions3d=["matrix3d","translate3d","translateZ","scale3d","scaleZ","rotate3d","rotateX","rotateY","perspective"];t0.exports=Tr});var s0=x((tL,n0)=>{u();var i0=Pe(),jT=j(),$u=class extends jT{normalize(){return"flex-direction"}insert(e,t,i){let n;if([n,t]=i0(t),n!==2009)return super.insert(e,t,i);if(e.parent.some(f=>f.prop===t+"box-orient"||f.prop===t+"box-direction"))return;let a=e.value,o,l;a==="inherit"||a==="initial"||a==="unset"?(o=a,l=a):(o=a.includes("row")?"horizontal":"vertical",l=a.includes("reverse")?"reverse":"normal");let c=this.clone(e);return c.prop=t+"box-orient",c.value=o,this.needCascade(e)&&(c.raws.before=this.calcBefore(i,e,t)),e.parent.insertBefore(e,c),c=this.clone(e),c.prop=t+"box-direction",c.value=l,this.needCascade(e)&&(c.raws.before=this.calcBefore(i,e,t)),e.parent.insertBefore(e,c)}old(e,t){let i;return[i,t]=i0(t),i===2009?[t+"box-orient",t+"box-direction"]:super.old(e,t)}};$u.names=["flex-direction","box-direction","box-orient"];n0.exports=$u});var o0=x((rL,a0)=>{u();var zT=j(),Lu=class extends zT{check(e){return e.value==="pixelated"}prefixed(e,t){return t==="-ms-"?"-ms-interpolation-mode":super.prefixed(e,t)}set(e,t){return t!=="-ms-"?super.set(e,t):(e.prop="-ms-interpolation-mode",e.value="nearest-neighbor",e)}normalize(){return"image-rendering"}process(e,t){return super.process(e,t)}};Lu.names=["image-rendering","interpolation-mode"];a0.exports=Lu});var u0=x((iL,l0)=>{u();var UT=j(),VT=_e(),Mu=class extends UT{constructor(e,t,i){super(e,t,i);this.prefixes&&(this.prefixes=VT.uniq(this.prefixes.map(n=>n==="-ms-"?"-webkit-":n)))}};Mu.names=["backdrop-filter"];l0.exports=Mu});var c0=x((nL,f0)=>{u();var HT=j(),WT=_e(),Nu=class extends HT{constructor(e,t,i){super(e,t,i);this.prefixes&&(this.prefixes=WT.uniq(this.prefixes.map(n=>n==="-ms-"?"-webkit-":n)))}check(e){return e.value.toLowerCase()==="text"}};Nu.names=["background-clip"];f0.exports=Nu});var d0=x((sL,p0)=>{u();var GT=j(),QT=["none","underline","overline","line-through","blink","inherit","initial","unset"],Bu=class extends GT{check(e){return e.value.split(/\s+/).some(t=>!QT.includes(t))}};Bu.names=["text-decoration"];p0.exports=Bu});var g0=x((aL,m0)=>{u();var h0=Pe(),YT=j(),Rr=class extends YT{prefixed(e,t){let i;return[i,t]=h0(t),i===2009?t+"box-pack":i===2012?t+"flex-pack":super.prefixed(e,t)}normalize(){return"justify-content"}set(e,t){let i=h0(t)[0];if(i===2009||i===2012){let n=Rr.oldValues[e.value]||e.value;if(e.value=n,i!==2009||n!=="distribute")return super.set(e,t)}else if(i==="final")return super.set(e,t)}};Rr.names=["justify-content","flex-pack","box-pack"];Rr.oldValues={"flex-end":"end","flex-start":"start","space-between":"justify","space-around":"distribute"};m0.exports=Rr});var b0=x((oL,y0)=>{u();var KT=j(),Fu=class extends KT{set(e,t){let i=e.value.toLowerCase();return t==="-webkit-"&&!i.includes(" ")&&i!=="contain"&&i!=="cover"&&(e.value=e.value+" "+e.value),super.set(e,t)}};Fu.names=["background-size"];y0.exports=Fu});var v0=x((lL,w0)=>{u();var XT=j(),ju=Bt(),zu=class extends XT{insert(e,t,i){if(t!=="-ms-")return super.insert(e,t,i);let n=ju.parse(e),[s,a]=ju.translate(n,0,1);n[0]&&n[0].includes("span")&&(a=n[0].join("").replace(/\D/g,"")),[[e.prop,s],[`${e.prop}-span`,a]].forEach(([l,c])=>{ju.insertDecl(e,l,c)})}};zu.names=["grid-row","grid-column"];w0.exports=zu});var S0=x((uL,k0)=>{u();var ZT=j(),{prefixTrackProp:x0,prefixTrackValue:JT,autoplaceGridItems:eR,getGridGap:tR,inheritGridGap:rR}=Bt(),iR=ou(),Uu=class extends ZT{prefixed(e,t){return t==="-ms-"?x0({prop:e,prefix:t}):super.prefixed(e,t)}normalize(e){return e.replace(/^grid-(rows|columns)/,"grid-template-$1")}insert(e,t,i,n){if(t!=="-ms-")return super.insert(e,t,i);let{parent:s,prop:a,value:o}=e,l=a.includes("rows"),c=a.includes("columns"),f=s.some(k=>k.prop==="grid-template"||k.prop==="grid-template-areas");if(f&&l)return!1;let d=new iR({options:{}}),p=d.gridStatus(s,n),h=tR(e);h=rR(e,h)||h;let b=l?h.row:h.column;(p==="no-autoplace"||p===!0)&&!f&&(b=null);let v=JT({value:o,gap:b});e.cloneBefore({prop:x0({prop:a,prefix:t}),value:v});let y=s.nodes.find(k=>k.prop==="grid-auto-flow"),w="row";if(y&&!d.disabled(y,n)&&(w=y.value.trim()),p==="autoplace"){let k=s.nodes.find(E=>E.prop==="grid-template-rows");if(!k&&f)return;if(!k&&!f){e.warn(n,"Autoplacement does not work without grid-template-rows property");return}!s.nodes.find(E=>E.prop==="grid-template-columns")&&!f&&e.warn(n,"Autoplacement does not work without grid-template-columns property"),c&&!f&&eR(e,n,h,w)}}};Uu.names=["grid-template-rows","grid-template-columns","grid-rows","grid-columns"];k0.exports=Uu});var C0=x((fL,A0)=>{u();var nR=j(),Vu=class extends nR{check(e){return!e.value.includes("flex-")&&e.value!=="baseline"}prefixed(e,t){return t+"grid-column-align"}normalize(){return"justify-self"}};Vu.names=["grid-column-align"];A0.exports=Vu});var E0=x((cL,_0)=>{u();var sR=j(),Hu=class extends sR{prefixed(e,t){return t+"scroll-chaining"}normalize(){return"overscroll-behavior"}set(e,t){return e.value==="auto"?e.value="chained":(e.value==="none"||e.value==="contain")&&(e.value="none"),super.set(e,t)}};Hu.names=["overscroll-behavior","scroll-chaining"];_0.exports=Hu});var R0=x((pL,T0)=>{u();var aR=j(),{parseGridAreas:oR,warnMissedAreas:lR,prefixTrackProp:uR,prefixTrackValue:O0,getGridGap:fR,warnGridGap:cR,inheritGridGap:pR}=Bt();function dR(r){return r.trim().slice(1,-1).split(/["']\s*["']?/g)}var Wu=class extends aR{insert(e,t,i,n){if(t!=="-ms-")return super.insert(e,t,i);let s=!1,a=!1,o=e.parent,l=fR(e);l=pR(e,l)||l,o.walkDecls(/-ms-grid-rows/,d=>d.remove()),o.walkDecls(/grid-template-(rows|columns)/,d=>{if(d.prop==="grid-template-rows"){a=!0;let{prop:p,value:h}=d;d.cloneBefore({prop:uR({prop:p,prefix:t}),value:O0({value:h,gap:l.row})})}else s=!0});let c=dR(e.value);s&&!a&&l.row&&c.length>1&&e.cloneBefore({prop:"-ms-grid-rows",value:O0({value:`repeat(${c.length}, auto)`,gap:l.row}),raws:{}}),cR({gap:l,hasColumns:s,decl:e,result:n});let f=oR({rows:c,gap:l});return lR(f,e,n),e}};Wu.names=["grid-template-areas"];T0.exports=Wu});var I0=x((dL,P0)=>{u();var hR=j(),Gu=class extends hR{set(e,t){return t==="-webkit-"&&(e.value=e.value.replace(/\s*(right|left)\s*/i,"")),super.set(e,t)}};Gu.names=["text-emphasis-position"];P0.exports=Gu});var q0=x((hL,D0)=>{u();var mR=j(),Qu=class extends mR{set(e,t){return e.prop==="text-decoration-skip-ink"&&e.value==="auto"?(e.prop=t+"text-decoration-skip",e.value="ink",e):super.set(e,t)}};Qu.names=["text-decoration-skip-ink","text-decoration-skip"];D0.exports=Qu});var F0=x((mL,B0)=>{u();"use strict";B0.exports={wrap:$0,limit:L0,validate:M0,test:Yu,curry:gR,name:N0};function $0(r,e,t){var i=e-r;return((t-r)%i+i)%i+r}function L0(r,e,t){return Math.max(r,Math.min(e,t))}function M0(r,e,t,i,n){if(!Yu(r,e,t,i,n))throw new Error(t+" is outside of range ["+r+","+e+")");return t}function Yu(r,e,t,i,n){return!(t<r||t>e||n&&t===e||i&&t===r)}function N0(r,e,t,i){return(t?"(":"[")+r+","+e+(i?")":"]")}function gR(r,e,t,i){var n=N0.bind(null,r,e,t,i);return{wrap:$0.bind(null,r,e),limit:L0.bind(null,r,e),validate:function(s){return M0(r,e,s,t,i)},test:function(s){return Yu(r,e,s,t,i)},toString:n,name:n}}});var U0=x((gL,z0)=>{u();var Ku=Ms(),yR=F0(),bR=xr(),wR=He(),vR=_e(),j0=/top|left|right|bottom/gi,wt=class extends wR{replace(e,t){let i=Ku(e);for(let n of i.nodes)if(n.type==="function"&&n.value===this.name)if(n.nodes=this.newDirection(n.nodes),n.nodes=this.normalize(n.nodes),t==="-webkit- old"){if(!this.oldWebkit(n))return!1}else n.nodes=this.convertDirection(n.nodes),n.value=t+n.value;return i.toString()}replaceFirst(e,...t){return t.map(n=>n===" "?{type:"space",value:n}:{type:"word",value:n}).concat(e.slice(1))}normalizeUnit(e,t){return`${parseFloat(e)/t*360}deg`}normalize(e){if(!e[0])return e;if(/-?\d+(.\d+)?grad/.test(e[0].value))e[0].value=this.normalizeUnit(e[0].value,400);else if(/-?\d+(.\d+)?rad/.test(e[0].value))e[0].value=this.normalizeUnit(e[0].value,2*Math.PI);else if(/-?\d+(.\d+)?turn/.test(e[0].value))e[0].value=this.normalizeUnit(e[0].value,1);else if(e[0].value.includes("deg")){let t=parseFloat(e[0].value);t=yR.wrap(0,360,t),e[0].value=`${t}deg`}return e[0].value==="0deg"?e=this.replaceFirst(e,"to"," ","top"):e[0].value==="90deg"?e=this.replaceFirst(e,"to"," ","right"):e[0].value==="180deg"?e=this.replaceFirst(e,"to"," ","bottom"):e[0].value==="270deg"&&(e=this.replaceFirst(e,"to"," ","left")),e}newDirection(e){if(e[0].value==="to"||(j0.lastIndex=0,!j0.test(e[0].value)))return e;e.unshift({type:"word",value:"to"},{type:"space",value:" "});for(let t=2;t<e.length&&e[t].type!=="div";t++)e[t].type==="word"&&(e[t].value=this.revertDirection(e[t].value));return e}isRadial(e){let t="before";for(let i of e)if(t==="before"&&i.type==="space")t="at";else if(t==="at"&&i.value==="at")t="after";else{if(t==="after"&&i.type==="space")return!0;if(i.type==="div")break;t="before"}return!1}convertDirection(e){return e.length>0&&(e[0].value==="to"?this.fixDirection(e):e[0].value.includes("deg")?this.fixAngle(e):this.isRadial(e)&&this.fixRadial(e)),e}fixDirection(e){e.splice(0,2);for(let t of e){if(t.type==="div")break;t.type==="word"&&(t.value=this.revertDirection(t.value))}}fixAngle(e){let t=e[0].value;t=parseFloat(t),t=Math.abs(450-t)%360,t=this.roundFloat(t,3),e[0].value=`${t}deg`}fixRadial(e){let t=[],i=[],n,s,a,o,l;for(o=0;o<e.length-2;o++)if(n=e[o],s=e[o+1],a=e[o+2],n.type==="space"&&s.value==="at"&&a.type==="space"){l=o+3;break}else t.push(n);let c;for(o=l;o<e.length;o++)if(e[o].type==="div"){c=e[o];break}else i.push(e[o]);e.splice(0,o,...i,c,...t)}revertDirection(e){return wt.directions[e.toLowerCase()]||e}roundFloat(e,t){return parseFloat(e.toFixed(t))}oldWebkit(e){let{nodes:t}=e,i=Ku.stringify(e.nodes);if(this.name!=="linear-gradient"||t[0]&&t[0].value.includes("deg")||i.includes("px")||i.includes("-corner")||i.includes("-side"))return!1;let n=[[]];for(let s of t)n[n.length-1].push(s),s.type==="div"&&s.value===","&&n.push([]);this.oldDirection(n),this.colorStops(n),e.nodes=[];for(let s of n)e.nodes=e.nodes.concat(s);return e.nodes.unshift({type:"word",value:"linear"},this.cloneDiv(e.nodes)),e.value="-webkit-gradient",!0}oldDirection(e){let t=this.cloneDiv(e[0]);if(e[0][0].value!=="to")return e.unshift([{type:"word",value:wt.oldDirections.bottom},t]);{let i=[];for(let s of e[0].slice(2))s.type==="word"&&i.push(s.value.toLowerCase());i=i.join(" ");let n=wt.oldDirections[i]||i;return e[0]=[{type:"word",value:n},t],e[0]}}cloneDiv(e){for(let t of e)if(t.type==="div"&&t.value===",")return t;return{type:"div",value:",",after:" "}}colorStops(e){let t=[];for(let i=0;i<e.length;i++){let n,s=e[i],a;if(i===0)continue;let o=Ku.stringify(s[0]);s[1]&&s[1].type==="word"?n=s[1].value:s[2]&&s[2].type==="word"&&(n=s[2].value);let l;i===1&&(!n||n==="0%")?l=`from(${o})`:i===e.length-1&&(!n||n==="100%")?l=`to(${o})`:n?l=`color-stop(${n}, ${o})`:l=`color-stop(${o})`;let c=s[s.length-1];e[i]=[{type:"word",value:l}],c.type==="div"&&c.value===","&&(a=e[i].push(c)),t.push(a)}return t}old(e){if(e==="-webkit-"){let t=this.name==="linear-gradient"?"linear":"radial",i="-gradient",n=vR.regexp(`-webkit-(${t}-gradient|gradient\\(\\s*${t})`,!1);return new bR(this.name,e+this.name,i,n)}else return super.old(e)}add(e,t){let i=e.prop;if(i.includes("mask")){if(t==="-webkit-"||t==="-webkit- old")return super.add(e,t)}else if(i==="list-style"||i==="list-style-image"||i==="content"){if(t==="-webkit-"||t==="-webkit- old")return super.add(e,t)}else return super.add(e,t)}};wt.names=["linear-gradient","repeating-linear-gradient","radial-gradient","repeating-radial-gradient"];wt.directions={top:"bottom",left:"right",bottom:"top",right:"left"};wt.oldDirections={top:"left bottom, left top",left:"right top, left top",bottom:"left top, left bottom",right:"left top, right top","top right":"left bottom, right top","top left":"right bottom, left top","right top":"left bottom, right top","right bottom":"left top, right bottom","bottom right":"left top, right bottom","bottom left":"right top, left bottom","left top":"right bottom, left top","left bottom":"right top, left bottom"};z0.exports=wt});var W0=x((yL,H0)=>{u();var xR=xr(),kR=He();function V0(r){return new RegExp(`(^|[\\s,(])(${r}($|[\\s),]))`,"gi")}var Xu=class extends kR{regexp(){return this.regexpCache||(this.regexpCache=V0(this.name)),this.regexpCache}isStretch(){return this.name==="stretch"||this.name==="fill"||this.name==="fill-available"}replace(e,t){return t==="-moz-"&&this.isStretch()?e.replace(this.regexp(),"$1-moz-available$3"):t==="-webkit-"&&this.isStretch()?e.replace(this.regexp(),"$1-webkit-fill-available$3"):super.replace(e,t)}old(e){let t=e+this.name;return this.isStretch()&&(e==="-moz-"?t="-moz-available":e==="-webkit-"&&(t="-webkit-fill-available")),new xR(this.name,t,t,V0(t))}add(e,t){if(!(e.prop.includes("grid")&&t!=="-webkit-"))return super.add(e,t)}};Xu.names=["max-content","min-content","fit-content","fill","fill-available","stretch"];H0.exports=Xu});var Y0=x((bL,Q0)=>{u();var G0=xr(),SR=He(),Zu=class extends SR{replace(e,t){return t==="-webkit-"?e.replace(this.regexp(),"$1-webkit-optimize-contrast"):t==="-moz-"?e.replace(this.regexp(),"$1-moz-crisp-edges"):super.replace(e,t)}old(e){return e==="-webkit-"?new G0(this.name,"-webkit-optimize-contrast"):e==="-moz-"?new G0(this.name,"-moz-crisp-edges"):super.old(e)}};Zu.names=["pixelated"];Q0.exports=Zu});var X0=x((wL,K0)=>{u();var AR=He(),Ju=class extends AR{replace(e,t){let i=super.replace(e,t);return t==="-webkit-"&&(i=i.replace(/("[^"]+"|'[^']+')(\s+\d+\w)/gi,"url($1)$2")),i}};Ju.names=["image-set"];K0.exports=Ju});var J0=x((vL,Z0)=>{u();var CR=$e().list,_R=He(),ef=class extends _R{replace(e,t){return CR.space(e).map(i=>{if(i.slice(0,+this.name.length+1)!==this.name+"(")return i;let n=i.lastIndexOf(")"),s=i.slice(n+1),a=i.slice(this.name.length+1,n);if(t==="-webkit-"){let o=a.match(/\d*.?\d+%?/);o?(a=a.slice(o[0].length).trim(),a+=`, ${o[0]}`):a+=", 0.5"}return t+this.name+"("+a+")"+s}).join(" ")}};ef.names=["cross-fade"];Z0.exports=ef});var tv=x((xL,ev)=>{u();var ER=Pe(),OR=xr(),TR=He(),tf=class extends TR{constructor(e,t){super(e,t);e==="display-flex"&&(this.name="flex")}check(e){return e.prop==="display"&&e.value===this.name}prefixed(e){let t,i;return[t,e]=ER(e),t===2009?this.name==="flex"?i="box":i="inline-box":t===2012?this.name==="flex"?i="flexbox":i="inline-flexbox":t==="final"&&(i=this.name),e+i}replace(e,t){return this.prefixed(t)}old(e){let t=this.prefixed(e);if(!!t)return new OR(this.name,t)}};tf.names=["display-flex","inline-flex"];ev.exports=tf});var iv=x((kL,rv)=>{u();var RR=He(),rf=class extends RR{constructor(e,t){super(e,t);e==="display-grid"&&(this.name="grid")}check(e){return e.prop==="display"&&e.value===this.name}};rf.names=["display-grid","inline-grid"];rv.exports=rf});var sv=x((SL,nv)=>{u();var PR=He(),nf=class extends PR{constructor(e,t){super(e,t);e==="filter-function"&&(this.name="filter")}};nf.names=["filter","filter-function"];nv.exports=nf});var uv=x((AL,lv)=>{u();var av=Ni(),z=j(),ov=zy(),IR=ab(),DR=ou(),qR=Cb(),sf=Mt(),Pr=kr(),$R=Db(),ut=He(),Ir=_e(),LR=$b(),MR=Mb(),NR=Bb(),BR=jb(),FR=Wb(),jR=Yb(),zR=Xb(),UR=Jb(),VR=tw(),HR=iw(),WR=sw(),GR=ow(),QR=uw(),YR=cw(),KR=dw(),XR=gw(),ZR=bw(),JR=xw(),e5=Sw(),t5=Cw(),r5=Ow(),i5=Rw(),n5=Dw(),s5=$w(),a5=Mw(),o5=Bw(),l5=jw(),u5=Vw(),f5=Ww(),c5=Qw(),p5=Kw(),d5=Zw(),h5=e0(),m5=r0(),g5=s0(),y5=o0(),b5=u0(),w5=c0(),v5=d0(),x5=g0(),k5=b0(),S5=v0(),A5=S0(),C5=C0(),_5=E0(),E5=R0(),O5=I0(),T5=q0(),R5=U0(),P5=W0(),I5=Y0(),D5=X0(),q5=J0(),$5=tv(),L5=iv(),M5=sv();Pr.hack(LR);Pr.hack(MR);Pr.hack(NR);Pr.hack(BR);z.hack(FR);z.hack(jR);z.hack(zR);z.hack(UR);z.hack(VR);z.hack(HR);z.hack(WR);z.hack(GR);z.hack(QR);z.hack(YR);z.hack(KR);z.hack(XR);z.hack(ZR);z.hack(JR);z.hack(e5);z.hack(t5);z.hack(r5);z.hack(i5);z.hack(n5);z.hack(s5);z.hack(a5);z.hack(o5);z.hack(l5);z.hack(u5);z.hack(f5);z.hack(c5);z.hack(p5);z.hack(d5);z.hack(h5);z.hack(m5);z.hack(g5);z.hack(y5);z.hack(b5);z.hack(w5);z.hack(v5);z.hack(x5);z.hack(k5);z.hack(S5);z.hack(A5);z.hack(C5);z.hack(_5);z.hack(E5);z.hack(O5);z.hack(T5);ut.hack(R5);ut.hack(P5);ut.hack(I5);ut.hack(D5);ut.hack(q5);ut.hack($5);ut.hack(L5);ut.hack(M5);var af=new Map,Fi=class{constructor(e,t,i={}){this.data=e,this.browsers=t,this.options=i,[this.add,this.remove]=this.preprocess(this.select(this.data)),this.transition=new IR(this),this.processor=new DR(this)}cleaner(){if(this.cleanerCache)return this.cleanerCache;if(this.browsers.selected.length){let e=new sf(this.browsers.data,[]);this.cleanerCache=new Fi(this.data,e,this.options)}else return this;return this.cleanerCache}select(e){let t={add:{},remove:{}};for(let i in e){let n=e[i],s=n.browsers.map(l=>{let c=l.split(" ");return{browser:`${c[0]} ${c[1]}`,note:c[2]}}),a=s.filter(l=>l.note).map(l=>`${this.browsers.prefix(l.browser)} ${l.note}`);a=Ir.uniq(a),s=s.filter(l=>this.browsers.isSelected(l.browser)).map(l=>{let c=this.browsers.prefix(l.browser);return l.note?`${c} ${l.note}`:c}),s=this.sort(Ir.uniq(s)),this.options.flexbox==="no-2009"&&(s=s.filter(l=>!l.includes("2009")));let o=n.browsers.map(l=>this.browsers.prefix(l));n.mistakes&&(o=o.concat(n.mistakes)),o=o.concat(a),o=Ir.uniq(o),s.length?(t.add[i]=s,s.length<o.length&&(t.remove[i]=o.filter(l=>!s.includes(l)))):t.remove[i]=o}return t}sort(e){return e.sort((t,i)=>{let n=Ir.removeNote(t).length,s=Ir.removeNote(i).length;return n===s?i.length-t.length:s-n})}preprocess(e){let t={selectors:[],"@supports":new qR(Fi,this)};for(let n in e.add){let s=e.add[n];if(n==="@keyframes"||n==="@viewport")t[n]=new $R(n,s,this);else if(n==="@resolution")t[n]=new ov(n,s,this);else if(this.data[n].selector)t.selectors.push(Pr.load(n,s,this));else{let a=this.data[n].props;if(a){let o=ut.load(n,s,this);for(let l of a)t[l]||(t[l]={values:[]}),t[l].values.push(o)}else{let o=t[n]&&t[n].values||[];t[n]=z.load(n,s,this),t[n].values=o}}}let i={selectors:[]};for(let n in e.remove){let s=e.remove[n];if(this.data[n].selector){let a=Pr.load(n,s);for(let o of s)i.selectors.push(a.old(o))}else if(n==="@keyframes"||n==="@viewport")for(let a of s){let o=`@${a}${n.slice(1)}`;i[o]={remove:!0}}else if(n==="@resolution")i[n]=new ov(n,s,this);else{let a=this.data[n].props;if(a){let o=ut.load(n,[],this);for(let l of s){let c=o.old(l);if(c)for(let f of a)i[f]||(i[f]={}),i[f].values||(i[f].values=[]),i[f].values.push(c)}}else for(let o of s){let l=this.decl(n).old(n,o);if(n==="align-self"){let c=t[n]&&t[n].prefixes;if(c){if(o==="-webkit- 2009"&&c.includes("-webkit-"))continue;if(o==="-webkit-"&&c.includes("-webkit- 2009"))continue}}for(let c of l)i[c]||(i[c]={}),i[c].remove=!0}}}return[t,i]}decl(e){return af.has(e)||af.set(e,z.load(e)),af.get(e)}unprefixed(e){let t=this.normalize(av.unprefixed(e));return t==="flex-direction"&&(t="flex-flow"),t}normalize(e){return this.decl(e).normalize(e)}prefixed(e,t){return e=av.unprefixed(e),this.decl(e).prefixed(e,t)}values(e,t){let i=this[e],n=i["*"]&&i["*"].values,s=i[t]&&i[t].values;return n&&s?Ir.uniq(n.concat(s)):n||s||[]}group(e){let t=e.parent,i=t.index(e),{length:n}=t.nodes,s=this.unprefixed(e.prop),a=(o,l)=>{for(i+=o;i>=0&&i<n;){let c=t.nodes[i];if(c.type==="decl"){if(o===-1&&c.prop===s&&!sf.withPrefix(c.value)||this.unprefixed(c.prop)!==s)break;if(l(c)===!0)return!0;if(o===1&&c.prop===s&&!sf.withPrefix(c.value))break}i+=o}return!1};return{up(o){return a(-1,o)},down(o){return a(1,o)}}}};lv.exports=Fi});var cv=x((CL,fv)=>{u();fv.exports={"backdrop-filter":{feature:"css-backdrop-filter",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5","safari 16.5"]},element:{props:["background","background-image","border-image","mask","list-style","list-style-image","content","mask-image"],feature:"css-element-function",browsers:["firefox 114"]},"user-select":{mistakes:["-khtml-"],feature:"user-select-none",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5","safari 16.5"]},"background-clip":{feature:"background-clip-text",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},hyphens:{feature:"css-hyphens",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5","safari 16.5"]},fill:{props:["width","min-width","max-width","height","min-height","max-height","inline-size","min-inline-size","max-inline-size","block-size","min-block-size","max-block-size","grid","grid-template","grid-template-rows","grid-template-columns","grid-auto-columns","grid-auto-rows"],feature:"intrinsic-width",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"fill-available":{props:["width","min-width","max-width","height","min-height","max-height","inline-size","min-inline-size","max-inline-size","block-size","min-block-size","max-block-size","grid","grid-template","grid-template-rows","grid-template-columns","grid-auto-columns","grid-auto-rows"],feature:"intrinsic-width",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},stretch:{props:["width","min-width","max-width","height","min-height","max-height","inline-size","min-inline-size","max-inline-size","block-size","min-block-size","max-block-size","grid","grid-template","grid-template-rows","grid-template-columns","grid-auto-columns","grid-auto-rows"],feature:"intrinsic-width",browsers:["firefox 114"]},"fit-content":{props:["width","min-width","max-width","height","min-height","max-height","inline-size","min-inline-size","max-inline-size","block-size","min-block-size","max-block-size","grid","grid-template","grid-template-rows","grid-template-columns","grid-auto-columns","grid-auto-rows"],feature:"intrinsic-width",browsers:["firefox 114"]},"text-decoration-style":{feature:"text-decoration",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5"]},"text-decoration-color":{feature:"text-decoration",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5"]},"text-decoration-line":{feature:"text-decoration",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5"]},"text-decoration":{feature:"text-decoration",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5"]},"text-decoration-skip":{feature:"text-decoration",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5"]},"text-decoration-skip-ink":{feature:"text-decoration",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5"]},"text-size-adjust":{feature:"text-size-adjust",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5"]},"mask-clip":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-composite":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-image":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-origin":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-repeat":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-border-repeat":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-border-source":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},mask:{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-position":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-size":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-border":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-border-outset":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-border-width":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"mask-border-slice":{feature:"css-masks",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},"clip-path":{feature:"css-clip-path",browsers:["samsung 21"]},"box-decoration-break":{feature:"css-boxdecorationbreak",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5","opera 99","safari 16.5","samsung 21"]},appearance:{feature:"css-appearance",browsers:["samsung 21"]},"image-set":{props:["background","background-image","border-image","cursor","mask","mask-image","list-style","list-style-image","content"],feature:"css-image-set",browsers:["and_uc 15.5","chrome 109","samsung 21"]},"cross-fade":{props:["background","background-image","border-image","mask","list-style","list-style-image","content","mask-image"],feature:"css-cross-fade",browsers:["and_chr 114","and_uc 15.5","chrome 109","chrome 113","chrome 114","edge 114","opera 99","samsung 21"]},isolate:{props:["unicode-bidi"],feature:"css-unicode-bidi",browsers:["ios_saf 16.1","ios_saf 16.3","ios_saf 16.4","ios_saf 16.5","safari 16.5"]},"color-adjust":{feature:"css-color-adjust",browsers:["chrome 109","chrome 113","chrome 114","edge 114","opera 99"]}}});var dv=x((_L,pv)=>{u();pv.exports={}});var yv=x((EL,gv)=>{u();var N5=Yl(),{agents:B5}=(Ps(),Rs),of=Oy(),F5=Mt(),j5=uv(),z5=cv(),U5=dv(),hv={browsers:B5,prefixes:z5},mv=`
  Replace Autoprefixer \`browsers\` option to Browserslist config.
  Use \`browserslist\` key in \`package.json\` or \`.browserslistrc\` file.

  Using \`browsers\` option can cause errors. Browserslist config can
  be used for Babel, Autoprefixer, postcss-normalize and other tools.

  If you really need to use option, rename it to \`overrideBrowserslist\`.

  Learn more at:
  https://github.com/browserslist/browserslist#readme
  https://twitter.com/browserslist

`;function V5(r){return Object.prototype.toString.apply(r)==="[object Object]"}var lf=new Map;function H5(r,e){e.browsers.selected.length!==0&&(e.add.selectors.length>0||Object.keys(e.add).length>2||r.warn(`Autoprefixer target browsers do not need any prefixes.You do not need Autoprefixer anymore.
Check your Browserslist config to be sure that your targets are set up correctly.

  Learn more at:
  https://github.com/postcss/autoprefixer#readme
  https://github.com/browserslist/browserslist#readme

`))}gv.exports=Dr;function Dr(...r){let e;if(r.length===1&&V5(r[0])?(e=r[0],r=void 0):r.length===0||r.length===1&&!r[0]?r=void 0:r.length<=2&&(Array.isArray(r[0])||!r[0])?(e=r[1],r=r[0]):typeof r[r.length-1]=="object"&&(e=r.pop()),e||(e={}),e.browser)throw new Error("Change `browser` option to `overrideBrowserslist` in Autoprefixer");if(e.browserslist)throw new Error("Change `browserslist` option to `overrideBrowserslist` in Autoprefixer");e.overrideBrowserslist?r=e.overrideBrowserslist:e.browsers&&(typeof console!="undefined"&&console.warn&&(of.red?console.warn(of.red(mv.replace(/`[^`]+`/g,n=>of.yellow(n.slice(1,-1))))):console.warn(mv)),r=e.browsers);let t={ignoreUnknownVersions:e.ignoreUnknownVersions,stats:e.stats,env:e.env};function i(n){let s=hv,a=new F5(s.browsers,r,n,t),o=a.selected.join(", ")+JSON.stringify(e);return lf.has(o)||lf.set(o,new j5(s.prefixes,a,e)),lf.get(o)}return{postcssPlugin:"autoprefixer",prepare(n){let s=i({from:n.opts.from,env:e.env});return{OnceExit(a){H5(n,s),e.remove!==!1&&s.processor.remove(a,n),e.add!==!1&&s.processor.add(a,n)}}},info(n){return n=n||{},n.from=n.from||m.cwd(),U5(i(n))},options:e,browsers:r}}Dr.postcss=!0;Dr.data=hv;Dr.defaults=N5.defaults;Dr.info=()=>Dr().info()});var bv={};Ge(bv,{default:()=>W5});var W5,wv=P(()=>{u();W5=[]});var xv={};Ge(xv,{default:()=>G5});var vv,G5,kv=P(()=>{u();Xi();vv=pe(rn()),G5=St(vv.default.theme)});var Av={};Ge(Av,{default:()=>Q5});var Sv,Q5,Cv=P(()=>{u();Xi();Sv=pe(rn()),Q5=St(Sv.default)});u();"use strict";var Y5=vt(_y()),K5=vt($e()),X5=vt(yv()),Z5=vt((wv(),bv)),J5=vt((kv(),xv)),eP=vt((Cv(),Av)),tP=vt((Vs(),_f)),rP=vt((al(),sl)),iP=vt((sa(),sc));function vt(r){return r&&r.__esModule?r:{default:r}}console.warn("cdn.tailwindcss.com should not be used in production. To use Tailwind CSS in production, install it as a PostCSS plugin or use the Tailwind CLI: https://tailwindcss.com/docs/installation");var Ns="tailwind",uf="text/tailwindcss",_v="/template.html",Yt,Ev=!0,Ov=0,ff=new Set,cf,Tv="",Rv=(r=!1)=>({get(e,t){return(!r||t==="config")&&typeof e[t]=="object"&&e[t]!==null?new Proxy(e[t],Rv()):e[t]},set(e,t,i){return e[t]=i,(!r||t==="config")&&pf(!0),!0}});window[Ns]=new Proxy({config:{},defaultTheme:J5.default,defaultConfig:eP.default,colors:tP.default,plugin:rP.default,resolveConfig:iP.default},Rv(!0));function Pv(r){cf.observe(r,{attributes:!0,attributeFilter:["type"],characterData:!0,subtree:!0,childList:!0})}new MutationObserver(async r=>{let e=!1;if(!cf){cf=new MutationObserver(async()=>await pf(!0));for(let t of document.querySelectorAll(`style[type="${uf}"]`))Pv(t)}for(let t of r)for(let i of t.addedNodes)i.nodeType===1&&i.tagName==="STYLE"&&i.getAttribute("type")===uf&&(Pv(i),e=!0);await pf(e)}).observe(document.documentElement,{attributes:!0,attributeFilter:["class"],childList:!0,subtree:!0});async function pf(r=!1){r&&(Ov++,ff.clear());let e="";for(let i of document.querySelectorAll(`style[type="${uf}"]`))e+=i.textContent;let t=new Set;for(let i of document.querySelectorAll("[class]"))for(let n of i.classList)ff.has(n)||t.add(n);if(document.body&&(Ev||t.size>0||e!==Tv||!Yt||!Yt.isConnected)){for(let n of t)ff.add(n);Ev=!1,Tv=e,self[_v]=Array.from(t).join(" ");let{css:i}=await(0,K5.default)([(0,Y5.default)({...window[Ns].config,_hash:Ov,content:{files:[_v],extract:{html:n=>n.split(" ")}},plugins:[...Z5.default,...Array.isArray(window[Ns].config.plugins)?window[Ns].config.plugins:[]]}),(0,X5.default)({remove:!1})]).process(`@tailwind base;@tailwind components;@tailwind utilities;${e}`);(!Yt||!Yt.isConnected)&&(Yt=document.createElement("style"),document.head.append(Yt)),Yt.textContent=i}}})();
/*!
 * fill-range <https://github.com/jonschlinkert/fill-range>
 *
 * Copyright (c) 2014-present, Jon Schlinkert.
 * Licensed under the MIT License.
 */
/*!
 * is-number <https://github.com/jonschlinkert/is-number>
 *
 * Copyright (c) 2014-present, Jon Schlinkert.
 * Released under the MIT License.
 */
/*!
 * to-regex-range <https://github.com/micromatch/to-regex-range>
 *
 * Copyright (c) 2015-present, Jon Schlinkert.
 * Released under the MIT License.
 */
/*! https://mths.be/cssesc v3.0.0 by @mathias */
"##;

const VUE_JS: &str = r##"/**
* vue v3.5.42
* (c) 2018-present Yuxi (Evan) You and Vue contributors
* @license MIT
**/var Vue=function(e){"use strict";var t,n,r;let i,l,s,o,a,c,u,d,h,f,p,g,m;function y(e){let t=Object.create(null);for(let n of e.split(","))t[n]=1;return e=>e in t}let b={},_=[],S=()=>{},x=()=>!1,k=e=>111===e.charCodeAt(0)&&110===e.charCodeAt(1)&&(e.charCodeAt(2)>122||97>e.charCodeAt(2)),C=e=>e.startsWith("onUpdate:"),T=Object.assign,w=(e,t)=>{let n=e.indexOf(t);n>-1&&e.splice(n,1)},A=Object.prototype.hasOwnProperty,N=(e,t)=>A.call(e,t),E=Array.isArray,I=e=>"function"==typeof e,R=e=>"string"==typeof e,O=e=>"symbol"==typeof e,M=e=>null!==e&&"object"==typeof e,P=e=>(M(e)||I(e))&&I(e.then)&&I(e.catch),F=Object.prototype.toString,L=e=>R(e)&&"NaN"!==e&&"-"!==e[0]&&""+parseInt(e,10)===e,$=y(",key,ref,ref_for,ref_key,onVnodeBeforeMount,onVnodeMounted,onVnodeBeforeUpdate,onVnodeUpdated,onVnodeBeforeUnmount,onVnodeUnmounted"),D=y("bind,cloak,else-if,else,for,html,if,model,on,once,pre,show,slot,text,memo"),V=e=>{let t=Object.create(null);return n=>t[n]||(t[n]=e(n))},j=/-\w/g,B=V(e=>e.replace(j,e=>e.slice(1).toUpperCase())),U=/\B([A-Z])/g,H=V(e=>e.replace(U,"-$1").toLowerCase()),q=V(e=>e.charAt(0).toUpperCase()+e.slice(1)),W=V(e=>e?`on${q(e)}`:""),K=(e,t)=>!Object.is(e,t),z=(e,...t)=>{for(let n=0;n<e.length;n++)e[n](...t)},J=(e,t,n,r=!1)=>{Object.defineProperty(e,t,{configurable:!0,enumerable:!1,writable:r,value:n})},G=e=>{let t=parseFloat(e);return isNaN(t)?e:t},X=e=>{let t=R(e)?Number(e):0/0;return isNaN(t)?e:t},Q=()=>i||(i="u">typeof globalThis?globalThis:"u">typeof self?self:"u">typeof window?window:"u">typeof global?global:{}),Z=y("Infinity,undefined,NaN,isFinite,isNaN,parseFloat,parseInt,decodeURI,decodeURIComponent,encodeURI,encodeURIComponent,Math,Number,Date,Array,Object,Boolean,String,RegExp,Map,Set,JSON,Intl,BigInt,console,Error,Symbol");function Y(e){if(E(e)){let t={};for(let n=0;n<e.length;n++){let r=e[n],i=R(r)?er(r):Y(r);if(i)for(let e in i)t[e]=i[e]}return t}if(R(e)||M(e))return e}let ee=/;(?![^(]*\))/g,et=/:([^]+)/,en=/\/\*[^]*?\*\//g;function er(e){let t={};return e.replace(en,"").split(ee).forEach(e=>{if(e){let n=e.split(et);n.length>1&&(t[n[0].trim()]=n[1].trim())}}),t}function ei(e){let t="";if(R(e))t=e;else if(E(e))for(let n=0;n<e.length;n++){let r=ei(e[n]);r&&(t+=r+" ")}else if(M(e))for(let n in e)e[n]&&(t+=n+" ");return t.trim()}let el=y("html,body,base,head,link,meta,style,title,address,article,aside,footer,header,hgroup,h1,h2,h3,h4,h5,h6,nav,section,div,dd,dl,dt,figcaption,figure,picture,hr,img,li,main,ol,p,pre,ul,a,b,abbr,bdi,bdo,br,cite,code,data,dfn,em,i,kbd,mark,q,rp,rt,ruby,s,samp,small,span,strong,sub,sup,time,u,var,wbr,area,audio,map,track,video,embed,object,param,source,canvas,script,noscript,del,ins,caption,col,colgroup,table,thead,tbody,td,th,tr,button,datalist,fieldset,form,input,label,legend,meter,optgroup,option,output,progress,select,textarea,details,dialog,menu,summary,template,blockquote,iframe,tfoot"),es=y("svg,animate,animateMotion,animateTransform,circle,clipPath,color-profile,defs,desc,discard,ellipse,feBlend,feColorMatrix,feComponentTransfer,feComposite,feConvolveMatrix,feDiffuseLighting,feDisplacementMap,feDistantLight,feDropShadow,feFlood,feFuncA,feFuncB,feFuncG,feFuncR,feGaussianBlur,feImage,feMerge,feMergeNode,feMorphology,feOffset,fePointLight,feSpecularLighting,feSpotLight,feTile,feTurbulence,filter,foreignObject,g,hatch,hatchpath,image,line,linearGradient,marker,mask,mesh,meshgradient,meshpatch,meshrow,metadata,mpath,path,pattern,polygon,polyline,radialGradient,rect,set,solidcolor,stop,switch,symbol,text,textPath,title,tspan,unknown,use,view"),eo=y("annotation,annotation-xml,maction,maligngroup,malignmark,math,menclose,merror,mfenced,mfrac,mfraction,mglyph,mi,mlabeledtr,mlongdiv,mmultiscripts,mn,mo,mover,mpadded,mphantom,mprescripts,mroot,mrow,ms,mscarries,mscarry,msgroup,msline,mspace,msqrt,msrow,mstack,mstyle,msub,msubsup,msup,mtable,mtd,mtext,mtr,munder,munderover,none,semantics"),ea=y("area,base,br,col,embed,hr,img,input,link,meta,param,source,track,wbr"),ec=y("itemscope,allowfullscreen,formnovalidate,ismap,nomodule,novalidate,readonly");function eu(e,t){if(e.size!==t.size)return!1;let n=Array.from(t),r=new Uint8Array(n.length);for(let t of e){let e=-1;for(let i=0;i<n.length;i++)if(!r[i]&&ed(t,n[i])){e=i;break}if(e<0)return!1;r[e]=1}return!0}function ed(e,t){let n,r;if(e===t)return!0;let i="[object Date]"===(n=e,F.call(n)),l="[object Date]"===(r=t,F.call(r));if(i||l)return!!i&&!!l&&e.getTime()===t.getTime();if(i=O(e),l=O(t),i||l)return e===t;if(i=E(e),l=E(t),i||l)return!!i&&!!l&&function(e,t){if(e.length!==t.length)return!1;let n=!0;for(let r=0;n&&r<e.length;r++)n=ed(e[r],t[r]);return n}(e,t);if(i=M(e),l=M(t),i||l){let n,r,s,o;if(!i||!l)return!1;i="[object Map]"===(n=e,F.call(n));if(l="[object Map]"===(r=t,F.call(r)),i||l)return!!i&&!!l&&eu(e,t);i="[object Set]"===(s=e,F.call(s));if(l="[object Set]"===(o=t,F.call(o)),i||l)return!!i&&!!l&&eu(e,t);if(Object.keys(e).length!==Object.keys(t).length)return!1;for(let n in e){let r=e.hasOwnProperty(n),i=t.hasOwnProperty(n);if(r&&!i||!r&&i||!ed(e[n],t[n]))return!1}}return String(e)===String(t)}function eh(e,t){return e.findIndex(e=>ed(e,t))}let ef=e=>!!(e&&!0===e.__v_isRef),ep=e=>R(e)?e:null==e?"":E(e)||M(e)&&(e.toString===F||!I(e.toString))?ef(e)?ep(e.value):JSON.stringify(e,eg,2):String(e),eg=(e,t)=>{let n;if(ef(t))return eg(e,t.value);if("[object Map]"===(n=t,F.call(n)))return{[`Map(${t.size})`]:[...t.entries()].reduce((e,[t,n],r)=>(e[em(t,r)+" =>"]=n,e),{})};{let e;if("[object Set]"===(e=t,F.call(e)))return{[`Set(${t.size})`]:[...t.values()].map(e=>em(e))};else{if(O(t))return em(t);let e;if(M(t)&&!E(t)&&"[object Object]"!==(e=t,F.call(e)))return String(t)}}return t},em=(e,t="")=>{var n;return O(e)?`Symbol(${null!=(n=e.description)?n:t})`:e};class ev{constructor(e=!1){this.detached=e,this._active=!0,this._on=0,this.effects=[],this.cleanups=[],this._isPaused=!1,this._warnOnRun=!0,this.__v_skip=!0,!e&&l&&(l.active?(this.parent=l,this.index=(l.scopes||(l.scopes=[])).push(this)-1):(this._active=!1,this._warnOnRun=!1))}get active(){return this._active}pause(){if(this._active){let e,t;if(this._isPaused=!0,this.scopes){let n=this.scopes.slice();for(e=0,t=n.length;e<t;e++)n[e].pause()}for(e=0,t=this.effects.length;e<t;e++)this.effects[e].pause()}}resume(){if(this._active&&this._isPaused){let e,t;if(this._isPaused=!1,this.scopes){let n=this.scopes.slice();for(e=0,t=n.length;e<t;e++)n[e].resume()}let n=this.effects.slice();for(e=0,t=n.length;e<t;e++)n[e].resume()}}run(e){if(this._active){let t=l;try{return l=this,e()}finally{l=t}}}on(){1==++this._on&&(this.prevScope=l,l=this)}off(){if(this._on>0&&0==--this._on){if(l===this)l=this.prevScope;else{let e=l;for(;e;){if(e.prevScope===this){e.prevScope=this.prevScope;break}e=e.prevScope}}this.prevScope=void 0}}stop(e){if(this._active){let t,n;for(this._active=!1,t=0,n=this.effects.length;t<n;t++)this.effects[t].stop();for(this.effects.length=0,t=0,n=this.cleanups.length;t<n;t++)this.cleanups[t]();if(this.cleanups.length=0,this.scopes){let e=this.scopes.slice();for(t=0,n=e.length;t<n;t++)e[t].stop(!0);this.scopes.length=0}if(!this.detached&&this.parent&&!e){let e=this.parent.scopes.pop();e&&e!==this&&(this.parent.scopes[this.index]=e,e.index=this.index)}this.parent=void 0}}}let ey=new WeakSet;class eb{constructor(e){this.fn=e,this.deps=void 0,this.depsTail=void 0,this.flags=5,this.next=void 0,this.cleanup=void 0,this.scheduler=void 0,l&&(l.active?l.effects.push(this):this.flags&=-2)}pause(){this.flags|=64}resume(){64&this.flags&&(this.flags&=-65,ey.has(this)&&(ey.delete(this),this.trigger()))}notify(){(!(2&this.flags)||32&this.flags)&&(8&this.flags||eS(this))}run(){if(!(1&this.flags))return this.fn();this.flags|=2,eO(this),ek(this);let e=s,t=eN;s=this,eN=!0;try{return this.fn()}finally{eC(this),s=e,eN=t,this.flags&=-3}}stop(){if(1&this.flags){for(let e=this.deps;e;e=e.nextDep)eA(e);this.deps=this.depsTail=void 0,eO(this),this.onStop&&this.onStop(),this.flags&=-2}}trigger(){64&this.flags?ey.add(this):this.scheduler?this.scheduler():this.runIfDirty()}runIfDirty(){eT(this)&&this.run()}get dirty(){return eT(this)}}let e_=0;function eS(e,t=!1){if(e.flags|=8,t){e.next=a,a=e;return}e.next=o,o=e}function ex(){let e;if(!(--e_>0)){if(a){let e=a;for(a=void 0;e;){let t=e.next;e.next=void 0,e.flags&=-9,e=t}}for(;o;){let t=o;for(o=void 0;t;){let n=t.next;if(t.next=void 0,t.flags&=-9,1&t.flags)try{t.trigger()}catch(t){e||(e=t)}t=n}}if(e)throw e}}function ek(e){for(let t=e.deps;t;t=t.nextDep)t.version=-1,t.prevActiveLink=t.dep.activeLink,t.dep.activeLink=t}function eC(e){let t,n=e.depsTail,r=n;for(;r;){let e=r.prevDep;-1===r.version?(r===n&&(n=e),eA(r),function(e){let{prevDep:t,nextDep:n}=e;t&&(t.nextDep=n,e.prevDep=void 0),n&&(n.prevDep=t,e.nextDep=void 0)}(r)):t=r,r.dep.activeLink=r.prevActiveLink,r.prevActiveLink=void 0,r=e}e.deps=t,e.depsTail=n}function eT(e){for(let t=e.deps;t;t=t.nextDep)if(t.dep.version!==t.version||t.dep.computed&&(ew(t.dep.computed)||t.dep.version!==t.version))return!0;return!!e._dirty}function ew(e){if(4&e.flags&&!(16&e.flags)||(e.flags&=-17,e.globalVersion===eM)||(e.globalVersion=eM,!e.isSSR&&128&e.flags&&(!e.deps&&!e._dirty||!eT(e))))return;e.flags|=2;let t=e.dep,n=s,r=eN;s=e,eN=!0;try{ek(e);let n=e.fn(e._value);(0===t.version||K(n,e._value))&&(e.flags|=128,e._value=n,t.version++)}catch(e){throw t.version++,e}finally{s=n,eN=r,eC(e),e.flags&=-3}}function eA(e,t=!1){let{dep:n,prevSub:r,nextSub:i}=e;if(r&&(r.nextSub=i,e.prevSub=void 0),i&&(i.prevSub=r,e.nextSub=void 0),n.subs===e&&(n.subs=r,!r&&n.computed)){n.computed.flags&=-5;for(let e=n.computed.deps;e;e=e.nextDep)eA(e,!0)}t||--n.sc||!n.map||n.map.delete(n.key)}let eN=!0,eE=[];function eI(){eE.push(eN),eN=!1}function eR(){let e=eE.pop();eN=void 0===e||e}function eO(e){let{cleanup:t}=e;if(e.cleanup=void 0,t){let e=s;s=void 0;try{t()}finally{s=e}}}let eM=0;class eP{constructor(e,t){this.sub=e,this.dep=t,this.version=t.version,this.nextDep=this.prevDep=this.nextSub=this.prevSub=this.prevActiveLink=void 0}}class eF{constructor(e){this.computed=e,this.version=0,this.activeLink=void 0,this.subs=void 0,this.map=void 0,this.key=void 0,this.sc=0,this.__v_skip=!0}track(e){if(!s||!eN||s===this.computed)return;let t=this.activeLink;if(void 0===t||t.sub!==s)t=this.activeLink=new eP(s,this),s.deps?(t.prevDep=s.depsTail,s.depsTail.nextDep=t,s.depsTail=t):s.deps=s.depsTail=t,function e(t){if(t.dep.sc++,4&t.sub.flags){let n=t.dep.computed;if(n&&!t.dep.subs){n.flags|=20;for(let t=n.deps;t;t=t.nextDep)e(t)}let r=t.dep.subs;r!==t&&(t.prevSub=r,r&&(r.nextSub=t)),t.dep.subs=t}}(t);else if(-1===t.version&&(t.version=this.version,t.nextDep)){let e=t.nextDep;e.prevDep=t.prevDep,t.prevDep&&(t.prevDep.nextDep=e),t.prevDep=s.depsTail,t.nextDep=void 0,s.depsTail.nextDep=t,s.depsTail=t,s.deps===t&&(s.deps=e)}return t}trigger(e){this.version++,eM++,this.notify(e)}notify(e){e_++;try{for(let e=this.subs;e;e=e.prevSub)e.sub.notify()&&e.sub.dep.notify()}finally{ex()}}}let eL=new WeakMap,e$=Symbol(""),eD=Symbol(""),eV=Symbol("");function ej(e,t,n){if(eN&&s){let t=eL.get(e);t||eL.set(e,t=new Map);let r=t.get(n);r||(t.set(n,r=new eF),r.map=t,r.key=n),r.track()}}function eB(e,t,n,r,i,l){let s=eL.get(e);if(!s)return void eM++;let o=e=>{e&&e.trigger()};if(e_++,"clear"===t)s.forEach(o);else{let i=E(e),l=i&&L(n);if(i&&"length"===n){let e=Number(r);s.forEach((t,n)=>{("length"===n||n===eV||!O(n)&&n>=e)&&o(t)})}else switch((void 0!==n||s.has(void 0))&&o(s.get(n)),l&&o(s.get(eV)),t){case"add":if(i)l&&o(s.get("length"));else{let t;o(s.get(e$));"[object Map]"===(t=e,F.call(t))&&o(s.get(eD))}break;case"delete":if(!i){let t;o(s.get(e$));"[object Map]"===(t=e,F.call(t))&&o(s.get(eD))}break;case"set":let a;"[object Map]"===(a=e,F.call(a))&&o(s.get(e$))}}ex()}function eU(e){let t=tv(e);return t===e?t:(ej(t,"iterate",eV),tg(e)?t:t.map(tb))}function eH(e){return ej(e=tv(e),"iterate",eV),e}function eq(e,t){return tp(e)?tf(e)?t_(tb(t)):t_(t):tb(t)}let eW={__proto__:null,[Symbol.iterator](){return eK(this,Symbol.iterator,e=>eq(this,e))},concat(...e){return eU(this).concat(...e.map(e=>E(e)?eU(e):e))},entries(){return eK(this,"entries",e=>(e[1]=eq(this,e[1]),e))},every(e,t){return eJ(this,"every",e,t,void 0,arguments)},filter(e,t){return eJ(this,"filter",e,t,e=>e.map(e=>eq(this,e)),arguments)},find(e,t){return eJ(this,"find",e,t,e=>eq(this,e),arguments)},findIndex(e,t){return eJ(this,"findIndex",e,t,void 0,arguments)},findLast(e,t){return eJ(this,"findLast",e,t,e=>eq(this,e),arguments)},findLastIndex(e,t){return eJ(this,"findLastIndex",e,t,void 0,arguments)},forEach(e,t){return eJ(this,"forEach",e,t,void 0,arguments)},includes(...e){return eX(this,"includes",e)},indexOf(...e){return eX(this,"indexOf",e)},join(e){return eU(this).join(e)},lastIndexOf(...e){return eX(this,"lastIndexOf",e)},map(e,t){return eJ(this,"map",e,t,void 0,arguments)},pop(){return eQ(this,"pop")},push(...e){return eQ(this,"push",e)},reduce(e,...t){return eG(this,"reduce",e,t)},reduceRight(e,...t){return eG(this,"reduceRight",e,t)},shift(){return eQ(this,"shift")},some(e,t){return eJ(this,"some",e,t,void 0,arguments)},splice(...e){return eQ(this,"splice",e)},toReversed(){return eU(this).toReversed()},toSorted(e){return eU(this).toSorted(e)},toSpliced(...e){return eU(this).toSpliced(...e)},unshift(...e){return eQ(this,"unshift",e)},values(){return eK(this,"values",e=>eq(this,e))}};function eK(e,t,n){let r=eH(e),i=r[t]();return r===e||tg(e)||(i._next=i.next,i.next=()=>{let e=i._next();return e.done||(e.value=n(e.value)),e}),i}let ez=Array.prototype;function eJ(e,t,n,r,i,l){let s=eH(e),o=s!==e&&!tg(e),a=s[t];if(a!==ez[t]){let t=a.apply(e,l);return o?tb(t):t}let c=n;s!==e&&(o?c=function(t,r){return n.call(this,eq(e,t),r,e)}:n.length>2&&(c=function(t,r){return n.call(this,t,r,e)}));let u=a.call(s,c,r);return o&&i?i(u):u}function eG(e,t,n,r){let i=eH(e),l=i!==e&&!tg(e),s=n,o=!1;i!==e&&(l?(o=0===r.length,s=function(t,r,i){return o&&(o=!1,t=eq(e,t)),n.call(this,t,eq(e,r),i,e)}):n.length>3&&(s=function(t,r,i){return n.call(this,t,r,i,e)}));let a=i[t](s,...r);return o?eq(e,a):a}function eX(e,t,n){let r=tv(e);ej(r,"iterate",eV);let i=r[t](...n);return(-1===i||!1===i)&&tm(n[0])?(n[0]=tv(n[0]),r[t](...n)):i}function eQ(e,t,n=[]){eI(),e_++;let r=tv(e)[t].apply(e,n);return ex(),eR(),r}let eZ=y("__proto__,__v_isRef,__isVue"),eY=new Set((((Object.getOwnPropertyNames(Symbol)).filter(e=>"arguments"!==e&&"caller"!==e)).map(e=>Symbol[e])).filter(O));function e0(e){O(e)||(e=String(e));let t=tv(this);return ej(t,"has",e),t.hasOwnProperty(e)}class e1{constructor(e=!1,t=!1){this._isReadonly=e,this._isShallow=t}get(e,t,n){if("__v_skip"===t)return e.__v_skip;let r=this._isReadonly,i=this._isShallow;if("__v_isReactive"===t)return!r;if("__v_isReadonly"===t)return r;if("__v_isShallow"===t)return i;if("__v_raw"===t)return n===(r?i?ta:to:i?ts:tl).get(e)||Object.getPrototypeOf(e)===Object.getPrototypeOf(n)?e:void 0;let l=E(e);if(!r){let e;if(l&&(e=eW[t]))return e;if("hasOwnProperty"===t)return e0}let s=Reflect.get(e,t,tS(e)?e:n);if((O(t)?eY.has(t):eZ(t))||(r||ej(e,"get",t),i))return s;if(tS(s)){let e=l&&L(t)?s:s.value;return r&&M(e)?td(e):e}return M(s)?r?td(s):tc(s):s}}class e2 extends e1{constructor(e=!1){super(!1,e)}set(e,t,n,r){let i=e[t],l=E(e)&&L(t);if(!this._isShallow){let e=tp(i);if(tg(n)||tp(n)||(i=tv(i),n=tv(n)),!l&&tS(i)&&!tS(n))if(e)return!0;else return i.value=n,!0}let s=l?Number(t)<e.length:N(e,t),o=Reflect.set(e,t,n,tS(e)?e:r);return e===tv(r)&&o&&(s?K(n,i)&&eB(e,"set",t,n):eB(e,"add",t,n)),o}deleteProperty(e,t){let n=N(e,t);e[t];let r=Reflect.deleteProperty(e,t);return r&&n&&eB(e,"delete",t,void 0),r}has(e,t){let n=Reflect.has(e,t);return O(t)&&eY.has(t)||ej(e,"has",t),n}ownKeys(e){return ej(e,"iterate",E(e)?"length":e$),Reflect.ownKeys(e)}}class e3 extends e1{constructor(e=!1){super(!0,e)}set(e,t){return!0}deleteProperty(e,t){return!0}}let e6=new e2,e4=new e3,e8=new e2(!0),e5=new e3(!0),e9=e=>e;function e7(e){return function(){return"delete"!==e&&("clear"===e?void 0:this)}}function te(e,t){let n,r=(T(n={get(n){let r=this.__v_raw,i=tv(r),l=tv(n);e||(K(n,l)&&ej(i,"get",n),ej(i,"get",l));let{has:s}=Reflect.getPrototypeOf(i),o=t?e9:e?t_:tb;return s.call(i,n)?o(r.get(n)):s.call(i,l)?o(r.get(l)):void(r!==i&&r.get(n))},get size(){let t=this.__v_raw;return e||ej(tv(t),"iterate",e$),t.size},has(t){let n=this.__v_raw,r=tv(n),i=tv(t);return e||(K(t,i)&&ej(r,"has",t),ej(r,"has",i)),t===i?n.has(t):n.has(t)||n.has(i)},forEach(n,r){let i=this,l=i.__v_raw,s=tv(l),o=t?e9:e?t_:tb;return e||ej(s,"iterate",e$),l.forEach((e,t)=>n.call(r,o(e),o(t),i))}},e?{add:e7("add"),set:e7("set"),delete:e7("delete"),clear:e7("clear")}:{add(e){let n=tv(this),r=Reflect.getPrototypeOf(n),i=tv(e),l=t||tg(e)||tp(e)?e:i;return r.has.call(n,l)||K(e,l)&&r.has.call(n,e)||K(i,l)&&r.has.call(n,i)||(n.add(l),eB(n,"add",l,l)),this},set(e,n){t||tg(n)||tp(n)||(n=tv(n));let r=tv(this),{has:i,get:l}=Reflect.getPrototypeOf(r),s=i.call(r,e);s||(e=tv(e),s=i.call(r,e));let o=l.call(r,e);return r.set(e,n),s?K(n,o)&&eB(r,"set",e,n):eB(r,"add",e,n),this},delete(e){let t=tv(this),{has:n,get:r}=Reflect.getPrototypeOf(t),i=n.call(t,e);i||(e=tv(e),i=n.call(t,e)),r&&r.call(t,e);let l=t.delete(e);return i&&eB(t,"delete",e,void 0),l},clear(){let e=tv(this),t=0!==e.size,n=e.clear();return t&&eB(e,"clear",void 0,void 0),n}}),["keys","values","entries",Symbol.iterator].forEach(r=>{n[r]=function(...n){let i,l=this.__v_raw,s=tv(l),o="[object Map]"===(i=s,F.call(i)),a="entries"===r||r===Symbol.iterator&&o,c=l[r](...n),u=t?e9:e?t_:tb;return e||ej(s,"iterate","keys"===r&&o?eD:e$),T(Object.create(c),{next(){let{value:e,done:t}=c.next();return t?{value:e,done:t}:{value:a?[u(e[0]),u(e[1])]:u(e),done:t}}})}}),n);return(t,n,i)=>"__v_isReactive"===n?!e:"__v_isReadonly"===n?e:"__v_raw"===n?t:Reflect.get(N(r,n)&&n in t?r:t,n,i)}let tt={get:te(!1,!1)},tn={get:te(!1,!0)},tr={get:te(!0,!1)},ti={get:te(!0,!0)},tl=new WeakMap,ts=new WeakMap,to=new WeakMap,ta=new WeakMap;function tc(e){return tp(e)?e:th(e,!1,e6,tt,tl)}function tu(e){return th(e,!1,e8,tn,ts)}function td(e){return th(e,!0,e4,tr,to)}function th(e,t,n,r,i){let l;if(!M(e)||e.__v_raw&&!(t&&e.__v_isReactive)||e.__v_skip||!Object.isExtensible(e))return e;let s=i.get(e);if(s)return s;let o=function(e){switch(e){case"Object":case"Array":return 1;case"Map":case"Set":case"WeakMap":case"WeakSet":return 2;default:return 0}}((l=e,F.call(l)).slice(8,-1));if(0===o)return e;let a=new Proxy(e,2===o?r:n);return i.set(e,a),a}function tf(e){return tp(e)?tf(e.__v_raw):!!(e&&e.__v_isReactive)}function tp(e){return!!(e&&e.__v_isReadonly)}function tg(e){return!!(e&&e.__v_isShallow)}function tm(e){return!!e&&!!e.__v_raw}function tv(e){let t=e&&e.__v_raw;return t?tv(t):e}function ty(e){return!N(e,"__v_skip")&&Object.isExtensible(e)&&J(e,"__v_skip",!0),e}let tb=e=>M(e)?tc(e):e,t_=e=>M(e)?td(e):e;function tS(e){return!!e&&!0===e.__v_isRef}function tx(e){return tC(e,!1)}function tk(e){return tC(e,!0)}function tC(e,t){return tS(e)?e:new tT(e,t)}class tT{constructor(e,t){this.dep=new eF,this.__v_isRef=!0,this.__v_isShallow=!1,this._rawValue=t?e:tv(e),this._value=t?e:tb(e),this.__v_isShallow=t}get value(){return this.dep.track(),this._value}set value(e){let t=this._rawValue,n=this.__v_isShallow||tg(e)||tp(e);K(e=n?e:tv(e),t)&&(this._rawValue=e,this._value=n?e:tb(e),this.dep.trigger())}}function tw(e){return tS(e)?e.value:e}let tA={get:(e,t,n)=>"__v_raw"===t?e:tw(Reflect.get(e,t,n)),set:(e,t,n,r)=>{let i=e[t];return tS(i)&&!tS(n)?(i.value=n,!0):Reflect.set(e,t,n,r)}};function tN(e){return tf(e)?e:new Proxy(e,tA)}class tE{constructor(e){this.__v_isRef=!0,this._value=void 0;const t=this.dep=new eF,{get:n,set:r}=e(t.track.bind(t),t.trigger.bind(t));this._get=n,this._set=r}get value(){return this._value=this._get()}set value(e){this._set(e)}}function tI(e){return new tE(e)}class tR{constructor(e,t,n){this._object=e,this._defaultValue=n,this.__v_isRef=!0,this._value=void 0,this._key=O(t)?t:String(t),this._raw=tv(e);let r=!0,i=e;if(!E(e)||O(this._key)||!L(this._key))do r=!tm(i)||tg(i);while(r&&(i=i.__v_raw));this._shallow=r}get value(){let e=this._object[this._key];return this._shallow&&(e=tw(e)),this._value=void 0===e?this._defaultValue:e}set value(e){if(this._shallow&&tS(this._raw[this._key])){let t=this._object[this._key];if(tS(t)){t.value=e;return}}this._object[this._key]=e}get dep(){var e,t;let n;return e=this._raw,t=this._key,(n=eL.get(e))&&n.get(t)}}class tO{constructor(e){this._getter=e,this.__v_isRef=!0,this.__v_isReadonly=!0,this._value=void 0}get value(){return this._value=this._getter()}}class tM{constructor(e,t,n){this.fn=e,this.setter=t,this._value=void 0,this.dep=new eF(this),this.__v_isRef=!0,this.deps=void 0,this.depsTail=void 0,this.flags=16,this.globalVersion=eM-1,this.next=void 0,this.effect=this,this.__v_isReadonly=!t,this.isSSR=n}notify(){if(this.flags|=16,!(8&this.flags)&&s!==this)return eS(this,!0),!0}get value(){let e=this.dep.track();return ew(this),e&&(e.version=this.dep.version),this._value}set value(e){this.setter&&this.setter(e)}}let tP={},tF=new WeakMap;function tL(e,t=!1,n=g){if(n){let t=tF.get(n);t||tF.set(n,t=[]),t.push(e)}}function t$(e,t=1/0,n){if(t<=0||!M(e)||e.__v_skip||((n=n||new Map).get(e)||0)>=t)return e;if(n.set(e,t),t--,tS(e))t$(e.value,t,n);else if(E(e))for(let r=0;r<e.length;r++)t$(e[r],t,n);else{let r,i;if("[object Set]"===(r=e,F.call(r))||"[object Map]"===(i=e,F.call(i)))e.forEach(e=>{t$(e,t,n)});else{let r;if("[object Object]"===(r=e,F.call(r))){for(let r in e)t$(e[r],t,n);for(let r of Object.getOwnPropertySymbols(e))Object.prototype.propertyIsEnumerable.call(e,r)&&t$(e[r],t,n)}}}return e}function tD(e,t,n,r){try{return r?e(...r):e()}catch(e){tj(e,t,n)}}function tV(e,t,n,r){if(I(e)){let i=tD(e,t,n,r);return i&&P(i)&&i.catch(e=>{tj(e,t,n)}),i}if(E(e)){let i=[];for(let l=0;l<e.length;l++)i.push(tV(e[l],t,n,r));return i}}function tj(e,t,n,r=!0){let i=t?t.vnode:null,{errorHandler:l,throwUnhandledErrorInProduction:s}=t&&t.appContext.config||b;if(t){let r=t.parent,i=t.proxy,s=`https://vuejs.org/error-reference/#runtime-${n}`;for(;r;){let t=r.ec;if(t){for(let n=0;n<t.length;n++)if(!1===t[n](e,i,s))return}r=r.parent}if(l){eI(),tD(l,null,10,[e,i,s]),eR();return}}!function(e,t=!0,n=!1){if(n)throw e;console.error(e)}(e,r,s)}let tB=[],tU=-1,tH=[],tq=null,tW=0,tK=Promise.resolve(),tz=null;function tJ(e){let t=tz||tK;return e?t.then(this?e.bind(this):e):t}function tG(e){if(!(1&e.flags)){let t=t0(e),n=tB[tB.length-1];!n||!(2&e.flags)&&t>=t0(n)?tB.push(e):tB.splice(function(e){let t=tU+1,n=tB.length;for(;t<n;){let r=t+n>>>1,i=tB[r],l=t0(i);l<e||l===e&&2&i.flags?t=r+1:n=r}return t}(t),0,e),e.flags|=1,tX()}}function tX(){tz||(tz=tK.then(function e(t){try{for(tU=0;tU<tB.length;tU++){let e=tB[tU];e&&!(8&e.flags)&&(4&e.flags&&(e.flags&=-2),tD(e,e.i,e.i?15:14),4&e.flags||(e.flags&=-2))}}finally{for(;tU<tB.length;tU++){let e=tB[tU];e&&(e.flags&=-2)}tU=-1,tB.length=0,tY(),tz=null,(tB.length||tH.length)&&e()}}))}function tQ(e){if(E(e))for(let t=0;t<e.length;t++)tH.push(e[t]);else tq&&-1===e.id?tq.splice(tW+1,0,e):1&e.flags||(tH.push(e),e.flags|=1);tX()}function tZ(e,t,n=tU+1){for(;n<tB.length;n++){let t=tB[n];if(t&&2&t.flags){if(e&&t.id!==e.uid)continue;tB.splice(n,1),n--,4&t.flags&&(t.flags&=-2),t(),4&t.flags||(t.flags&=-2)}}}function tY(e){if(tH.length){let e=[...new Set(tH)].sort((e,t)=>t0(e)-t0(t));if(tH.length=0,tq){for(let t=0;t<e.length;t++)tq.push(e[t]);return}for(tq=e,tW=0;tW<tq.length;tW++){let e=tq[tW];4&e.flags&&(e.flags&=-2),8&e.flags||e(),e.flags&=-2}tq=null,tW=0}}let t0=e=>null==e.id?2&e.flags?-1:1/0:e.id,t1=null,t2=null;function t3(e){let t=t1;return t1=e,t2=e&&e.type.__scopeId||null,t}function t6(e,t=t1,n){if(!t||e._n)return e;let r=(...n)=>{let i;r._d&&ia(-1);let l=t3(t),s=ir.length;try{i=e(...n)}finally{for(let e=ir.length;e>s;e--)is();t3(l),r._d&&ia(1)}return i};return r._n=!0,r._c=!0,r._d=!0,r}function t4(e,t,n,r){let i=e.dirs,l=t&&t.dirs;for(let s=0;s<i.length;s++){let o=i[s];l&&(o.oldValue=l[s].value);let a=o.dir[r];a&&(eI(),tV(a,n,8,[e.el,o,e,t]),eR())}}function t8(e,t){if(iE){let n=iE.provides,r=iE.parent&&iE.parent.provides;r===n&&(n=iE.provides=Object.create(r)),n[e]=t}}function t5(e,t,n=!1){let r=iI();if(r||rC){let i=rC?rC._context.provides:r?null==r.parent||r.ce?r.vnode.appContext&&r.vnode.appContext.provides:r.parent.provides:void 0;if(i&&e in i)return i[e];if(arguments.length>1)return n&&I(t)?t.call(r&&r.proxy):t}}let t9=Symbol.for("v-scx");function t7(e,t){return ne(e,null,{flush:"sync"})}function ne(e,t,n=b){let{flush:r}=n,i=T({},n),s=iE;i.call=(e,t,n)=>tV(e,s,t,n);let o=!1;return"post"===r?i.scheduler=e=>{rz(e,s&&s.suspense)}:"sync"!==r&&(o=!0,i.scheduler=(e,t)=>{t?e():tG(e)}),i.augmentJob=e=>{t&&(e.flags|=4),o&&(e.flags|=2,s&&(e.id=s.uid,e.i=s))},function(e,t,n=b){let r,i,s,o,{immediate:a,deep:c,once:u,scheduler:d,augmentJob:h,call:f}=n,p=e=>c?e:tg(e)||!1===c||0===c?t$(e,1):t$(e),m=!1,y=!1;if(tS(e)?(i=()=>e.value,m=tg(e)):tf(e)?(i=()=>p(e),m=!0):E(e)?(y=!0,m=e.some(e=>tf(e)||tg(e)),i=()=>e.map(e=>tS(e)?e.value:tf(e)?p(e):I(e)?f?f(e,2):e():void 0)):i=I(e)?t?f?()=>f(e,2):e:()=>{if(s){eI();try{s()}finally{eR()}}let t=g;g=r;try{return f?f(e,3,[o]):e(o)}finally{g=t}}:S,t&&c){let e=i,t=!0===c?1/0:c;i=()=>t$(e(),t)}let _=l,x=()=>{r.stop(),_&&_.active&&w(_.effects,r)};if(u&&t){let e=t;t=(...t)=>{let n=e(...t);return x(),n}}let k=y?Array(e.length).fill(tP):tP,C=e=>{if(1&r.flags&&(r.dirty||e))if(t){let n=r.run();if(e||c||m||(y?n.some((e,t)=>K(e,k[t])):K(n,k))){s&&s();let e=g;g=r;try{let e=[n,k===tP?void 0:y&&k[0]===tP?[]:k,o];k=n,f?f(t,3,e):t(...e)}finally{g=e}}}else r.run()};return h&&h(C),(r=new eb(i)).scheduler=d?()=>d(C,!1):C,o=e=>tL(e,!1,r),s=r.onStop=()=>{let e=tF.get(r);if(e){if(f)f(e,4);else for(let t of e)t();tF.delete(r)}},t?a?C(!0):k=r.run():d?d(C.bind(null,!0),!0):r.run(),x.pause=r.pause.bind(r),x.resume=r.resume.bind(r),x.stop=x,x}(e,t,i)}function nt(e,t,n){let r,i=this.proxy,l=R(e)?e.includes(".")?nn(i,e):()=>i[e]:e.bind(i,i);I(t)?r=t:(r=t.handler,n=t);let s=iR(this),o=ne(l,r.bind(i),n);return s(),o}function nn(e,t){let n=t.split(".");return()=>{let t=e;for(let e=0;e<n.length&&t;e++)t=t[n[e]];return t}}let nr=new WeakMap,ni=Symbol("_vte"),nl=e=>e&&(e.disabled||""===e.disabled),ns=e=>"u">typeof SVGElement&&e instanceof SVGElement,no=e=>"function"==typeof MathMLElement&&e instanceof MathMLElement,na=(e,t)=>{let n=e&&e.to;return R(n)?t?t(n):null:n};function nc(e,t,n,{o:{insert:r},m:i},l=2){0===l&&r(e.targetAnchor,t,n);let{el:s,anchor:o,shapeFlag:a,children:c,props:u}=e,d=2===l;if(d&&r(s,t,n),!nr.has(e)&&(!d||nl(u))&&16&a)for(let e=0;e<c.length;e++)i(c[e],t,n,2);d&&r(o,t,n)}function nu(e,t){let n=e.ctx;if(n&&n.ut){let r,i;for(t?(r=e.el,i=e.anchor):(r=e.targetStart,i=e.targetAnchor);r&&r!==i;)1===r.nodeType&&r.setAttribute("data-v-owner",n.uid),r=r.nextSibling;n.ut()}}function nd(e,t,n,r,i=null){let l=t.targetStart=n(""),s=t.targetAnchor=n("");return l[ni]=s,e&&(r(l,e,i),r(s,e,i)),s}let nh=Symbol("_leaveCb"),nf=Symbol("_enterCb");function np(){let e={isMounted:!1,isLeaving:!1,isUnmounting:!1,leavingVNodes:new Map};return n2(()=>{e.isMounted=!0}),n4(()=>{e.isUnmounting=!0}),e}let ng=[Function,Array],nm={mode:String,appear:Boolean,persisted:Boolean,onBeforeEnter:ng,onEnter:ng,onAfterEnter:ng,onEnterCancelled:ng,onBeforeLeave:ng,onLeave:ng,onAfterLeave:ng,onLeaveCancelled:ng,onBeforeAppear:ng,onAppear:ng,onAfterAppear:ng,onAppearCancelled:ng},nv=e=>{let t=e.subTree;return t.component?nv(t.component):t};function ny(e){let t=e[0];if(e.length>1){for(let n of e)if(n.type!==ie){t=n;break}}return t}let nb={name:"BaseTransition",props:nm,setup(e,{slots:t}){let n=iI(),r=np();return()=>{let i=t.default&&nT(t.default(),!0),l=i&&i.length?ny(i):n.subTree?iS():void 0;if(!l)return;let s=tv(e),{mode:o}=s;if(r.isLeaving)return nx(l);let a=nk(l);if(!a)return nx(l);let c=nS(a,s,r,n,e=>c=e);a.type!==ie&&nC(a,c);let u=n.subTree&&nk(n.subTree);if(u&&u.type!==ie&&!ih(u,a)&&nv(n).type!==ie){let e=nS(u,s,r,n);if(nC(u,e),"out-in"===o&&a.type!==ie)return r.isLeaving=!0,e.afterLeave=()=>{r.isLeaving=!1,8&n.job.flags||n.update(),delete e.afterLeave,u=void 0},nx(l);"in-out"===o&&a.type!==ie?e.delayLeave=(e,t,n)=>{n_(r,u)[String(u.key)]=u,e[nh]=()=>{t(),e[nh]=void 0,delete c.delayedLeave,u=void 0},c.delayedLeave=()=>{n(),delete c.delayedLeave,u=void 0}}:u=void 0}else u&&(u=void 0);return l}}};function n_(e,t){let{leavingVNodes:n}=e,r=n.get(t.type);return r||(r=Object.create(null),n.set(t.type,r)),r}function nS(e,t,n,r,i){let{appear:l,mode:s,persisted:o=!1,onBeforeEnter:a,onEnter:c,onAfterEnter:u,onEnterCancelled:d,onBeforeLeave:h,onLeave:f,onAfterLeave:p,onLeaveCancelled:g,onBeforeAppear:m,onAppear:y,onAfterAppear:b,onAppearCancelled:_}=t,S=String(e.key),x=n_(n,e),k=(e,t)=>{e&&tV(e,r,9,t)},C=(e,t)=>{let n=t[1];k(e,t),E(e)?e.every(e=>e.length<=1)&&n():e.length<=1&&n()},T={mode:s,persisted:o,beforeEnter(t){let r=a;if(!n.isMounted)if(!l)return;else r=m||a;t[nh]&&t[nh](!0);let i=x[S];i&&ih(e,i)&&i.el[nh]&&i.el[nh](),k(r,[t])},enter(t){if(x[S]===e)return;let r=c,i=u,s=d;if(!n.isMounted)if(!l)return;else r=y||c,i=b||u,s=_||d;let o=!1;t[nf]=e=>{o||(o=!0,e?k(s,[t]):k(i,[t]),T.delayedLeave&&T.delayedLeave(),t[nf]=void 0)};let a=t[nf].bind(null,!1);r?C(r,[t,a]):a()},leave(t,r){let i=String(e.key);if(t[nf]&&t[nf](!0),n.isUnmounting)return r();k(h,[t]);let l=!1;t[nh]=n=>{l||(l=!0,r(),n?k(g,[t]):k(p,[t]),t[nh]=void 0,x[i]===e&&delete x[i])};let s=t[nh].bind(null,!1);x[i]=e,f?C(f,[t,s]):s()},clone(e){let l=nS(e,t,n,r,i);return i&&i(l),l}};return T}function nx(e){if(nK(e))return(e=ib(e)).children=null,e}function nk(e){if(!nK(e))return e.type.__isTeleport&&e.children?ny(e.children):e;if(e.component)return e.component.subTree;let{shapeFlag:t,children:n}=e;if(n){if(16&t)return n[0];if(32&t&&I(n.default))return n.default()}}function nC(e,t){if(6&e.shapeFlag&&e.component){e.transition=t;let n=e.component.subTree;nC(n.type.__isTeleport&&nk(n)||n,t)}else 128&e.shapeFlag?(e.ssContent.transition=t.clone(e.ssContent),e.ssFallback.transition=t.clone(e.ssFallback)):e.transition=t}function nT(e,t=!1,n){let r=[],i=0;for(let l=0;l<e.length;l++){let s=e[l],o=null==n?s.key:String(n)+String(null!=s.key?s.key:l);s.type===r9?(128&s.patchFlag&&i++,r=r.concat(nT(s.children,t,o))):(t||s.type!==ie)&&r.push(null!=o?ib(s,{key:o}):s)}if(i>1)for(let e=0;e<r.length;e++)r[e].patchFlag=-2;return r}function nw(e,t){return I(e)?T({name:e.name},t,{setup:e}):e}function nA(e){e.ids=[e.ids[0]+e.ids[2]+++"-",0,0]}function nN(e,t){let n;return!!((n=Object.getOwnPropertyDescriptor(e,t))&&!n.configurable)}let nE=new WeakMap;function nI(e,t,n,r,i=!1){if(E(e))return void e.forEach((e,l)=>nI(e,t&&(E(t)?t[l]:t),n,r,i));if(nq(r)&&!i){512&r.shapeFlag&&r.type.__asyncResolved&&r.component.subTree.component&&nI(e,t,n,r.component.subTree);return}let l=4&r.shapeFlag?ij(r.component):r.el,s=i?null:l,{i:o,r:a}=e,c=t&&t.r,u=o.refs===b?o.refs={}:o.refs,d=o.setupState,h=tv(d),f=d===b?x:e=>!nN(u,e)&&N(h,e),p=(e,t)=>!(t&&nN(u,t));if(null!=c&&c!==a&&(nR(t),R(c)?(u[c]=null,f(c)&&(d[c]=null)):tS(c)&&(p(c,t.k)&&(c.value=null),t.k&&(u[t.k]=null))),I(a))tD(a,o,12,[s,u]);else{let t=R(a),r=tS(a);if(t||r){let o=()=>{if(e.f){let n=t?f(a)?d[a]:u[a]:p()||!e.k?a.value:u[e.k];if(i)E(n)&&w(n,l);else if(E(n))n.includes(l)||n.push(l);else if(t)u[a]=[l],f(a)&&(d[a]=u[a]);else{let t=[l];p(a,e.k)&&(a.value=t),e.k&&(u[e.k]=t)}}else t?(u[a]=s,f(a)&&(d[a]=s)):r&&(p(a,e.k)&&(a.value=s),e.k&&(u[e.k]=s))};if(s){let t=()=>{o(),nE.delete(e)};t.id=-1,nE.set(e,t),rz(t,n)}else nR(e),o()}}}function nR(e){let t=nE.get(e);t&&(t.flags|=8,nE.delete(e))}let nO=!1,nM=()=>{nO||(console.error("Hydration completed but contains mismatches."),nO=!0)},nP=e=>{if(1===e.nodeType){if(e.namespaceURI.includes("svg")&&"foreignObject"!==e.tagName)return"svg";if(e.namespaceURI.includes("MathML"))return"mathml"}},nF=e=>8===e.nodeType;function nL(e){let{mt:t,p:n,o:{patchProp:r,createText:i,nextSibling:l,parentNode:s,remove:o,insert:a,createComment:c}}=e,u=(n,r,o,c,b,_=!1)=>{_=_||!!r.dynamicChildren;let S=nF(n)&&"["===n.data,x=()=>p(n,r,o,c,b,S),{type:k,ref:C,shapeFlag:T,patchFlag:w}=r,A=n.nodeType;r.el=n,-2===w&&(_=!1,r.dynamicChildren=null);let N=null;switch(k){case r7:3!==A?""===r.children?(a(r.el=i(""),s(n),n),N=n):N=x():(n.data!==r.children&&(nM(),n.data=r.children),N=l(n));break;case ie:y(n)?(N=l(n),m(r.el=n.content.firstChild,n,o)):N=8!==A||S?x():l(n);break;case it:if(S&&(A=(n=l(n)).nodeType),1===A||3===A){N=n;let e=!r.children.length;for(let t=0;t<r.staticCount;t++)e&&(r.children+=1===N.nodeType?N.outerHTML:N.data),t===r.staticCount-1&&(r.anchor=N),N=l(N);return S?l(N):N}x();break;case r9:N=S?f(n,r,o,c,b,_):x();break;default:if(1&T)N=1===A&&r.type.toLowerCase()===n.tagName.toLowerCase()||y(n)?d(n,r,o,c,b,_):x();else if(6&T){r.slotScopeIds=b;let e=s(n);if(N=S?g(n):nF(n)&&"teleport start"===n.data?g(n,n.data,"teleport end"):l(n),t(r,e,null,o,c,nP(e),_),nq(r)&&!r.component.subTree){let t;S?(t=iv(it)).anchor=N?N.previousSibling:e.lastChild:t=3===n.nodeType?i_(""):iv("div"),t.el=n,r.component.subTree=t}}else 64&T?N=8!==A?x():r.type.hydrate(n,r,o,c,b,_,e,h):128&T&&(N=r.type.hydrate(n,r,o,c,nP(s(n)),b,_,e,u))}return null!=C&&nI(C,null,c,r),N},d=(e,t,n,i,l,s)=>{s=s||!!t.dynamicChildren;let{type:a,dynamicProps:c,props:u,patchFlag:d,shapeFlag:f,dirs:p,transition:g}=t,b="input"===a||"option"===a,_=!!c;if(b||_||-1!==d){let a;p&&t4(t,null,n,"created");let T=!1;if(y(e)){T=rZ(null,g)&&n&&n.vnode.props&&n.vnode.props.appear;let r=e.content.firstChild;if(T){let e=r.getAttribute("class");e&&(r.$cls=e),g.beforeEnter(r)}m(r,e,n),t.el=e=r}if(16&f&&!(u&&(u.innerHTML||u.textContent))){let r=h(e.firstChild,t,e,n,i,l,s);for(r&&!nj(e,1)&&nM();r;){let e=r;r=r.nextSibling,o(e)}}else if(8&f){let n=t.children;`
`===n[0]&&("PRE"===e.tagName||"TEXTAREA"===e.tagName)&&(n=n.slice(1));let{textContent:r}=e;r!==n&&r!==n.replace(/\r\n|\r/g,`
`)&&(nj(e,0)||nM(),e.textContent=t.children)}if(u){if(b||_||!s||48&d){let t=e.tagName.includes("-"),i=e.namespaceURI.includes("svg")?"svg":e.namespaceURI.includes("MathML")?"mathml":void 0;for(let l in u)if(b&&(l.endsWith("value")||"indeterminate"===l)||k(l)&&!$(l)||"."===l[0]||t&&!$(l)||c&&c.includes(l)){var S,x,C;if(S=e,x=l,C=u[l],n$.has(x)&&S.getAttribute(x)===(null==C?null:`${C}`))continue;r(e,l,null,u[l],i,n)}}else if(u.onClick)r(e,"onClick",null,u.onClick,void 0,n);else if(4&d&&tf(u.style))for(let e in u.style)u.style[e]}(a=u&&u.onVnodeBeforeMount)&&iw(a,n,t),p&&t4(t,null,n,"beforeMount"),((a=u&&u.onVnodeMounted)||p||T)&&r8(()=>{a&&iw(a,n,t),T&&g.enter(e),p&&t4(t,null,n,"mounted")},i)}return e.nextSibling},h=(e,t,r,s,o,c,d)=>{d=d||!!t.dynamicChildren;let h=t.children,f=h.length,p=!1;for(let t=0;t<f;t++){let g=d?h[t]:h[t]=ix(h[t]),m=g.type===r7;e?(m&&!d&&t+1<f&&ix(h[t+1]).type===r7&&(a(i(e.data.slice(g.children.length)),r,l(e)),e.data=g.children),e=u(e,g,s,o,c,d)):m&&!g.children?a(g.el=i(""),r):(!p&&(p=!0,nj(r,1)||nM()),n(null,g,r,null,s,o,nP(r),c))}return e},f=(e,t,n,r,i,o)=>{let{slotScopeIds:u}=t;u&&(i=i?i.concat(u):u);let d=s(e),f=h(l(e),t,d,n,r,i,o);return f&&nF(f)&&"]"===f.data?l(t.anchor=f):(nM(),a(t.anchor=c("]"),d,f),f)},p=(e,t,r,i,a,c)=>{var u,d,h;if(u=e,d=t,nj(u.parentElement,1)||1===(h=u).nodeType&&nB(h.getAttribute(nD),1)||function({props:e}){let t=e&&e[nD];return"string"==typeof t&&nB(t,1)}(d)||nM(),t.el=null,c){let t=g(e);for(;;){let n=l(e);if(n&&n!==t)o(n);else break}}let f=l(e),p=s(e);return o(e),n(null,t,p,f,r,i,nP(p),a),r&&(r.vnode.el=t.el,rP(r,t.el)),f},g=(e,t="[",n="]")=>{let r=0;for(;e;)if((e=l(e))&&nF(e)&&(e.data===t&&r++,e.data===n))if(0===r)return l(e);else r--;return e},m=(e,t,n)=>{let r=t.parentNode;r&&r.replaceChild(e,t);let i=n;for(;i;)i.vnode.el===t&&(i.vnode.el=i.subTree.el=e),i=i.parent},y=e=>1===e.nodeType&&"TEMPLATE"===e.tagName;return[(e,t)=>{if(!t.hasChildNodes()){n(null,e,t),tY(),t._vnode=e;return}u(t.firstChild,e,null,null,null),tY(),t._vnode=e},u]}let n$=new Set(["src","srcset","href","poster"]),nD="data-allow-mismatch",nV={0:"text",1:"children",2:"class",3:"style",4:"attribute"};function nj(e,t){if(0===t||1===t)for(;e&&!e.hasAttribute(nD);)e=e.parentElement;return nB(e&&e.getAttribute(nD),t)}function nB(e,t){if(null==e)return!1;{if(""===e)return!0;let n=e.split(",");return!!(0===t&&n.includes("children"))||n.includes(nV[t])}}let nU=Q().requestIdleCallback||(e=>setTimeout(e,1)),nH=Q().cancelIdleCallback||(e=>clearTimeout(e)),nq=e=>!!e.type.__asyncLoader;function nW(e,t){let{ref:n,props:r,children:i,ce:l}=t.vnode,s=iv(e,r,i);return s.ref=n,s.ce=l,delete t.vnode.ce,s}let nK=e=>e.type.__isKeepAlive;function nz(e,t){let n;if(E(e))return e.some(e=>nz(e,t));if(R(e))return e.split(",").includes(t);return"[object RegExp]"===(n=e,F.call(n))&&(e.lastIndex=0,e.test(t))}function nJ(e,t){nX(e,"a",t)}function nG(e,t){nX(e,"da",t)}function nX(e,t,n=iE){let r=e.__wdc||(e.__wdc=()=>{let t=n;for(;t;){if(t.isDeactivated)return;t=t.parent}return e()});if(nY(t,r,n),n){let e=n.parent;for(;e&&e.parent;)nK(e.parent.vnode)&&function(e,t,n,r){let i=nY(t,e,r,!0);n8(()=>{w(r[t],i)},n)}(r,t,n,e),e=e.parent}}function nQ(e){e.shapeFlag&=-257,e.shapeFlag&=-513}function nZ(e){return 128&e.shapeFlag?e.ssContent:e}function nY(e,t,n=iE,r=!1){if(n){let i=n[e]||(n[e]=[]),l=t.__weh||(t.__weh=(...r)=>{eI();let i=iR(n),l=tV(t,n,e,r);return i(),eR(),l});return r?i.unshift(l):i.push(l),l}}let n0=e=>(t,n=iE)=>{iP&&"sp"!==e||nY(e,(...e)=>t(...e),n)},n1=n0("bm"),n2=n0("m"),n3=n0("bu"),n6=n0("u"),n4=n0("bum"),n8=n0("um"),n5=n0("sp"),n9=n0("rtg"),n7=n0("rtc");function re(e,t=iE){nY("ec",e,t)}let rt="components",rn=Symbol.for("v-ndc");function rr(e,t,n=!0,r=!1){let i=t1||iE;if(i){let n=i.type;if(e===rt){let e=iB(n,!1);if(e&&(e===t||e===B(t)||e===q(B(t))))return n}let l=ri(i[e]||n[e],t)||ri(i.appContext[e],t);return!l&&r?n:l}}function ri(e,t){return e&&(e[t]||e[B(t)]||e[q(B(t))])}let rl=e=>e?iM(e)?ij(e):rl(e.parent):null,rs=T(Object.create(null),{$:e=>e,$el:e=>e.vnode.el,$data:e=>e.data,$props:e=>e.props,$attrs:e=>e.attrs,$slots:e=>e.slots,$refs:e=>e.refs,$parent:e=>rl(e.parent),$root:e=>rl(e.root),$host:e=>e.ce,$emit:e=>e.emit,$options:e=>rp(e),$forceUpdate:e=>e.f||(e.f=()=>{tG(e.update)}),$nextTick:e=>e.n||(e.n=tJ.bind(e.proxy)),$watch:e=>nt.bind(e)}),ro=(e,t)=>e!==b&&!e.__isScriptSetup&&N(e,t),ra={get({_:e},t){let n,r;if("__v_skip"===t)return!0;let{ctx:i,setupState:l,data:s,props:o,accessCache:a,type:c,appContext:u}=e;if("$"!==t[0]){let e=a[t];if(void 0!==e)switch(e){case 1:return l[t];case 2:return s[t];case 4:return i[t];case 3:return o[t]}else{if(ro(l,t))return a[t]=1,l[t];if(s!==b&&N(s,t))return a[t]=2,s[t];if(N(o,t))return a[t]=3,o[t];if(i!==b&&N(i,t))return a[t]=4,i[t];rh&&(a[t]=0)}}let d=rs[t];return d?("$attrs"===t&&ej(e.attrs,"get",""),d(e)):(n=c.__cssModules)&&(n=n[t])?n:i!==b&&N(i,t)?(a[t]=4,i[t]):N(r=u.config.globalProperties,t)?r[t]:void 0},set({_:e},t,n){let{data:r,setupState:i,ctx:l}=e;return ro(i,t)?(i[t]=n,!0):r!==b&&N(r,t)?(r[t]=n,!0):!N(e.props,t)&&!("$"===t[0]&&t.slice(1)in e)&&(l[t]=n,!0)},has({_:{data:e,setupState:t,accessCache:n,ctx:r,appContext:i,props:l,type:s}},o){let a;return!!(n[o]||e!==b&&"$"!==o[0]&&N(e,o)||ro(t,o)||N(l,o)||N(r,o)||N(rs,o)||N(i.config.globalProperties,o)||(a=s.__cssModules)&&a[o])},defineProperty(e,t,n){return null!=n.get?e._.accessCache[t]=0:N(n,"value")&&this.set(e,t,n.value,null),Reflect.defineProperty(e,t,n)}},rc=T({},ra,{get(e,t){if(t!==Symbol.unscopables)return ra.get(e,t,e)},has:(e,t)=>"_"!==t[0]&&!Z(t)});function ru(e){let t=iI();return t.setupContext||(t.setupContext=iV(t))}function rd(e){return E(e)?e.reduce((e,t)=>(e[t]=null,e),{}):e}let rh=!0;function rf(e,t,n){tV(E(e)?e.map(e=>e.bind(t.proxy)):e.bind(t.proxy),t,n)}function rp(e){let t,n=e.type,{mixins:r,extends:i}=n,{mixins:l,optionsCache:s,config:{optionMergeStrategies:o}}=e.appContext,a=s.get(n);return a?t=a:l.length||r||i?(t={},l.length&&l.forEach(e=>rg(t,e,o,!0)),rg(t,n,o)):t=n,M(n)&&s.set(n,t),t}function rg(e,t,n,r=!1){let{mixins:i,extends:l}=t;for(let s in l&&rg(e,l,n,!0),i&&i.forEach(t=>rg(e,t,n,!0)),t)if(r&&"expose"===s);else{let r=rm[s]||n&&n[s];e[s]=r?r(e[s],t[s]):t[s]}return e}let rm={data:rv,props:rS,emits:rS,methods:r_,computed:r_,beforeCreate:rb,created:rb,beforeMount:rb,mounted:rb,beforeUpdate:rb,updated:rb,beforeDestroy:rb,beforeUnmount:rb,destroyed:rb,unmounted:rb,activated:rb,deactivated:rb,errorCaptured:rb,serverPrefetch:rb,components:r_,directives:r_,watch:function(e,t){if(!e)return t;if(!t)return e;let n=T(Object.create(null),e);for(let r in t)n[r]=rb(e[r],t[r]);return n},provide:rv,inject:function(e,t){return r_(ry(e),ry(t))}};function rv(e,t){return t?e?function(){return T(I(e)?e.call(this,this):e,I(t)?t.call(this,this):t)}:t:e}function ry(e){if(E(e)){let t={};for(let n=0;n<e.length;n++)t[e[n]]=e[n];return t}return e}function rb(e,t){return e?[...new Set([].concat(e,t))]:t}function r_(e,t){return e?T(Object.create(null),e,t):t}function rS(e,t){return e?E(e)&&E(t)?[...new Set([...e,...t])]:T(Object.create(null),rd(e),rd(null!=t?t:{})):t}function rx(){return{app:null,config:{isNativeTag:x,performance:!1,globalProperties:{},optionMergeStrategies:{},errorHandler:void 0,warnHandler:void 0,compilerOptions:{}},mixins:[],components:{},directives:{},provides:Object.create(null),optionsCache:new WeakMap,propsCache:new WeakMap,emitsCache:new WeakMap}}let rk=0,rC=null,rT=(e,t)=>"modelValue"===t||"model-value"===t?e.modelModifiers:e[`${t}Modifiers`]||e[`${B(t)}Modifiers`]||e[`${H(t)}Modifiers`];function rw(e,t,...n){let r;if(e.isUnmounted)return;let i=e.vnode.props||b,l=n,s=t.startsWith("update:"),o=s&&rT(i,t.slice(7));o&&(o.trim&&(l=n.map(e=>R(e)?e.trim():e)),o.number&&(l=l.map(G)));let a=i[r=W(t)]||i[r=W(B(t))];!a&&s&&(a=i[r=W(H(t))]),a&&tV(a,e,6,l);let c=i[r+"Once"];if(c){if(e.emitted){if(e.emitted[r])return}else e.emitted={};e.emitted[r]=!0,tV(c,e,6,l)}}let rA=new WeakMap;function rN(e,t){return!!e&&!!k(t)&&(N(e,(t="Once"===(t=t.slice(2))?t:t.replace(/Once$/,""))[0].toLowerCase()+t.slice(1))||N(e,H(t))||N(e,t))}function rE(e){let t,n,{type:r,vnode:i,proxy:l,withProxy:s,propsOptions:[o],slots:a,attrs:c,emit:u,render:d,renderCache:h,props:f,data:p,setupState:g,ctx:m,inheritAttrs:y}=e,b=t3(e);try{if(4&i.shapeFlag){let e=s||l;t=ix(d.call(e,e,h,f,g,p,m)),n=c}else t=ix(r.length>1?r(f,{attrs:c,slots:a,emit:u}):r(f,null)),n=r.props?c:rI(c)}catch(n){ir.length=0,tj(n,e,1),t=iv(ie)}let _=t;if(n&&!1!==y){let e=Object.keys(n),{shapeFlag:t}=_;e.length&&7&t&&(o&&e.some(C)&&(n=rR(n,o)),_=ib(_,n,!1,!0))}return i.dirs&&((_=ib(_,null,!1,!0)).dirs=_.dirs?_.dirs.concat(i.dirs):i.dirs),i.transition&&nC(_.type.__isTeleport&&nk(_)||_,i.transition),t=_,t3(b),t}let rI=e=>{let t;for(let n in e)("class"===n||"style"===n||k(n))&&((t||(t={}))[n]=e[n]);return t},rR=(e,t)=>{let n={};for(let r in e)C(r)&&r.slice(9)in t||(n[r]=e[r]);return n};function rO(e,t,n){let r=Object.keys(t);if(r.length!==Object.keys(e).length)return!0;for(let i=0;i<r.length;i++){let l=r[i];if(rM(t,e,l)&&!rN(n,l))return!0}return!1}function rM(e,t,n){let r=e[n],i=t[n];return"style"===n&&M(r)&&M(i)?!ed(r,i):r!==i}function rP({vnode:e,parent:t,suspense:n},r){for(;t;){let n=t.subTree;if(n.suspense&&n.suspense.activeBranch===e&&(n.suspense.vnode.el=n.el=r,e=n),n===e)(e=t.vnode).el=r,t=t.parent;else break}n&&n.activeBranch===e&&(n.vnode.el=r)}let rF={},rL=e=>Object.getPrototypeOf(e)===rF;function r$(e,t,n,r){let i,[l,s]=e.propsOptions,o=!1;if(t)for(let a in t){let c;if($(a))continue;let u=t[a];l&&N(l,c=B(a))?s&&s.includes(c)?(i||(i={}))[c]=u:n[c]=u:rN(e.emitsOptions,a)||a in r&&u===r[a]||(r[a]=u,o=!0)}if(s){let t=tv(n),r=i||b;for(let i=0;i<s.length;i++){let o=s[i];n[o]=rD(l,t,o,r[o],e,!N(r,o))}}return o}function rD(e,t,n,r,i,l){let s=e[n];if(null!=s){let e=N(s,"default");if(e&&void 0===r){let e=s.default;if(s.type!==Function&&!s.skipFactory&&I(e)){let{propsDefaults:l}=i;if(n in l)r=l[n];else{let s=iR(i);r=l[n]=e.call(null,t),s()}}else r=e;i.ce&&i.ce._setProp(n,r)}s[0]&&(l&&!e?r=!1:s[1]&&(""===r||r===H(n))&&(r=!0))}return r}let rV=new WeakMap;function rj(e){return!("$"===e[0]||$(e))}let rB=e=>"_"===e||"_ctx"===e||"$stable"===e,rU=e=>E(e)?e.map(ix):[ix(e)],rH=(e,t,n)=>{if(t._n)return t;let r=t6((...e)=>rU(t(...e)),n);return r._c=!1,r},rq=(e,t,n)=>{let r=e._ctx;for(let n in e){if(rB(n))continue;let i=e[n];if(I(i))t[n]=rH(n,i,r);else if(null!=i){let e=rU(i);t[n]=()=>e}}},rW=(e,t)=>{let n=rU(t);e.slots.default=()=>n},rK=(e,t,n)=>{for(let r in t)(n||!rB(r))&&(e[r]=t[r])},rz=r8;function rJ(e){return rG(e,nL)}function rG(e,t){var n;let r,i;Q().__VUE__=!0;let{insert:l,remove:s,patchProp:o,createElement:a,createText:c,createComment:d,setText:h,setElementText:f,parentNode:p,nextSibling:g,setScopeId:m=S,insertStaticContent:y}=e,x=(e,t,n,r=null,i=null,l=null,s,o=null,a=!!t.dynamicChildren)=>{if(e===t)return;e&&!ih(e,t)&&(r=es(e),et(e,i,l,!0),e=null),-2===t.patchFlag&&(a=!1,t.dynamicChildren=null);let{type:c,ref:u,shapeFlag:d}=t;switch(c){case r7:k(e,t,n,r);break;case ie:C(e,t,n,r);break;case it:null==e&&w(t,n,r,s);break;case r9:j(e,t,n,r,i,l,s,o,a);break;default:1&d?A(e,t,n,r,i,l,s,o,a):6&d?U(e,t,n,r,i,l,s,o,a):64&d?c.process(e,t,n,r,i,l,s,o,a,ec):128&d&&c.process(e,t,n,r,i,l,s,o,a,ec)}null!=u&&i?nI(u,e&&e.ref,l,t||e,!t):null==u&&e&&null!=e.ref&&nI(e.ref,null,l,e,!0)},k=(e,t,n,r)=>{if(null==e)l(t.el=c(t.children),n,r);else{let n=t.el=e.el;t.children!==e.children&&h(n,t.children)}},C=(e,t,n,r)=>{null==e?l(t.el=d(t.children||""),n,r):t.el=e.el},w=(e,t,n,r)=>{[e.el,e.anchor]=y(e.children,t,n,r,e.el,e.anchor)},A=(e,t,n,r,i,l,s,o,a)=>{if("svg"===t.type?s="svg":"math"===t.type&&(s="mathml"),null==e)R(t,n,r,i,l,s,o,a);else{let n=e.el&&e.el._isVueCE?e.el:null;try{n&&n._beginPatch(),L(e,t,i,l,s,o,a)}finally{n&&n._endPatch()}}},R=(e,t,n,r,i,s,c,u)=>{let d,h,{props:p,shapeFlag:g,transition:m,dirs:y}=e;if(d=e.el=a(e.type,s,p&&p.is,p),8&g?f(d,e.children):16&g&&F(e.children,d,null,r,i,rX(e,s),c,u),y&&t4(e,null,r,"created"),O(d,e,e.scopeId,c,r),p){for(let e in p)"value"===e||$(e)||o(d,e,null,p[e],s,r);"value"in p&&o(d,"value",null,p.value,s),(h=p.onVnodeBeforeMount)&&iw(h,r,e)}y&&t4(e,null,r,"beforeMount");let b=rZ(i,m);b&&m.beforeEnter(d),l(d,t,n),((h=p&&p.onVnodeMounted)||b||y)&&rz(()=>{h&&iw(h,r,e),b&&m.enter(d),y&&t4(e,null,r,"mounted")},i)},O=(e,t,n,r,i)=>{if(n&&m(e,n),r)for(let t=0;t<r.length;t++)m(e,r[t]);if(i){let n=i.subTree;if(t===n||r1(n.type)&&(n.ssContent===t||n.ssFallback===t)){let t=i.vnode;O(e,t,t.scopeId,t.slotScopeIds,i.parent)}}},F=(e,t,n,r,i,l,s,o,a=0)=>{for(let c=a;c<e.length;c++)x(null,e[c]=o?ik(e[c]):ix(e[c]),t,n,r,i,l,s,o)},L=(e,t,n,r,i,l,s)=>{let a,c=t.el=e.el,{patchFlag:u,dynamicChildren:d,dirs:h}=t;u|=16&e.patchFlag;let p=e.props||b,g=t.props||b;if(n&&rQ(n,!1),(a=g.onVnodeBeforeUpdate)&&iw(a,n,t,e),h&&t4(t,e,n,"beforeUpdate"),n&&rQ(n,!0),d&&(!e.dynamicChildren||e.dynamicChildren.length!==d.length)&&(u=0,s=!1,d=null),(p.innerHTML&&null==g.innerHTML||p.textContent&&null==g.textContent)&&f(c,""),d?D(e.dynamicChildren,d,c,n,r,rX(t,i),l):s||X(e,t,c,null,n,r,rX(t,i),l,!1),u>0){if(16&u)V(c,p,g,n,i);else if(2&u&&p.class!==g.class&&o(c,"class",null,g.class,i),4&u&&o(c,"style",p.style,g.style,i),8&u){let e=t.dynamicProps;for(let t=0;t<e.length;t++){let r=e[t],l=p[r],s=g[r];(s!==l||"value"===r)&&o(c,r,l,s,i,n)}}1&u&&e.children!==t.children&&f(c,t.children)}else s||null!=d||V(c,p,g,n,i);((a=g.onVnodeUpdated)||h)&&rz(()=>{a&&iw(a,n,t,e),h&&t4(t,e,n,"updated")},r)},D=(e,t,n,r,i,l,s)=>{for(let o=0;o<t.length;o++){let a=e[o],c=t[o],u=a.el&&(a.type===r9||!ih(a,c)||198&a.shapeFlag)?p(a.el):n;x(a,c,u,null,r,i,l,s,!0)}},V=(e,t,n,r,i)=>{if(t!==n){if(t!==b)for(let l in t)$(l)||l in n||o(e,l,t[l],null,i,r);for(let l in n){if($(l))continue;let s=n[l],a=t[l];s!==a&&"value"!==l&&o(e,l,a,s,i,r)}"value"in n&&o(e,"value",t.value,n.value,i)}},j=(e,t,n,r,i,s,o,a,u)=>{let d=t.el=e?e.el:c(""),h=t.anchor=e?e.anchor:c(""),{patchFlag:f,dynamicChildren:p,slotScopeIds:g}=t;g&&(a=a?a.concat(g):g),null==e?(l(d,n,r),l(h,n,r),F(t.children||[],n,h,i,s,o,a,u)):f>0&&64&f&&p&&e.dynamicChildren&&e.dynamicChildren.length===p.length?(D(e.dynamicChildren,p,n,i,s,o,a),(null!=t.key||i&&t===i.subTree)&&rY(e,t,!0)):X(e,t,n,h,i,s,o,a,u)},U=(e,t,n,r,i,l,s,o,a)=>{t.slotScopeIds=o,null==e?512&t.shapeFlag?i.ctx.activate(t,n,r,s,a):q(t,n,r,i,l,s,a):W(e,t,a)},q=(e,t,n,r,i,l,s)=>{var o,a,c;let d,h,f,p=(o=e,a=r,c=i,d=o.type,h=(a?a.appContext:o.appContext)||iA,(f={uid:iN++,vnode:o,type:d,parent:a,appContext:h,root:null,next:null,subTree:null,effect:null,update:null,job:null,scope:new ev(!0),render:null,proxy:null,exposed:null,exposeProxy:null,withProxy:null,provides:a?a.provides:Object.create(h.provides),ids:a?a.ids:["",0,0],accessCache:null,renderCache:[],components:null,directives:null,propsOptions:function e(t,n,r=!1){let i=r?rV:n.propsCache,l=i.get(t);if(l)return l;let s=t.props,o={},a=[],c=!1;if(!I(t)){let i=t=>{c=!0;let[r,i]=e(t,n,!0);T(o,r),i&&a.push(...i)};!r&&n.mixins.length&&n.mixins.forEach(i),t.extends&&i(t.extends),t.mixins&&t.mixins.forEach(i)}if(!s&&!c)return M(t)&&i.set(t,_),_;if(E(s))for(let e=0;e<s.length;e++){let t=B(s[e]);rj(t)&&(o[t]=b)}else if(s)for(let e in s){let t=B(e);if(rj(t)){let n=s[e],r=o[t]=E(n)||I(n)?{type:n}:T({},n),i=r.type,l=!1,c=!0;if(E(i))for(let e=0;e<i.length;++e){let t=i[e],n=I(t)&&t.name;if("Boolean"===n){l=!0;break}"String"===n&&(c=!1)}else l=I(i)&&"Boolean"===i.name;r[0]=l,r[1]=c,(l||N(r,"default"))&&a.push(t)}}let u=[o,a];return M(t)&&i.set(t,u),u}(d,h),emitsOptions:function e(t,n,r=!1){let i=r?rA:n.emitsCache,l=i.get(t);if(void 0!==l)return l;let s=t.emits,o={},a=!1;if(!I(t)){let i=t=>{let r=e(t,n,!0);r&&(a=!0,T(o,r))};!r&&n.mixins.length&&n.mixins.forEach(i),t.extends&&i(t.extends),t.mixins&&t.mixins.forEach(i)}return s||a?(E(s)?s.forEach(e=>o[e]=null):T(o,s),M(t)&&i.set(t,o),o):(M(t)&&i.set(t,null),null)}(d,h),emit:null,emitted:null,propsDefaults:b,inheritAttrs:d.inheritAttrs,ctx:b,data:b,props:b,attrs:b,slots:b,refs:b,setupState:b,setupContext:null,suspense:c,suspenseId:c?c.pendingId:0,asyncDep:null,asyncResolved:!1,isMounted:!1,isUnmounted:!1,isDeactivated:!1,bc:null,c:null,bm:null,m:null,bu:null,u:null,um:null,bum:null,da:null,a:null,rtg:null,rtc:null,ec:null,sp:null}).ctx={_:f},f.root=a?a.root:f,f.emit=rw.bind(null,f),o.ce&&o.ce(f),e.component=f);if(nK(e)&&(p.ctx.renderer=ec),function(e,t=!1,n=!1){t&&u(t);let{props:r,children:i}=e.vnode,l=iM(e);!function(e,t,n,r=!1){let i={},l=Object.create(rF);for(let n in e.propsDefaults=Object.create(null),r$(e,t,i,l),e.propsOptions[0])n in i||(i[n]=void 0);n?e.props=r?i:tu(i):e.type.props?e.props=i:e.props=l,e.attrs=l}(e,r,l,t);var s=n||t;let o=e.slots=Object.create(rF);if(32&e.vnode.shapeFlag){let e=i._;e?(rK(o,i,s),s&&J(o,"_",e,!0)):rq(i,o)}else i&&rW(e,i);l&&function(e,t){let n=e.type;e.accessCache=Object.create(null),e.proxy=new Proxy(e.ctx,ra);let{setup:r}=n;if(r){eI();let n=e.setupContext=r.length>1?iV(e):null,i=iR(e),l=tD(r,e,0,[e.props,n]),s=P(l);if(eR(),i(),(s||e.sp)&&!nq(e)&&nA(e),s){if(l.then(iO,iO),t)return l.then(n=>{u(!0);try{iF(e,n,t)}finally{u(!1)}}).catch(t=>{tj(t,e,0)});e.asyncDep=l}else iF(e,l,t)}else i$(e,t)}(e,t),t&&u(!1)}(p,!1,s),p.asyncDep){if(i&&i.registerDep(p,K,s),!e.el){let r=p.subTree=iv(ie);C(null,r,t,n),e.placeholder=r.el}}else K(p,e,t,n,i,l,s)},W=(e,t,n)=>{let r=t.component=e.component;if(function(e,t,n){let{props:r,children:i,component:l}=e,{props:s,children:o,patchFlag:a}=t,c=l.emitsOptions;if(t.dirs||t.transition)return!0;if(!n||!(a>=0))return(!!i||!!o)&&(!o||!o.$stable)||r!==s&&(r?!s||rO(r,s,c):!!s);if(1024&a)return!0;if(16&a)return r?rO(r,s,c):!!s;if(8&a){let e=t.dynamicProps;for(let t=0;t<e.length;t++){let n=e[t];if(rM(s,r,n)&&!rN(c,n))return!0}}return!1}(e,t,n))if(r.asyncDep&&!r.asyncResolved)return void G(r,t,n);else r.next=t,r.update();else t.el=e.el,r.vnode=t},K=(e,t,n,r,l,s,o)=>{e.scope.on();let a=e.effect=new eb(()=>{if(e.isMounted){let t,{next:n,bu:r,u:i,parent:a,vnode:u}=e;{let t=function e(t){let n=t.subTree.component;if(n)if(n.asyncDep&&!n.asyncResolved)return n;else return e(n)}(e);if(t){n&&(n.el=u.el,G(e,n,o)),t.asyncDep.then(()=>{rz(()=>{e.isUnmounted||c()},l)});return}}let d=n;rQ(e,!1),n?(n.el=u.el,G(e,n,o)):n=u,r&&z(r),(t=n.props&&n.props.onVnodeBeforeUpdate)&&iw(t,a,n,u),rQ(e,!0);let h=rE(e),f=e.subTree;e.subTree=h,x(f,h,p(f.el),es(f),e,l,s),n.el=h.el,null===d&&rP(e,h.el),i&&rz(i,l),(t=n.props&&n.props.onVnodeUpdated)&&rz(()=>iw(t,a,n,u),l)}else{let o,{el:a,props:c}=t,{bm:u,m:d,parent:h,root:f,type:p}=e,g=nq(t);if(rQ(e,!1),u&&z(u),!g&&(o=c&&c.onVnodeBeforeMount)&&iw(o,h,t),rQ(e,!0),a&&i){let t=()=>{e.subTree=rE(e),i(a,e.subTree,e,l,null)};g&&p.__asyncHydrate?p.__asyncHydrate(a,e,t):t()}else{f.ce&&f.ce._hasShadowRoot()&&f.ce._injectChildStyle(p,e.parent?e.parent.type:void 0);let i=e.subTree=rE(e);x(null,i,n,r,e,l,s),t.el=i.el}if(d&&rz(d,l),!g&&(o=c&&c.onVnodeMounted)){let e=t;rz(()=>iw(o,h,e),l)}(256&t.shapeFlag||h&&nq(h.vnode)&&256&h.vnode.shapeFlag)&&e.a&&rz(e.a,l),e.isMounted=!0,t=n=r=null}});e.scope.off();let c=e.update=a.run.bind(a),u=e.job=a.runIfDirty.bind(a);u.i=e,u.id=e.uid,a.scheduler=()=>tG(u),rQ(e,!0),c()},G=(e,t,n)=>{t.component=e;let r=e.vnode.props;e.vnode=t,e.next=null,function(e,t,n,r){let{props:i,attrs:l,vnode:{patchFlag:s}}=e,o=tv(i),[a]=e.propsOptions,c=!1;if((r||s>0)&&!(16&s)){if(8&s){let n=e.vnode.dynamicProps;for(let r=0;r<n.length;r++){let s=n[r];if(rN(e.emitsOptions,s))continue;let u=t[s];if(a)if(N(l,s))u!==l[s]&&(l[s]=u,c=!0);else{let t=B(s);i[t]=rD(a,o,t,u,e,!1)}else u!==l[s]&&(l[s]=u,c=!0)}}}else{let r;for(let s in r$(e,t,i,l)&&(c=!0),o)t&&(N(t,s)||(r=H(s))!==s&&N(t,r))||(a?n&&(void 0!==n[s]||void 0!==n[r])&&(i[s]=rD(a,o,s,void 0,e,!0)):delete i[s]);if(l!==o)for(let e in l)t&&N(t,e)||(delete l[e],c=!0)}c&&eB(e.attrs,"set","")}(e,t.props,r,n),((e,t,n)=>{let{vnode:r,slots:i}=e,l=!0,s=b;if(32&r.shapeFlag){let e=t._;e?n&&1===e?l=!1:rK(i,t,n):(l=!t.$stable,rq(t,i)),s=t}else t&&(rW(e,t),s={default:1});if(l)for(let e in i)rB(e)||null!=s[e]||delete i[e]})(e,t.children,n),eI(),tZ(e),eR()},X=(e,t,n,r,i,l,s,o,a=!1)=>{let c=e&&e.children,u=e?e.shapeFlag:0,d=t.children,{patchFlag:h,shapeFlag:p}=t;if(h>0){if(128&h)return void Y(c,d,n,r,i,l,s,o,a);else if(256&h)return void Z(c,d,n,r,i,l,s,o,a)}8&p?(16&u&&el(c,i,l),d!==c&&f(n,d)):16&u?16&p?Y(c,d,n,r,i,l,s,o,a):el(c,i,l,!0):(8&u&&f(n,""),16&p&&F(d,n,r,i,l,s,o,a))},Z=(e,t,n,r,i,l,s,o,a)=>{let c;e=e||_,t=t||_;let u=e.length,d=t.length,h=Math.min(u,d);for(c=0;c<h;c++){let r=t[c]=a?ik(t[c]):ix(t[c]);x(e[c],r,n,null,i,l,s,o,a)}u>d?el(e,i,l,!0,!1,h):F(t,n,r,i,l,s,o,a,h)},Y=(e,t,n,r,i,l,s,o,a)=>{let c=0,u=t.length,d=e.length-1,h=u-1;for(;c<=d&&c<=h;){let r=e[c],u=t[c]=a?ik(t[c]):ix(t[c]);if(ih(r,u))x(r,u,n,null,i,l,s,o,a);else break;c++}for(;c<=d&&c<=h;){let r=e[d],c=t[h]=a?ik(t[h]):ix(t[h]);if(ih(r,c))x(r,c,n,null,i,l,s,o,a);else break;d--,h--}if(c>d){if(c<=h){let e=h+1,d=e<u?t[e].el:r;for(;c<=h;)x(null,t[c]=a?ik(t[c]):ix(t[c]),n,d,i,l,s,o,a),c++}}else if(c>h)for(;c<=d;)et(e[c],i,l,!0),c++;else{let f,p=c,g=c,m=new Map;for(c=g;c<=h;c++){let e=t[c]=a?ik(t[c]):ix(t[c]);null!=e.key&&m.set(e.key,c)}let y=0,b=h-g+1,S=!1,k=0,C=Array(b);for(c=0;c<b;c++)C[c]=0;for(c=p;c<=d;c++){let r,u=e[c];if(y>=b){et(u,i,l,!0);continue}if(null!=u.key)r=m.get(u.key);else for(f=g;f<=h;f++)if(0===C[f-g]&&ih(u,t[f])){r=f;break}void 0===r?et(u,i,l,!0):(C[r-g]=c+1,r>=k?k=r:S=!0,x(u,t[r],n,null,i,l,s,o,a),y++)}let T=S?function(e){let t,n,r,i,l,s=e.slice(),o=[0],a=e.length;for(t=0;t<a;t++){let a=e[t];if(0!==a){if(e[n=o[o.length-1]]<a){s[t]=n,o.push(t);continue}for(r=0,i=o.length-1;r<i;)e[o[l=r+i>>1]]<a?r=l+1:i=l;a<e[o[r]]&&(r>0&&(s[t]=o[r-1]),o[r]=t)}}for(r=o.length,i=o[r-1];r-- >0;)o[r]=i,i=s[i];return o}(C):_;for(f=T.length-1,c=b-1;c>=0;c--){let e=g+c,d=t[e],h=t[e+1],p=e+1<u?h.el||function e(t){if(t.placeholder)return t.placeholder;let n=t.component;return n?e(n.subTree):null}(h):r;0===C[c]?x(null,d,n,p,i,l,s,o,a):S&&(f<0||c!==T[f]?ee(d,n,p,2):f--)}}},ee=(e,t,n,r,i=null)=>{let{el:o,type:a,transition:c,children:u,shapeFlag:d}=e;if(6&d)return void ee(e.component.subTree,t,n,r);if(128&d)return void e.suspense.move(t,n,r);if(64&d)return void a.move(e,t,n,ec);if(a===r9){l(o,t,n);for(let e=0;e<u.length;e++)ee(u[e],t,n,r);l(e.anchor,t,n);return}if(a===it)return void(({el:e,anchor:t},n,r)=>{let i;for(;e&&e!==t;)i=g(e),l(e,n,r),e=i;l(t,n,r)})(e,t,n);if(2!==r&&1&d&&c)if(0===r)c.persisted&&!o[nh]?l(o,t,n):(c.beforeEnter(o),l(o,t,n),rz(()=>c.enter(o),i));else{let{leave:r,delayLeave:i,afterLeave:a}=c,u=()=>{e.ctx.isUnmounted?s(o):l(o,t,n)},d=()=>{let e=o._isLeaving||!!o[nh];o._isLeaving&&o[nh](!0),c.persisted&&!e?u():r(o,()=>{u(),a&&a()})};i?i(o,u,d):d()}else l(o,t,n)},et=(e,t,n,r=!1,i=!1)=>{let l,{type:s,props:o,ref:a,children:c,dynamicChildren:u,shapeFlag:d,patchFlag:h,dirs:f,cacheIndex:p,memo:g}=e;if(-2===h&&(i=!1),null!=a&&(eI(),nI(a,null,n,e,!0),eR()),null!=p&&(t.renderCache[p]=void 0),256&d)return void t.ctx.deactivate(e);let m=1&d&&f,y=!nq(e);if(y&&(l=o&&o.onVnodeBeforeUnmount)&&iw(l,t,e),6&d)ei(e.component,n,r);else{if(128&d)return void e.suspense.unmount(n,r);m&&t4(e,null,t,"beforeUnmount"),64&d?e.type.remove(e,t,n,ec,r):u&&!u.hasOnce&&(s!==r9||h>0&&64&h)?el(u,t,n,!1,!0):(s===r9&&384&h||!i&&16&d)&&el(c,t,n),r&&en(e)}let b=null!=g&&null==p;(y&&(l=o&&o.onVnodeUnmounted)||m||b)&&rz(()=>{l&&iw(l,t,e),m&&t4(e,null,t,"unmounted"),b&&(e.el=null)},n)},en=e=>{let{type:t,el:n,anchor:r,transition:i}=e;if(t===r9)return void er(n,r);if(t===it)return void(({el:e,anchor:t})=>{let n;for(;e&&e!==t;)n=g(e),s(e),e=n;s(t)})(e);let l=()=>{s(n),i&&!i.persisted&&i.afterLeave&&i.afterLeave()};if(1&e.shapeFlag&&i&&!i.persisted){let{leave:t,delayLeave:r}=i,s=()=>t(n,l);r?r(e.el,l,s):s()}else l()},er=(e,t)=>{let n;for(;e!==t;)n=g(e),s(e),e=n;s(t)},ei=(e,t,n)=>{let{bum:r,scope:i,job:l,subTree:s,um:o,m:a,a:c}=e;r0(a),r0(c),r&&z(r),i.stop(),l&&(l.flags|=8,et(s,e,t,n)),o&&rz(o,t),rz(()=>{e.isUnmounted=!0},t)},el=(e,t,n,r=!1,i=!1,l=0)=>{for(let s=l;s<e.length;s++)et(e[s],t,n,r,i)},es=e=>{if(6&e.shapeFlag)return es(e.component.subTree);if(128&e.shapeFlag)return e.suspense.next();let t=g(e.anchor||e.el),n=t&&t[ni];return n?g(n):t},eo=!1,ea=(e,t,n)=>{let r;null==e?t._vnode&&(et(t._vnode,null,null,!0),r=t._vnode.component):x(t._vnode||null,e,t,null,null,null,n),t._vnode=e,eo||(eo=!0,tZ(r),tY(),eo=!1)},ec={p:x,um:et,m:ee,r:en,mt:q,mc:F,pc:X,pbc:D,n:es,o:e};return t&&([r,i]=t(ec)),{render:ea,hydrate:r,createApp:(n=r,function(e,t=null){I(e)||(e=T({},e)),null==t||M(t)||(t=null);let r=rx(),i=new WeakSet,l=[],s=!1,o=r.app={_uid:rk++,_component:e,_props:t,_container:null,_context:r,_instance:null,version:iW,get config(){return r.config},set config(v){},use:(e,...t)=>(i.has(e)||(e&&I(e.install)?(i.add(e),e.install(o,...t)):I(e)&&(i.add(e),e(o,...t))),o),mixin:e=>(r.mixins.includes(e)||r.mixins.push(e),o),component:(e,t)=>t?(r.components[e]=t,o):r.components[e],directive:(e,t)=>t?(r.directives[e]=t,o):r.directives[e],mount(i,l,a){if(!s){let c=o._ceVNode||iv(e,t);return c.appContext=r,!0===a?a="svg":!1===a&&(a=void 0),l&&n?n(c,i):ea(c,i,a),s=!0,o._container=i,i.__vue_app__=o,ij(c.component)}},onUnmount(e){l.push(e)},unmount(){s&&(tV(l,o._instance,16),ea(null,o._container),delete o._container.__vue_app__)},provide:(e,t)=>(r.provides[e]=t,o),runWithContext(e){let t=rC;rC=o;try{return e()}finally{rC=t}}};return o})}}function rX({type:e,props:t},n){return"svg"===n&&"foreignObject"===e||"mathml"===n&&"annotation-xml"===e&&t&&t.encoding&&t.encoding.includes("html")?void 0:n}function rQ({effect:e,job:t},n){n?(e.flags|=32,t.flags|=4):(e.flags&=-33,t.flags&=-5)}function rZ(e,t){return(!e||e&&!e.pendingBranch)&&t&&!t.persisted}function rY(e,t,n=!1){let r=e.children,i=t.children;if(E(r)&&E(i))for(let e=0;e<r.length;e++){let t=r[e],l=i[e];1&l.shapeFlag&&!l.dynamicChildren&&((l.patchFlag<=0||32===l.patchFlag)&&((l=i[e]=ik(i[e])).el=t.el),n||-2===l.patchFlag||rY(t,l)),l.type===r7&&(-1===l.patchFlag&&(l=i[e]=ik(l)),l.el=t.el),l.type!==ie||l.el||(l.el=t.el)}}function r0(e){if(e)for(let t=0;t<e.length;t++)e[t].flags|=8}let r1=e=>e.__isSuspense,r2=0;function r3(e,t){let n=e.props&&e.props[t];I(n)&&n()}function r6(e,t,n,r,i,l,s,o,a,c,u=!1){var d;let h,f,{p:p,m:g,um:m,n:y,o:{parentNode:b,remove:_}}=c,S=null!=(h=(d=e).props&&d.props.suspensible)&&!1!==h;S&&t&&t.pendingBranch&&(f=t.pendingId,t.deps++);let x=e.props?X(e.props.timeout):void 0,k=l,C={vnode:e,parent:t,parentComponent:n,namespace:s,container:r,hiddenContainer:i,deps:0,pendingId:r2++,timeout:"number"==typeof x?x:-1,activeBranch:null,isFallbackMountPending:!1,pendingBranch:null,isInFallback:!u,isHydrating:u,isUnmounted:!1,effects:[],resolve(e=!1,n=!1){let{vnode:r,activeBranch:i,pendingBranch:s,pendingId:o,effects:a,parentComponent:c,container:u,isInFallback:d}=C,h=!1;if(C.isHydrating)C.isHydrating=!1;else if(!e){h=i&&s.transition&&"out-in"===s.transition.mode;let e=!1;h&&(i.transition.afterLeave=()=>{o===C.pendingId&&(g(s,u,l!==k||e?l:y(i),0),tQ(a),d&&r.ssFallback&&(r.ssFallback.el=null))}),i&&!C.isFallbackMountPending&&(b(i.el)===u&&(l=y(i),e=!0),m(i,c,C,!0),!h&&d&&r.ssFallback&&rz(()=>r.ssFallback.el=null,C)),h||g(s,u,l,0)}C.isFallbackMountPending=!1,r5(C,s),C.pendingBranch=null,C.isInFallback=!1;let p=C.parent,_=!1;for(;p;){if(p.pendingBranch){for(let e=0;e<a.length;e++)p.effects.push(a[e]);_=!0;break}p=p.parent}_||h||tQ(a),C.effects=[],S&&t&&t.pendingBranch&&f===t.pendingId&&(t.deps--,0!==t.deps||n||t.resolve()),r3(r,"onResolve")},fallback(e){if(!C.pendingBranch)return;let{vnode:t,activeBranch:n,parentComponent:r,container:i,namespace:l}=C;r3(t,"onFallback");let s=y(n),c=()=>{if(C.isFallbackMountPending=!1,!C.isInFallback)return;let e=C.vnode.ssFallback;p(null,e,i,s,r,null,l,o,a),r5(C,e)},u=e.transition&&"out-in"===e.transition.mode;u&&(C.isFallbackMountPending=!0,n.transition.afterLeave=c),C.isInFallback=!0,m(n,r,null,!0),u||c()},move(e,t,n){C.activeBranch&&g(C.activeBranch,e,t,n),C.container=e},next:()=>C.activeBranch&&y(C.activeBranch),registerDep(e,t,n){let r=!!C.pendingBranch;r&&C.deps++;let i=e.vnode.el;e.asyncDep.catch(t=>{tj(t,e,0)}).then(l=>{if(e.isUnmounted||C.isUnmounted||C.pendingId!==e.suspenseId)return;iO(),e.asyncResolved=!0;let{vnode:o}=e;iF(e,l,!1),i&&(o.el=i);let a=!i&&e.subTree.el;t(e,o,b(i||e.subTree.el),i?null:y(e.subTree),C,s,n),a&&(o.placeholder=null,_(a)),rP(e,o.el),r&&0==--C.deps&&C.resolve()})},unmount(e,t){C.isUnmounted=!0,C.activeBranch&&m(C.activeBranch,n,e,t),C.pendingBranch&&m(C.pendingBranch,n,e,t)}};return C}function r4(e){let t;if(I(e)){let n=io&&e._c;n&&(e._d=!1,il()),e=e(),n&&(e._d=!0,t=ii,is())}return E(e)&&(e=function(e){let t;for(let n=0;n<e.length;n++){let r=e[n];if(!id(r))return;if(r.type!==ie||"v-if"===r.children)if(t)return;else t=r}return t}(e)),e=ix(e),t&&!e.dynamicChildren&&(e.dynamicChildren=t.filter(t=>t!==e)),e}function r8(e,t){t&&t.pendingBranch?E(e)?t.effects.push(...e):t.effects.push(e):tQ(e)}function r5(e,t){e.activeBranch=t;let{vnode:n,parentComponent:r}=e,i=t.el;for(;!i&&t.component;)i=(t=t.component.subTree).el;n.el=i,r&&r.subTree===n&&(r.vnode.el=i,rP(r,i))}let r9=Symbol.for("v-fgt"),r7=Symbol.for("v-txt"),ie=Symbol.for("v-cmt"),it=Symbol.for("v-stc"),ir=[],ii=null;function il(e=!1){ir.push(ii=e?null:[])}function is(){ir.pop(),ii=ir[ir.length-1]||null}let io=1;function ia(e,t=!1){io+=e,e<0&&ii&&t&&(ii.hasOnce=!0)}function ic(e){return e.dynamicChildren=io>0?ii||_:null,is(),io>0&&ii&&ii.push(e),e}function iu(e,t,n,r,i){return ic(iv(e,t,n,r,i,!0))}function id(e){return!!e&&!0===e.__v_isVNode}function ih(e,t){return e.type===t.type&&e.key===t.key}let ip=({key:e})=>null!=e?e:null,ig=({ref:e,ref_key:t,ref_for:n})=>("number"==typeof e&&(e=""+e),null!=e?R(e)||tS(e)||I(e)?{i:t1,r:e,k:t,f:!!n}:e:null);function im(e,t=null,n=null,r=0,i=null,l=+(e!==r9),s=!1,o=!1){let a={__v_isVNode:!0,__v_skip:!0,type:e,props:t,key:t&&ip(t),ref:t&&ig(t),scopeId:t2,slotScopeIds:null,children:n,component:null,suspense:null,ssContent:null,ssFallback:null,dirs:null,transition:null,el:null,anchor:null,target:null,targetStart:null,targetAnchor:null,staticCount:0,shapeFlag:l,patchFlag:r,dynamicProps:i,dynamicChildren:null,appContext:null,ctx:t1};return o?(iC(a,n),128&l&&e.normalize(a)):n&&(a.shapeFlag|=R(n)?8:16),io>0&&!s&&ii&&(a.patchFlag>0||6&l)&&32!==a.patchFlag&&ii.push(a),a}let iv=function(e,t=null,n=null,r=0,i=null,l=!1){var s;if(e&&e!==rn||(e=ie),id(e)){let r=ib(e,t,!0);return n&&iC(r,n),io>0&&!l&&ii&&(6&r.shapeFlag?ii[ii.indexOf(e)]=r:ii.push(r)),r.patchFlag=-2,r}if(I(s=e)&&"__vccOpts"in s&&(e=e.__vccOpts),t){let{class:e,style:n}=t=iy(t);e&&!R(e)&&(t.class=ei(e)),M(n)&&(tm(n)&&!E(n)&&(n=T({},n)),t.style=Y(n))}let o=R(e)?1:r1(e)?128:e.__isTeleport?64:M(e)?4:2*!!I(e);return im(e,t,n,r,i,o,l,!0)};function iy(e){return e?tm(e)||rL(e)?T({},e):e:null}function ib(e,t,n=!1,r=!1){let{props:i,ref:l,patchFlag:s,children:o,transition:a}=e,c=t?iT(i||{},t):i,u={__v_isVNode:!0,__v_skip:!0,type:e.type,props:c,key:c&&ip(c),ref:t&&t.ref?n&&l?E(l)?l.concat(ig(t)):[l,ig(t)]:ig(t):l,scopeId:e.scopeId,slotScopeIds:e.slotScopeIds,children:o,target:e.target,targetStart:e.targetStart,targetAnchor:e.targetAnchor,staticCount:e.staticCount,shapeFlag:e.shapeFlag,patchFlag:t&&e.type!==r9?-1===s?16:16|s:s,dynamicProps:e.dynamicProps,dynamicChildren:e.dynamicChildren,appContext:e.appContext,dirs:e.dirs,transition:a,component:e.component,suspense:e.suspense,ssContent:e.ssContent&&ib(e.ssContent),ssFallback:e.ssFallback&&ib(e.ssFallback),placeholder:e.placeholder,el:e.el,anchor:e.anchor,ctx:e.ctx,ce:e.ce};return a&&r&&nC(u,a.clone(u)),u}function i_(e=" ",t=0){return iv(r7,null,e,t)}function iS(e="",t=!1){return t?(il(),iu(ie,null,e)):iv(ie,null,e)}function ix(e){return null==e||"boolean"==typeof e?iv(ie):E(e)?iv(r9,null,e.slice()):id(e)?ik(e):iv(r7,null,String(e))}function ik(e){return null===e.el&&-1!==e.patchFlag||e.memo?e:ib(e)}function iC(e,t){let n=0,{shapeFlag:r}=e;if(null==t)t=null;else if(E(t))n=16;else if("object"==typeof t)if(65&r){let n=t.default;n&&(n._c&&(n._d=!1),iC(e,n()),n._c&&(n._d=!0));return}else{n=32;let r=t._;r||rL(t)?3===r&&t1&&(1===t1.slots._?t._=1:(t._=2,e.patchFlag|=1024)):t._ctx=t1}else if(I(t)){if(65&r)return void iC(e,{default:t});t={default:t,_ctx:t1},n=32}else t=String(t),64&r?(n=16,t=[i_(t)]):n=8;e.children=t,e.shapeFlag|=n}function iT(...e){let t={};for(let n=0;n<e.length;n++){let r=e[n];for(let e in r)if("class"===e)t.class!==r.class&&(t.class=ei([t.class,r.class]));else if("style"===e)t.style=Y([t.style,r.style]);else if(k(e)){let n=t[e],i=r[e];i&&n!==i&&!(E(n)&&n.includes(i))?t[e]=n?[].concat(n,i):i:null!=i||null!=n||C(e)||(t[e]=i)}else""!==e&&(t[e]=r[e])}return t}function iw(e,t,n,r=null){tV(e,t,7,[n,r])}let iA=rx(),iN=0,iE=null,iI=()=>iE||t1;c=e=>{iE=e},u=e=>{iP=e};let iR=e=>{let t=iE;return c(e),e.scope.on(),()=>{e.scope.off(),c(t)}},iO=()=>{iE&&iE.scope.off(),c(null)};function iM(e){return 4&e.vnode.shapeFlag}let iP=!1;function iF(e,t,n){I(t)?e.render=t:M(t)&&(e.setupState=tN(t)),i$(e,n)}function iL(e){d=e,h=e=>{e.render._rc&&(e.withProxy=new Proxy(e.ctx,rc))}}function i$(e,t,n){let r=e.type;if(!e.render){if(!t&&d&&!r.render){let t=r.template||rp(e).template;if(t){let{isCustomElement:n,compilerOptions:i}=e.appContext.config,{delimiters:l,compilerOptions:s}=r,o=T(T({isCustomElement:n,delimiters:l},i),s);r.render=d(t,o)}}e.render=r.render||S,h&&h(e)}{let t=iR(e);eI();try{!function(e){let t=rp(e),n=e.proxy,r=e.ctx;rh=!1,t.beforeCreate&&rf(t.beforeCreate,e,"bc");let{data:i,computed:l,methods:s,watch:o,provide:a,inject:c,created:u,beforeMount:d,mounted:h,beforeUpdate:f,updated:p,activated:g,deactivated:m,beforeUnmount:y,unmounted:b,render:_,renderTracked:x,renderTriggered:k,errorCaptured:C,serverPrefetch:T,expose:w,inheritAttrs:A,components:N,directives:O}=t;if(c&&function(e,t){for(let n in E(e)&&(e=ry(e)),e){let r,i=e[n];tS(r=M(i)?"default"in i?t5(i.from||n,i.default,!0):t5(i.from||n):t5(i))?Object.defineProperty(t,n,{enumerable:!0,configurable:!0,get:()=>r.value,set:e=>r.value=e}):t[n]=r}}(c,r),s)for(let e in s){let t=s[e];I(t)&&(r[e]=t.bind(n))}if(i){let t=i.call(n,n);M(t)&&(e.data=tc(t))}if(rh=!0,l)for(let e in l){let t=l[e],i=I(t)?t.bind(n,n):I(t.get)?t.get.bind(n,n):S,s=iU({get:i,set:!I(t)&&I(t.set)?t.set.bind(n):S});Object.defineProperty(r,e,{enumerable:!0,configurable:!0,get:()=>s.value,set:e=>s.value=e})}if(o)for(let e in o)!function e(t,n,r,i){let l=i.includes(".")?nn(r,i):()=>r[i];if(R(t)){let e=n[t];I(e)&&ne(l,e,void 0)}else if(I(t))ne(l,t.bind(r),void 0);else if(M(t))if(E(t))t.forEach(t=>e(t,n,r,i));else{let e=I(t.handler)?t.handler.bind(r):n[t.handler];I(e)&&ne(l,e,t)}}(o[e],r,n,e);if(a){let e=I(a)?a.call(n):a;Reflect.ownKeys(e).forEach(t=>{t8(t,e[t])})}function P(e,t){E(t)?t.forEach(t=>e(t.bind(n))):t&&e(t.bind(n))}if(u&&rf(u,e,"c"),P(n1,d),P(n2,h),P(n3,f),P(n6,p),P(nJ,g),P(nG,m),P(re,C),P(n7,x),P(n9,k),P(n4,y),P(n8,b),P(n5,T),E(w))if(w.length){let t=e.exposed||(e.exposed={});w.forEach(e=>{Object.defineProperty(t,e,{get:()=>n[e],set:t=>n[e]=t,enumerable:!0})})}else e.exposed||(e.exposed={});_&&e.render===S&&(e.render=_),null!=A&&(e.inheritAttrs=A),N&&(e.components=N),O&&(e.directives=O)}(e)}finally{eR(),t()}}}let iD={get:(e,t)=>(ej(e,"get",""),e[t])};function iV(e){return{attrs:new Proxy(e.attrs,iD),slots:e.slots,emit:e.emit,expose:t=>{e.exposed=t||{}}}}function ij(e){return e.exposed?e.exposeProxy||(e.exposeProxy=new Proxy(tN(ty(e.exposed)),{get:(t,n)=>n in t?t[n]:n in rs?rs[n](e):void 0,has:(e,t)=>t in e||t in rs})):e.proxy}function iB(e,t=!0){return I(e)?e.displayName||e.name:e.name||t&&e.__name}let iU=(e,t)=>(function(e,t=!1){let n,r;return I(e)?n=e:(n=e.get,r=e.set),new tM(n,r,t)})(e,iP);function iH(e,t,n){try{ia(-1);let r=arguments.length;if(2!==r)return r>3?n=Array.prototype.slice.call(arguments,2):3===r&&id(n)&&(n=[n]),iv(e,t,n);if(!M(t)||E(t))return iv(e,null,t);if(id(t))return iv(e,null,[t]);return iv(e,t)}finally{ia(1)}}function iq(e,t){let n=e.memo;if(n.length!=t.length)return!1;for(let e=0;e<n.length;e++)if(K(n[e],t[e]))return!1;return io>0&&ii&&ii.push(e),!0}let iW="3.5.42",iK="u">typeof window&&window.trustedTypes;if(iK)try{m=iK.createPolicy("vue",{createHTML:e=>e})}catch(e){}let iz=m?e=>m.createHTML(e):e=>e,iJ="u">typeof document?document:null,iG=iJ&&iJ.createElement("template"),iX={insert:(e,t,n)=>{t.insertBefore(e,n||null)},remove:e=>{let t=e.parentNode;t&&t.removeChild(e)},createElement:(e,t,n,r)=>{let i="svg"===t?iJ.createElementNS("http://www.w3.org/2000/svg",e):"mathml"===t?iJ.createElementNS("http://www.w3.org/1998/Math/MathML",e):n?iJ.createElement(e,{is:n}):iJ.createElement(e);return"select"===e&&r&&null!=r.multiple&&i.setAttribute("multiple",r.multiple),i},createText:e=>iJ.createTextNode(e),createComment:e=>iJ.createComment(e),setText:(e,t)=>{e.nodeValue=t},setElementText:(e,t)=>{e.textContent=t},parentNode:e=>e.parentNode,nextSibling:e=>e.nextSibling,querySelector:e=>iJ.querySelector(e),setScopeId(e,t){e.setAttribute(t,"")},insertStaticContent(e,t,n,r,i,l){let s=n?n.previousSibling:t.lastChild;if(i&&(i===l||i.nextSibling))for(;t.insertBefore(i.cloneNode(!0),n),i!==l&&(i=i.nextSibling););else{iG.innerHTML=iz("svg"===r?`<svg>${e}</svg>`:"mathml"===r?`<math>${e}</math>`:e);let i=iG.content;if("svg"===r||"mathml"===r){let e=i.firstChild;for(;e.firstChild;)i.appendChild(e.firstChild);i.removeChild(e)}t.insertBefore(i,n)}return[s?s.nextSibling:t.firstChild,n?n.previousSibling:t.lastChild]}},iQ="transition",iZ="animation",iY=Symbol("_vtc"),i0={name:String,type:String,css:{type:Boolean,default:!0},duration:[String,Number,Object],enterFromClass:String,enterActiveClass:String,enterToClass:String,appearFromClass:String,appearActiveClass:String,appearToClass:String,leaveFromClass:String,leaveActiveClass:String,leaveToClass:String},i1=T({},nm,i0),i2=((t=(e,{slots:t})=>iH(nb,i4(e),t)).displayName="Transition",t.props=i1,t),i3=(e,t=[])=>{E(e)?e.forEach(e=>e(...t)):e&&e(...t)},i6=e=>!!e&&(E(e)?e.some(e=>e.length>1):e.length>1);function i4(e){let t={};for(let n in e)n in i0||(t[n]=e[n]);if(!1===e.css)return t;let{name:n="v",type:r,duration:i,enterFromClass:l=`${n}-enter-from`,enterActiveClass:s=`${n}-enter-active`,enterToClass:o=`${n}-enter-to`,appearFromClass:a=l,appearActiveClass:c=s,appearToClass:u=o,leaveFromClass:d=`${n}-leave-from`,leaveActiveClass:h=`${n}-leave-active`,leaveToClass:f=`${n}-leave-to`}=e,p=function(e){if(null==e)return null;{if(M(e))return[function(e){return X(e)}(e.enter),function(e){return X(e)}(e.leave)];let t=function(e){return X(e)}(e);return[t,t]}}(i),g=p&&p[0],m=p&&p[1],{onBeforeEnter:y,onEnter:b,onEnterCancelled:_,onLeave:S,onLeaveCancelled:x,onBeforeAppear:k=y,onAppear:C=b,onAppearCancelled:w=_}=t,A=(e,t,n,r)=>{e._enterCancelled=r,i5(e,t?u:o),i5(e,t?c:s),n&&n()},N=(e,t)=>{e._isLeaving=!1,i5(e,d),i5(e,f),i5(e,h),t&&t()},E=e=>(t,n)=>{let i=e?C:b,s=()=>A(t,e,n);i3(i,[t,s]),i9(()=>{i5(t,e?a:l),i8(t,e?u:o),i6(i)||le(t,r,g,s)})};return T(t,{onBeforeEnter(e){i3(y,[e]),i8(e,l),i8(e,s)},onBeforeAppear(e){i3(k,[e]),i8(e,a),i8(e,c)},onEnter:E(!1),onAppear:E(!0),onLeave(e,t){e._isLeaving=!0;let n=()=>N(e,t);i8(e,d),e._enterCancelled?(i8(e,h),li(e)):(li(e),i8(e,h)),i9(()=>{e._isLeaving&&(i5(e,d),i8(e,f),i6(S)||le(e,r,m,n))}),i3(S,[e,n])},onEnterCancelled(e){A(e,!1,void 0,!0),i3(_,[e])},onAppearCancelled(e){A(e,!0,void 0,!0),i3(w,[e])},onLeaveCancelled(e){N(e),i3(x,[e])}})}function i8(e,t){t.split(/\s+/).forEach(t=>t&&e.classList.add(t)),(e[iY]||(e[iY]=new Set)).add(t)}function i5(e,t){t.split(/\s+/).forEach(t=>t&&e.classList.remove(t));let n=e[iY];n&&(n.delete(t),n.size||(e[iY]=void 0))}function i9(e){requestAnimationFrame(()=>{requestAnimationFrame(e)})}let i7=0;function le(e,t,n,r){let i=e._endId=++i7,l=()=>{i===e._endId&&r()};if(null!=n)return setTimeout(l,n);let{type:s,timeout:o,propCount:a}=lt(e,t);if(!s)return r();let c=s+"end",u=0,d=()=>{e.removeEventListener(c,h),l()},h=t=>{t.target===e&&++u>=a&&d()};setTimeout(()=>{u<a&&d()},o+1),e.addEventListener(c,h)}function lt(e,t){let n=window.getComputedStyle(e),r=e=>(n[e]||"").split(", "),i=r(`${iQ}Delay`),l=r(`${iQ}Duration`),s=ln(i,l),o=r(`${iZ}Delay`),a=r(`${iZ}Duration`),c=ln(o,a),u=null,d=0,h=0;t===iQ?s>0&&(u=iQ,d=s,h=l.length):t===iZ?c>0&&(u=iZ,d=c,h=a.length):h=(u=(d=Math.max(s,c))>0?s>c?iQ:iZ:null)?u===iQ?l.length:a.length:0;let f=u===iQ&&/\b(?:transform|all)(?:,|$)/.test(r(`${iQ}Property`).toString());return{type:u,timeout:d,propCount:h,hasTransform:f}}function ln(e,t){for(;e.length<t.length;)e=e.concat(e);return Math.max(...t.map((t,n)=>lr(t)+lr(e[n])))}function lr(e){return"auto"===e?0:1e3*Number(e.slice(0,-1).replace(",","."))}function li(e){return(e?e.ownerDocument:document).body.offsetHeight}let ll=Symbol("_vod"),ls=Symbol("_vsh");function lo(e,t){e.style.display=t?e[ll]:"none",e[ls]=!t}let la=Symbol("");function lc(e,t){if(1===e.nodeType){let r=e.style,i="";for(let e in t){var n;let l=null==(n=t[e])?"initial":"string"==typeof n?""===n?" ":n:String(n);r.setProperty(`--${e}`,l),i+=`--${e}: ${l};`}r[la]=i}}let lu=/(?:^|;)\s*display\s*:/,ld=/\s*!important$/;function lh(e,t,n){if(E(n))n.forEach(n=>lh(e,t,n));else if(null==n&&(n=""),t.startsWith("--"))ld.test(n)?e.setProperty(t,n.replace(ld,""),"important"):e.setProperty(t,n);else{let r=function(e,t){let n=lp[t];if(n)return n;let r=B(t);if("filter"!==r&&r in e)return lp[t]=r;r=q(r);for(let n=0;n<lf.length;n++){let i=lf[n]+r;if(i in e)return lp[t]=i}return t}(e,t);ld.test(n)?e.setProperty(H(r),n.replace(ld,""),"important"):e[r]=n}}let lf=["Webkit","Moz","ms"],lp={},lg="http://www.w3.org/1999/xlink";function lm(e,t,n,r,i,l=ec(t)){if(r&&t.startsWith("xlink:"))null==n?e.removeAttributeNS(lg,t.slice(6,t.length)):e.setAttributeNS(lg,t,n);else null==n||l&&!(n||""===n)?e.removeAttribute(t):e.setAttribute(t,l?"":O(n)?String(n):n)}function lv(e,t,n,r,i){if("innerHTML"===t||"textContent"===t){null!=n&&(e[t]="innerHTML"===t?iz(n):n);return}let l=e.tagName;if("value"===t&&"PROGRESS"!==l&&!l.includes("-")){let r="OPTION"===l?e.getAttribute("value")||"":e.value,i=null==n?"checkbox"===e.type?"on":"":String(n);r===i&&"_value"in e||(e.value=i),null==n&&e.removeAttribute(t),e._value=n;return}let s=!1;if(""===n||null==n){let r=typeof e[t];if("boolean"===r){var o;n=!!(o=n)||""===o}else null==n&&"string"===r?(n="",s=!0):"number"===r&&(n=0,s=!0)}try{e[t]=n}catch(e){}s&&e.removeAttribute(i||t)}function ly(e,t,n,r){e.addEventListener(t,n,r)}let lb=Symbol("_vei"),l_=/(Once|Passive|Capture)$/,lS=/^on:?(?:Once|Passive|Capture)$/,lx=0,lk=Promise.resolve(),lC=e=>111===e.charCodeAt(0)&&110===e.charCodeAt(1)&&e.charCodeAt(2)>96&&123>e.charCodeAt(2),lT=(e,t,n,r,i,l)=>{let s="svg"===i;if("class"===t){var o;let t;o=r,(t=e[iY])&&(o=(o?[o,...t]:[...t]).join(" ")),null==o?e.removeAttribute("class"):s?e.setAttribute("class",o):e.className=o}else"style"===t?function(e,t,n){let r=e.style,i=R(n),l=!1;if(n&&!i){if(t)if(R(t))for(let e of t.split(";")){let t=e.slice(0,e.indexOf(":")).trim();null==n[t]&&lh(r,t,"")}else for(let e in t)null==n[e]&&lh(r,e,"");for(let i in n){var s,o,a,c;"display"===i&&(l=!0);let u=n[i];null!=u?(s=e,o=i,a=!R(t)&&t?t[i]:void 0,c=u,"TEXTAREA"===s.tagName&&("width"===o||"height"===o)&&R(c)&&a===c||lh(r,i,u)):lh(r,i,"")}}else if(i){if(t!==n){let e=r[la];e&&(n+=";"+e),r.cssText=n,l=lu.test(n)}}else t&&e.removeAttribute("style");ll in e&&(e[ll]=l?r.display:"",e[ls]&&(r.display="none"))}(e,n,r):k(t)?C(t)||function(e,t,n,r=null){let i=e[lb]||(e[lb]={}),l=i[t];if(n&&l)l.value=n;else{let[a,c]=function(e){let t,n;for(;(n=e.match(l_))&&!lS.test(e);)t||(t={}),e=e.slice(0,e.length-n[1].length),t[n[1].toLowerCase()]=!0;return[":"===e[2]?e.slice(3):H(e.slice(2)),t]}(t);if(n){var s,o;let l;ly(e,a,i[t]=(s=n,o=r,(l=e=>{if(e._vts){if(e._vts<=l.attached)return}else e._vts=Date.now();let t=l.value;if(E(t)){let n=e.stopImmediatePropagation;e.stopImmediatePropagation=()=>{n.call(e),e._stopped=!0};let r=t.slice(),i=[e];for(let t=0;t<r.length&&!e._stopped;t++){let e=r[t];e&&tV(e,o,5,i)}}else tV(t,o,5,[e])}).value=s,l.attached=lx||(lk.then(()=>lx=0),lx=Date.now()),l),c)}else l&&(e.removeEventListener(a,l,c),i[t]=void 0)}}(e,t,r,l):("."===t[0]?(t=t.slice(1),0):"^"===t[0]?(t=t.slice(1),1):!function(e,t,n,r){if(r)return!!("innerHTML"===t||"textContent"===t||t in e&&lC(t)&&I(n));if("spellcheck"===t||"draggable"===t||"translate"===t||"autocorrect"===t||"sandbox"===t&&"IFRAME"===e.tagName||"form"===t||"list"===t&&"INPUT"===e.tagName||"type"===t&&"TEXTAREA"===e.tagName)return!1;if("width"===t||"height"===t){let t=e.tagName;if("IMG"===t||"VIDEO"===t||"CANVAS"===t||"SOURCE"===t)return!1}return!(lC(t)&&R(n))&&t in e}(e,t,r,s))?e._isVueCE&&(function(e,t){let n=e._def.props;if(!n)return!1;let r=B(t);return Array.isArray(n)?n.some(e=>B(e)===r):Object.keys(n).some(e=>B(e)===r)}(e,t)||e._def.__asyncLoader&&(/[A-Z]/.test(t)||!R(r)))?lv(e,B(t),r,l,t):("true-value"===t?e._trueValue=r:"false-value"===t&&(e._falseValue=r),lm(e,t,r,s)):(lv(e,t,r),e.tagName.includes("-")||"value"!==t&&"checked"!==t&&"selected"!==t||lm(e,t,r,s,l,"value"!==t))},lw={};function lA(e,t,n){let r,i=nw(e,t);"[object Object]"===(r=i,F.call(r))&&(i=T({},i,t));class l extends lE{constructor(e){super(i,e,n)}}return l.def=i,l}let lN="u">typeof HTMLElement?HTMLElement:class{};class lE extends lN{constructor(e,t={},n=l9){super(),this._def=e,this._props=t,this._createApp=n,this._isVueCE=!0,this._instance=null,this._app=null,this._nonce=this._def.nonce,this._connected=!1,this._resolved=!1,this._patching=!1,this._dirty=!1,this._numberProps=null,this._styleChildren=new WeakSet,this._styleAnchors=new WeakMap,this._ob=null,this.shadowRoot&&n!==l9?this._root=this.shadowRoot:!1!==e.shadowRoot?(this.attachShadow(T({},e.shadowRootOptions,{mode:"open"})),this._root=this.shadowRoot):this._root=this}connectedCallback(){if(!this.isConnected)return;this.shadowRoot||this._resolved||this._parseSlots(),this._connected=!0;let e=this;for(;e=e&&(e.assignedSlot||e.parentNode||e.host);)if(e instanceof lE){this._parent=e;break}this._instance||(this._resolved?this._mount(this._def):e&&e._pendingResolve?this._pendingResolve=e._pendingResolve.then(()=>{if(this._pendingResolve=void 0,this.isConnected)return this._resolveDef()}):this._resolveDef())}_setParent(e=this._parent){e&&(this._instance.parent=e._instance,this._inheritParentContext(e))}_inheritParentContext(e=this._parent){e&&this._app&&Object.setPrototypeOf(this._app._context.provides,e._instance.provides)}disconnectedCallback(){this._connected=!1,tJ(()=>{!this._connected&&(this._ob&&(this._ob.disconnect(),this._ob=null),this._app&&this._app.unmount(),this._instance&&(this._instance.ce=void 0),this._app=this._instance=null,this._teleportTargets&&(this._teleportTargets.clear(),this._teleportTargets=void 0))})}_processMutations(e){for(let t of e)this._setAttr(t.attributeName)}_resolveDef(){if(this._pendingResolve)return this._pendingResolve;for(let e=0;e<this.attributes.length;e++)this._setAttr(this.attributes[e].name);this._ob=new MutationObserver(this._processMutations.bind(this)),this._ob.observe(this,{attributes:!0});let e=(e,t=!1)=>{let n;this._resolved=!0,this._pendingResolve=void 0;let{props:r,styles:i}=e;if(r&&!E(r))for(let e in r){let t=r[e];(t===Number||t&&t.type===Number)&&(e in this._props&&(this._props[e]=X(this._props[e])),(n||(n=Object.create(null)))[B(e)]=!0)}this._numberProps=n,this._resolveProps(e),this.shadowRoot&&this._applyStyles(i),this._mount(e)},t=this._def.__asyncLoader;if(t)return this._pendingResolve=t().then(t=>{t.configureApp=this._def.configureApp,e(this._def=t,!0)}),this._pendingResolve;e(this._def)}_mount(e){this._app=this._createApp(e),this._inheritParentContext(),e.configureApp&&e.configureApp(this._app),this._app._ceVNode=this._createVNode(),this._app.mount(this._root);let t=this._instance&&this._instance.exposed;if(t)for(let e in t)N(this,e)||Object.defineProperty(this,e,{get:()=>tw(t[e])})}_resolveProps(e){let{props:t}=e,n=E(t)?t:Object.keys(t||{});for(let e of Object.keys(this))"_"!==e[0]&&n.includes(e)&&this._setProp(e,this[e]);for(let e of n.map(B))Object.defineProperty(this,e,{get(){return this._getProp(e)},set(t){this._setProp(e,t,!0,!this._patching)}})}_setAttr(e){if(e.startsWith("data-v-"))return;let t=this.hasAttribute(e),n=t?this.getAttribute(e):lw,r=B(e);t&&this._numberProps&&this._numberProps[r]&&(n=X(n)),this._setProp(r,n,!1,!0)}_getProp(e){return this._props[e]}_setProp(e,t,n=!0,r=!1){if(t!==this._props[e]&&(this._dirty=!0,t===lw?delete this._props[e]:(this._props[e]=t,"key"===e&&this._app&&(this._app._ceVNode.key=t)),r&&this._instance&&this._update(),n)){let n=this._ob;n&&(this._processMutations(n.takeRecords()),n.disconnect()),!0===t?this.setAttribute(H(e),""):"string"==typeof t||"number"==typeof t?this.setAttribute(H(e),t+""):t||this.removeAttribute(H(e)),n&&n.observe(this,{attributes:!0})}}_update(){let e=this._createVNode();this._app&&(e.appContext=this._app._context),l5(e,this._root)}_createVNode(){let e={};this.shadowRoot||(e.onVnodeMounted=e.onVnodeUpdated=this._renderSlots.bind(this));let t=iv(this._def,T(e,this._props));return this._instance||(t.ce=e=>{this._instance=e,e.ce=this,e.isCE=!0;let t=(e,t)=>{let n;this.dispatchEvent(new CustomEvent(e,"[object Object]"===(n=t[0],F.call(n))?T({detail:t},t[0]):{detail:t}))};e.emit=(e,...n)=>{t(e,n),H(e)!==e&&t(H(e),n)},this._setParent()}),t}_applyStyles(e,t,n){if(!e)return;if(t){if(t===this._def||this._styleChildren.has(t))return;this._styleChildren.add(t)}let r=this._nonce,i=this.shadowRoot,l=n?this._getStyleAnchor(n)||this._getStyleAnchor(this._def):this._getRootStyleInsertionAnchor(i),s=null;for(let o=e.length-1;o>=0;o--){let a=document.createElement("style");r&&a.setAttribute("nonce",r),a.textContent=e[o],i.insertBefore(a,s||l),s=a,0===o&&(n||this._styleAnchors.set(this._def,a),t&&this._styleAnchors.set(t,a))}}_getStyleAnchor(e){if(!e)return null;let t=this._styleAnchors.get(e);return t&&t.parentNode===this.shadowRoot?t:(t&&this._styleAnchors.delete(e),null)}_getRootStyleInsertionAnchor(e){for(let t=0;t<e.childNodes.length;t++){let n=e.childNodes[t];if(!(n instanceof HTMLStyleElement))return n}return null}_parseSlots(){let e,t=this._slots={};for(;e=this.firstChild;){let n=1===e.nodeType&&e.getAttribute("slot")||"default";(t[n]||(t[n]=[])).push(e),this.removeChild(e)}}_renderSlots(){let e=this._getSlots(),t=this._instance.type.__scopeId;for(let n=0;n<e.length;n++){let r=e[n],i=r.getAttribute("name")||"default",l=this._slots[i],s=r.parentNode;if(l)for(let e of l){if(t&&1===e.nodeType){let n,r=t+"-s",i=document.createTreeWalker(e,1);for(e.setAttribute(r,"");n=i.nextNode();)n.setAttribute(r,"")}s.insertBefore(e,r)}else for(;r.firstChild;)s.insertBefore(r.firstChild,r);s.removeChild(r)}}_getSlots(){let e=[this];this._teleportTargets&&e.push(...this._teleportTargets);let t=new Set;for(let n of e){let e=n.querySelectorAll("slot");for(let n=0;n<e.length;n++)t.add(e[n])}return Array.from(t)}_injectChildStyle(e,t){this._applyStyles(e.styles,e,t)}_beginPatch(){this._patching=!0,this._dirty=!1}_endPatch(){this._patching=!1,this._dirty&&this._instance&&this._update()}_hasShadowRoot(){return!1!==this._def.shadowRoot}_removeChildStyle(e){}}function lI(e){let t=iI(),n=t&&t.ce;return n||null}let lR=new WeakMap,lO=new WeakMap,lM=Symbol("_moveCb"),lP=Symbol("_enterCb"),lF=(n={name:"TransitionGroup",props:T({},i1,{tag:String,moveClass:String}),setup(e,{slots:t}){let n,r,i=iI(),l=np();return n6(()=>{if(!n.length)return;let t=e.moveClass||`${e.name||"v"}-move`;if(!function(e,t,n){let r=e.cloneNode(),i=e[iY];i&&i.forEach(e=>{e.split(/\s+/).forEach(e=>e&&r.classList.remove(e))}),n.split(/\s+/).forEach(e=>e&&r.classList.add(e)),r.style.display="none";let l=1===t.nodeType?t:t.parentNode;l.appendChild(r);let{hasTransform:s}=lt(r);return l.removeChild(r),s}(n[0].el,i.vnode.el,t)){n=[];return}n.forEach(lL),n.forEach(l$);let r=n.filter(lD);li(i.vnode.el),r.forEach(e=>{let n=e.el,r=n.style;i8(n,t),r.transform=r.webkitTransform=r.transitionDuration="";let i=n[lM]=e=>{(!e||e.target===n)&&(!e||e.propertyName.endsWith("transform"))&&(n.removeEventListener("transitionend",i),n[lM]=null,i5(n,t))};n.addEventListener("transitionend",i)}),n=[]}),()=>{let s=tv(e),o=i4(s),a=s.tag||r9;if(n=[],r)for(let e=0;e<r.length;e++){let t=r[e];t.el&&t.el instanceof Element&&!t.el[ls]&&(n.push(t),nC(t,nS(t,o,l,i)),lR.set(t,lV(t.el)))}r=t.default?nT(t.default()):[];for(let e=0;e<r.length;e++){let t=r[e];null!=t.key&&nC(t,nS(t,o,l,i))}return iv(a,null,r)}}},delete n.props.mode,n);function lL(e){let t=e.el;t[lM]&&t[lM](),t[lP]&&t[lP]()}function l$(e){lO.set(e,lV(e.el))}function lD(e){let t=lR.get(e),n=lO.get(e),r=t.left-n.left,i=t.top-n.top;if(r||i){let t=e.el,n=t.style,l=t.getBoundingClientRect(),s=1,o=1;return t.offsetWidth&&(s=l.width/t.offsetWidth),t.offsetHeight&&(o=l.height/t.offsetHeight),Number.isFinite(s)&&0!==s||(s=1),Number.isFinite(o)&&0!==o||(o=1),.01>Math.abs(s-1)&&(s=1),.01>Math.abs(o-1)&&(o=1),n.transform=n.webkitTransform=`translate(${r/s}px,${i/o}px)`,n.transitionDuration="0s",e}}function lV(e){let t=e.getBoundingClientRect();return{left:t.left,top:t.top}}let lj=e=>{let t=e.props["onUpdate:modelValue"]||!1;return E(t)?e=>z(t,e):t};function lB(e){e.target.composing=!0}function lU(e){let t=e.target;t.composing&&(t.composing=!1,t.dispatchEvent(new Event("input")))}let lH=Symbol("_assign"),lq=Symbol("_initialValue");function lW(e,t,n){return t&&(e=e.trim()),n&&(e=G(e)),e}let lK={created(e,{modifiers:{lazy:t,trim:n,number:r}},i){e.parentNode&&("text"===e.type?e[lq]=e.defaultValue.replace(/[\r\n]/g,""):"textarea"===e.type&&(e[lq]=e.defaultValue.replace(/\r\n?/g,`
`))),e[lH]=lj(i);let l=r||i.props&&"number"===i.props.type;ly(e,t?"change":"input",t=>{t.target.composing||e[lH](lW(e.value,n,l))}),(n||l)&&ly(e,"change",()=>{e.value=lW(e.value,n,l)}),t||(ly(e,"compositionstart",lB),ly(e,"compositionend",lU),ly(e,"change",lU))},mounted(e,{value:t,modifiers:{trim:n,number:r}}){let i=e[lq];delete e[lq],void 0!==i&&("text"===e.type||"textarea"===e.type)&&e.value!==i?e[lH](lW(e.value,n,r)):e.value=null==t?"":t},beforeUpdate(e,{value:t,oldValue:n,modifiers:{lazy:r,trim:i,number:l}},s){if(e[lH]=lj(s),e.composing)return;let o=(l||"number"===e.type)&&!/^0\d/.test(e.value)?G(e.value):e.value,a=null==t?"":t;if(o===a)return;let c=e.getRootNode();(c instanceof Document||c instanceof ShadowRoot)&&c.activeElement===e&&"range"!==e.type&&(r&&t===n||i&&e.value.trim()===a)||(e.value=a)}},lz={deep:!0,created(e,t,n){e[lH]=lj(n),ly(e,"change",()=>{let t=e._modelValue,n=lZ(e),r=e.checked,i=e[lH];if(E(t)){let e=eh(t,n),l=-1!==e;if(r&&!l)i(t.concat(n));else if(!r&&l){let n=[...t];n.splice(e,1),i(n)}}else{let l;if("[object Set]"===(l=t,F.call(l))){let e=new Set(t);r?e.add(n):e.delete(n),i(e)}else i(lY(e,r))}})},mounted:lJ,beforeUpdate(e,t,n){e[lH]=lj(n),lJ(e,t,n)}};function lJ(e,{value:t,oldValue:n},r){let i;if(e._modelValue=t,E(t))i=eh(t,r.props.value)>-1;else{let l;if("[object Set]"===(l=t,F.call(l)))i=t.has(r.props.value);else{if(t===n)return;i=ed(t,lY(e,!0))}}e.checked!==i&&(e.checked=i)}let lG={created(e,{value:t},n){e.checked=ed(t,n.props.value),e[lH]=lj(n),ly(e,"change",()=>{e[lH](lZ(e))})},beforeUpdate(e,{value:t,oldValue:n},r){e[lH]=lj(r),t!==n&&(e.checked=ed(t,r.props.value))}},lX={deep:!0,created(e,{value:t,modifiers:{number:n}},r){e._modelValue=t,ly(e,"change",()=>{let t,r=Array.prototype.filter.call(e.options,e=>e.selected).map(e=>n?G(lZ(e)):lZ(e)),i=e.multiple,l=i?"[object Set]"===(t=e._modelValue,F.call(t))?new Set(r):r:r[0],s=e._pendingValue=[i,i?E(l)?r.slice():r:l];try{e[lH](l)}finally{tJ(()=>{e._pendingValue===s&&(e._pendingValue=void 0)})}}),e[lH]=lj(r)},mounted(e,{value:t}){lQ(e,t)},beforeUpdate(e,{value:t},n){e._modelValue=t,e[lH]=lj(n)},updated(e,{value:t}){let n=e._pendingValue;e._pendingValue=void 0,n&&n[0]===e.multiple&&function(e,t,n){let r;if(!n||E(e))return ed(e,t);if("[object Set]"===(r=e,F.call(r))){if(e.size!==t.length)return!1;for(let n of t)if(!e.has(n))return!1;return!0}return!1}(t,n[1],n[0])||lQ(e,t)}};function lQ(e,t){let n,r=e.multiple,i=E(t);if(!r||i||"[object Set]"===(n=t,F.call(n))){for(let n=0,l=e.options.length;n<l;n++){let l=e.options[n],s=lZ(l);if(r)if(i){let e=typeof s;"string"===e||"number"===e?l.selected=t.some(e=>String(e)===String(s)):l.selected=eh(t,s)>-1}else l.selected=t.has(s);else if(ed(lZ(l),t)){e.selectedIndex!==n&&(e.selectedIndex=n);return}}r||-1===e.selectedIndex||(e.selectedIndex=-1)}}function lZ(e){return"_value"in e?e._value:e.value}function lY(e,t){let n=t?"_trueValue":"_falseValue";return n in e?e[n]:t}function l0(e,t,n,r,i){let l=function(e,t){switch(e){case"SELECT":return lX;case"TEXTAREA":return lK;default:switch(t){case"checkbox":return lz;case"radio":return lG;default:return lK}}}(e.tagName,n.props&&n.props.type)[i];l&&l(e,t,n,r)}let l1=["ctrl","shift","alt","meta"],l2={stop:e=>e.stopPropagation(),prevent:e=>e.preventDefault(),self:e=>e.target!==e.currentTarget,ctrl:e=>!e.ctrlKey,shift:e=>!e.shiftKey,alt:e=>!e.altKey,meta:e=>!e.metaKey,left:e=>"button"in e&&0!==e.button,middle:e=>"button"in e&&1!==e.button,right:e=>"button"in e&&2!==e.button,exact:(e,t)=>l1.some(n=>e[`${n}Key`]&&!t.includes(n))},l3={esc:"escape",space:" ",up:"arrow-up",left:"arrow-left",right:"arrow-right",down:"arrow-down",delete:"backspace"},l6=T({patchProp:lT},iX),l4=!1;function l8(){return f=l4?f:rJ(l6),l4=!0,f}let l5=(...e)=>{(f||(f=rG(l6))).render(...e)},l9=(...e)=>{let t=(f||(f=rG(l6))).createApp(...e),{mount:n}=t;return t.mount=e=>{let r=st(e);if(!r)return;let i=t._component;I(i)||i.render||i.template||(i.template=r.innerHTML),1===r.nodeType&&(r.textContent="");let l=n(r,!1,se(r));return r instanceof Element&&(r.removeAttribute("v-cloak"),r.setAttribute("data-v-app","")),l},t},l7=(...e)=>{let t=l8().createApp(...e),{mount:n}=t;return t.mount=e=>{let t=st(e);if(t)return n(t,!0,se(t))},t};function se(e){return e instanceof SVGElement?"svg":"function"==typeof MathMLElement&&e instanceof MathMLElement?"mathml":void 0}function st(e){return R(e)?document.querySelector(e):e}let sn=Symbol(""),sr=Symbol(""),si=Symbol(""),sl=Symbol(""),ss=Symbol(""),so=Symbol(""),sa=Symbol(""),sc=Symbol(""),su=Symbol(""),sd=Symbol(""),sh=Symbol(""),sf=Symbol(""),sp=Symbol(""),sg=Symbol(""),sm=Symbol(""),sv=Symbol(""),sy=Symbol(""),sb=Symbol(""),s_=Symbol(""),sS=Symbol(""),sx=Symbol(""),sk=Symbol(""),sC=Symbol(""),sT=Symbol(""),sw=Symbol(""),sA=Symbol(""),sN=Symbol(""),sE=Symbol(""),sI=Symbol(""),sR=Symbol(""),sO=Symbol(""),sM=Symbol(""),sP=Symbol(""),sF=Symbol(""),sL=Symbol(""),s$=Symbol(""),sD=Symbol(""),sV=Symbol(""),sj=Symbol(""),sB={[sn]:"Fragment",[sr]:"Teleport",[si]:"Suspense",[sl]:"KeepAlive",[ss]:"BaseTransition",[so]:"openBlock",[sa]:"createBlock",[sc]:"createElementBlock",[su]:"createVNode",[sd]:"createElementVNode",[sh]:"createCommentVNode",[sf]:"createTextVNode",[sp]:"createStaticVNode",[sg]:"resolveComponent",[sm]:"resolveDynamicComponent",[sv]:"resolveDirective",[sy]:"resolveFilter",[sb]:"withDirectives",[s_]:"renderList",[sS]:"renderSlot",[sx]:"createSlots",[sk]:"toDisplayString",[sC]:"mergeProps",[sT]:"normalizeClass",[sw]:"normalizeStyle",[sA]:"normalizeProps",[sN]:"guardReactiveProps",[sE]:"toHandlers",[sI]:"camelize",[sR]:"capitalize",[sO]:"toHandlerKey",[sM]:"setBlockTracking",[sP]:"pushScopeId",[sF]:"popScopeId",[sL]:"withCtx",[s$]:"unref",[sD]:"isRef",[sV]:"withMemo",[sj]:"isMemoSame"},sU={start:{line:1,column:1,offset:0},end:{line:1,column:1,offset:0},source:""};function sH(e,t,n,r,i,l,s,o=!1,a=!1,c=!1,u=sU){var d,h,f,p;return e&&(o?(e.helper(so),e.helper((d=e.inSSR,h=c,d||h?sa:sc))):e.helper((f=e.inSSR,p=c,f||p?su:sd)),s&&e.helper(sb)),{type:13,tag:t,props:n,children:r,patchFlag:i,dynamicProps:l,directives:s,isBlock:o,disableTracking:a,isComponent:c,loc:u}}function sq(e,t=sU){return{type:17,loc:t,elements:e}}function sW(e,t=sU){return{type:15,loc:t,properties:e}}function sK(e,t){return{type:16,loc:sU,key:R(e)?sz(e,!0):e,value:t}}function sz(e,t=!1,n=sU,r=0){return{type:4,loc:n,content:e,isStatic:t,constType:t?3:r}}function sJ(e,t=sU){return{type:8,loc:t,children:e}}function sG(e,t=[],n=sU){return{type:14,loc:n,callee:e,arguments:t}}function sX(e,t,n=!1,r=!1,i=sU){return{type:18,params:e,returns:t,newline:n,isSlot:r,loc:i}}function sQ(e,t,n,r=!0){return{type:19,test:e,consequent:t,alternate:n,newline:r,loc:sU}}function sZ(e,{helper:t,removeHelper:n,inSSR:r}){if(!e.isBlock){var i,l;e.isBlock=!0,n((i=e.isComponent,r||i?su:sd)),t(so),t((l=e.isComponent,r||l?sa:sc))}}let sY=new Uint8Array([123,123]),s0=new Uint8Array([125,125]);function s1(e){return e>=97&&e<=122||e>=65&&e<=90}function s2(e){return 32===e||10===e||9===e||12===e||13===e}function s3(e){return 47===e||62===e||s2(e)}function s6(e){let t=new Uint8Array(e.length);for(let n=0;n<e.length;n++)t[n]=e.charCodeAt(n);return t}let s4={Cdata:new Uint8Array([67,68,65,84,65,91]),CdataEnd:new Uint8Array([93,93,62]),CommentEnd:new Uint8Array([45,45,62]),ScriptEnd:new Uint8Array([60,47,115,99,114,105,112,116]),StyleEnd:new Uint8Array([60,47,115,116,121,108,101]),TitleEnd:new Uint8Array([60,47,116,105,116,108,101]),TextareaEnd:new Uint8Array([60,47,116,101,120,116,97,114,101,97])};function s8(e){throw e}function s5(e){}function s9(e,t,n,r){let i=SyntaxError(String(`https://vuejs.org/error-reference/#compiler-${e}`));return i.code=e,i.loc=t,i}let s7=e=>4===e.type&&e.isStatic;function oe(e){switch(e){case"Teleport":case"teleport":return sr;case"Suspense":case"suspense":return si;case"KeepAlive":case"keep-alive":return sl;case"BaseTransition":case"base-transition":return ss}}let ot=/^$|^\d|[^\$\w\xA0-\uFFFF]/,on=/[A-Za-z_$\xA0-\uFFFF]/,or=/[\.\?\w$\xA0-\uFFFF]/,oi=/\s+[.[]\s*|\s*[.[]\s+/g,ol=e=>4===e.type?e.content:e.loc.source,os=e=>{let t=ol(e).trim().replace(oi,e=>e.trim()),n=0,r=[],i=0,l=0,s=null;for(let e=0;e<t.length;e++){let o=t.charAt(e);switch(n){case 0:if("["===o)r.push(n),n=1,i++;else if("("===o)r.push(n),n=2,l++;else if(!(0===e?on:or).test(o))return!1;break;case 1:"'"===o||'"'===o||"`"===o?(r.push(n),n=3,s=o):"["===o?i++:"]"!==o||--i||(n=r.pop());break;case 2:if("'"===o||'"'===o||"`"===o)r.push(n),n=3,s=o;else if("("===o)l++;else if(")"===o){if(e===t.length-1)return!1;--l||(n=r.pop())}break;case 3:o===s&&(n=r.pop(),s=null)}}return!i&&!l},oo=/^\s*(?:async\s*)?(?:\([^)]*?\)|[\w$_]+)\s*(?::[^=]+)?=>|^\s*(?:async\s+)?function(?:\s+[\w$]+)?\s*\(/;function oa(e,t,n=!1){for(let r=0;r<e.props.length;r++){let i=e.props[r];if(7===i.type&&(n||i.exp)&&(R(t)?i.name===t:t.test(i.name)))return i}}function oc(e,t,n=!1,r=!1){for(let i=0;i<e.props.length;i++){let l=e.props[i];if(6===l.type){if(n)continue;if(l.name===t&&(l.value||r))return l}else if("bind"===l.name&&(l.exp||r)&&ou(l.arg,t))return l}}function ou(e,t){return!!(e&&s7(e)&&e.content===t)}function od(e){return 5===e.type||2===e.type}function oh(e){return 7===e.type&&"pre"===e.name}function of(e){return 7===e.type&&"slot"===e.name}function op(e){return 1===e.type&&3===e.tagType}function og(e){return 1===e.type&&2===e.tagType}let om=new Set([sA,sN]);function ov(e,t=[]){if(e&&!R(e)&&14===e.type){let n=e.callee;if(!R(n)&&om.has(n))return ov(e.arguments[0],t.concat(e))}return[e,t]}function oy(e,t,n){let r,i;if(13!==e.type&&function(e,t){var n,r,i;if(4!==t.key.type||"key"!==t.key.content)return!1;let l=e.arguments[2];if(l&&!R(l)){let[e]=ov(l);if(e&&!R(e)&&15===e.type&&ob(t,e))return!0}return(n=e.arguments)[2]||(n[2]="{}"),(r=e.arguments)[3]||(r[3]="undefined"),(i=e.arguments)[4]||(i[4]="undefined"),e.arguments[5]=t.value,!0}(e,t))return;let l=13===e.type?e.props:e.arguments[2],s=[];if(l&&!R(l)&&14===l.type){let e=ov(l);l=e[0],i=(s=e[1])[s.length-1]}if(null==l||R(l))r=sW([t]);else if(14===l.type){let e=l.arguments[0];R(e)||15!==e.type?l.callee===sE?r=sG(n.helper(sC),[sW([t]),l]):l.arguments.unshift(sW([t])):ob(t,e)||e.properties.unshift(t),r||(r=l)}else 15===l.type?(ob(t,l)||l.properties.unshift(t),r=l):(r=sG(n.helper(sC),[sW([t]),l]),i&&i.callee===sN&&(i=s[s.length-2]));13===e.type?i?i.arguments[0]=r:e.props=r:i?i.arguments[0]=r:e.arguments[2]=r}function ob(e,t){let n=!1;if(4===e.key.type){let r=e.key.content;n=t.properties.some(e=>4===e.key.type&&e.key.content===r)}return n}function o_(e,t){return`_${t}_${e.replace(/[^\w]/g,(t,n)=>"-"===t?"_":e.charCodeAt(n).toString())}`}let oS=/([\s\S]*?)\s+(?:in|of)\s+(\S[\s\S]*)/;function ox(e){for(let t=0;t<e.length;t++)if(!s2(e.charCodeAt(t)))return!1;return!0}function ok(e){return 2===e.type&&ox(e.content)||12===e.type&&ok(e.content)}function oC(e){return 3===e.type||ok(e)}let oT={parseMode:"base",ns:0,delimiters:["{{","}}"],getNamespace:()=>0,isVoidTag:x,isPreTag:x,isIgnoreNewlineTag:x,isCustomElement:x,onError:s8,onWarn:s5,comments:!1,prefixIdentifiers:!1},ow=oT,oA=null,oN="",oE=null,oI=null,oR="",oO=-1,oM=-1,oP=0,oF=!1,oL=null,o$=[],oD=new class{constructor(e,t){this.stack=e,this.cbs=t,this.state=1,this.buffer="",this.sectionStart=0,this.index=0,this.entityStart=0,this.baseState=1,this.inRCDATA=!1,this.inXML=!1,this.inVPre=!1,this.newlines=[],this.mode=0,this.delimiterOpen=sY,this.delimiterClose=s0,this.delimiterIndex=-1,this.currentSequence=void 0,this.sequenceIndex=0}get inSFCRoot(){return 2===this.mode&&0===this.stack.length}reset(){this.state=1,this.mode=0,this.buffer="",this.sectionStart=0,this.index=0,this.baseState=1,this.inRCDATA=!1,this.currentSequence=void 0,this.newlines.length=0,this.delimiterOpen=sY,this.delimiterClose=s0}getPos(e){let t=1,n=e+1,r=this.newlines.length,i=-1;if(r>100){let t=-1,n=r;for(;t+1<n;){let r=t+n>>>1;this.newlines[r]<e?t=r:n=r}i=t}else for(let t=r-1;t>=0;t--)if(e>this.newlines[t]){i=t;break}return i>=0&&(t=i+2,n=e-this.newlines[i]),{column:n,line:t,offset:e}}peek(){return this.buffer.charCodeAt(this.index+1)}stateText(e){60===e?(this.index>this.sectionStart&&this.cbs.ontext(this.sectionStart,this.index),this.state=5,this.sectionStart=this.index):this.inVPre||e!==this.delimiterOpen[0]||(this.state=2,this.delimiterIndex=0,this.stateInterpolationOpen(e))}stateInterpolationOpen(e){if(e===this.delimiterOpen[this.delimiterIndex])if(this.delimiterIndex===this.delimiterOpen.length-1){let e=this.index+1-this.delimiterOpen.length;e>this.sectionStart&&this.cbs.ontext(this.sectionStart,e),this.state=3,this.sectionStart=e}else this.delimiterIndex++;else this.inRCDATA?(this.state=32,this.stateInRCDATA(e)):(this.state=1,this.stateText(e))}stateInterpolation(e){e===this.delimiterClose[0]&&(this.state=4,this.delimiterIndex=0,this.stateInterpolationClose(e))}stateInterpolationClose(e){e===this.delimiterClose[this.delimiterIndex]?this.delimiterIndex===this.delimiterClose.length-1?(this.cbs.oninterpolation(this.sectionStart,this.index+1),this.inRCDATA?this.state=32:this.state=1,this.sectionStart=this.index+1):this.delimiterIndex++:(this.state=3,this.stateInterpolation(e))}stateSpecialStartSequence(e){let t=this.sequenceIndex===this.currentSequence.length;if(t?s3(e):(32|e)===this.currentSequence[this.sequenceIndex]){if(!t)return void this.sequenceIndex++}else this.inRCDATA=!1;this.sequenceIndex=0,this.state=6,this.stateInTagName(e)}stateInRCDATA(e){if(this.sequenceIndex===this.currentSequence.length){if(62===e||s2(e)){let t=this.index-this.currentSequence.length;if(this.sectionStart<t){let e=this.index;this.index=t,this.cbs.ontext(this.sectionStart,t),this.index=e}this.sectionStart=t+2,this.stateInClosingTagName(e),this.inRCDATA=!1;return}this.sequenceIndex=0}(32|e)===this.currentSequence[this.sequenceIndex]?this.sequenceIndex+=1:0===this.sequenceIndex?this.currentSequence!==s4.TitleEnd&&(this.currentSequence!==s4.TextareaEnd||this.inSFCRoot)?this.fastForwardTo(60)&&(this.sequenceIndex=1):this.inVPre||e!==this.delimiterOpen[0]||(this.state=2,this.delimiterIndex=0,this.stateInterpolationOpen(e)):this.sequenceIndex=Number(60===e)}stateCDATASequence(e){e===s4.Cdata[this.sequenceIndex]?++this.sequenceIndex===s4.Cdata.length&&(this.state=28,this.currentSequence=s4.CdataEnd,this.sequenceIndex=0,this.sectionStart=this.index+1):(this.sequenceIndex=0,this.state=23,this.stateInDeclaration(e))}fastForwardTo(e){for(;++this.index<this.buffer.length;){let t=this.buffer.charCodeAt(this.index);if(10===t&&this.newlines.push(this.index),t===e)return!0}return this.index=this.buffer.length-1,!1}stateInCommentLike(e){e===this.currentSequence[this.sequenceIndex]?++this.sequenceIndex===this.currentSequence.length&&(this.currentSequence===s4.CdataEnd?this.cbs.oncdata(this.sectionStart,this.index-2):this.cbs.oncomment(this.sectionStart,this.index-2),this.sequenceIndex=0,this.sectionStart=this.index+1,this.state=1):0===this.sequenceIndex?this.fastForwardTo(this.currentSequence[0])&&(this.sequenceIndex=1):e!==this.currentSequence[this.sequenceIndex-1]&&(this.sequenceIndex=0)}startSpecial(e,t){this.enterRCDATA(e,t),this.state=31}enterRCDATA(e,t){this.inRCDATA=!0,this.currentSequence=e,this.sequenceIndex=t}stateBeforeTagName(e){33===e?(this.state=22,this.sectionStart=this.index+1):63===e?(this.state=24,this.sectionStart=this.index+1):s1(e)?(this.sectionStart=this.index,0===this.mode?this.state=6:this.inSFCRoot?this.state=34:this.inXML?this.state=6:116===e?this.state=30:this.state=115===e?29:6):47===e?this.state=8:(this.state=1,this.stateText(e))}stateInTagName(e){s3(e)&&this.handleTagName(e)}stateInSFCRootTagName(e){if(s3(e)){let t=this.buffer.slice(this.sectionStart,this.index);"template"!==t&&this.enterRCDATA(s6("</"+t),0),this.handleTagName(e)}}handleTagName(e){this.cbs.onopentagname(this.sectionStart,this.index),this.sectionStart=-1,this.state=11,this.stateBeforeAttrName(e)}stateBeforeClosingTagName(e){s2(e)||(62===e?(this.state=1,this.sectionStart=this.index+1):(this.state=s1(e)?9:27,this.sectionStart=this.index))}stateInClosingTagName(e){(62===e||s2(e))&&(this.cbs.onclosetag(this.sectionStart,this.index),this.sectionStart=-1,this.state=10,this.stateAfterClosingTagName(e))}stateAfterClosingTagName(e){62===e&&(this.state=1,this.sectionStart=this.index+1)}stateBeforeAttrName(e){62===e?(this.cbs.onopentagend(this.index),this.inRCDATA?this.state=32:this.state=1,this.sectionStart=this.index+1):47===e?this.state=7:60===e&&47===this.peek()?(this.cbs.onopentagend(this.index),this.state=5,this.sectionStart=this.index):s2(e)||this.handleAttrStart(e)}handleAttrStart(e){118===e&&45===this.peek()?(this.state=13,this.sectionStart=this.index):46===e||58===e||64===e||35===e?(this.cbs.ondirname(this.index,this.index+1),this.state=14,this.sectionStart=this.index+1):(this.state=12,this.sectionStart=this.index)}stateInSelfClosingTag(e){62===e?(this.cbs.onselfclosingtag(this.index),this.state=1,this.sectionStart=this.index+1,this.inRCDATA=!1):s2(e)||(this.state=11,this.stateBeforeAttrName(e))}stateInAttrName(e){(61===e||s3(e))&&(this.cbs.onattribname(this.sectionStart,this.index),this.handleAttrNameEnd(e))}stateInDirName(e){61===e||s3(e)?(this.cbs.ondirname(this.sectionStart,this.index),this.handleAttrNameEnd(e)):58===e?(this.cbs.ondirname(this.sectionStart,this.index),this.state=14,this.sectionStart=this.index+1):46===e&&(this.cbs.ondirname(this.sectionStart,this.index),this.state=16,this.sectionStart=this.index+1)}stateInDirArg(e){61===e||s3(e)?(this.cbs.ondirarg(this.sectionStart,this.index),this.handleAttrNameEnd(e)):91===e?this.state=15:46===e&&(this.cbs.ondirarg(this.sectionStart,this.index),this.state=16,this.sectionStart=this.index+1)}stateInDynamicDirArg(e){93===e?this.state=14:(61===e||s3(e))&&(this.cbs.ondirarg(this.sectionStart,this.index+1),this.handleAttrNameEnd(e))}stateInDirModifier(e){61===e||s3(e)?(this.cbs.ondirmodifier(this.sectionStart,this.index),this.handleAttrNameEnd(e)):46===e&&(this.cbs.ondirmodifier(this.sectionStart,this.index),this.sectionStart=this.index+1)}handleAttrNameEnd(e){this.sectionStart=this.index,this.state=17,this.cbs.onattribnameend(this.index),this.stateAfterAttrName(e)}stateAfterAttrName(e){61===e?this.state=18:47===e||62===e?(this.cbs.onattribend(0,this.sectionStart),this.sectionStart=-1,this.state=11,this.stateBeforeAttrName(e)):s2(e)||(this.cbs.onattribend(0,this.sectionStart),this.handleAttrStart(e))}stateBeforeAttrValue(e){34===e?(this.state=19,this.sectionStart=this.index+1):39===e?(this.state=20,this.sectionStart=this.index+1):s2(e)||(this.sectionStart=this.index,this.state=21,this.stateInAttrValueNoQuotes(e))}handleInAttrValue(e,t){(e===t||this.fastForwardTo(t))&&(this.cbs.onattribdata(this.sectionStart,this.index),this.sectionStart=-1,this.cbs.onattribend(34===t?3:2,this.index+1),this.state=11)}stateInAttrValueDoubleQuotes(e){this.handleInAttrValue(e,34)}stateInAttrValueSingleQuotes(e){this.handleInAttrValue(e,39)}stateInAttrValueNoQuotes(e){s2(e)||62===e?(this.cbs.onattribdata(this.sectionStart,this.index),this.sectionStart=-1,this.cbs.onattribend(1,this.index),this.state=11,this.stateBeforeAttrName(e)):(39===e||60===e||61===e||96===e)&&this.cbs.onerr(18,this.index)}stateBeforeDeclaration(e){91===e?(this.state=26,this.sequenceIndex=0):this.state=45===e?25:23}stateInDeclaration(e){(62===e||this.fastForwardTo(62))&&(this.state=1,this.sectionStart=this.index+1)}stateInProcessingInstruction(e){(62===e||this.fastForwardTo(62))&&(this.cbs.onprocessinginstruction(this.sectionStart,this.index),this.state=1,this.sectionStart=this.index+1)}stateBeforeComment(e){45===e?(this.state=28,this.currentSequence=s4.CommentEnd,this.sequenceIndex=2,this.sectionStart=this.index+1):this.state=23}stateInSpecialComment(e){(62===e||this.fastForwardTo(62))&&(this.cbs.oncomment(this.sectionStart,this.index),this.state=1,this.sectionStart=this.index+1)}stateBeforeSpecialS(e){e===s4.ScriptEnd[3]?this.startSpecial(s4.ScriptEnd,4):e===s4.StyleEnd[3]?this.startSpecial(s4.StyleEnd,4):(this.state=6,this.stateInTagName(e))}stateBeforeSpecialT(e){e===s4.TitleEnd[3]?this.startSpecial(s4.TitleEnd,4):e===s4.TextareaEnd[3]?this.startSpecial(s4.TextareaEnd,4):(this.state=6,this.stateInTagName(e))}startEntity(){}stateInEntity(){}parse(e){for(this.buffer=e;this.index<this.buffer.length;){let e=this.buffer.charCodeAt(this.index);switch(10===e&&33!==this.state&&this.newlines.push(this.index),this.state){case 1:this.stateText(e);break;case 2:this.stateInterpolationOpen(e);break;case 3:this.stateInterpolation(e);break;case 4:this.stateInterpolationClose(e);break;case 31:this.stateSpecialStartSequence(e);break;case 32:this.stateInRCDATA(e);break;case 26:this.stateCDATASequence(e);break;case 19:this.stateInAttrValueDoubleQuotes(e);break;case 12:this.stateInAttrName(e);break;case 13:this.stateInDirName(e);break;case 14:this.stateInDirArg(e);break;case 15:this.stateInDynamicDirArg(e);break;case 16:this.stateInDirModifier(e);break;case 28:this.stateInCommentLike(e);break;case 27:this.stateInSpecialComment(e);break;case 11:this.stateBeforeAttrName(e);break;case 6:this.stateInTagName(e);break;case 34:this.stateInSFCRootTagName(e);break;case 9:this.stateInClosingTagName(e);break;case 5:this.stateBeforeTagName(e);break;case 17:this.stateAfterAttrName(e);break;case 20:this.stateInAttrValueSingleQuotes(e);break;case 18:this.stateBeforeAttrValue(e);break;case 8:this.stateBeforeClosingTagName(e);break;case 10:this.stateAfterClosingTagName(e);break;case 29:this.stateBeforeSpecialS(e);break;case 30:this.stateBeforeSpecialT(e);break;case 21:this.stateInAttrValueNoQuotes(e);break;case 7:this.stateInSelfClosingTag(e);break;case 23:this.stateInDeclaration(e);break;case 22:this.stateBeforeDeclaration(e);break;case 25:this.stateBeforeComment(e);break;case 24:this.stateInProcessingInstruction(e);break;case 33:this.stateInEntity()}this.index++}this.cleanup(),this.finish()}cleanup(){this.sectionStart!==this.index&&(1===this.state||32===this.state&&0===this.sequenceIndex?(this.cbs.ontext(this.sectionStart,this.index),this.sectionStart=this.index):(19===this.state||20===this.state||21===this.state)&&(this.cbs.onattribdata(this.sectionStart,this.index),this.sectionStart=this.index))}finish(){this.handleTrailingData(),this.cbs.onend()}handleTrailingData(){let e=this.buffer.length;this.sectionStart>=e||(28===this.state?this.currentSequence===s4.CdataEnd?this.cbs.oncdata(this.sectionStart,e):this.cbs.oncomment(this.sectionStart,e):6===this.state||11===this.state||18===this.state||17===this.state||12===this.state||13===this.state||14===this.state||15===this.state||16===this.state||20===this.state||19===this.state||21===this.state||9===this.state||this.cbs.ontext(this.sectionStart,e))}emitCodePoint(e,t){}}(o$,{onerr:o0,ontext(e,t){oH(oB(e,t),e,t)},ontextentity(e,t,n){oH(e,t,n)},oninterpolation(e,t){if(oF)return oH(oB(e,t),e,t);let n=e+oD.delimiterOpen.length,r=t-oD.delimiterClose.length;for(;s2(oN.charCodeAt(n));)n++;for(;s2(oN.charCodeAt(r-1));)r--;let i=oB(n,r);i.includes("&")&&(i=ow.decodeEntities(i,!1)),oX({type:5,content:oY(i,!1,oQ(n,r)),loc:oQ(e,t)})},onopentagname(e,t){let n=oB(e,t);oE={type:1,tag:n,ns:ow.getNamespace(n,o$[0],ow.ns),tagType:0,props:[],children:[],loc:oQ(e-1,t),codegenNode:void 0}},onopentagend(e){oU(e)},onclosetag(e,t){let n=oB(e,t);if(!ow.isVoidTag(n)){let r=!1;for(let e=0;e<o$.length;e++)if(o$[e].tag.toLowerCase()===n.toLowerCase()){r=!0,e>0&&o$[0].loc.start.offset;for(let n=0;n<=e;n++)oq(o$.shift(),t,n<e);break}r||oW(e,60)}},onselfclosingtag(e){let t=oE.tag;oE.isSelfClosing=!0,oU(e),o$[0]&&o$[0].tag===t&&oq(o$.shift(),e)},onattribname(e,t){oI={type:6,name:oB(e,t),nameLoc:oQ(e,t),value:void 0,loc:oQ(e)}},ondirname(e,t){let n=oB(e,t),r="."===n||":"===n?"bind":"@"===n?"on":"#"===n?"slot":n.slice(2);if(oF||""===r)oI={type:6,name:n,nameLoc:oQ(e,t),value:void 0,loc:oQ(e)};else if(oI={type:7,name:r,rawName:n,exp:void 0,arg:void 0,modifiers:"."===n?[sz("prop")]:[],loc:oQ(e)},"pre"===r){oF=oD.inVPre=!0,oL=oE;let e=oE.props;for(let t=0;t<e.length;t++)7===e[t].type&&(e[t]=function(e){let t={type:6,name:e.rawName,nameLoc:oQ(e.loc.start.offset,e.loc.start.offset+e.rawName.length),value:void 0,loc:e.loc};if(e.exp){let n=e.exp.loc;n.end.offset<e.loc.end.offset&&(n.start.offset--,n.start.column--,n.end.offset++,n.end.column++),t.value={type:2,content:e.exp.content,loc:n}}return t}(e[t]))}},ondirarg(e,t){if(e===t)return;let n=oB(e,t);if(oF&&!oh(oI))oI.name+=n,oZ(oI.nameLoc,t);else{let r="["!==n[0];oI.arg=oY(r?n:n.slice(1,-1),r,oQ(e,t),3*!!r)}},ondirmodifier(e,t){let n=oB(e,t);if(oF&&!oh(oI))oI.name+="."+n,oZ(oI.nameLoc,t);else if("slot"===oI.name){let e=oI.arg;e&&(e.content+="."+n,oZ(e.loc,t))}else{let r=sz(n,!0,oQ(e,t));oI.modifiers.push(r)}},onattribdata(e,t){oR+=oB(e,t),oO<0&&(oO=e),oM=t},onattribentity(e,t,n){oR+=e,oO<0&&(oO=t),oM=n},onattribnameend(e){let t=oB(oI.loc.start.offset,e);7===oI.type&&(oI.rawName=t),oE.props.some(e=>(7===e.type?e.rawName:e.name)===t)},onattribend(e,t){oE&&oI&&(oZ(oI.loc,t),0!==e&&(oR.includes("&")&&(oR=ow.decodeEntities(oR,!0)),6===oI.type?("class"===oI.name&&(oR=oG(oR).trim()),oI.value={type:2,content:oR,loc:1===e?oQ(oO,oM):oQ(oO-1,oM+1)},oD.inSFCRoot&&"template"===oE.tag&&"lang"===oI.name&&oR&&"html"!==oR&&oD.enterRCDATA(s6("</template"),0)):(oI.exp=oY(oR,!1,oQ(oO,oM),0,0),"for"===oI.name&&(oI.forParseResult=function(e){let t=e.loc,n=e.content,r=n.match(oS);if(!r)return;let[,i,l]=r,s=(e,n,r=!1)=>{let i=t.start.offset+n,l=i+e.length;return oY(e,!1,oQ(i,l),0,+!!r)},o={source:s(l.trim(),n.indexOf(l,i.length)),value:void 0,key:void 0,index:void 0,finalized:!1},a=i.trim().replace(oj,"").trim(),c=i.indexOf(a),u=a.match(oV);if(u){let e;a=a.replace(oV,"").trim();let t=u[1].trim();if(t&&(e=n.indexOf(t,c+a.length),o.key=s(t,e,!0)),u[2]){let r=u[2].trim();r&&(o.index=s(r,n.indexOf(r,o.key?e+t.length:c+a.length),!0))}}return a&&(o.value=s(a,c,!0)),o}(oI.exp)))),(7!==oI.type||"pre"!==oI.name)&&oE.props.push(oI)),oR="",oO=oM=-1},oncomment(e,t){ow.comments&&oX({type:3,content:oB(e,t),loc:oQ(e-4,t+3)})},onend(){let e=oN.length;for(let t=0;t<o$.length;t++)oq(o$[t],e-1),o$[t].loc.start.offset},oncdata(e,t){(o$[0]?o$[0].ns:ow.ns)!==0&&oH(oB(e,t),e,t)},onprocessinginstruction(e){(o$[0]?o$[0].ns:ow.ns)===0&&o0(21,e-1)}}),oV=/,([^,\}\]]*)(?:,([^,\}\]]*))?$/,oj=/^\(|\)$/g;function oB(e,t){return oN.slice(e,t)}function oU(e){oD.inSFCRoot&&(oE.innerLoc=oQ(e+1,e+1)),oX(oE);let{tag:t,ns:n}=oE;0===n&&ow.isPreTag(t)&&oP++,ow.isVoidTag(t)?oq(oE,e):(o$.unshift(oE),(1===n||2===n)&&(oD.inXML=!0)),oE=null}function oH(e,t,n){{let t=o$[0]&&o$[0].tag;"script"!==t&&"style"!==t&&e.includes("&")&&(e=ow.decodeEntities(e,!1))}let r=o$[0]||oA,i=r.children[r.children.length-1];i&&2===i.type?(i.content+=e,oZ(i.loc,n)):r.children.push({type:2,content:e,loc:oQ(t,n)})}function oq(e,t,n=!1){n?oZ(e.loc,oW(t,60)):oZ(e.loc,function(e){let t=e;for(;62!==oN.charCodeAt(t)&&t<oN.length-1;)t++;return t}(t)+1),oD.inSFCRoot&&(e.children.length?e.innerLoc.end=T({},e.children[e.children.length-1].loc.end):e.innerLoc.end=T({},e.innerLoc.start),e.innerLoc.source=oB(e.innerLoc.start.offset,e.innerLoc.end.offset));let{tag:r,ns:i,children:l}=e;if(!oF&&("slot"===r?e.tagType=2:!function({tag:e,props:t}){if("template"===e){for(let e=0;e<t.length;e++)if(7===t[e].type&&oK.has(t[e].name))return!0}return!1}(e)?function({tag:e,props:t}){var n;if(ow.isCustomElement(e))return!1;if("component"===e||(n=e.charCodeAt(0))>64&&n<91||oe(e)||ow.isBuiltInComponent&&ow.isBuiltInComponent(e)||ow.isNativeTag&&!ow.isNativeTag(e))return!0;for(let e=0;e<t.length;e++){let n=t[e];if(6===n.type&&"is"===n.name&&n.value&&n.value.content.startsWith("vue:"))return!0}return!1}(e)&&(e.tagType=1):e.tagType=3),oD.inRCDATA||(e.children=oJ(l)),0===i&&ow.isIgnoreNewlineTag(r)){let e=l[0];e&&2===e.type&&(e.content=e.content.replace(/^\r?\n/,""))}0===i&&ow.isPreTag(r)&&oP--,oL===e&&(oF=oD.inVPre=!1,oL=null),oD.inXML&&(o$[0]?o$[0].ns:ow.ns)===0&&(oD.inXML=!1)}function oW(e,t){let n=e;for(;oN.charCodeAt(n)!==t&&n>=0;)n--;return n}let oK=new Set(["if","else","else-if","for","slot"]),oz=/\r\n/g;function oJ(e){let t="preserve"!==ow.whitespace,n=!1;for(let r=0;r<e.length;r++){let i=e[r];if(2===i.type)if(oP)i.content=i.content.replace(oz,`
`);else if(ox(i.content)){let l=e[r-1]&&e[r-1].type,s=e[r+1]&&e[r+1].type;!l||!s||t&&(3===l&&(3===s||1===s)||1===l&&(3===s||1===s&&function(e){for(let t=0;t<e.length;t++){let n=e.charCodeAt(t);if(10===n||13===n)return!0}return!1}(i.content)))?(n=!0,e[r]=null):i.content=" "}else t&&(i.content=oG(i.content))}return n?e.filter(Boolean):e}function oG(e){let t="",n=!1;for(let r=0;r<e.length;r++)s2(e.charCodeAt(r))?n||(t+=" ",n=!0):(t+=e[r],n=!1);return t}function oX(e){(o$[0]||oA).children.push(e)}function oQ(e,t){return{start:oD.getPos(e),end:null==t?t:oD.getPos(t),source:null==t?t:oB(e,t)}}function oZ(e,t){e.end=oD.getPos(t),e.source=oB(e.start.offset,t)}function oY(e,t=!1,n,r=0,i=0){return sz(e,t,n,r)}function o0(e,t,n){ow.onError(s9(e,oQ(t,t)))}function o1(e){let t=e.children.filter(e=>3!==e.type);return 1!==t.length||1!==t[0].type||og(t[0])?null:t[0]}function o2(e,t){let{constantCache:n}=t;switch(e.type){case 1:if(0!==e.tagType)return 0;let r=n.get(e);if(void 0!==r)return r;let i=e.codegenNode;if(13!==i.type||i.isBlock&&"svg"!==e.tag&&"foreignObject"!==e.tag&&"math"!==e.tag)return 0;if(void 0!==i.patchFlag)return n.set(e,0),0;{let r=3,c=o6(e,t);if(0===c)return n.set(e,0),0;c<r&&(r=c);for(let i=0;i<e.children.length;i++){let l=o2(e.children[i],t);if(0===l)return n.set(e,0),0;l<r&&(r=l)}if(r>1)for(let i=0;i<e.props.length;i++){let l=e.props[i];if(7===l.type&&"bind"===l.name&&l.exp){let i=o2(l.exp,t);if(0===i)return n.set(e,0),0;i<r&&(r=i)}}if(i.isBlock){var l,s,o,a;for(let t=0;t<e.props.length;t++)if(7===e.props[t].type)return n.set(e,0),0;t.removeHelper(so),t.removeHelper((l=t.inSSR,s=i.isComponent,l||s?sa:sc)),i.isBlock=!1,t.helper((o=t.inSSR,a=i.isComponent,o||a?su:sd))}return n.set(e,r),r}case 2:case 3:return 3;case 9:case 11:case 10:default:return 0;case 5:case 12:return o2(e.content,t);case 4:return e.constType;case 8:let c=3;for(let n=0;n<e.children.length;n++){let r=e.children[n];if(R(r)||O(r))continue;let i=o2(r,t);if(0===i)return 0;i<c&&(c=i)}return c;case 20:return 2}}let o3=new Set([sT,sw,sA,sN]);function o6(e,t){let n=3,r=o4(e);if(r&&15===r.type){let{properties:e}=r;for(let r=0;r<e.length;r++){let i,{key:l,value:s}=e[r],o=o2(l,t);if(0===o)return o;if(o<n&&(n=o),0===(i=4===s.type?o2(s,t):14===s.type?function e(t,n){if(14===t.type&&!R(t.callee)&&o3.has(t.callee)){let r=t.arguments[0];if(4===r.type)return o2(r,n);if(14===r.type)return e(r,n)}return 0}(s,t):0))return i;i<n&&(n=i)}}return n}function o4(e){let t=e.codegenNode;if(13===t.type)return t.props}function o8(e,t){t.currentNode=e;let{nodeTransforms:n}=t,r=[];for(let i=0;i<n.length;i++){let l=n[i](e,t);if(l&&(E(l)?r.push(...l):r.push(l)),!t.currentNode)return;e=t.currentNode}switch(e.type){case 3:t.ssr||t.helper(sh);break;case 5:t.ssr||t.helper(sk);break;case 9:for(let n=0;n<e.branches.length;n++)o8(e.branches[n],t);break;case 10:case 11:case 1:case 0:var i=e;let l=0,s=()=>{l--};for(;l<i.children.length;l++){let e=i.children[l];R(e)||(t.grandParent=t.parent,t.parent=i,t.childIndex=l,t.onNodeRemoved=s,o8(e,t))}}t.currentNode=e;let o=r.length;for(;o--;)r[o]()}function o5(e,t){let n=R(e)?t=>t===e:t=>e.test(t);return(e,r)=>{if(1===e.type){let{props:i}=e;if(3===e.tagType&&i.some(of))return;let l=[];for(let s=0;s<i.length;s++){let o=i[s];if(7===o.type&&n(o.name)){i.splice(s,1),s--;let n=t(e,o,r);n&&l.push(n)}}return l}}}let o9="/*@__PURE__*/",o7=e=>`${sB[e]}: _${sB[e]}`;function ae(e,t,{helper:n,push:r,newline:i,isTS:l}){let s=n("component"===t?sg:sv);for(let n=0;n<e.length;n++){let o=e[n],a=o.endsWith("__self");a&&(o=o.slice(0,-6)),r(`const ${o_(o,t)} = ${s}(${JSON.stringify(o)}${a?", true":""})${l?"!":""}`),n<e.length-1&&i()}}function at(e,t){let n=e.length>3;t.push("["),n&&t.indent(),an(e,t,n),n&&t.deindent(),t.push("]")}function an(e,t,n=!1,r=!0){let{push:i,newline:l}=t;for(let s=0;s<e.length;s++){let o=e[s];R(o)?i(o,-3):E(o)?at(o,t):ar(o,t),s<e.length-1&&(n?(r&&i(","),l()):r&&i(", "))}}function ar(e,t){var n,r,i;if(R(e))return void t.push(e,-3);if(O(e))return void t.push(t.helper(e));switch(e.type){case 1:case 9:case 11:case 12:ar(e.codegenNode,t);break;case 2:n=e,t.push(JSON.stringify(n.content),-3,n);break;case 4:ai(e,t);break;case 5:!function(e,t){let{push:n,helper:r,pure:i}=t;i&&n(o9),n(`${r(sk)}(`),ar(e.content,t),n(")")}(e,t);break;case 8:al(e,t);break;case 3:!function(e,t){let{push:n,helper:r,pure:i}=t;i&&n(o9),n(`${r(sh)}(${JSON.stringify(e.content)})`,-3,e)}(e,t);break;case 13:!function(e,t){var n,r;let i,{push:l,helper:s,pure:o}=t,{tag:a,props:c,children:u,patchFlag:d,dynamicProps:h,directives:f,isBlock:p,disableTracking:g,isComponent:m}=e;d&&(i=String(d)),f&&l(s(sb)+"("),p&&l(`(${s(so)}(${g?"true":""}), `),o&&l(o9),l(s(p?(n=t.inSSR,n||m?sa:sc):(r=t.inSSR,r||m?su:sd))+"(",-2,e),an(function(e){let t=e.length;for(;t--&&null==e[t];);return e.slice(0,t+1).map(e=>e||"null")}([a,c,u,i,h]),t),l(")"),p&&l(")"),f&&(l(", "),ar(f,t),l(")"))}(e,t);break;case 14:!function(e,t){let{push:n,helper:r,pure:i}=t,l=R(e.callee)?e.callee:r(e.callee);i&&n(o9),n(l+"(",-2,e),an(e.arguments,t),n(")")}(e,t);break;case 15:!function(e,t){let{push:n,indent:r,deindent:i,newline:l}=t,{properties:s}=e;if(!s.length)return n("{}",-2,e);let o=s.length>1;n(o?"{":"{ "),o&&r();for(let e=0;e<s.length;e++){let{key:r,value:i}=s[e];!function(e,t){let{push:n}=t;if(8===e.type)n("["),al(e,t),n("]");else if(e.isStatic){let t;n((t=e.content,ot.test(t))?JSON.stringify(e.content):e.content,-2,e)}else n(`[${e.content}]`,-3,e)}(r,t),n(": "),ar(i,t),e<s.length-1&&(n(","),l())}o&&i(),n(o?"}":" }")}(e,t);break;case 17:r=e,i=t,at(r.elements,i);break;case 18:!function(e,t){let{push:n,indent:r,deindent:i}=t,{params:l,returns:s,body:o,newline:a,isSlot:c}=e;c&&n(`_${sB[sL]}(`),n("(",-2,e),E(l)?an(l,t):l&&ar(l,t),n(") => "),(a||o)&&(n("{"),r()),s?(a&&n("return "),E(s)?at(s,t):ar(s,t)):o&&ar(o,t),(a||o)&&(i(),n("}")),c&&n(")")}(e,t);break;case 19:!function(e,t){let{test:n,consequent:r,alternate:i,newline:l}=e,{push:s,indent:o,deindent:a,newline:c}=t;if(4===n.type){let e,r=(e=n.content,!!ot.test(e));r&&s("("),ai(n,t),r&&s(")")}else s("("),ar(n,t),s(")");l&&o(),t.indentLevel++,l||s(" "),s("? "),ar(r,t),t.indentLevel--,l&&c(),l||s(" "),s(": ");let u=19===i.type;!u&&t.indentLevel++,ar(i,t),!u&&t.indentLevel--,l&&a(!0)}(e,t);break;case 20:!function(e,t){let{push:n,helper:r,indent:i,deindent:l,newline:s}=t,{needPauseTracking:o,needArraySpread:a}=e;a&&n("[...("),n(`_cache[${e.index}] || (`),o&&(i(),n(`${r(sM)}(-1`),e.inVOnce&&n(", true"),n("),"),s(),n("(")),n(`_cache[${e.index}] = `),ar(e.value,t),o&&(n(`).cacheIndex = ${e.index},`),s(),n(`${r(sM)}(1),`),s(),n(`_cache[${e.index}]`),l()),n(")"),a&&n(")]")}(e,t);break;case 21:an(e.body,t,!0,!1)}}function ai(e,t){let{content:n,isStatic:r}=e;t.push(r?JSON.stringify(n):n,-3,e)}function al(e,t){for(let n=0;n<e.children.length;n++){let r=e.children[n];R(r)?t.push(r,-3):ar(r,t)}}let as=o5(/^(?:if|else|else-if)$/,(e,t,n)=>(function(e,t,n,r){if("else"!==t.name&&(!t.exp||!t.exp.content.trim())){let r=t.exp?t.exp.loc:e.loc;n.onError(s9(28,t.loc)),t.exp=sz("true",!1,r)}if("if"===t.name){var i;let l=ao(e,t),s={type:9,loc:oQ((i=e.loc).start.offset,i.end.offset),branches:[l]};if(n.replaceNode(s),r)return r(s,l,!0)}else{let i=n.parent.children,l=i.indexOf(e);for(;l-- >=-1;){let s=i[l];if(s&&oC(s)){n.removeNode(s);continue}if(s&&9===s.type){("else-if"===t.name||"else"===t.name)&&void 0===s.branches[s.branches.length-1].condition&&n.onError(s9(30,e.loc)),n.removeNode();let i=ao(e,t);s.branches.push(i);let l=r&&r(s,i,!1);o8(i,n),l&&l(),n.currentNode=null}else n.onError(s9(30,e.loc));break}}})(e,t,n,(e,t,r)=>{let i=n.parent.children,l=i.indexOf(e),s=0;for(;l-- >=0;){let e=i[l];e&&9===e.type&&(s+=e.branches.length)}return()=>{r?e.codegenNode=aa(t,s,n):function(e){for(;;)if(19===e.type)if(19!==e.alternate.type)return e;else e=e.alternate;else 20===e.type&&(e=e.value)}(e.codegenNode).alternate=aa(t,s+e.branches.length-1,n)}}));function ao(e,t){let n=3===e.tagType;return{type:10,loc:e.loc,condition:"else"===t.name?void 0:t.exp,children:n&&!oa(e,"for")?e.children:[e],userKey:oc(e,"key"),isTemplateIf:n}}function aa(e,t,n){return e.condition?sQ(e.condition,ac(e,t,n),sG(n.helper(sh),['""',"true"])):ac(e,t,n)}function ac(e,t,n){let{helper:r}=n,i=sK("key",sz(`${t}`,!1,sU,2)),{children:l}=e,s=l[0];if(1!==l.length||1!==s.type)if(1!==l.length||11!==s.type)return sH(n,r(sn),sW([i]),l,64,void 0,void 0,!0,!1,!1,e.loc);else{let e=s.codegenNode;return oy(e,i,n),e}{let e=s.codegenNode,t=14===e.type&&e.callee===sV?e.arguments[1].returns:e;return 13===t.type&&sZ(t,n),oy(t,i,n),e}}let au=o5("for",(e,t,n)=>{let{helper:r,removeHelper:i}=n;return function(e,t,n,r){if(!t.exp)return void n.onError(s9(31,t.loc));let i=t.forParseResult;if(!i)return void n.onError(s9(32,t.loc));ad(i);let{scopes:l}=n,{source:s,value:o,key:a,index:c}=i,u={type:11,loc:t.loc,source:s,valueAlias:o,keyAlias:a,objectIndexAlias:c,parseResult:i,children:op(e)?e.children:[e]};n.replaceNode(u),l.vFor++;let d=r&&r(u);return()=>{l.vFor--,d&&d()}}(e,t,n,t=>{let l=sG(r(s_),[t.source]),s=op(e),o=oa(e,"memo"),a=oc(e,"key",!1,!0);a&&a.type;let c=a&&(6===a.type?a.value?sz(a.value.content,!0):void 0:a.exp),u=c?sK("key",c):null,d=4===t.source.type&&t.source.constType>0,h=d?64:a?128:256;return t.codegenNode=sH(n,r(sn),void 0,l,h,void 0,void 0,!0,!d,!1,e.loc),()=>{var a,h,f,p,g,m,y,b,_;let S,{children:x}=t,k=1!==x.length||1!==x[0].type,C=og(e)?e:s&&1===e.children.length&&og(e.children[0])?e.children[0]:null;if(C)S=C.codegenNode,s&&u&&oy(S,u,n);else if(k)S=sH(n,r(sn),u?sW([u]):void 0,e.children,64,void 0,void 0,!0,void 0,!1);else{S=x[0].codegenNode,s&&u&&oy(S,u,n);let e=!d||!0===S.isBlockRequired;S.isBlock!==e&&(S.isBlock?(i(so),i((h=n.inSSR,f=S.isComponent,h||f?sa:sc))):i((p=n.inSSR,g=S.isComponent,p||g?su:sd))),(S.isBlock=e,S.isBlock)?(r(so),r((m=n.inSSR,y=S.isComponent,m||y?sa:sc))):(r((b=n.inSSR,_=S.isComponent,b||_?su:sd)),S.needsPatch&&(S.patchFlag=(null!=(a=S.patchFlag)?a:0)|512))}if(o){let e=sX(ah(t.parseResult,[sz("_cached")]));e.body={type:21,body:[sJ(["const _memo = (",o.exp,")"]),sJ(["if (_cached && _cached.el",...c?[" && _cached.key === ",c]:[],` && ${n.helperString(sj)}(_cached, _memo)) return _cached`]),sJ(["const _item = ",S]),sz("_item.memo = _memo"),sz("return _item")],loc:sU},l.arguments.push(e,sz("_cache"),sz(String(n.cached.length))),n.cached.push(null)}else l.arguments.push(sX(ah(t.parseResult),S,!0))}})});function ad(e,t){e.finalized||(e.finalized=!0)}function ah({value:e,key:t,index:n},r=[]){var i=[e,t,n,...r];let l=i.length;for(;l--&&!i[l];);return i.slice(0,l+1).map((e,t)=>e||sz("_".repeat(t+1),!1))}let af=sz("undefined",!1),ap=(e,t)=>{if(1===e.type&&(1===e.tagType||3===e.tagType)){let n=oa(e,"slot");if(n)return n.exp,t.scopes.vSlot++,()=>{t.scopes.vSlot--}}};function ag(e,t,n){let r=[sK("name",e),sK("fn",t)];return null!=n&&r.push(sK("key",sz(String(n),!0))),sW(r)}let am=new WeakMap,av=(e,t)=>function(){let n,r,i,l,s;if(1!==(e=t.currentNode).type||0!==e.tagType&&1!==e.tagType)return;let{tag:o,props:a}=e,c=1===e.tagType,u=c?function(e,t,n=!1){let{tag:r}=e,i=a_(r),l=oc(e,"is",!1,!0);if(l)if(i){let e;if(6===l.type?e=l.value&&sz(l.value.content,!0):(e=l.exp)||(e=sz("is",!1,l.arg.loc)),e)return sG(t.helper(sm),[e])}else 6===l.type&&l.value.content.startsWith("vue:")&&(r=l.value.content.slice(4));let s=oe(r)||t.isBuiltInComponent(r);return s?(n||t.helper(s),s):(t.helper(sg),t.components.add(r),o_(r,"component"))}(e,t):`"${o}"`,d=M(u)&&u.callee===sm,h=0,f=!1,p=!1,g=d||u===sr||u===si||!c&&("svg"===o||"foreignObject"===o||"math"===o);if(a.length>0){let r=ay(e,t,void 0,c,d);n=r.props,h=r.patchFlag,l=r.dynamicPropNames,f=r.needsPatch,p=r.isBlockRequired;let i=r.directives;s=i&&i.length?sq(i.map(e=>(function(e,t){let n=[],r=am.get(e);r?n.push(t.helperString(r)):(t.helper(sv),t.directives.add(e.name),n.push(o_(e.name,"directive")));let{loc:i}=e;if(e.exp&&n.push(e.exp),e.arg&&(e.exp||n.push("void 0"),n.push(e.arg)),Object.keys(e.modifiers).length){e.arg||(e.exp||n.push("void 0"),n.push("void 0"));let t=sz("true",!1,i);n.push(sW(e.modifiers.map(e=>sK(e,t)),i))}return sq(n,e.loc)})(e,t))):void 0,r.shouldUseBlock&&(g=!0)}if(e.children.length>0)if(u===sl&&(g=!0,h|=1024),c&&u!==sr&&u!==sl){let{slots:n,hasDynamicSlots:i}=function(e,t,n=(e,t,n,r)=>sX(e,n,!1,!0,n.length?n[0].loc:r)){t.helper(sL);let{children:r,loc:i}=e,l=[],s=[],o=t.scopes.vSlot>0||t.scopes.vFor>0,a=oa(e,"slot",!0);if(a){let{arg:e,exp:t}=a;e&&!s7(e)&&(o=!0),l.push(sK(e||sz("default",!0),n(t,void 0,r,i)))}let c=!1,u=!1,d=[],h=new Set,f=0;for(let e=0;e<r.length;e++){let i,p,g,m,y=r[e];if(!op(y)||!(i=oa(y,"slot",!0))){3!==y.type&&d.push(y);continue}if(a){t.onError(s9(37,i.loc));break}c=!0;let{children:b,loc:_}=y,{arg:S=sz("default",!0),exp:x,loc:k}=i;s7(S)?p=S?S.content:"default":o=!0;let C=oa(y,"for"),T=n(x,C,b,_);if(g=oa(y,"if"))o=!0,s.push(sQ(g.exp,ag(S,T,f++),af));else if(m=oa(y,/^else(?:-if)?$/,!0)){let n,i=e;for(;i--&&oC(n=r[i]););if(n&&op(n)&&oa(n,/^(?:else-)?if$/)){let e=s[s.length-1];for(;19===e.alternate.type;)e=e.alternate;e.alternate=m.exp?sQ(m.exp,ag(S,T,f++),af):ag(S,T,f++)}else t.onError(s9(30,m.loc))}else if(C){o=!0;let e=C.forParseResult;e?(ad(e),s.push(sG(t.helper(s_),[e.source,sX(ah(e),ag(S,T),!0)]))):t.onError(s9(32,C.loc))}else{if(p){if(h.has(p)){t.onError(s9(38,k));continue}h.add(p),"default"===p&&(u=!0)}l.push(sK(S,T))}}if(!a){let e=(e,t)=>sK("default",n(e,void 0,t,i));c?d.length&&!d.every(ok)&&(u?t.onError(s9(39,d[0].loc)):l.push(e(void 0,d))):l.push(e(void 0,r))}let p=o?2:!function e(t){for(let n=0;n<t.length;n++){let r=t[n];switch(r.type){case 1:if(2===r.tagType||e(r.children))return!0;break;case 9:if(e(r.branches))return!0;break;case 10:case 11:if(e(r.children))return!0}}return!1}(e.children)?1:3,g=sW(l.concat(sK("_",sz(p+"",!1))),i);return s.length&&(g=sG(t.helper(sx),[g,sq(s)])),{slots:g,hasDynamicSlots:o}}(e,t);r=n,i&&(h|=1024)}else if(1===e.children.length&&u!==sr){let n=e.children[0],i=n.type,l=5===i||8===i;l&&0===o2(n,t)&&(h|=1),r=l||2===i?n:e.children}else r=e.children;l&&l.length&&(i=function(e){let t="[";for(let n=0,r=e.length;n<r;n++)t+=JSON.stringify(e[n]),n<r-1&&(t+=", ");return t+"]"}(l));let m=e.codegenNode=sH(t,u,n,r,0===h?void 0:h,i,s,!!g,!1,c,e.loc);(f=f&&(0===h||32===h))&&(m.needsPatch=!0),p&&(m.isBlockRequired=!0)};function ay(e,t,n=e.props,r,i,l=!1){let s,{tag:o,loc:a,children:c}=e,u=[],d=[],h=[],f=c.length>0,p=!1,g=!1,m=0,y=!1,b=!1,_=!1,S=!1,x=!1,C=!1,T=[],w=e=>{u.length&&(d.push(sW(ab(u),a)),u=[]),e&&d.push(e)},A=()=>{t.scopes.vFor>0&&u.push(sK(sz("ref_for",!0),sz("true")))},N=({key:e,value:n})=>{if(s7(e)){let l=e.content,s=k(l);s&&(!r||i)&&"onclick"!==l.toLowerCase()&&"onUpdate:modelValue"!==l&&!$(l)&&(S=!0),s&&$(l)&&(C=!0),"ref"===l&&(y=!0),s&&14===n.type&&(n=n.arguments[0]),20===n.type||(4===n.type||8===n.type)&&o2(n,t)>0||("class"===l?b=!0:"style"===l?_=!0:"ref"===l||"key"===l||T.includes(l)||T.push(l),r&&("class"===l||"style"===l)&&!T.includes(l)&&T.push(l))}else x=!0};for(let i=0;i<n.length;i++){let s=n[i];if(6===s.type){let{loc:e,name:t,nameLoc:n,value:r}=s;if("ref"===t&&(y=!0,A()),"is"===t&&(a_(o)||r&&r.content.startsWith("vue:")))continue;u.push(sK(sz(t,!0,n),sz(r?r.content:"",!0,r?r.loc:e)))}else{let{name:n,arg:i,exp:c,loc:y,modifiers:b}=s,_="bind"===n,S="on"===n;if("slot"===n){r||t.onError(s9(40,y));continue}if("once"===n||"memo"===n||"is"===n||_&&ou(i,"is")&&a_(o)||S&&l)continue;if(_&&ou(i,"key")&&(p=!0),S&&f&&i&&s7(i)&&"vue:beforeUpdate"===B(i.content)&&(p=!0,g=!0),_&&ou(i,"ref")&&A(),!i&&(_||S)){x=!0,c?_?(A(),w(),d.push(c)):w({type:14,loc:y,callee:t.helper(sE),arguments:r?[c]:[c,"true"]}):t.onError(s9(_?34:35,y));continue}_&&b.some(e=>"prop"===e.content)&&(m|=32);let k=t.directiveTransforms[n];if(k){let{props:n,needRuntime:r}=k(s,e,t);l||n.forEach(N),S&&i&&!s7(i)?w(sW(n,a)):u.push(...n),r&&(h.push(s),O(r)&&am.set(s,r))}else!D(n)&&(h.push(s),f&&(p=!0,g=!0))}}d.length?(w(),s=d.length>1?sG(t.helper(sC),d,a):d[0]):u.length&&(s=sW(ab(u),a)),x?m|=16:(b&&!r&&(m|=2),_&&!r&&(m|=4),T.length&&(m|=8),S&&(m|=32));let E=(0===m||32===m)&&(y||C||h.length>0);if(!p&&E&&(m|=512),!t.inSSR&&s)switch(s.type){case 15:let I=-1,R=-1,M=!1;for(let e=0;e<s.properties.length;e++){let t=s.properties[e].key;s7(t)?"class"===t.content?I=e:"style"===t.content&&(R=e):t.isHandlerKey||(M=!0)}let P=s.properties[I],F=s.properties[R];M?s=sG(t.helper(sA),[s]):(P&&!s7(P.value)&&(P.value=sG(t.helper(sT),[P.value])),F&&(_||4===F.value.type&&"["===F.value.content.trim()[0]||17===F.value.type)&&(F.value=sG(t.helper(sw),[F.value])));break;case 14:break;default:s=sG(t.helper(sA),[sG(t.helper(sN),[s])])}return{props:s,directives:h,patchFlag:m,dynamicPropNames:T,shouldUseBlock:p,needsPatch:E,isBlockRequired:g}}function ab(e){let t=new Map,n=[];for(let l=0;l<e.length;l++){var r,i;let s=e[l];if(8===s.key.type||!s.key.isStatic){n.push(s);continue}let o=s.key.content,a=t.get(o);a?("style"===o||"class"===o||k(o))&&(r=a,i=s,17===r.value.type?r.value.elements.push(i.value):r.value=sq([r.value,i.value],r.loc)):(t.set(o,s),n.push(s))}return n}function a_(e){return"component"===e||"Component"===e}let aS=(e,t)=>{if(og(e)){let{children:n,loc:r}=e,{slotName:i,slotProps:l}=function(e,t){let n,r='"default"',i=[];for(let t=0;t<e.props.length;t++){let n=e.props[t];if(6===n.type)n.value&&("name"===n.name?r=JSON.stringify(n.value.content):(n.name=B(n.name),i.push(n)));else if("bind"===n.name&&ou(n.arg,"name")){if(n.exp)r=n.exp;else if(n.arg&&4===n.arg.type){let e=B(n.arg.content);r=n.exp=sz(e,!1,n.arg.loc)}}else"bind"===n.name&&n.arg&&s7(n.arg)&&(n.arg.content=B(n.arg.content)),i.push(n)}if(i.length>0){let{props:r,directives:l}=ay(e,t,i,!1,!1);n=r,l.length&&t.onError(s9(36,l[0].loc))}return{slotName:r,slotProps:n}}(e,t),s=[t.prefixIdentifiers?"_ctx.$slots":"$slots",i,"{}","undefined","true"],o=2;l&&(s[2]=l,o=3),n.length&&(s[3]=sX([],n,!1,!1,r),o=4),t.scopeId&&!t.slotted&&(o=5),s.splice(o),e.codegenNode=sG(t.helper(sS),s,r)}},ax=(e,t,n,r)=>{let i,{loc:l,modifiers:s,arg:o}=e;if(!e.exp&&!s.length,4===o.type)if(o.isStatic){let e=o.content;e.startsWith("vue:")&&(e=`vnode-${e.slice(4)}`),i=sz(0!==t.tagType||e.startsWith("vnode")||!/[A-Z]/.test(e)?W(B(e)):`on:${e}`,!0,o.loc)}else i=sJ([`${n.helperString(sO)}(`,o,")"]);else(i=o).children.unshift(`${n.helperString(sO)}(`),i.children.push(")");let a=e.exp;a&&!a.content.trim()&&(a=void 0);let c=n.cacheHandlers&&!a&&!n.inVOnce;if(a){let e,t=os(a),n=!(t||(e=a,oo.test(ol(e)))),r=a.content.includes(";");(n||c&&t)&&(a=sJ([`${n?"$event":"(...args)"} => ${r?"{":"("}`,a,r?"}":")"]))}let u={props:[sK(i,a||sz("() => {}",!1,l))]};return r&&(u=r(u)),c&&(u.props[0].value=n.cache(u.props[0].value)),u.props.forEach(e=>e.key.isHandlerKey=!0),u},ak=(e,t,n)=>{let{modifiers:r}=e,i=e.arg,{exp:l}=e;return l&&4===l.type&&!l.content.trim()&&(l=void 0),4!==i.type?(i.children.unshift("("),i.children.push(') || ""')):i.isStatic||(i.content=i.content?`${i.content} || ""`:'""'),r.some(e=>"camel"===e.content)&&(4===i.type?i.isStatic?i.content=B(i.content):i.content=`${n.helperString(sI)}(${i.content})`:(i.children.unshift(`${n.helperString(sI)}(`),i.children.push(")"))),!n.inSSR&&(r.some(e=>"prop"===e.content)&&aC(i,"."),r.some(e=>"attr"===e.content)&&aC(i,"^")),{props:[sK(i,l)]}},aC=(e,t)=>{4===e.type?e.isStatic?e.content=t+e.content:e.content=`\`${t}\${${e.content}}\``:(e.children.unshift(`'${t}' + (`),e.children.push(")"))},aT=(e,t)=>{if(0===e.type||1===e.type||11===e.type||10===e.type)return()=>{let n,r=e.children,i=!1;for(let e=0;e<r.length;e++){let t=r[e];if(od(t)){i=!0;for(let i=e+1;i<r.length;i++){let l=r[i];if(od(l))n||(n=r[e]=sJ([t],t.loc)),n.children.push(" + ",l),r.splice(i,1),i--;else{n=void 0;break}}}}if(i&&(1!==r.length||0!==e.type&&(1!==e.type||0!==e.tagType||e.props.find(e=>7===e.type&&!t.directiveTransforms[e.name]))))for(let e=0;e<r.length;e++){let n=r[e];if(od(n)||8===n.type){let i=[];(2!==n.type||" "!==n.content)&&i.push(n),t.ssr||0!==o2(n,t)||i.push("1"),r[e]={type:12,content:n,loc:n.loc,codegenNode:sG(t.helper(sf),i)}}}}},aw=new WeakSet,aA=(e,t)=>{if(1===e.type&&oa(e,"once",!0)&&!aw.has(e)&&!t.inVOnce&&!t.inSSR)return aw.add(e),t.inVOnce=!0,t.helper(sM),()=>{t.inVOnce=!1;let e=t.currentNode;e.codegenNode&&(e.codegenNode=t.cache(e.codegenNode,!0,!0))}},aN=(e,t,n)=>{let r,{exp:i,arg:l}=e;if(!i)return n.onError(s9(41,e.loc)),aE();let s=i.loc.source.trim(),o=4===i.type?i.content:s,a=n.bindingMetadata[s];if("props"===a||"props-aliased"===a||"literal-const"===a||"setup-const"===a)return i.loc,aE();if(!o.trim()||!os(i))return n.onError(s9(42,i.loc)),aE();let c=l||sz("modelValue",!0),u=l?s7(l)?`onUpdate:${B(l.content)}`:sJ(['"onUpdate:" + ',l]):"onUpdate:modelValue",d=n.isTS?"($event: any)":"$event";r=sJ([`${d} => ((`,i,") = $event)"]);let h=[sK(c,e.exp),sK(u,r)];if(e.modifiers.length&&1===t.tagType){let t=e.modifiers.map(e=>e.content).map(e=>(ot.test(e)?JSON.stringify(e):e)+": true").join(", "),n=l?s7(l)?`${l.content}Modifiers`:sJ([l,' + "Modifiers"']):"modelModifiers";h.push(sK(n,sz(`{ ${t} }`,!1,e.loc,2)))}return aE(h)};function aE(e=[]){return{props:e}}let aI=new WeakSet,aR=(e,t)=>{if(1===e.type){let n=oa(e,"memo");if(!(!n||aI.has(e))&&!t.inSSR)return aI.add(e),()=>{let r=e.codegenNode||t.currentNode.codegenNode;r&&13===r.type&&(1!==e.tagType&&sZ(r,t),e.codegenNode=sG(t.helper(sV),[n.exp,sX(void 0,r),"_cache",String(t.cached.length)]),t.cached.push(null))}}},aO=(e,t)=>{if(1===e.type){for(let n of e.props)if(7===n.type&&"bind"===n.name&&(!n.exp||4===n.exp.type&&!n.exp.content.trim())&&n.arg){let e=n.arg;if(4===e.type&&e.isStatic){let t=B(e.content);(on.test(t[0])||"-"===t[0])&&(n.exp=sz(t,!1,e.loc))}else t.onError(s9(53,e.loc)),n.exp=sz("",!0,e.loc)}}},aM=Symbol(""),aP=Symbol(""),aF=Symbol(""),aL=Symbol(""),a$=Symbol(""),aD=Symbol(""),aV=Symbol(""),aj=Symbol(""),aB=Symbol(""),aU=Symbol("");Object.getOwnPropertySymbols(r={[aM]:"vModelRadio",[aP]:"vModelCheckbox",[aF]:"vModelText",[aL]:"vModelSelect",[a$]:"vModelDynamic",[aD]:"withModifiers",[aV]:"withKeys",[aj]:"vShow",[aB]:"Transition",[aU]:"TransitionGroup"}).forEach(e=>{sB[e]=r[e]});let aH={parseMode:"html",isVoidTag:ea,isNativeTag:e=>el(e)||es(e)||eo(e),isPreTag:e=>"pre"===e,isIgnoreNewlineTag:e=>"pre"===e||"textarea"===e,decodeEntities:function(e,t=!1){return(p||(p=document.createElement("div")),t)?(p.innerHTML=`<div foo="${e.replace(/"/g,"&quot;")}">`,p.children[0].getAttribute("foo")):(p.innerHTML=e,p.textContent)},isBuiltInComponent:e=>"Transition"===e||"transition"===e?aB:"TransitionGroup"===e||"transition-group"===e?aU:void 0,getNamespace(e,t,n){let r=t?t.ns:n;if(t&&2===r)if("annotation-xml"===t.tag){if("svg"===e)return 1;t.props.some(e=>6===e.type&&"encoding"===e.name&&null!=e.value&&("text/html"===e.value.content||"application/xhtml+xml"===e.value.content))&&(r=0)}else/^m(?:[ions]|text)$/.test(t.tag)&&"mglyph"!==e&&"malignmark"!==e&&(r=0);else t&&1===r&&("foreignObject"===t.tag||"desc"===t.tag||"title"===t.tag)&&(r=0);if(0===r){if("svg"===e)return 1;if("math"===e)return 2}return r}},aq=y("passive,once,capture"),aW=y("stop,prevent,self,ctrl,shift,alt,meta,exact,middle"),aK=y("left,right"),az=y("onkeyup,onkeydown,onkeypress"),aJ=(e,t)=>s7(e)&&"onclick"===e.content.toLowerCase()?sz(t,!0):4!==e.type?sJ(["(",e,`) === "onClick" ? "${t}" : (`,e,")"]):e,aG=(e,t)=>{1===e.type&&0===e.tagType&&("script"===e.tag||"style"===e.tag)&&t.removeNode()},aX=[e=>{1===e.type&&e.props.forEach((t,n)=>{let r,i;6===t.type&&"style"===t.name&&t.value&&(e.props[n]={type:7,name:"bind",arg:sz("style",!0,t.loc),exp:(r=t.value.content,i=t.loc,sz(JSON.stringify(er(r)),!1,i,3)),modifiers:[],loc:t.loc})})}],aQ={cloak:()=>({props:[]}),html:(e,t,n)=>{let{exp:r,loc:i}=e;return r||n.onError(s9(54,i)),t.children.length&&(n.onError(s9(55,i)),t.children.length=0),{props:[sK(sz("innerHTML",!0,i),r||sz("",!0))]}},text:(e,t,n)=>{let{exp:r,loc:i}=e;return r||n.onError(s9(56,i)),t.children.length&&(n.onError(s9(57,i)),t.children.length=0),{props:[sK(sz("textContent",!0),r?o2(r,n)>0?r:sG(n.helperString(sk),[r],i):sz("",!0))]}},model:(e,t,n)=>{let r=aN(e,t,n);if(!r.props.length||1===t.tagType)return r;e.arg&&n.onError(s9(59,e.arg.loc));let{tag:i}=t,l=n.isCustomElement(i);if("input"===i||"textarea"===i||"select"===i||l){let s=aF,o=!1;if("input"===i||l){let r=oc(t,"type");if(r){if(7===r.type)s=a$;else if(r.value)switch(r.value.content){case"radio":s=aM;break;case"checkbox":s=aP;break;case"file":o=!0,n.onError(s9(60,e.loc))}}else t.props.some(e=>7===e.type&&"bind"===e.name&&(!e.arg||4!==e.arg.type||!e.arg.isStatic))&&(s=a$)}else"select"===i&&(s=aL);o||(r.needRuntime=n.helper(s))}else n.onError(s9(58,e.loc));return r.props=r.props.filter(e=>4!==e.key.type||"modelValue"!==e.key.content),r},on:(e,t,n)=>ax(e,t,n,t=>{let{modifiers:r}=e;if(!r.length)return t;let{key:i,value:l}=t.props[0],{keyModifiers:s,nonKeyModifiers:o,eventOptionModifiers:a}=((e,t,n)=>{let r=[],i=[],l=[];for(let n=0;n<t.length;n++){let s=t[n].content;aq(s)?l.push(s):aK(s)?s7(e)?az(e.content.toLowerCase())?r.push(s):i.push(s):(r.push(s),i.push(s)):aW(s)?i.push(s):r.push(s)}return{keyModifiers:r,nonKeyModifiers:i,eventOptionModifiers:l}})(i,r,e.loc);if(o.includes("right")&&(i=aJ(i,"onContextmenu")),o.includes("middle")&&(i=aJ(i,"onMouseup")),o.length&&(l=sG(n.helper(aD),[l,JSON.stringify(o)])),s.length&&(!s7(i)||az(i.content.toLowerCase()))&&(l=sG(n.helper(aV),[l,JSON.stringify(s)])),a.length){let e=a.map(q).join("");i=s7(i)?sz(`${i.content}${e}`,!0):sJ(["(",i,`) + "${e}"`])}return{props:[sK(i,l)]}}),show:(e,t,n)=>{let{exp:r,loc:i}=e;return r||n.onError(s9(62,i)),{props:[],needRuntime:n.helper(aj)}}},aZ=Object.create(null);function aY(e,t){if(!R(e))if(!e.nodeType)return S;else e=e.innerHTML;let n=e+JSON.stringify(t,(e,t)=>"function"==typeof t?t.toString():t),r=aZ[n];if(r)return r;if("#"===e[0]){let t=document.querySelector(e);e=t?t.innerHTML:""}let i=T({hoistStatic:!0,onError:void 0,onWarn:S},t);!i.isCustomElement&&"u">typeof customElements&&(i.isCustomElement=e=>!!customElements.get(e));let{code:l}=function(e,t={}){return function(e,t={}){var n;let r,i=t.onError||s8,l="module"===t.mode;!0===t.prefixIdentifiers?i(s9(48)):l&&i(s9(49)),t.cacheHandlers&&i(s9(50)),t.scopeId&&!l&&i(s9(51));let s=T({},t,{prefixIdentifiers:!1}),o=R(e)?function(e,t){if(oD.reset(),oE=null,oI=null,oR="",oO=-1,oM=-1,o$.length=0,oN=e,ow=T({},oT),t){let e;for(e in t)null!=t[e]&&(ow[e]=t[e])}oD.mode="html"===ow.parseMode?1:2*("sfc"===ow.parseMode),oD.inXML=1===ow.ns||2===ow.ns;let n=t&&t.delimiters;n&&(oD.delimiterOpen=s6(n[0]),oD.delimiterClose=s6(n[1]));let r=oA=function(e,t=""){return{type:0,source:t,children:e,helpers:new Set,components:[],directives:[],hoists:[],imports:[],cached:[],temps:0,codegenNode:void 0,loc:sU}}([],e);return oD.parse(oN),r.loc=oQ(0,e.length),r.children=oJ(r.children),oA=null,r}(e,s):e,[a,c]=[[aO,aA,as,aR,au,aS,av,ap,aT],{on:ax,bind:ak,model:aN}];return r=function(e,{filename:t="",prefixIdentifiers:n=!1,hoistStatic:r=!1,hmr:i=!1,cacheHandlers:l=!1,nodeTransforms:s=[],directiveTransforms:o={},transformHoist:a=null,isBuiltInComponent:c=S,isCustomElement:u=S,expressionPlugins:d=[],scopeId:h=null,slotted:f=!0,ssr:p=!1,inSSR:g=!1,ssrCssVars:m="",bindingMetadata:y=b,inline:_=!1,isTS:x=!1,onError:k=s8,onWarn:C=s5,compatConfig:T}){let w=t.replace(/\?.*$/,"").match(/([^/\\]+)\.\w+$/),A={filename:t,selfName:w&&q(B(w[1])),prefixIdentifiers:n,hoistStatic:r,hmr:i,cacheHandlers:l,nodeTransforms:s,directiveTransforms:o,transformHoist:a,isBuiltInComponent:c,isCustomElement:u,expressionPlugins:d,scopeId:h,slotted:f,ssr:p,inSSR:g,ssrCssVars:m,bindingMetadata:y,inline:_,isTS:x,onError:k,onWarn:C,compatConfig:T,root:e,helpers:new Map,components:new Set,directives:new Set,hoists:[],imports:[],cached:[],constantCache:new WeakMap,vForMemoKeyedNodes:new WeakSet,temps:0,identifiers:Object.create(null),scopes:{vFor:0,vSlot:0,vPre:0,vOnce:0},parent:null,grandParent:null,currentNode:e,childIndex:0,inVOnce:!1,helper(e){let t=A.helpers.get(e)||0;return A.helpers.set(e,t+1),e},removeHelper(e){let t=A.helpers.get(e);if(t){let n=t-1;n?A.helpers.set(e,n):A.helpers.delete(e)}},helperString:e=>`_${sB[A.helper(e)]}`,replaceNode(e){A.parent.children[A.childIndex]=A.currentNode=e},removeNode(e){let t=A.parent.children,n=e?t.indexOf(e):A.currentNode?A.childIndex:-1;e&&e!==A.currentNode?A.childIndex>n&&(A.childIndex--,A.onNodeRemoved()):(A.currentNode=null,A.onNodeRemoved()),A.parent.children.splice(n,1)},onNodeRemoved:S,addIdentifiers(e){},removeIdentifiers(e){},hoist(e){R(e)&&(e=sz(e)),A.hoists.push(e);let t=sz(`_hoisted_${A.hoists.length}`,!1,e.loc,2);return t.hoisted=e,t},cache(e,t=!1,n=!1){let r=function(e,t,n=!1,r=!1){return{type:20,index:e,value:t,needPauseTracking:n,inVOnce:r,needArraySpread:!1,loc:sU}}(A.cached.length,e,t,n);return A.cached.push(r),r}};return A}(o,n=T({},s,{nodeTransforms:[...a,...t.nodeTransforms||[]],directiveTransforms:T({},c,t.directiveTransforms||{})})),o8(o,r),n.hoistStatic&&function e(t,n,r,i=!1,l=!1){let{children:s}=t,o=[];for(let n=0;n<s.length;n++){let a=s[n];if(1===a.type&&0===a.tagType){let e=i?0:o2(a,r);if(e>0){if(e>=2){a.codegenNode.patchFlag=-1,o.push(a);continue}}else{let e=a.codegenNode;if(13===e.type){let t=e.patchFlag;if((void 0===t||512===t||1===t)&&o6(a,r)>=2){let t=o4(a);t&&(e.props=r.hoist(t))}e.dynamicProps&&(e.dynamicProps=r.hoist(e.dynamicProps))}}}else if(12===a.type&&(i?0:o2(a,r))>=2){14===a.codegenNode.type&&a.codegenNode.arguments.length>0&&a.codegenNode.arguments.push("-1"),o.push(a);continue}if(1===a.type){let n=1===a.tagType;n&&r.scopes.vSlot++,e(a,t,r,!1,l),n&&r.scopes.vSlot--}else if(11===a.type)e(a,t,r,1===a.children.length,!0);else if(9===a.type)for(let n=0;n<a.branches.length;n++)e(a.branches[n],t,r,1===a.branches[n].children.length,l)}let a=!1;if(o.length===s.length&&1===t.type){if(0===t.tagType&&t.codegenNode&&13===t.codegenNode.type&&E(t.codegenNode.children))t.codegenNode.children=c(sq(t.codegenNode.children)),a=!0;else if(1===t.tagType&&t.codegenNode&&13===t.codegenNode.type&&t.codegenNode.children&&!E(t.codegenNode.children)&&15===t.codegenNode.children.type){let e=u(t.codegenNode,"default");e&&(e.returns=c(sq(e.returns)),a=!0)}else if(3===t.tagType&&n&&1===n.type&&1===n.tagType&&n.codegenNode&&13===n.codegenNode.type&&n.codegenNode.children&&!E(n.codegenNode.children)&&15===n.codegenNode.children.type){let e=oa(t,"slot",!0),r=e&&e.arg&&u(n.codegenNode,e.arg);r&&(r.returns=c(sq(r.returns)),a=!0)}}if(!a)for(let e of o)e.codegenNode=r.cache(e.codegenNode);function c(e){let t=r.cache(e);return t.needArraySpread=!0,t}function u(e,t){if(e.children&&!E(e.children)&&15===e.children.type){let n=e.children.properties.find(e=>e.key===t||e.key.content===t);return n&&n.value}}o.length&&r.transformHoist&&r.transformHoist(s,r,t)}(o,void 0,r,!!o1(o)),n.ssr||function(e,t){let{helper:n}=t,{children:r}=e;if(1===r.length){let n=o1(e);if(n&&n.codegenNode){let r=n.codegenNode;13===r.type&&sZ(r,t),e.codegenNode=r}else e.codegenNode=r[0]}else r.length>1&&(e.codegenNode=sH(t,n(sn),void 0,e.children,64,void 0,void 0,!0,void 0,!1))}(o,r),o.helpers=new Set([...r.helpers.keys()]),o.components=[...r.components],o.directives=[...r.directives],o.imports=r.imports,o.hoists=r.hoists,o.temps=r.temps,o.cached=r.cached,o.transformed=!0,function(e,t={}){let n=function(e,{mode:t="function",prefixIdentifiers:n="module"===t,sourceMap:r=!1,filename:i="template.vue.html",scopeId:l=null,optimizeImports:s=!1,runtimeGlobalName:o="Vue",runtimeModuleName:a="vue",ssrRuntimeModuleName:c="vue/server-renderer",ssr:u=!1,isTS:d=!1,inSSR:h=!1}){let f={mode:t,prefixIdentifiers:n,sourceMap:r,filename:i,scopeId:l,optimizeImports:s,runtimeGlobalName:o,runtimeModuleName:a,ssrRuntimeModuleName:c,ssr:u,isTS:d,inSSR:h,source:e.source,code:"",column:1,line:1,offset:0,indentLevel:0,pure:!1,map:void 0,helper:e=>`_${sB[e]}`,push(e,t=-2,n){f.code+=e},indent(){p(++f.indentLevel)},deindent(e=!1){e?--f.indentLevel:p(--f.indentLevel)},newline(){p(f.indentLevel)}};function p(e){f.push(`
`+"  ".repeat(e),0)}return f}(e,t);t.onContextCreated&&t.onContextCreated(n);let{mode:r,push:i,prefixIdentifiers:l,indent:s,deindent:o,newline:a,ssr:c}=n,u=Array.from(e.helpers),d=u.length>0,h=!l&&"module"!==r;!function(e,t){let{push:n,newline:r,runtimeGlobalName:i}=t,l=Array.from(e.helpers);if(l.length>0&&(n(`const _Vue = ${i}
`,-1),e.hoists.length)){let e=[su,sd,sh,sf,sp].filter(e=>l.includes(e)).map(o7).join(", ");n(`const { ${e} } = _Vue
`,-1)}(function(e,t){if(!e.length)return;t.pure=!0;let{push:n,newline:r}=t;r();for(let i=0;i<e.length;i++){let l=e[i];l&&(n(`const _hoisted_${i+1} = `),ar(l,t),r())}t.pure=!1})(e.hoists,t),r(),n("return ")}(e,n);let f=(c?["_ctx","_push","_parent","_attrs"]:["_ctx","_cache"]).join(", ");if(i(`function ${c?"ssrRender":"render"}(${f}) {`),s(),h&&(i("with (_ctx) {"),s(),d&&(i(`const { ${u.map(o7).join(", ")} } = _Vue
`,-1),a())),e.components.length&&(ae(e.components,"component",n),(e.directives.length||e.temps>0)&&a()),e.directives.length&&(ae(e.directives,"directive",n),e.temps>0&&a()),e.temps>0){i("let ");for(let t=0;t<e.temps;t++)i(`${t>0?", ":""}_temp${t}`)}return(e.components.length||e.directives.length||e.temps)&&(i(`
`,0),a()),c||i("return "),e.codegenNode?ar(e.codegenNode,n):i("null"),h&&(o(),i("}")),o(),i("}"),{ast:e,code:n.code,preamble:"",map:n.map?n.map.toJSON():void 0}}(o,s)}(e,T({},aH,t,{nodeTransforms:[aG,...aX,...t.nodeTransforms||[]],directiveTransforms:T({},aQ,t.directiveTransforms||{}),transformHoist:null}))}(e,i),s=Function(l)();return s._rc=!0,aZ[n]=s}return iL(aY),e.BaseTransition=nb,e.BaseTransitionPropsValidators=nm,e.Comment=ie,e.DeprecationTypes=null,e.EffectScope=ev,e.ErrorCodes={SETUP_FUNCTION:0,0:"SETUP_FUNCTION",RENDER_FUNCTION:1,1:"RENDER_FUNCTION",NATIVE_EVENT_HANDLER:5,5:"NATIVE_EVENT_HANDLER",COMPONENT_EVENT_HANDLER:6,6:"COMPONENT_EVENT_HANDLER",VNODE_HOOK:7,7:"VNODE_HOOK",DIRECTIVE_HOOK:8,8:"DIRECTIVE_HOOK",TRANSITION_HOOK:9,9:"TRANSITION_HOOK",APP_ERROR_HANDLER:10,10:"APP_ERROR_HANDLER",APP_WARN_HANDLER:11,11:"APP_WARN_HANDLER",FUNCTION_REF:12,12:"FUNCTION_REF",ASYNC_COMPONENT_LOADER:13,13:"ASYNC_COMPONENT_LOADER",SCHEDULER:14,14:"SCHEDULER",COMPONENT_UPDATE:15,15:"COMPONENT_UPDATE",APP_UNMOUNT_CLEANUP:16,16:"APP_UNMOUNT_CLEANUP"},e.ErrorTypeStrings=null,e.Fragment=r9,e.KeepAlive={name:"KeepAlive",__isKeepAlive:!0,props:{include:[String,RegExp,Array],exclude:[String,RegExp,Array],max:[String,Number]},setup(e,{slots:t}){let n=iI(),r=n.ctx,i=new Map,l=new Set,s=null,o=n.suspense,{renderer:{p:a,m:c,um:u,o:{createElement:d}}}=r,h=d("div");function f(e){nQ(e),u(e,n,o,!0)}function p(e){i.forEach((t,n)=>{let r=iB(nq(t)?t.type.__asyncResolved||{}:t.type);r&&!e(r)&&g(n)})}function g(e){let t=i.get(e);!t||s&&ih(t,s)?s&&nQ(s):f(t),i.delete(e),l.delete(e)}r.activate=(e,t,n,r,i)=>{let l=e.component;c(e,t,n,0,o),a(l.vnode,e,t,n,l,o,r,e.slotScopeIds,i),rz(()=>{l.isDeactivated=!1,l.a&&z(l.a);let t=e.props&&e.props.onVnodeMounted;t&&iw(t,l.parent,e)},o)},r.deactivate=e=>{let t=e.component;r0(t.m),r0(t.a),c(e,h,null,1,o),rz(()=>{t.da&&z(t.da);let n=e.props&&e.props.onVnodeUnmounted;n&&iw(n,t.parent,e),t.isDeactivated=!0},o)},ne(()=>[e.include,e.exclude],([e,t])=>{e&&p(t=>nz(e,t)),t&&p(e=>!nz(t,e))},{flush:"post",deep:!0});let m=null,y=()=>{null!=m&&(r1(n.subTree.type)?rz(()=>{let e=nZ(n.subTree);e.component&&i.set(m,e)},n.subTree.suspense):i.set(m,nZ(n.subTree)))};return n2(y),n6(y),n4(()=>{i.forEach(e=>{let{subTree:t,suspense:r}=n,i=nZ(t);if(e.type===i.type&&e.key===i.key){nQ(i);let e=i.component.da;e&&rz(e,r);return}f(e)})}),()=>{if(m=null,!t.default)return s=null;let n=t.default(),r=n[0];if(n.length>1)return s=null,n;if(!id(r)||!(4&r.shapeFlag)&&!(128&r.shapeFlag))return s=null,r;let o=nZ(r);if(o.type===ie)return s=null,o;let a=o.type,c=iB(nq(o)?o.type.__asyncResolved||{}:a),{include:u,exclude:d,max:h}=e;if(u&&(!c||!nz(u,c))||d&&c&&nz(d,c))return o.shapeFlag&=-257,s=o,r;let f=null==o.key?a:o.key,p=i.get(f);return o.el&&(o=ib(o),128&r.shapeFlag&&(r.ssContent=o)),m=f,p?(o.el=p.el,o.component=p.component,o.transition&&nC(o,o.transition),o.shapeFlag|=512,l.delete(f),l.add(f)):(l.add(f),h&&l.size>parseInt(h,10)&&g(l.values().next().value)),o.shapeFlag|=256,s=o,r1(r.type)?r:o}}},e.ReactiveEffect=eb,e.Static=it,e.Suspense={name:"Suspense",__isSuspense:!0,process(e,t,n,r,i,l,s,o,a,c){if(null==e)!function(e,t,n,r,i,l,s,o,a){let{p:c,o:{createElement:u}}=a,d=u("div"),h=e.suspense=r6(e,i,r,t,d,n,l,s,o,a);c(null,h.pendingBranch=e.ssContent,d,null,r,h,l,s),h.deps>0?(r3(e,"onPending"),r3(e,"onFallback"),c(null,e.ssFallback,t,n,r,null,l,s),r5(h,e.ssFallback)):h.resolve(!1,!0)}(t,n,r,i,l,s,o,a,c);else{if(l&&l.deps>0&&!e.suspense.isInFallback){t.suspense=e.suspense,t.suspense.vnode=t,t.el=e.el;return}!function(e,t,n,r,i,l,s,o,{p:a,um:c,o:{createElement:u}}){let d=t.suspense=e.suspense;d.vnode=t,t.el=e.el;let h=t.ssContent,f=t.ssFallback,{activeBranch:p,pendingBranch:g,isInFallback:m,isHydrating:y}=d;if(g)d.pendingBranch=h,ih(g,h)?(a(g,h,d.hiddenContainer,null,i,d,l,s,o),d.deps<=0?d.resolve():!m||y||d.isFallbackMountPending||(a(p,f,n,r,i,null,l,s,o),r5(d,f))):(d.pendingId=r2++,y?(d.isHydrating=!1,d.activeBranch=g):c(g,i,d),d.deps=0,d.effects.length=0,d.hiddenContainer=u("div"),m?(a(null,h,d.hiddenContainer,null,i,d,l,s,o),d.deps<=0?d.resolve():d.isFallbackMountPending||(a(p,f,n,r,i,null,l,s,o),r5(d,f))):p&&ih(p,h)?(a(p,h,n,r,i,d,l,s,o),d.resolve(!0)):(a(null,h,d.hiddenContainer,null,i,d,l,s,o),d.deps<=0&&d.resolve()));else if(p&&ih(p,h))a(p,h,n,r,i,d,l,s,o),r5(d,h);else if(r3(t,"onPending"),d.pendingBranch=h,512&h.shapeFlag?d.pendingId=h.component.suspenseId:d.pendingId=r2++,a(null,h,d.hiddenContainer,null,i,d,l,s,o),d.deps<=0)d.resolve();else{let{timeout:e,pendingId:t}=d;e>0?setTimeout(()=>{d.pendingId===t&&d.fallback(f)},e):0===e&&d.fallback(f)}}(e,t,n,r,i,s,o,a,c)}},hydrate:function(e,t,n,r,i,l,s,o,a){let c=t.suspense=r6(t,r,n,e.parentNode,document.createElement("div"),null,i,l,s,o,!0),u=a(e,c.pendingBranch=t.ssContent,n,c,l,s);return 0===c.deps&&c.resolve(!1,!0),u},normalize:function(e){let{shapeFlag:t,children:n}=e,r=32&t;e.ssContent=r4(r?n.default:n),e.ssFallback=r?r4(n.fallback):iv(ie)}},e.Teleport={name:"Teleport",__isTeleport:!0,process(e,t,n,r,i,l,s,o,a,c){let{mc:u,pc:d,pbc:h,o:{insert:f,querySelector:p,createText:g,parentNode:m}}=c,y=nl(t.props),{dynamicChildren:b}=t,_=(e,t,n)=>{16&e.shapeFlag&&u(e.children,t,n,i,l,s,o,a)},S=(e=t)=>{let n=nl(e.props),r=e.target=na(e.props,p),l=nd(r,e,g,f);r&&("svg"!==s&&ns(r)?s="svg":"mathml"!==s&&no(r)&&(s="mathml"),i&&i.isCE&&(i.ce._teleportTargets||(i.ce._teleportTargets=new Set)).add(r),n||(_(e,r,l),nu(e,!1)))},x=e=>{let t=()=>{if(nr.get(e)===t){if(nr.delete(e),nl(e.props)){let t=m(e.el)||n;_(e,t,e.anchor),nu(e,!0)}S(e)}};nr.set(e,t),rz(t,l)};if(null==e){let e,i=t.el=g(""),s=t.anchor=g("");if(f(i,n,r),f(s,n,r),(e=t.props)&&(e.defer||""===e.defer)||l&&l.pendingBranch)return void x(t);y&&(_(t,n,s),nu(t,!0)),S()}else{t.el=e.el;let r=t.anchor=e.anchor,u=nr.get(e);if(u){u.flags|=8,nr.delete(e),x(t);return}t.targetStart=e.targetStart;let f=t.target=e.target,g=t.targetAnchor=e.targetAnchor,m=nl(e.props),_=m?n:f,S=m?r:g;if("svg"===s||ns(f)?s="svg":("mathml"===s||no(f))&&(s="mathml"),b?(h(e.dynamicChildren,b,_,i,l,s,o),rY(e,t,!0)):a||d(e,t,_,S,i,l,s,o,!1),y)m?t.props&&e.props&&t.props.to!==e.props.to&&(t.props.to=e.props.to):nc(t,n,r,c,1);else if((t.props&&t.props.to)!==(e.props&&e.props.to)){let e=na(t.props,p);e&&(t.target=e,nc(t,e,null,c,0))}else m&&nc(t,f,g,c,1);nu(t,y)}},remove(e,t,n,{um:r,o:{remove:i}},l){let{shapeFlag:s,children:o,anchor:a,targetStart:c,targetAnchor:u,target:d,props:h}=e,f=nl(h),p=l||!f,g=nr.get(e);if(g&&(g.flags|=8,nr.delete(e)),d&&(i(c),i(u)),l&&i(a),!g&&(f||d)&&16&s)for(let e=0;e<o.length;e++){let i=o[e];r(i,t,n,p,!!i.dynamicChildren)}},move:nc,hydrate:function(e,t,n,r,i,l,{o:{nextSibling:s,parentNode:o,querySelector:a,insert:c,createText:u}},d){function h(e,n){let r=n;for(;r;){if(r&&8===r.nodeType){if("teleport start anchor"===r.data)t.targetStart=r;else if("teleport anchor"===r.data){t.targetAnchor=r,e._lpa=t.targetAnchor&&s(t.targetAnchor);break}}r=s(r)}}function f(e,t){t.anchor=d(s(e),t,o(e),n,r,i,l)}let p=t.target=na(t.props,a),g=nl(t.props);if(p){let a=p._lpa||p.firstChild;16&t.shapeFlag&&(g?(f(e,t),h(p,a),t.targetAnchor||nd(p,t,u,c,o(e)===p?e:null)):(t.anchor=s(e),h(p,a),t.targetAnchor||nd(p,t,u,c),d(a&&s(a),t,p,n,r,i,l))),nu(t,g)}else g&&16&t.shapeFlag&&(f(e,t),t.targetStart=e,t.targetAnchor=s(e));return t.anchor&&s(t.anchor)}},e.Text=r7,e.TrackOpTypes={GET:"get",HAS:"has",ITERATE:"iterate"},e.Transition=i2,e.TransitionGroup=lF,e.TriggerOpTypes={SET:"set",ADD:"add",DELETE:"delete",CLEAR:"clear"},e.VueElement=lE,e.assertNumber=function(e,t){},e.callWithAsyncErrorHandling=tV,e.callWithErrorHandling=tD,e.camelize=B,e.capitalize=q,e.cloneVNode=ib,e.compatUtils=null,e.compile=aY,e.computed=iU,e.createApp=l9,e.createBlock=iu,e.createCommentVNode=iS,e.createElementBlock=function(e,t,n,r,i,l){return ic(im(e,t,n,r,i,l,!0))},e.createElementVNode=im,e.createHydrationRenderer=rJ,e.createPropsRestProxy=function(e,t){let n={};for(let r in e)t.includes(r)||Object.defineProperty(n,r,{enumerable:!0,get:()=>e[r]});return n},e.createRenderer=function(e){return rG(e)},e.createSSRApp=l7,e.createSlots=function(e,t){for(let n=0;n<t.length;n++){let r=t[n];if(E(r))for(let t=0;t<r.length;t++)e[r[t].name]=r[t].fn;else r&&(e[r.name]=r.key?(...e)=>{let t=r.fn(...e);return t&&(t.key=r.key),t}:r.fn)}return e},e.createStaticVNode=function(e,t){let n=iv(it,null,e);return n.staticCount=t,n},e.createTextVNode=i_,e.createVNode=iv,e.customRef=tI,e.defineAsyncComponent=function(e){let t;I(e)&&(e={loader:e});let{loader:n,loadingComponent:r,errorComponent:i,delay:l=200,hydrate:s,timeout:o,suspensible:a=!0,onError:c}=e,u=null,d=0,h=()=>{let e;return u||(e=u=n().catch(e=>{if(e=e instanceof Error?e:Error(String(e)),c)return new Promise((t,n)=>{c(e,()=>t((d++,u=null,h())),()=>n(e),d+1)});throw e}).then(n=>e!==u&&u?u:(n&&(n.__esModule||"Module"===n[Symbol.toStringTag])&&(n=n.default),t=n,n)))};return nw({name:"AsyncComponentWrapper",__asyncLoader:h,__asyncHydrate(e,n,r){let i=e.isConnected,l=!1;(n.bu||(n.bu=[])).push(()=>l=!0);let o=()=>{l||e.parentNode&&(!i||e.isConnected)&&r()},a=s?()=>{let t=s(o,t=>(function(e,t){if(nF(e)&&"["===e.data){let n=1,r=e.nextSibling;for(;r;){if(1===r.nodeType){if(!1===t(r))break}else if(nF(r))if("]"===r.data){if(0==--n)break}else"["===r.data&&n++;r=r.nextSibling}}else t(e)})(e,t));t&&(n.bum||(n.bum=[])).push(t)}:o;t?a():h().then(()=>!n.isUnmounted&&a())},get __asyncResolved(){return t},setup(){let e,n,s=iE;if(nA(s),t)return()=>nW(t,s);let c=e=>{u=null,tj(e,s,13,!i)};if(a&&s.suspense)return h().then(e=>()=>nW(e,s)).catch(e=>(c(e),()=>i?iv(i,{error:e}):null));let d=tx(!1),f=tx(),p=tx(!!l);return n8(()=>{null!=e&&clearTimeout(e),null!=n&&clearTimeout(n)}),l&&(n=setTimeout(()=>{s.isUnmounted||(p.value=!1)},l)),null!=o&&(e=setTimeout(()=>{if(!s.isUnmounted&&!d.value&&!f.value){let e=Error(`Async component timed out after ${o}ms.`);c(e),f.value=e}},o)),h().then(()=>{!s.isUnmounted&&(d.value=!0,s.parent&&nK(s.parent.vnode)&&s.parent.update())}).catch(e=>{if(s.isUnmounted){u=null;return}c(e),f.value=e}),()=>d.value&&t?nW(t,s):f.value&&i?iv(i,{error:f.value}):r&&!p.value?nW(r,s):void 0}})},e.defineComponent=nw,e.defineCustomElement=lA,e.defineEmits=function(){return null},e.defineExpose=function(e){},e.defineModel=function(){},e.defineOptions=function(e){},e.defineProps=function(){return null},e.defineSSRCustomElement=(e,t)=>lA(e,t,l7),e.defineSlots=function(){return null},e.devtools=void 0,e.effect=function(e,t){e.effect instanceof eb&&(e=e.effect.fn);let n=new eb(e);t&&T(n,t);try{n.run()}catch(e){throw n.stop(),e}let r=n.run.bind(n);return r.effect=n,r},e.effectScope=function(e){return new ev(e)},e.getCurrentInstance=iI,e.getCurrentScope=function(){return l},e.getCurrentWatcher=function(){return g},e.getTransitionRawChildren=nT,e.guardReactiveProps=iy,e.h=iH,e.handleError=tj,e.hasInjectionContext=function(){return!!(iI()||rC)},e.hydrate=(...e)=>{l8().hydrate(...e)},e.hydrateOnIdle=(e=1e4)=>t=>{let n=nU(t,{timeout:e});return()=>nH(n)},e.hydrateOnInteraction=(e=[])=>(t,n)=>{R(e)&&(e=[e]);let r=!1,i=e=>{r||(r=!0,l(),t(),e.target.dispatchEvent(new e.constructor(e.type,e)))},l=()=>{n(t=>{for(let n of e)t.removeEventListener(n,i)})};return n(t=>{for(let n of e)t.addEventListener(n,i,{once:!0})}),l},e.hydrateOnMediaQuery=e=>t=>{if(e){let n=matchMedia(e);if(!n.matches)return n.addEventListener("change",t,{once:!0}),()=>n.removeEventListener("change",t);t()}},e.hydrateOnVisible=e=>(t,n)=>{let r=new IntersectionObserver(e=>{for(let n of e)if(n.isIntersecting){r.disconnect(),t();break}},e);return n(e=>{if(e instanceof Element){if(function(e){let{top:t,left:n,bottom:r,right:i}=e.getBoundingClientRect(),{innerHeight:l,innerWidth:s}=window;return(t>0&&t<l||r>0&&r<l)&&(n>0&&n<s||i>0&&i<s)}(e))return t(),r.disconnect(),!1;r.observe(e)}}),()=>r.disconnect()},e.initCustomFormatter=function(){},e.initDirectivesForSSR=S,e.inject=t5,e.isMemoSame=iq,e.isProxy=tm,e.isReactive=tf,e.isReadonly=tp,e.isRef=tS,e.isRuntimeOnly=()=>!d,e.isShallow=tg,e.isVNode=id,e.markRaw=ty,e.mergeDefaults=function(e,t){let n=rd(e);for(let e in t){if(e.startsWith("__skip"))continue;let r=n[e];r?E(r)||I(r)?r=n[e]={type:r,default:t[e]}:r.default=t[e]:null===r&&(r=n[e]={default:t[e]}),r&&t[`__skip_${e}`]&&(r.skipFactory=!0)}return n},e.mergeModels=function(e,t){return e&&t?E(e)&&E(t)?e.concat(t):T({},rd(e),rd(t)):e||t},e.mergeProps=iT,e.nextTick=tJ,e.nodeOps=iX,e.normalizeClass=ei,e.normalizeProps=function(e){if(!e)return null;let{class:t,style:n}=e;return t&&!R(t)&&(e.class=ei(t)),n&&(e.style=Y(n)),e},e.normalizeStyle=Y,e.onActivated=nJ,e.onBeforeMount=n1,e.onBeforeUnmount=n4,e.onBeforeUpdate=n3,e.onDeactivated=nG,e.onErrorCaptured=re,e.onMounted=n2,e.onRenderTracked=n7,e.onRenderTriggered=n9,e.onScopeDispose=function(e,t=!1){l&&l.cleanups.push(e)},e.onServerPrefetch=n5,e.onUnmounted=n8,e.onUpdated=n6,e.onWatcherCleanup=tL,e.openBlock=il,e.patchProp=lT,e.popScopeId=function(){t2=null},e.provide=t8,e.proxyRefs=tN,e.pushScopeId=function(e){t2=e},e.queuePostFlushCb=tQ,e.reactive=tc,e.readonly=td,e.ref=tx,e.registerRuntimeCompiler=iL,e.render=l5,e.renderList=function(e,t,n,r){let i,l=n&&n[r],s=E(e);if(s||R(e)){let n=s&&tf(e),r=!1,o=!1;n&&(r=!tg(e),o=tp(e),e=eH(e)),i=Array(e.length);for(let n=0,s=e.length;n<s;n++)i[n]=t(r?o?t_(tb(e[n])):tb(e[n]):e[n],n,void 0,l&&l[n])}else if("number"==typeof e){i=Array(e);for(let n=0;n<e;n++)i[n]=t(n+1,n,void 0,l&&l[n])}else if(M(e))if(e[Symbol.iterator])i=Array.from(e,(e,n)=>t(e,n,void 0,l&&l[n]));else{let n=Object.keys(e);i=Array(n.length);for(let r=0,s=n.length;r<s;r++){let s=n[r];i[r]=t(e[s],s,r,l&&l[r])}}else i=[];return n&&(n[r]=i),i},e.renderSlot=function(e,t,n,r,i,l){let s;if(null==n&&(n={}),t1.ce||t1.parent&&nq(t1.parent)&&t1.parent.ce){let e=null!=l&&null==n.key?T({},n,{key:l}):n,i=Object.keys(e).length>0;return"default"!==t&&(e.name=t),il(),iu(r9,null,[iv("slot",e,r&&r())],i?-2:64)}let o=e[t];o&&o._c&&(o._d=!1);let a=ir.length;il();try{let i=o&&function e(t){return t.some(t=>!id(t)||t.type!==ie&&(t.type!==r9||!!e(t.children)))?t:null}(o(n)),a=n.key||l||i&&i.key;s=iu(r9,{key:(a&&!O(a)?a:`_${t}`)+(!i&&r?"_fb":"")},i||(r?r():[]),i&&1===e._?64:-2)}catch(e){for(let e=ir.length;e>a;e--)is();throw e}finally{o&&o._c&&(o._d=!0)}return!i&&s.scopeId&&(s.slotScopeIds=[s.scopeId+"-s"]),s},e.resolveComponent=function(e,t){return rr(rt,e,!0,t)||e},e.resolveDirective=function(e){return rr("directives",e)},e.resolveDynamicComponent=function(e){return R(e)?rr(rt,e,!1)||e:e||rn},e.resolveFilter=null,e.resolveTransitionHooks=nS,e.setBlockTracking=ia,e.setDevtoolsHook=S,e.setTransitionHooks=nC,e.shallowReactive=tu,e.shallowReadonly=function(e){return th(e,!0,e5,ti,ta)},e.shallowRef=tk,e.ssrContextKey=t9,e.ssrUtils=null,e.stop=function(e){e.effect.stop()},e.toDisplayString=ep,e.toHandlerKey=W,e.toHandlers=function(e,t){let n={};for(let r in e)n[t&&/[A-Z]/.test(r)?`on:${r}`:W(r)]=e[r];return n},e.toRaw=tv,e.toRef=function(e,t,n){if(tS(e))return e;if(I(e))return new tO(e);if(!M(e)||!(arguments.length>1))return tx(e);return new tR(e,t,n)},e.toRefs=function(e){let t=E(e)?Array(e.length):{};for(let n in e)t[n]=new tR(e,n,void 0);return t},e.toValue=function(e){return I(e)?e():tw(e)},e.transformVNodeArgs=function(e){},e.triggerRef=function(e){e.dep&&e.dep.trigger()},e.unref=tw,e.useAttrs=function(){return ru().attrs},e.useCssModule=function(e="$style"){return b},e.useCssVars=function(e){let t=iI();if(!t)return;let n=t.ut=(n=e(t.proxy))=>{Array.from(document.querySelectorAll(`[data-v-owner="${t.uid}"]`)).forEach(e=>lc(e,n))},r=()=>{let r=e(t.proxy);t.ce?lc(t.ce,r):function e(t,n){if(128&t.shapeFlag){let r=t.suspense;t=r.activeBranch,r.pendingBranch&&!r.isHydrating&&r.effects.push(()=>{e(r.activeBranch,n)})}for(;t.component;)t=t.component.subTree;if(1&t.shapeFlag&&t.el)lc(t.el,n);else if(t.type===r9)t.children.forEach(t=>e(t,n));else if(t.type===it){let{el:e,anchor:r}=t;for(;e&&(lc(e,n),e!==r);)e=e.nextSibling}}(t.subTree,r),n(r)};n3(()=>{tQ(r)}),n2(()=>{ne(r,S,{flush:"post"});let e=new MutationObserver(r);e.observe(t.subTree.el.parentNode,{childList:!0}),n8(()=>e.disconnect())})},e.useHost=lI,e.useId=function(){let e=iI();return e?(e.appContext.config.idPrefix||"v")+"-"+e.ids[0]+e.ids[1]++:""},e.useModel=function(e,t,n=b){let r=iI(),i=B(t),l=H(t),s=rT(e,i),o=tI((s,o)=>{let a,c,u=b;return t7(()=>{let t=e[i];K(a,t)&&(a=t,o())}),{get:()=>(s(),n.get?n.get(a):a),set(e){let s=n.set?n.set(e):e;if(!K(s,a)&&!(u!==b&&K(e,u)))return;let d=r.vnode.props,h=!!(d&&(t in d||i in d||l in d)&&(`onUpdate:${t}`in d||`onUpdate:${i}`in d||`onUpdate:${l}`in d));h||(a=e,o()),r.emit(`update:${t}`,s),K(e,u)&&(K(e,s)&&!K(s,c)||h&&u!==b&&!K(s,a))&&o(),u=e,c=s}}});return o[Symbol.iterator]=()=>{let e=0;return{next:()=>e<2?{value:e++?s||b:o,done:!1}:{done:!0}}},o},e.useSSRContext=()=>{},e.useShadowRoot=function(){let e=lI();return e&&e.shadowRoot},e.useSlots=function(){return ru().slots},e.useTemplateRef=function(e){let t=iI(),n=tk(null);return t&&Object.defineProperty(t.refs===b?t.refs={}:t.refs,e,{enumerable:!0,get:()=>n.value,set:e=>n.value=e}),n},e.useTransitionState=np,e.vModelCheckbox=lz,e.vModelDynamic={created(e,t,n){l0(e,t,n,null,"created")},mounted(e,t,n){l0(e,t,n,null,"mounted")},beforeUpdate(e,t,n,r){l0(e,t,n,r,"beforeUpdate")},updated(e,t,n,r){l0(e,t,n,r,"updated")}},e.vModelRadio=lG,e.vModelSelect=lX,e.vModelText=lK,e.vShow={name:"show",beforeMount(e,{value:t},{transition:n}){e[ll]="none"===e.style.display?"":e.style.display,n&&t?n.beforeEnter(e):lo(e,t)},mounted(e,{value:t},{transition:n}){n&&t&&n.enter(e)},updated(e,{value:t,oldValue:n},{transition:r}){!t!=!n&&(r?t?(r.beforeEnter(e),lo(e,!0),r.enter(e)):r.leave(e,()=>{lo(e,!1)}):lo(e,t))},beforeUnmount(e,{value:t}){lo(e,t)}},e.version=iW,e.warn=S,e.watch=function(e,t,n){return ne(e,t,n)},e.watchEffect=function(e,t){return ne(e,null,t)},e.watchPostEffect=function(e,t){return ne(e,null,{flush:"post"})},e.watchSyncEffect=t7,e.withAsyncContext=function(e){let t=iI(),n=iP,r=e();iO(),n&&u(!1);let i=()=>{iR(t),n&&u(!0)},l=()=>{iI()!==t&&t.scope.off(),iO(),n&&u(!1)};return P(r)&&(r=r.catch(e=>{throw i(),Promise.resolve().then(()=>Promise.resolve().then(l)),e})),[r,()=>{i(),Promise.resolve().then(l)}]},e.withCtx=t6,e.withDefaults=function(e,t){return null},e.withDirectives=function(e,t){if(null===t1)return e;let n=ij(t1),r=e.dirs||(e.dirs=[]);for(let e=0;e<t.length;e++){let[i,l,s,o=b]=t[e];i&&(I(i)&&(i={mounted:i,updated:i}),i.deep&&t$(l),r.push({dir:i,instance:n,value:l,oldValue:void 0,arg:s,modifiers:o}))}return e},e.withKeys=(e,t)=>{let n=e._withKeys||(e._withKeys={}),r=t.join(".");return n[r]||(n[r]=n=>{if(!("key"in n))return;let r=H(n.key);if(t.some(e=>e===r||l3[e]===r))return e(n)})},e.withMemo=function(e,t,n,r){let i=n[r];if(i&&iq(i,e))return i;let l=t();return l.memo=e.slice(),l.cacheIndex=r,n[r]=l},e.withModifiers=(e,t)=>{if(!e)return e;let n=e._withMods||(e._withMods={}),r=t.join(".");return n[r]||(n[r]=(n,...r)=>{for(let e=0;e<t.length;e++){let r=l2[t[e]];if(r&&r(n,t))return}return e(n,...r)})},e.withScopeId=e=>t6,e}({});
"##;

const MARKED_JS: &str = r##"/**
 * marked v15.0.12 - a markdown parser
 * Copyright (c) 2011-2025, Christopher Jeffrey. (MIT Licensed)
 * https://github.com/markedjs/marked
 */

/**
 * DO NOT EDIT THIS FILE
 * The code in this file is generated from files in ./src/
 */
(function(g,f){if(typeof exports=="object"&&typeof module<"u"){module.exports=f()}else if("function"==typeof define && define.amd){define("marked",f)}else {g["marked"]=f()}}(typeof globalThis < "u" ? globalThis : typeof self < "u" ? self : this,function(){var exports={};var __exports=exports;var module={exports};
"use strict";var H=Object.defineProperty;var be=Object.getOwnPropertyDescriptor;var Te=Object.getOwnPropertyNames;var we=Object.prototype.hasOwnProperty;var ye=(l,e)=>{for(var t in e)H(l,t,{get:e[t],enumerable:!0})},Re=(l,e,t,n)=>{if(e&&typeof e=="object"||typeof e=="function")for(let s of Te(e))!we.call(l,s)&&s!==t&&H(l,s,{get:()=>e[s],enumerable:!(n=be(e,s))||n.enumerable});return l};var Se=l=>Re(H({},"__esModule",{value:!0}),l);var kt={};ye(kt,{Hooks:()=>L,Lexer:()=>x,Marked:()=>E,Parser:()=>b,Renderer:()=>$,TextRenderer:()=>_,Tokenizer:()=>S,defaults:()=>w,getDefaults:()=>z,lexer:()=>ht,marked:()=>k,options:()=>it,parse:()=>pt,parseInline:()=>ct,parser:()=>ut,setOptions:()=>ot,use:()=>lt,walkTokens:()=>at});module.exports=Se(kt);function z(){return{async:!1,breaks:!1,extensions:null,gfm:!0,hooks:null,pedantic:!1,renderer:null,silent:!1,tokenizer:null,walkTokens:null}}var w=z();function N(l){w=l}var I={exec:()=>null};function h(l,e=""){let t=typeof l=="string"?l:l.source,n={replace:(s,i)=>{let r=typeof i=="string"?i:i.source;return r=r.replace(m.caret,"$1"),t=t.replace(s,r),n},getRegex:()=>new RegExp(t,e)};return n}var m={codeRemoveIndent:/^(?: {1,4}| {0,3}\t)/gm,outputLinkReplace:/\\([\[\]])/g,indentCodeCompensation:/^(\s+)(?:```)/,beginningSpace:/^\s+/,endingHash:/#$/,startingSpaceChar:/^ /,endingSpaceChar:/ $/,nonSpaceChar:/[^ ]/,newLineCharGlobal:/\n/g,tabCharGlobal:/\t/g,multipleSpaceGlobal:/\s+/g,blankLine:/^[ \t]*$/,doubleBlankLine:/\n[ \t]*\n[ \t]*$/,blockquoteStart:/^ {0,3}>/,blockquoteSetextReplace:/\n {0,3}((?:=+|-+) *)(?=\n|$)/g,blockquoteSetextReplace2:/^ {0,3}>[ \t]?/gm,listReplaceTabs:/^\t+/,listReplaceNesting:/^ {1,4}(?=( {4})*[^ ])/g,listIsTask:/^\[[ xX]\] /,listReplaceTask:/^\[[ xX]\] +/,anyLine:/\n.*\n/,hrefBrackets:/^<(.*)>$/,tableDelimiter:/[:|]/,tableAlignChars:/^\||\| *$/g,tableRowBlankLine:/\n[ \t]*$/,tableAlignRight:/^ *-+: *$/,tableAlignCenter:/^ *:-+: *$/,tableAlignLeft:/^ *:-+ *$/,startATag:/^<a /i,endATag:/^<\/a>/i,startPreScriptTag:/^<(pre|code|kbd|script)(\s|>)/i,endPreScriptTag:/^<\/(pre|code|kbd|script)(\s|>)/i,startAngleBracket:/^</,endAngleBracket:/>$/,pedanticHrefTitle:/^([^'"]*[^\s])\s+(['"])(.*)\2/,unicodeAlphaNumeric:/[\p{L}\p{N}]/u,escapeTest:/[&<>"']/,escapeReplace:/[&<>"']/g,escapeTestNoEncode:/[<>"']|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)/,escapeReplaceNoEncode:/[<>"']|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)/g,unescapeTest:/&(#(?:\d+)|(?:#x[0-9A-Fa-f]+)|(?:\w+));?/ig,caret:/(^|[^\[])\^/g,percentDecode:/%25/g,findPipe:/\|/g,splitPipe:/ \|/,slashPipe:/\\\|/g,carriageReturn:/\r\n|\r/g,spaceLine:/^ +$/gm,notSpaceStart:/^\S*/,endingNewline:/\n$/,listItemRegex:l=>new RegExp(`^( {0,3}${l})((?:[	 ][^\\n]*)?(?:\\n|$))`),nextBulletRegex:l=>new RegExp(`^ {0,${Math.min(3,l-1)}}(?:[*+-]|\\d{1,9}[.)])((?:[ 	][^\\n]*)?(?:\\n|$))`),hrRegex:l=>new RegExp(`^ {0,${Math.min(3,l-1)}}((?:- *){3,}|(?:_ *){3,}|(?:\\* *){3,})(?:\\n+|$)`),fencesBeginRegex:l=>new RegExp(`^ {0,${Math.min(3,l-1)}}(?:\`\`\`|~~~)`),headingBeginRegex:l=>new RegExp(`^ {0,${Math.min(3,l-1)}}#`),htmlBeginRegex:l=>new RegExp(`^ {0,${Math.min(3,l-1)}}<(?:[a-z].*>|!--)`,"i")},$e=/^(?:[ \t]*(?:\n|$))+/,_e=/^((?: {4}| {0,3}\t)[^\n]+(?:\n(?:[ \t]*(?:\n|$))*)?)+/,Le=/^ {0,3}(`{3,}(?=[^`\n]*(?:\n|$))|~{3,})([^\n]*)(?:\n|$)(?:|([\s\S]*?)(?:\n|$))(?: {0,3}\1[~`]* *(?=\n|$)|$)/,O=/^ {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)/,ze=/^ {0,3}(#{1,6})(?=\s|$)(.*)(?:\n+|$)/,F=/(?:[*+-]|\d{1,9}[.)])/,ie=/^(?!bull |blockCode|fences|blockquote|heading|html|table)((?:.|\n(?!\s*?\n|bull |blockCode|fences|blockquote|heading|html|table))+?)\n {0,3}(=+|-+) *(?:\n+|$)/,oe=h(ie).replace(/bull/g,F).replace(/blockCode/g,/(?: {4}| {0,3}\t)/).replace(/fences/g,/ {0,3}(?:`{3,}|~{3,})/).replace(/blockquote/g,/ {0,3}>/).replace(/heading/g,/ {0,3}#{1,6}/).replace(/html/g,/ {0,3}<[^\n>]+>\n/).replace(/\|table/g,"").getRegex(),Me=h(ie).replace(/bull/g,F).replace(/blockCode/g,/(?: {4}| {0,3}\t)/).replace(/fences/g,/ {0,3}(?:`{3,}|~{3,})/).replace(/blockquote/g,/ {0,3}>/).replace(/heading/g,/ {0,3}#{1,6}/).replace(/html/g,/ {0,3}<[^\n>]+>\n/).replace(/table/g,/ {0,3}\|?(?:[:\- ]*\|)+[\:\- ]*\n/).getRegex(),Q=/^([^\n]+(?:\n(?!hr|heading|lheading|blockquote|fences|list|html|table| +\n)[^\n]+)*)/,Pe=/^[^\n]+/,U=/(?!\s*\])(?:\\.|[^\[\]\\])+/,Ae=h(/^ {0,3}\[(label)\]: *(?:\n[ \t]*)?([^<\s][^\s]*|<.*?>)(?:(?: +(?:\n[ \t]*)?| *\n[ \t]*)(title))? *(?:\n+|$)/).replace("label",U).replace("title",/(?:"(?:\\"?|[^"\\])*"|'[^'\n]*(?:\n[^'\n]+)*\n?'|\([^()]*\))/).getRegex(),Ee=h(/^( {0,3}bull)([ \t][^\n]+?)?(?:\n|$)/).replace(/bull/g,F).getRegex(),v="address|article|aside|base|basefont|blockquote|body|caption|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title|tr|track|ul",K=/<!--(?:-?>|[\s\S]*?(?:-->|$))/,Ce=h("^ {0,3}(?:<(script|pre|style|textarea)[\\s>][\\s\\S]*?(?:</\\1>[^\\n]*\\n+|$)|comment[^\\n]*(\\n+|$)|<\\?[\\s\\S]*?(?:\\?>\\n*|$)|<![A-Z][\\s\\S]*?(?:>\\n*|$)|<!\\[CDATA\\[[\\s\\S]*?(?:\\]\\]>\\n*|$)|</?(tag)(?: +|\\n|/?>)[\\s\\S]*?(?:(?:\\n[ 	]*)+\\n|$)|<(?!script|pre|style|textarea)([a-z][\\w-]*)(?:attribute)*? */?>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n[ 	]*)+\\n|$)|</(?!script|pre|style|textarea)[a-z][\\w-]*\\s*>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n[ 	]*)+\\n|$))","i").replace("comment",K).replace("tag",v).replace("attribute",/ +[a-zA-Z:_][\w.:-]*(?: *= *"[^"\n]*"| *= *'[^'\n]*'| *= *[^\s"'=<>`]+)?/).getRegex(),le=h(Q).replace("hr",O).replace("heading"," {0,3}#{1,6}(?:\\s|$)").replace("|lheading","").replace("|table","").replace("blockquote"," {0,3}>").replace("fences"," {0,3}(?:`{3,}(?=[^`\\n]*\\n)|~{3,})[^\\n]*\\n").replace("list"," {0,3}(?:[*+-]|1[.)]) ").replace("html","</?(?:tag)(?: +|\\n|/?>)|<(?:script|pre|style|textarea|!--)").replace("tag",v).getRegex(),Ie=h(/^( {0,3}> ?(paragraph|[^\n]*)(?:\n|$))+/).replace("paragraph",le).getRegex(),X={blockquote:Ie,code:_e,def:Ae,fences:Le,heading:ze,hr:O,html:Ce,lheading:oe,list:Ee,newline:$e,paragraph:le,table:I,text:Pe},re=h("^ *([^\\n ].*)\\n {0,3}((?:\\| *)?:?-+:? *(?:\\| *:?-+:? *)*(?:\\| *)?)(?:\\n((?:(?! *\\n|hr|heading|blockquote|code|fences|list|html).*(?:\\n|$))*)\\n*|$)").replace("hr",O).replace("heading"," {0,3}#{1,6}(?:\\s|$)").replace("blockquote"," {0,3}>").replace("code","(?: {4}| {0,3}	)[^\\n]").replace("fences"," {0,3}(?:`{3,}(?=[^`\\n]*\\n)|~{3,})[^\\n]*\\n").replace("list"," {0,3}(?:[*+-]|1[.)]) ").replace("html","</?(?:tag)(?: +|\\n|/?>)|<(?:script|pre|style|textarea|!--)").replace("tag",v).getRegex(),Oe={...X,lheading:Me,table:re,paragraph:h(Q).replace("hr",O).replace("heading"," {0,3}#{1,6}(?:\\s|$)").replace("|lheading","").replace("table",re).replace("blockquote"," {0,3}>").replace("fences"," {0,3}(?:`{3,}(?=[^`\\n]*\\n)|~{3,})[^\\n]*\\n").replace("list"," {0,3}(?:[*+-]|1[.)]) ").replace("html","</?(?:tag)(?: +|\\n|/?>)|<(?:script|pre|style|textarea|!--)").replace("tag",v).getRegex()},Be={...X,html:h(`^ *(?:comment *(?:\\n|\\s*$)|<(tag)[\\s\\S]+?</\\1> *(?:\\n{2,}|\\s*$)|<tag(?:"[^"]*"|'[^']*'|\\s[^'"/>\\s]*)*?/?> *(?:\\n{2,}|\\s*$))`).replace("comment",K).replace(/tag/g,"(?!(?:a|em|strong|small|s|cite|q|dfn|abbr|data|time|code|var|samp|kbd|sub|sup|i|b|u|mark|ruby|rt|rp|bdi|bdo|span|br|wbr|ins|del|img)\\b)\\w+(?!:|[^\\w\\s@]*@)\\b").getRegex(),def:/^ *\[([^\]]+)\]: *<?([^\s>]+)>?(?: +(["(][^\n]+[")]))? *(?:\n+|$)/,heading:/^(#{1,6})(.*)(?:\n+|$)/,fences:I,lheading:/^(.+?)\n {0,3}(=+|-+) *(?:\n+|$)/,paragraph:h(Q).replace("hr",O).replace("heading",` *#{1,6} *[^
]`).replace("lheading",oe).replace("|table","").replace("blockquote"," {0,3}>").replace("|fences","").replace("|list","").replace("|html","").replace("|tag","").getRegex()},qe=/^\\([!"#$%&'()*+,\-./:;<=>?@\[\]\\^_`{|}~])/,ve=/^(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/,ae=/^( {2,}|\\)\n(?!\s*$)/,De=/^(`+|[^`])(?:(?= {2,}\n)|[\s\S]*?(?:(?=[\\<!\[`*_]|\b_|$)|[^ ](?= {2,}\n)))/,D=/[\p{P}\p{S}]/u,W=/[\s\p{P}\p{S}]/u,ce=/[^\s\p{P}\p{S}]/u,Ze=h(/^((?![*_])punctSpace)/,"u").replace(/punctSpace/g,W).getRegex(),pe=/(?!~)[\p{P}\p{S}]/u,Ge=/(?!~)[\s\p{P}\p{S}]/u,He=/(?:[^\s\p{P}\p{S}]|~)/u,Ne=/\[[^[\]]*?\]\((?:\\.|[^\\\(\)]|\((?:\\.|[^\\\(\)])*\))*\)|`[^`]*?`|<[^<>]*?>/g,ue=/^(?:\*+(?:((?!\*)punct)|[^\s*]))|^_+(?:((?!_)punct)|([^\s_]))/,je=h(ue,"u").replace(/punct/g,D).getRegex(),Fe=h(ue,"u").replace(/punct/g,pe).getRegex(),he="^[^_*]*?__[^_*]*?\\*[^_*]*?(?=__)|[^*]+(?=[^*])|(?!\\*)punct(\\*+)(?=[\\s]|$)|notPunctSpace(\\*+)(?!\\*)(?=punctSpace|$)|(?!\\*)punctSpace(\\*+)(?=notPunctSpace)|[\\s](\\*+)(?!\\*)(?=punct)|(?!\\*)punct(\\*+)(?!\\*)(?=punct)|notPunctSpace(\\*+)(?=notPunctSpace)",Qe=h(he,"gu").replace(/notPunctSpace/g,ce).replace(/punctSpace/g,W).replace(/punct/g,D).getRegex(),Ue=h(he,"gu").replace(/notPunctSpace/g,He).replace(/punctSpace/g,Ge).replace(/punct/g,pe).getRegex(),Ke=h("^[^_*]*?\\*\\*[^_*]*?_[^_*]*?(?=\\*\\*)|[^_]+(?=[^_])|(?!_)punct(_+)(?=[\\s]|$)|notPunctSpace(_+)(?!_)(?=punctSpace|$)|(?!_)punctSpace(_+)(?=notPunctSpace)|[\\s](_+)(?!_)(?=punct)|(?!_)punct(_+)(?!_)(?=punct)","gu").replace(/notPunctSpace/g,ce).replace(/punctSpace/g,W).replace(/punct/g,D).getRegex(),Xe=h(/\\(punct)/,"gu").replace(/punct/g,D).getRegex(),We=h(/^<(scheme:[^\s\x00-\x1f<>]*|email)>/).replace("scheme",/[a-zA-Z][a-zA-Z0-9+.-]{1,31}/).replace("email",/[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+(@)[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(?![-_])/).getRegex(),Je=h(K).replace("(?:-->|$)","-->").getRegex(),Ve=h("^comment|^</[a-zA-Z][\\w:-]*\\s*>|^<[a-zA-Z][\\w-]*(?:attribute)*?\\s*/?>|^<\\?[\\s\\S]*?\\?>|^<![a-zA-Z]+\\s[\\s\\S]*?>|^<!\\[CDATA\\[[\\s\\S]*?\\]\\]>").replace("comment",Je).replace("attribute",/\s+[a-zA-Z:_][\w.:-]*(?:\s*=\s*"[^"]*"|\s*=\s*'[^']*'|\s*=\s*[^\s"'=<>`]+)?/).getRegex(),q=/(?:\[(?:\\.|[^\[\]\\])*\]|\\.|`[^`]*`|[^\[\]\\`])*?/,Ye=h(/^!?\[(label)\]\(\s*(href)(?:(?:[ \t]*(?:\n[ \t]*)?)(title))?\s*\)/).replace("label",q).replace("href",/<(?:\\.|[^\n<>\\])+>|[^ \t\n\x00-\x1f]*/).replace("title",/"(?:\\"?|[^"\\])*"|'(?:\\'?|[^'\\])*'|\((?:\\\)?|[^)\\])*\)/).getRegex(),ke=h(/^!?\[(label)\]\[(ref)\]/).replace("label",q).replace("ref",U).getRegex(),ge=h(/^!?\[(ref)\](?:\[\])?/).replace("ref",U).getRegex(),et=h("reflink|nolink(?!\\()","g").replace("reflink",ke).replace("nolink",ge).getRegex(),J={_backpedal:I,anyPunctuation:Xe,autolink:We,blockSkip:Ne,br:ae,code:ve,del:I,emStrongLDelim:je,emStrongRDelimAst:Qe,emStrongRDelimUnd:Ke,escape:qe,link:Ye,nolink:ge,punctuation:Ze,reflink:ke,reflinkSearch:et,tag:Ve,text:De,url:I},tt={...J,link:h(/^!?\[(label)\]\((.*?)\)/).replace("label",q).getRegex(),reflink:h(/^!?\[(label)\]\s*\[([^\]]*)\]/).replace("label",q).getRegex()},j={...J,emStrongRDelimAst:Ue,emStrongLDelim:Fe,url:h(/^((?:ftp|https?):\/\/|www\.)(?:[a-zA-Z0-9\-]+\.?)+[^\s<]*|^email/,"i").replace("email",/[A-Za-z0-9._+-]+(@)[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![-_])/).getRegex(),_backpedal:/(?:[^?!.,:;*_'"~()&]+|\([^)]*\)|&(?![a-zA-Z0-9]+;$)|[?!.,:;*_'"~)]+(?!$))+/,del:/^(~~?)(?=[^\s~])((?:\\.|[^\\])*?(?:\\.|[^\s~\\]))\1(?=[^~]|$)/,text:/^([`~]+|[^`~])(?:(?= {2,}\n)|(?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)|[\s\S]*?(?:(?=[\\<!\[`*~_]|\b_|https?:\/\/|ftp:\/\/|www\.|$)|[^ ](?= {2,}\n)|[^a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-](?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)))/},nt={...j,br:h(ae).replace("{2,}","*").getRegex(),text:h(j.text).replace("\\b_","\\b_| {2,}\\n").replace(/\{2,\}/g,"*").getRegex()},B={normal:X,gfm:Oe,pedantic:Be},P={normal:J,gfm:j,breaks:nt,pedantic:tt};var st={"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"},fe=l=>st[l];function R(l,e){if(e){if(m.escapeTest.test(l))return l.replace(m.escapeReplace,fe)}else if(m.escapeTestNoEncode.test(l))return l.replace(m.escapeReplaceNoEncode,fe);return l}function V(l){try{l=encodeURI(l).replace(m.percentDecode,"%")}catch{return null}return l}function Y(l,e){let t=l.replace(m.findPipe,(i,r,o)=>{let a=!1,c=r;for(;--c>=0&&o[c]==="\\";)a=!a;return a?"|":" |"}),n=t.split(m.splitPipe),s=0;if(n[0].trim()||n.shift(),n.length>0&&!n.at(-1)?.trim()&&n.pop(),e)if(n.length>e)n.splice(e);else for(;n.length<e;)n.push("");for(;s<n.length;s++)n[s]=n[s].trim().replace(m.slashPipe,"|");return n}function A(l,e,t){let n=l.length;if(n===0)return"";let s=0;for(;s<n;){let i=l.charAt(n-s-1);if(i===e&&!t)s++;else if(i!==e&&t)s++;else break}return l.slice(0,n-s)}function de(l,e){if(l.indexOf(e[1])===-1)return-1;let t=0;for(let n=0;n<l.length;n++)if(l[n]==="\\")n++;else if(l[n]===e[0])t++;else if(l[n]===e[1]&&(t--,t<0))return n;return t>0?-2:-1}function me(l,e,t,n,s){let i=e.href,r=e.title||null,o=l[1].replace(s.other.outputLinkReplace,"$1");n.state.inLink=!0;let a={type:l[0].charAt(0)==="!"?"image":"link",raw:t,href:i,title:r,text:o,tokens:n.inlineTokens(o)};return n.state.inLink=!1,a}function rt(l,e,t){let n=l.match(t.other.indentCodeCompensation);if(n===null)return e;let s=n[1];return e.split(`
`).map(i=>{let r=i.match(t.other.beginningSpace);if(r===null)return i;let[o]=r;return o.length>=s.length?i.slice(s.length):i}).join(`
`)}var S=class{options;rules;lexer;constructor(e){this.options=e||w}space(e){let t=this.rules.block.newline.exec(e);if(t&&t[0].length>0)return{type:"space",raw:t[0]}}code(e){let t=this.rules.block.code.exec(e);if(t){let n=t[0].replace(this.rules.other.codeRemoveIndent,"");return{type:"code",raw:t[0],codeBlockStyle:"indented",text:this.options.pedantic?n:A(n,`
`)}}}fences(e){let t=this.rules.block.fences.exec(e);if(t){let n=t[0],s=rt(n,t[3]||"",this.rules);return{type:"code",raw:n,lang:t[2]?t[2].trim().replace(this.rules.inline.anyPunctuation,"$1"):t[2],text:s}}}heading(e){let t=this.rules.block.heading.exec(e);if(t){let n=t[2].trim();if(this.rules.other.endingHash.test(n)){let s=A(n,"#");(this.options.pedantic||!s||this.rules.other.endingSpaceChar.test(s))&&(n=s.trim())}return{type:"heading",raw:t[0],depth:t[1].length,text:n,tokens:this.lexer.inline(n)}}}hr(e){let t=this.rules.block.hr.exec(e);if(t)return{type:"hr",raw:A(t[0],`
`)}}blockquote(e){let t=this.rules.block.blockquote.exec(e);if(t){let n=A(t[0],`
`).split(`
`),s="",i="",r=[];for(;n.length>0;){let o=!1,a=[],c;for(c=0;c<n.length;c++)if(this.rules.other.blockquoteStart.test(n[c]))a.push(n[c]),o=!0;else if(!o)a.push(n[c]);else break;n=n.slice(c);let p=a.join(`
`),u=p.replace(this.rules.other.blockquoteSetextReplace,`
    $1`).replace(this.rules.other.blockquoteSetextReplace2,"");s=s?`${s}
${p}`:p,i=i?`${i}
${u}`:u;let d=this.lexer.state.top;if(this.lexer.state.top=!0,this.lexer.blockTokens(u,r,!0),this.lexer.state.top=d,n.length===0)break;let g=r.at(-1);if(g?.type==="code")break;if(g?.type==="blockquote"){let T=g,f=T.raw+`
`+n.join(`
`),y=this.blockquote(f);r[r.length-1]=y,s=s.substring(0,s.length-T.raw.length)+y.raw,i=i.substring(0,i.length-T.text.length)+y.text;break}else if(g?.type==="list"){let T=g,f=T.raw+`
`+n.join(`
`),y=this.list(f);r[r.length-1]=y,s=s.substring(0,s.length-g.raw.length)+y.raw,i=i.substring(0,i.length-T.raw.length)+y.raw,n=f.substring(r.at(-1).raw.length).split(`
`);continue}}return{type:"blockquote",raw:s,tokens:r,text:i}}}list(e){let t=this.rules.block.list.exec(e);if(t){let n=t[1].trim(),s=n.length>1,i={type:"list",raw:"",ordered:s,start:s?+n.slice(0,-1):"",loose:!1,items:[]};n=s?`\\d{1,9}\\${n.slice(-1)}`:`\\${n}`,this.options.pedantic&&(n=s?n:"[*+-]");let r=this.rules.other.listItemRegex(n),o=!1;for(;e;){let c=!1,p="",u="";if(!(t=r.exec(e))||this.rules.block.hr.test(e))break;p=t[0],e=e.substring(p.length);let d=t[2].split(`
`,1)[0].replace(this.rules.other.listReplaceTabs,Z=>" ".repeat(3*Z.length)),g=e.split(`
`,1)[0],T=!d.trim(),f=0;if(this.options.pedantic?(f=2,u=d.trimStart()):T?f=t[1].length+1:(f=t[2].search(this.rules.other.nonSpaceChar),f=f>4?1:f,u=d.slice(f),f+=t[1].length),T&&this.rules.other.blankLine.test(g)&&(p+=g+`
`,e=e.substring(g.length+1),c=!0),!c){let Z=this.rules.other.nextBulletRegex(f),te=this.rules.other.hrRegex(f),ne=this.rules.other.fencesBeginRegex(f),se=this.rules.other.headingBeginRegex(f),xe=this.rules.other.htmlBeginRegex(f);for(;e;){let G=e.split(`
`,1)[0],C;if(g=G,this.options.pedantic?(g=g.replace(this.rules.other.listReplaceNesting,"  "),C=g):C=g.replace(this.rules.other.tabCharGlobal,"    "),ne.test(g)||se.test(g)||xe.test(g)||Z.test(g)||te.test(g))break;if(C.search(this.rules.other.nonSpaceChar)>=f||!g.trim())u+=`
`+C.slice(f);else{if(T||d.replace(this.rules.other.tabCharGlobal,"    ").search(this.rules.other.nonSpaceChar)>=4||ne.test(d)||se.test(d)||te.test(d))break;u+=`
`+g}!T&&!g.trim()&&(T=!0),p+=G+`
`,e=e.substring(G.length+1),d=C.slice(f)}}i.loose||(o?i.loose=!0:this.rules.other.doubleBlankLine.test(p)&&(o=!0));let y=null,ee;this.options.gfm&&(y=this.rules.other.listIsTask.exec(u),y&&(ee=y[0]!=="[ ] ",u=u.replace(this.rules.other.listReplaceTask,""))),i.items.push({type:"list_item",raw:p,task:!!y,checked:ee,loose:!1,text:u,tokens:[]}),i.raw+=p}let a=i.items.at(-1);if(a)a.raw=a.raw.trimEnd(),a.text=a.text.trimEnd();else return;i.raw=i.raw.trimEnd();for(let c=0;c<i.items.length;c++)if(this.lexer.state.top=!1,i.items[c].tokens=this.lexer.blockTokens(i.items[c].text,[]),!i.loose){let p=i.items[c].tokens.filter(d=>d.type==="space"),u=p.length>0&&p.some(d=>this.rules.other.anyLine.test(d.raw));i.loose=u}if(i.loose)for(let c=0;c<i.items.length;c++)i.items[c].loose=!0;return i}}html(e){let t=this.rules.block.html.exec(e);if(t)return{type:"html",block:!0,raw:t[0],pre:t[1]==="pre"||t[1]==="script"||t[1]==="style",text:t[0]}}def(e){let t=this.rules.block.def.exec(e);if(t){let n=t[1].toLowerCase().replace(this.rules.other.multipleSpaceGlobal," "),s=t[2]?t[2].replace(this.rules.other.hrefBrackets,"$1").replace(this.rules.inline.anyPunctuation,"$1"):"",i=t[3]?t[3].substring(1,t[3].length-1).replace(this.rules.inline.anyPunctuation,"$1"):t[3];return{type:"def",tag:n,raw:t[0],href:s,title:i}}}table(e){let t=this.rules.block.table.exec(e);if(!t||!this.rules.other.tableDelimiter.test(t[2]))return;let n=Y(t[1]),s=t[2].replace(this.rules.other.tableAlignChars,"").split("|"),i=t[3]?.trim()?t[3].replace(this.rules.other.tableRowBlankLine,"").split(`
`):[],r={type:"table",raw:t[0],header:[],align:[],rows:[]};if(n.length===s.length){for(let o of s)this.rules.other.tableAlignRight.test(o)?r.align.push("right"):this.rules.other.tableAlignCenter.test(o)?r.align.push("center"):this.rules.other.tableAlignLeft.test(o)?r.align.push("left"):r.align.push(null);for(let o=0;o<n.length;o++)r.header.push({text:n[o],tokens:this.lexer.inline(n[o]),header:!0,align:r.align[o]});for(let o of i)r.rows.push(Y(o,r.header.length).map((a,c)=>({text:a,tokens:this.lexer.inline(a),header:!1,align:r.align[c]})));return r}}lheading(e){let t=this.rules.block.lheading.exec(e);if(t)return{type:"heading",raw:t[0],depth:t[2].charAt(0)==="="?1:2,text:t[1],tokens:this.lexer.inline(t[1])}}paragraph(e){let t=this.rules.block.paragraph.exec(e);if(t){let n=t[1].charAt(t[1].length-1)===`
`?t[1].slice(0,-1):t[1];return{type:"paragraph",raw:t[0],text:n,tokens:this.lexer.inline(n)}}}text(e){let t=this.rules.block.text.exec(e);if(t)return{type:"text",raw:t[0],text:t[0],tokens:this.lexer.inline(t[0])}}escape(e){let t=this.rules.inline.escape.exec(e);if(t)return{type:"escape",raw:t[0],text:t[1]}}tag(e){let t=this.rules.inline.tag.exec(e);if(t)return!this.lexer.state.inLink&&this.rules.other.startATag.test(t[0])?this.lexer.state.inLink=!0:this.lexer.state.inLink&&this.rules.other.endATag.test(t[0])&&(this.lexer.state.inLink=!1),!this.lexer.state.inRawBlock&&this.rules.other.startPreScriptTag.test(t[0])?this.lexer.state.inRawBlock=!0:this.lexer.state.inRawBlock&&this.rules.other.endPreScriptTag.test(t[0])&&(this.lexer.state.inRawBlock=!1),{type:"html",raw:t[0],inLink:this.lexer.state.inLink,inRawBlock:this.lexer.state.inRawBlock,block:!1,text:t[0]}}link(e){let t=this.rules.inline.link.exec(e);if(t){let n=t[2].trim();if(!this.options.pedantic&&this.rules.other.startAngleBracket.test(n)){if(!this.rules.other.endAngleBracket.test(n))return;let r=A(n.slice(0,-1),"\\");if((n.length-r.length)%2===0)return}else{let r=de(t[2],"()");if(r===-2)return;if(r>-1){let a=(t[0].indexOf("!")===0?5:4)+t[1].length+r;t[2]=t[2].substring(0,r),t[0]=t[0].substring(0,a).trim(),t[3]=""}}let s=t[2],i="";if(this.options.pedantic){let r=this.rules.other.pedanticHrefTitle.exec(s);r&&(s=r[1],i=r[3])}else i=t[3]?t[3].slice(1,-1):"";return s=s.trim(),this.rules.other.startAngleBracket.test(s)&&(this.options.pedantic&&!this.rules.other.endAngleBracket.test(n)?s=s.slice(1):s=s.slice(1,-1)),me(t,{href:s&&s.replace(this.rules.inline.anyPunctuation,"$1"),title:i&&i.replace(this.rules.inline.anyPunctuation,"$1")},t[0],this.lexer,this.rules)}}reflink(e,t){let n;if((n=this.rules.inline.reflink.exec(e))||(n=this.rules.inline.nolink.exec(e))){let s=(n[2]||n[1]).replace(this.rules.other.multipleSpaceGlobal," "),i=t[s.toLowerCase()];if(!i){let r=n[0].charAt(0);return{type:"text",raw:r,text:r}}return me(n,i,n[0],this.lexer,this.rules)}}emStrong(e,t,n=""){let s=this.rules.inline.emStrongLDelim.exec(e);if(!s||s[3]&&n.match(this.rules.other.unicodeAlphaNumeric))return;if(!(s[1]||s[2]||"")||!n||this.rules.inline.punctuation.exec(n)){let r=[...s[0]].length-1,o,a,c=r,p=0,u=s[0][0]==="*"?this.rules.inline.emStrongRDelimAst:this.rules.inline.emStrongRDelimUnd;for(u.lastIndex=0,t=t.slice(-1*e.length+r);(s=u.exec(t))!=null;){if(o=s[1]||s[2]||s[3]||s[4]||s[5]||s[6],!o)continue;if(a=[...o].length,s[3]||s[4]){c+=a;continue}else if((s[5]||s[6])&&r%3&&!((r+a)%3)){p+=a;continue}if(c-=a,c>0)continue;a=Math.min(a,a+c+p);let d=[...s[0]][0].length,g=e.slice(0,r+s.index+d+a);if(Math.min(r,a)%2){let f=g.slice(1,-1);return{type:"em",raw:g,text:f,tokens:this.lexer.inlineTokens(f)}}let T=g.slice(2,-2);return{type:"strong",raw:g,text:T,tokens:this.lexer.inlineTokens(T)}}}}codespan(e){let t=this.rules.inline.code.exec(e);if(t){let n=t[2].replace(this.rules.other.newLineCharGlobal," "),s=this.rules.other.nonSpaceChar.test(n),i=this.rules.other.startingSpaceChar.test(n)&&this.rules.other.endingSpaceChar.test(n);return s&&i&&(n=n.substring(1,n.length-1)),{type:"codespan",raw:t[0],text:n}}}br(e){let t=this.rules.inline.br.exec(e);if(t)return{type:"br",raw:t[0]}}del(e){let t=this.rules.inline.del.exec(e);if(t)return{type:"del",raw:t[0],text:t[2],tokens:this.lexer.inlineTokens(t[2])}}autolink(e){let t=this.rules.inline.autolink.exec(e);if(t){let n,s;return t[2]==="@"?(n=t[1],s="mailto:"+n):(n=t[1],s=n),{type:"link",raw:t[0],text:n,href:s,tokens:[{type:"text",raw:n,text:n}]}}}url(e){let t;if(t=this.rules.inline.url.exec(e)){let n,s;if(t[2]==="@")n=t[0],s="mailto:"+n;else{let i;do i=t[0],t[0]=this.rules.inline._backpedal.exec(t[0])?.[0]??"";while(i!==t[0]);n=t[0],t[1]==="www."?s="http://"+t[0]:s=t[0]}return{type:"link",raw:t[0],text:n,href:s,tokens:[{type:"text",raw:n,text:n}]}}}inlineText(e){let t=this.rules.inline.text.exec(e);if(t){let n=this.lexer.state.inRawBlock;return{type:"text",raw:t[0],text:t[0],escaped:n}}}};var x=class l{tokens;options;state;tokenizer;inlineQueue;constructor(e){this.tokens=[],this.tokens.links=Object.create(null),this.options=e||w,this.options.tokenizer=this.options.tokenizer||new S,this.tokenizer=this.options.tokenizer,this.tokenizer.options=this.options,this.tokenizer.lexer=this,this.inlineQueue=[],this.state={inLink:!1,inRawBlock:!1,top:!0};let t={other:m,block:B.normal,inline:P.normal};this.options.pedantic?(t.block=B.pedantic,t.inline=P.pedantic):this.options.gfm&&(t.block=B.gfm,this.options.breaks?t.inline=P.breaks:t.inline=P.gfm),this.tokenizer.rules=t}static get rules(){return{block:B,inline:P}}static lex(e,t){return new l(t).lex(e)}static lexInline(e,t){return new l(t).inlineTokens(e)}lex(e){e=e.replace(m.carriageReturn,`
`),this.blockTokens(e,this.tokens);for(let t=0;t<this.inlineQueue.length;t++){let n=this.inlineQueue[t];this.inlineTokens(n.src,n.tokens)}return this.inlineQueue=[],this.tokens}blockTokens(e,t=[],n=!1){for(this.options.pedantic&&(e=e.replace(m.tabCharGlobal,"    ").replace(m.spaceLine,""));e;){let s;if(this.options.extensions?.block?.some(r=>(s=r.call({lexer:this},e,t))?(e=e.substring(s.raw.length),t.push(s),!0):!1))continue;if(s=this.tokenizer.space(e)){e=e.substring(s.raw.length);let r=t.at(-1);s.raw.length===1&&r!==void 0?r.raw+=`
`:t.push(s);continue}if(s=this.tokenizer.code(e)){e=e.substring(s.raw.length);let r=t.at(-1);r?.type==="paragraph"||r?.type==="text"?(r.raw+=`
`+s.raw,r.text+=`
`+s.text,this.inlineQueue.at(-1).src=r.text):t.push(s);continue}if(s=this.tokenizer.fences(e)){e=e.substring(s.raw.length),t.push(s);continue}if(s=this.tokenizer.heading(e)){e=e.substring(s.raw.length),t.push(s);continue}if(s=this.tokenizer.hr(e)){e=e.substring(s.raw.length),t.push(s);continue}if(s=this.tokenizer.blockquote(e)){e=e.substring(s.raw.length),t.push(s);continue}if(s=this.tokenizer.list(e)){e=e.substring(s.raw.length),t.push(s);continue}if(s=this.tokenizer.html(e)){e=e.substring(s.raw.length),t.push(s);continue}if(s=this.tokenizer.def(e)){e=e.substring(s.raw.length);let r=t.at(-1);r?.type==="paragraph"||r?.type==="text"?(r.raw+=`
`+s.raw,r.text+=`
`+s.raw,this.inlineQueue.at(-1).src=r.text):this.tokens.links[s.tag]||(this.tokens.links[s.tag]={href:s.href,title:s.title});continue}if(s=this.tokenizer.table(e)){e=e.substring(s.raw.length),t.push(s);continue}if(s=this.tokenizer.lheading(e)){e=e.substring(s.raw.length),t.push(s);continue}let i=e;if(this.options.extensions?.startBlock){let r=1/0,o=e.slice(1),a;this.options.extensions.startBlock.forEach(c=>{a=c.call({lexer:this},o),typeof a=="number"&&a>=0&&(r=Math.min(r,a))}),r<1/0&&r>=0&&(i=e.substring(0,r+1))}if(this.state.top&&(s=this.tokenizer.paragraph(i))){let r=t.at(-1);n&&r?.type==="paragraph"?(r.raw+=`
`+s.raw,r.text+=`
`+s.text,this.inlineQueue.pop(),this.inlineQueue.at(-1).src=r.text):t.push(s),n=i.length!==e.length,e=e.substring(s.raw.length);continue}if(s=this.tokenizer.text(e)){e=e.substring(s.raw.length);let r=t.at(-1);r?.type==="text"?(r.raw+=`
`+s.raw,r.text+=`
`+s.text,this.inlineQueue.pop(),this.inlineQueue.at(-1).src=r.text):t.push(s);continue}if(e){let r="Infinite loop on byte: "+e.charCodeAt(0);if(this.options.silent){console.error(r);break}else throw new Error(r)}}return this.state.top=!0,t}inline(e,t=[]){return this.inlineQueue.push({src:e,tokens:t}),t}inlineTokens(e,t=[]){let n=e,s=null;if(this.tokens.links){let o=Object.keys(this.tokens.links);if(o.length>0)for(;(s=this.tokenizer.rules.inline.reflinkSearch.exec(n))!=null;)o.includes(s[0].slice(s[0].lastIndexOf("[")+1,-1))&&(n=n.slice(0,s.index)+"["+"a".repeat(s[0].length-2)+"]"+n.slice(this.tokenizer.rules.inline.reflinkSearch.lastIndex))}for(;(s=this.tokenizer.rules.inline.anyPunctuation.exec(n))!=null;)n=n.slice(0,s.index)+"++"+n.slice(this.tokenizer.rules.inline.anyPunctuation.lastIndex);for(;(s=this.tokenizer.rules.inline.blockSkip.exec(n))!=null;)n=n.slice(0,s.index)+"["+"a".repeat(s[0].length-2)+"]"+n.slice(this.tokenizer.rules.inline.blockSkip.lastIndex);let i=!1,r="";for(;e;){i||(r=""),i=!1;let o;if(this.options.extensions?.inline?.some(c=>(o=c.call({lexer:this},e,t))?(e=e.substring(o.raw.length),t.push(o),!0):!1))continue;if(o=this.tokenizer.escape(e)){e=e.substring(o.raw.length),t.push(o);continue}if(o=this.tokenizer.tag(e)){e=e.substring(o.raw.length),t.push(o);continue}if(o=this.tokenizer.link(e)){e=e.substring(o.raw.length),t.push(o);continue}if(o=this.tokenizer.reflink(e,this.tokens.links)){e=e.substring(o.raw.length);let c=t.at(-1);o.type==="text"&&c?.type==="text"?(c.raw+=o.raw,c.text+=o.text):t.push(o);continue}if(o=this.tokenizer.emStrong(e,n,r)){e=e.substring(o.raw.length),t.push(o);continue}if(o=this.tokenizer.codespan(e)){e=e.substring(o.raw.length),t.push(o);continue}if(o=this.tokenizer.br(e)){e=e.substring(o.raw.length),t.push(o);continue}if(o=this.tokenizer.del(e)){e=e.substring(o.raw.length),t.push(o);continue}if(o=this.tokenizer.autolink(e)){e=e.substring(o.raw.length),t.push(o);continue}if(!this.state.inLink&&(o=this.tokenizer.url(e))){e=e.substring(o.raw.length),t.push(o);continue}let a=e;if(this.options.extensions?.startInline){let c=1/0,p=e.slice(1),u;this.options.extensions.startInline.forEach(d=>{u=d.call({lexer:this},p),typeof u=="number"&&u>=0&&(c=Math.min(c,u))}),c<1/0&&c>=0&&(a=e.substring(0,c+1))}if(o=this.tokenizer.inlineText(a)){e=e.substring(o.raw.length),o.raw.slice(-1)!=="_"&&(r=o.raw.slice(-1)),i=!0;let c=t.at(-1);c?.type==="text"?(c.raw+=o.raw,c.text+=o.text):t.push(o);continue}if(e){let c="Infinite loop on byte: "+e.charCodeAt(0);if(this.options.silent){console.error(c);break}else throw new Error(c)}}return t}};var $=class{options;parser;constructor(e){this.options=e||w}space(e){return""}code({text:e,lang:t,escaped:n}){let s=(t||"").match(m.notSpaceStart)?.[0],i=e.replace(m.endingNewline,"")+`
`;return s?'<pre><code class="language-'+R(s)+'">'+(n?i:R(i,!0))+`</code></pre>
`:"<pre><code>"+(n?i:R(i,!0))+`</code></pre>
`}blockquote({tokens:e}){return`<blockquote>
${this.parser.parse(e)}</blockquote>
`}html({text:e}){return e}heading({tokens:e,depth:t}){return`<h${t}>${this.parser.parseInline(e)}</h${t}>
`}hr(e){return`<hr>
`}list(e){let t=e.ordered,n=e.start,s="";for(let o=0;o<e.items.length;o++){let a=e.items[o];s+=this.listitem(a)}let i=t?"ol":"ul",r=t&&n!==1?' start="'+n+'"':"";return"<"+i+r+`>
`+s+"</"+i+`>
`}listitem(e){let t="";if(e.task){let n=this.checkbox({checked:!!e.checked});e.loose?e.tokens[0]?.type==="paragraph"?(e.tokens[0].text=n+" "+e.tokens[0].text,e.tokens[0].tokens&&e.tokens[0].tokens.length>0&&e.tokens[0].tokens[0].type==="text"&&(e.tokens[0].tokens[0].text=n+" "+R(e.tokens[0].tokens[0].text),e.tokens[0].tokens[0].escaped=!0)):e.tokens.unshift({type:"text",raw:n+" ",text:n+" ",escaped:!0}):t+=n+" "}return t+=this.parser.parse(e.tokens,!!e.loose),`<li>${t}</li>
`}checkbox({checked:e}){return"<input "+(e?'checked="" ':"")+'disabled="" type="checkbox">'}paragraph({tokens:e}){return`<p>${this.parser.parseInline(e)}</p>
`}table(e){let t="",n="";for(let i=0;i<e.header.length;i++)n+=this.tablecell(e.header[i]);t+=this.tablerow({text:n});let s="";for(let i=0;i<e.rows.length;i++){let r=e.rows[i];n="";for(let o=0;o<r.length;o++)n+=this.tablecell(r[o]);s+=this.tablerow({text:n})}return s&&(s=`<tbody>${s}</tbody>`),`<table>
<thead>
`+t+`</thead>
`+s+`</table>
`}tablerow({text:e}){return`<tr>
${e}</tr>
`}tablecell(e){let t=this.parser.parseInline(e.tokens),n=e.header?"th":"td";return(e.align?`<${n} align="${e.align}">`:`<${n}>`)+t+`</${n}>
`}strong({tokens:e}){return`<strong>${this.parser.parseInline(e)}</strong>`}em({tokens:e}){return`<em>${this.parser.parseInline(e)}</em>`}codespan({text:e}){return`<code>${R(e,!0)}</code>`}br(e){return"<br>"}del({tokens:e}){return`<del>${this.parser.parseInline(e)}</del>`}link({href:e,title:t,tokens:n}){let s=this.parser.parseInline(n),i=V(e);if(i===null)return s;e=i;let r='<a href="'+e+'"';return t&&(r+=' title="'+R(t)+'"'),r+=">"+s+"</a>",r}image({href:e,title:t,text:n,tokens:s}){s&&(n=this.parser.parseInline(s,this.parser.textRenderer));let i=V(e);if(i===null)return R(n);e=i;let r=`<img src="${e}" alt="${n}"`;return t&&(r+=` title="${R(t)}"`),r+=">",r}text(e){return"tokens"in e&&e.tokens?this.parser.parseInline(e.tokens):"escaped"in e&&e.escaped?e.text:R(e.text)}};var _=class{strong({text:e}){return e}em({text:e}){return e}codespan({text:e}){return e}del({text:e}){return e}html({text:e}){return e}text({text:e}){return e}link({text:e}){return""+e}image({text:e}){return""+e}br(){return""}};var b=class l{options;renderer;textRenderer;constructor(e){this.options=e||w,this.options.renderer=this.options.renderer||new $,this.renderer=this.options.renderer,this.renderer.options=this.options,this.renderer.parser=this,this.textRenderer=new _}static parse(e,t){return new l(t).parse(e)}static parseInline(e,t){return new l(t).parseInline(e)}parse(e,t=!0){let n="";for(let s=0;s<e.length;s++){let i=e[s];if(this.options.extensions?.renderers?.[i.type]){let o=i,a=this.options.extensions.renderers[o.type].call({parser:this},o);if(a!==!1||!["space","hr","heading","code","table","blockquote","list","html","paragraph","text"].includes(o.type)){n+=a||"";continue}}let r=i;switch(r.type){case"space":{n+=this.renderer.space(r);continue}case"hr":{n+=this.renderer.hr(r);continue}case"heading":{n+=this.renderer.heading(r);continue}case"code":{n+=this.renderer.code(r);continue}case"table":{n+=this.renderer.table(r);continue}case"blockquote":{n+=this.renderer.blockquote(r);continue}case"list":{n+=this.renderer.list(r);continue}case"html":{n+=this.renderer.html(r);continue}case"paragraph":{n+=this.renderer.paragraph(r);continue}case"text":{let o=r,a=this.renderer.text(o);for(;s+1<e.length&&e[s+1].type==="text";)o=e[++s],a+=`
`+this.renderer.text(o);t?n+=this.renderer.paragraph({type:"paragraph",raw:a,text:a,tokens:[{type:"text",raw:a,text:a,escaped:!0}]}):n+=a;continue}default:{let o='Token with "'+r.type+'" type was not found.';if(this.options.silent)return console.error(o),"";throw new Error(o)}}}return n}parseInline(e,t=this.renderer){let n="";for(let s=0;s<e.length;s++){let i=e[s];if(this.options.extensions?.renderers?.[i.type]){let o=this.options.extensions.renderers[i.type].call({parser:this},i);if(o!==!1||!["escape","html","link","image","strong","em","codespan","br","del","text"].includes(i.type)){n+=o||"";continue}}let r=i;switch(r.type){case"escape":{n+=t.text(r);break}case"html":{n+=t.html(r);break}case"link":{n+=t.link(r);break}case"image":{n+=t.image(r);break}case"strong":{n+=t.strong(r);break}case"em":{n+=t.em(r);break}case"codespan":{n+=t.codespan(r);break}case"br":{n+=t.br(r);break}case"del":{n+=t.del(r);break}case"text":{n+=t.text(r);break}default:{let o='Token with "'+r.type+'" type was not found.';if(this.options.silent)return console.error(o),"";throw new Error(o)}}}return n}};var L=class{options;block;constructor(e){this.options=e||w}static passThroughHooks=new Set(["preprocess","postprocess","processAllTokens"]);preprocess(e){return e}postprocess(e){return e}processAllTokens(e){return e}provideLexer(){return this.block?x.lex:x.lexInline}provideParser(){return this.block?b.parse:b.parseInline}};var E=class{defaults=z();options=this.setOptions;parse=this.parseMarkdown(!0);parseInline=this.parseMarkdown(!1);Parser=b;Renderer=$;TextRenderer=_;Lexer=x;Tokenizer=S;Hooks=L;constructor(...e){this.use(...e)}walkTokens(e,t){let n=[];for(let s of e)switch(n=n.concat(t.call(this,s)),s.type){case"table":{let i=s;for(let r of i.header)n=n.concat(this.walkTokens(r.tokens,t));for(let r of i.rows)for(let o of r)n=n.concat(this.walkTokens(o.tokens,t));break}case"list":{let i=s;n=n.concat(this.walkTokens(i.items,t));break}default:{let i=s;this.defaults.extensions?.childTokens?.[i.type]?this.defaults.extensions.childTokens[i.type].forEach(r=>{let o=i[r].flat(1/0);n=n.concat(this.walkTokens(o,t))}):i.tokens&&(n=n.concat(this.walkTokens(i.tokens,t)))}}return n}use(...e){let t=this.defaults.extensions||{renderers:{},childTokens:{}};return e.forEach(n=>{let s={...n};if(s.async=this.defaults.async||s.async||!1,n.extensions&&(n.extensions.forEach(i=>{if(!i.name)throw new Error("extension name required");if("renderer"in i){let r=t.renderers[i.name];r?t.renderers[i.name]=function(...o){let a=i.renderer.apply(this,o);return a===!1&&(a=r.apply(this,o)),a}:t.renderers[i.name]=i.renderer}if("tokenizer"in i){if(!i.level||i.level!=="block"&&i.level!=="inline")throw new Error("extension level must be 'block' or 'inline'");let r=t[i.level];r?r.unshift(i.tokenizer):t[i.level]=[i.tokenizer],i.start&&(i.level==="block"?t.startBlock?t.startBlock.push(i.start):t.startBlock=[i.start]:i.level==="inline"&&(t.startInline?t.startInline.push(i.start):t.startInline=[i.start]))}"childTokens"in i&&i.childTokens&&(t.childTokens[i.name]=i.childTokens)}),s.extensions=t),n.renderer){let i=this.defaults.renderer||new $(this.defaults);for(let r in n.renderer){if(!(r in i))throw new Error(`renderer '${r}' does not exist`);if(["options","parser"].includes(r))continue;let o=r,a=n.renderer[o],c=i[o];i[o]=(...p)=>{let u=a.apply(i,p);return u===!1&&(u=c.apply(i,p)),u||""}}s.renderer=i}if(n.tokenizer){let i=this.defaults.tokenizer||new S(this.defaults);for(let r in n.tokenizer){if(!(r in i))throw new Error(`tokenizer '${r}' does not exist`);if(["options","rules","lexer"].includes(r))continue;let o=r,a=n.tokenizer[o],c=i[o];i[o]=(...p)=>{let u=a.apply(i,p);return u===!1&&(u=c.apply(i,p)),u}}s.tokenizer=i}if(n.hooks){let i=this.defaults.hooks||new L;for(let r in n.hooks){if(!(r in i))throw new Error(`hook '${r}' does not exist`);if(["options","block"].includes(r))continue;let o=r,a=n.hooks[o],c=i[o];L.passThroughHooks.has(r)?i[o]=p=>{if(this.defaults.async)return Promise.resolve(a.call(i,p)).then(d=>c.call(i,d));let u=a.call(i,p);return c.call(i,u)}:i[o]=(...p)=>{let u=a.apply(i,p);return u===!1&&(u=c.apply(i,p)),u}}s.hooks=i}if(n.walkTokens){let i=this.defaults.walkTokens,r=n.walkTokens;s.walkTokens=function(o){let a=[];return a.push(r.call(this,o)),i&&(a=a.concat(i.call(this,o))),a}}this.defaults={...this.defaults,...s}}),this}setOptions(e){return this.defaults={...this.defaults,...e},this}lexer(e,t){return x.lex(e,t??this.defaults)}parser(e,t){return b.parse(e,t??this.defaults)}parseMarkdown(e){return(n,s)=>{let i={...s},r={...this.defaults,...i},o=this.onError(!!r.silent,!!r.async);if(this.defaults.async===!0&&i.async===!1)return o(new Error("marked(): The async option was set to true by an extension. Remove async: false from the parse options object to return a Promise."));if(typeof n>"u"||n===null)return o(new Error("marked(): input parameter is undefined or null"));if(typeof n!="string")return o(new Error("marked(): input parameter is of type "+Object.prototype.toString.call(n)+", string expected"));r.hooks&&(r.hooks.options=r,r.hooks.block=e);let a=r.hooks?r.hooks.provideLexer():e?x.lex:x.lexInline,c=r.hooks?r.hooks.provideParser():e?b.parse:b.parseInline;if(r.async)return Promise.resolve(r.hooks?r.hooks.preprocess(n):n).then(p=>a(p,r)).then(p=>r.hooks?r.hooks.processAllTokens(p):p).then(p=>r.walkTokens?Promise.all(this.walkTokens(p,r.walkTokens)).then(()=>p):p).then(p=>c(p,r)).then(p=>r.hooks?r.hooks.postprocess(p):p).catch(o);try{r.hooks&&(n=r.hooks.preprocess(n));let p=a(n,r);r.hooks&&(p=r.hooks.processAllTokens(p)),r.walkTokens&&this.walkTokens(p,r.walkTokens);let u=c(p,r);return r.hooks&&(u=r.hooks.postprocess(u)),u}catch(p){return o(p)}}}onError(e,t){return n=>{if(n.message+=`
Please report this to https://github.com/markedjs/marked.`,e){let s="<p>An error occurred:</p><pre>"+R(n.message+"",!0)+"</pre>";return t?Promise.resolve(s):s}if(t)return Promise.reject(n);throw n}}};var M=new E;function k(l,e){return M.parse(l,e)}k.options=k.setOptions=function(l){return M.setOptions(l),k.defaults=M.defaults,N(k.defaults),k};k.getDefaults=z;k.defaults=w;k.use=function(...l){return M.use(...l),k.defaults=M.defaults,N(k.defaults),k};k.walkTokens=function(l,e){return M.walkTokens(l,e)};k.parseInline=M.parseInline;k.Parser=b;k.parser=b.parse;k.Renderer=$;k.TextRenderer=_;k.Lexer=x;k.lexer=x.lex;k.Tokenizer=S;k.Hooks=L;k.parse=k;var it=k.options,ot=k.setOptions,lt=k.use,at=k.walkTokens,ct=k.parseInline,pt=k,ut=b.parse,ht=x.lex;

if(__exports != exports)module.exports = exports;return module.exports}));
"##;

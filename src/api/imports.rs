use crate::models::{normalize_departments, AppState, ImportPayload};
use axum::{extract::State, response::IntoResponse, Json};

pub async fn import_data(
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
        let departments = match c.departments.clone().filter(|s| !s.trim().is_empty()) {
            Some(d) => normalize_departments(Some(d), c.department.clone()),
            None => normalize_departments(None, department.clone()),
        };
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
            "INSERT INTO contacts (name, department, departments, phone, email, description) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&name)
        .bind(department)
        .bind(departments)
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
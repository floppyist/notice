use crate::models::{AppState, CreateNote, Note, SearchQuery, UpdateNote};
use crate::recurrence;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use sqlx::SqlitePool;

async fn resolve_department(pool: &SqlitePool, department: Option<String>) -> Option<String> {
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

pub async fn get_notes(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Note>("SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned FROM notes WHERE deleted_at IS NULL AND status != 'archived' ORDER BY sort_order ASC, id ASC")
        .fetch_all(&state.pool)
        .await
    {
        Ok(notes) => Json(notes).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn search_notes(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let q = params.q.unwrap_or_default().trim().to_lowercase();
    if q.is_empty() {
        match sqlx::query_as::<_, (i64, String)>(
            "SELECT id, title FROM notes WHERE deleted_at IS NULL AND status != 'archived' ORDER BY id DESC LIMIT 8",
        )
        .fetch_all(&state.pool)
        .await
        {
            Ok(results) => {
                let json: Vec<serde_json::Value> = results
                    .into_iter()
                    .map(|(id, title)| serde_json::json!({ "id": id, "title": title }))
                    .collect();
                return Json(json).into_response();
            }
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
    let pattern = format!("%{}%", q);
    match sqlx::query_as::<_, (i64, String)>(
        "SELECT id, title FROM notes WHERE LOWER(title) LIKE ? AND deleted_at IS NULL AND status != 'archived' ORDER BY sort_order ASC LIMIT 8",
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

pub async fn create_note(
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

pub async fn update_note(
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
                let (nd, na) = recurrence::apply_recurrence(rule, due_date.clone(), appointments.clone());
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

pub async fn delete_note(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
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

pub async fn restore_note(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("UPDATE notes SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL")
        .bind(id)
        .execute(&state.pool)
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn force_delete_note(
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

pub async fn duplicate_note(
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

pub async fn archive_note(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let note = match sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ? AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let res = sqlx::query(
        r#"
        INSERT INTO notes (title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
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
    .execute(&state.archive_pool)
    .await;

    if res.is_ok() {
        let _ = sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(id)
            .execute(&state.pool)
            .await;
        StatusCode::OK.into_response()
    } else {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}
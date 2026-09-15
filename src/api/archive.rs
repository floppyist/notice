use crate::models::{AppState, Note};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

pub async fn get_archived(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Note>(
        "SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, deleted_at, repeat_rule, pinned FROM notes ORDER BY completed_at DESC, id DESC",
    )
    .fetch_all(&state.archive_pool)
    .await
    {
        Ok(notes) => Json(notes).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn restore_from_archive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let note = match sqlx::query_as::<_, Note>(
        "SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, deleted_at, repeat_rule, pinned FROM notes WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.archive_pool)
    .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let max_sort: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(sort_order) FROM notes WHERE status = ? AND deleted_at IS NULL",
    )
    .bind(&note.status)
    .fetch_one(&state.pool)
    .await
    .ok()
    .flatten();
    let sort_order = max_sort.unwrap_or(0) + 1;

    let res = sqlx::query(
        r#"
        INSERT INTO notes (title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
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
    .bind(sort_order)
    .bind(&note.repeat_rule)
    .bind(note.pinned)
    .execute(&state.pool)
    .await;

    if res.is_ok() {
        let _ = sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(id)
            .execute(&state.archive_pool)
            .await;
        StatusCode::OK.into_response()
    } else {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

pub async fn force_delete_archived(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let res = sqlx::query("DELETE FROM notes WHERE id = ?")
        .bind(id)
        .execute(&state.archive_pool)
        .await;
    if res.is_ok() {
        StatusCode::OK.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

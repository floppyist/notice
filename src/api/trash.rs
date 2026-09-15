use crate::models::{AppState, Contact, Note, PurgePayload};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};

pub async fn get_trash(State(state): State<AppState>) -> impl IntoResponse {
    let notes = sqlx::query_as::<_, Note>(
        "SELECT id, title, content, status, priority, date, due_date, department, departments, appointments, sort_order, completed_at, repeat_rule, pinned FROM notes WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, id DESC",
    )
    .fetch_all(&state.pool)
    .await;
    let contacts = sqlx::query_as::<_, Contact>(
        "SELECT id, name, title, department, departments, phone, mobile, fax, email, description FROM contacts WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, id DESC",
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

pub async fn clear_trash(State(state): State<AppState>) -> impl IntoResponse {
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

pub async fn purge_trash(
    State(state): State<AppState>,
    Json(payload): Json<PurgePayload>,
) -> impl IntoResponse {
    let days = payload.days.max(0);
    let cutoff = chrono::Local::now() - chrono::Duration::days(days);
    let cutoff_str = cutoff.format("%Y-%m-%d %H:%M:%S").to_string();
    let notes_res = sqlx::query(
        "DELETE FROM notes WHERE deleted_at IS NOT NULL AND deleted_at <= ?",
    )
    .bind(&cutoff_str)
    .execute(&state.pool)
    .await;
    let contacts_res = sqlx::query(
        "DELETE FROM contacts WHERE deleted_at IS NOT NULL AND deleted_at <= ?",
    )
    .bind(&cutoff_str)
    .execute(&state.db)
    .await;
    if notes_res.is_ok() && contacts_res.is_ok() {
        StatusCode::OK.into_response()
    } else {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}
use crate::models::{AppState, CreateWiki, SearchQuery, UpdateWiki, WikiPage};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

pub async fn get_wiki_pages(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, WikiPage>(
        "SELECT id, title, content, created_at, updated_at FROM wiki ORDER BY LOWER(title) ASC, id ASC",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(pages) => Json(pages).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn create_wiki_page(
    State(state): State<AppState>,
    Json(payload): Json<CreateWiki>,
) -> impl IntoResponse {
    let title = payload.title.trim().to_string();
    if title.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let content = payload
        .content
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let res = sqlx::query(
        "INSERT INTO wiki (title, content, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&title)
    .bind(&content)
    .bind(&now)
    .bind(&now)
    .execute(&state.pool)
    .await;
    match res {
        Ok(result) => {
            // Read the created row back on the SAME connection so last_insert_rowid()
            // is still valid (querying it on a fresh pooled connection returns 0).
            let row_pool = result;
            let new_id: i64 = row_pool.last_insert_rowid();
            match sqlx::query_as::<_, WikiPage>(
                "SELECT id, title, content, created_at, updated_at FROM wiki WHERE id = ?",
            )
            .bind(new_id)
            .fetch_one(&state.pool)
            .await
            {
                Ok(page) => (StatusCode::OK, Json(page)).into_response(),
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        Err(_) => StatusCode::CONFLICT.into_response(),
    }
}

pub async fn update_wiki_page(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateWiki>,
) -> impl IntoResponse {
    let existing = match sqlx::query_as::<_, WikiPage>(
        "SELECT id, title, content, created_at, updated_at FROM wiki WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(Some(p)) => p,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let title = match payload.title {
        Some(s) => {
            let t = s.trim().to_string();
            if t.is_empty() {
                return StatusCode::BAD_REQUEST.into_response();
            }
            t
        }
        None => existing.title,
    };
    let content = match payload.content {
        Some(s) => {
            let t = s.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        }
        None => existing.content,
    };
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let res = sqlx::query(
        "UPDATE wiki SET title = ?, content = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&title)
    .bind(&content)
    .bind(&now)
    .bind(id)
    .execute(&state.pool)
    .await;
    match res {
        Ok(_) => {
            match sqlx::query_as::<_, WikiPage>(
                "SELECT id, title, content, created_at, updated_at FROM wiki WHERE id = ?",
            )
            .bind(id)
            .fetch_one(&state.pool)
            .await
            {
                Ok(page) => Json(page).into_response(),
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        Err(_) => StatusCode::CONFLICT.into_response(),
    }
}

pub async fn delete_wiki_page(State(state): State<AppState>, Path(id): Path<i64>) -> impl IntoResponse {
    match sqlx::query("DELETE FROM wiki WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
    {
        Ok(rows) if rows.rows_affected() > 0 => StatusCode::OK.into_response(),
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn search_wiki(State(state): State<AppState>, Query(params): Query<SearchQuery>) -> impl IntoResponse {
    let q = params.q.unwrap_or_default().trim().to_string();
    if q.is_empty() {
        match sqlx::query_as::<_, (i64, String)>(
            "SELECT id, title FROM wiki ORDER BY id DESC LIMIT 8",
        )
        .fetch_all(&state.pool)
        .await
        {
            Ok(results) => {
                let json: Vec<serde_json::Value> = results
                    .into_iter()
                    .map(|(id, title)| serde_json::json!({ "id": id, "name": title }))
                    .collect();
                return Json(json).into_response();
            }
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
    let pattern = format!("%{}%", q);
    match sqlx::query_as::<_, (i64, String)>(
        "SELECT id, title FROM wiki WHERE LOWER(title) LIKE ? ORDER BY LOWER(title) ASC LIMIT 8",
    )
    .bind(pattern)
    .fetch_all(&state.pool)
    .await
    {
        Ok(results) => {
            let json: Vec<serde_json::Value> = results
                .into_iter()
                .map(|(id, title)| serde_json::json!({ "id": id, "name": title }))
                .collect();
            Json(json).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
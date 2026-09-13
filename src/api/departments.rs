use crate::models::{AppState, CreateDepartment, DeleteDepartment};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};

pub async fn get_departments(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT name, cnt FROM (
            SELECT dep.value AS name, COUNT(*) AS cnt, LOWER(dep.value) AS lname
            FROM notes,
                 json_each(COALESCE(notes.departments, '[]')) AS dep
            WHERE json_type(COALESCE(notes.departments, '[]')) IS NOT NULL
              AND notes.deleted_at IS NULL
              AND TRIM(dep.value) != ''
            GROUP BY LOWER(dep.value)
            UNION
            SELECT s.name AS name, 0 AS cnt, LOWER(s.name) AS lname
            FROM departments_store s
            WHERE TRIM(s.name) != ''
        )
        GROUP BY lname
        ORDER BY cnt DESC, LOWER(name) ASC
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

pub async fn create_department(
    State(state): State<AppState>,
    Json(payload): Json<CreateDepartment>,
) -> impl IntoResponse {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    // Case-insensitive duplicate check
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM departments_store WHERE LOWER(TRIM(name)) = LOWER(?) LIMIT 1",
    )
    .bind(&name)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    if existing.is_some() {
        return StatusCode::CONFLICT.into_response();
    }
    let res = sqlx::query("INSERT INTO departments_store (name) VALUES (?)")
        .bind(&name)
        .execute(&state.pool)
        .await;
    match res {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn delete_department(
    State(state): State<AppState>,
    Json(payload): Json<DeleteDepartment>,
) -> impl IntoResponse {
    let name = payload.name.trim().to_lowercase();
    if name.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    // Remove the department from the DATEN-View store
    let store_result = sqlx::query("DELETE FROM departments_store WHERE LOWER(TRIM(name)) = LOWER(?)")
        .bind(&name)
        .execute(&state.pool)
        .await;

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

    // Update the contacts db: remove the department from every contact's departments JSON array
    // and clear the legacy single-value department column where it matches.
    let contacts_result = sqlx::query(
        r#"
        UPDATE contacts
        SET departments = (
            SELECT COALESCE(json_group_array(e.value), '[]')
            FROM json_each(COALESCE(contacts.departments, '[]')) AS e
            WHERE LOWER(TRIM(e.value)) != LOWER(?)
              AND TRIM(e.value) != ''
        ),
        department = CASE WHEN LOWER(TRIM(department)) = LOWER(?) THEN NULL ELSE department END
        "#,
    )
    .bind(&name)
    .bind(&name)
    .execute(&state.db)
    .await;

    if store_result.is_err() || notes_result.is_err() || contacts_result.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Return the freshly aggregated department list
    match sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT name, cnt FROM (
            SELECT dep.value AS name, COUNT(*) AS cnt, LOWER(dep.value) AS lname
            FROM notes,
                 json_each(COALESCE(notes.departments, '[]')) AS dep
            WHERE json_type(COALESCE(notes.departments, '[]')) IS NOT NULL
              AND notes.deleted_at IS NULL
              AND TRIM(dep.value) != ''
            GROUP BY LOWER(dep.value)
            UNION
            SELECT s.name AS name, 0 AS cnt, LOWER(s.name) AS lname
            FROM departments_store s
            WHERE TRIM(s.name) != ''
        )
        GROUP BY lname
        ORDER BY cnt DESC, LOWER(name) ASC
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
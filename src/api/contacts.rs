use crate::models::{
    first_department, normalize_departments, AppState, Contact, CreateContact, SearchQuery,
    UpdateContact,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

pub async fn get_contacts(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Contact>(
        "SELECT id, name, department, departments, phone, email, description FROM contacts WHERE deleted_at IS NULL ORDER BY LOWER(name) ASC, id ASC",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(contacts) => Json(contacts).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn create_contact(
    State(state): State<AppState>,
    Json(payload): Json<CreateContact>,
) -> impl IntoResponse {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let departments = normalize_departments(
        payload.departments.map(|s| s.trim().to_string()),
        payload.department,
    );
    let department = first_department(&Some(departments.clone()));
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
        "INSERT INTO contacts (name, department, departments, phone, email, description) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(name)
    .bind(department)
    .bind(departments)
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

pub async fn update_contact(
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

    // Build departments JSON array from the sent value, the legacy single department field,
    // or the previously stored departments array — in that priority order.
    let departments = match payload.departments {
        Some(s) if !s.trim().is_empty() => normalize_departments(Some(s), None),
        _ => match existing.departments {
            Some(d) if !d.trim().is_empty() && d.trim() != "[]" => d,
            _ => normalize_departments(None, payload.department.clone().or(existing.department.clone())),
        },
    };
    // Derive legacy department from the first element of the resolved array.
    let department = first_department(&Some(departments.clone()));

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
        "UPDATE contacts SET name = ?, department = ?, departments = ?, phone = ?, email = ?, description = ? WHERE id = ?",
    )
    .bind(name)
    .bind(department)
    .bind(departments)
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

pub async fn delete_contact(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
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

pub async fn restore_contact(
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

pub async fn force_delete_contact(
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

pub async fn search_contacts(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let q = params.q.unwrap_or_default().trim().to_lowercase();
    if q.is_empty() {
        match sqlx::query_as::<_, (i64, String)>(
            "SELECT id, name FROM contacts WHERE deleted_at IS NULL ORDER BY id DESC LIMIT 8",
        )
        .fetch_all(&state.db)
        .await
        {
            Ok(results) => {
                let json: Vec<serde_json::Value> = results
                    .into_iter()
                    .map(|(id, name)| serde_json::json!({ "id": id, "name": name }))
                    .collect();
                return Json(json).into_response();
            }
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
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
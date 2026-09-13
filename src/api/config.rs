use crate::config::Config;
use crate::models::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};

pub async fn get_config(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.lock().await;
    Json(json!({
        "config": &*cfg,
        "created_fresh": state.created_fresh,
        "path": Config::FILE
    }))
    .into_response()
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(patch): Json<Value>,
) -> impl IntoResponse {
    if !patch.is_object() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut cfg = state.config.lock().await;
    let base = serde_json::to_value(&*cfg).unwrap_or_else(|_| json!({}));
    let merged = Config::merge_json(&base, &patch);
    let new_cfg = match serde_json::from_value::<Config>(merged) {
        Ok(c) => c,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    if !Config::LANGUAGES.contains(&new_cfg.app.language.as_str()) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !Config::THEMES.contains(&new_cfg.app.theme.as_str()) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if new_cfg.app.auto_archive_day < 1 || new_cfg.app.auto_archive_day > 31 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if new_cfg.app.trash_purge_day < 1 || new_cfg.app.trash_purge_day > 365 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if new_cfg.save(Config::FILE).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    *cfg = new_cfg.clone();
    Json(json!({ "config": new_cfg })).into_response()
}
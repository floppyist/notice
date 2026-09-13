use crate::config::Config;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub db: SqlitePool,
    pub config: Arc<Mutex<Config>>,
    pub created_fresh: bool,
}

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub status: String,
    pub priority: String,
    pub date: String,
    pub due_date: Option<String>,
    pub department: Option<String>,
    pub departments: Option<String>,
    pub appointments: Option<String>,
    pub sort_order: i64,
    pub completed_at: Option<String>,
    pub repeat_rule: Option<String>,
    pub pinned: i64,
}

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct Contact {
    pub id: i64,
    pub name: String,
    pub department: Option<String>,
    pub departments: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateContact {
    pub name: String,
    pub department: Option<String>,
    pub departments: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateContact {
    pub name: Option<String>,
    pub department: Option<String>,
    pub departments: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct DeleteDepartment {
    pub name: String,
}

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct WikiPage {
    pub id: i64,
    pub title: String,
    pub content: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateWiki {
    pub title: String,
    pub content: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateWiki {
    pub title: Option<String>,
    pub content: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateNote {
    pub title: String,
    pub content: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub due_date: Option<String>,
    pub department: Option<String>,
    pub departments: Option<String>,
    pub appointments: Option<String>,
    pub repeat_rule: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateNote {
    pub title: Option<String>,
    pub content: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub due_date: Option<String>,
    pub department: Option<String>,
    pub departments: Option<String>,
    pub appointments: Option<String>,
    pub sort_order: Option<i64>,
    pub repeat_rule: Option<String>,
    pub pinned: Option<Option<bool>>,
}

#[derive(Deserialize, Clone)]
pub struct ImportNote {
    pub title: String,
    pub content: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub date: Option<String>,
    pub due_date: Option<String>,
    pub department: Option<String>,
    pub departments: Option<String>,
    pub appointments: Option<String>,
    pub repeat_rule: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct ImportContact {
    pub name: String,
    pub department: Option<String>,
    pub departments: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct ImportPayload {
    pub mode: Option<String>,
    pub notes: Option<Vec<ImportNote>>,
    pub contacts: Option<Vec<ImportContact>>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

#[derive(Deserialize)]
pub struct PurgePayload {
    pub days: i64,
}

#[derive(Deserialize)]
pub struct CreateDepartment {
    pub name: String,
}

// Returns the first department name of a JSON-array string (e.g. `["Küche","Bad"]`).
pub fn first_department(departments: &Option<String>) -> Option<String> {
    let raw = departments.as_deref()?;
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    let arr = parsed.as_array()?;
    arr.iter()
        .filter_map(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
}

// Normalizes a departments JSON-array input into a canonical JSON-array string ("[]" when empty).
pub fn normalize_departments(departments: Option<String>, first: Option<String>) -> String {
    match departments {
        Some(raw) => {
            let trimmed = raw.trim().to_string();
            if trimmed.is_empty() {
                "[]".to_string()
            } else {
                match serde_json::from_str::<serde_json::Value>(&trimmed) {
                    Ok(v) if v.is_array() => {
                        serde_json::to_string(&v).unwrap_or_else(|_| "[]".to_string())
                    }
                    _ => "[]".to_string(),
                }
            }
        }
        None => match first {
            Some(name) => {
                let cleaned = name.trim().to_string();
                if cleaned.is_empty() {
                    "[]".to_string()
                } else {
                    serde_json::to_string(&vec![cleaned]).unwrap_or_else(|_| "[]".to_string())
                }
            }
            None => "[]".to_string(),
        },
    }
}
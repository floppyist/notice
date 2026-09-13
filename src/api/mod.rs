use crate::models::AppState;
use axum::{routing::{delete, get, post, put}, Router};

pub mod config;
pub mod contacts;
pub mod departments;
pub mod imports;
pub mod notes;
pub mod trash;
pub mod wiki;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(crate::frontend::index_handler))
        .route("/api/notes", get(notes::get_notes).post(notes::create_note))
        .route("/api/notes/search", get(notes::search_notes))
        .route(
            "/api/departments",
            get(departments::get_departments).post(departments::create_department),
        )
        .route("/api/wiki", get(wiki::get_wiki_pages).post(wiki::create_wiki_page))
        .route("/api/wiki/search", get(wiki::search_wiki))
        .route(
            "/api/wiki/:id",
            put(wiki::update_wiki_page).delete(wiki::delete_wiki_page),
        )
        .route(
            "/api/departments/delete",
            post(departments::delete_department),
        )
        .route(
            "/api/notes/:id",
            post(notes::update_note).put(notes::update_note).delete(notes::delete_note),
        )
        .route("/api/notes/:id/duplicate", post(notes::duplicate_note))
        .route("/api/notes/:id/restore", post(notes::restore_note))
        .route("/api/notes/:id/force", delete(notes::force_delete_note))
        .route("/api/import", post(imports::import_data))
        .route("/api/trash", get(trash::get_trash))
        .route("/api/trash/clear", post(trash::clear_trash))
        .route("/api/trash/purge", post(trash::purge_trash))
        .route("/api/config", get(config::get_config).put(config::update_config))
        .route("/api/contacts", get(contacts::get_contacts).post(contacts::create_contact))
        .route("/api/contacts/search", get(contacts::search_contacts))
        .route(
            "/api/contacts/:id",
            put(contacts::update_contact).delete(contacts::delete_contact),
        )
        .route("/api/contacts/:id/restore", post(contacts::restore_contact))
        .route("/api/contacts/:id/force", delete(contacts::force_delete_contact))
        .with_state(state)
}
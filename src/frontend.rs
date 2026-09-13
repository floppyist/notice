use crate::i18n;
use crate::models::AppState;
use axum::{extract::State, response::Html};

pub async fn index_handler(State(state): State<AppState>) -> Html<String> {
    let cfg = state.config.lock().await;
    let initial_cfg = serde_json::to_string(&serde_json::json!({
        "config": &*cfg,
        "created_fresh": state.created_fresh
    }))
    .unwrap_or_else(|_| "{}".to_string());
    let langs = i18n::all_json();

    let html = include_str!("../frontend/index.html")
        .replace("__HTML_LANG__", &cfg.app.language)
        .replace("__HTML_CLASS__", &cfg.app.theme)
        .replace("__TAILWIND_JS__", include_str!("../frontend/vendor/tailwind.js"))
        .replace("__VUE_JS__", include_str!("../frontend/vendor/vue.min.js"))
        .replace("__MARKED_JS__", include_str!("../frontend/vendor/marked.min.js"))
        .replace("__APP_JS__", include_str!("../frontend/app.js"))
        .replace("__INITIAL_CONFIG_JSON__", &initial_cfg)
        .replace("__I18N_JSON__", &langs);
    Html(html)
}
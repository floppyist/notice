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
        .replace("__HTML_CLASS__", "dark")
        .replace("__TAILWIND_JS__", include_str!("../frontend/vendor/tailwind.js"))
        .replace("__VUE_JS__", include_str!("../frontend/vendor/vue.min.js"))
        .replace("__MARKED_JS__", include_str!("../frontend/vendor/marked.min.js"))
        .replace("__THEME_CSS__", include_str!("../frontend/theme.css"))
        .replace("__APP_JS__", include_str!("../frontend/app.js"))
        .replace("__INITIAL_CONFIG_JSON__", &initial_cfg)
        .replace("__I18N_JSON__", &langs);
    Html(html)
}

/// Eingebettete Schrift (JetBrainsMono Nerd Font, Subset), siehe support/make_font.sh
pub async fn font_handler(axum::extract::Path(name): axum::extract::Path<String>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let bytes: &'static [u8] = match name.as_str() {
        "jbm-Regular.woff2" => include_bytes!("../frontend/fonts/jbm-Regular.woff2"),
        "jbm-Bold.woff2" => include_bytes!("../frontend/fonts/jbm-Bold.woff2"),
        _ => return axum::http::StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (axum::http::header::CONTENT_TYPE, "font/woff2"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response()
}

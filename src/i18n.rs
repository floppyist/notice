use serde_json::{json, Value};

fn load(name: &str) -> Value {
    let raw = match name {
        "en" => include_str!("../frontend/i18n/en.json"),
        "es" => include_str!("../frontend/i18n/es.json"),
        "fr" => include_str!("../frontend/i18n/fr.json"),
        _ => include_str!("../frontend/i18n/de.json"),
    };
    serde_json::from_str(raw).unwrap_or_else(|_| json!({}))
}

/// Build the full { de: {…}, en: {…}, es: {…}, fr: {…} } object injected into
/// the page as `window.__I18N__`. All languages are embedded at compile time so
/// the app works fully offline and can switch languages without a round trip.
pub fn all_json() -> String {
    json!({
        "de": load("de"),
        "en": load("en"),
        "es": load("es"),
        "fr": load("fr"),
    })
    .to_string()
}
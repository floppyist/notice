use serde_json::{json, Map, Value};

fn load(name: &str) -> Value {
    let raw = match name {
        "en" => include_str!("../frontend/i18n/en.json"),
        "es" => include_str!("../frontend/i18n/es.json"),
        "fr" => include_str!("../frontend/i18n/fr.json"),
        _ => include_str!("../frontend/i18n/de.json"),
    };
    serde_json::from_str(raw).unwrap_or_else(|_| json!({}))
}

/// True when an object is a plural map (`{"one": …, "other": …}`) that must be
/// kept as a single value instead of being flattened into dotted keys.
fn is_plural(map: &Map<String, Value>) -> bool {
    map.contains_key("one") && map.contains_key("other")
}

fn flatten_into(value: &Value, prefix: &str, out: &mut Map<String, Value>) {
    match value {
        Value::Object(map) if !is_plural(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_into(v, &key, out);
            }
        }
        _ => {
            out.insert(prefix.to_string(), value.clone());
        }
    }
}

/// Flatten one locale dict into dotted keys (`{"nav": {"board": …}}` →
/// `{"nav.board": …}`) matching the flat lookups of the frontend `t()`.
fn flatten(value: &Value) -> Value {
    let mut out = Map::new();
    flatten_into(value, "", &mut out);
    Value::Object(out)
}

/// Build the full { de: {…}, en: {…}, es: {…}, fr: {…} } object injected into
/// the page as `window.__I18N__`. All languages are embedded at compile time so
/// the app works fully offline and can switch languages without a round trip.
pub fn all_json() -> String {
    json!({
        "de": flatten(&load("de")),
        "en": flatten(&load("en")),
        "es": flatten(&load("es")),
        "fr": flatten(&load("fr")),
    })
    .to_string()
}
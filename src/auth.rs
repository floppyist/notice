//! Token-Authentifizierung für den Mehrgeräte-Betrieb (zentraler Server).
//!
//! Ist ein Token gesetzt (`server.auth_token` in config.toml oder Umgebungsvariable `NOTICE_TOKEN`),
//! verlangen alle Routen entweder `Authorization: Bearer <token>` (Android/Skripte) oder das
//! Cookie `notice_token` (Browser, wird über `/login` gesetzt). TLS gehört davor
//! (Reverse-Proxy wie Caddy/nginx oder ein VPN wie Tailscale/WireGuard).

use crate::models::AppState;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use serde::Deserialize;
use std::time::Duration;

const COOKIE: &str = "notice_token";

/// Zeitkonstanter Vergleich (verhindert Timing-Angriffe auf das Token).
fn eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (*a.get(i).unwrap_or(&0) ^ *b.get(i).unwrap_or(&0)) as usize;
    }
    diff == 0
}

fn cookie_value(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').filter_map(|p| p.trim().split_once('=')).find(|(k, _)| *k == COOKIE).map(|(_, v)| v.to_string())
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    if let Some(v) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(t) = v.strip_prefix("Bearer ") {
            if eq(t.trim(), token) {
                return true;
            }
        }
    }
    cookie_value(headers).map_or(false, |c| eq(&c, token))
}

pub async fn require_token(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let Some(token) = state.token.as_deref() else { return next.run(req).await };
    let path = req.uri().path();
    if path == "/login" || authorized(req.headers(), token) {
        return next.run(req).await;
    }
    if path.starts_with("/api/") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Redirect::to("/login").into_response()
}

pub async fn login_page() -> Html<&'static str> {
    Html(LOGIN_HTML)
}

#[derive(Deserialize)]
pub struct LoginForm {
    token: String,
}

pub async fn login_submit(State(state): State<AppState>, headers: HeaderMap, Form(form): Form<LoginForm>) -> Response {
    let Some(token) = state.token.as_deref() else { return Redirect::to("/").into_response() };
    if !eq(form.token.trim(), token) {
        // Bremst Brute-Force-Versuche aus.
        tokio::time::sleep(Duration::from_millis(800)).await;
        return (StatusCode::UNAUTHORIZED, Html(LOGIN_HTML.replace("<!--ERR-->", "<p class=e>Token ung&uuml;ltig</p>"))).into_response();
    }
    let secure = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    let cookie = format!(
        "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000{}",
        if secure { "; Secure" } else { "" }
    );
    let mut res = Redirect::to("/").into_response();
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        res.headers_mut().insert(header::SET_COOKIE, v);
    }
    res
}

/// 24 Zufallsbytes als Hex (aus /dev/urandom) – für `notice --gen-token`.
pub fn generate_token() -> std::io::Result<String> {
    use std::io::Read;
    let mut buf = [0u8; 24];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

const LOGIN_HTML: &str = r#"<!doctype html><html><head><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1">
<title>NOTICE</title><style>
body{background:#191c1c;color:#d4d4c8;font-family:monospace;display:flex;min-height:100vh;align-items:center;justify-content:center;margin:0}
form{border-left:4px solid #e4572e;background:#212525;padding:24px;display:flex;flex-direction:column;gap:12px;width:300px}
h1{margin:0;font-size:18px;letter-spacing:2px}input,button{padding:10px;font:inherit;border:1px solid #3a4040;background:#191c1c;color:inherit;border-radius:0}
button{background:#e4572e;color:#191c1c;font-weight:bold;cursor:pointer}.e{color:#e4572e;margin:0}
</style></head><body><form method=post action=/login><h1>NOTICE</h1><!--ERR-->
<input type=password name=token placeholder="Token" autofocus required><button>Anmelden</button></form></body></html>"#;

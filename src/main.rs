mod api;
mod auth;
mod config;
mod db;
mod frontend;
mod i18n;
mod models;
mod recurrence;
mod tui;

use config::Config;
use models::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Parses `--config <path>` (default "config.toml"), `--port <u16>`
/// (overrides the configured server port, used by the Android wrapper) and
/// `--host <ip>`, `--gen-token` (prints a fresh access token) and
/// `--tui` (starts the terminal interface instead of the web server).
fn parse_args() -> (String, Option<u16>, Option<String>, bool) {
    let mut config_file = String::from(Config::FILE);
    let mut port = None;
    let mut host = None;
    let mut tui_mode = false;
    let mut i = 1;
    while i < std::env::args().len() {
        match std::env::args().nth(i).as_deref() {
            Some("--config") => {
                if let Some(v) = std::env::args().nth(i + 1) {
                    config_file = v;
                    i += 1;
                }
            }
            Some("--port") => {
                if let Some(v) = std::env::args().nth(i + 1) {
                    port = v.parse::<u16>().ok();
                    i += 1;
                }
            }
            Some("--host") => {
                if let Some(v) = std::env::args().nth(i + 1) {
                    host = Some(v);
                    i += 1;
                }
            }
            Some("--gen-token") => {
                match auth::generate_token() {
                    Ok(t) => println!("{t}"),
                    Err(e) => eprintln!("Token konnte nicht erzeugt werden: {e}"),
                }
                std::process::exit(0);
            }
            Some("--tui") => tui_mode = true,
            _ => {}
        }
        i += 1;
    }
    (config_file, port, host, tui_mode)
}

#[tokio::main]
async fn main() {
    let (config_file, port_override, host_override, tui_mode) = parse_args();

    let (mut cfg, created_fresh) = Config::load(&config_file);
    if let Some(port) = port_override {
        cfg.server.port = port;
    }
    if let Some(host) = host_override {
        cfg.server.host = host;
    }

    // Token: Umgebungsvariable hat Vorrang vor config.toml (wird nie zurückgeschrieben).
    let token: Option<std::sync::Arc<str>> = std::env::var("NOTICE_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| Some(cfg.server.auth_token.clone()).filter(|t| !t.trim().is_empty()))
        .map(|t| t.trim().into());

    let pool = db::connect(&cfg.database.notice).await;
    db::setup_notice(&pool).await;

    let ddb = db::connect(&cfg.database.contacts).await;
    db::setup_contacts(&ddb).await;

    let adb = db::connect(&cfg.database.archive).await;
    db::setup_archive(&adb).await;

    let state = AppState {
        pool,
        db: ddb,
        archive_pool: adb,
        config: Arc::new(Mutex::new(cfg.clone())),
        created_fresh,
        token,
    };

    if tui_mode {
        if let Err(error) = tui::run(state).await {
            eprintln!("TUI-Fehler: {error}");
        }
        return;
    }

    // Snapshot databases on every startup (rolling window per config)
    db::backup_db(&state.pool, &cfg.database.backup_dir, "notice", cfg.database.backup_keep).await;
    db::backup_db(&state.db, &cfg.database.backup_dir, "contacts", cfg.database.backup_keep).await;
    db::backup_db(&state.archive_pool, &cfg.database.backup_dir, "archive", cfg.database.backup_keep).await;

    let addr = cfg.addr();
    if state.token.is_none() && !addr.ip().is_loopback() {
        eprintln!(
            "[Notice] Abbruch: Bindung an {} ohne Zugriffstoken ist unsicher.\n\
             Token erzeugen: `notice --gen-token`, dann `auth_token = \"…\"` unter [server] in config.toml \
             oder NOTICE_TOKEN setzen (und TLS per Reverse-Proxy/VPN davorschalten).",
            addr
        );
        std::process::exit(1);
    }
    let app = api::router(state);
    println!("Server läuft auf http://{}{}", addr, if cfg.server.auth_token.is_empty() && std::env::var("NOTICE_TOKEN").is_err() { "" } else { " (Token-Schutz aktiv)" });

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
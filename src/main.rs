mod api;
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
/// `--tui` (starts the terminal interface instead of the web server).
fn parse_args() -> (String, Option<u16>, bool) {
    let mut config_file = String::from(Config::FILE);
    let mut port = None;
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
            Some("--tui") => tui_mode = true,
            _ => {}
        }
        i += 1;
    }
    (config_file, port, tui_mode)
}

#[tokio::main]
async fn main() {
    let (config_file, port_override, tui_mode) = parse_args();

    let (mut cfg, created_fresh) = Config::load(&config_file);
    if let Some(port) = port_override {
        cfg.server.port = port;
    }

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

    let app = api::router(state);
    let addr = cfg.addr();
    println!("Server läuft auf http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
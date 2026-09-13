mod api;
mod config;
mod db;
mod frontend;
mod i18n;
mod models;
mod recurrence;

use config::Config;
use models::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let (cfg, created_fresh) = Config::load(Config::FILE);

    let pool = db::connect(&cfg.database.notice).await;
    db::setup_notice(&pool).await;

    let ddb = db::connect(&cfg.database.contacts).await;
    db::setup_contacts(&ddb).await;

    // Snapshot both databases on every startup (rolling window per config)
    db::backup_db(&pool, &cfg.database.backup_dir, "notice", cfg.database.backup_keep).await;
    db::backup_db(&ddb, &cfg.database.backup_dir, "contacts", cfg.database.backup_keep).await;

    let state = AppState {
        pool,
        db: ddb,
        config: Arc::new(Mutex::new(cfg.clone())),
        created_fresh,
    };

    let app = api::router(state);
    let addr = cfg.addr();
    println!("Server läuft auf http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
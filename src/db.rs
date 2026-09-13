use sqlx::SqlitePool;

pub async fn connect(path: &str) -> SqlitePool {
    SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path))
        .await
        .expect("Fehler beim Verbinden mit der SQLite-Datenbank")
}

pub async fn setup_notice(pool: &SqlitePool) {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            status TEXT NOT NULL,
            priority TEXT NOT NULL,
            date TEXT NOT NULL,
            due_date TEXT,
            department TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("Fehler beim Erstellen der Tabelle");

    // Migration: add sort_order if an existing table is missing the column
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await;

    // Departments can also be created in the DATEN-View (previously only derived from notes)
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS departments_store (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE)",
    )
    .execute(pool)
    .await;

    // Wiki pages referenced via <<...>> links
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS wiki (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL UNIQUE, content TEXT, created_at TEXT, updated_at TEXT)",
    )
    .execute(pool)
    .await;

    // Migration: existing databases still use the old column name "einrichtung"
    let _ = sqlx::query("ALTER TABLE notes RENAME COLUMN einrichtung TO department")
        .execute(pool)
        .await;

    // Migration: add departments (JSON array) column
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN departments TEXT")
        .execute(pool)
        .await;

    // Migration: add appointments (JSON array) column
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN appointments TEXT")
        .execute(pool)
        .await;

    // Migration: migrate existing single department values into the departments JSON array
    let _ = sqlx::query(
        r#"
        UPDATE notes
        SET departments = json_array(department)
        WHERE departments IS NULL
          AND department IS NOT NULL
          AND TRIM(department) != ''
        "#,
    )
    .execute(pool)
    .await;

    // Migration: set empty department values to an empty JSON array
    let _ = sqlx::query(
        r#"
        UPDATE notes
        SET departments = '[]'
        WHERE departments IS NULL
        "#,
    )
    .execute(pool)
    .await;

    // Migration: add completed_at column (tracks when a note was finished/archived)
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN completed_at TEXT")
        .execute(pool)
        .await;

    // Migration: add deleted_at column (soft-delete / Papierkorb)
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN deleted_at TEXT")
        .execute(pool)
        .await;

    // Migration: add repeat_rule column (daily/weekly/monthly recurrence)
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN repeat_rule TEXT")
        .execute(pool)
        .await;

    // Migration: add pinned column (board pins)
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0")
        .execute(pool)
        .await;
}

pub async fn setup_contacts(db: &SqlitePool) {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS contacts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            department TEXT,
            phone TEXT,
            email TEXT,
            description TEXT
        )
        "#,
    )
    .execute(db)
    .await
    .expect("Fehler beim Erstellen der Kontakte-Tabelle");

    // Migration: add deleted_at column (soft-delete / Papierkorb)
    let _ = sqlx::query("ALTER TABLE contacts ADD COLUMN deleted_at TEXT")
        .execute(db)
        .await;

    // Migration: add departments (JSON array) column to contacts
    let _ = sqlx::query("ALTER TABLE contacts ADD COLUMN departments TEXT")
        .execute(db)
        .await;

    // Migration: migrate existing single department values into the departments JSON array
    let _ = sqlx::query(
        r#"
        UPDATE contacts
        SET departments = json_array(department)
        WHERE departments IS NULL
          AND department IS NOT NULL
          AND TRIM(department) != ''
        "#,
    )
    .execute(db)
    .await;

    // Migration: set empty department values to an empty JSON array
    let _ = sqlx::query(
        r#"
        UPDATE contacts
        SET departments = '[]'
        WHERE departments IS NULL
        "#,
    )
    .execute(db)
    .await;

    // Seed sample contacts on first run (empty table)
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts")
        .fetch_one(db)
        .await
        .unwrap_or(0);
    if count == 0 {
        let _ = sqlx::query(
            r#"
            INSERT INTO contacts (name, department, departments, phone, email, description) VALUES
                ('Max Mustermann', 'Küche', '["Küche"]', '030 12345678', 'max.mustermann@example.de', 'Küchenchef, verantwortlich für den Speiseplan.'),
                ('Erika Musterfrau', 'Verwaltung', '["Verwaltung"]', '030 87654321', 'erika.musterfrau@example.de', 'Leiterin der Verwaltung, Ansprechpartnerin für Rechnungen.')
            "#,
        )
        .execute(db)
        .await;
    }
}

// Create a consistent snapshot of a database into the backups dir using VACUUM INTO,
// keeping a rolling window of keep snapshots per database.
pub async fn backup_db(pool: &SqlitePool, dir: &str, prefix: &str, keep: usize) {
    let _ = std::fs::create_dir_all(dir);
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let path = format!("{}/{}_{}.db", dir, prefix, ts);
    let _ = sqlx::query(&format!("VACUUM INTO '{}'", path))
        .execute(pool)
        .await;

    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut files: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(&format!("{}_", prefix)) && n.ends_with(".db"))
            .collect();
        files.sort();
        while files.len() > keep {
            if let Some(oldest) = files.first() {
                let _ = std::fs::remove_file(format!("{}/{}", dir, oldest));
            }
            files.remove(0);
        }
    }
}
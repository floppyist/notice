use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::net::SocketAddr;

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
}

#[derive(Serialize, Deserialize, FromRow, Clone)]
struct Note {
    id: i64,
    title: String,
    content: String,
    status: String,
    priority: String,
    date: String,
    due_date: Option<String>,
    sort_order: i64,
}

#[derive(Deserialize)]
struct CreateNote {
    title: String,
    content: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    due_date: Option<String>,
}

#[derive(Deserialize)]
struct UpdateNote {
    title: Option<String>,
    content: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    due_date: Option<String>,
    sort_order: Option<i64>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

#[tokio::main]
async fn main() {
    let database_url = "sqlite://notice.db?mode=rwc";
    let pool = SqlitePool::connect(database_url)
        .await
        .expect("Fehler beim Verbinden mit der SQLite-Datenbank");

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
            sort_order INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Fehler beim Erstellen der Tabelle");

    // Migration: sort_order falls vorhandene Tabelle die Spalte fehlt
    let _ = sqlx::query("ALTER TABLE notes ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0")
        .execute(&pool)
        .await;

    let state = AppState { pool };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/notes", get(get_notes).post(create_note))
        .route("/api/notes/search", get(search_notes))
        .route("/api/notes/:id", post(update_note).put(update_note).delete(delete_note))
        .route("/api/notes/:id/duplicate", post(duplicate_note))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("Server läuft auf http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn index_handler() -> Html<&'static str> {
    Html(FRONTEND_HTML)
}

async fn get_notes(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_as::<_, Note>("SELECT id, title, content, status, priority, date, due_date, sort_order FROM notes ORDER BY sort_order ASC, id ASC")
        .fetch_all(&state.pool)
        .await
    {
        Ok(notes) => Json(notes).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn search_notes(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let q = params.q.unwrap_or_default().trim().to_lowercase();
    if q.is_empty() {
        return Json(Vec::<serde_json::Value>::new()).into_response();
    }
    let pattern = format!("%{}%", q);
    match sqlx::query_as::<_, (i64, String)>(
        "SELECT id, title FROM notes WHERE LOWER(title) LIKE ? ORDER BY sort_order ASC LIMIT 8",
    )
    .bind(pattern)
    .fetch_all(&state.pool)
    .await
    {
        Ok(results) => {
            let json: Vec<serde_json::Value> = results
                .into_iter()
                .map(|(id, title)| serde_json::json!({ "id": id, "title": title }))
                .collect();
            Json(json).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn create_note(
    State(state): State<AppState>,
    Json(payload): Json<CreateNote>,
) -> impl IntoResponse {
    let date = chrono::Local::now().format("%d.%m.%Y").to_string();
    let content = payload.content.unwrap_or_default();
    let status = payload.status.unwrap_or_else(|| "backlog".to_string());
    let priority = payload.priority.unwrap_or_else(|| "medium".to_string());
    let due_date = payload.due_date.filter(|s| !s.is_empty());

    // sort_order: größter Wert in der Spalte + 1
    let max_sort: Result<Option<i64>, _> =
        sqlx::query_scalar("SELECT MAX(sort_order) FROM notes WHERE status = ?")
            .bind(&status)
            .fetch_one(&state.pool)
            .await;
    let sort_order = max_sort.unwrap_or(None).unwrap_or(0) + 1;

    let result = sqlx::query(
        r#"
        INSERT INTO notes (title, content, status, priority, date, due_date, sort_order)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(payload.title)
    .bind(content)
    .bind(status)
    .bind(priority)
    .bind(date)
    .bind(due_date)
    .bind(sort_order)
    .execute(&state.pool)
    .await;

    match result {
        Ok(res) => {
            let id = res.last_insert_rowid();
            if let Ok(note) = sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ?")
                .bind(id)
                .fetch_one(&state.pool)
                .await
            {
                return (StatusCode::CREATED, Json(note)).into_response();
            }
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn update_note(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    Json(payload): Json<UpdateNote>,
) -> impl IntoResponse {
    let existing = match sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(note)) => note,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let title = payload.title.unwrap_or(existing.title);
    let content = payload.content.unwrap_or(existing.content);
    let status = payload.status.unwrap_or(existing.status);
    let priority = payload.priority.unwrap_or(existing.priority);
    let due_date = payload.due_date.or(existing.due_date);
    let sort_order = payload.sort_order.unwrap_or(existing.sort_order);

    let result = sqlx::query(
        r#"
        UPDATE notes SET title = ?, content = ?, status = ?, priority = ?, due_date = ?, sort_order = ? WHERE id = ?
        "#,
    )
    .bind(title)
    .bind(content)
    .bind(status)
    .bind(priority)
    .bind(due_date)
    .bind(sort_order)
    .bind(id)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn delete_note(Path(id): Path<i64>, State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query("DELETE FROM notes WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn duplicate_note(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let existing = match sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(Some(note)) => note,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let new_title = format!("{} (Kopie)", existing.title);
    let date = chrono::Local::now().format("%d.%m.%Y").to_string();

    let max_sort: Result<Option<i64>, _> =
        sqlx::query_scalar("SELECT MAX(sort_order) FROM notes WHERE status = ?")
            .bind(&existing.status)
            .fetch_one(&state.pool)
            .await;
    let sort_order = max_sort.unwrap_or(None).unwrap_or(0) + 1;

    let result = sqlx::query(
        r#"
        INSERT INTO notes (title, content, status, priority, date, due_date, sort_order)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(new_title)
    .bind(existing.content)
    .bind(existing.status)
    .bind(existing.priority)
    .bind(date)
    .bind(existing.due_date)
    .bind(sort_order)
    .execute(&state.pool)
    .await;

    match result {
        Ok(res) => {
            let new_id = res.last_insert_rowid();
            if let Ok(note) = sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ?")
                .bind(new_id)
                .fetch_one(&state.pool)
                .await
            {
                return (StatusCode::CREATED, Json(note)).into_response();
            }
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

const FRONTEND_HTML: &str = r#"<!DOCTYPE html>
<html lang="de">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Notice - Pro Kanban Notes</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <script src="https://unpkg.com/vue@3/dist/vue.global.js"></script>
    <script src="https://cdn.jsdelivr.net/npm/marked/marked.min.js"></script>
    <script>
        tailwind.config = {
            theme: {
                extend: {
                    borderRadius: { none: '0px', DEFAULT: '0px', sm: '0px', md: '0px', lg: '0px', xl: '0px', '2xl': '0px', '3xl': '0px', full: '0px' }
                }
            }
        }
    </script>
    <style>
        html, body {
            height: 100vh;
            margin: 0;
            overflow: hidden;
            background-color: #0d0d0f;
            color: #d4d4d8;
            font-family: 'Courier New', Courier, Lucida Console, Monaco, monospace;
        }
        .markdown-body h1 { font-size: 1.15rem; font-weight: bold; margin-bottom: 0.4rem; }
        .markdown-body h2 { font-size: 1.05rem; font-weight: bold; margin-bottom: 0.3rem; }
        .markdown-body p { margin-bottom: 0.4rem; }
        .markdown-body ul { list-style-type: disc; padding-left: 1.1rem; margin-bottom: 0.4rem; }
        .markdown-body ol { list-style-type: decimal; padding-left: 1.1rem; margin-bottom: 0.4rem; }
        .markdown-body code { background: #27272a; padding: 0.1rem 0.2rem; font-size: 0.8em; }
        /* Benutzerdefinierte Farben für Markdown HTML-Ausgabe */
        .markdown-body span[style*="color"] { opacity: 0.9; }
        .note-link { color: #34d399; text-decoration: underline; cursor: pointer; }
        .note-link:hover { color: #6ee7b7; background: rgba(52, 211, 153, 0.1); }
        .autocomplete-dropdown { position: absolute; z-index: 70; min-width: 220px; max-width: 320px; }
        .autocomplete-item { cursor: pointer; }
        .autocomplete-item.active { background: rgba(52, 211, 153, 0.15); }
        .archive-expand-btn {
            writing-mode: vertical-rl;
            text-orientation: mixed;
            letter-spacing: 0.2em;
            font-size: 9px;
            font-weight: bold;
            text-transform: uppercase;
        }
    </style>
</head>
<body>
    <div id="app" class="h-screen flex flex-col" @click="closeContextMenu">
        <header class="bg-zinc-900 border-b border-zinc-800 px-3 py-2 flex justify-between items-center shrink-0 gap-4">
            <h1 class="text-xs font-bold tracking-widest text-emerald-400 flex items-center gap-2 shrink-0">
                <span class="inline-block w-2 h-2 bg-emerald-500"></span> NOTICE_V1.1
            </h1>

            <div class="flex-1 flex gap-1.5 items-center min-w-0">
                <input 
                    ref="searchInputRef"
                    type="text" 
                    v-model="searchQuery" 
                    placeholder="Suchen (Strg+K)..." 
                    class="bg-zinc-950 border border-zinc-700 px-2.5 py-1 text-xs text-zinc-100 focus:outline-none focus:border-emerald-500 flex-1 min-w-0"
                >
                <button 
                    @click="openNewNote" 
                    class="bg-emerald-700 hover:bg-emerald-600 text-zinc-100 px-3 py-1 text-xs border border-emerald-600 font-semibold cursor-pointer transition-colors shrink-0">
                    + HINZUFÜGEN
                </button>
                <div class="relative shrink-0">
                    <button 
                        @click.stop="exportMenuOpen = !exportMenuOpen" 
                        :disabled="notes.length === 0"
                        :class="notes.length === 0 ? 'opacity-40 cursor-not-allowed bg-zinc-900 border-zinc-800 text-zinc-600' : 'bg-zinc-800 hover:bg-zinc-700 text-zinc-300 border-zinc-700 cursor-pointer'"
                        class="px-2.5 py-1 text-xs border">
                        📤 Export ▾
                    </button>
                    <div 
                        v-if="exportMenuOpen"
                        class="absolute right-0 top-full mt-1 bg-zinc-900 border border-zinc-700 shadow-2xl py-1 text-xs font-mono z-50 min-w-[140px]"
                    >
                        <button 
                            @click="exportJson; exportMenuOpen = false" 
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-800 text-zinc-200 cursor-pointer">
                            📥 Als JSON
                        </button>
                        <button 
                            @click="exportCsv; exportMenuOpen = false" 
                            class="w-full text-left px-3 py-1.5 hover:bg-zinc-800 text-zinc-200 cursor-pointer">
                            📊 Als CSV
                        </button>
                    </div>
                </div>
            </div>
        </header>

        <main class="flex-1 flex gap-1.5 p-2 overflow-hidden bg-zinc-950">

            <!-- Links: vertikaler Tab-Stapel für minimierte Status -->
            <div v-if="collapsedStatuses.length > 0" class="flex flex-col gap-1.5 shrink-0 overflow-y-auto">
                <div 
                    v-for="status in collapsedStatuses" 
                    :key="status.id"
                    class="bg-zinc-900/70 border border-zinc-800 flex flex-col items-center py-1.5 shrink-0"
                    style="width:40px"
                >
                    <button 
                        @click="expandColumn(status.id)"
                        class="archive-expand-btn text-zinc-500 hover:text-zinc-200 flex items-center px-1 py-2 cursor-pointer transition-colors"
                        :title="status.title + ' öffnen'"
                    >
                        {{ status.title }}
                    </button>
                    <span class="text-zinc-600 text-[10px] mt-1 leading-none">{{ getNotesByColumn(status.id).length }} ▲</span>
                </div>
            </div>

            <!-- Mitte: erweiterte Spalten als Grid -->
            <div 
                class="flex-1 grid gap-1.5 overflow-hidden transition-all duration-300 min-w-0"
                :style="{ gridTemplateColumns: gridColsStyle }"
            >
                <div 
                    v-for="column in expandedColumns" 
                    :key="column.id"
                    class="bg-zinc-900/70 border border-zinc-800 flex flex-col h-full overflow-hidden"
                    @dragover.prevent
                    @drop="onDrop(column.id)"
                >
                    <div class="bg-zinc-900 border-b border-zinc-800 px-2.5 py-1.5 flex justify-between items-center shrink-0">
                        <button @click="toggleCollapse(column.id)" class="text-[11px] font-bold tracking-wider text-zinc-300 uppercase flex items-center gap-1.5 cursor-pointer">
                            <span class="w-1.5 h-1.5" :class="{
                                'bg-zinc-500': column.id === 'backlog',
                                'bg-blue-500': column.id === 'in_progress',
                                'bg-amber-500': column.id === 'review',
                                'bg-emerald-500': column.id === 'done'
                            }"></span>
                            {{ column.title }}
                        </button>
                        <span class="bg-zinc-950 text-zinc-400 text-[10px] px-1.5 py-0.5 border border-zinc-800">
                            {{ getNotesByColumn(column.id).length }}
                        </span>
                    </div>

                    <div class="flex-1 p-1.5 overflow-y-auto space-y-1.5">
                        <div v-if="getNotesByColumn(column.id).length === 0" class="text-[10px] text-zinc-600 italic text-center py-4">
                            Keine Notizen.
                        </div>
                        <div 
                            v-for="note in getNotesByColumn(column.id)" 
                            :key="note.id"
                            draggable="true"
                            @dragstart="startDrag(note)"
                            @dragover.prevent="onDragOver(note, $event)"
                            @drop.prevent.stop="onDrop(column.id, note)"
                            @dblclick="openModal(note)"
                            class="bg-zinc-900 border p-2.5 cursor-pointer hover:border-zinc-500 transition-colors shadow-sm group relative"
                            :class="{
                                'border-red-500/80 bg-red-950/10': note.priority === 'high' && note.status !== 'done',
                                'border-amber-500/80 bg-amber-950/10': note.priority === 'medium' && note.status !== 'done',
                                'border-zinc-700 bg-zinc-900': note.priority === 'low' && note.status !== 'done',
                                'border-emerald-600/70 bg-emerald-950/15': note.status === 'done'
                            }"
                        >
                            <div class="flex justify-between items-start gap-1 mb-1.5">
                                <h3 class="font-bold text-xs text-zinc-100 break-all leading-snug pr-1">{{ note.title }}</h3>
                                <div class="flex gap-1 shrink-0">
                                    <button @click.stop="duplicateNote(note)" title="Duplizieren" class="text-zinc-600 hover:text-emerald-400 text-[10px] px-1 font-mono">[+]</button>
                                    <button @click.stop="archiveNote(note)" title="Archivieren" class="text-zinc-600 hover:text-zinc-300 text-[10px] px-1 font-mono">[A]</button>
                                    <button @click.stop="deleteNote(note.id)" class="text-zinc-600 hover:text-red-400 text-[10px] px-1 font-mono shrink-0">[X]</button>
                                </div>
                            </div>

                            <div class="flex justify-between items-center text-[10px] text-zinc-400 pt-1 border-t border-zinc-800/60">
                                <span v-if="isOverdue(note)" class="text-red-400/90 font-mono flex items-center gap-1">
                                    <span class="inline-block w-1.5 h-1.5 bg-red-500 rounded-full animate-pulse"></span>
                                    ÜBERFÄLLIG ({{ note.due_date }})
                                </span>
                                <span v-else-if="isDueSoon(note)" class="text-orange-400/90 font-mono flex items-center gap-1">
                                    <span class="inline-block w-1.5 h-1.5 bg-orange-400 rounded-full animate-pulse"></span>
                                    FÄLLIG ({{ note.due_date }})
                                </span>
                                <span v-else-if="note.due_date" class="text-zinc-500 font-mono">Fällig: {{ note.due_date }}</span>
                                <span v-else class="text-zinc-600 font-mono">Kein Datum</span>
                                <span class="uppercase tracking-widest text-[9px] px-1 bg-zinc-950 border border-zinc-800" :class="{
                                    'text-emerald-400 border-emerald-900/50': note.status === 'done',
                                    'text-zinc-400': note.status !== 'done'
                                }">
                                    {{ note.status === 'done' ? 'DONE' : note.priority }}
                                </span>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            <!-- Archiv-Spalte (rechts, separat auf-/zuklappbar) -->
            <div 
                class="bg-zinc-900/70 border border-zinc-800 flex flex-col h-full overflow-hidden transition-all duration-300 shrink-0"
                :style="{ width: isArchiveCollapsed ? '40px' : '220px' }"
                @dragover.prevent
                @drop.prevent.stop="onDrop('archived', null)"
            >
                <div class="bg-zinc-900 border-b border-zinc-800 px-2.5 py-1.5 flex justify-between items-center shrink-0">
                    <button v-if="!isArchiveCollapsed" @click="isArchiveCollapsed = !isArchiveCollapsed" class="text-[11px] font-bold tracking-wider text-zinc-500 uppercase flex items-center gap-1.5 cursor-pointer hover:text-zinc-300 transition-colors">
                        <span class="w-1.5 h-1.5 bg-zinc-700"></span>
                        <span>Archiv</span>
                    </button>
                    <div v-else class="flex flex-col items-center justify-center h-full">
                        <button 
                            @click="isArchiveCollapsed = false"
                            class="archive-expand-btn text-zinc-500 hover:text-zinc-200 flex items-center px-1 py-2 cursor-pointer transition-colors"
                            title="Archiv öffnen"
                        >
                            Archiv
                        </button>
                        <span class="text-zinc-600 text-[10px] mt-1 leading-none">{{ getNotesByColumn('archived').length }} ▲</span>
                    </div>
                    <span v-if="!isArchiveCollapsed" class="bg-zinc-950 text-zinc-400 text-[10px] px-1.5 py-0.5 border border-zinc-800">
                        {{ getNotesByColumn('archived').length }}
                    </span>
                </div>

                <div v-if="isArchiveCollapsed" class="flex-1"></div>
                <div v-else class="flex-1 p-1.5 overflow-y-auto space-y-1.5">
                    <div v-if="getNotesByColumn('archived').length === 0" class="text-[10px] text-zinc-600 italic text-center py-4">
                        Keine archivierten Notizen.
                    </div>
                    <div 
                        v-for="note in getNotesByColumn('archived')" 
                        :key="note.id"
                        draggable="true"
                        @dragstart="startDrag(note)"
                        @dragover.prevent="onDragOver(note, $event)"
                        @drop.prevent.stop="onDrop('archived', note)"
                        @dblclick="openModal(note)"
                        class="bg-zinc-900 border border-zinc-800 p-2.5 cursor-pointer hover:border-zinc-500 transition-colors shadow-sm group relative opacity-70"
                    >
                        <div class="flex justify-between items-start gap-1 mb-1.5">
                            <h3 class="font-bold text-xs text-zinc-400 break-all leading-snug pr-1">{{ note.title }}</h3>
                            <button @click.stop="unarchiveNote(note)" title="Wiederherstellen" class="text-zinc-600 hover:text-emerald-400 text-[10px] px-1 font-mono shrink-0">[R]</button>
                        </div>
                        <div class="flex justify-between items-center text-[10px] text-zinc-500 pt-1 border-t border-zinc-800/60">
                            <span v-if="note.due_date" class="text-orange-400/70 font-mono">Fällig: {{ note.due_date }}</span>
                            <span v-else class="text-zinc-600 font-mono">Kein Datum</span>
                            <span class="uppercase tracking-widest text-[9px] px-1 bg-zinc-950 border border-zinc-800 text-zinc-500">ARCH</span>
                        </div>
                    </div>
                </div>
            </div>
        </main>

        <!-- Neue Notiz Maske -->
        <div v-if="isNewNoteOpen" class="fixed inset-0 bg-black/80 flex items-center justify-center p-3 z-40" @keydown.enter="handleNewNoteEnter">
            <div class="bg-zinc-900 border border-zinc-700 w-full max-w-3xl max-h-[90vh] flex flex-col shadow-2xl" @click.stop>
                <div class="bg-zinc-900 border-b border-zinc-800 px-3 py-2 flex justify-between items-center shrink-0">
                    <span class="text-xs font-bold tracking-widest text-emerald-400 uppercase">Neue Notiz</span>
                    <button @click="closeNewNote" class="ml-3 text-zinc-400 hover:text-zinc-100 text-xs font-bold px-2 cursor-pointer">X</button>
                </div>
                <div class="p-3 flex flex-col gap-3 bg-zinc-950 overflow-y-auto">
                    <div class="grid grid-cols-1 md:grid-cols-4 gap-3">
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-400 md:col-span-4">
                            Titel
                            <input 
                                ref="newNoteTitleInputRef"
                                type="text" 
                                v-model="newNoteTitle" 
                                placeholder="Notiz-Titel..." 
                                class="bg-zinc-900 border border-zinc-700 px-2 py-1 text-xs text-zinc-100 focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
                            >
                        </label>
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-400">
                            Status
                            <select v-model="newNoteStatus" class="bg-zinc-900 border border-zinc-700 px-2 py-1 text-xs text-zinc-200 focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500">
                                <option value="backlog">Backlog</option>
                                <option value="in_progress">In Arbeit</option>
                                <option value="review">Review</option>
                                <option value="done">Abgeschlossen</option>
                            </select>
                        </label>
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-400">
                            Priorität
                            <select v-model="newNotePriority" class="bg-zinc-900 border border-zinc-700 px-2 py-1 text-xs text-zinc-200 focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500">
                                <option value="low">Niedrig</option>
                                <option value="medium">Mittel</option>
                                <option value="high">Hoch</option>
                            </select>
                        </label>
                        <label class="flex flex-col gap-1 text-[11px] text-zinc-400">
                            Fällig
                            <input 
                                type="date" 
                                v-model="newNoteDueDate" 
                                class="bg-zinc-900 border border-zinc-700 px-2 py-1 text-xs text-zinc-200 focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
                            >
                        </label>
                    </div>

                    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
                        <div class="flex flex-col gap-1 relative">
                            <span class="text-[11px] text-zinc-400">Inhalt (Markdown)</span>
                            <textarea 
                                ref="newNoteTextareaRef"
                                v-model="newNoteContent" 
                                placeholder="Inhalt schreiben... ([[ Für Notiz-Links)" 
                                @input="handleAutocomplete"
                                @keyup="updateAutocompletePos"
                                @click="updateAutocompletePos"
                                @keydown="handleAutocompleteKeydown"
                                class="w-full h-40 bg-zinc-900 border border-zinc-700 px-2 py-1 text-xs text-zinc-100 font-mono focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500 resize-none"
                            ></textarea>
                            <div ref="newNoteCaretMirrorRef" class="absolute invisible whitespace-pre break-all" style="font-family:'Courier New',Courier,Lucida Console,Monaco,monospace; font-size:12px; line-height:16px; padding:10px; border:1px solid transparent; left:0; top:0; z-index:-1; pointer-events:none;"></div>
                            <div 
                                v-if="showAutocomplete"
                                :style="{ top: autocompletePos.y + 'px', left: autocompletePos.x + 'px' }"
                                class="autocomplete-dropdown bg-zinc-900 border border-zinc-700 shadow-2xl py-1 text-xs font-mono overflow-y-auto max-h-56"
                            >
                                <div class="px-2.5 py-1 text-[10px] text-zinc-500 uppercase tracking-wider border-b border-zinc-800 mb-1 flex justify-between items-center">
                                    <span>Notiz verlinken</span>
                                    <span class="text-zinc-600">↑↓ Enter Esc</span>
                                </div>
                                <button 
                                    v-for="(result, index) in autocompleteResults" 
                                    :key="result.id"
                                    @click="selectAutocomplete(index)"
                                    @mousedown.prevent
                                    class="w-full text-left px-3 py-1.5 hover:bg-zinc-800 text-zinc-200 flex justify-between items-center gap-2"
                                    :class="{ 'bg-zinc-800': index === autocompleteIndex }"
                                >
                                    <span class="truncate">{{ result.title }}</span>
                                    <span class="text-zinc-500 text-[10px] shrink-0">#{{ result.id }}</span>
                                </button>
                                <div v-if="autocompleteResults.length === 0" class="px-3 py-2 text-zinc-500">
                                    Keine Treffer.
                                </div>
                            </div>
                        </div>
                        <div class="bg-zinc-900 border border-zinc-800 p-2.5 flex flex-col">
                            <span class="text-[11px] font-bold text-zinc-300 uppercase mb-2 border-b border-zinc-800 pb-1">Checklist / Steps</span>
                            <div class="flex gap-1 mb-2">
                                <input 
                                    ref="newNoteStepInputRef"
                                    type="text" 
                                    v-model="newNoteStepText" 
                                    @keyup.enter="addNewNoteStep"
                                    placeholder="Neuer Step..." 
                                    class="bg-zinc-950 border border-zinc-700 px-2 py-1 text-[11px] text-zinc-100 w-full focus:outline-none focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500"
                                >
                                <button @click="addNewNoteStep" class="bg-zinc-800 hover:bg-zinc-700 text-zinc-200 px-2 text-[11px] border border-zinc-700 cursor-pointer shrink-0">+</button>
                            </div>
                            <div class="flex-1 overflow-y-auto space-y-1.5 pr-1 max-h-28">
                                <div v-if="newNoteChecklist.length === 0" class="text-[10px] text-zinc-600 italic text-center py-2">Keine Zwischensteps.</div>
                                <div v-for="(step, index) in newNoteChecklist" :key="index" class="flex items-center justify-between bg-zinc-950 border border-zinc-800/80 px-2 py-1 gap-2">
                                    <span class="text-[11px] text-zinc-300 break-all">{{ step.text }}</span>
                                    <button @click="removeNewNoteStep(index)" class="text-zinc-600 hover:text-red-400 text-[10px] shrink-0 font-mono">[x]</button>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
                <div class="bg-zinc-900 border-t border-zinc-800 px-3 py-2 flex justify-end gap-2">
                    <button 
                        @click="createNote" 
                        class="bg-emerald-700 hover:bg-emerald-600 text-zinc-100 px-3 py-1 text-[11px] font-semibold border border-emerald-600 cursor-pointer">
                        ERSTELLEN & SCHLIESSEN
                    </button>
                </div>
            </div>
        </div>

        <!-- Detail Modal -->
        <div v-if="isModalOpen" class="fixed inset-0 bg-black/80 flex items-center justify-center p-3 z-50">
            <div class="bg-zinc-900 border border-zinc-700 w-full max-w-3xl flex flex-col h-[80vh] shadow-2xl" @click.stop>
                <div class="bg-zinc-900 border-b border-zinc-800 px-3 py-2 flex justify-between items-center">
                    <input 
                        type="text" 
                        v-model="activeNote.title" 
                        class="bg-zinc-950 border border-zinc-700 px-2 py-1 text-xs text-zinc-100 w-full font-bold focus:outline-none focus:border-emerald-500"
                    >
                    <button @click="closeModal" class="ml-3 text-zinc-400 hover:text-zinc-100 text-xs font-bold px-2 cursor-pointer">X</button>
                </div>

                <div class="bg-zinc-900/50 border-b border-zinc-800 px-3 py-1.5 flex justify-between items-center text-[11px]">
                    <div class="flex gap-3 items-center flex-wrap">
                        <label class="flex items-center gap-1 text-zinc-400">
                            Status:
                            <select v-model="activeNote.status" class="bg-zinc-950 border border-zinc-700 text-zinc-200 px-1 py-0.5 focus:outline-none">
                                <option value="backlog">Backlog</option>
                                <option value="in_progress">In Arbeit</option>
                                <option value="review">Review</option>
                                <option value="done">Abgeschlossen</option>
                                <option value="archived">Archiv</option>
                            </select>
                        </label>
                        <label class="flex items-center gap-1 text-zinc-400">
                            Prio:
                            <select v-model="activeNote.priority" class="bg-zinc-950 border border-zinc-700 text-zinc-200 px-1 py-0.5 focus:outline-none">
                                <option value="low">Niedrig</option>
                                <option value="medium">Mittel</option>
                                <option value="high">Hoch</option>
                            </select>
                        </label>
                        <label class="flex items-center gap-1 text-zinc-400">
                            Fällig:
                            <input type="date" v-model="activeNote.due_date" class="bg-zinc-950 border border-zinc-700 text-zinc-200 px-1 py-0.5 focus:outline-none">
                        </label>
                    </div>
                    <div>
                        <button 
                            @click="isPreviewMode = !isPreviewMode" 
                            class="bg-zinc-800 border border-zinc-700 hover:bg-zinc-700 text-zinc-200 px-2.5 py-0.5 font-semibold transition-colors text-[11px] cursor-pointer">
                            {{ isPreviewMode ? '✏️ Bearbeiten' : '👁️ Vorschau' }}
                        </button>
                    </div>
                </div>

                <!-- Inhalt & Checklisten-Bereich -->
                <div class="flex-1 grid grid-cols-1 md:grid-cols-3 p-3 gap-3 overflow-hidden bg-zinc-950 relative">
                    <div class="md:col-span-2 h-full flex flex-col relative">
                        <textarea 
                            ref="textareaRef"
                            v-if="!isPreviewMode"
                            v-model="activeNote.content" 
                            @contextmenu.prevent="openContextMenu"
                            @input="handleAutocomplete"
                            @keyup="updateAutocompletePos"
                            @click="updateAutocompletePos"
                            @keydown="handleAutocompleteKeydown"
                            placeholder="Inhalt mit Markdown schreiben... (Rechtsklick für Text-Formatierung)"
                            class="w-full h-full bg-zinc-900 border border-zinc-800 text-zinc-200 p-2.5 text-xs font-mono resize-none focus:outline-none focus:border-zinc-600"
                        ></textarea>
                        <!-- Unsichtbarer Caret-Spiegel zur Berechnung der Cursorkoordinaten -->
                        <div ref="caretMirrorRef" class="absolute invisible whitespace-pre break-all" style="font-family:'Courier New',Courier,Lucida Console,Monaco,monospace; font-size:12px; line-height:16px; padding:10px; border:1px solid transparent; left:0; top:0; z-index:-1; pointer-events:none;"></div>
                        <div 
                             v-else 
                             class="markdown-body w-full h-full bg-zinc-900 border border-zinc-800 p-2.5 text-zinc-200 overflow-y-auto text-xs"
                             v-html="renderedMarkdown"
                             @click="handleNoteLinkClick">
                        </div>

                        <!-- Autocomplete Dropdown für [[ Links -->
                        <div 
                            v-if="showAutocomplete"
                            :style="{ top: autocompletePos.y + 'px', left: autocompletePos.x + 'px' }"
                            class="autocomplete-dropdown bg-zinc-900 border border-zinc-700 shadow-2xl py-1 text-xs font-mono overflow-y-auto max-h-56"
                        >
                            <div class="px-2.5 py-1 text-[10px] text-zinc-500 uppercase tracking-wider border-b border-zinc-800 mb-1 flex justify-between items-center">
                                <span>Notiz verlinken</span>
                                <span class="text-zinc-600">↑↓ Enter Esc</span>
                            </div>
                            <button 
                                v-for="(result, index) in autocompleteResults" 
                                :key="result.id"
                                @click="selectAutocomplete(index)"
                                @mousedown.prevent
                                class="w-full text-left px-3 py-1.5 hover:bg-zinc-800 text-zinc-200 flex justify-between items-center gap-2"
                                :class="{ 'bg-zinc-800': index === autocompleteIndex }"
                            >
                                <span class="truncate">{{ result.title }}</span>
                                <span class="text-zinc-500 text-[10px] shrink-0">#{{ result.id }}</span>
                            </button>
                            <div v-if="autocompleteResults.length === 0" class="px-3 py-2 text-zinc-500">
                                Keine Treffer.
                            </div>
                        </div>

                        <!-- Kontextmenü (Rechtsklick) -->
                        <div 
                            v-if="contextMenu.show" 
                            :style="{ top: contextMenu.y + 'px', left: contextMenu.x + 'px' }"
                            class="absolute z-50 bg-zinc-900 border border-zinc-700 shadow-2xl py-1 text-xs w-48 font-mono select-none"
                        >
                            <div class="px-2.5 py-1 text-[10px] text-zinc-500 uppercase tracking-wider border-b border-zinc-800 mb-1">Markdown Format</div>
                            <button @click="applyFormat('**')" class="w-full text-left px-3 py-1 hover:bg-zinc-800 text-zinc-200 flex justify-between"><span>Fett</span><span class="text-zinc-500">**text**</span></button>
                            <button @click="applyFormat('*')" class="w-full text-left px-3 py-1 hover:bg-zinc-800 text-zinc-200 flex justify-between"><span>Kursiv</span><span class="text-zinc-500">*text*</span></button>
                            <button @click="applyFormat('~~')" class="w-full text-left px-3 py-1 hover:bg-zinc-800 text-zinc-200 flex justify-between"><span>Durchgestrichen</span><span class="text-zinc-500">~~text~~</span></button>
                            <button @click="applyFormat('`')" class="w-full text-left px-3 py-1 hover:bg-zinc-800 text-zinc-200 flex justify-between border-b border-zinc-800 pb-1.5 mb-1"><span>Code-Snippet</span><span class="text-zinc-500">`text`</span></button>
                            <div class="px-2.5 py-1 text-[10px] text-zinc-500 uppercase tracking-wider mb-1">Farben (HTML)</div>
                            <button @click="applyHtmlColor('#34d399')" class="w-full text-left px-3 py-1 hover:bg-zinc-800 text-emerald-400 flex items-center gap-2"><span class="w-2 h-2 bg-emerald-400 inline-block"></span> Smaragdgrün</button>
                            <button @click="applyHtmlColor('#fb923c')" class="w-full text-left px-3 py-1 hover:bg-zinc-800 text-orange-400 flex items-center gap-2"><span class="w-2 h-2 bg-orange-400 inline-block"></span> Orange</button>
                            <button @click="applyHtmlColor('#60a5fa')" class="w-full text-left px-3 py-1 hover:bg-zinc-800 text-blue-400 flex items-center gap-2"><span class="w-2 h-2 bg-blue-400 inline-block"></span> Blau</button>
                        </div>
                    </div>

                    <!-- Zwischensteps / Checklist -->
                    <div class="bg-zinc-900 border border-zinc-800 p-2.5 flex flex-col h-full overflow-hidden">
                        <span class="text-[11px] font-bold text-zinc-300 uppercase mb-2 block border-b border-zinc-800 pb-1">Checklist / Steps</span>
                        <div class="flex gap-1 mb-2">
                            <input 
                                type="text" 
                                v-model="newStepText" 
                                @keyup.enter="addStep"
                                placeholder="Neuer Step..." 
                                class="bg-zinc-950 border border-zinc-700 px-2 py-1 text-[11px] text-zinc-100 w-full focus:outline-none focus:border-emerald-500"
                            >
                            <button @click="addStep" class="bg-zinc-800 hover:bg-zinc-700 text-zinc-200 px-2 text-[11px] border border-zinc-700 cursor-pointer">+</button>
                        </div>
                        <div class="flex-1 overflow-y-auto space-y-1.5 pr-1">
                            <div v-if="activeChecklist.length === 0" class="text-[10px] text-zinc-600 italic text-center py-4">Keine Zwischensteps vorhanden.</div>
                            <div v-for="(step, index) in activeChecklist" :key="index" class="flex items-center justify-between bg-zinc-950 border border-zinc-800/80 px-2 py-1 gap-2">
                                <label class="flex items-center gap-2 text-[11px] text-zinc-300 cursor-pointer overflow-hidden">
                                    <input type="checkbox" v-model="step.done" class="accent-emerald-600 cursor-pointer">
                                    <span :class="{'line-through text-zinc-600': step.done}" class="break-all">{{ step.text }}</span>
                                </label>
                                <button @click="removeStep(index)" class="text-zinc-600 hover:text-red-400 text-[10px] shrink-0 font-mono">[x]</button>
                            </div>
                        </div>
                    </div>
                </div>

                <div class="bg-zinc-900 border-t border-zinc-800 px-3 py-2 flex justify-end gap-2">
                    <button 
                        @click="saveActiveNote" 
                        class="bg-emerald-700 hover:bg-emerald-600 text-zinc-100 px-3 py-1 text-[11px] font-semibold border border-emerald-600 cursor-pointer">
                        SPEICHERN & SCHLIESSEN
                    </button>
                </div>
            </div>
        </div>
    </div>

    <script>
        const { createApp, ref, computed, onMounted, onUnmounted } = Vue

        createApp({
            setup() {
                const newNoteTitle = ref('')
                const newNotePriority = ref('medium')
                const newNoteDueDate = ref('')
                const newNoteStatus = ref('backlog')
                const newNoteContent = ref('')
                const newNoteChecklist = ref([])
                const newNoteStepText = ref('')
                const isNewNoteOpen = ref(false)
                const newNoteTitleInputRef = ref(null)
                const newNoteTextareaRef = ref(null)
                const newNoteCaretMirrorRef = ref(null)
                const newNoteStepInputRef = ref(null)
                const draggedNote = ref(null)
                const isBacklogCollapsed = ref(true)
                const isInProgressCollapsed = ref(false)
                const isReviewCollapsed = ref(false)
                const isDoneCollapsed = ref(false)
                const isArchiveCollapsed = ref(true)
                const searchQuery = ref('')
                const searchInputRef = ref(null)
                const textareaRef = ref(null)
                const caretMirrorRef = ref(null)

                const isModalOpen = ref(false)
                const isPreviewMode = ref(false)
                const activeNote = ref(null)
                const activeChecklist = ref([])
                const newStepText = ref('')

                // Autocomplete state für [[ Links
                const showAutocomplete = ref(false)
                const autocompleteResults = ref([])
                const autocompleteIndex = ref(0)
                const autocompleteQuery = ref('')
                const autocompleteStart = ref(0)
                const autocompletePos = ref({ x: 0, y: 0, flip: false })
                let autocompleteTimer = null
                const autocompleteContainerRef = ref(null)
                const exportMenuOpen = ref(false)

                const contextMenu = ref({
                    show: false,
                    x: 0,
                    y: 0,
                    selectionStart: 0,
                    selectionEnd: 0
                })

                const columns = [
                    { id: 'backlog', title: '01_Backlog' },
                    { id: 'in_progress', title: '02_In Arbeit' },
                    { id: 'review', title: '03_Review' },
                    { id: 'done', title: '04_Abgeschlossen' }
                ]

                const collapsedMap = {
                    backlog: isBacklogCollapsed,
                    in_progress: isInProgressCollapsed,
                    review: isReviewCollapsed,
                    done: isDoneCollapsed
                }

                const isCollapsed = (colId) => collapsedMap[colId] ? collapsedMap[colId].value : false

                const collapseColumn = (colId) => {
                    if (collapsedMap[colId]) collapsedMap[colId].value = true
                }
                const expandColumn = (colId) => {
                    if (collapsedMap[colId]) collapsedMap[colId].value = false
                }
                const toggleCollapse = (colId) => {
                    if (isCollapsed(colId)) expandColumn(colId)
                    else collapseColumn(colId)
                }

                const collapsedStatuses = computed(() => columns.filter(c => isCollapsed(c.id)))
                const expandedColumns = computed(() => columns.filter(c => !isCollapsed(c.id)))

                const gridColsStyle = computed(() => {
                    const parts = expandedColumns.value.map(() => 'minmax(200px, 1fr)')
                    return parts.join(' ')
                })

                const notes = ref([])

                const fetchNotes = async () => {
                    try {
                        const res = await fetch('/api/notes')
                        if (res.ok) {
                            notes.value = await res.json()
                        }
                    } catch (e) {
                        console.error('Fehler beim Laden der Notizen', e)
                    }
                }

                const focusSearchInput = () => {
                    if (searchInputRef.value) searchInputRef.value.focus()
                }

                const handleGlobalKeydown = (e) => {
                    const isCtrlOrMeta = e.ctrlKey || e.metaKey;
                    const isAlt = e.altKey;

                    if (isAlt && (e.key === 'n' || e.key === 'N' || e.code === 'KeyN')) {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        openNewNote()
                    } else if (isCtrlOrMeta && (e.key === 'k' || e.key === 'K' || e.code === 'KeyK')) {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        focusSearchInput()
                    } else if (isCtrlOrMeta && (e.key === 's' || e.key === 'S')) {
                        if (isModalOpen.value) {
                            e.preventDefault()
                            e.stopPropagation()
                            e.stopImmediatePropagation()
                            saveActiveNote()
                        }
                    } else if (e.key === 'Escape') {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        if (showAutocomplete.value) {
                            cancelAutocomplete()
                        } else if (isModalOpen.value) {
                            closeModal()
                        } else if (isNewNoteOpen.value) {
                            closeNewNote()
                        }
                    }
                }

                onMounted(() => {
                    fetchNotes()
                    window.addEventListener('keydown', handleGlobalKeydown, { capture: true })
                })

                onUnmounted(() => {
                    window.removeEventListener('keydown', handleGlobalKeydown, { capture: true })
                })

                const dueStatus = (note) => {
                    if (note.status === 'done' || note.status === 'archived' || !note.due_date) return 'none'
                    const now = new Date()
                    const todayStr = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`
                    if (note.due_date < todayStr) return 'overdue'
                    if (note.due_date === todayStr) return 'due'
                    const due = new Date(note.due_date + 'T00:00:00')
                    const today = new Date(todayStr + 'T00:00:00')
                    if (Math.round((due - today) / 86400000) <= 3) return 'due'
                    return 'none'
                }

                const isOverdue = (note) => dueStatus(note) === 'overdue'
                const isDueSoon = (note) => dueStatus(note) === 'due'

                const openNewNote = () => {
                    isNewNoteOpen.value = true
                    requestAnimationFrame(() => {
                        if (newNoteTitleInputRef.value) {
                            newNoteTitleInputRef.value.focus()
                        }
                    })
                }

                const closeNewNote = () => {
                    isNewNoteOpen.value = false
                    newNoteTitle.value = ''
                    newNotePriority.value = 'medium'
                    newNoteDueDate.value = ''
                    newNoteStatus.value = 'backlog'
                    newNoteContent.value = ''
                    newNoteChecklist.value = []
                    newNoteStepText.value = ''
                }

                const addNewNoteStep = () => {
                    if (!newNoteStepText.value.trim()) return
                    newNoteChecklist.value.push({ text: newNoteStepText.value.trim(), done: false })
                    newNoteStepText.value = ''
                }

                const removeNewNoteStep = (index) => {
                    newNoteChecklist.value.splice(index, 1)
                }

                const handleNewNoteEnter = (e) => {
                    if (e.target && (e.target.tagName === 'SELECT' || e.target.tagName === 'BUTTON')) return
                    if (newNoteStepInputRef.value && e.target === newNoteStepInputRef.value) return
                    if (newNoteTextareaRef.value && e.target === newNoteTextareaRef.value) return
                    e.preventDefault()
                    createNote()
                }

                const createNote = async () => {
                    if (!newNoteTitle.value.trim()) return

                    const storagePayload = {
                        text: newNoteContent.value,
                        checklist: newNoteChecklist.value
                    }

                    try {
                        const res = await fetch('/api/notes', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: newNoteTitle.value.trim(),
                                content: JSON.stringify(storagePayload),
                                status: newNoteStatus.value,
                                priority: newNotePriority.value,
                                due_date: newNoteDueDate.value || null
                            })
                        })
                        if (res.ok) {
                            const createdNote = await res.json()
                            notes.value.push(createdNote)
                            closeNewNote()
                        }
                    } catch (e) {
                        console.error('Fehler beim Erstellen', e)
                    }
                }

                const exportJson = () => {
                    if (notes.value.length === 0) return
                    const dataStr = "data:text/json;charset=utf-8," + encodeURIComponent(JSON.stringify(notes.value, null, 2))
                    const downloadAnchor = document.createElement('a')
                    downloadAnchor.setAttribute("href", dataStr)
                    downloadAnchor.setAttribute("download", "kanban_notes_export.json")
                    document.body.appendChild(downloadAnchor)
                    downloadAnchor.click()
                    downloadAnchor.remove()
                }

                const exportCsv = () => {
                    if (notes.value.length === 0) return
                    let csvContent = "data:text/csv;charset=utf-8,ID,Title,Status,Priority,Date,DueDate\r\n";
                    notes.value.forEach(note => {
                        let row = [note.id, `"${note.title.replace(/"/g, '""')}"`, note.status, note.priority, note.date, note.due_date || ''];
                        csvContent += row.join(",") + "\r\n";
                    });
                    const encodedUri = encodeURI(csvContent);
                    const downloadAnchor = document.createElement('a');
                    downloadAnchor.setAttribute("href", encodedUri);
                    downloadAnchor.setAttribute("download", "kanban_notes_export.csv");
                    document.body.appendChild(downloadAnchor);
                    downloadAnchor.click();
                    downloadAnchor.remove();
                }

                const openModal = (note) => {
                    activeNote.value = { ...note }
                    try {
                        const parsed = JSON.parse(note.content);
                        if (parsed && typeof parsed === 'object' && parsed.text !== undefined) {
                            activeNote.value.content = parsed.text || '';
                            activeChecklist.value = parsed.checklist || [];
                        } else {
                            activeChecklist.value = [];
                        }
                    } catch {
                        activeChecklist.value = [];
                    }
                    isPreviewMode.value = true
                    isModalOpen.value = true
                    closeContextMenu()
                }

                const closeModal = () => {
                    isModalOpen.value = false
                    activeNote.value = null
                    activeChecklist.value = []
                    newStepText.value = ''
                    closeContextMenu()
                    showAutocomplete.value = false
                    clearTimeout(autocompleteTimer)
                }

                // Kontextmenü Logik
                const openContextMenu = (e) => {
                    if (!textareaRef.value) return
                    const rect = e.target.getBoundingClientRect()
                    contextMenu.value = {
                        show: true,
                        x: e.clientX - rect.left,
                        y: e.clientY - rect.top,
                        selectionStart: textareaRef.value.selectionStart,
                        selectionEnd: textareaRef.value.selectionEnd
                    }
                }

                const closeContextMenu = () => {
                    contextMenu.value.show = false
                    exportMenuOpen.value = false
                }

                // --- Autocomplete für [[ Notiz-Links ---
                const searchNotesAutocomplete = async (q) => {
                    try {
                        const res = await fetch(`/api/notes/search?q=${encodeURIComponent(q)}`)
                        if (res.ok) {
                            const data = await res.json()
                            autocompleteResults.value = data
                            autocompleteIndex.value = 0
                            showAutocomplete.value = true
                            updateAutocompletePos()
                        } else {
                            showAutocomplete.value = false
                        }
                    } catch (e) {
                        showAutocomplete.value = false
                    }
                }

                const acTextarea = () => isNewNoteOpen.value ? newNoteTextareaRef.value : textareaRef.value
                const acMirror = () => isNewNoteOpen.value ? newNoteCaretMirrorRef.value : caretMirrorRef.value
                const acGetContent = () => isNewNoteOpen.value ? (newNoteContent.value || '') : (activeNote.value ? activeNote.value.content : '')
                const acSetContent = (str) => {
                    if (isNewNoteOpen.value) newNoteContent.value = str
                    else if (activeNote.value) activeNote.value.content = str
                }

                const updateAutocompletePos = () => {
                    const ta = acTextarea()
                    const mirror = acMirror()
                    if (!ta || !mirror) return
                    const cursor = ta.selectionStart
                    const text = acGetContent()
                    const before = text.substring(0, cursor)
                    const line = before.split('\n').length - 1
                    const colLines = before.split('\n')
                    const col = colLines[colLines.length - 1].length

                    // Spiegel aktualisieren: Text mit Markier-Span an der Cursorposition
                    let html = ''
                    for (let i = 0; i < colLines.length; i++) {
                        const chunk = colLines[i]
                        if (i < colLines.length - 1) {
                            html += chunk.replace(/</g, '&lt;') + '\n'
                        } else {
                            // letzte Zeile
                            const colonIdx = chunk.lastIndexOf(':')
                            if (colonIdx >= col) {
                                // Cursor in der Zeile, Marker nach prefix setzen
                                html += chunk.substring(0, col).replace(/</g, '&lt;')
                                html += '<span id="caret-marker">a</span>'
                            } else {
                                html += chunk.substring(0, col).replace(/</g, '&lt;')
                                html += '<span id="caret-marker">a</span>'
                            }
                        }
                    }
                    mirror.innerHTML = html + '<br>'

                    const marker = mirror.querySelector('#caret-marker')
                    if (!marker) return
                    const taRect = ta.getBoundingClientRect()
                    const markRect = marker.getBoundingClientRect()
                    const x = markRect.left - taRect.left
                    const y = markRect.top - taRect.top

                    // Flippen: wenn oberhalb genügend Platz / unterhalb nicht, Dropdown nach oben
                    const container = ta.closest('.md\\:col-span-2') || ta.parentElement
                    const containerHeight = container ? container.clientHeight : 300
                    const estHeight = Math.min(224, (autocompleteResults.value.length * 26) + 30)
                    const flip = (y + 24 + estHeight) > containerHeight && y > estHeight
                    autocompletePos.value = { x, y: flip ? (y - estHeight) : (y + 24), flip }
                }

                const cancelAutocomplete = () => {
                    clearTimeout(autocompleteTimer)
                    const ta = acTextarea()
                    if (ta) {
                        const text = acGetContent()
                        const cursor = ta.selectionStart
                        const start = autocompleteStart.value
                        const removeFrom = Math.max(0, start - 2)
                        if (text.substring(removeFrom, cursor).startsWith('[[')) {
                            acSetContent(text.substring(0, removeFrom) + text.substring(cursor))
                            const target = removeFrom
                            requestAnimationFrame(() => {
                                if (ta) {
                                    ta.selectionStart = ta.selectionEnd = target
                                    ta.focus()
                                }
                            })
                        }
                    }
                    showAutocomplete.value = false
                    autocompleteResults.value = []
                }

                const handleAutocomplete = () => {
                    const ta = acTextarea()
                    if (!ta) return
                    const text = acGetContent()
                    const cursor = ta.selectionStart

                    // Rückwärts suchen nach "[["
                    const lastOpen = text.lastIndexOf('[[', cursor - 1)
                    if (lastOpen === -1) {
                        showAutocomplete.value = false
                        return
                    }
                    // Wenn "]]" zwischen [[ und Cursor liegt, schließen
                    const between = text.substring(lastOpen + 2, cursor)
                    if (between.includes(']]')) {
                        showAutocomplete.value = false
                        return
                    }
                    // Nach "|" abbrechen (Anzeige-Titel wird typisiert)
                    if (between.includes('|')) {
                        showAutocomplete.value = false
                        return
                    }

                    autocompleteStart.value = lastOpen + 2
                    autocompleteQuery.value = between
                    updateAutocompletePos()
                    clearTimeout(autocompleteTimer)
                    autocompleteTimer = setTimeout(() => {
                        searchNotesAutocomplete(between)
                    }, 200)
                }

                const selectAutocomplete = (index) => {
                    const ta = acTextarea()
                    if (!ta) return
                    const selected = autocompleteResults.value[index]
                    if (!selected) return
                    const text = acGetContent()
                    const cursor = ta.selectionStart
                    const start = autocompleteStart.value
                    // Ersetze [[partial durch [[Vollständiger Titel
                    const newText = text.substring(0, start) + selected.title + ']]' + text.substring(cursor)
                    acSetContent(newText)
                    showAutocomplete.value = false
                    // Cursor hinter ]]
                    requestAnimationFrame(() => {
                        if (ta) {
                            const newCursor = start + selected.title.length + 2
                            ta.selectionStart = ta.selectionEnd = newCursor
                            ta.focus()
                        }
                    })
                }


                const handleAutocompleteKeydown = (e) => {
                    if (!showAutocomplete.value) return
                    if (e.key === 'ArrowDown') {
                        e.preventDefault()
                        autocompleteIndex.value = (autocompleteIndex.value + 1) % autocompleteResults.value.length
                    } else if (e.key === 'ArrowUp') {
                        e.preventDefault()
                        autocompleteIndex.value = (autocompleteIndex.value - 1 + autocompleteResults.value.length) % autocompleteResults.value.length
                    } else if (e.key === 'Enter') {
                        e.preventDefault()
                        selectAutocomplete(autocompleteIndex.value)
                    }
                }

                // --- Notiz-Links im Preview: [[Titel|Anzeige-Name]] ---
                const processNoteLinks = (html) => {
                    return html.replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (match, title, display) => {
                        const safeTitle = title.replace(/"/g, '&quot;')
                        const safeDisplay = (display || title).replace(/"/g, '&quot;')
                        return `<a class="note-link" data-note-title="${safeTitle}">${safeDisplay}</a>`
                    })
                }

                const handleNoteLinkClick = (e) => {
                    const link = e.target.closest('.note-link')
                    if (!link) return
                    e.preventDefault()
                    const title = link.getAttribute('data-note-title')
                    const note = notes.value.find(n => n.title === title)
                    if (note) {
                        openModal(note)
                    } else {
                        alert(`Notiz "${title}" nicht gefunden.`)
                    }
                }

                // --- Notiz duplizieren ---
                const duplicateNote = async (note) => {
                    try {
                        const res = await fetch(`/api/notes/${note.id}/duplicate`, { method: 'POST' })
                        if (res.ok) {
                            const newNote = await res.json()
                            notes.value.push(newNote)
                        }
                    } catch (e) {
                        console.error('Fehler beim Duplizieren', e)
                    }
                }

                // --- Archivieren / Wiederherstellen ---
                const archiveNote = async (note) => {
                    await updateNoteStatus(note, 'archived')
                }

                const unarchiveNote = async (note) => {
                    await updateNoteStatus(note, 'backlog')
                }

                const updateNoteStatus = async (note, newStatus) => {
                    try {
                        const res = await fetch(`/api/notes/${note.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: note.title,
                                content: note.content,
                                status: newStatus,
                                priority: note.priority,
                                due_date: note.due_date,
                                sort_order: -1
                            })
                        })
                        if (res.ok) {
                            note.status = newStatus
                            note.sort_order = -1
                        }
                    } catch (e) {
                        console.error('Fehler beim Status-Update', e)
                    }
                }

                // --- Drag & Drop Sortierung ---
                let dragOverNote = null

                const onDragOver = (note, e) => {
                    dragOverNote = note
                }

                const reorderColumn = async (columnId, dragged, target) => {
                    let colNotes = notes.value
                        .filter(n => n.status === columnId)
                        .sort((a, b) => (a.sort_order || 0) - (b.sort_order || 0) || a.id - b.id)

                    // Entferne das gezogene Element
                    const fromIndex = colNotes.findIndex(n => n.id === dragged.id)
                    if (fromIndex !== -1) colNotes.splice(fromIndex, 1)

                    // Finde Ziel-Index
                    let toIndex = colNotes.findIndex(n => n.id === target.id)
                    if (toIndex === -1) toIndex = colNotes.length

                    // Einfügen
                    colNotes.splice(toIndex, 0, dragged)

                    // Neue sort_order zuweisen
                    const updates = colNotes.map((n, i) => {
                        if ((n.sort_order || 0) !== i || n.id === dragged.id) {
                            return { id: n.id, sort_order: i }
                        }
                        return null
                    }).filter(Boolean)

                    // Lokal aktualisieren
                    colNotes.forEach((n, i) => { n.sort_order = i })
                    dragged.status = columnId

                    // API aktualisieren
                    await Promise.all(updates.map(async (u) => {
                        const n = notes.value.find(x => x.id === u.id)
                        if (!n) return
                        try {
                            await fetch(`/api/notes/${u.id}`, {
                                method: 'PUT',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({
                                    title: n.title,
                                    content: n.content,
                                    status: n.status,
                                    priority: n.priority,
                                    due_date: n.due_date,
                                    sort_order: u.sort_order
                                })
                            })
                        } catch (e) {
                            console.error('Fehler beim Sortieren', e)
                        }
                    }))
                }

                const applyFormat = (syntax) => {
                    if (!activeNote.value) return
                    const start = contextMenu.value.selectionStart
                    const end = contextMenu.value.selectionEnd
                    const text = activeNote.value.content
                    
                    const selectedText = text.substring(start, end)
                    const replacement = `${syntax}${selectedText || 'Text'}${syntax}`
                    
                    activeNote.value.content = text.substring(0, start) + replacement + text.substring(end)
                    closeContextMenu()
                }

                const applyHtmlColor = (colorHex) => {
                    if (!activeNote.value) return
                    const start = contextMenu.value.selectionStart
                    const end = contextMenu.value.selectionEnd
                    const text = activeNote.value.content
                    
                    const selectedText = text.substring(start, end)
                    const replacement = `<span style="color: ${colorHex}">${selectedText || 'Text'}</span>`
                    
                    activeNote.value.content = text.substring(0, start) + replacement + text.substring(end)
                    closeContextMenu()
                }

                const addStep = () => {
                    if (!newStepText.value.trim()) return
                    activeChecklist.value.push({ text: newStepText.value.trim(), done: false })
                    newStepText.value = ''
                }

                const removeStep = (index) => {
                    activeChecklist.value.splice(index, 1)
                }

                const renderedMarkdown = computed(() => {
                    if (!activeNote.value || !activeNote.value.content) return '<p class="text-zinc-500">Kein Inhalt vorhanden.</p>'
                    const html = marked.parse(activeNote.value.content)
                    return processNoteLinks(html)
                })

                const saveActiveNote = async () => {
                    if (!activeNote.value) return

                    const storagePayload = {
                        text: activeNote.value.content,
                        checklist: activeChecklist.value
                    };
                    const serializedContent = JSON.stringify(storagePayload);

                    try {
                        const res = await fetch(`/api/notes/${activeNote.value.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: activeNote.value.title,
                                content: serializedContent,
                                status: activeNote.value.status,
                                priority: activeNote.value.priority,
                                due_date: activeNote.value.due_date || null
                            })
                        })
                        if (res.ok) {
                            const index = notes.value.findIndex(n => n.id === activeNote.value.id)
                            if (index !== -1) {
                                notes.value[index] = { ...activeNote.value, content: serializedContent }
                            }
                            closeModal()
                        }
                    } catch (e) {
                        console.error('Fehler beim Aktualisieren', e)
                    }
                }

                const deleteNote = async (id) => {
                    try {
                        const res = await fetch(`/api/notes/${id}`, { method: 'DELETE' })
                        if (res.ok) {
                            notes.value = notes.value.filter(note => note.id !== id)
                        }
                    } catch (e) {
                        console.error('Fehler beim Löschen', e)
                    }
                }

                // --- Such-Syntax: key:value Parser ---
                const STATUS_ALIASES = {
                    'backlog': 'backlog',
                    'in_progress': 'in_progress',
                    'inprogress': 'in_progress',
                    'in-progress': 'in_progress',
                    'in arbeit': 'in_progress',
                    'review': 'review',
                    'done': 'done',
                    'abgeschlossen': 'done',
                    'archived': 'archived',
                    'archiv': 'archived'
                }

                const PRIO_ALIASES = {
                    'high': 'high',
                    'h': 'high',
                    'hoch': 'high',
                    'medium': 'medium',
                    'm': 'medium',
                    'mittel': 'medium',
                    'low': 'low',
                    'l': 'low',
                    'niedrig': 'low'
                }

                const parseFlexDate = (str) => {
                    // YYYY-MM-DD
                    let m = str.match(/^(\d{4})-(\d{2})-(\d{2})$/)
                    if (m) return new Date(+m[1], +m[2] - 1, +m[3]).getTime()
                    // DD.MM.YYYY
                    m = str.match(/^(\d{2})\.(\d{2})\.(\d{4})$/)
                    if (m) return new Date(+m[3], +m[2] - 1, +m[1]).getTime()
                    // YYYY-MM-DD ohne führende Nullen tolerant
                    m = str.match(/^(\d{4})-(\d{1,2})-(\d{1,2})$/)
                    if (m) return new Date(+m[1], +m[2] - 1, +m[3]).getTime()
                    return null
                }

                const startOfDay = (t) => new Date(new Date(t).setHours(0, 0, 0, 0)).getTime()
                const endOfDay = (t) => new Date(new Date(t).setHours(23, 59, 59, 999)).getTime()

                const parseSearchQuery = (raw) => {
                    const result = {
                        freeText: [],
                        titel: [],
                        inhalt: [],
                        status: [],
                        prio: [],
                        datum: [],
                        faellig: []
                    }
                    // Tokens erkennen key:value (Wert darf .. oder Datumszeichen enthalten, kein Leerzeichen)
                    const tokens = raw.match(/(\w+):([^\s]+)/g) || []
                    let rest = raw
                    tokens.forEach(tok => {
                        const idx = rest.indexOf(tok)
                        const valStart = tok.indexOf(':') + 1
                        const val = tok.substring(valStart)
                        const key = tok.substring(0, tok.indexOf(':')).toLowerCase()
                        // bereich: key:value
                        rest = rest.replace(tok, ' ')
                        if (key === 'titel' || key === 'title') result.titel.push(val)
                        else if (key === 'inhalt' || key === 'content') result.inhalt.push(val)
                        else if (key === 'status') result.status.push(val)
                        else if (key === 'prio' || key === 'priority') result.prio.push(val)
                        else if (key === 'datum' || key === 'date') result.datum.push(val)
                        else if (key === 'faellig' || key === 'due' || key === 'due_date' || key === 'fällig') result.faellig.push(val)
                        else result.freeText.push(tok)
                    })
                    // Übrig bleibende freie Texte
                    const freeTokens = rest.split(/\s+/).filter(t => t.trim() !== '')
                    result.freeText.push(...freeTokens)
                    return result
                }

                const evalDateSpec = (fieldVal, spec) => {
                    const today = startOfDay(Date.now())
                    const endToday = endOfDay(Date.now())
                    if (spec === 'none') return !fieldVal || fieldVal.trim() === ''
                    if (spec === 'any') return !!fieldVal && fieldVal.trim() !== ''
                    if (!fieldVal || fieldVal.trim() === '') return false

                    const valTime = parseFlexDate(fieldVal)
                    if (valTime === null) return false

                    if (spec === 'today') return valTime >= today && valTime <= endToday
                    if (spec === 'overdue') return valTime < today
                    if (spec === 'this_week' || spec === 'thisweek' || spec === 'woche') {
                        const now = new Date()
                        const day = (now.getDay() + 6) % 7
                        const monday = startOfDay(new Date(now).setDate(now.getDate() - day))
                        const sunday = endOfDay(new Date(now).setDate(now.getDate() - day + 6))
                        return valTime >= monday && valTime <= sunday
                    }
                    if (spec.startsWith('>=')) {
                        const t = parseFlexDate(spec.substring(2))
                        return t !== null && valTime >= startOfDay(t)
                    }
                    if (spec.startsWith('<=')) {
                        const t = parseFlexDate(spec.substring(2))
                        return t !== null && valTime <= endOfDay(t)
                    }
                    if (spec.startsWith('>')) {
                        const t = parseFlexDate(spec.substring(1))
                        return t !== null && valTime > endOfDay(t)
                    }
                    if (spec.startsWith('<')) {
                        const t = parseFlexDate(spec.substring(1))
                        return t !== null && valTime < startOfDay(t)
                    }
                    if (spec.includes('..')) {
                        const [a, b] = spec.split('..')
                        const tA = parseFlexDate(a)
                        const tB = parseFlexDate(b)
                        if (tA === null || tB === null) return false
                        return valTime >= startOfDay(tA) && valTime <= endOfDay(tB)
                    }
                    const t = parseFlexDate(spec)
                    if (t === null) return false
                    return valTime >= startOfDay(t) && valTime <= endOfDay(t)
                }

                const notePlainContent = (note) => {
                    try {
                        const parsed = JSON.parse(note.content)
                        if (parsed && typeof parsed === 'object' && parsed.text !== undefined) return String(parsed.text)
                    } catch {}
                    return note.content || ''
                }

                const matchesNote = (note, q) => {
                    if (q.freeText.length) {
                        const haystack = (note.title + ' ' + notePlainContent(note)).toLowerCase()
                        if (!q.freeText.every(t => haystack.includes(t.toLowerCase()))) return false
                    }
                    if (q.titel.length) {
                        const title = note.title.toLowerCase()
                        if (!q.titel.every(t => title.includes(t.toLowerCase()))) return false
                    }
                    if (q.inhalt.length) {
                        const content = notePlainContent(note).toLowerCase()
                        if (!q.inhalt.every(t => content.includes(t.toLowerCase()))) return false
                    }
                    if (q.status.length) {
                        const resolved = q.status.map(s => STATUS_ALIASES[s.trim().toLowerCase()]).filter(Boolean)
                        if (resolved.length && !resolved.includes(note.status)) return false
                    }
                    if (q.prio.length) {
                        const resolved = q.prio.map(p => PRIO_ALIASES[p.trim().toLowerCase()]).filter(Boolean)
                        if (resolved.length && !resolved.includes(note.priority)) return false
                    }
                    if (q.datum.length) {
                        if (!q.datum.every(d => evalDateSpec(note.date, d))) return false
                    }
                    if (q.faellig.length) {
                        if (!q.faellig.every(d => evalDateSpec(note.due_date, d))) return false
                    }
                    return true
                }

                const getNotesByColumn = (status) => {
                    let filtered = notes.value.filter(note => note.status === status)
                    filtered.sort((a, b) => (a.sort_order || 0) - (b.sort_order || 0) || a.id - b.id)
                    if (searchQuery.value.trim() !== '') {
                        const q = parseSearchQuery(searchQuery.value)
                        filtered = filtered.filter(note => matchesNote(note, q))
                    }
                    return filtered
                }

                const startDrag = (note) => {
                    draggedNote.value = note
                }

                const onDrop = async (columnId, targetNote) => {
                    if (!draggedNote.value) return
                    const dragged = draggedNote.value

                    if (targetNote && dragged.id === targetNote.id) {
                        draggedNote.value = null
                        return
                    }

                    if (dragged.status === columnId && targetNote) {
                        // Reorder innerhalb derselben Spalte
                        await reorderColumn(columnId, dragged, targetNote)
                    } else if (dragged.status !== columnId) {
                        // Über Spalte wechseln, an Ende anhängen
                        dragged.status = columnId
                        const maxSort = notes.value
                            .filter(n => n.status === columnId && n.id !== dragged.id)
                            .reduce((max, n) => Math.max(max, n.sort_order || 0), 0)
                        dragged.sort_order = maxSort + 1
                        await fetch(`/api/notes/${dragged.id}`, {
                            method: 'PUT',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: dragged.title,
                                content: dragged.content,
                                status: dragged.status,
                                priority: dragged.priority,
                                due_date: dragged.due_date,
                                sort_order: dragged.sort_order
                            })
                        })
                        if (targetNote && columnId !== 'archived') {
                            await reorderColumn(columnId, dragged, targetNote)
                        }
                    }
                    draggedNote.value = null
                }

                return {
                    newNoteTitle,
                    newNotePriority,
                    newNoteDueDate,
                    newNoteStatus,
                    newNoteContent,
                    newNoteChecklist,
                    newNoteStepText,
                    isNewNoteOpen,
                    newNoteTitleInputRef,
                    newNoteTextareaRef,
                    newNoteCaretMirrorRef,
                    newNoteStepInputRef,
                    columns,
                    notes,
                    isModalOpen,
                    isPreviewMode,
                    activeNote,
                    activeChecklist,
                    newStepText,
                    isBacklogCollapsed,
                    isArchiveCollapsed,
                    collapsedStatuses,
                    expandedColumns,
                    isCollapsed,
                    toggleCollapse,
                    expandColumn,
                    collapseColumn,
                    gridColsStyle,
                    searchQuery,
                    searchInputRef,
                    textareaRef,
                    caretMirrorRef,
                    isOverdue,
                    isDueSoon,
                    renderedMarkdown,
                    contextMenu,
                    exportMenuOpen,
                    openContextMenu,
                    closeContextMenu,
                    applyFormat,
                    applyHtmlColor,
                    openNewNote,
                    closeNewNote,
                    createNote,
                    addNewNoteStep,
                    removeNewNoteStep,
                    handleNewNoteEnter,
                    cancelAutocomplete,
                    exportJson,
                    exportCsv,
                    openModal,
                    closeModal,
                    addStep,
                    removeStep,
                    saveActiveNote,
                    deleteNote,
                    duplicateNote,
                    archiveNote,
                    unarchiveNote,
                    getNotesByColumn,
                    startDrag,
                    onDragOver,
                    onDrop,
                    handleAutocomplete,
                    handleAutocompleteKeydown,
                    selectAutocomplete,
                    showAutocomplete,
                    autocompleteResults,
                    autocompleteIndex,
                    autocompletePos,
                    updateAutocompletePos,
                    handleNoteLinkClick,
                    focusSearchInput,
                    processNoteLinks
                }
            }
        }).mount('#app')
    </script>
</body>
</html>
"#;

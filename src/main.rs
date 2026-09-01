use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post, put, delete},
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
            due_date TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Fehler beim Erstellen der Tabelle");

    let state = AppState { pool };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/notes", get(get_notes).post(create_note))
        .route("/api/notes/:id", post(update_note).put(update_note).delete(delete_note))
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
    match sqlx::query_as::<_, Note>("SELECT id, title, content, status, priority, date, due_date FROM notes")
        .fetch_all(&state.pool)
        .await
    {
        Ok(notes) => Json(notes).into_response(),
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

    let result = sqlx::query(
        r#"
        INSERT INTO notes (title, content, status, priority, date, due_date)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(payload.title)
    .bind(content)
    .bind(status)
    .bind(priority)
    .bind(date)
    .bind(due_date)
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

    let result = sqlx::query(
        r#"
        UPDATE notes SET title = ?, content = ?, status = ?, priority = ?, due_date = ? WHERE id = ?
        "#,
    )
    .bind(title)
    .bind(content)
    .bind(status)
    .bind(priority)
    .bind(due_date)
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
    </style>
</head>
<body>
    <div id="app" class="h-screen flex flex-col" @click="closeContextMenu">
        <header class="bg-zinc-900 border-b border-zinc-800 px-3 py-2 flex justify-between items-center shrink-0 flex-wrap gap-2">
            <h1 class="text-xs font-bold tracking-widest text-emerald-400 flex items-center gap-2">
                <span class="inline-block w-2 h-2 bg-emerald-500"></span> NOTICE_V1.1
            </h1>

            <div v-if="urgentNotes.length > 0" class="bg-orange-950/40 border border-orange-500/50 text-orange-300 px-2 py-0.5 text-[10px] animate-pulse whitespace-nowrap">
                ⚠️ {{ urgentNotes.length }} fällig/überfällig!
            </div>

            <div class="flex gap-1.5 items-center flex-wrap">
                <input 
                    ref="searchInputRef"
                    type="text" 
                    v-model="searchQuery" 
                    placeholder="Suchen (Strg+K)..." 
                    class="bg-zinc-950 border border-zinc-700 px-2.5 py-1 text-xs text-zinc-100 focus:outline-none focus:border-emerald-500 w-36"
                >
                <input 
                    ref="titleInputRef"
                    type="text" 
                    v-model="newNoteTitle" 
                    @keyup.enter="addNote"
                    placeholder="Titel (Alt+N)..." 
                    class="bg-zinc-950 border border-zinc-700 px-2.5 py-1 text-xs text-zinc-100 focus:outline-none focus:border-emerald-500 w-48"
                >
                <select v-model="newNotePriority" class="bg-zinc-950 border border-zinc-700 px-2 py-1 text-xs text-zinc-300 focus:outline-none">
                    <option value="low">Prio: Niedrig</option>
                    <option value="medium">Prio: Mittel</option>
                    <option value="high">Prio: Hoch</option>
                </select>
                <input 
                    type="date" 
                    v-model="newNoteDueDate" 
                    class="bg-zinc-950 border border-zinc-700 px-2 py-1 text-xs text-zinc-300 focus:outline-none"
                >
                <button 
                    @click="addNote" 
                    class="bg-emerald-700 hover:bg-emerald-600 text-zinc-100 px-3 py-1 text-xs border border-emerald-600 font-semibold cursor-pointer transition-colors">
                    + HINZUFÜGEN
                </button>
                <button 
                    @click="exportJson" 
                    :disabled="notes.length === 0"
                    :class="notes.length === 0 ? 'opacity-40 cursor-not-allowed bg-zinc-900 border-zinc-800 text-zinc-600' : 'bg-zinc-800 hover:bg-zinc-700 text-zinc-300 border-zinc-700 cursor-pointer'"
                    class="px-2.5 py-1 text-xs border">
                    📥 JSON
                </button>
                <button 
                    @click="exportCsv" 
                    :disabled="notes.length === 0"
                    :class="notes.length === 0 ? 'opacity-40 cursor-not-allowed bg-zinc-900 border-zinc-800 text-zinc-600' : 'bg-zinc-800 hover:bg-zinc-700 text-zinc-300 border-zinc-700 cursor-pointer'"
                    class="px-2.5 py-1 text-xs border">
                    📊 CSV
                </button>
            </div>
        </header>

        <main class="flex-1 grid gap-1.5 p-2 overflow-hidden bg-zinc-950 transition-all duration-300" :class="isBacklogCollapsed ? 'grid-cols-[40px_1fr_1fr_1fr]' : 'grid-cols-4'">

            <!-- Spalte 1: Backlog -->
            <div 
                class="bg-zinc-900/70 border border-zinc-800 flex flex-col h-full overflow-hidden transition-all"
                @dragover.prevent
                @drop="onDrop('backlog')"
            >
                <div class="bg-zinc-900 border-b border-zinc-800 px-2.5 py-1.5 flex justify-between items-center shrink-0">
                    <button @click="isBacklogCollapsed = !isBacklogCollapsed" class="text-[11px] font-bold tracking-wider text-zinc-300 uppercase flex items-center gap-1.5 cursor-pointer">
                        <span class="w-1.5 h-1.5 bg-zinc-500"></span>
                        <span v-if="!isBacklogCollapsed">01_Backlog</span>
                        <span v-else class="rotate-90 inline-block">...</span>
                    </button>
                    <span v-if="!isBacklogCollapsed" class="bg-zinc-950 text-zinc-400 text-[10px] px-1.5 py-0.5 border border-zinc-800">
                        {{ getNotesByColumn('backlog').length }}
                    </span>
                </div>

                <div v-if="!isBacklogCollapsed" class="flex-1 p-1.5 overflow-y-auto space-y-1.5">
                    <div 
                        v-for="note in getNotesByColumn('backlog')" 
                        :key="note.id"
                        draggable="true"
                        @dragstart="startDrag(note)"
                        @dblclick="openModal(note)"
                        class="bg-zinc-900 border p-2.5 cursor-pointer hover:border-zinc-500 transition-colors shadow-sm group relative"
                        :class="{
                            'border-red-500/80 bg-red-950/10': note.priority === 'high',
                            'border-amber-500/80 bg-amber-950/10': note.priority === 'medium',
                            'border-zinc-700 bg-zinc-900': note.priority === 'low'
                        }"
                    >
                        <div class="flex justify-between items-start gap-1 mb-1.5">
                            <h3 class="font-bold text-xs text-zinc-100 break-all leading-snug pr-1">{{ note.title }}</h3>
                            <button @click.stop="deleteNote(note.id)" class="text-zinc-600 hover:text-red-400 text-[10px] px-1 font-mono shrink-0">[X]</button>
                        </div>
                        <div class="flex justify-between items-center text-[10px] text-zinc-400 pt-1 border-t border-zinc-800/60">
                            <span v-if="note.due_date" class="text-orange-400/90 font-mono">Fällig: {{ note.due_date }}</span>
                            <span v-else class="text-zinc-600 font-mono">Kein Datum</span>
                            <span class="uppercase tracking-widest text-[9px] px-1 bg-zinc-950 border border-zinc-800 text-zinc-400">{{ note.priority }}</span>
                        </div>
                    </div>
                </div>
            </div>

            <!-- Spalten 2 bis 4 -->
            <template v-for="column in columns.slice(1)" :key="column.id">
                <div 
                    class="bg-zinc-900/70 border border-zinc-800 flex flex-col h-full overflow-hidden"
                    @dragover.prevent
                    @drop="onDrop(column.id)"
                >
                    <div class="bg-zinc-900 border-b border-zinc-800 px-2.5 py-1.5 flex justify-between items-center shrink-0">
                        <span class="text-[11px] font-bold tracking-wider text-zinc-300 uppercase flex items-center gap-1.5">
                            <span class="w-1.5 h-1.5" :class="{
                                'bg-blue-500': column.id === 'in_progress',
                                'bg-amber-500': column.id === 'review',
                                'bg-emerald-500': column.id === 'done'
                            }"></span>
                            {{ column.title }}
                        </span>
                        <span class="bg-zinc-950 text-zinc-400 text-[10px] px-1.5 py-0.5 border border-zinc-800">
                            {{ getNotesByColumn(column.id).length }}
                        </span>
                    </div>

                    <div class="flex-1 p-1.5 overflow-y-auto space-y-1.5">
                        <div 
                            v-for="note in getNotesByColumn(column.id)" 
                            :key="note.id"
                            draggable="true"
                            @dragstart="startDrag(note)"
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
                                <button @click.stop="deleteNote(note.id)" class="text-zinc-600 hover:text-red-400 text-[10px] px-1 font-mono shrink-0">[X]</button>
                            </div>

                            <div class="flex justify-between items-center text-[10px] text-zinc-400 pt-1 border-t border-zinc-800/60">
                                <span v-if="note.due_date" class="text-orange-400/90 font-mono">Fällig: {{ note.due_date }}</span>
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
            </template>
        </main>

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
                            placeholder="Inhalt mit Markdown schreiben... (Rechtsklick für Text-Formatierung)"
                            class="w-full h-full bg-zinc-900 border border-zinc-800 text-zinc-200 p-2.5 text-xs font-mono resize-none focus:outline-none focus:border-zinc-600"
                        ></textarea>
                        <div 
                             v-else 
                             class="markdown-body w-full h-full bg-zinc-900 border border-zinc-800 p-2.5 text-zinc-200 overflow-y-auto text-xs"
                             v-html="renderedMarkdown">
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
                const draggedNote = ref(null)
                const isBacklogCollapsed = ref(false)
                const searchQuery = ref('')
                const titleInputRef = ref(null)
                const searchInputRef = ref(null)
                const textareaRef = ref(null)

                const isModalOpen = ref(false)
                const isPreviewMode = ref(false)
                const activeNote = ref(null)
                const activeChecklist = ref([])
                const newStepText = ref('')

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

                const focusTitleInput = () => {
                    if (titleInputRef.value) titleInputRef.value.focus()
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
                        focusTitleInput()
                    } else if (isCtrlOrMeta && (e.key === 'k' || e.key === 'K' || e.code === 'KeyK')) {
                        e.preventDefault()
                        e.stopPropagation()
                        e.stopImmediatePropagation()
                        focusSearchInput()
                    }
                }

                onMounted(() => {
                    fetchNotes()
                    window.addEventListener('keydown', handleGlobalKeydown, { capture: true })
                })

                onUnmounted(() => {
                    window.removeEventListener('keydown', handleGlobalKeydown, { capture: true })
                })

                const urgentNotes = computed(() => {
                    const today = new Date()
                    today.setHours(0, 0, 0, 0)

                    return notes.value.filter(note => {
                        if (note.status === 'done' || !note.due_date) return false
                        const dueDate = new Date(note.due_date)
                        dueDate.setHours(0, 0, 0, 0)

                        const diffTime = dueDate - today
                        const diffDays = Math.ceil(diffTime / (1000 * 60 * 60 * 24))

                        return diffDays <= 3
                    })
                })

                const addNote = async () => {
                    if (!newNoteTitle.value.trim()) return

                    try {
                        const res = await fetch('/api/notes', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({
                                title: newNoteTitle.value.trim(),
                                content: '',
                                status: 'backlog',
                                priority: newNotePriority.value,
                                due_date: newNoteDueDate.value || null
                            })
                        })
                        if (res.ok) {
                            const createdNote = await res.json()
                            notes.value.push(createdNote)
                            newNoteTitle.value = ''
                            newNotePriority.value = 'medium'
                            newNoteDueDate.value = ''
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
                    isPreviewMode.value = false
                    isModalOpen.value = true
                    closeContextMenu()
                }

                const closeModal = () => {
                    isModalOpen.value = false
                    activeNote.value = null
                    activeChecklist.value = []
                    newStepText.value = ''
                    closeContextMenu()
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
                    return marked.parse(activeNote.value.content)
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

                const getNotesByColumn = (status) => {
                    let filtered = notes.value.filter(note => note.status === status)
                    if (searchQuery.value.trim() !== '') {
                        const q = searchQuery.value.toLowerCase()
                        filtered = filtered.filter(note => {
                            let plainContent = note.content;
                            try {
                                const parsed = JSON.parse(note.content);
                                if (parsed.text) plainContent = parsed.text;
                            } catch {}
                            return note.title.toLowerCase().includes(q) || plainContent.toLowerCase().includes(q)
                        })
                    }
                    return filtered
                }

                const startDrag = (note) => {
                    draggedNote.value = note
                }

                const onDrop = async (columnId) => {
                    if (draggedNote.value && draggedNote.value.status !== columnId) {
                        draggedNote.value.status = columnId
                        try {
                            await fetch(`/api/notes/${draggedNote.value.id}`, {
                                method: 'PUT',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({
                                    title: draggedNote.value.title,
                                    content: draggedNote.value.content,
                                    status: draggedNote.value.status,
                                    priority: draggedNote.value.priority,
                                    due_date: draggedNote.value.due_date
                                })
                            })
                        } catch (e) {
                            console.error('Fehler beim Status-Update', e)
                        }
                        draggedNote.value = null
                    }
                }

                return {
                    newNoteTitle,
                    newNotePriority,
                    newNoteDueDate,
                    columns,
                    notes,
                    isModalOpen,
                    isPreviewMode,
                    activeNote,
                    activeChecklist,
                    newStepText,
                    isBacklogCollapsed,
                    searchQuery,
                    titleInputRef,
                    searchInputRef,
                    textareaRef,
                    urgentNotes,
                    renderedMarkdown,
                    contextMenu,
                    openContextMenu,
                    closeContextMenu,
                    applyFormat,
                    applyHtmlColor,
                    addNote,
                    exportJson,
                    exportCsv,
                    openModal,
                    closeModal,
                    addStep,
                    removeStep,
                    saveActiveNote,
                    deleteNote,
                    getNotesByColumn,
                    startDrag,
                    onDrop,
                    focusTitleInput,
                    focusSearchInput
                }
            }
        }).mount('#app')
    </script>
</body>
</html>
"#;

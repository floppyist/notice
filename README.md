<p align="center">
  <b><span style="color:#34d399">■</span> NOTICE_V1.2</b> — <i>Pro Kanban Notes</i>
</p>

<p align="center">
  A compact, single-file Kanban notes board as a web app:<br/>
  an <b>Axum REST API (Rust)</b> serves an embedded <b>Vue 3 / Tailwind</b> UI and stores everything in a SQLite database. Backend, frontend HTML and JavaScript all live in a single file (<code>src/main.rs</code>).
</p>

---

## `// VIEWS` — Board & Calendar

Two views can be switched via the header bar (`BOARD` / `CALENDAR`).

![Board view](screenshots/board.png)

**Board** – columns for `Backlog`, `In Progress`, `Review` and `Done`, plus a dedicated **collapsed** archive by default.

![Calendar view (month)](screenshots/kalender.png)

**Calendar** – Outlook-style view with `MONTH`, `WEEK` and `YEAR`. Cards with a due date appear as **blue** markers, booked **appointments** as **green**, overdue ones as **red**. Archived notes are hidden.

![Detail modal](screenshots/detail.png)

**Detail modal** (double-click a card) – title, status, priority, due date, departments, appointments and content/checklist editing with a preview mode.

---

## `// FEATURES`

- **Kanban board** with collapsible columns:
  - Collapsed statuses show as a **vertical tab stack on the left** (with a divider under each tab) – including the **archive**.
  - Expanded columns fill a grid in the middle.
- **Calendar** (`MONTH` / `WEEK` / `YEAR`) combines `due_date` (blue), appointments (green) and overdue (red).
- **New-note form** (`Alt+N`): title, status, priority, due date, content (Markdown), **checklist/steps** and **appointments**.
- **Detail modal** (double-click): all fields incl. **departments** and **appointments**, with an **edit/preview toggle**.
- **Markdown** for the content, incl. **supported HTML colors** and **cross-linked notes** via `[[Title|Display name]]` (with autocomplete).
- **Departments**: optional per note, freely creatable, **autocomplete** (most frequent first, case-insensitive deduplication). On cards the departments render as **inline wrapping badges** (single line, no emoji).
- **Appointments**: any number per note, with start, optional time (`HH:MM`), optional end date and end time.
- **Week view** is a 24-hour timeline with position/styleable event blocks; overlapping events are placed side by side in lanes; timed and all-day events are handled separately.
- **Drag & Drop** between columns as well as for reordering within a column; notes can be dragged directly into the **archive**.
- **Archive**: archive notes with `[A]`, restore with `[R]`, delete with `[X]`.
- **Priority and due-date badges** on the cards (`OVERDUE` and `DUE` respectively).
- **Overdue reminder**: on startup (and after changes) a floating panel lists overdue appointments and due dates.
- **Auto-archive**: optional (settings, default day = 1st), moves all `Done` notes to the archive on startup when that day is reached.
- **Export** as **JSON** or **CSV** via the export menu.
- **Dark/light theme**: dark by default, toggle in the header, persists in `localStorage`.
- **Full searchability** (`Ctrl+K`), incl. structured `key:value` syntax (see below).

---

## `// QUICKSTART`

### Prerequisites

- Rust (stable) incl. Cargo

### Build & Run

```bash
cargo build
cargo run
# → serves at http://127.0.0.1:8080
```

On first start, `notice.db` (SQLite) is created automatically. Existing databases are migrated with `ALTER TABLE` (e.g. `departments`, `appointments`), existing data is preserved.

---

## `// USAGE`

| Action | Input |
|---|---|
| Focus the search bar | `Ctrl+K` |
| Open a new note | `Alt+N` |
| Open a note in the detail modal | Double-click a card |
| Toggle edit / preview | Button `Edit` / `Preview` |
| Save (in the modal) | `Ctrl+S` |
| Escape | Closes the modal/form or autocomplete (removes the `[[` remnant) |
| Link a note | `[[` in the content, then pick with `↑↓`/`Enter` |

### Departments

- On focus the **most frequent** existing departments are suggested, filtered live as you type.
- Inputs are **case-insensitively deduplicated** ("küche" becomes "Küche").
- Departments render as badges on the cards.

### Appointments

- In form and modal you can create appointments with `Start`, optional `Time` (`HH:MM`), optional `End` (date) and `End time`.
- Appointments appear as **green** calendar entries and in the card/modal list under "Appointments".

---

## `// SEARCH SYNTAX`

The search covers e.g. title, content and departments. It additionally supports `key:value` filters:

```
status:done
status:in_progress          # or: inprogress / in arbeit
prio:high                   # or: priority:hoch / h
datum:2026-09-01            # or datum today / none / any / this_week
faellig:overdue             # or due / due_date
faellig:>=2026-09-01
faellig:2026-09-01..2026-09-30
dep:Küche
titel:Part-of-title
inhalt:Keyword
```

Multiple free-text terms are AND-ed together.

---

## `// API`

### Notes

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/notes` | All notes (sorted by `sort_order`) |
| `POST` | `/api/notes` | Create a new note |
| `POST` / `PUT` | `/api/notes/:id` | Update a note (both intentionally mapped) |
| `DELETE` | `/api/notes/:id` | Delete a note |
| `GET` | `/api/notes/search?q=…` | Search for the `[[` autocomplete |
| `POST` | `/api/notes/:id/duplicate` | Duplicate a note (title gets a `(copy)` suffix) |

Example `POST /api/notes`:

```json
{
  "title": "Einkauf",
  "content": "{\"text\": \"Milch\", \"checklist\": []}",
  "status": "backlog",
  "priority": "medium",
  "due_date": "2026-09-30",
  "departments": "[\"Küche\"]",
  "appointments": "[{\"title\": \"Markt\", \"start\": \"2026-09-30\", \"time\": \"14:30\"}]"
}
```

> **Note:** `content` stores **JSON** (`{"text": …, "checklist": […]}`), not plain text. Likewise `departments` and `appointments` are **JSON-array strings** – never send raw strings/objects, always `JSON.stringify(...)` from the JS.

### Departments

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/departments` | `[{name, count}]`, sorted by frequency, case-insensitively deduplicated |

---

## `// DATA MODEL`

Table `notes`:

| Column | Type | Meaning |
|---|---|---|
| `id` | INTEGER (PK) | Auto ID |
| `title` | TEXT | Title |
| `content` | TEXT | Content as JSON (`text` + `checklist`) |
| `status` | TEXT | `backlog`, `in_progress`, `review`, `done`, `archived` |
| `priority` | TEXT | `low`, `medium`, `high` |
| `date` | TEXT | Creation date (DD.MM.YYYY) |
| `due_date` | TEXT | Optional due date (YYYY-MM-DD) |
| `department` | TEXT | Legacy: single department (compatibility) |
| `departments` | TEXT | JSON array of departments |
| `appointments` | TEXT | JSON array of appointments |
| `sort_order` | INTEGER | Sort order within a column |

---

## `// PROJECT STRUCTURE`

- `src/main.rs` – the whole app (Rust backend, database setup, embedded HTML/JS frontend).
- `notice.db` – SQLite database, created at runtime (in `.gitignore`).
- `screenshots/` – screenshots of the UI (board, calendar, detail modal).
- `AGENTS.md` – extra notes for development assistants.

---

## `// TECHNICAL NOTES`

- `sqlx` connects via `sqlite://notice.db?mode=rwc`; `?mode=rwc` creates the file when needed.
- The SQL queries are runtime strings (no `sqlx::query!` macros) – no compile-time SQL checks.
- The frontend libraries (Vue production, Tailwind, marked) are **embedded locally** (offline-capable) instead of a CDN.
- The whole UI and all texts are in **German**.
- No test/lint/CI setup; verified via `cargo build` + manual smoke test.

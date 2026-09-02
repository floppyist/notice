<p align="center">
  <b><span style="color:#34d399">■</span> NOTICE_V1.2</b> — <i>Pro Kanban Notes</i>
</p>

<p align="center">
  Ein kompaktes, einzeldateibasiertes Kanban-Notizboard als webbasierte App:<br/>
  <b>Axum REST-API (Rust)</b> serviert eine eingebettete <b>Vue 3 / Tailwind</b>-Oberfläche und speichert alles in einer SQLite-Datenbank. Backend, Frontend-HTML und JavaScript leben in einer einzigen Datei (<code>src/main.rs</code>).
</p>

---

## `// VIEWS` — Board & Kalender

Zwei Ansichten lassen sich über die Kopfleiste (`BOARD` / `KALENDER`) umschalten.

![Board-Ansicht](screenshots/board.png)

**Board** – Spalten für `Backlog`, `In Arbeit`, `Review` und `Abgeschlossen` sowie ein eigenes, standardmäßig **eingeklapptes Archiv**.

![Kalender-Ansicht (Monat)](screenshots/kalender.png)

**Kalender** – Outlook-artige Ansicht mit `MONAT`, `WOCHE` und `JAHR`. Karten mit Fälligkeitsdatum erscheinen als **blaue** Markierung, gebuchte **Termine** als **grüne**, überfällige als **rote**. Archivierte Notizen werden ausgeblendet.

![Detail-Modal](screenshots/detail.png)

**Detail-Modal** (Doppelklick auf eine Karte) – Titel, Status, Priorität, Fälligkeit, Departments, Termine und Inhalts-/Checklisten-Bearbeitung mit Vorschau-Modus.

---

## `// FEATURES`

- **Kanban-Board** mit ein-/ausklappbaren Spalten:
  - Eingeklappte Status erscheinen als **vertikaler Tab-Stapel links** (mit Trennlinie unter jedem Tab) – inklusive des **Archivs**.
  - Erweiterte Spalten füllen ein Grid in der Mitte.
- **Kalender** (`MONAT` / `WOCHE` / `JAHR`) kombiniert `due_date` (blau), `Termine` (grün) und Überfälliges (rot).
- **Neue-Notiz-Maske** (`Alt+N`): Titel, Status, Priorität, Fälligkeit, Inhalt (Markdown), **Checkliste/Steps** und **Termine**.
- **Detail-Modal** (Doppelklick): alle Felder inkl. **Departments** und **Termine**, mit **Bearbeiten/Vorschau-Umschaltung**.
- **Markdown** für den Inhalt inkl. **unterstützter HTML-Farben** und **verlinkbarer Notizen** über `[[Titel|Anzeige-Name]]` (mit Autocomplete).
- **Departments**: optional pro Notiz, frei erstellbar, **Autocomplete** (häufigste zuerst, case-insensitiv dedupliziert). Auf Karten erscheinen die Departments als **inline umbrechende Badges** (eine Zeile, ohne Emoji).
- **Termine**: pro Notiz beliebig viele, mit Start, optionaler Uhrzeit (`HH:MM`), optionalem End-Datum und End-Uhrzeit.
- **Drag & Drop** zwischen Spalten sowie zum Umsortieren innerhalb einer Spalte; Notizen lassen sich direkt ins **Archiv** ziehen.
- **Archiv**: Notizen per `[A]` archivieren, per `[R]` wiederherstellen, `[X]` löschen.
- **Prioritäts- und Fälligkeits-Badges** auf den Karten (`ÜBERFÄLLIG` bzw. `FÄLLIG`).
- **Export** als **JSON** oder **CSV** über das Export-Menü.
- **Volle Durchsuchbarkeit** (`Strg+K`), inkl. strukturierter `key:value`-Syntax (siehe unten).

---

## `// QUICKSTART`

### Voraussetzungen

- Rust (stable) inkl. Cargo

### Build & Start

```bash
cargo build
cargo run
# → läuft auf http://127.0.0.1:8080
```

Beim ersten Start wird `notice.db` (SQLite) automatisch angelegt. Vorhandene Datenbanken werden per `ALTER TABLE` migriert (z. B. `departments`, `appointments`), bestehende Daten bleiben erhalten.

---

## `// BEDIENUNG`

| Aktion | Eingabe |
|---|---|
| Suchleiste fokussieren | `Strg+K` |
| Neue Notiz öffnen | `Alt+N` |
| Notiz im Detail-Modal öffnen | Doppelklick auf Karte |
| Bearbeiten / Vorschau umschalten | Knopf `Bearbeiten` / `Vorschau` |
| Speichern (im Modal) | `Strg+S` |
| Escape | Schließt Modal/Maske bzw. Autocomplete (entfernt `[[`-Rest) |
| Notiz verlinken | `[[` im Inhalt, dann `↑↓`/`Enter` auswählen |

### Departments

- Beim Fokussieren werden die **häufigsten** vorhandenen Departments vorgeschlagen, beim Tippen live gefiltert.
- Eingaben werden **case-insensitiv dedupliziert** („küche“ wird zu „Küche“).
- Auf Karten erscheinen Departments als Badges.

### Termine

- In Maske und Modal lassen sich Termine mit `Start`, optionaler `Zeit` (`HH:MM`), optionalem `Ende` (Datum) und `End-Zeit` anlegen.
- Termine erscheinen als **grüne** Kalendereinträge und in der Karten-/Modal-Liste unter „Termine“.

---

## `// SUCHSYNTAX`

Die Suche durchsucht u. a. Titel, Inhalt und Departments. Zusätzlich unterstützt sie `key:value`-Filter:

```
status:done
status:in_progress          # oder: inprogress / in arbeit
prio:high                   # oder: priority:hoch / h
datum:2026-09-01            # oder datum today / none / any / this_week
faellig:overdue             # oder due / due_date
faellig:>=2026-09-01
faellig:2026-09-01..2026-09-30
dep:Küche
titel:Teiltext
inhalt:Schlüsselwort
```

Mehrere Freitext-Begriffe werden als UND verknüpft.

---

## `// API`

### Notizen

| Methode | Pfad | Beschreibung |
|---|---|---|
| `GET` | `/api/notes` | Alle Notizen (sortiert nach `sort_order`) |
| `POST` | `/api/notes` | Neue Notiz anlegen |
| `POST` / `PUT` | `/api/notes/:id` | Notiz aktualisieren (bewusst beides gemappt) |
| `DELETE` | `/api/notes/:id` | Notiz löschen |
| `GET` | `/api/notes/search?q=…` | Suche für die `[[`-Autovervollständigung |
| `POST` | `/api/notes/:id/duplicate` | Notiz duplizieren (Titel mit `(Kopie)`) |

Beispiel `POST /api/notes`:

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

> **Hinweis:** `content` speichert **JSON** (`{"text": …, "checklist": […]}`), keinen Klartext. Ebenso sind `departments` und `appointments` **JSON-Array-Strings** – nie rohe Strings/Objekte senden, immer `JSON.stringify(...)` aus der JS.

### Departments

| Methode | Pfad | Beschreibung |
|---|---|---|
| `GET` | `/api/departments` | `[{name, count}]`, nach Häufigkeit sortiert, case-insensitiv dedupliziert |

---

## `// DATENMODELL`

Tabelle `notes`:

| Spalte | Typ | Bedeutung |
|---|---|---|
| `id` | INTEGER (PK) | Auto-ID |
| `title` | TEXT | Titel |
| `content` | TEXT | Inhalt als JSON (`text` + `checklist`) |
| `status` | TEXT | `backlog`, `in_progress`, `review`, `done`, `archived` |
| `priority` | TEXT | `low`, `medium`, `high` |
| `date` | TEXT | Erstellungsdatum (TT.MM.JJJJ) |
| `due_date` | TEXT | Optionales Fälligkeitsdatum (YYYY-MM-DD) |
| `department` | TEXT | Legacy: einzelnes Department (Kompatibilität) |
| `departments` | TEXT | JSON-Array der Departments |
| `appointments` | TEXT | JSON-Array der Termine |
| `sort_order` | INTEGER | Sortierung innerhalb einer Spalte |

---

## `// PROJEKTSTRUKTUR`

- `src/main.rs` – die komplette App (Rust-Backend, Datenbank-Setup, eingebettetes HTML/JS-Frontend).
- `notice.db` – SQLite-Datenbank, wird zur Laufzeit erzeugt (in `.gitignore`).
- `screenshots/` – Screenshots der Oberfläche (Board, Kalender, Detail-Modal).
- `AGENTS.md` – zusätzliche Hinweise für Entwicklungs-Assistenten.

---

## `// TECHNISCHE NOTIZEN`

- `sqlx` verbindet via `sqlite://notice.db?mode=rwc`; `?mode=rwc` legt die Datei bei Bedarf an.
- Die SQL-Queries sind Laufzeit-Strings (keine `sqlx::query!`-Makros) – keine compilierten SQL-Checks.
- Die Frontend-Bibliotheken (Vue production, Tailwind, marked) sind **lokal eingebettet** (offline-fähig) statt CDN.
- Die gesamte UI und alle Texte sind auf **Deutsch**.
- Kein Test-/Lint-/CI-Setup; verifiziert per `cargo build` + manuellem Smoke-Test.

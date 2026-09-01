# Notice – Pro Kanban Notes

Ein kompaktes, einzeldateibasiertes Kanban-Notizboard als webbasierte App: **Axum REST-API (Rust)** serviert eine eingebettete **Vue 3 / Tailwind**-Oberfläche und speichert alles in einer **SQLite**-Datenbank. Die gesamte App – Backend, Frontend-HTML und JavaScript – lebt in einer einzigen Datei (`src/main.rs`).

## Features

- **Kanban-Board** mit den Spalten `Backlog`, `In Arbeit`, `Review`, `Abgeschlossen` und einem separaten, standardmäßig eingeklappten **Archiv**.
- **Einzelne Spalten ein-/ausklappbar**: Eingeklappte Status erscheinen als vertikaler Tab-Stapel links, erweiterte Spalten füllen ein Grid in der Mitte.
- **Neue Notiz-Maske** (Öffnen mit `Alt+N`): Titel, Status, Priorität, Fälligkeit, Inhalt (Markdown), **Checkliste/Steps**.
- **Detail-Modal** (Doppelklick auf eine Karte): Titel, Status, Priorität, Fälligkeit, **Department** und Inhalts-/Checklisten-Bearbeitung mit **Vorschau-Modus**.
- **Markdown** für den Inhalt inkl. **unterstützter HTML-Farben** und **verlinkbarer Notizen** über `[[Titel|Anzeige-Name]]` (mit Autocomplete).
- **Departments**: Optional pro Notiz, frei erstellbar, mit **Autocomplete-Vorschlägen** (häufigste zuerst, case-insensitiv dedupliziert) in Maske und Modal.
- **Drag & Drop** zum Verschieben zwischen Spalten sowie zum Umsortieren innerhalb einer Spalte.
- **Prioritäts-Badges** (`Niedrig`/`Mittel`/`Hoch`) und **Fälligkeits-Badges** (`ÜBERFÄLLIG`/`FÄLLIG` …) auf den Karten.
- **Export** als **JSON** oder **CSV** über ein Export-Menü.
- **Volle Durchsuchbarkeit** (Standardmäßig mit `Strg+K` auf die Suchleiste fokussieren), inkl. strukturierter Syntax (siehe unten).

## Schnellstart

### Voraussetzungen

- Rust (stable) inkl. Cargo

### Build & Start

```bash
cargo build
cargo run
# → läuft auf http://127.0.0.1:8080
```

Beim ersten Start wird `notice.db` (SQLite) automatisch in das Projektverzeichnis angelegt, sofern sie noch nicht existiert. Vorhandene Datenbanken werden bei Bedarf per `ALTER TABLE` migriert (z. B. neue Spalten), bestehende Daten bleiben erhalten.

## Bedienung

| Aktion | Eingabe |
|---|---|
| Suchleiste fokussieren | `Strg+K` |
| Neue Notiz öffnen | `Alt+N` |
| Notiz im Detail-Modal öffnen | Doppelklick auf Karte |
| Bearbeiten / Vorschau umschalten | Knopf `✏️ Bearbeiten` / `👁️ Vorschau` |
| Speichern (im Modal) | `Strg+S` |
| Escape | Schließt Modal/Maske bzw. Autocomplete (und entfernt `[[`-Rest bei aktiver Eingabe) |
| Notiz verlinken | `[[` im Inhalt eingeben, dann `↑↓`/`Enter` auswählen |

### Neue-Notiz-Maske

- **Titel**, **Status**, **Priorität**, **Fällig**, **Department** und **Inhalt (Markdown)**.
- Unter „Checklist / Steps" lassen sich einzelne Zwischenschritte hinzufügen.
- Position: „Erstellen & Schließen".

### Departments

- Im Feld **Department** (in Maske und Detail-Modal) werden beim Fokussieren die **häufigsten** vorhandenen Departments vorgeschlagen; beim Tippen wird live gefiltert (wie bei `[[`-Links).
- Eingaben werden **case-insensitiv dedupliziert**: Existiert z. B. "Küche" bereits, wird auch "küche" als "Küche" gespeichert – so entstehen keine Duplikate.
- Auf den Karten erscheint ein Department als Badge (`🏠 …`).

## Suchsyntax

Die Suche durchsucht u.a. Titel, Inhalt und Department. Zusätzlich unterstützt sie `key:value`-Filter:

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

## API

### Notizen

| Methode | Pfad | Beschreibung |
|---|---|---|
| `GET` | `/api/notes` | Alle Notizen (sortiert nach `sort_order`) |
| `POST` | `/api/notes` | Neue Notiz anlegen |
| `POST` / `PUT` | `/api/notes/:id` | Notiz aktualisieren (bewusst beides gemappt) |
| `DELETE` | `/api/notes/:id` | Notiz löschen |
| `GET` | `/api/notes/search?q=…` | Notiz-Suche für die `[[`-Autovervollständigung |
| `POST` | `/api/notes/:id/duplicate` | Notiz duplizieren (Titel mit `(Kopie)`) |

Beispiel `POST /api/notes`:

```json
{
  "title": "Einkauf",
  "content": "{\"text\": \"Milch\", \"checklist\": []}",
  "status": "backlog",
  "priority": "medium",
  "due_date": "2026-09-30",
  "department": "Küche"
}
```

> **Hinweis:** Das Feld `content` speichert **JSON** (`{"text": …, "checklist": […]}`), keinen Klartext. Wer über die API Klartext speichert, bricht die Checklisten-Ansicht beim nächsten Laden.

### Departments

| Methode | Pfad | Beschreibung |
|---|---|---|
| `GET` | `/api/departments` | Liste der vorhandenen Departments als `[{name, count}]`, nach Häufigkeit sortiert und case-insensitiv dedupliziert |

## Datenmodell

Die Tabelle `notes` enthält:

| Spalte | Typ | Bedeutung |
|---|---|---|
| `id` | INTEGER (PK) | Auto-ID |
| `title` | TEXT | Titel |
| `content` | TEXT | Inhalt als JSON (`text` + `checklist`) |
| `status` | TEXT | `backlog`, `in_progress`, `review`, `done`, `archived` |
| `priority` | TEXT | `low`, `medium`, `high` |
| `date` | TEXT | Erstellungsdatum (TT.MM.JJJJ) |
| `due_date` | TEXT | Optionales Fälligkeitsdatum (YYYY-MM-DD) |
| `department` | TEXT | Optionales Department (case-insensitiv dedupliziert) |
| `sort_order` | INTEGER | Sortierung innerhalb einer Spalte |

## Projektstruktur

- `src/main.rs` – die komplette App (Rust-Backend, Datenbank-Setup, eingebettetes HTML/JS-Frontend).
- `notice.db` – SQLite-Datenbank, wird zur Laufzeit erzeugt (in `.gitignore`).
- `AGENTS.md` – zusätzliche Hinweise für Entwicklungs-Assistenten.

## Technische Details & Hinweise

- `sqlx` verbindet via `sqlite://notice.db?mode=rwc`; `?mode=rwc` legt die Datei bei Bedarf an.
- Es gibt keine kompilierten SQL-Checks – die Queries sind Laufzeit-Strings (keine `sqlx::query!`-Makros).
- Die gesamte UI und alle Texte sind auf **Deutsch**.
- Markdown-Rendering über `marked` (CDN); CSS über das Tailwind-CDN.

## Kein Test-/Lint-Setup

Dieses Projekt enthält weder Tests noch Lint- oder CI-Konfiguration. Die smokeartige Verifikation erfolgt manuell über `cargo build` sowie das Starten und Abfragen des Servers (`curl`).
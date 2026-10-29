# DESIGN.md

Gemeinsames Design für Web, TUI und Android. Quelle: `~/workspace/dotfiles` (Gruvbox auf Teal-Schwarz).
Feste Werte (kein Laufzeit-Bezug zu den dotfiles). Keine neuen Grautöne erfinden.

| Token | Hex | Verwendung |
| --- | --- | --- |
| bg | `#191c1c` | Hintergründe |
| surface | `#252a2a` | Kopfleisten, Eingaben, Auswahlzeilen |
| overlay | `#313838` | Rahmen, Trenner |
| fg | `#ebdbb2` | Text |
| subtle | `#a89984` | Sekundärtext |
| muted | `#7c6f64` | Hinweise, Chrome (nicht für Fließtext) |
| accent | `#cc241d` | **der** Akzent: aktiver Reiter, primäre Buttons, Auswahlbalken, Fokus |
| accent_text | `#fb4934` | Akzent als Text |
| on_accent | `#191c1c` | Text auf Akzentflächen |

Status: backlog = blue `#83a598`, in_progress = aqua `#8ec07c`, review = yellow `#fabd2f`, done = green `#b8bb26`.
Fehler/überfällig = red `#fb4934`, Warnung = yellow, dringend = orange `#fe8019`, Kontakte/Adressen = purple `#d3869b`.

## Regeln

- Überall eckig (Radius 0). Schrift: JetBrainsMono (Nerd Font). Ein einziges dunkles Theme.
- Aktiv/fokussiert = Akzent-Block (Reiter, aktuelle Ansicht). Ausgewählte Zeile = `▌` in Akzent auf surface.
- Rahmen einzeilig in overlay. Keine Verläufe, Schatten, Blur, Runde Formen. Kein Rot als Füllung für Status.
- Mobil: Aktionen als Liste (Bottom-Sheet), nicht als Rad.

## Umsetzung

- **Web:** `frontend/theme.css` (CSS-Variablen, globale Regeln) und die Tailwind-Palette in `frontend/index.html`
  (`zinc` = Graustufen aus den Tokens, `emerald`/`red` = Akzent, `green/blue/cyan/amber/violet/orange` = Gruvbox).
  Schrift als WOFF2-Subset in `frontend/fonts/` (`support/make_font.sh`), ausgeliefert über `/fonts/:name`.
- **TUI:** `src/tui/theme.rs` (Truecolor, 256-Farben-Fallback, 16 ANSI-Farben bei `TERM=linux`).
  Kopfzeile wie polybar (aktive Ansicht = Akzent-Block), Panels mit einzeiligem Rahmen in overlay,
  fokussiertes Panel = Rahmen in accent_text, Auswahl = `▌` auf surface, Modus-Block in der Statuszeile.
- **Android:** `Theme.kt` (`object C` + Komponenten `Btn`, `Chip`, `Field`, `Sheet`, `BarRow` …),
  Schrift `res/font/jbm_*.ttf`. Untere Leiste: aktiver Reiter = Akzent-Block.

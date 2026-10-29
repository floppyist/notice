//! Farben und Bausteine im dotfiles-Stil (siehe DESIGN.md).
//! Truecolor, sonst 256 Farben; `TERM=linux`/`dumb` bekommen die 16 ANSI-Farben.

use std::sync::OnceLock;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders},
};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    True,
    Indexed,
    Ansi,
}

fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| {
        let term = std::env::var("TERM").unwrap_or_default();
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        if term == "linux" || term == "dumb" {
            Mode::Ansi
        } else if colorterm.contains("truecolor") || colorterm.contains("24bit") || term.contains("direct") {
            Mode::True
        } else {
            Mode::Indexed
        }
    })
}

/// Farbe wählen: (rgb, 256-Index, ANSI-Farbe)
fn pick(rgb: (u8, u8, u8), idx: u8, ansi: Color) -> Color {
    match mode() {
        Mode::True => Color::Rgb(rgb.0, rgb.1, rgb.2),
        Mode::Indexed => Color::Indexed(idx),
        Mode::Ansi => ansi,
    }
}

pub fn unicode_ok() -> bool {
    mode() != Mode::Ansi
}

// Grundtokens
pub fn bg() -> Color { pick((0x19, 0x1c, 0x1c), 234, Color::Reset) }
pub fn surface() -> Color { pick((0x25, 0x2a, 0x2a), 235, Color::Black) }
pub fn overlay() -> Color { pick((0x31, 0x38, 0x38), 237, Color::DarkGray) }
pub fn fg() -> Color { pick((0xeb, 0xdb, 0xb2), 187, Color::White) }
pub fn subtle() -> Color { pick((0xa8, 0x99, 0x84), 138, Color::Gray) }
pub fn muted() -> Color { pick((0x7c, 0x6f, 0x64), 243, Color::DarkGray) }
pub fn accent() -> Color { pick((0xcc, 0x24, 0x1d), 160, Color::Red) }
pub fn accent_text() -> Color { pick((0xfb, 0x49, 0x34), 203, Color::LightRed) }
pub fn on_accent() -> Color { pick((0x19, 0x1c, 0x1c), 234, Color::Black) }

// Gruvbox bright
pub fn red() -> Color { accent_text() }
pub fn green() -> Color { pick((0xb8, 0xbb, 0x26), 142, Color::LightGreen) }
pub fn yellow() -> Color { pick((0xfa, 0xbd, 0x2f), 214, Color::LightYellow) }
pub fn blue() -> Color { pick((0x83, 0xa5, 0x98), 109, Color::LightBlue) }
pub fn purple() -> Color { pick((0xd3, 0x86, 0x9b), 175, Color::LightMagenta) }
pub fn aqua() -> Color { pick((0x8e, 0xc0, 0x7c), 108, Color::LightCyan) }
pub fn orange() -> Color { pick((0xfe, 0x80, 0x19), 208, Color::Yellow) }

pub fn status_color(status: &str) -> Color {
    match status {
        "backlog" => blue(),
        "in_progress" => aqua(),
        "review" => yellow(),
        "done" => green(),
        _ => muted(),
    }
}

pub fn priority_color(priority: &str) -> Color {
    match priority {
        "high" => red(),
        "medium" => yellow(),
        _ => muted(),
    }
}

// Stile
pub fn base() -> Style { Style::default().fg(fg()).bg(bg()) }
pub fn text() -> Style { Style::default().fg(fg()) }
pub fn dim() -> Style { Style::default().fg(subtle()) }
pub fn faint() -> Style { Style::default().fg(muted()) }
pub fn bold() -> Style { Style::default().fg(fg()).add_modifier(Modifier::BOLD) }
pub fn heading() -> Style { Style::default().fg(accent_text()).add_modifier(Modifier::BOLD) }
/// Block in Akzentfarbe (aktuelle Ansicht, Modus)
pub fn block_accent() -> Style { Style::default().fg(on_accent()).bg(accent()).add_modifier(Modifier::BOLD) }
pub fn block_color(c: Color) -> Style { Style::default().fg(on_accent()).bg(c).add_modifier(Modifier::BOLD) }
pub fn capsule() -> Style { Style::default().fg(fg()).bg(surface()) }
pub fn bar() -> Style { Style::default().fg(subtle()).bg(surface()) }

/// Fläche mit Rahmen in Overlay-Farbe; fokussiert: Rahmen in Akzent-Text.
pub fn panel(title: &str, focused: bool) -> Block<'static> {
    let border = if focused { accent_text() } else { overlay() };
    let title_style = if focused { heading() } else { dim() };
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(Span::styled(format!(" {} ", title), title_style))
        .style(base())
}

/// Wie `panel`, der Titel trägt zusätzlich eine eigene Farbe (Status-Spalten).
pub fn panel_colored(title: String, color: Color, focused: bool) -> Block<'static> {
    let border = if focused { accent_text() } else { overlay() };
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title(Span::styled(format!(" {} ", title), Style::default().fg(color).add_modifier(Modifier::BOLD)))
        .style(base())
}

pub fn bar_glyph() -> &'static str {
    if unicode_ok() { "▌" } else { "|" }
}

/// Zeile mit Auswahlbalken: `▌` in Akzent auf Surface; Fokus auf anderer Spalte: Balken gedämpft.
pub fn row(selected: bool, focused: bool, mut spans: Vec<Span<'static>>) -> Line<'static> {
    if selected {
        let bar_color = if focused { accent() } else { muted() };
        let mut all = vec![Span::styled(bar_glyph(), Style::default().fg(bar_color))];
        all.append(&mut spans);
        Line::from(all).style(Style::default().bg(surface()).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }))
    } else {
        let mut all = vec![Span::raw(" ")];
        all.append(&mut spans);
        Line::from(all)
    }
}

/// `taste beschreibung`-Hinweis: Taste in Akzent-Text, Text gedämpft
pub fn hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!(" {}", key), Style::default().fg(accent_text()).bg(surface())),
        Span::styled(format!(" {}", label), Style::default().fg(subtle()).bg(surface())),
    ]
}

//! Verlinkungen in Texten: `[[Notiz]]`, `<<Wiki-Seite>>`, `{{Kontakt}}`
//! (jeweils mit optionalem `|Anzeigename`) – gleiche Syntax wie im Web-Frontend.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkKind {
    Note,
    Wiki,
    Contact,
}

impl LinkKind {
    pub fn prefix(self) -> char {
        match self {
            LinkKind::Note => 'n',
            LinkKind::Wiki => 'w',
            LinkKind::Contact => 'c',
        }
    }
}

#[derive(Clone, Debug)]
pub struct Link {
    pub kind: LinkKind,
    pub target: String,
    pub display: String,
}

impl Link {
    /// Schlüssel für den Existenz-Index (`App::wiki_links`).
    pub fn key(&self) -> String {
        link_key(self.kind, &self.target)
    }

    pub fn raw(&self) -> String {
        let (open, close) = match self.kind {
            LinkKind::Note => ("[[", "]]"),
            LinkKind::Wiki => ("<<", ">>"),
            LinkKind::Contact => ("{{", "}}"),
        };
        if self.display == self.target {
            format!("{open}{}{close}", self.target)
        } else {
            format!("{open}{}|{}{close}", self.target, self.display)
        }
    }
}

pub fn link_key(kind: LinkKind, target: &str) -> String {
    format!("{}:{}", kind.prefix(), target.trim().to_lowercase())
}

pub enum Seg {
    Text(String),
    Link(Link),
}

const OPENERS: [(&str, &str, LinkKind); 3] = [
    ("[[", "]]", LinkKind::Note),
    ("<<", ">>", LinkKind::Wiki),
    ("{{", "}}", LinkKind::Contact),
];

/// Zerlegt eine Zeile in Text- und Link-Segmente.
pub fn segments(line: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut rest = line;
    loop {
        // Nächsten gültigen Link suchen (frühester Öffner gewinnt).
        let mut best: Option<(usize, usize, Link)> = None; // (start, end, link)
        for (open, close, kind) in OPENERS {
            let mut from = 0;
            while let Some(p) = rest[from..].find(open) {
                let start = from + p;
                let inner_start = start + open.len();
                match rest[inner_start..].find(close) {
                    Some(len) => {
                        let inner = &rest[inner_start..inner_start + len];
                        if !inner.trim().is_empty() {
                            let (target, display) = match inner.split_once('|') {
                                Some((t, d)) if !d.trim().is_empty() => (t.trim(), d.trim()),
                                Some((t, _)) => (t.trim(), t.trim()),
                                None => (inner.trim(), inner.trim()),
                            };
                            if !target.is_empty() {
                                let end = inner_start + len + close.len();
                                let link = Link { kind, target: target.to_string(), display: display.to_string() };
                                if best.as_ref().map_or(true, |(s, _, _)| start < *s) {
                                    best = Some((start, end, link));
                                }
                                break;
                            }
                        }
                        from = start + open.len();
                    }
                    None => break,
                }
            }
        }
        match best {
            Some((start, end, link)) => {
                if start > 0 {
                    out.push(Seg::Text(rest[..start].to_string()));
                }
                out.push(Seg::Link(link));
                rest = &rest[end..];
            }
            None => {
                if !rest.is_empty() || out.is_empty() {
                    out.push(Seg::Text(rest.to_string()));
                }
                return out;
            }
        }
    }
}

/// Alle Links eines mehrzeiligen Textes (ohne Duplikate, in Reihenfolge).
pub fn extract(text: &str) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    for line in text.split('\n') {
        for seg in segments(line) {
            if let Seg::Link(l) = seg {
                if !links.iter().any(|x| x.key() == l.key()) {
                    links.push(l);
                }
            }
        }
    }
    links
}

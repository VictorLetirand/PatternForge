//! Journal d'erreurs local (US-17.5).
//!
//! Les erreurs rencontrées sont gardées **sur la machine** (dans le fichier de réglages, hors
//! projet), exportables à la main ; rien n'est jamais envoyé. Un message est **assaini avant
//! d'être gardé** : aucun chemin (lecteur, partage réseau, chemin absolu, segment avec
//! antislash), aucun texte entre guillemets (noms d'objets, de feuilles, de projet), aucune des
//! valeurs sensibles fournies par l'appelant (nom du projet, nom d'utilisateur, dossier
//! personnel…), longueur bornée. Le journal est borné en nombre d'entrées **et** en taille
//! sérialisée : les plus anciennes sont retirées (rotation).

use serde_json::{json, Value};

/// Nombre maximal d'entrées gardées.
pub const MAX_ENTRIES: usize = 100;
/// Taille maximale du journal sérialisé, en octets.
pub const MAX_BYTES: usize = 16 * 1024;
/// Longueur maximale d'un message assaini, en caractères.
pub const MAX_MESSAGE_CHARS: usize = 200;
/// Longueur maximale du genre d'une erreur.
pub const MAX_KIND_CHARS: usize = 24;

const PATH_MARK: &str = "<chemin>";
const SECRET_MARK: &str = "<…>";
const EXTENSIONS: [&str; 9] = [
    ".pforge.json",
    ".pforge",
    ".json",
    ".pdf",
    ".svg",
    ".dxf",
    ".hpgl",
    ".csv",
    ".autosave",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Secondes Unix (UTC).
    pub time: u64,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorLog {
    entries: Vec<LogEntry>,
}

fn same_char(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

fn replace_ci(text: &[char], needle: &[char], mark: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if i + needle.len() <= text.len()
            && needle
                .iter()
                .enumerate()
                .all(|(j, &n)| same_char(text[i + j], n))
        {
            out.extend(mark.chars());
            i += needle.len();
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

fn blank_quoted(text: &[char]) -> Vec<char> {
    let pairs = [('«', '»'), ('“', '”'), ('"', '"')];
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let Some(&(open, close)) = pairs.iter().find(|(o, _)| *o == text[i]) else {
            out.push(text[i]);
            i += 1;
            continue;
        };
        let end = text[i + 1..].iter().position(|&c| c == close);
        out.push(open);
        out.push('…');
        match end {
            Some(n) => {
                out.push(close);
                i += n + 2;
            }
            // Guillemet jamais refermé : tout le reste est masqué.
            None => i = text.len(),
        }
    }
    out
}

fn ends_with_extension(text: &[char]) -> bool {
    let lower: String = text.iter().collect::<String>().to_lowercase();
    EXTENSIONS.iter().any(|e| lower.ends_with(e))
}

fn path_starts_at(text: &[char], i: usize) -> bool {
    let c = text[i];
    let prev = i.checked_sub(1).map(|p| text[p]);
    let next = text.get(i + 1).copied();
    if c.is_ascii_alphabetic()
        && next == Some(':')
        && prev.is_none_or(|p| !p.is_alphanumeric())
        && matches!(text.get(i + 2), Some('\\') | Some('/'))
    {
        return true;
    }
    if c == '\\' && next == Some('\\') {
        return true;
    }
    if (c == '/' || (c == '~' && next == Some('/')))
        && prev.is_none_or(|p| p.is_whitespace() || matches!(p, '(' | '"' | '«' | '=' | '['))
    {
        let after = if c == '~' {
            text.get(i + 2).copied()
        } else {
            next
        };
        return after.is_some_and(|a| a.is_alphanumeric() || matches!(a, '.' | '_' | '~' | '<'));
    }
    false
}

fn mask_paths(text: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        // Mot contenant un antislash (chemin relatif) : masqué en entier.
        if text[i] == '\\' && !path_starts_at(text, i) {
            while out.last().is_some_and(|c: &char| !c.is_whitespace()) {
                out.pop();
            }
            out.extend(PATH_MARK.chars());
            while i < text.len() && !text[i].is_whitespace() {
                i += 1;
            }
            continue;
        }
        if !path_starts_at(text, i) {
            out.push(text[i]);
            i += 1;
            continue;
        }
        // Un chemin peut contenir des espaces : il s'arrête sur « : », un saut de ligne, une
        // extension connue suivie d'une fin de mot, ou la fin du texte.
        let mut j = i;
        while j < text.len() {
            let c = text[j];
            let colon = c == ' ' && text.get(j + 1) == Some(&':') && text.get(j + 2) == Some(&' ');
            if c == '\n' || colon {
                break;
            }
            j += 1;
            if ends_with_extension(&text[i..j]) && text.get(j).is_none_or(|n| !n.is_alphanumeric())
            {
                break;
            }
        }
        out.extend(PATH_MARK.chars());
        i = j;
    }
    out
}

/// Assainit un message : voir la documentation du module. `secrets` : valeurs à masquer
/// partout (nom du projet, nom d'utilisateur, dossier personnel, chemin du fichier…).
pub fn sanitize(raw: &str, secrets: &[String]) -> String {
    let mut text: Vec<char> = raw
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect();
    let mut needles: Vec<Vec<char>> = secrets
        .iter()
        .map(|s| s.trim().chars().collect::<Vec<_>>())
        .filter(|n| !n.is_empty())
        .collect();
    needles.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for needle in &needles {
        text = replace_ci(&text, needle, SECRET_MARK);
    }
    text = blank_quoted(&text);
    text = mask_paths(&text);
    let flat: String = text.iter().collect::<String>().replace('\n', " ");
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > MAX_MESSAGE_CHARS {
        let mut cut: String = flat.chars().take(MAX_MESSAGE_CHARS - 1).collect();
        cut.push('…');
        cut
    } else {
        flat
    }
}

fn sanitize_kind(kind: &str) -> String {
    kind.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .take(MAX_KIND_CHARS)
        .collect::<String>()
        .to_lowercase()
}

impl ErrorLog {
    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Enregistre une erreur (assainie). Un message vide ou identique au précédent (même
    /// genre) n'ajoute rien. Renvoie vrai si le journal a changé.
    pub fn record(&mut self, time: u64, kind: &str, raw: &str, secrets: &[String]) -> bool {
        let entry = LogEntry {
            time,
            kind: sanitize_kind(kind),
            message: sanitize(raw, secrets),
        };
        if entry.message.is_empty()
            || self
                .entries
                .last()
                .is_some_and(|l| l.kind == entry.kind && l.message == entry.message)
        {
            return false;
        }
        self.entries.push(entry);
        self.rotate();
        true
    }

    fn rotate(&mut self) {
        while self.entries.len() > MAX_ENTRIES
            || (self.entries.len() > 1 && self.to_value().to_string().len() > MAX_BYTES)
        {
            self.entries.remove(0);
        }
    }

    pub fn to_value(&self) -> Value {
        Value::Array(
            self.entries
                .iter()
                .map(|e| json!({"t": e.time, "k": e.kind, "m": e.message}))
                .collect(),
        )
    }

    /// Relit le journal d'un réglage. Tolérant : tout ce qui n'est pas une entrée valide est
    /// ignoré. Les messages sont de nouveau assainis (sans valeur sensible connue) et le
    /// journal borné : un fichier de réglages édité à la main ne contourne pas les bornes.
    pub fn from_value(value: &Value) -> ErrorLog {
        let mut log = ErrorLog::default();
        if let Some(items) = value.as_array() {
            for item in items {
                let (Some(t), Some(k), Some(m)) = (
                    item.get("t").and_then(Value::as_u64),
                    item.get("k").and_then(Value::as_str),
                    item.get("m").and_then(Value::as_str),
                ) else {
                    continue;
                };
                log.entries.push(LogEntry {
                    time: t,
                    kind: sanitize_kind(k),
                    message: sanitize(m, &[]),
                });
            }
        }
        log.entries.retain(|e| !e.message.is_empty());
        log.rotate();
        log
    }

    /// Une ligne par entrée, la plus ancienne d'abord : `2023-11-14T22:13:20Z [kind] message`.
    pub fn export_lines(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|e| format!("{} [{}] {}", format_utc(e.time), e.kind, e.message))
            .collect()
    }
}

/// Date UTC `AAAA-MM-JJThh:mm:ssZ` d'un horodatage Unix (calendrier grégorien).
pub fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

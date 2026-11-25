//! Lecteur SVG (US-12.4) : XML minimal écrit à la main (balises, attributs, commentaires,
//! CDATA, DOCTYPE sans expansion d'entités personnalisées), `g` avec `transform`, `viewBox` et
//! unités, formes de base et `path` (M L H V C S Q T A Z, relatif et absolu).
//!
//! Lu : `path`, `line`, `polyline`, `polygon`, `rect` (coins arrondis compris), `circle`,
//! `ellipse`, `g`, `a`. Ignoré et **listé** : tout autre élément (texte, image, `use`, dégradés,
//! `clipPath`, masques, motifs...) avec son sous-arbre ; les attributs `style`, `clip-path`,
//! `mask` et `filter` sont comptés en remarque. Un XML mal formé (balise non fermée, attribut
//! sans guillemets, imbrication incohérente) est **refusé** : jamais d'import partiel.

use crate::vector_import::*;
use pf_core::MAX_IMPORT_OBJECTS;
use std::f64::consts::{PI, TAU};

/// Plafonds de **lecture** : éléments retenus ou signalés (double des objets importables),
/// sommets et commandes de chemin de tout le fichier (triple), attributs d'une même balise.
pub const MAX_SVG_ELEMENTS: usize = 2 * MAX_IMPORT_OBJECTS;
pub const MAX_SVG_SEGMENTS: usize = 3 * MAX_IMPORT_OBJECTS;
pub const MAX_SVG_ATTRIBUTES: usize = 10_000;

/// Unité de lecture d'un fichier SVG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgUnit {
    Millimetre,
    Centimetre,
    Inch,
    Point,
    Pica,
    /// Pixel CSS : 1/96 de pouce.
    Pixel,
}

impl SvgUnit {
    pub const CHOICES: [SvgUnit; 5] = [
        SvgUnit::Millimetre,
        SvgUnit::Centimetre,
        SvgUnit::Inch,
        SvgUnit::Point,
        SvgUnit::Pixel,
    ];

    pub fn mm_per_unit(self) -> f64 {
        match self {
            SvgUnit::Millimetre => 1.0,
            SvgUnit::Centimetre => 10.0,
            SvgUnit::Inch => 25.4,
            SvgUnit::Point => 25.4 / 72.0,
            SvgUnit::Pica => 25.4 / 6.0,
            SvgUnit::Pixel => 25.4 / 96.0,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SvgUnit::Millimetre => "mm",
            SvgUnit::Centimetre => "cm",
            SvgUnit::Inch => "in",
            SvgUnit::Point => "pt",
            SvgUnit::Pica => "pc",
            SvgUnit::Pixel => "px",
        }
    }
}

/// Choix de l'utilisatrice pour l'unité des coordonnées.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvgUnitChoice {
    /// Celle que déclarent `width` / `height` / `viewBox`, sinon le pixel (96 dpi).
    Declared,
    Fixed(SvgUnit),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SvgDocument {
    shapes: Vec<ShapeEntry>,
    stats: ReadStats,
    view_box: Option<[f64; 4]>,
    /// Millimètres par unité utilisateur déduits de `width` / `height` et `viewBox`.
    declared_mm: Option<f64>,
}

// ----- XML -------------------------------------------------------------------------------------

struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    close: bool,
    self_close: bool,
}

struct Xml<'a> {
    s: &'a [u8],
    i: usize,
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0c)
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn decode_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

impl<'a> Xml<'a> {
    fn find(&self, pat: &[u8], from: usize) -> Option<usize> {
        self.s[from.min(self.s.len())..]
            .windows(pat.len())
            .position(|w| w == pat)
            .map(|p| p + from)
    }

    fn err(&self) -> VectorImportError {
        VectorImportError::Xml { offset: self.i }
    }

    /// Prochaine balise ; `None` en fin de fichier. Le texte, les commentaires, CDATA,
    /// instructions et DOCTYPE sont sautés.
    fn next_tag(&mut self) -> Result<Option<Tag>, VectorImportError> {
        loop {
            while self.i < self.s.len() && self.s[self.i] != b'<' {
                self.i += 1;
            }
            if self.i >= self.s.len() {
                return Ok(None);
            }
            let rest = &self.s[self.i..];
            if rest.starts_with(b"<!--") {
                let e = self.find(b"-->", self.i + 4).ok_or_else(|| self.err())?;
                self.i = e + 3;
            } else if rest.starts_with(b"<![CDATA[") {
                let e = self.find(b"]]>", self.i + 9).ok_or_else(|| self.err())?;
                self.i = e + 3;
            } else if rest.starts_with(b"<?") {
                let e = self.find(b"?>", self.i + 2).ok_or_else(|| self.err())?;
                self.i = e + 2;
            } else if rest.starts_with(b"<!") {
                // DOCTYPE : saute jusqu'au `>` hors crochets ; aucune entité n'est étendue.
                let mut depth = 0i32;
                let mut j = self.i + 2;
                loop {
                    let Some(&b) = self.s.get(j) else {
                        return Err(self.err());
                    };
                    match b {
                        b'[' => depth += 1,
                        b']' => depth -= 1,
                        b'>' if depth <= 0 => break,
                        _ => {}
                    }
                    j += 1;
                }
                self.i = j + 1;
            } else {
                return self.tag().map(Some);
            }
        }
    }

    fn name(&mut self) -> Result<String, VectorImportError> {
        let st = self.i;
        while self.i < self.s.len()
            && !is_ws(self.s[self.i])
            && !matches!(self.s[self.i], b'>' | b'/' | b'=' | b'<' | b'"' | b'\'')
        {
            self.i += 1;
        }
        if self.i == st {
            return Err(self.err());
        }
        Ok(String::from_utf8_lossy(&self.s[st..self.i]).into_owned())
    }

    fn skip_ws(&mut self) {
        while self.i < self.s.len() && is_ws(self.s[self.i]) {
            self.i += 1;
        }
    }

    fn tag(&mut self) -> Result<Tag, VectorImportError> {
        self.i += 1; // '<'
        let close = self.s.get(self.i) == Some(&b'/');
        if close {
            self.i += 1;
        }
        let name = self.name()?;
        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            match self.s.get(self.i) {
                None => return Err(self.err()),
                Some(b'>') => {
                    self.i += 1;
                    return Ok(Tag {
                        name,
                        attrs,
                        close,
                        self_close: false,
                    });
                }
                Some(b'/') => {
                    if close || self.s.get(self.i + 1) != Some(&b'>') {
                        return Err(self.err());
                    }
                    self.i += 2;
                    return Ok(Tag {
                        name,
                        attrs,
                        close: false,
                        self_close: true,
                    });
                }
                Some(_) if close => return Err(self.err()),
                Some(_) => {
                    let key = self.name()?;
                    self.skip_ws();
                    if self.s.get(self.i) != Some(&b'=') {
                        return Err(self.err());
                    }
                    self.i += 1;
                    self.skip_ws();
                    let q = match self.s.get(self.i) {
                        Some(&q @ (b'"' | b'\'')) => q,
                        _ => return Err(self.err()),
                    };
                    self.i += 1;
                    let st = self.i;
                    while self.i < self.s.len() && self.s[self.i] != q {
                        if self.s[self.i] == b'<' {
                            return Err(self.err());
                        }
                        self.i += 1;
                    }
                    if self.i >= self.s.len() {
                        return Err(self.err());
                    }
                    let val = decode_entities(&String::from_utf8_lossy(&self.s[st..self.i]));
                    self.i += 1;
                    attrs.push((key, val));
                    if attrs.len() > MAX_SVG_ATTRIBUTES {
                        return Err(VectorImportError::TooManyElements {
                            max: MAX_SVG_ATTRIBUTES,
                        });
                    }
                }
            }
        }
    }
}

fn attr<'a>(t: &'a Tag, key: &str) -> Option<&'a str> {
    t.attrs
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

// ----- nombres, longueurs, transformations -----------------------------------------------------

struct Scan<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Scan<'a> {
    fn new(s: &'a str) -> Scan<'a> {
        Scan {
            s: s.as_bytes(),
            i: 0,
        }
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && is_ws(self.s[self.i]) {
            self.i += 1;
        }
    }

    fn eof(&mut self) -> bool {
        self.ws();
        self.i >= self.s.len()
    }

    fn comma(&mut self) {
        self.ws();
        if self.s.get(self.i) == Some(&b',') {
            self.i += 1;
        }
    }

    fn at_number(&mut self) -> bool {
        self.ws();
        matches!(self.s.get(self.i), Some(b'0'..=b'9' | b'.' | b'+' | b'-'))
    }

    /// Nombre SVG (`-1.5e2`, `.5`, `1.`) ; refuse tout ce qui n'est pas fini.
    fn number(&mut self) -> Option<f64> {
        self.ws();
        let st = self.i;
        let mut j = self.i;
        if matches!(self.s.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let d0 = j;
        while matches!(self.s.get(j), Some(b'0'..=b'9')) {
            j += 1;
        }
        let mut digits = j - d0;
        if self.s.get(j) == Some(&b'.') {
            j += 1;
            let f0 = j;
            while matches!(self.s.get(j), Some(b'0'..=b'9')) {
                j += 1;
            }
            digits += j - f0;
        }
        if digits == 0 {
            return None;
        }
        if matches!(self.s.get(j), Some(b'e' | b'E')) {
            let mut k = j + 1;
            if matches!(self.s.get(k), Some(b'+' | b'-')) {
                k += 1;
            }
            let e0 = k;
            while matches!(self.s.get(k), Some(b'0'..=b'9')) {
                k += 1;
            }
            if k > e0 {
                j = k;
            }
        }
        let v: f64 = std::str::from_utf8(&self.s[st..j]).ok()?.parse().ok()?;
        self.i = j;
        v.is_finite().then_some(v)
    }

    /// Indicateur d'arc : un seul caractère `0` ou `1`.
    fn flag(&mut self) -> Option<bool> {
        self.ws();
        let v = match self.s.get(self.i) {
            Some(b'0') => false,
            Some(b'1') => true,
            _ => return None,
        };
        self.i += 1;
        Some(v)
    }
}

/// Longueur avec unité -> unités utilisateur (px) ; `%`, `em`, `ex` ne sont pas lus.
fn parse_length(s: &str) -> Option<f64> {
    let t = s.trim();
    let mut sc = Scan::new(t);
    let v = sc.number()?;
    let unit = t[sc.i..].trim();
    let mm = match unit {
        "" | "px" => return Some(v),
        "mm" => SvgUnit::Millimetre,
        "cm" => SvgUnit::Centimetre,
        "in" => SvgUnit::Inch,
        "pt" => SvgUnit::Point,
        "pc" => SvgUnit::Pica,
        _ => return None,
    };
    Some(v * mm.mm_per_unit() / SvgUnit::Pixel.mm_per_unit())
}

fn parse_transform(s: &str) -> Option<Matrix> {
    let mut sc = Scan::new(s);
    let mut total = Matrix::IDENTITY;
    loop {
        sc.ws();
        sc.comma();
        sc.ws();
        if sc.i >= sc.s.len() {
            return Some(total);
        }
        let st = sc.i;
        while sc.i < sc.s.len() && sc.s[sc.i].is_ascii_alphabetic() {
            sc.i += 1;
        }
        let name = std::str::from_utf8(&sc.s[st..sc.i]).ok()?.to_string();
        sc.ws();
        if sc.s.get(sc.i) != Some(&b'(') {
            return None;
        }
        sc.i += 1;
        let mut args = Vec::new();
        loop {
            sc.ws();
            if sc.s.get(sc.i) == Some(&b')') {
                sc.i += 1;
                break;
            }
            args.push(sc.number()?);
            sc.comma();
            if args.len() > 6 {
                return None;
            }
        }
        let m = match (name.as_str(), args.as_slice()) {
            ("translate", [x]) => Matrix::new(1.0, 0.0, 0.0, 1.0, *x, 0.0),
            ("translate", [x, y]) => Matrix::new(1.0, 0.0, 0.0, 1.0, *x, *y),
            ("scale", [x]) => Matrix::new(*x, 0.0, 0.0, *x, 0.0, 0.0),
            ("scale", [x, y]) => Matrix::new(*x, 0.0, 0.0, *y, 0.0, 0.0),
            ("rotate", [a]) => rotation(*a, 0.0, 0.0),
            ("rotate", [a, cx, cy]) => rotation(*a, *cx, *cy),
            ("matrix", [a, b, c, d, e, f]) => Matrix::new(*a, *b, *c, *d, *e, *f),
            ("skewX", [a]) => Matrix::new(1.0, 0.0, (a * PI / 180.0).tan(), 1.0, 0.0, 0.0),
            ("skewY", [a]) => Matrix::new(1.0, (a * PI / 180.0).tan(), 0.0, 1.0, 0.0, 0.0),
            _ => return None,
        };
        if !m.is_finite() {
            return None;
        }
        total = total.compose(m);
        if !total.is_finite() {
            return None;
        }
    }
}

fn rotation(deg: f64, cx: f64, cy: f64) -> Matrix {
    let (s, c) = (deg * PI / 180.0).sin_cos();
    Matrix::new(c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy)
}

// ----- données de chemin -----------------------------------------------------------------------

fn angle(u: P2, v: P2) -> f64 {
    let (nu, nv) = (u.0.hypot(u.1), v.0.hypot(v.1));
    let cos = ((u.0 * v.0 + u.1 * v.1) / (nu * nv)).clamp(-1.0, 1.0);
    let a = cos.acos();
    if u.0 * v.1 - u.1 * v.0 < 0.0 {
        -a
    } else {
        a
    }
}

/// Arc SVG (extrémités, rayons, rotation, indicateurs) -> segment ; `None` = arc nul (omis).
#[allow(clippy::too_many_arguments)]
fn arc_from_endpoints(
    p0: P2,
    p1: P2,
    rx: f64,
    ry: f64,
    rot_deg: f64,
    large: bool,
    sweep: bool,
    snapped: &mut bool,
) -> Option<Seg> {
    if p0 == p1 {
        return None;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        return Some(Seg::L(p1));
    }
    let half = (p1.0 - p0.0).hypot(p1.1 - p0.1) / 2.0;
    if (rx - ry).abs() <= 1e-9 * rx.max(ry) && (half - rx).abs() <= SEMICIRCLE_SNAP && half != rx {
        rx = half;
        ry = half;
        *snapped = true;
    }
    let phi = rot_deg * PI / 180.0;
    let (sp, cp) = phi.sin_cos();
    let (dx, dy) = ((p0.0 - p1.0) / 2.0, (p0.1 - p1.1) / 2.0);
    let (x1, y1) = (cp * dx + sp * dy, -sp * dx + cp * dy);
    let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lambda > 1.0 {
        let k = lambda.sqrt();
        rx *= k;
        ry *= k;
    }
    let (rx2, ry2) = (rx * rx, ry * ry);
    let den = rx2 * y1 * y1 + ry2 * x1 * x1;
    let num = (rx2 * ry2 - den).max(0.0);
    let mut coef = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
    if large == sweep {
        coef = -coef;
    }
    let (cxp, cyp) = (coef * rx * y1 / ry, -coef * ry * x1 / rx);
    let c = (
        cp * cxp - sp * cyp + (p0.0 + p1.0) / 2.0,
        sp * cxp + cp * cyp + (p0.1 + p1.1) / 2.0,
    );
    let u = ((x1 - cxp) / rx, (y1 - cyp) / ry);
    let v = ((-x1 - cxp) / rx, (-y1 - cyp) / ry);
    let t1 = angle((1.0, 0.0), u);
    let mut dt = angle(u, v);
    if !sweep && dt > 0.0 {
        dt -= TAU;
    } else if sweep && dt < 0.0 {
        dt += TAU;
    }
    if !(t1.is_finite() && dt.is_finite() && c.0.is_finite() && c.1.is_finite()) {
        return Some(Seg::L(p1));
    }
    Some(Seg::A(ArcSeg {
        c,
        rx,
        ry,
        phi,
        t1,
        dt,
        start: p0,
        end: p1,
    }))
}

struct PathOut {
    segs: Vec<Seg>,
    raised: bool,
    snapped: bool,
}

/// `budget` : la lecture s'arrête dès qu'il est dépassé (l'appelant refuse alors le fichier).
fn parse_path(d: &str, budget: usize) -> Option<PathOut> {
    let mut sc = Scan::new(d);
    let mut out = PathOut {
        segs: Vec::new(),
        raised: false,
        snapped: false,
    };
    let (mut cur, mut start) = ((0.0, 0.0), (0.0, 0.0));
    let (mut last_c2, mut last_q): (Option<P2>, Option<P2>) = (None, None);
    let mut prev = b' ';
    let mut cmd = b' ';
    let mut need_move = false;
    if sc.eof() {
        return None;
    }
    while !sc.eof() && out.segs.len() <= budget {
        let b = sc.s[sc.i];
        if b.is_ascii_alphabetic() {
            if !b"MmLlHhVvCcSsQqTtAaZz".contains(&b) {
                return None;
            }
            cmd = b;
            sc.i += 1;
        } else if sc.at_number() && cmd != b' ' && !matches!(cmd, b'Z' | b'z') {
            cmd = match cmd {
                b'M' => b'L',
                b'm' => b'l',
                other => other,
            };
        } else {
            return None;
        }
        if out.segs.is_empty() && !matches!(cmd, b'M' | b'm') {
            return None;
        }
        let rel = cmd.is_ascii_lowercase();
        let up = cmd.to_ascii_uppercase();
        let (ox, oy) = if rel { cur } else { (0.0, 0.0) };
        if need_move && up != b'M' && up != b'Z' {
            out.segs.push(Seg::M(cur));
            need_move = false;
        }
        match up {
            b'Z' => {
                if sc.at_number() {
                    return None;
                }
                out.segs.push(Seg::Z);
                cur = start;
                need_move = true;
                last_c2 = None;
                last_q = None;
                prev = up;
                continue;
            }
            b'M' => {
                let p = (ox + sc.number()?, {
                    sc.comma();
                    oy + sc.number()?
                });
                out.segs.push(Seg::M(p));
                cur = p;
                start = p;
                need_move = false;
                last_c2 = None;
                last_q = None;
            }
            b'L' => {
                let p = (ox + sc.number()?, {
                    sc.comma();
                    oy + sc.number()?
                });
                out.segs.push(Seg::L(p));
                cur = p;
                last_c2 = None;
                last_q = None;
            }
            b'H' => {
                let p = (ox + sc.number()?, cur.1);
                out.segs.push(Seg::L(p));
                cur = p;
                last_c2 = None;
                last_q = None;
            }
            b'V' => {
                let p = (cur.0, oy + sc.number()?);
                out.segs.push(Seg::L(p));
                cur = p;
                last_c2 = None;
                last_q = None;
            }
            b'C' | b'S' => {
                let c1 = if up == b'C' {
                    let p = (ox + sc.number()?, {
                        sc.comma();
                        oy + sc.number()?
                    });
                    sc.comma();
                    p
                } else if matches!(prev, b'C' | b'S') {
                    let l = last_c2.unwrap_or(cur);
                    (2.0 * cur.0 - l.0, 2.0 * cur.1 - l.1)
                } else {
                    cur
                };
                let c2 = (ox + sc.number()?, {
                    sc.comma();
                    oy + sc.number()?
                });
                sc.comma();
                let p = (ox + sc.number()?, {
                    sc.comma();
                    oy + sc.number()?
                });
                out.segs.push(Seg::C(c1, c2, p));
                cur = p;
                last_c2 = Some(c2);
                last_q = None;
            }
            b'Q' | b'T' => {
                let q = if up == b'Q' {
                    let p = (ox + sc.number()?, {
                        sc.comma();
                        oy + sc.number()?
                    });
                    sc.comma();
                    p
                } else if matches!(prev, b'Q' | b'T') {
                    let l = last_q.unwrap_or(cur);
                    (2.0 * cur.0 - l.0, 2.0 * cur.1 - l.1)
                } else {
                    cur
                };
                let p = (ox + sc.number()?, {
                    sc.comma();
                    oy + sc.number()?
                });
                let c1 = (
                    cur.0 + 2.0 / 3.0 * (q.0 - cur.0),
                    cur.1 + 2.0 / 3.0 * (q.1 - cur.1),
                );
                let c2 = (p.0 + 2.0 / 3.0 * (q.0 - p.0), p.1 + 2.0 / 3.0 * (q.1 - p.1));
                out.segs.push(Seg::C(c1, c2, p));
                out.raised = true;
                cur = p;
                last_q = Some(q);
                last_c2 = None;
            }
            b'A' => {
                let rx = sc.number()?;
                sc.comma();
                let ry = sc.number()?;
                sc.comma();
                let rot = sc.number()?;
                sc.comma();
                let large = sc.flag()?;
                sc.comma();
                let sweep = sc.flag()?;
                sc.comma();
                let p = (ox + sc.number()?, {
                    sc.comma();
                    oy + sc.number()?
                });
                if let Some(seg) =
                    arc_from_endpoints(cur, p, rx, ry, rot, large, sweep, &mut out.snapped)
                {
                    out.segs.push(seg);
                }
                cur = p;
                last_c2 = None;
                last_q = None;
            }
            _ => return None,
        }
        if !(cur.0.is_finite() && cur.1.is_finite()) {
            return None;
        }
        sc.comma();
        prev = up;
    }
    Some(out)
}

// ----- formes de base --------------------------------------------------------------------------

fn num_attr(t: &Tag, key: &str, default: Option<f64>) -> Result<f64, IgnoreReason> {
    match attr(t, key) {
        None => default.ok_or(IgnoreReason::Invalid),
        Some(v) => parse_length(v).ok_or(IgnoreReason::Invalid),
    }
}

fn points_attr(t: &Tag, budget: usize) -> Result<Vec<P2>, IgnoreReason> {
    let mut sc = Scan::new(attr(t, "points").ok_or(IgnoreReason::Invalid)?);
    let mut v = Vec::new();
    // Au moins deux points lus : une liste coupée ne doit pas passer pour dégénérée.
    while !sc.eof() && v.len() <= budget.max(1) {
        let x = sc.number().ok_or(IgnoreReason::Invalid)?;
        sc.comma();
        let y = sc.number().ok_or(IgnoreReason::Invalid)?;
        sc.comma();
        v.push((x, y));
    }
    Ok(v)
}

fn rect_segs(t: &Tag) -> Result<Vec<Seg>, IgnoreReason> {
    let (x, y) = (num_attr(t, "x", Some(0.0))?, num_attr(t, "y", Some(0.0))?);
    let (w, h) = (num_attr(t, "width", None)?, num_attr(t, "height", None)?);
    if w <= 0.0 || h <= 0.0 {
        return Err(IgnoreReason::Degenerate);
    }
    let rx = attr(t, "rx").map(|_| num_attr(t, "rx", None)).transpose()?;
    let ry = attr(t, "ry").map(|_| num_attr(t, "ry", None)).transpose()?;
    if rx.is_some_and(|v| v < 0.0) || ry.is_some_and(|v| v < 0.0) {
        return Err(IgnoreReason::Invalid);
    }
    let (rx, ry) = match (rx, ry) {
        (None, None) => (0.0, 0.0),
        (Some(a), None) | (None, Some(a)) => (a, a),
        (Some(a), Some(b)) => (a, b),
    };
    let (rx, ry) = (rx.min(w / 2.0), ry.min(h / 2.0));
    if rx == 0.0 || ry == 0.0 {
        return Ok(vec![
            Seg::M((x, y)),
            Seg::L((x + w, y)),
            Seg::L((x + w, y + h)),
            Seg::L((x, y + h)),
            Seg::Z,
        ]);
    }
    let q = |c: P2, t1: f64| Seg::A(ArcSeg::from_params(c, rx, ry, 0.0, t1, PI / 2.0));
    let mut segs = vec![Seg::M((x + rx, y)), Seg::L((x + w - rx, y))];
    segs.push(q((x + w - rx, y + ry), -PI / 2.0));
    segs.push(Seg::L((x + w, y + h - ry)));
    segs.push(q((x + w - rx, y + h - ry), 0.0));
    segs.push(Seg::L((x + rx, y + h)));
    segs.push(q((x + rx, y + h - ry), PI / 2.0));
    segs.push(Seg::L((x, y + ry)));
    segs.push(q((x + rx, y + ry), PI));
    segs.push(Seg::Z);
    Ok(segs)
}

fn shape_of(t: &Tag, budget: usize) -> (Result<ShapeGeom, IgnoreReason>, bool, bool) {
    let plain = |r: Result<ShapeGeom, IgnoreReason>| (r, false, false);
    match local_name(&t.name) {
        "path" => match attr(t, "d").and_then(|d| parse_path(d, budget)) {
            Some(p) => (Ok(ShapeGeom::Path(p.segs)), p.raised, p.snapped),
            None => plain(Err(IgnoreReason::Invalid)),
        },
        "line" => plain((|| {
            let p = |k| num_attr(t, k, Some(0.0));
            Ok(ShapeGeom::Path(vec![
                Seg::M((p("x1")?, p("y1")?)),
                Seg::L((p("x2")?, p("y2")?)),
            ]))
        })()),
        name @ ("polyline" | "polygon") => plain((|| {
            let pts = points_attr(t, budget)?;
            if pts.len() < 2 {
                return Err(IgnoreReason::Degenerate);
            }
            let mut segs = vec![Seg::M(pts[0])];
            segs.extend(pts[1..].iter().map(|p| Seg::L(*p)));
            if name == "polygon" {
                segs.push(Seg::Z);
            }
            Ok(ShapeGeom::Path(segs))
        })()),
        "rect" => plain(rect_segs(t).map(ShapeGeom::Path)),
        "circle" => plain((|| {
            let r = num_attr(t, "r", None)?;
            if r <= 0.0 {
                return Err(IgnoreReason::Degenerate);
            }
            Ok(ShapeGeom::Ellipse {
                c: (num_attr(t, "cx", Some(0.0))?, num_attr(t, "cy", Some(0.0))?),
                rx: r,
                ry: r,
            })
        })()),
        "ellipse" => plain((|| {
            let rx = attr(t, "rx").map(|_| num_attr(t, "rx", None)).transpose()?;
            let ry = attr(t, "ry").map(|_| num_attr(t, "ry", None)).transpose()?;
            let (rx, ry) = match (rx, ry) {
                (Some(a), Some(b)) => (a, b),
                (Some(a), None) | (None, Some(a)) => (a, a),
                (None, None) => return Err(IgnoreReason::Invalid),
            };
            if rx <= 0.0 || ry <= 0.0 {
                return Err(IgnoreReason::Degenerate);
            }
            Ok(ShapeGeom::Ellipse {
                c: (num_attr(t, "cx", Some(0.0))?, num_attr(t, "cy", Some(0.0))?),
                rx,
                ry,
            })
        })()),
        _ => plain(Err(IgnoreReason::Unsupported)),
    }
}

// ----- document --------------------------------------------------------------------------------

struct Frame {
    name: String,
    ctm: Option<Matrix>,
    skip: bool,
}

const MAX_DEPTH: usize = 512;

impl SvgDocument {
    pub fn parse(bytes: &[u8]) -> Result<SvgDocument, VectorImportError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(VectorImportError::TooLarge { bytes: bytes.len() });
        }
        let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let mut xml = Xml { s: body, i: 0 };
        let mut stack: Vec<Frame> = Vec::new();
        let mut doc = SvgDocument {
            shapes: Vec::new(),
            stats: ReadStats::default(),
            view_box: None,
            declared_mm: None,
        };
        let mut seen_root = false;
        let mut closed_root = false;
        let (mut elements, mut segments) = (0usize, 0usize);
        let too_many = |max| Err(VectorImportError::TooManyElements { max });
        while let Some(t) = xml.next_tag()? {
            if elements > MAX_SVG_ELEMENTS {
                return too_many(MAX_SVG_ELEMENTS);
            }
            let name = local_name(&t.name).to_string();
            if t.close {
                match stack.pop() {
                    Some(f) if f.name == t.name => {}
                    _ => return Err(xml.err()),
                }
                if stack.is_empty() {
                    closed_root = true;
                }
                continue;
            }
            if closed_root {
                return Err(xml.err());
            }
            if !seen_root {
                if name != "svg" {
                    return Err(VectorImportError::NotSvg);
                }
                seen_root = true;
                doc.read_root(&t);
                let ctm = Some(Matrix::IDENTITY);
                if t.self_close {
                    closed_root = true;
                } else {
                    stack.push(Frame {
                        name: t.name.clone(),
                        ctm,
                        skip: false,
                    });
                }
                continue;
            }
            if stack.len() >= MAX_DEPTH {
                return Err(xml.err());
            }
            let parent_skip = stack.last().is_some_and(|f| f.skip);
            let parent_ctm = stack.last().and_then(|f| f.ctm);
            let mut skip = parent_skip;
            let mut ctm = parent_ctm;
            if !parent_skip {
                match name.as_str() {
                    "g" | "a" => {
                        doc.element_attrs(&t);
                        ctm = Self::compose(parent_ctm, &t);
                    }
                    "path" | "line" | "polyline" | "polygon" | "rect" | "circle" | "ellipse" => {
                        doc.element_attrs(&t);
                        let local = Self::compose(parent_ctm, &t);
                        let (geom, raised, snapped) = shape_of(&t, MAX_SVG_SEGMENTS - segments);
                        if let Ok(ShapeGeom::Path(s)) = &geom {
                            segments += s.len();
                            if segments > MAX_SVG_SEGMENTS {
                                return too_many(MAX_SVG_SEGMENTS);
                            }
                        }
                        elements += 1;
                        doc.shapes.push(ShapeEntry {
                            tag: name.clone(),
                            geom: match (local, geom) {
                                (None, _) => Err(IgnoreReason::Invalid),
                                (_, g) => g,
                            },
                            ctm: local.unwrap_or(Matrix::IDENTITY),
                            raised,
                            snapped,
                        });
                        skip = true;
                    }
                    "title" | "desc" | "metadata" => skip = true,
                    other => {
                        doc.stats.ignore(other, IgnoreReason::Unsupported);
                        elements += 1;
                        skip = true;
                    }
                }
            }
            if !t.self_close {
                stack.push(Frame {
                    name: t.name.clone(),
                    ctm,
                    skip,
                });
            }
        }
        if elements > MAX_SVG_ELEMENTS {
            return too_many(MAX_SVG_ELEMENTS);
        }
        if !seen_root {
            return Err(VectorImportError::NotSvg);
        }
        if !stack.is_empty() {
            return Err(xml.err());
        }
        Ok(doc)
    }

    fn compose(parent: Option<Matrix>, t: &Tag) -> Option<Matrix> {
        let parent = parent?;
        match attr(t, "transform") {
            None => Some(parent),
            Some(s) => parse_transform(s).map(|m| parent.compose(m)),
        }
    }

    fn element_attrs(&mut self, t: &Tag) {
        if attr(t, "style").is_some() {
            self.stats.note(NoteKind::StyleIgnored);
        }
        if ["clip-path", "mask", "filter"]
            .iter()
            .any(|k| attr(t, k).is_some())
        {
            self.stats.note(NoteKind::ClipIgnored);
        }
    }

    fn read_root(&mut self, t: &Tag) {
        let vb = attr(t, "viewBox").and_then(|s| {
            let mut sc = Scan::new(s);
            let mut v = [0.0; 4];
            for slot in &mut v {
                *slot = sc.number()?;
                sc.comma();
            }
            (sc.eof() && v[2] > 0.0 && v[3] > 0.0).then_some(v)
        });
        self.view_box = vb;
        let px_mm = SvgUnit::Pixel.mm_per_unit();
        let dim = |k: &str| {
            attr(t, k)
                .and_then(parse_length)
                .filter(|v| *v > 0.0)
                .map(|v| v * px_mm)
        };
        let (w, h) = (dim("width"), dim("height"));
        self.declared_mm = vb.and_then(|v| match (w, h) {
            (Some(w), Some(h)) => Some((w / v[2]).min(h / v[3])),
            (Some(w), None) => Some(w / v[2]),
            (None, Some(h)) => Some(h / v[3]),
            (None, None) => None,
        });
    }

    /// Millimètres par unité utilisateur déclarés par le fichier (`width` / `height` avec
    /// `viewBox`), sinon `None` (le fichier ne dit pas : pixel à 96 dpi).
    pub fn declared_mm_per_unit(&self) -> Option<f64> {
        self.declared_mm
    }

    pub fn view_box(&self) -> Option<[f64; 4]> {
        self.view_box
    }

    /// Éléments géométriques lus (importables ou non).
    pub fn element_count(&self) -> usize {
        self.shapes.len() + self.stats.ignored.values().sum::<usize>()
    }

    pub fn build_choice(
        &self,
        choice: SvgUnitChoice,
        scale: f64,
    ) -> Result<VectorBuild, VectorImportError> {
        let unit = match choice {
            SvgUnitChoice::Declared => self
                .declared_mm
                .unwrap_or_else(|| SvgUnit::Pixel.mm_per_unit()),
            SvgUnitChoice::Fixed(u) => u.mm_per_unit(),
        };
        self.build(unit, scale)
    }

    /// `mm_per_unit` : millimètres par unité utilisateur ; `scale` : facteur supplémentaire.
    pub fn build(&self, mm_per_unit: f64, scale: f64) -> Result<VectorBuild, VectorImportError> {
        check_scale(scale)?;
        let k = mm_per_unit * scale;
        if !(k.is_finite() && k > 0.0) {
            return Err(VectorImportError::BadScale);
        }
        // L'axe Y du SVG descend, celui du modèle monte : l'origine est le coin bas-gauche du
        // `viewBox`, la figure garde son sens.
        let base = match self.view_box {
            Some([x, y, _, h]) => Matrix::new(k, 0.0, 0.0, -k, -k * x, k * (y + h)),
            None => Matrix::new(k, 0.0, 0.0, -k, 0.0, 0.0),
        };
        build_shapes(
            &self.shapes,
            self.stats.clone(),
            base,
            k,
            DEFAULT_TOLERANCE_MM,
        )
    }
}

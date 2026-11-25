//! Lecteurs et fixtures **propres à PH-5c** (indépendants de `tests/common/`) : texte placé des
//! quatre exports (SVG, PDF, DXF, HPGL) et des rapports (CSV, PDF). Ils ne partagent aucun code avec
//! l'écrivain de `pf-export` ni avec les lecteurs de `common/` : seules les grammaires des formats
//! sont connues. Positions en mm, repère du fichier lu.
#![allow(dead_code)]

use pf_core::{Command, EntityId, PathRole, PieceId, PieceSource, PointRef, Project, SheetId};
use pf_geometry::PointMm;
use std::collections::HashMap;

pub fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ---------------------------------------------------------------------------------------------
// Fixture : rectangle extrait en pièce
// ---------------------------------------------------------------------------------------------

/// Les quatre bords d'un rectangle (identifiants de tracés).
#[derive(Debug, Clone, Copy)]
pub struct Edges {
    pub bottom: EntityId,
    pub right: EntityId,
    pub top: EntityId,
    pub left: EntityId,
}

/// Rectangle `w × h` dont le coin bas-gauche est `(x ; y)`, tracés de rôle `role`, extrait en pièce.
pub fn rect(
    p: &mut Project,
    s: SheetId,
    (x, y): (f64, f64),
    (w, h): (f64, f64),
    name: &str,
    role: PathRole,
) -> (PieceId, Edges) {
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(x, y)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    for &e in &r.entities {
        p.apply(Command::SetPathRole {
            sheet: s,
            entity: e,
            role,
        })
        .unwrap();
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: r.entities.clone(),
            },
            name: name.into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let sheet = p.sheet(s).unwrap();
    let pick = |f: &dyn Fn((f64, f64), (f64, f64)) -> bool| -> EntityId {
        *r.entities
            .iter()
            .find(|e| match sheet.shape(**e) {
                Ok(pf_core::Shape::Line(a, b)) => f((a.x, a.y), (b.x, b.y)),
                _ => false,
            })
            .expect("bord du rectangle")
    };
    let eq = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let edges = Edges {
        bottom: pick(&|a, b| eq(a.1, y) && eq(b.1, y)),
        top: pick(&|a, b| eq(a.1, y + h) && eq(b.1, y + h)),
        left: pick(&|a, b| eq(a.0, x) && eq(b.0, x)),
        right: pick(&|a, b| eq(a.0, x + w) && eq(b.0, x + w)),
    };
    (piece, edges)
}

// ---------------------------------------------------------------------------------------------
// SVG : éléments <text> et traits
// ---------------------------------------------------------------------------------------------

/// Valeur d'un attribut `nom="valeur"` d'une balise.
pub fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(tag[at..at + tag[at..].find('"')?].to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct SvgText {
    pub role: String,
    pub x: f64,
    pub y: f64,
    pub text: String,
}

/// Tous les `<text data-role="…">` du SVG (hors textes sans rôle).
pub fn svg_texts(svg: &str) -> Vec<SvgText> {
    svg.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("<text ") && l.contains("data-role=\""))
        .map(|l| {
            let body = l.split_once('>').unwrap().1;
            SvgText {
                role: attr(l, "data-role").unwrap(),
                x: attr(l, "x").unwrap().parse().unwrap(),
                y: attr(l, "y").unwrap().parse().unwrap(),
                text: body.rsplit_once("</text>").unwrap().0.to_string(),
            }
        })
        .collect()
}

/// Extrémités `(x1, y1, x2, y2)` des `<line data-role="role">`.
pub fn svg_lines(svg: &str, role: &str) -> Vec<[f64; 4]> {
    let tag = format!("data-role=\"{role}\"");
    svg.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("<line ") && l.contains(&tag))
        .map(|l| ["x1", "y1", "x2", "y2"].map(|k| attr(l, k).unwrap().parse().unwrap()))
        .collect()
}

/// Passage repère modèle → SVG `(dx, dy)` (`x_svg = x + dx`, `y_svg = dy − y`), lu sur les repères
/// de point du fichier (`<circle … r="0.6"/>` sans `data-form`) : `corners` sont les sommets du
/// rectangle de la pièce (la construction y pose un point chacun) ; le point le plus à gauche est à
/// `min x + dx`, le plus bas du modèle (plus grand `y` SVG) à `dy − min y`.
pub fn svg_frame(svg: &str, corners: &[(f64, f64)]) -> (f64, f64) {
    let found: Vec<(f64, f64)> = svg
        .lines()
        .map(str::trim)
        .filter(|l| {
            l.starts_with("<circle ") && !l.contains("data-form") && l.contains("r=\"0.6\"")
        })
        .map(|l| {
            (
                attr(l, "cx").unwrap().parse().unwrap(),
                attr(l, "cy").unwrap().parse().unwrap(),
            )
        })
        .collect();
    assert_eq!(found.len(), corners.len(), "repères de point : {found:?}");
    let min_cx = found.iter().map(|p| p.0).fold(f64::MAX, f64::min);
    let max_cy = found.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let min_x = corners.iter().map(|p| p.0).fold(f64::MAX, f64::min);
    let min_y = corners.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    (min_cx - min_x, max_cy + min_y)
}

// ---------------------------------------------------------------------------------------------
// PDF : texte placé et traits
// ---------------------------------------------------------------------------------------------

const PT_PER_MM: f64 = 72.0 / 25.4;

#[derive(Debug, Clone, PartialEq)]
pub struct PdfText {
    /// Rang de la page (flux de contenu) dans le fichier.
    pub page: usize,
    pub x_mm: f64,
    pub y_mm: f64,
    /// Angle de la ligne de base (degrés) : 0 sans matrice de texte.
    pub angle_deg: f64,
    pub size_pt: f64,
    pub text: String,
}

fn to_unicode(pdf: &str) -> HashMap<u16, String> {
    let mut map = HashMap::new();
    let mut inside = false;
    for line in pdf.lines() {
        if line.ends_with("beginbfchar") {
            inside = true;
        } else if line == "endbfchar" {
            inside = false;
        } else if inside {
            let mut it = line.split_whitespace();
            if let (Some(a), Some(b)) = (it.next(), it.next()) {
                let cid = u16::from_str_radix(a.trim_matches(['<', '>']), 16).unwrap();
                let units: Vec<u16> = b
                    .trim_matches(['<', '>'])
                    .as_bytes()
                    .chunks(4)
                    .map(|q| u16::from_str_radix(std::str::from_utf8(q).unwrap(), 16).unwrap())
                    .collect();
                map.insert(cid, String::from_utf16(&units).unwrap());
            }
        }
    }
    map
}

fn decode(map: &HashMap<u16, String>, hex: &str) -> String {
    hex.as_bytes()
        .chunks(4)
        .map(|q| {
            let cid = u16::from_str_radix(std::str::from_utf8(q).unwrap(), 16).unwrap();
            map.get(&cid).cloned().unwrap_or_else(|| "\u{FFFD}".into())
        })
        .collect()
}

/// Flux de contenu du PDF (une chaîne par flux, dans l'ordre du fichier).
pub fn pdf_streams(bytes: &[u8]) -> Vec<String> {
    let text = std::str::from_utf8(bytes).expect("PDF ASCII");
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").expect("endstream");
        out.push(body[..end].to_string());
        rest = &body[end + 9..];
    }
    out
}

/// Textes du PDF : `BT /F1 {corps} Tf 0 g {x} {y} Td <hex> Tj ET` (angle 0) ou
/// `… {c} {s} {-s} {c} {x} {y} Tm <hex> Tj ET` (texte tourné). Chaînes décodées par la table
/// `ToUnicode` **du fichier**.
pub fn pdf_texts(bytes: &[u8]) -> Vec<PdfText> {
    let text = std::str::from_utf8(bytes).expect("PDF ASCII");
    let map = to_unicode(text);
    let mut out = Vec::new();
    for (page, stream) in pdf_streams(bytes).iter().enumerate() {
        for line in stream.lines().filter(|l| l.starts_with("BT /F1 ")) {
            let toks: Vec<&str> = line.split_whitespace().collect();
            // BT /F1 size Tf 0 g …
            let size: f64 = toks[2].parse().unwrap();
            let (hex_open, hex_close) = (line.rfind('<').unwrap(), line.rfind('>').unwrap());
            let hex = &line[hex_open + 1..hex_close];
            let (x, y, angle) = if toks.contains(&"Tm") {
                let at = toks.iter().position(|t| *t == "Tm").unwrap();
                let v: Vec<f64> = toks[at - 6..at]
                    .iter()
                    .map(|t| t.parse().unwrap())
                    .collect();
                (v[4], v[5], v[1].atan2(v[0]).to_degrees())
            } else {
                let at = toks.iter().position(|t| *t == "Td").unwrap();
                (
                    toks[at - 2].parse().unwrap(),
                    toks[at - 1].parse().unwrap(),
                    0.0,
                )
            };
            out.push(PdfText {
                page,
                x_mm: x / PT_PER_MM,
                y_mm: y / PT_PER_MM,
                angle_deg: angle,
                size_pt: size,
                text: decode(&map, hex),
            });
        }
    }
    out
}

/// Segments `x1 y1 m x2 y2 l S` (mm) de chaque flux, avec leur rang de page.
pub fn pdf_segments(bytes: &[u8]) -> Vec<(usize, [f64; 4])> {
    let mut out = Vec::new();
    for (page, stream) in pdf_streams(bytes).iter().enumerate() {
        for line in stream.lines().filter(|l| l.ends_with(" l S")) {
            let t: Vec<&str> = line.split_whitespace().collect();
            if t.len() == 7 && t[2] == "m" && t[5] == "l" {
                let v = [t[0], t[1], t[3], t[4]].map(|x| x.parse::<f64>().unwrap() / PT_PER_MM);
                out.push((page, v));
            }
        }
    }
    out
}

/// Largeur d'avance (mm) d'un texte de corps `size_pt` d'après la police **embarquée dans le PDF**
/// (`/W`, `CIDToGIDMap` non nécessaire : les CID de ce PDF sont ceux de la table `/W`).
pub fn pdf_text_width_mm(bytes: &[u8], text: &str, size_pt: f64) -> f64 {
    let pdf = std::str::from_utf8(bytes).expect("PDF ASCII");
    let map = to_unicode(pdf);
    let w_at = pdf.find("/W [").expect("/W") + 4;
    let w_txt = &pdf[w_at..w_at + pdf[w_at..].find("] /").unwrap() + 1];
    let toks: Vec<&str> = w_txt
        .split_whitespace()
        .map(|t| t.trim_matches(['[', ']']))
        .filter(|t| !t.is_empty())
        .collect();
    let widths: HashMap<u16, f64> = toks
        .chunks(2)
        .map(|p| (p[0].parse().unwrap(), p[1].parse().unwrap()))
        .collect();
    let by_char: HashMap<String, u16> = map.iter().map(|(k, v)| (v.clone(), *k)).collect();
    // Largeurs en millièmes d'em.
    text.chars()
        .map(|c| widths[&by_char[&c.to_string()]])
        .sum::<f64>()
        / 1000.0
        * size_pt
        / PT_PER_MM
}

// ---------------------------------------------------------------------------------------------
// DXF : entités TEXT
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct DxfText {
    pub layer: String,
    pub x: f64,
    pub y: f64,
    pub text: String,
}

/// Tous les `TEXT` du fichier (blocs et entités), paires code / valeur lues ligne à ligne.
pub fn dxf_texts(dxf: &str) -> Vec<DxfText> {
    let lines: Vec<&str> = dxf.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].trim() == "0" && lines[i + 1].trim() == "TEXT" {
            let (mut layer, mut x, mut y, mut text) = (String::new(), 0.0, 0.0, String::new());
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j].trim() != "0" {
                let v = lines[j + 1].trim();
                match lines[j].trim() {
                    "8" => layer = v.to_string(),
                    "10" => x = v.parse().unwrap(),
                    "20" => y = v.parse().unwrap(),
                    "1" => text = v.to_string(),
                    _ => {}
                }
                j += 2;
            }
            out.push(DxfText { layer, x, y, text });
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// Sommets de la polyligne de la couche `layer` ayant le plus de sommets : on s'en sert comme
/// repère du contour de la pièce dans le fichier (jamais pour valider une valeur).
pub fn dxf_layer_vertices(dxf: &str, layer: &str) -> Vec<(f64, f64)> {
    let lines: Vec<&str> = dxf.lines().collect();
    let mut best: Vec<(f64, f64)> = Vec::new();
    // Paires code / valeur ; `POLYLINE`, ses `VERTEX` (codes 10 et 20), puis `SEQEND`.
    let (mut in_poly, mut on_layer, mut pts) = (false, false, Vec::<(f64, f64)>::new());
    let (mut entity, mut x) = (String::new(), 0.0);
    for pair in lines.chunks(2).filter(|p| p.len() == 2) {
        let (code, v) = (pair[0].trim(), pair[1].trim());
        match code {
            "0" => {
                if entity == "SEQEND" && in_poly {
                    if on_layer && pts.len() > best.len() {
                        best = pts.clone();
                    }
                    in_poly = false;
                }
                entity = v.to_string();
                if entity == "POLYLINE" {
                    (in_poly, on_layer, pts) = (true, false, Vec::new());
                }
            }
            "8" if entity == "POLYLINE" => on_layer = v == layer,
            "10" if entity == "VERTEX" => x = v.parse().unwrap(),
            "20" if entity == "VERTEX" => pts.push((x, v.parse().unwrap())),
            _ => {}
        }
    }
    best
}

// ---------------------------------------------------------------------------------------------
// HPGL : instructions LB et plumes
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct HpglLabel {
    pub pen: u32,
    /// Position en unités HPGL (40 par mm).
    pub x: i64,
    pub y: i64,
    pub text: String,
}

/// Les `LB` du plot, avec la plume courante (`SPn;`) et le dernier `PUx,y;` lu.
pub fn hpgl_labels(plot: &str) -> Vec<HpglLabel> {
    let (mut pen, mut at) = (0u32, (0i64, 0i64));
    let mut out = Vec::new();
    for line in plot.lines() {
        if let Some(n) = line.strip_prefix("SP") {
            pen = n.trim_end_matches(';').parse().unwrap_or(0);
        } else if let Some(xy) = line.strip_prefix("PU") {
            let xy = xy.trim_end_matches(';');
            if let Some((x, y)) = xy.split_once(',') {
                at = (x.parse().unwrap(), y.parse().unwrap());
            }
        } else if let Some(t) = line.strip_prefix("LB") {
            out.push(HpglLabel {
                pen,
                x: at.0,
                y: at.1,
                text: t
                    .trim_end_matches(';')
                    .trim_end_matches('\u{3}')
                    .to_string(),
            });
        }
    }
    out
}

/// Traits d'une plume : suites de points des `PD` précédées d'un `PU x,y;` sous cette plume.
pub fn hpgl_strokes(plot: &str, pen_wanted: u32) -> Vec<Vec<(i64, i64)>> {
    let (mut pen, mut start) = (0u32, None);
    let mut out = Vec::new();
    for line in plot.lines() {
        if let Some(n) = line.strip_prefix("SP") {
            pen = n.trim_end_matches(';').parse().unwrap_or(0);
        } else if let Some(xy) = line.strip_prefix("PU") {
            start = xy
                .trim_end_matches(';')
                .split_once(',')
                .map(|(x, y)| (x.parse::<i64>().unwrap(), y.parse::<i64>().unwrap()));
        } else if let Some(v) = line.strip_prefix("PD") {
            if pen == pen_wanted {
                let nums: Vec<i64> = v
                    .trim_end_matches(';')
                    .split(',')
                    .map(|n| n.parse().unwrap())
                    .collect();
                let mut pts: Vec<(i64, i64)> = start.into_iter().collect();
                pts.extend(nums.chunks(2).map(|c| (c[0], c[1])));
                out.push(pts);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// CSV du rapport
// ---------------------------------------------------------------------------------------------

/// Lignes du CSV (BOM retiré, fins de ligne CR LF, séparateur `;`, champs entre guillemets
/// dépouillés).
pub fn csv_rows(bytes: &[u8]) -> Vec<Vec<String>> {
    let text = std::str::from_utf8(bytes).expect("UTF-8");
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    text.split("\r\n")
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.split(';')
                .map(|c| c.trim_matches('"').replace("\"\"", "\""))
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// PDF : repère de point
// ---------------------------------------------------------------------------------------------

/// Quart de tour(s) trigonométrique(s) d'un point du modèle : (x, y) → (−y, x) pour un quart.
pub fn rot(p: (f64, f64), turns: u8) -> (f64, f64) {
    match turns % 4 {
        0 => p,
        1 => (-p.1, p.0),
        2 => (-p.0, -p.1),
        _ => (p.1, -p.0),
    }
}

/// Passage modèle → page `(px, py)` (`page = rot(modèle, turns) + (px, py)`), lu sur les croix de
/// point de la page `page` (deux traits de 2 mm qui se coupent en leur milieu) : `corners` sont
/// les sommets du rectangle de la pièce, un point chacun ; le plus petit `(x, y)` lexicographique
/// des croix est celui du sommet tourné de plus petit `(x, y)`.
pub fn pdf_frame(bytes: &[u8], page: usize, corners: &[(f64, f64)], turns: u8) -> (f64, f64) {
    let mut centres: Vec<(f64, f64)> = Vec::new();
    for (p, s) in pdf_segments(bytes) {
        if p == page && ((s[2] - s[0]).hypot(s[3] - s[1]) - 2.0).abs() < 1e-3 {
            let m = ((s[0] + s[2]) / 2.0, (s[1] + s[3]) / 2.0);
            if !centres
                .iter()
                .any(|c| (c.0 - m.0).abs() < 1e-3 && (c.1 - m.1).abs() < 1e-3)
            {
                centres.push(m);
            }
        }
    }
    assert_eq!(centres.len(), corners.len(), "croix de point : {centres:?}");
    // Ordre (x, y) à 0,001 mm près (le fichier arrondit les coordonnées).
    let lex = |a: &(f64, f64), b: &(f64, f64)| {
        if (a.0 - b.0).abs() > 1e-3 {
            a.0.total_cmp(&b.0)
        } else {
            a.1.total_cmp(&b.1)
        }
    };
    let found = centres.iter().copied().min_by(lex).unwrap();
    let model = corners.iter().map(|c| rot(*c, turns)).min_by(lex).unwrap();
    (found.0 - model.0, found.1 - model.1)
}

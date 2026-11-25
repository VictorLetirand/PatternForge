//! Aides communes aux tests de la nomenclature et de la fiche technique (US-11.2, US-11.3).
#![allow(dead_code)]

use pf_core::*;
use pf_geometry::PointMm;

pub fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Polygone fermé sur la première feuille, extrait en pièce nommée.
pub fn poly_piece(p: &mut Project, pts: &[(f64, f64)], name: &str) -> PieceId {
    let sheet = p.sheets[0].id;
    let entities = p
        .apply(Command::AddPolygon {
            sheet,
            vertices: pts.iter().map(|q| PointRef::New(pt(q.0, q.1))).collect(),
            closed: true,
            role: PathRole::default(),
            layer: None,
        })
        .unwrap()
        .entities;
    p.apply(Command::ExtractPiece {
        sheet,
        source: PieceSource::Paths { entities },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

/// Quart de disque de rayon `r` centré en (cx, cy) : un arc et deux rayons.
pub fn quarter_piece(p: &mut Project, cx: f64, cy: f64, r: f64, name: &str) -> PieceId {
    let sheet = p.sheets[0].id;
    let arc = p
        .apply(Command::AddArc {
            sheet,
            center: PointRef::New(pt(cx, cy)),
            radius_mm: r,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    let mut seg = |a: (f64, f64), b: (f64, f64)| {
        p.apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(a.0, a.1)),
            to: PointRef::New(pt(b.0, b.1)),
        })
        .unwrap()
        .entities[0]
    };
    let r1 = seg((cx, cy), (cx + r, cy));
    let r2 = seg((cx, cy), (cx, cy + r));
    p.apply(Command::ExtractPiece {
        sheet,
        source: PieceSource::Paths {
            entities: vec![arc, r1, r2],
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

pub fn card(
    p: &mut Project,
    piece: PieceId,
    quantity: u32,
    pair: bool,
    material: Option<&str>,
    comment: Option<&str>,
) {
    let name = p.piece(piece).unwrap().name.clone();
    p.apply(Command::SetPieceCard {
        piece,
        name,
        code: None,
        quantity,
        mirrored_pair: pair,
        material: material.map(str::to_string),
        category: None,
        comment: comment.map(str::to_string),
    })
    .unwrap();
}

pub fn point_at(p: &Project, at: (f64, f64)) -> PointId {
    let s = &p.sheets[0];
    s.points
        .iter()
        .find(|q| pf_geometry::distance_mm(s.position(q.id).unwrap(), pt(at.0, at.1)) < 1e-9)
        .unwrap()
        .id
}

/// Gamme de `labels` (base = indice `base`) et son identifiant.
pub fn add_range(p: &mut Project, labels: &[&str], base: usize) -> SizeRangeId {
    p.apply(Command::AddSizeRange {
        name: "Gamme".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels.iter().map(|s| s.to_string()).collect(),
        base,
    })
    .unwrap();
    p.sizes.last().unwrap().id
}

pub fn grade(
    p: &mut Project,
    range: SizeRangeId,
    size_index: usize,
    at: (f64, f64),
    d: (f64, f64),
) {
    let point = point_at(p, at);
    let size = p.size_range(range).unwrap().sizes[size_index].id;
    let sheet = p.sheets[0].id;
    p.apply(Command::SetPointGrading {
        sheet,
        point,
        size,
        dx_mm: d.0,
        dy_mm: d.1,
    })
    .unwrap();
}

/// Page d'une fiche technique relue **indépendamment** : textes (chaînes décodées) et segments
/// dessinés `x1 y1 x2 y2` en mm (la fiche trace des polylignes, pas seulement des filets).
#[derive(Default)]
pub struct FichePage {
    pub texts: Vec<String>,
    pub segments: Vec<[f64; 4]>,
}

pub fn fiche_pages(bytes: &[u8]) -> Vec<FichePage> {
    const PT_PER_MM: f64 = 72.0 / 25.4;
    let text = crate::common::pdf_text::readable(bytes);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").expect("endstream");
        let mut page = FichePage::default();
        for line in body[..end].lines() {
            if line.starts_with("BT /F1 ") {
                let open = line.find(" Td (").unwrap() + 5;
                let close = line.rfind(") Tj ET").unwrap();
                let mut s = String::new();
                let mut chars = line[open..close].chars();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        s.extend(chars.next());
                    } else {
                        s.push(c);
                    }
                }
                page.texts.push(s);
            } else if line.ends_with(" l S") {
                let t: Vec<&str> = line.split_whitespace().collect();
                let v: Vec<f64> = [t[0], t[1], t[3], t[4]]
                    .iter()
                    .map(|x| x.parse::<f64>().unwrap() / PT_PER_MM)
                    .collect();
                page.segments.push([v[0], v[1], v[2], v[3]]);
            }
        }
        out.push(page);
        rest = &body[end + 9..];
    }
    out
}

//! Outils des tests du DXF générique (US-12.3) : fichiers « d'un tiers imaginaire » écrits à la
//! main (autre style que l'écrivain : R2018, poignées, marqueurs de sous-classe, fins de ligne
//! CRLF, nombres en notation scientifique), mathématiques de contrôle **indépendantes** du code
//! testé (longueurs, aires, B-splines par la définition de Cox–de Boor), lecteur minimal des
//! fichiers produits par l'export, projets aléatoires et résumé géométrique d'une feuille.
#![allow(dead_code)]

use super::Rng;
use pf_core::{Command, EntityKind, ImportItem, PathRole, PointRef, Project, SheetId};
use pf_geometry::PointMm;

// ----- construction de fichiers ----------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Pairs(pub Vec<(i32, String)>);

impl Pairs {
    pub fn p(&mut self, code: i32, v: impl ToString) -> &mut Self {
        self.0.push((code, v.to_string()));
        self
    }
    pub fn extend(&mut self, o: &Pairs) -> &mut Self {
        self.0.extend(o.0.iter().cloned());
        self
    }
    /// Texte : `crlf` pour des fins de ligne Windows, valeurs précédées d'espaces comme certains
    /// logiciels en écrivent pour les codes.
    pub fn text(&self, crlf: bool) -> String {
        let nl = if crlf { "\r\n" } else { "\n" };
        let mut s = String::new();
        for (c, v) in &self.0 {
            s.push_str(&format!("{c:>3}{nl}{v}{nl}"));
        }
        s
    }
}

pub fn section(name: &str, body: &Pairs) -> Pairs {
    let mut p = Pairs::default();
    p.p(0, "SECTION").p(2, name);
    p.extend(body);
    p.p(0, "ENDSEC");
    p
}

pub fn file(header: &Pairs, entities: &Pairs) -> Pairs {
    let mut p = Pairs::default();
    p.extend(&section("HEADER", header));
    p.extend(&section("ENTITIES", entities));
    p.p(0, "EOF");
    p
}

pub fn header(insunits: Option<i64>) -> Pairs {
    let mut h = Pairs::default();
    h.p(9, "$ACADVER").p(1, "AC1032");
    h.p(9, "$DWGCODEPAGE").p(3, "ANSI_1252");
    h.p(9, "$EXTMIN").p(10, "0.0").p(20, "0.0").p(30, "0.0");
    h.p(9, "$EXTMAX")
        .p(10, "1.0E+03")
        .p(20, "1.0E+03")
        .p(30, "0.0");
    if let Some(u) = insunits {
        h.p(9, "$INSUNITS").p(70, u);
    }
    h.p(9, "$MEASUREMENT").p(70, 1);
    h
}

fn entity_start(p: &mut Pairs, kind: &str, handle: u32, layer: &str, sub: &str) {
    p.p(0, kind)
        .p(5, format!("{handle:X}"))
        .p(100, "AcDbEntity")
        .p(8, layer)
        .p(100, sub);
}

pub fn line(h: u32, layer: &str, a: (f64, f64, f64), b: (f64, f64, f64)) -> Pairs {
    let mut p = Pairs::default();
    entity_start(&mut p, "LINE", h, layer, "AcDbLine");
    p.p(10, a.0).p(20, a.1).p(30, a.2);
    p.p(11, b.0).p(21, b.1).p(31, b.2);
    p
}

pub fn point(h: u32, layer: &str, a: (f64, f64)) -> Pairs {
    let mut p = Pairs::default();
    entity_start(&mut p, "POINT", h, layer, "AcDbPoint");
    p.p(10, a.0).p(20, a.1).p(30, 0.0);
    p
}

pub fn circle(h: u32, layer: &str, c: (f64, f64), r: f64) -> Pairs {
    let mut p = Pairs::default();
    entity_start(&mut p, "CIRCLE", h, layer, "AcDbCircle");
    p.p(10, c.0).p(20, c.1).p(30, 0.0).p(40, r);
    p
}

/// Arc anti-horaire, angles en degrés.
pub fn arc(h: u32, layer: &str, c: (f64, f64), r: f64, a0: f64, a1: f64) -> Pairs {
    let mut p = Pairs::default();
    entity_start(&mut p, "ARC", h, layer, "AcDbCircle");
    p.p(10, c.0).p(20, c.1).p(30, 0.0).p(40, r);
    p.p(100, "AcDbArc").p(50, a0).p(51, a1);
    p
}

pub fn lwpolyline(h: u32, layer: &str, verts: &[(f64, f64, f64)], closed: bool) -> Pairs {
    let mut p = Pairs::default();
    entity_start(&mut p, "LWPOLYLINE", h, layer, "AcDbPolyline");
    p.p(90, verts.len()).p(70, i32::from(closed)).p(43, 0.0);
    for (x, y, b) in verts {
        p.p(10, x).p(20, y);
        if *b != 0.0 {
            p.p(42, b);
        }
    }
    p
}

/// POLYLINE / VERTEX / SEQEND ; `flags` = code 70 de l'en-tête, `z` = cote des sommets.
pub fn polyline_old(h: u32, layer: &str, verts: &[(f64, f64, f64)], flags: i32, z: f64) -> Pairs {
    let mut p = Pairs::default();
    p.p(0, "POLYLINE").p(5, format!("{h:X}")).p(8, layer);
    p.p(66, 1).p(10, 0.0).p(20, 0.0).p(30, 0.0).p(70, flags);
    for (i, (x, y, b)) in verts.iter().enumerate() {
        p.p(0, "VERTEX")
            .p(5, format!("{:X}", h + 1 + i as u32))
            .p(8, layer);
        p.p(10, x).p(20, y).p(30, z);
        if *b != 0.0 {
            p.p(42, b);
        }
        p.p(70, if flags & 8 != 0 { 32 } else { 0 });
    }
    p.p(0, "SEQEND").p(5, format!("{:X}", h + 900)).p(8, layer);
    p
}

/// SPLINE à points de contrôle : `degree`, nœuds, contrôle, poids facultatifs.
pub fn spline(
    h: u32,
    layer: &str,
    degree: usize,
    knots: &[f64],
    ctrl: &[(f64, f64)],
    weights: &[f64],
) -> Pairs {
    let mut p = Pairs::default();
    entity_start(&mut p, "SPLINE", h, layer, "AcDbSpline");
    p.p(210, 0.0).p(220, 0.0).p(230, 1.0);
    p.p(70, if weights.is_empty() { 8 } else { 12 });
    p.p(71, degree)
        .p(72, knots.len())
        .p(73, ctrl.len())
        .p(74, 0);
    p.p(42, "1.0E-07").p(43, "1.0E-07").p(44, "1.0E-10");
    for k in knots {
        p.p(40, k);
    }
    for w in weights {
        p.p(41, w);
    }
    for (x, y) in ctrl {
        p.p(10, x).p(20, y).p(30, 0.0);
    }
    p
}

pub fn spline_fit(h: u32, layer: &str, fit: &[(f64, f64)], closed: bool) -> Pairs {
    let mut p = Pairs::default();
    entity_start(&mut p, "SPLINE", h, layer, "AcDbSpline");
    p.p(70, if closed { 1 | 8 } else { 8 }).p(71, 3);
    p.p(72, 0).p(73, 0).p(74, fit.len());
    for (x, y) in fit {
        p.p(11, x).p(21, y).p(31, 0.0);
    }
    p
}

pub fn other(kind: &str, h: u32, layer: &str) -> Pairs {
    let mut p = Pairs::default();
    p.p(0, kind)
        .p(5, format!("{h:X}"))
        .p(100, "AcDbEntity")
        .p(8, layer);
    p.p(10, 1.0).p(20, 2.0).p(30, 0.0);
    p
}

// ----- mathématiques de contrôle (indépendantes) -----------------------------------------------

pub fn d((ax, ay): (f64, f64), (bx, by): (f64, f64)) -> f64 {
    ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
}

pub fn xy(p: PointMm) -> (f64, f64) {
    (p.x, p.y)
}

/// Balayage signé (rad) d'un arc de `s` à `e` autour de `c` dans le sens `ccw`.
pub fn sweep(c: (f64, f64), s: (f64, f64), e: (f64, f64), ccw: bool) -> f64 {
    let a0 = (s.1 - c.1).atan2(s.0 - c.0);
    let a1 = (e.1 - c.1).atan2(e.0 - c.0);
    let tau = std::f64::consts::TAU;
    if ccw {
        (a1 - a0).rem_euclid(tau)
    } else {
        -((a0 - a1).rem_euclid(tau))
    }
}

pub fn item_length(i: &ImportItem) -> f64 {
    match i {
        ImportItem::Point { .. } => 0.0,
        ImportItem::Segment { from, to } => d(xy(*from), xy(*to)),
        ImportItem::Circle { radius_mm, .. } => std::f64::consts::TAU * radius_mm,
        ImportItem::Arc {
            center,
            start,
            end,
            ccw,
        } => {
            let r = d(xy(*center), xy(*start));
            r * sweep(xy(*center), xy(*start), xy(*end), *ccw).abs()
        }
        ImportItem::Spline { points } => points
            .windows(2)
            .map(|w| d(xy(w[0]), xy(w[1])))
            .sum::<f64>(),
        ImportItem::Bezier { start, c1, c2, end } => {
            bezier_length([xy(*start), xy(*c1), xy(*c2), xy(*end)])
        }
    }
}

/// Aire signée (anti-horaire positive) d'une chaîne **fermée** d'objets dans l'ordre : théorème
/// de Green, `½ ∮ x dy − y dx` ; un arc ajoute `½ [r²θ + cx (qy − py) + cy (px − qx)]`.
pub fn chain_area(items: &[ImportItem]) -> f64 {
    let mut a = 0.0;
    for i in items {
        match i {
            ImportItem::Segment { from, to } => a += 0.5 * (from.x * to.y - to.x * from.y),
            ImportItem::Arc {
                center,
                start,
                end,
                ccw,
            } => {
                let r = d(xy(*center), xy(*start));
                let th = sweep(xy(*center), xy(*start), xy(*end), *ccw);
                a += 0.5
                    * (r * r * th + center.x * (end.y - start.y) + center.y * (start.x - end.x));
            }
            _ => panic!("chaîne de segments et d'arcs seulement"),
        }
    }
    a
}

/// Longueur d'une Bézier cubique par Simpson (20 000 intervalles).
pub fn bezier_length(p: [(f64, f64); 4]) -> f64 {
    let deriv = |t: f64| {
        let u = 1.0 - t;
        let x = 3.0
            * (u * u * (p[1].0 - p[0].0)
                + 2.0 * u * t * (p[2].0 - p[1].0)
                + t * t * (p[3].0 - p[2].0));
        let y = 3.0
            * (u * u * (p[1].1 - p[0].1)
                + 2.0 * u * t * (p[2].1 - p[1].1)
                + t * t * (p[3].1 - p[2].1));
        (x * x + y * y).sqrt()
    };
    let n = 20_000;
    let h = 1.0 / n as f64;
    let mut s = deriv(0.0) + deriv(1.0);
    for i in 1..n {
        s += deriv(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * h / 3.0
}

pub fn bezier_point(p: [(f64, f64); 4], t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    (
        (0..4).map(|i| w[i] * p[i].0).sum(),
        (0..4).map(|i| w[i] * p[i].1).sum(),
    )
}

/// Fonction de base B-spline `N_{i,p}(t)` par la récurrence de Cox–de Boor (définition).
pub fn basis(i: usize, p: usize, t: f64, knots: &[f64]) -> f64 {
    if p == 0 {
        return if knots[i] <= t && t < knots[i + 1] {
            1.0
        } else {
            0.0
        };
    }
    let mut v = 0.0;
    let d1 = knots[i + p] - knots[i];
    if d1 > 0.0 {
        v += (t - knots[i]) / d1 * basis(i, p - 1, t, knots);
    }
    let d2 = knots[i + p + 1] - knots[i + 1];
    if d2 > 0.0 {
        v += (knots[i + p + 1] - t) / d2 * basis(i + 1, p - 1, t, knots);
    }
    v
}

pub fn bspline_point(
    degree: usize,
    knots: &[f64],
    ctrl: &[(f64, f64)],
    weights: &[f64],
    t: f64,
) -> (f64, f64) {
    let (mut x, mut y, mut w) = (0.0, 0.0, 0.0);
    for (i, c) in ctrl.iter().enumerate() {
        let wi = weights.get(i).copied().unwrap_or(1.0);
        let n = basis(i, degree, t, knots) * wi;
        x += n * c.0;
        y += n * c.1;
        w += n;
    }
    (x / w, y / w)
}

/// Distance d'un point à une polyligne ouverte.
pub fn dist_to_polyline(p: (f64, f64), pts: &[(f64, f64)]) -> f64 {
    pts.windows(2)
        .map(|w| dist_to_segment(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

pub fn dist_to_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let l2 = vx * vx + vy * vy;
    if l2 == 0.0 {
        return d(p, a);
    }
    let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / l2).clamp(0.0, 1.0);
    d(p, (a.0 + vx * t, a.1 + vy * t))
}

// ----- lecture minimale de l'export (indépendante) ---------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Ex {
    Line {
        layer: String,
        a: (f64, f64),
        b: (f64, f64),
    },
    Point {
        layer: String,
        p: (f64, f64),
    },
    Circle {
        layer: String,
        c: (f64, f64),
        r: f64,
    },
    Arc {
        layer: String,
        c: (f64, f64),
        r: f64,
        a0: f64,
        a1: f64,
    },
    Poly {
        layer: String,
        pts: Vec<(f64, f64)>,
        closed: bool,
    },
}

pub struct ExFile {
    pub header: Vec<(String, Vec<(i32, String)>)>,
    pub layers: Vec<(String, i64, i64)>,
    pub entities: Vec<Ex>,
}

pub fn read_export(text: &str) -> Result<ExFile, String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() % 2 != 0 {
        return Err("nombre de lignes impair".into());
    }
    let pairs: Vec<(i32, String)> = lines
        .chunks(2)
        .map(|c| {
            c[0].trim()
                .parse::<i32>()
                .map(|code| (code, c[1].to_string()))
                .map_err(|_| format!("code invalide : {}", c[0]))
        })
        .collect::<Result<_, _>>()?;
    let mut f = ExFile {
        header: Vec::new(),
        layers: Vec::new(),
        entities: Vec::new(),
    };
    let num = |v: &str| v.trim().parse::<f64>().map_err(|e| format!("{v}: {e}"));
    let mut i = 0;
    let mut section = String::new();
    // Enregistrements : liste de (type, paires).
    let mut recs: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    while i < pairs.len() {
        if pairs[i].0 == 0 {
            let kind = pairs[i].1.trim().to_string();
            let mut j = i + 1;
            while j < pairs.len() && pairs[j].0 != 0 {
                j += 1;
            }
            recs.push((kind, pairs[i + 1..j].to_vec()));
            i = j;
        } else {
            return Err(format!("paire hors enregistrement : {:?}", pairs[i]));
        }
    }
    let get = |p: &[(i32, String)], c: i32| p.iter().find(|x| x.0 == c).map(|x| x.1.clone());
    let n = |p: &[(i32, String)], c: i32| -> Result<f64, String> {
        num(&get(p, c).ok_or(format!("code {c} absent"))?)
    };
    let mut k = 0;
    while k < recs.len() {
        let (kind, body) = &recs[k];
        match kind.as_str() {
            "SECTION" => section = get(body, 2).unwrap_or_default(),
            "ENDSEC" => section.clear(),
            "EOF" => {
                if k + 1 != recs.len() {
                    return Err("contenu après EOF".into());
                }
            }
            "LAYER" if section == "TABLES" => f.layers.push((
                get(body, 2).unwrap_or_default(),
                n(body, 62)? as i64,
                n(body, 70)? as i64,
            )),
            "LINE" if section == "ENTITIES" => f.entities.push(Ex::Line {
                layer: get(body, 8).unwrap_or_default(),
                a: (n(body, 10)?, n(body, 20)?),
                b: (n(body, 11)?, n(body, 21)?),
            }),
            "POINT" if section == "ENTITIES" => f.entities.push(Ex::Point {
                layer: get(body, 8).unwrap_or_default(),
                p: (n(body, 10)?, n(body, 20)?),
            }),
            "CIRCLE" if section == "ENTITIES" => f.entities.push(Ex::Circle {
                layer: get(body, 8).unwrap_or_default(),
                c: (n(body, 10)?, n(body, 20)?),
                r: n(body, 40)?,
            }),
            "ARC" if section == "ENTITIES" => f.entities.push(Ex::Arc {
                layer: get(body, 8).unwrap_or_default(),
                c: (n(body, 10)?, n(body, 20)?),
                r: n(body, 40)?,
                a0: n(body, 50)?,
                a1: n(body, 51)?,
            }),
            "POLYLINE" if section == "ENTITIES" => {
                let layer = get(body, 8).unwrap_or_default();
                let closed = n(body, 70)? as i64 & 1 == 1;
                let mut pts = Vec::new();
                k += 1;
                loop {
                    let (kk, bb) = recs.get(k).ok_or("POLYLINE sans SEQEND")?;
                    match kk.as_str() {
                        "VERTEX" => pts.push((n(bb, 10)?, n(bb, 20)?)),
                        "SEQEND" => break,
                        other => return Err(format!("{other} dans une POLYLINE")),
                    }
                    k += 1;
                }
                f.entities.push(Ex::Poly { layer, pts, closed });
            }
            "LTYPE" | "TABLE" | "ENDTAB" | "LAYER" => {}
            other if section == "ENTITIES" => return Err(format!("entité inattendue : {other}")),
            _ => {}
        }
        if section == "HEADER" && kind == "SECTION" {
            // En-tête : (9 nom) suivi de ses paires.
        }
        k += 1;
    }
    // En-tête relu à part.
    let mut cur: Option<(String, Vec<(i32, String)>)> = None;
    let mut in_header = false;
    for (c, v) in &pairs {
        if *c == 2 && v.trim() == "HEADER" {
            in_header = true;
            continue;
        }
        if *c == 0 && v.trim() == "ENDSEC" && in_header {
            if let Some(x) = cur.take() {
                f.header.push(x);
            }
            break;
        }
        if in_header {
            if *c == 9 {
                if let Some(x) = cur.take() {
                    f.header.push(x);
                }
                cur = Some((v.trim().to_string(), Vec::new()));
            } else if let Some(x) = cur.as_mut() {
                x.1.push((*c, v.clone()));
            }
        }
    }
    Ok(f)
}

impl ExFile {
    pub fn var(&self, name: &str) -> Option<&Vec<(i32, String)>> {
        self.header.iter().find(|h| h.0 == name).map(|h| &h.1)
    }
}

// ----- projets aléatoires et résumé géométrique ------------------------------------------------

pub fn pm(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn coord(rng: &mut Rng) -> f64 {
    (rng.range(-400.0, 400.0) * 1000.0).round() / 1000.0
}

/// Feuille aléatoire de géométrie de construction : segments, arcs (deux sens), cercles,
/// Béziers, courbes lisses, points isolés, rôles et calques variés. Tout passe par les commandes.
pub fn random_project(rng: &mut Rng) -> (Project, SheetId) {
    let mut p = Project::new("aleatoire");
    let s = p.sheets[0].id;
    let layers: Vec<_> = (0..rng.below(3))
        .map(|i| {
            p.apply(Command::AddLayer {
                sheet: s,
                name: format!("Calque {}", i + 1),
            })
            .unwrap()
            .layers[0]
        })
        .collect();
    let n = 2 + rng.below(9);
    for _ in 0..n {
        let from = PointRef::New(pm(coord(rng), coord(rng)));
        let cmd = match rng.below(7) {
            0 | 1 => Command::AddSegment {
                sheet: s,
                from,
                to: PointRef::New(pm(coord(rng), coord(rng))),
            },
            2 => Command::AddArc {
                sheet: s,
                center: from,
                radius_mm: rng.range(5.0, 150.0),
                start_deg: rng.range(0.0, 360.0),
                end_deg: rng.range(0.0, 360.0),
                ccw: rng.below(2) == 0,
            },
            3 => Command::AddCircle {
                sheet: s,
                center: from,
                radius_mm: rng.range(1.0, 100.0),
            },
            4 => Command::AddCurve {
                sheet: s,
                start: from,
                c1: pm(coord(rng), coord(rng)),
                c2: pm(coord(rng), coord(rng)),
                end: PointRef::New(pm(coord(rng), coord(rng))),
            },
            5 => Command::AddSpline {
                sheet: s,
                points: (0..2 + rng.below(4))
                    .map(|_| PointRef::New(pm(coord(rng), coord(rng))))
                    .collect(),
            },
            _ => Command::AddPoint {
                sheet: s,
                at: pm(coord(rng), coord(rng)),
            },
        };
        let Ok(out) = p.apply(cmd) else { continue };
        if let Some(&e) = out.entities.first() {
            if rng.below(3) == 0 {
                let role = rng.pick(&PathRole::ALL);
                let _ = p.apply(Command::SetPathRole {
                    sheet: s,
                    entity: e,
                    role,
                });
            }
            if !layers.is_empty() && rng.below(2) == 0 {
                let l = rng.pick(&layers);
                let _ = p.apply(Command::SetObjectsLayer {
                    sheet: s,
                    points: vec![],
                    entities: vec![e],
                    layer: Some(l),
                });
            }
        }
    }
    (p, s)
}

/// (centre, rayon, départ, arrivée) d'un arc anti-horaire.
pub type ArcSummary = ((f64, f64), f64, (f64, f64), (f64, f64));

/// Résumé géométrique d'une feuille, calculé sans le code d'export ni d'import.
#[derive(Debug, Default)]
pub struct Summary {
    pub segments: Vec<((f64, f64), (f64, f64))>,
    pub circles: Vec<((f64, f64), f64)>,
    /// (centre, rayon, départ, arrivée) dans le sens anti-horaire.
    pub arcs: Vec<ArcSummary>,
    pub points: Vec<(f64, f64)>,
    /// Échantillons de courbes (Bézier, lisses).
    pub curve_samples: Vec<(f64, f64)>,
    pub curve_length: f64,
    pub total_length: f64,
    pub curves: usize,
}

pub fn summarize(p: &Project, s: SheetId) -> Summary {
    let sheet = p.sheet(s).unwrap();
    let mut out = Summary::default();
    let mut used = std::collections::HashSet::new();
    for e in &sheet.entities {
        used.extend(e.kind.all_points());
        let shape = sheet.shape(e.id).unwrap();
        match (&e.kind, shape) {
            (EntityKind::Segment { .. }, pf_core::Shape::Line(a, b)) => {
                out.segments.push((xy(a), xy(b)));
                out.total_length += d(xy(a), xy(b));
            }
            (EntityKind::Circle { .. }, pf_core::Shape::Circle(c)) => {
                out.circles.push((xy(c.circle.center), c.circle.radius_mm));
                out.total_length += std::f64::consts::TAU * c.circle.radius_mm;
            }
            (EntityKind::Arc { ccw, .. }, pf_core::Shape::Arc(a)) => {
                let (s0, e0) = (xy(a.start_point()), xy(a.end_point()));
                let (s1, e1) = if *ccw { (s0, e0) } else { (e0, s0) };
                out.arcs.push((xy(a.center), a.radius_mm, s1, e1));
                out.total_length += a.length_mm();
            }
            (_, pf_core::Shape::Cubic(c)) => {
                out.curves += 1;
                let l = c.length_mm(1e-9).unwrap();
                out.curve_length += l;
                out.total_length += l;
                out.curve_samples.extend(c.sample(60).into_iter().map(xy));
            }
            (_, pf_core::Shape::Spline(sp)) => {
                out.curves += 1;
                let l = sp.length_mm().unwrap();
                out.curve_length += l;
                out.total_length += l;
                out.curve_samples.extend(sp.sample(30).into_iter().map(xy));
            }
            _ => panic!("forme inattendue"),
        }
    }
    for pt in &sheet.points {
        if !used.contains(&pt.id) {
            out.points.push(xy(sheet.position(pt.id).unwrap()));
        }
    }
    out
}

fn has_segment(imp: &Summary, a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
    imp.segments
        .iter()
        .any(|(c, e)| (d(a, *c) <= tol && d(b, *e) <= tol) || (d(a, *e) <= tol && d(b, *c) <= tol))
}

/// Compare la géométrie réimportée à l'originale ; renvoie la première différence.
pub fn compare(orig: &Summary, imp: &Summary, tol: f64) -> Result<(), String> {
    // Segments, cercles, arcs et points : exacts à 1e-4 mm ; courbes : `tol` (discrétisation).
    let exact = tol.min(1e-4);
    for (a, b) in &orig.segments {
        if !has_segment(imp, *a, *b, exact) {
            return Err(format!("segment {a:?}-{b:?} absent après réimport"));
        }
    }
    for (c, r) in &orig.circles {
        if !imp
            .circles
            .iter()
            .any(|(c2, r2)| d(*c, *c2) <= exact && (r - r2).abs() <= exact)
        {
            return Err(format!("cercle {c:?} r={r} absent"));
        }
    }
    if orig.circles.len() != imp.circles.len() {
        return Err("nombre de cercles différent".into());
    }
    for (c, r, s, e) in &orig.arcs {
        if !imp.arcs.iter().any(|(c2, r2, s2, e2)| {
            d(*c, *c2) <= exact
                && (r - r2).abs() <= exact
                && d(*s, *s2) <= exact
                && d(*e, *e2) <= exact
        }) {
            return Err(format!("arc {c:?} r={r} {s:?}->{e:?} absent"));
        }
    }
    if orig.arcs.len() != imp.arcs.len() {
        return Err("nombre d'arcs différent".into());
    }
    for p in &orig.points {
        if !imp.points.iter().any(|q| d(*p, *q) <= exact) {
            return Err(format!("point isolé {p:?} absent"));
        }
    }
    // Courbes : chaque échantillon de l'original est à moins de `tol` d'un segment réimporté.
    for p in &orig.curve_samples {
        let near = imp
            .segments
            .iter()
            .map(|(a, b)| dist_to_segment(*p, *a, *b))
            .fold(f64::INFINITY, f64::min);
        if near > tol {
            return Err(format!("échantillon de courbe {p:?} à {near} mm"));
        }
    }
    // Longueur totale : l'écart ne vient que de la discrétisation des courbes.
    let slack = tol * (orig.curves as f64 + 1.0);
    if (orig.total_length - imp.total_length).abs() > slack {
        return Err(format!(
            "longueur totale {} contre {}",
            orig.total_length, imp.total_length
        ));
    }
    Ok(())
}

// ----- le fichier d'un tiers imaginaire --------------------------------------------------------

pub fn t8() -> f64 {
    (std::f64::consts::PI / 8.0).tan()
}

/// Le fichier d'un tiers imaginaire.
pub fn tiers(insunits: Option<i64>) -> Vec<u8> {
    let mut e = Pairs::default();
    let rect = [
        (10.0, 0.0, 0.0),
        (90.0, 0.0, t8()),
        (100.0, 10.0, 0.0),
        (100.0, 50.0, t8()),
        (90.0, 60.0, 0.0),
        (10.0, 60.0, t8()),
        (0.0, 50.0, 0.0),
        (0.0, 10.0, t8()),
    ];
    e.extend(&lwpolyline(0x30, "Rect", &rect, true));
    e.extend(&arc(0x31, "Arcs", (200.0, 100.0), 50.0, 30.0, 150.0));
    e.extend(&arc(0x32, "Arcs", (200.0, 100.0), 50.0, 300.0, 60.0));
    e.extend(&circle(0x33, "Cercle", (300.0, 0.0), 25.0));
    e.extend(&spline(
        0x34,
        "Bez",
        3,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        &[(400.0, 0.0), (400.0, 40.0), (460.0, 40.0), (460.0, 0.0)],
        &[],
    ));
    e.extend(&spline(
        0x35,
        "Spl6",
        3,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0, 3.0],
        &SPL6,
        &[],
    ));
    e.extend(&spline(
        0x36,
        "Rat",
        2,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[(500.0, 300.0), (500.0, 350.0), (450.0, 350.0)],
        &[1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0],
    ));
    e.extend(&polyline_old(
        0x40,
        "Poly2d",
        &[(0.0, 200.0, 1.0), (100.0, 200.0, 0.0), (100.0, 260.0, 0.0)],
        0,
        0.0,
    ));
    e.extend(&polyline_old(
        0x50,
        "Poly3d",
        &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0)],
        8,
        5.0,
    ));
    e.extend(&line(0x60, "0", (0.0, 700.0, 5.0), (10.0, 700.0, 5.0)));
    e.extend(&point(0x61, "0", (7.0, 7.0)));
    e.extend(&line(0x62, "Deg", (3.0, 3.0, 0.0), (3.0, 3.0, 0.0)));
    e.extend(&line(0x63, "Dup", (0.0, 800.0, 0.0), (10.0, 800.0, 0.0)));
    e.extend(&line(0x64, "Dup", (10.0, 800.0, 0.0), (0.0, 800.0, 0.0)));
    e.extend(&line(
        0x65,
        "RECT",
        (500.0, 500.0, 0.0),
        (510.0, 500.0, 0.0),
    ));
    e.extend(&line(
        0x66,
        "Fa\\U+00E7ade",
        (0.0, 900.0, 0.0),
        (10.0, 900.0, 0.0),
    ));
    e.extend(&spline_fit(
        0x70,
        "Fit",
        &[(0.0, 500.0), (50.0, 540.0), (100.0, 500.0), (150.0, 540.0)],
        false,
    ));
    e.extend(&spline_fit(
        0x71,
        "FitClosed",
        &[(0.0, 600.0), (50.0, 650.0), (100.0, 600.0), (50.0, 550.0)],
        true,
    ));
    for (i, k) in [
        "TEXT",
        "MTEXT",
        "HATCH",
        "DIMENSION",
        "INSERT",
        "INSERT",
        "3DFACE",
        "ELLIPSE",
    ]
    .iter()
    .enumerate()
    {
        e.extend(&other(k, 0x80 + i as u32, "Autres"));
    }
    let mut blocks = Pairs::default();
    blocks
        .p(0, "BLOCK")
        .p(8, "0")
        .p(2, "BLK")
        .p(70, 0)
        .p(10, 0)
        .p(20, 0)
        .p(30, 0);
    blocks.extend(&line(0x90, "0", (0.0, 0.0, 0.0), (1.0, 1.0, 0.0)));
    blocks.p(0, "ENDBLK").p(8, "0");
    let mut classes = Pairs::default();
    classes
        .p(0, "CLASS")
        .p(1, "ACDBDICTIONARYWDFLT")
        .p(2, "AcDbDictionaryWithDefault");
    let mut f = Pairs::default();
    f.extend(&section("HEADER", &header(insunits)));
    f.extend(&section("CLASSES", &classes));
    f.extend(&section("BLOCKS", &blocks));
    f.extend(&section("ENTITIES", &e));
    f.p(0, "EOF");
    f.text(true).into_bytes()
}

pub const SPL6: [(f64, f64); 6] = [
    (0.0, 300.0),
    (30.0, 380.0),
    (80.0, 260.0),
    (130.0, 360.0),
    (180.0, 280.0),
    (220.0, 340.0),
];

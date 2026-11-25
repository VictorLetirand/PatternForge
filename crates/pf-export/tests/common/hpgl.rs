//! Lecteur HPGL et référence géométrique indépendants de l'écrivain (US-12.5).
//!
//! Rien n'est partagé avec `pf_export::hpgl` : ni constantes (40 unités par mm écrit en dur ici),
//! ni table de plumes (réécrite ci-dessous), ni discrétisation. Le lecteur rejoue `IN`, `PA`, `SP`,
//! `PU`, `PD`, plus `SI` (taille de caractère) et `LB` (étiquette terminée par ETX) ; toute autre
//! instruction est refusée.
#![allow(dead_code)]

use pf_core::PathRole;
use pf_export::{PathGeom, Scene};
use pf_geometry::{CubicBezier, PointMm};

pub const UNITS: f64 = 40.0;

/// Un tracé rejoué : plume et sommets en unités.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub pen: u8,
    pub pts: Vec<(i64, i64)>,
}

impl Stroke {
    pub fn length_mm(&self) -> f64 {
        self.pts.windows(2).map(|w| dist(w[0], w[1])).sum::<f64>() / UNITS
    }
}

/// Une ligne d'étiquette relue : plume, début (unités), taille de caractère (cm) et texte.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub pen: u8,
    pub at: (i64, i64),
    pub size_cm: (f64, f64),
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Replay {
    pub labels: Vec<Label>,
    pub strokes: Vec<Stroke>,
    /// Numéros de plume dans l'ordre des `SP`.
    pub selections: Vec<u8>,
    /// Somme des déplacements plume levée (mm), depuis l'origine.
    pub pen_up_mm: f64,
    pub commands: usize,
}

pub fn dist(a: (i64, i64), b: (i64, i64)) -> f64 {
    let (dx, dy) = ((a.0 - b.0) as f64, (a.1 - b.1) as f64);
    dx.hypot(dy)
}

fn coords(params: &str) -> Result<Vec<(i64, i64)>, String> {
    if params.is_empty() {
        return Ok(Vec::new());
    }
    let n: Vec<i64> = params
        .split(',')
        .map(|v| v.trim().parse::<i64>().map_err(|e| format!("{v:?} : {e}")))
        .collect::<Result<_, _>>()?;
    if n.len() % 2 != 0 {
        return Err("nombre impair de coordonnées".into());
    }
    Ok(n.chunks(2).map(|c| (c[0], c[1])).collect())
}

pub fn read_hpgl(text: &str) -> Result<Replay, String> {
    if !text.is_ascii() {
        return Err("fichier non ASCII".into());
    }
    let mut r = Replay::default();
    let (mut pos, mut pen, mut down) = ((0i64, 0i64), 0u8, false);
    let mut current: Option<Stroke> = None;
    let mut ended = false;
    let mut size = (0.0, 0.0);
    for raw in text.split(';') {
        // Étiquette : tout ce qui suit `LB` jusqu'à ETX (0x03), espaces compris.
        if let Some(rest) = raw.trim_start().strip_prefix("LB") {
            let body = rest
                .strip_suffix('\u{3}')
                .ok_or("LB sans terminateur ETX")?;
            if ended || pen == 0 || size.0 <= 0.0 || down {
                return Err("LB hors contexte (plume, taille ou plume baissée)".into());
            }
            r.commands += 1;
            r.labels.push(Label {
                pen,
                at: pos,
                size_cm: size,
                text: body.to_string(),
            });
            continue;
        }
        let cmd: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
        if cmd.is_empty() {
            continue;
        }
        if ended {
            return Err(format!("instruction après la fin : {cmd}"));
        }
        r.commands += 1;
        let (name, params) = cmd.split_at(2.min(cmd.len()));
        match name {
            "IN" => {
                pos = (0, 0);
                pen = 0;
                down = false;
                current = None;
            }
            "PA" => {
                if !params.is_empty() {
                    return Err("PA avec paramètres non attendu".into());
                }
            }
            "SP" => {
                pen = params.parse().map_err(|e| format!("SP : {e}"))?;
                if pen > 9 {
                    return Err(format!("plume {pen} hors 0 à 9"));
                }
                if pen == 0 {
                    ended = true;
                } else {
                    r.selections.push(pen);
                }
                if let Some(s) = current.take() {
                    r.strokes.push(s);
                }
            }
            "PU" | "PD" => {
                let to_down = name == "PD";
                let pts = coords(params)?;
                if down != to_down {
                    if let Some(s) = current.take() {
                        r.strokes.push(s);
                    }
                }
                down = to_down;
                for p in pts {
                    if down {
                        if pen == 0 {
                            return Err("PD sans plume".into());
                        }
                        let s = current.get_or_insert_with(|| Stroke {
                            pen,
                            pts: vec![pos],
                        });
                        s.pts.push(p);
                    } else {
                        r.pen_up_mm += dist(pos, p) / UNITS;
                    }
                    pos = p;
                }
            }
            "SI" => {
                let v: Vec<f64> = params
                    .split(',')
                    .map(|n| n.parse::<f64>().map_err(|e| format!("SI : {e}")))
                    .collect::<Result<_, _>>()?;
                if v.len() != 2 {
                    return Err("SI attend largeur,hauteur".into());
                }
                size = (v[0], v[1]);
            }
            other => return Err(format!("instruction inconnue : {other}")),
        }
    }
    if let Some(s) = current.take() {
        r.strokes.push(s);
    }
    Ok(r)
}

// ----- référence : géométrie de la scène -------------------------------------------------------

/// Plume attendue d'un rôle (table écrite ici, indépendamment du code).
pub fn pen_of_role(role: PathRole) -> u8 {
    match role {
        PathRole::CuttingContour => 1,
        PathRole::SewingContour => 2,
        PathRole::Construction => 3,
        PathRole::Grainline => 4,
        _ => 8,
    }
}

pub type Poly = Vec<(f64, f64)>;

fn cubic_poly(c: &CubicBezier) -> Poly {
    let n = 4000;
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let f = |a: f64, b: f64, c_: f64, d: f64| {
                u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c_ + t * t * t * d
            };
            (
                f(c.p0.x, c.c1.x, c.c2.x, c.p3.x),
                f(c.p0.y, c.c1.y, c.c2.y, c.p3.y),
            )
        })
        .collect()
}

fn arc_poly(center: PointMm, r: f64, start_rad: f64, sweep_rad: f64) -> Poly {
    let n = 4000;
    (0..=n)
        .map(|i| {
            let a = start_rad + sweep_rad * i as f64 / n as f64;
            (center.x + r * a.cos(), center.y + r * a.sin())
        })
        .collect()
}

/// Polyligne fine d'un tracé de la scène.
pub fn geom_poly(g: &PathGeom) -> Poly {
    match g {
        PathGeom::Line(a, b) => vec![(a.x, a.y), (b.x, b.y)],
        PathGeom::Cubic(c) => cubic_poly(c),
        PathGeom::Arc(a) => arc_poly(a.center, a.radius_mm, a.start_rad, a.sweep_rad),
        PathGeom::Circle(c) => {
            let sign = if c.ccw { 1.0 } else { -1.0 };
            arc_poly(
                c.circle.center,
                c.circle.radius_mm,
                c.start_deg.to_radians(),
                sign * std::f64::consts::TAU,
            )
        }
        PathGeom::Spline(s) => s.iter().flat_map(cubic_poly).collect(),
    }
}

/// Traits attendus d'une scène : (plume, polyligne fine, courbe ?). Couvre les tracés (avec les
/// flèches de droit-fil), les crans, les croix et les cercles de marquage de type perçage, les
/// boutons ronds et les traits des cotations simples.
pub fn reference(scene: &Scene) -> Vec<(u8, Poly, bool)> {
    let mut out = Vec::new();
    let seg = |a: PointMm, b: PointMm| vec![(a.x, a.y), (b.x, b.y)];
    for p in &scene.paths {
        let pen = pen_of_role(p.role);
        let curved = !matches!(p.geom, PathGeom::Line(..));
        out.push((pen, geom_poly(&p.geom), curved));
        if let (PathRole::Grainline, &PathGeom::Line(a, b)) = (p.role, &p.geom) {
            for (q, r) in pf_export::grain_arrow_marks(a, b, p.sense) {
                out.push((pen, seg(q, r), false));
            }
        }
    }
    for (a, b) in &scene.notch_marks {
        out.push((5, seg(*a, *b), false));
    }
    for d in &scene.dimensions {
        for (a, b) in d.marks() {
            out.push((7, seg(a, b), false));
        }
    }
    for m in &scene.markings {
        for (a, b) in m.cross_marks() {
            out.push((6, seg(a, b), false));
        }
        if let Some((r, _)) = m.circle() {
            out.push((6, arc_poly(m.pos, r, 0.0, std::f64::consts::TAU), true));
        }
    }
    for b in &scene.buttons {
        if let Some(r) = b.circle_radius() {
            out.push((6, arc_poly(b.pos, r, 0.0, std::f64::consts::TAU), true));
        }
    }
    out
}

pub fn poly_len(p: &Poly) -> f64 {
    p.windows(2)
        .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
        .sum()
}

/// Boîte (min x, min y, max x, max y) de polylignes.
pub fn bbox_of<'a>(polys: impl Iterator<Item = &'a Poly>) -> Option<(f64, f64, f64, f64)> {
    let mut bb: Option<(f64, f64, f64, f64)> = None;
    for &(x, y) in polys.flatten() {
        bb = Some(match bb {
            None => (x, y, x, y),
            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
        });
    }
    bb
}

/// Segment non orienté (extrémités triées) et plume.
pub type Seg = ((i64, i64), (i64, i64), u8);

/// Segments non orientés d'une relecture, triés (pour comparer deux ordres de tracé).
pub fn segment_multiset(r: &Replay) -> Vec<Seg> {
    let mut v: Vec<_> = r
        .strokes
        .iter()
        .flat_map(|s| {
            s.pts.windows(2).map(|w| {
                let (a, b) = if w[0] <= w[1] {
                    (w[0], w[1])
                } else {
                    (w[1], w[0])
                };
                (a, b, s.pen)
            })
        })
        .collect();
    v.sort();
    v
}

/// Scène aléatoire (graine fixe) : tous les types de tracé et de rôle, crans, cotations,
/// marquages (perçages, croix), boutons ronds et points (non tracés). `spread` multiplie la taille
/// de la boîte de base (120 mm) ; un décalage aléatoire déplace le tout (coordonnées négatives
/// comprises).
pub fn random_scene(rng: &mut super::Rng, spread: f64) -> Scene {
    use pf_core::{ButtonShape, GrainSense, MarkingShape};
    use pf_export::{SceneButton, SceneDimension, SceneMarking, ScenePath, ScenePoint};
    use pf_geometry::{Arc, CircleTrace};
    let (ox, oy) = (rng.range(-200.0, 200.0), rng.range(-200.0, 200.0));
    let size = 120.0 * spread;
    let at =
        |rng: &mut super::Rng| PointMm::new(ox + rng.range(0.0, size), oy + rng.range(0.0, size));
    let far = |rng: &mut super::Rng, a: PointMm| loop {
        let b = PointMm::new(ox + rng.range(0.0, size), oy + rng.range(0.0, size));
        if (b.x - a.x).hypot(b.y - a.y) > 2.0 {
            return b;
        }
    };
    let mut paths = Vec::new();
    for _ in 0..1 + rng.below(12) {
        let role = rng.pick(&PathRole::ALL);
        let geom = match rng.below(5) {
            0 => {
                let a = at(rng);
                PathGeom::Line(a, far(rng, a))
            }
            1 => {
                let p0 = at(rng);
                PathGeom::Cubic(CubicBezier::new(p0, at(rng), at(rng), far(rng, p0)))
            }
            2 => PathGeom::Arc(Arc {
                center: at(rng),
                radius_mm: rng.range(3.0, 25.0) * spread,
                start_rad: rng.range(0.0, std::f64::consts::TAU),
                sweep_rad: rng.range(0.3, 5.5) * if rng.below(2) == 0 { 1.0 } else { -1.0 },
            }),
            3 => PathGeom::Circle(
                CircleTrace::new(
                    at(rng),
                    rng.range(3.0, 25.0) * spread,
                    rng.range(0.0, 360.0),
                    rng.below(2) == 0,
                )
                .unwrap(),
            ),
            _ => {
                let mut segs = Vec::new();
                let mut p0 = at(rng);
                for _ in 0..2 + rng.below(3) {
                    let p3 = far(rng, p0);
                    segs.push(CubicBezier::new(p0, at(rng), at(rng), p3));
                    p0 = p3;
                }
                PathGeom::Spline(segs)
            }
        };
        let sense = if role == PathRole::Grainline {
            rng.pick(&GrainSense::ALL)
        } else {
            GrainSense::Unmarked
        };
        paths.push(ScenePath { geom, role, sense });
    }
    let mut scene = super::scene_of(Vec::new());
    scene.paths = paths;
    for _ in 0..rng.below(4) {
        let a = at(rng);
        scene.notch_marks.push((a, far(rng, a)));
    }
    for _ in 0..rng.below(3) {
        let (from, to) = (at(rng), at(rng));
        scene.dimensions.push(SceneDimension {
            from,
            to,
            from_offset: PointMm::new(from.x + 5.0, from.y + 5.0),
            to_offset: PointMm::new(to.x + 5.0, to.y + 5.0),
            text: "12.00 mm".into(),
        });
    }
    for _ in 0..rng.below(4) {
        let shape = if rng.below(2) == 0 {
            MarkingShape::Drill {
                diameter_mm: rng.range(2.0, 8.0),
            }
        } else {
            MarkingShape::Cross {
                size_mm: rng.range(4.0, 10.0),
            }
        };
        scene.markings.push(SceneMarking {
            pos: at(rng),
            shape,
        });
    }
    for _ in 0..rng.below(3) {
        scene.buttons.push(SceneButton {
            pos: at(rng),
            shape: ButtonShape::Button {
                diameter_mm: rng.range(8.0, 20.0),
            },
        });
    }
    for i in 0..rng.below(4) {
        scene.points.push(ScenePoint {
            pos: at(rng),
            label: format!("P{i}"),
            show_label: true,
        });
    }
    scene
}

/// Virages cumulés (radians) d'une polyligne fine : sert à borner l'écart de corde des courbes.
fn turning(p: &Poly) -> f64 {
    let ang = |a: (f64, f64), b: (f64, f64)| (b.1 - a.1).atan2(b.0 - a.0);
    p.windows(3)
        .filter(|w| w[0] != w[1] && w[1] != w[2])
        .map(|w| {
            let d = (ang(w[1], w[2]) - ang(w[0], w[1])).rem_euclid(std::f64::consts::TAU);
            d.min(std::f64::consts::TAU - d)
        })
        .sum()
}

/// Relit `text` et le compare à la géométrie de la scène (mise à l'échelle `sx` / `sy`) :
/// nombre de tracés et longueur par plume, boîte englobante, calage à l'origine. Longueur : marge
/// par tracé (arrondi des sommets, corde des courbes ; voir plus bas), la somme sur les tracés
/// d'une plume ; boîte : 0,02 mm (0,03 mm avec une courbe). Renvoie la relecture.
pub fn check_against_scene(scene: &Scene, text: &str, sx: f64, sy: f64) -> Result<Replay, String> {
    let replay = read_hpgl(text)?;
    let refs: Vec<(u8, Poly, bool)> = reference(scene)
        .into_iter()
        .map(|(pen, poly, c)| {
            let scaled: Poly = poly.into_iter().map(|(x, y)| (x * sx, y * sy)).collect();
            (pen, scaled, c)
        })
        .collect();
    for pen in 1..=8u8 {
        let want: Vec<&(u8, Poly, bool)> = refs.iter().filter(|r| r.0 == pen).collect();
        let got: Vec<&Stroke> = replay.strokes.iter().filter(|s| s.pen == pen).collect();
        if want.len() != got.len() {
            return Err(format!(
                "plume {pen} : {} tracés attendus, {} relus",
                want.len(),
                got.len()
            ));
        }
        let want_len: f64 = want.iter().map(|r| poly_len(&r.1)).sum();
        let got_len: f64 = got.iter().map(|s| s.length_mm()).sum();
        // Marge par tracé : arrondi des deux extrémités (2 × 0,0125 × √2 < 0,036 mm), des sommets
        // intermédiaires (0,0178 mm par radian de virage) et corde des courbes (flèche 0,005 mm :
        // 2,1 × flèche par tour, soit 0,0017 mm par radian).
        let allowance: f64 = want
            .iter()
            .map(|r| 0.036 + if r.2 { 0.0195 * turning(&r.1) } else { 0.0 })
            .sum();
        if (want_len - got_len).abs() > allowance {
            return Err(format!(
                "plume {pen} : longueur {got_len} relue, {want_len} attendue (marge {allowance})"
            ));
        }
    }
    let curved = refs.iter().any(|r| r.2);
    let tol = if curved { 0.03 } else { 0.02 };
    if let Some((x0, y0, x1, y1)) = bbox_of(refs.iter().map(|r| &r.1)) {
        let pts: Vec<(i64, i64)> = replay.strokes.iter().flat_map(|s| s.pts.clone()).collect();
        let (gx0, gy0) = (
            pts.iter().map(|p| p.0).min().unwrap(),
            pts.iter().map(|p| p.1).min().unwrap(),
        );
        let (gx1, gy1) = (
            pts.iter().map(|p| p.0).max().unwrap(),
            pts.iter().map(|p| p.1).max().unwrap(),
        );
        if (gx0, gy0) != (0, 0) {
            return Err(format!("emprise non calée à l'origine : {gx0}, {gy0}"));
        }
        let (gw, gh) = ((gx1 - gx0) as f64 / UNITS, (gy1 - gy0) as f64 / UNITS);
        if (gw - (x1 - x0)).abs() > tol || (gh - (y1 - y0)).abs() > tol {
            return Err(format!(
                "emprise {gw} x {gh} relue, {} x {} attendue",
                x1 - x0,
                y1 - y0
            ));
        }
    }
    Ok(replay)
}

/// Déplacement à vide recalculé sur les tracés relus (origine → premier départ → ...).
pub fn travel_of(r: &Replay) -> f64 {
    let mut pos = (0, 0);
    let mut total = 0.0;
    for s in &r.strokes {
        total += dist(pos, s.pts[0]);
        pos = *s.pts.last().unwrap();
    }
    total / UNITS
}

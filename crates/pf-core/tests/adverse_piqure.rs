//! US-06.9 — campagne adverse des lignes de piqûre : 200 séquences à graine fixe sur des
//! rectangles à coins vifs (coins en pointe) ou arrondis (arcs tangents), avec chaînes de 1 à 3
//! tracés ou contour entier, pas / retrait / nombre / formes aléatoires, entrées invalides
//! (prédites par l'oracle), continuation, fusion, suivi du bord (déplacement d'un coin),
//! annulations, rétablissements et relecture JSON.
//!
//! Oracle **indépendant** : le trajet décalé est reconstruit ici d'après les paramètres du
//! rectangle (jamais lus dans le code testé) : segments décalés de d vers l'intérieur, coins vifs
//! en pointe (somme des deux normales), arcs de rayon r − d échantillonnés en 2000 points ;
//! abscisses par cumul des longueurs de la polyligne ; nombre de trous par ⌈L / pas⌉ (arrondi
//! à 1e-6 près) ; pas continu vérifié sur le trajet. Aucune fonction de `stitch_line` ni de
//! `pf_geometry::offset` n'y sert à juger. Tolérance annoncée : 2e-3 mm sur les positions.
//!
//! Propriétés : jamais de panique ; entrée invalide = refus, octets identiques, aucune étape ;
//! entrée valide = acceptée, trous conformes à l'oracle ; annuler / rétablir exacts ; relecture
//! JSON identique ; compteurs prouvant que les contrôles forts se sont exécutés.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

const SEQUENCES: usize = 200;
const ARC_SAMPLES: usize = 2000;
const POS_TOL: f64 = 2e-3;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    /// Multiple de 0,25 dans [lo ; hi].
    fn quarter(&mut self, lo: f64, hi: f64) -> f64 {
        let n = ((hi - lo) * 4.0).round() as usize;
        lo + self.below(n + 1) as f64 / 4.0
    }
}

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    accepted: usize,
    refused_predicted: usize,
    hole_checks: usize,
    sharp_corner_checks: usize,
    arc_checks: usize,
    closed_checks: usize,
    fixed_count_checks: usize,
    continuation_checks: usize,
    merge_checks: usize,
    follow_checks: usize,
    undo_checks: usize,
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

/// Un morceau du contour construit par l'oracle.
#[derive(Clone)]
enum Prim {
    Line(PointMm, PointMm),
    /// centre, rayon, angle de départ, angle d'arrivée (degrés, anti-horaire)
    Arc(PointMm, f64, f64, f64),
}

impl Prim {
    fn mid(&self) -> PointMm {
        match self {
            Prim::Line(a, b) => pt((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
            Prim::Arc(c, r, a0, a1) => {
                let t = ((a0 + a1) / 2.0).to_radians();
                pt(c.x + r * t.cos(), c.y + r * t.sin())
            }
        }
    }
}

struct Shape {
    w: f64,
    h: f64,
    r: f64,
    prims: Vec<Prim>,
}

/// Rectangle `w × h`, coins vifs si `r == 0`, arrondis de rayon `r` sinon ; morceaux dans l'ordre
/// anti-horaire à partir du bas.
fn shape(w: f64, h: f64, r: f64) -> Shape {
    let mut prims = Vec::new();
    if r == 0.0 {
        let c = [pt(0.0, 0.0), pt(w, 0.0), pt(w, h), pt(0.0, h)];
        for i in 0..4 {
            prims.push(Prim::Line(c[i], c[(i + 1) % 4]));
        }
    } else {
        prims.push(Prim::Line(pt(r, 0.0), pt(w - r, 0.0)));
        prims.push(Prim::Arc(pt(w - r, r), r, -90.0, 0.0));
        prims.push(Prim::Line(pt(w, r), pt(w, h - r)));
        prims.push(Prim::Arc(pt(w - r, h - r), r, 0.0, 90.0));
        prims.push(Prim::Line(pt(w - r, h), pt(r, h)));
        prims.push(Prim::Arc(pt(r, h - r), r, 90.0, 180.0));
        prims.push(Prim::Line(pt(0.0, h - r), pt(0.0, r)));
        prims.push(Prim::Arc(pt(r, r), r, 180.0, 270.0));
    }
    Shape { w, h, r, prims }
}

/// Trajet décalé de `d` des morceaux `from .. from + count` (cycliques), en polyligne.
fn path(sh: &Shape, from: usize, count: usize, d: f64) -> Vec<PointMm> {
    let n = sh.prims.len();
    let mut pts: Vec<PointMm> = Vec::new();
    let nl = |a: PointMm, b: PointMm| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l = dx.hypot(dy);
        (-dy / l, dx / l)
    };
    if sh.r == 0.0 {
        // Coins vifs : sommets décalés ; extrémités = décalage perpendiculaire.
        for k in 0..count {
            let Prim::Line(a, b) = sh.prims[(from + k) % n].clone() else {
                unreachable!()
            };
            let (nx, ny) = nl(a, b);
            if k == 0 {
                pts.push(pt(a.x + d * nx, a.y + d * ny));
            }
            if k + 1 < count {
                let Prim::Line(a2, b2) = sh.prims[(from + k + 1) % n].clone() else {
                    unreachable!()
                };
                let (mx, my) = nl(a2, b2);
                pts.push(pt(b.x + d * (nx + mx), b.y + d * (ny + my)));
            } else {
                pts.push(pt(b.x + d * nx, b.y + d * ny));
            }
        }
        return pts;
    }
    for k in 0..count {
        match sh.prims[(from + k) % n].clone() {
            Prim::Line(a, b) => {
                let (nx, ny) = nl(a, b);
                if k == 0 {
                    pts.push(pt(a.x + d * nx, a.y + d * ny));
                }
                pts.push(pt(b.x + d * nx, b.y + d * ny));
            }
            Prim::Arc(c, r, a0, a1) => {
                for i in 0..=ARC_SAMPLES {
                    if k > 0 && i == 0 {
                        continue;
                    }
                    let t = (a0 + (a1 - a0) * i as f64 / ARC_SAMPLES as f64).to_radians();
                    pts.push(pt(c.x + (r - d) * t.cos(), c.y + (r - d) * t.sin()));
                }
            }
        }
    }
    pts
}

fn length(p: &[PointMm]) -> f64 {
    p.windows(2)
        .map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y))
        .sum()
}

/// Point d'abscisse `s` sur la polyligne.
fn at(p: &[PointMm], s: f64) -> PointMm {
    let mut acc = 0.0;
    for w in p.windows(2) {
        let l = (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
        if acc + l >= s - 1e-12 {
            let t = if l > 0.0 {
                ((s - acc) / l).clamp(0.0, 1.0)
            } else {
                0.0
            };
            return pt(
                w[0].x + (w[1].x - w[0].x) * t,
                w[0].y + (w[1].y - w[0].y) * t,
            );
        }
        acc += l;
    }
    *p.last().unwrap()
}

/// ⌈q⌉ à 1e-6 près.
fn ceil_tol(q: f64) -> usize {
    let r = q.round();
    if (q - r).abs() < 1e-6 {
        r as usize
    } else {
        q.ceil() as usize
    }
}

/// Abscisse du point de la polyligne le plus proche de `q`, et distance.
fn project(p: &[PointMm], q: PointMm) -> (f64, f64) {
    let (mut best, mut best_s, mut acc) = (f64::MAX, 0.0, 0.0);
    for w in p.windows(2) {
        let (dx, dy) = (w[1].x - w[0].x, w[1].y - w[0].y);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 {
            (((q.x - w[0].x) * dx + (q.y - w[0].y) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let c = pt(w[0].x + dx * t, w[0].y + dy * t);
        let dist = (c.x - q.x).hypot(c.y - q.y);
        if dist < best {
            best = dist;
            best_s = acc + t * l2.sqrt();
        }
        acc += l2.sqrt();
    }
    (best_s, best)
}

fn mid_of(s: &pf_core::Shape) -> PointMm {
    let v = s.sample(2);
    if v.len() == 2 {
        pt((v[0].x + v[1].x) / 2.0, (v[0].y + v[1].y) / 2.0)
    } else {
        v[1]
    }
}

struct Built {
    p: Project,
    s: SheetId,
    piece: PieceId,
    /// entité du contour pour chaque morceau de l'oracle
    ent: Vec<EntityId>,
}

fn build(sh: &Shape) -> Built {
    let mut p = Project::new("Adverse piqure");
    let s = p.sheets[0].id;
    if sh.r == 0.0 {
        p.apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: sh.w,
            height_mm: sh.h,
        })
        .unwrap();
    } else {
        p.apply(Command::AddRoundedRectangle {
            sheet: s,
            rect: pf_geometry::RectSpec {
                origin: pt(0.0, 0.0),
                width_mm: sh.w,
                height_mm: sh.h,
                rotation_deg: 0.0,
                centered: false,
            },
            radius_mm: sh.r,
            hide_centers: true,
            role: PathRole::Construction,
            layer: None,
        })
        .unwrap();
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click {
                at: pt(sh.w / 2.0, sh.h / 2.0),
            },
            name: "Adverse".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    // Associe chaque morceau de l'oracle au tracé du contour dont le milieu est le plus proche.
    let sheet = p.sheet(s).unwrap();
    let outline: Vec<EntityId> = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| e.entity)
        .collect();
    assert_eq!(outline.len(), sh.prims.len(), "contour attendu");
    let ent = sh
        .prims
        .iter()
        .map(|pr| {
            let m = pr.mid();
            *outline
                .iter()
                .min_by(|a, b| {
                    let da = {
                        let q = mid_of(&sheet.shape(**a).unwrap());
                        (q.x - m.x).hypot(q.y - m.y)
                    };
                    let db = {
                        let q = mid_of(&sheet.shape(**b).unwrap());
                        (q.x - m.x).hypot(q.y - m.y)
                    };
                    da.total_cmp(&db)
                })
                .unwrap()
        })
        .collect();
    Built { p, s, piece, ent }
}

fn spec(edges: Vec<EntityId>, pitch: f64, setback: f64, count: Option<u32>) -> StitchLineSpec {
    StitchLineSpec {
        edges,
        hole: StitchHole::Round { diameter_mm: 1.2 },
        pitch_mm: pitch,
        setback_mm: setback,
        count,
        continues: None,
    }
}

/// Vérifie les trous d'une ligne ouverte contre l'oracle ; renvoie `(pas réel, dernier trou)`.
fn check_open(
    got: &StitchLayout,
    route: &[PointMm],
    start: f64,
    pitch: f64,
    count: Option<u32>,
    c: &mut Counters,
) -> (f64, PointMm) {
    let l = length(route);
    let span = l - start;
    let (n, step) = match count {
        Some(1) => (1, 0.0),
        Some(k) => (k as usize, span / (k as usize - 1) as f64),
        None => {
            let k = ceil_tol(span / pitch);
            (k + 1, span / k as f64)
        }
    };
    assert_eq!(got.holes.len(), n, "nombre de trous");
    assert!(
        (got.length_mm - l).abs() < POS_TOL,
        "longueur {} / {l}",
        got.length_mm
    );
    for (i, h) in got.holes.iter().enumerate() {
        let s = if n == 1 {
            start + span / 2.0
        } else {
            start + i as f64 * step
        };
        let want = at(route, s.min(l));
        assert!(
            (h.position.x - want.x).hypot(h.position.y - want.y) < POS_TOL,
            "trou {i} : {:?} / {want:?}",
            h.position
        );
        c.hole_checks += 1;
    }
    (step, got.holes.last().unwrap().position)
}

fn one_sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed);
    let rounded = rng.chance(50);
    let w = rng.quarter(60.0, 90.0);
    let h = rng.quarter(40.0, 60.0);
    let r = if rounded { rng.quarter(8.0, 14.0) } else { 0.0 };
    let sh = shape(w, h, r);
    let mut b = build(&sh);
    let n = sh.prims.len();
    let dmax = if rounded { r - 1.0 } else { 0.4 * h.min(w) };
    let d = rng.quarter(1.0, dmax.min(12.0));
    let pitch = rng.quarter(2.0, 20.0);
    // Chaîne de 1 à n−1 morceaux (consécutifs) ou contour entier.
    let whole = rng.chance(15);
    let (from, count) = if whole {
        (0, n)
    } else {
        (rng.below(n), 1 + rng.below(if rounded { 5 } else { 3 }))
    };
    // Les entités doivent suivre l'ordre du contour du modèle : on part du morceau « from » de
    // l'oracle et on lit l'ordre du contour dans le modèle.
    let outline: Vec<EntityId> =
        b.p.piece(b.piece)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .collect();
    let first_pos = outline.iter().position(|e| *e == b.ent[from]).unwrap();
    let ordered: Vec<EntityId> = (0..count).map(|k| outline[(first_pos + k) % n]).collect();
    // Le sens du contour du modèle est anti-horaire comme l'oracle : l'ordre est le même.
    assert_eq!(ordered[0], b.ent[from]);
    assert_eq!(ordered.last(), Some(&b.ent[(from + count - 1) % n]));

    // --- entrée invalide : prédite par l'oracle -----------------------------------------
    let invalid = match rng.below(6) {
        0 => Some(spec(ordered.clone(), 0.1, d, None)),
        1 => Some(spec(ordered.clone(), 500.0, d, None)),
        2 => Some(spec(ordered.clone(), pitch, 0.1, None)),
        3 => Some(spec(ordered.clone(), pitch, 100.0 + 1.0, None)),
        4 => Some(spec(ordered.clone(), pitch, d, Some(0))),
        // Retrait qui sort de la pièce : plus grand que toute dimension, encore dans les bornes.
        _ => Some(spec(
            ordered.clone(),
            pitch,
            (w.max(h) + 5.0).min(100.0),
            None,
        )),
    };
    if let Some(s) = invalid {
        let before = bytes(&b.p);
        let steps = b.p.clone();
        let res = catch_unwind(AssertUnwindSafe(|| {
            b.p.apply(Command::AddStitchLine {
                piece: b.piece,
                line: s,
            })
        }));
        assert!(
            res.expect("pas de panique").is_err(),
            "seed {seed} : refus attendu"
        );
        assert_eq!(before, bytes(&b.p), "refus sans trace");
        assert_eq!(steps, b.p);
        c.refused_predicted += 1;
    }

    // --- entrée valide -------------------------------------------------------------------
    let fixed = (!whole && rng.chance(25)).then(|| 1 + rng.below(12) as u32);
    let mut doc = Document::new(b.p.clone());
    let original = bytes(doc.project());
    let hole = match rng.below(4) {
        0 => StitchHole::Round { diameter_mm: 1.2 },
        1 => StitchHole::Flat {
            length_mm: 3.0,
            width_mm: 0.8,
        },
        2 => StitchHole::French {
            length_mm: 3.5,
            width_mm: 1.5,
        },
        _ => StitchHole::Japanese {
            length_mm: 3.0,
            width_mm: 0.9,
            slant_deg: 45.0,
        },
    };
    let mut line = spec(ordered.clone(), pitch, d, fixed);
    line.hole = hole;
    let id = doc
        .execute(Command::AddStitchLine {
            piece: b.piece,
            line: line.clone(),
        })
        .unwrap_or_else(|e| panic!("seed {seed} : {e}"))
        .stitch_lines[0];
    c.accepted += 1;
    let layout = doc.project().stitch_line_layout(b.piece, id).unwrap();
    let route = if whole && !rounded {
        // Contour entier à coins vifs : rectangle réduit de d, fermé.
        vec![
            pt(d, d),
            pt(w - d, d),
            pt(w - d, h - d),
            pt(d, h - d),
            pt(d, d),
        ]
    } else {
        path(&sh, from, count, d)
    };
    if whole {
        // Contour entier : n trous espacés de L / n, tous sur le trajet, pas continu.
        let l = length(&route);
        let k = ceil_tol(l / pitch).max(1);
        assert_eq!(layout.holes.len(), k);
        assert!(layout.closed);
        let step = l / k as f64;
        let mut ss = Vec::new();
        for h in &layout.holes {
            let (s, dist) = project(&route, h.position);
            assert!(dist < POS_TOL, "seed {seed} : trou hors du trajet ({dist})");
            ss.push(s);
        }
        for i in 0..k {
            let mut delta = ss[(i + 1) % k] - ss[i];
            if delta < 0.0 {
                delta += l;
            }
            if k > 1 {
                assert!(
                    (delta - step).abs() < 5.0 * POS_TOL,
                    "seed {seed} : pas {delta}/{step}"
                );
            }
        }
        c.closed_checks += 1;
    } else {
        check_open(&layout, &route, 0.0, pitch, fixed, c);
        if fixed.is_some() {
            c.fixed_count_checks += 1;
        }
        if !rounded && count >= 2 {
            c.sharp_corner_checks += 1;
        }
        if rounded && count >= 2 {
            c.arc_checks += 1;
        }
    }
    // Les trous sont dans la pièce (rectangle) et tournés comme le bord (vérif. grossière).
    for h in &layout.holes {
        assert!(
            h.position.x > -1e-6
                && h.position.x < w + 1e-6
                && h.position.y > -1e-6
                && h.position.y < sh.h + 1e-6
        );
    }
    // Aperçu = création.
    let preview = b.p.stitch_line_preview(b.piece, &line).unwrap();
    assert_eq!(preview.holes.len(), layout.holes.len());

    // --- annuler / rétablir / JSON --------------------------------------------------------
    let done = bytes(doc.project());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), original);
    assert!(!doc.can_undo(), "une seule étape");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), done);
    let back = persist::from_json(&done).unwrap();
    assert_eq!(&back, doc.project());
    assert_eq!(
        back.stitch_line_layout(b.piece, id).unwrap(),
        doc.project().stitch_line_layout(b.piece, id).unwrap()
    );
    c.undo_checks += 1;
    b.p = doc.project().clone();

    // --- continuation et fusion sur deux morceaux adjacents (lignes droites uniquement) ----
    if !whole && !rounded {
        let i = rng.below(4);
        let (ea, eb) = (b.ent[i], b.ent[(i + 1) % 4]);
        let mut p2 = Project::new("x");
        std::mem::swap(&mut p2, &mut b.p);
        // retire la ligne précédente pour repartir d'un état simple
        p2.apply(Command::RemoveStitchLine {
            piece: b.piece,
            line: id,
        })
        .unwrap();
        let a = p2
            .apply(Command::AddStitchLine {
                piece: b.piece,
                line: spec(vec![ea], pitch, d, None),
            })
            .unwrap()
            .stitch_lines[0];
        let mut cont = spec(vec![eb], pitch, d, None);
        cont.continues = Some(a);
        match p2.apply(Command::AddStitchLine {
            piece: b.piece,
            line: cont,
        }) {
            Ok(o) => {
                let la = p2.stitch_line_layout(b.piece, a).unwrap();
                let lb = p2.stitch_line_layout(b.piece, o.stitch_lines[0]).unwrap();
                let route_a = path(&sh, i, 1, d);
                let route_b = path(&sh, (i + 1) % 4, 1, d);
                let (step, last) = check_open(&la, &route_a, 0.0, pitch, None, c);
                let gap = (last.x - route_b[0].x).hypot(last.y - route_b[0].y);
                let start = (step - gap).max(0.0);
                check_open(&lb, &route_b, start, pitch, None, c);
                c.continuation_checks += 1;
            }
            // Seul refus admis : plus rien à continuer (ligne B plus courte que le décalage).
            Err(e) => assert!(e.to_string().contains("continuer"), "seed {seed} : {e}"),
        }
        // Fusion : une ligne indépendante sur chaque morceau puis fusion = chaîne de deux.
        let c1 = p2
            .apply(Command::AddStitchLine {
                piece: b.piece,
                line: spec(vec![ea], pitch, d, None),
            })
            .unwrap()
            .stitch_lines[0];
        let c2 = p2
            .apply(Command::AddStitchLine {
                piece: b.piece,
                line: spec(vec![eb], pitch, d, None),
            })
            .unwrap()
            .stitch_lines[0];
        p2.apply(Command::MergeStitchLines {
            piece: b.piece,
            first: c1,
            second: c2,
        })
        .unwrap();
        let lm = p2.stitch_line_layout(b.piece, c1).unwrap();
        check_open(&lm, &path(&sh, i, 2, d), 0.0, pitch, None, c);
        c.merge_checks += 1;
        b.p = p2;
    }

    // --- suivi du bord : déplacement du coin haut-droit (rectangle à coins vifs) -----------
    if !rounded && !whole {
        // Projet neuf : les lignes posées plus haut passent aussi par le coin déplacé.
        let fresh = build(&sh);
        let (mut p3, right) = (fresh.p, fresh.ent[1]);
        let sheet = p3.sheet(b.s).unwrap();
        let corner = sheet
            .points
            .iter()
            .find(|q| {
                sheet
                    .position(q.id)
                    .is_ok_and(|a| (a.x - w).abs() < 1e-9 && (a.y - h).abs() < 1e-9)
            })
            .unwrap()
            .id;
        // Ligne sur le bord droit seul.
        let rid = p3
            .apply(Command::AddStitchLine {
                piece: b.piece,
                line: spec(vec![right], pitch, d, None),
            })
            .unwrap()
            .stitch_lines[0];
        let h2 = rng.quarter(h * 0.5, h - 1.0);
        let before = bytes(&p3);
        p3.apply(Command::MovePoint {
            sheet: b.s,
            point: corner,
            to: pt(w, h2),
        })
        .unwrap_or_else(|e| panic!("seed {seed} : {e}"));
        // x = w − d, de y = 0 à y = h2 (bord droit seul : extrémités perpendiculaires).
        let route = vec![pt(w - d, 0.0), pt(w - d, h2)];
        let l = p3.stitch_line_layout(b.piece, rid).unwrap();
        check_open(&l, &route, 0.0, pitch, None, c);
        assert_ne!(before, bytes(&p3));
        // Au-dessus du coin d'origine, le dernier trou sortirait de la pièce : refus sans trace.
        let kept = bytes(&p3);
        let up = p3.apply(Command::MovePoint {
            sheet: b.s,
            point: corner,
            to: pt(w, h + 15.0),
        });
        assert!(
            up.is_err(),
            "seed {seed} : déplacement hors de la pièce accepté"
        );
        assert_eq!(kept, bytes(&p3));
        c.follow_checks += 1;
    }
    c.sequences += 1;
}

#[test]
fn campagne_adverse_lignes_de_piqure() {
    let mut c = Counters::default();
    for i in 0..SEQUENCES {
        one_sequence(0x5714_0000 + i as u64 * 7919, &mut c);
    }
    eprintln!("{c:#?}");
    assert_eq!(c.sequences, SEQUENCES);
    assert_eq!(c.accepted, SEQUENCES);
    assert!(
        c.refused_predicted >= SEQUENCES,
        "refus prédits : {}",
        c.refused_predicted
    );
    assert!(
        c.hole_checks >= 2000,
        "positions vérifiées : {}",
        c.hole_checks
    );
    assert!(
        c.sharp_corner_checks >= 20,
        "coins vifs : {}",
        c.sharp_corner_checks
    );
    assert!(c.arc_checks >= 20, "arcs : {}", c.arc_checks);
    assert!(
        c.closed_checks >= 15,
        "contours entiers : {}",
        c.closed_checks
    );
    assert!(
        c.fixed_count_checks >= 10,
        "nombre fixé : {}",
        c.fixed_count_checks
    );
    assert!(
        c.continuation_checks >= 10,
        "continuations : {}",
        c.continuation_checks
    );
    assert!(c.merge_checks >= 20, "fusions : {}", c.merge_checks);
    assert!(c.follow_checks >= 20, "suivi du bord : {}", c.follow_checks);
    assert_eq!(c.undo_checks, SEQUENCES);
}

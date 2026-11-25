//! US-06.10 — campagne adverse des zones de parage, de collage et de découpe interne : 200
//! séquences à graine fixe sur un rectangle (dimensions et origine aléatoires, multiples de 0,25).
//! Chaque séquence mêle : bandes de 1, 2 ou 3 bords consécutifs (ou ordre faux, ou contour
//! entier), largeurs valides et invalides, polygones libres (dedans, dehors, croisés, alignés,
//! trop courts, coordonnée non finie), translation du rectangle, étirement (largeur ou hauteur,
//! en un `Batch`), remplacement et retrait d'une zone, puis annulations / rétablissements et
//! relecture JSON.
//!
//! Oracle **indépendant** : les coins, normales intérieures et sommets d'une bande sont
//! reconstruits ici d'après les dimensions (bord décalé de w vers l'intérieur, coins vifs =
//! somme des deux normales) ; l'aire est la formule des lacets écrite dans ce fichier ; la
//! validité d'une bande est prédite par des inégalités sur les dimensions (largeur < dimension
//! perpendiculaire, < moitié de la dimension pour trois bords) ; celle d'un polygone par un test
//! de simplicité en arithmétique exacte (multiples de 0,25) et l'appartenance au rectangle. Aucune
//! fonction de `piece_zone` ni de `pf_geometry::{offset, polygon, hatch}` n'y sert à juger. Les
//! cas à la frontière exacte (largeur égale à la dimension) sont écartés avant l'essai.
//!
//! Propriétés : jamais de panique ; prédit refusé = refus, octets identiques, aucune étape ;
//! prédit accepté = accepté, surface conforme à l'oracle (sommets, aire, largeur lue à 0,005 mm) ;
//! la bande suit le bord (translation, étirement) ; annuler / rétablir exacts ; relecture JSON
//! identique ; compteurs prouvant que les contrôles forts se sont exécutés.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Transform};
use std::panic::{catch_unwind, AssertUnwindSafe};

const SEQUENCES: usize = 200;
const OPS: usize = 10;

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
    band1: usize,
    band2: usize,
    band3: usize,
    poly_accepted: usize,
    poly_refused: usize,
    vertex_checks: usize,
    width_checks: usize,
    area_checks: usize,
    follow_checks: usize,
    follow_refused: usize,
    set_checks: usize,
    remove_checks: usize,
    undo_checks: usize,
    json_checks: usize,
    bad_order_refused: usize,
    whole_refused: usize,
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

// ----- Modèle de l'oracle ---------------------------------------------------------------------

#[derive(Clone, Debug)]
enum OShape {
    Band { start: usize, n: usize, width: f64 },
    Poly(Vec<(f64, f64)>),
}

#[derive(Clone, Debug)]
struct OZone {
    id: ZoneId,
    kind: ZoneKind,
    shape: OShape,
}

#[derive(Clone, Debug)]
struct Model {
    w: f64,
    h: f64,
    ox: f64,
    oy: f64,
    zones: Vec<OZone>,
}

/// Normales intérieures des bords c_i → c_{i+1} d'un rectangle anti-horaire.
const NORMALS: [(f64, f64); 4] = [(0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)];

impl Model {
    fn corners(&self) -> [(f64, f64); 4] {
        let (x0, y0, x1, y1) = (self.ox, self.oy, self.ox + self.w, self.oy + self.h);
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }
    /// Longueur du bord i.
    fn len(&self, i: usize) -> f64 {
        if i % 2 == 0 {
            self.w
        } else {
            self.h
        }
    }
    fn inside(&self, p: (f64, f64)) -> bool {
        p.0 >= self.ox && p.0 <= self.ox + self.w && p.1 >= self.oy && p.1 <= self.oy + self.h
    }
}

/// Prédiction de la validité d'une bande : `None` = frontière exacte (écartée avant l'essai).
fn band_ok(m: &Model, start: usize, n: usize, w: f64) -> Option<bool> {
    if !w.is_finite() || !(0.5..=100.0).contains(&w) {
        return Some(false);
    }
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
    match n {
        1 => {
            let d = m.len(start + 1);
            if near(w, d) {
                None
            } else {
                Some(w < d)
            }
        }
        2 => {
            let d = m.w.min(m.h);
            if near(w, d) {
                None
            } else {
                Some(w < d)
            }
        }
        3 => {
            let (mid, a) = (m.len(start + 1), m.len(start));
            if near(w, mid / 2.0) || near(w, a) {
                None
            } else {
                Some(w < mid / 2.0 && w < a)
            }
        }
        _ => Some(false),
    }
}

/// Sommets attendus d'une bande : bord(s) puis trajet décalé à l'envers.
fn band_vertices(m: &Model, start: usize, n: usize, w: f64) -> Vec<(f64, f64)> {
    let c = m.corners();
    let at = |k: usize| c[(start + k) % 4];
    let nr = |k: usize| NORMALS[(start + k) % 4];
    let mut v: Vec<(f64, f64)> = (0..=n).map(at).collect();
    let mut q: Vec<(f64, f64)> = Vec::new();
    q.push((at(0).0 + w * nr(0).0, at(0).1 + w * nr(0).1));
    for k in 1..n {
        q.push((
            at(k).0 + w * (nr(k - 1).0 + nr(k).0),
            at(k).1 + w * (nr(k - 1).1 + nr(k).1),
        ));
    }
    q.push((at(n).0 + w * nr(n - 1).0, at(n).1 + w * nr(n - 1).1));
    v.extend(q.into_iter().rev());
    v
}

/// Aire par la formule des lacets.
fn shoelace(v: &[(f64, f64)]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

fn orient(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

fn on_seg(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> bool {
    orient(a, b, p) == 0.0
        && p.0 >= a.0.min(b.0)
        && p.0 <= a.0.max(b.0)
        && p.1 >= a.1.min(b.1)
        && p.1 <= a.1.max(b.1)
}

/// Les segments fermés [a, b] et [c, d] ont-ils un point commun ?
fn seg_meet(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    on_seg(c, a, b) || on_seg(d, a, b) || on_seg(a, c, d) || on_seg(b, c, d)
}

/// Polygone fermé simple : aucun sommet consécutif confondu, aucun retour en arrière, aucun
/// couple de côtés non voisins qui se rencontre.
fn oracle_simple(v: &[(f64, f64)]) -> bool {
    let n = v.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        let (a, b, c) = (v[i], v[(i + 1) % n], v[(i + 2) % n]);
        if a == b {
            return false;
        }
        // a et c du même côté de b sur une même droite : retour en arrière.
        if orient(a, b, c) == 0.0 && (a.0 - b.0) * (c.0 - b.0) + (a.1 - b.1) * (c.1 - b.1) > 0.0 {
            return false;
        }
    }
    for i in 0..n {
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if seg_meet(v[i], v[(i + 1) % n], v[j], v[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

fn poly_ok(m: &Model, pts: &[(f64, f64)]) -> bool {
    pts.len() >= 3
        && pts.iter().all(|p| p.0.is_finite() && p.1.is_finite())
        && pts.iter().all(|p| m.inside(*p))
        && oracle_simple(pts)
        && shoelace(pts) >= 0.1
}

fn same_set(a: &[PointMm], b: &[(f64, f64)]) -> bool {
    a.len() == b.len()
        && a.iter().all(|p| {
            b.iter()
                .any(|q| (p.x - q.0).abs() < 1e-9 && (p.y - q.1).abs() < 1e-9)
        })
        && b.iter().all(|q| {
            a.iter()
                .any(|p| (p.x - q.0).abs() < 1e-9 && (p.y - q.1).abs() < 1e-9)
        })
}

/// Distance d'un point à la droite portant le bord i.
fn dist_to_edge_line(m: &Model, i: usize, p: PointMm) -> f64 {
    let c = m.corners();
    let (a, b) = (c[i % 4], c[(i + 1) % 4]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    ((p.x - a.0) * dy - (p.y - a.1) * dx).abs() / dx.hypot(dy)
}

// ----- Projet de départ ------------------------------------------------------------------------

struct Fixture {
    doc: Document,
    sheet: SheetId,
    piece: PieceId,
    edges: [EntityId; 4],
    /// Position de chaque bord dans le contour de la pièce.
    pos: [usize; 4],
    points: Vec<PointId>,
}

fn find_edge(p: &Project, s: SheetId, a: (f64, f64), b: (f64, f64)) -> EntityId {
    let sheet = p.sheet(s).unwrap();
    let near = |q: PointMm, t: (f64, f64)| (q.x - t.0).abs() < 1e-9 && (q.y - t.1).abs() < 1e-9;
    sheet
        .entities
        .iter()
        .find(|e| {
            sheet
                .segment_ends(e.id)
                .is_ok_and(|(x, y)| (near(x, a) && near(y, b)) || (near(x, b) && near(y, a)))
        })
        .expect("bord")
        .id
}

fn fixture(m: &Model) -> Fixture {
    let mut p = Project::new("Adverse zones");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(m.ox, m.oy)),
        width_mm: m.w,
        height_mm: m.h,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click {
                at: pt(m.ox + m.w / 2.0, m.oy + m.h / 2.0),
            },
            name: "Rect".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let c = m.corners();
    let edges = [
        find_edge(&p, s, c[0], c[1]),
        find_edge(&p, s, c[1], c[2]),
        find_edge(&p, s, c[2], c[3]),
        find_edge(&p, s, c[3], c[0]),
    ];
    let outline = &p.piece(piece).unwrap().outline;
    let pos = edges.map(|e| outline.iter().position(|o| o.entity == e).unwrap());
    let points = p.sheet(s).unwrap().points.iter().map(|q| q.id).collect();
    Fixture {
        doc: Document::new(p),
        sheet: s,
        piece,
        edges,
        pos,
        points,
    }
}

fn point_id_at(f: &Fixture, x: f64, y: f64) -> PointId {
    let sheet = f.doc.project().sheet(f.sheet).unwrap();
    f.points
        .iter()
        .copied()
        .find(|id| {
            sheet
                .position(*id)
                .is_ok_and(|q| (q.x - x).abs() < 1e-9 && (q.y - y).abs() < 1e-9)
        })
        .expect("coin")
}

// ----- Vérification de l'état : zones du projet contre l'oracle --------------------------------

fn check_state(f: &Fixture, m: &Model, ctr: &mut Counters) {
    let project = f.doc.project();
    let stored = &project.piece(f.piece).unwrap().zones;
    assert_eq!(stored.len(), m.zones.len(), "nombre de zones");
    for oz in &m.zones {
        let z = project
            .zone_layout(f.piece, oz.id)
            .unwrap_or_else(|e| panic!("zone {:?} illisible : {e} ({oz:?}, {m:?})", oz.id));
        assert_eq!(z.kind, oz.kind);
        match &oz.shape {
            OShape::Band { start, n, width } => {
                let want = band_vertices(m, *start, *n, *width);
                assert!(
                    same_set(&z.polygon, &want),
                    "sommets {:?} attendus {want:?} ({m:?})",
                    z.polygon
                );
                ctr.vertex_checks += 1;
                assert!((z.area_mm2 - shoelace(&want)).abs() < 1e-6);
                // Formules d'aire indépendantes (cohérence de l'oracle lui-même).
                let (l1, l2) = (m.len(*start), m.len(*start + 1));
                let formula = match n {
                    1 => l1 * width,
                    2 => l1 * width + l2 * width - width * width,
                    _ => 2.0 * l1 * width + l2 * width - 2.0 * width * width,
                };
                assert!((shoelace(&want) - formula).abs() < 1e-6, "{n} {m:?}");
                ctr.area_checks += 1;
                assert_eq!(z.width_mm, Some(*width));
                // Largeur lue sur le plan : tout sommet qui n'est pas sur le bord est à `width`
                // (à 0,005 mm) d'au moins une droite de bord de la bande et pas plus près.
                let c = m.corners();
                for v in &z.polygon {
                    let on_chain = (0..*n).any(|k| {
                        let (a, b) = (c[(start + k) % 4], c[(start + k + 1) % 4]);
                        on_seg((v.x, v.y), a, b)
                    });
                    if on_chain {
                        continue;
                    }
                    let d = (0..*n)
                        .map(|k| dist_to_edge_line(m, start + k, *v))
                        .fold(f64::INFINITY, f64::min);
                    assert!((d - width).abs() < 0.005, "largeur lue {d} pour {width}");
                    ctr.width_checks += 1;
                }
            }
            OShape::Poly(pts) => {
                assert!(same_set(&z.polygon, pts));
                assert!((z.area_mm2 - shoelace(pts)).abs() < 1e-6);
                assert_eq!(z.width_mm, None);
                ctr.area_checks += 1;
            }
        }
    }
}

// ----- Une séquence ----------------------------------------------------------------------------

fn random_width(rng: &mut Rng) -> f64 {
    if rng.chance(12) {
        [0.0, 0.25, 0.4, -1.0, 100.25, 150.0, f64::NAN][rng.below(7)]
    } else if rng.chance(70) {
        rng.quarter(0.5, 45.0)
    } else {
        rng.quarter(0.5, 100.0)
    }
}

fn random_kind(rng: &mut Rng) -> ZoneKind {
    ZoneKind::ALL[rng.below(3)]
}

fn random_poly(rng: &mut Rng, m: &Model) -> Vec<(f64, f64)> {
    let n = [1, 2, 3, 3, 3, 4, 4, 4, 5][rng.below(9)];
    let mut pts: Vec<(f64, f64)> = (0..n)
        .map(|_| {
            if rng.chance(85) {
                (rng.quarter(m.ox, m.ox + m.w), rng.quarter(m.oy, m.oy + m.h))
            } else {
                (
                    rng.quarter(m.ox - 8.0, m.ox + m.w + 8.0),
                    rng.quarter(m.oy - 8.0, m.oy + m.h + 8.0),
                )
            }
        })
        .collect();
    if rng.chance(8) && !pts.is_empty() {
        let i = rng.below(pts.len());
        pts[i].0 = f64::NAN;
    }
    if rng.chance(8) && pts.len() >= 3 {
        // Sommet répété : alignement ou retour en arrière.
        pts[2] = pts[0];
    }
    if pts.len() == 5 && rng.chance(60) {
        // Pincement : un sommet posé au milieu d'un côté non voisin (contact sans croisement).
        pts[3] = ((pts[0].0 + pts[1].0) / 2.0, (pts[0].1 + pts[1].1) / 2.0);
    }
    pts
}

fn zone_spec_poly(kind: ZoneKind, pts: &[(f64, f64)]) -> ZoneSpec {
    ZoneSpec {
        kind,
        shape: ZoneShape::Polygon {
            points: pts.iter().map(|&(x, y)| pt(x, y)).collect(),
        },
    }
}

/// Choisit une bande : (début, nombre de bords, ordre donné à la commande, largeur, prédiction).
struct BandPick {
    start: usize,
    n: usize,
    ids: Vec<EntityId>,
    order_ok: bool,
    w: f64,
}

fn pick_band(rng: &mut Rng, f: &Fixture, m: &Model) -> Option<BandPick> {
    let start = rng.below(4);
    let n = if rng.chance(3) {
        4
    } else {
        [1, 1, 2, 2, 3][rng.below(5)]
    };
    let w = random_width(rng);
    if n < 4 && band_ok(m, start, n, w).is_none() {
        return None;
    }
    let mut ids: Vec<EntityId> = (0..n).map(|k| f.edges[(start + k) % 4]).collect();
    // Le contour peut être horaire : l'ordre qui suit le contour est celui des positions.
    let follows = |ids: &[EntityId]| {
        let pos = |e: EntityId| f.pos[f.edges.iter().position(|x| *x == e).unwrap()];
        ids.windows(2).all(|p| pos(p[1]) == (pos(p[0]) + 1) % 4)
    };
    if !follows(&ids) {
        ids.reverse();
    }
    if n >= 2 && rng.chance(10) {
        ids.reverse();
    }
    let order_ok = follows(&ids);
    Some(BandPick {
        start,
        n,
        ids,
        order_ok,
        w,
    })
}

type OnAccept = Box<dyn FnOnce(&mut Model, &Outcome)>;

fn run_sequence(seed: u64, ctr: &mut Counters) {
    let mut rng = Rng(seed);
    let mut m = Model {
        w: rng.quarter(60.0, 160.0),
        h: rng.quarter(40.0, 120.0),
        ox: rng.quarter(-20.0, 20.0),
        oy: rng.quarter(-20.0, 20.0),
        zones: Vec::new(),
    };
    let mut f = fixture(&m);
    let original = bytes(f.doc.project());
    let mut steps = 0usize;
    ctr.sequences += 1;
    for _ in 0..OPS {
        let before = bytes(f.doc.project());
        let before_p = f.doc.project().clone();
        let op = rng.below(100);
        // Résultat : Some(prédiction acceptée) ; `apply` est exécutée par la branche.
        let (cmd, predicted, on_accept): (Command, bool, OnAccept) = if op < 38 {
            let Some(b) = pick_band(&mut rng, &f, &m) else {
                continue;
            };
            let kind = random_kind(&mut rng);
            let ok = b.n < 4 && b.order_ok && band_ok(&m, b.start, b.n, b.w) == Some(true);
            if b.n == 4 {
                ctr.whole_refused += 1;
            } else if !b.order_ok {
                ctr.bad_order_refused += 1;
            }
            let (start, n, w) = (b.start, b.n, b.w);
            (
                Command::AddPieceZone {
                    piece: f.piece,
                    zone: ZoneSpec {
                        kind,
                        shape: ZoneShape::Band {
                            edges: b.ids,
                            width_mm: b.w,
                        },
                    },
                },
                ok,
                Box::new(move |m: &mut Model, o: &Outcome| {
                    m.zones.push(OZone {
                        id: o.zones[0],
                        kind,
                        shape: OShape::Band { start, n, width: w },
                    });
                }),
            )
        } else if op < 60 {
            let pts = random_poly(&mut rng, &m);
            let kind = random_kind(&mut rng);
            let ok = poly_ok(&m, &pts);
            let spec = zone_spec_poly(kind, &pts);
            (
                Command::AddPieceZone {
                    piece: f.piece,
                    zone: spec,
                },
                ok,
                Box::new(move |m: &mut Model, o: &Outcome| {
                    m.zones.push(OZone {
                        id: o.zones[0],
                        kind,
                        shape: OShape::Poly(pts),
                    });
                }),
            )
        } else if op < 70 {
            // Translation du rectangle : les bandes suivent, les polygones restent en place.
            let (dx, dy) = (rng.quarter(-40.0, 40.0), rng.quarter(-40.0, 40.0));
            let moved = Model {
                ox: m.ox + dx,
                oy: m.oy + dy,
                ..m.clone()
            };
            let ok = m.zones.iter().all(|z| match &z.shape {
                OShape::Poly(p) => p.iter().all(|q| moved.inside(*q)),
                OShape::Band { .. } => true,
            });
            if !m.zones.is_empty() {
                ctr.follow_checks += 1;
            }
            (
                Command::Transform {
                    sheet: f.sheet,
                    points: f.points.clone(),
                    transform: Transform::Translate {
                        dx_mm: dx,
                        dy_mm: dy,
                    },
                },
                ok,
                Box::new(move |m: &mut Model, _: &Outcome| {
                    m.ox += dx;
                    m.oy += dy;
                }),
            )
        } else if op < 82 {
            // Étirement : largeur ou hauteur, deux déplacements de coin en un Batch.
            let widen = rng.chance(50);
            let target = if widen {
                rng.quarter(30.0, 180.0)
            } else {
                rng.quarter(30.0, 140.0)
            };
            let moved = if widen {
                Model {
                    w: target,
                    ..m.clone()
                }
            } else {
                Model {
                    h: target,
                    ..m.clone()
                }
            };
            let ok = moved_ok(&moved, &m.zones);
            if !m.zones.is_empty() {
                ctr.follow_checks += 1;
            }
            let c = m.corners();
            let cmds = if widen {
                vec![
                    move_corner(&f, c[1], (moved.ox + target, c[1].1)),
                    move_corner(&f, c[2], (moved.ox + target, c[2].1)),
                ]
            } else {
                vec![
                    move_corner(&f, c[2], (c[2].0, moved.oy + target)),
                    move_corner(&f, c[3], (c[3].0, moved.oy + target)),
                ]
            };
            (
                Command::Batch { commands: cmds },
                ok,
                Box::new(move |m: &mut Model, _: &Outcome| {
                    if widen {
                        m.w = target;
                    } else {
                        m.h = target;
                    }
                }),
            )
        } else if op < 92 && !m.zones.is_empty() {
            // Remplacement d'une zone par une autre (bande ou polygone).
            let i = rng.below(m.zones.len());
            let id = m.zones[i].id;
            let kind = random_kind(&mut rng);
            ctr.set_checks += 1;
            if rng.chance(50) {
                let Some(b) = pick_band(&mut rng, &f, &m) else {
                    continue;
                };
                let ok = b.n < 4 && b.order_ok && band_ok(&m, b.start, b.n, b.w) == Some(true);
                let (start, n, w) = (b.start, b.n, b.w);
                (
                    Command::SetPieceZone {
                        piece: f.piece,
                        zone: id,
                        spec: ZoneSpec {
                            kind,
                            shape: ZoneShape::Band {
                                edges: b.ids,
                                width_mm: b.w,
                            },
                        },
                    },
                    ok,
                    Box::new(move |m: &mut Model, _: &Outcome| {
                        m.zones[i] = OZone {
                            id,
                            kind,
                            shape: OShape::Band { start, n, width: w },
                        };
                    }),
                )
            } else {
                let pts = random_poly(&mut rng, &m);
                let ok = poly_ok(&m, &pts);
                (
                    Command::SetPieceZone {
                        piece: f.piece,
                        zone: id,
                        spec: zone_spec_poly(kind, &pts),
                    },
                    ok,
                    Box::new(move |m: &mut Model, _: &Outcome| {
                        m.zones[i] = OZone {
                            id,
                            kind,
                            shape: OShape::Poly(pts),
                        };
                    }),
                )
            }
        } else if !m.zones.is_empty() {
            let i = rng.below(m.zones.len());
            let id = m.zones[i].id;
            ctr.remove_checks += 1;
            (
                Command::RemovePieceZone {
                    piece: f.piece,
                    zone: id,
                },
                true,
                Box::new(move |m: &mut Model, _: &Outcome| {
                    m.zones.remove(i);
                }),
            )
        } else {
            continue;
        };
        let is_poly_add = matches!(
            &cmd,
            Command::AddPieceZone { zone, .. } if matches!(zone.shape, ZoneShape::Polygon { .. })
        );
        let band_edges = match &cmd {
            Command::AddPieceZone {
                zone:
                    ZoneSpec {
                        shape: ZoneShape::Band { edges, width_mm },
                        ..
                    },
                ..
            } if (0.5..=100.0).contains(width_mm) => Some(edges.len()),
            _ => None,
        };
        let is_follow = matches!(&cmd, Command::Transform { .. } | Command::Batch { .. })
            && !m.zones.is_empty();
        let result = catch_unwind(AssertUnwindSafe(|| f.doc.execute(cmd)));
        let result = result.unwrap_or_else(|_| panic!("panique (graine {seed})"));
        match result {
            Ok(o) => {
                assert!(
                    predicted,
                    "graine {seed} : accepté alors que prédit refusé ({m:?})"
                );
                steps += 1;
                ctr.accepted += 1;
                on_accept(&mut m, &o);
                if is_poly_add {
                    ctr.poly_accepted += 1;
                }
                match band_edges {
                    Some(1) => ctr.band1 += 1,
                    Some(2) => ctr.band2 += 1,
                    Some(3) => ctr.band3 += 1,
                    _ => {}
                }
            }
            Err(e) => {
                assert!(
                    !predicted,
                    "graine {seed} : refusé ({e}) alors que prédit accepté ({m:?})"
                );
                if band_edges == Some(4) {
                    let why = e.to_string();
                    assert!(
                        why.contains("contour entier") || why.contains("ne se suivent pas"),
                        "{why}"
                    );
                }
                ctr.refused_predicted += 1;
                if is_poly_add {
                    ctr.poly_refused += 1;
                }
                if is_follow {
                    ctr.follow_refused += 1;
                }
                assert_eq!(bytes(f.doc.project()), before, "refus sans trace (octets)");
                assert_eq!(*f.doc.project(), before_p, "refus sans trace (valeur)");
            }
        }
        check_state(&f, &m, ctr);
    }
    // Annulations puis rétablissements : octets exacts, une étape par commande acceptée.
    let last = bytes(f.doc.project());
    let mut undone = 0;
    while f.doc.can_undo() {
        assert!(f.doc.undo());
        undone += 1;
    }
    assert_eq!(undone, steps, "graine {seed}");
    assert_eq!(bytes(f.doc.project()), original);
    for _ in 0..steps {
        assert!(f.doc.redo());
    }
    assert_eq!(bytes(f.doc.project()), last);
    ctr.undo_checks += 1;
    // Relecture JSON identique, zones recalculées identiques.
    let back = persist::from_json(&last).unwrap_or_else(|e| panic!("graine {seed} : {e}"));
    assert_eq!(back, *f.doc.project());
    assert_eq!(bytes(&back), last);
    for z in &back.piece(f.piece).unwrap().zones {
        assert_eq!(
            back.zone_layout(f.piece, z.id).unwrap(),
            f.doc.project().zone_layout(f.piece, z.id).unwrap()
        );
    }
    ctr.json_checks += 1;
}

fn move_corner(f: &Fixture, from: (f64, f64), to: (f64, f64)) -> Command {
    Command::MovePoint {
        sheet: f.sheet,
        point: point_id_at(f, from.0, from.1),
        to: pt(to.0, to.1),
    }
}

/// Toutes les zones restent-elles valables dans le rectangle `m` ? (`false` dès qu'une est
/// invalide ou à la frontière exacte, traitée comme refusée : voir `band_ok`.)
fn moved_ok(m: &Model, zones: &[OZone]) -> bool {
    zones.iter().all(|z| match &z.shape {
        OShape::Band { start, n, width } => band_ok(m, *start, *n, *width) == Some(true),
        OShape::Poly(p) => poly_ok(m, p),
    })
}

#[test]
fn campagne_adverse_des_zones() {
    let mut ctr = Counters::default();
    for i in 0..SEQUENCES {
        run_sequence(0x5A0E_0000 + i as u64 * 7919, &mut ctr);
    }
    eprintln!("{ctr:#?}");
    assert_eq!(ctr.sequences, SEQUENCES);
    assert!(ctr.accepted > 300, "{ctr:?}");
    assert!(ctr.refused_predicted > 300, "{ctr:?}");
    assert!(
        ctr.band1 > 40 && ctr.band2 > 40 && ctr.band3 > 20,
        "{ctr:?}"
    );
    assert!(ctr.poly_accepted > 40 && ctr.poly_refused > 40, "{ctr:?}");
    assert!(ctr.vertex_checks > 300, "{ctr:?}");
    assert!(ctr.width_checks > 300, "{ctr:?}");
    assert!(ctr.area_checks > 400, "{ctr:?}");
    assert!(
        ctr.follow_checks > 100 && ctr.follow_refused > 10,
        "{ctr:?}"
    );
    assert!(ctr.set_checks > 40 && ctr.remove_checks > 20, "{ctr:?}");
    assert!(
        ctr.bad_order_refused > 5 && ctr.whole_refused > 5,
        "{ctr:?}"
    );
    assert_eq!(ctr.undo_checks, SEQUENCES);
    assert_eq!(ctr.json_checks, SEQUENCES);
}

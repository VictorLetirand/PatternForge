//! Plans de coupe, lecture (US-15.1 / US-15.2) — campagne adverse : 180 séquences à graine fixe
//! de 8 à 14 pas (pose, déplacement, rotation, miroir, suppression, « poser la sélection »,
//! annuler / rétablir) sur quatre pièces (rectangle à droit-fil, carré en paire, triangle, L
//! concave) et des plans plats ou pliés.
//!
//! **Oracle indépendant de la géométrie du cœur** : il connaît les sommets locaux de chaque pièce,
//! pose ses propres contours avec `cos` / `sin` (jamais `place_point`), mesure les aires par
//! **échantillonnage** de points (point dans un polygone par lancer de rayon, écrit ici) et, pour
//! les rectangles tournés d'un multiple de 90°, par **intervalles exacts** ; il mesure les écarts
//! par échantillonnage dense des bords (erreur bornée), décide seul du débordement (bandes,
//! pli, début), du droit-fil (angle du segment posé modulo 180) et des quantités. Il est comparé
//! au cœur après **chaque** pas. Des compteurs prouvent que chaque famille de diagnostic est
//! réellement exercée. Mutations : voir `docs/JOURNAL.md` (tranche 125).

use pf_core::cutting_layout::OverflowReason;
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::*;
use pf_geometry::PointMm;

struct Rng(u64);

impl Rng {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.step() % n
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize].clone()
    }
}

type P = (f64, f64);

// ---------------------------------------------------------------------------
// Pièces (sommets locaux : la boîte englobante a son coin bas-gauche en (0 ; 0))
// ---------------------------------------------------------------------------

struct OPiece {
    verts: Vec<P>,
    /// Droit-fil local.
    grain: Option<(P, P)>,
    expected: usize,
    pair: bool,
    rect: Option<(f64, f64)>,
}

fn pieces_def() -> Vec<OPiece> {
    vec![
        OPiece {
            verts: vec![(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
            grain: Some(((50.0, 10.0), (50.0, 40.0))),
            expected: 3,
            pair: false,
            rect: Some((100.0, 50.0)),
        },
        OPiece {
            verts: vec![(0.0, 0.0), (60.0, 0.0), (60.0, 60.0), (0.0, 60.0)],
            grain: None,
            expected: 2,
            pair: true,
            rect: Some((60.0, 60.0)),
        },
        OPiece {
            verts: vec![(0.0, 0.0), (80.0, 0.0), (0.0, 40.0)],
            grain: None,
            expected: 1,
            pair: false,
            rect: None,
        },
        OPiece {
            verts: vec![
                (0.0, 0.0),
                (80.0, 0.0),
                (80.0, 30.0),
                (30.0, 30.0),
                (30.0, 70.0),
                (0.0, 70.0),
            ],
            grain: None,
            expected: 1,
            pair: false,
            rect: None,
        },
    ]
}

struct Sc {
    p: Project,
    ids: Vec<PieceId>,
}

fn build() -> Sc {
    let defs = pieces_def();
    let mut p = Project::new("Adverse pose");
    let s = p.sheets[0].id;
    let mut ids = Vec::new();
    for (k, d) in defs.iter().enumerate() {
        let (ox, oy) = (300.0 * (k as f64 + 1.0), 40.0 + 13.0 * k as f64);
        let vertices: Vec<PointRef> = d
            .verts
            .iter()
            .map(|v| PointRef::New(PointMm::new(ox + v.0, oy + v.1)))
            .collect();
        let poly = p
            .apply(Command::AddPolygon {
                sheet: s,
                vertices,
                closed: true,
                role: PathRole::default(),
                layer: None,
            })
            .unwrap();
        let id = p
            .apply(Command::ExtractPiece {
                sheet: s,
                source: PieceSource::Paths {
                    entities: poly.entities,
                },
                name: format!("P{k}"),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        p.apply(Command::SetPieceInfo {
            piece: id,
            name: format!("P{k}"),
            quantity: if d.pair { 1 } else { d.expected as u32 },
            material: None,
            mirrored_pair: d.pair,
        })
        .unwrap();
        if let Some((a, b)) = d.grain {
            let g = p
                .apply(Command::AddSegment {
                    sheet: s,
                    from: PointRef::New(PointMm::new(ox + a.0, oy + a.1)),
                    to: PointRef::New(PointMm::new(ox + b.0, oy + b.1)),
                })
                .unwrap();
            p.apply(Command::SetPathRole {
                sheet: s,
                entity: g.entities[0],
                role: PathRole::Grainline,
            })
            .unwrap();
            let outline = p.piece(id).unwrap().outline.clone();
            p.apply(Command::SetPieceOutline {
                piece: id,
                outline,
                grainline: Some(g.entities[0]),
            })
            .unwrap();
        }
        ids.push(id);
    }
    Sc { p, ids }
}

// ---------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct OPl {
    piece: usize,
    x: f64,
    y: f64,
    rot: f64,
    mir: bool,
}

#[derive(Clone)]
struct OPlan {
    w: f64,
    sel: f64,
    gap: f64,
    fold: bool,
    grain: Option<f64>,
}

fn o_poly(d: &OPiece, pl: &OPl) -> Vec<P> {
    let (s, c) = pl.rot.to_radians().sin_cos();
    d.verts
        .iter()
        .map(|&(x, y)| {
            let x = if pl.mir { -x } else { x };
            (c * x - s * y + pl.x, s * x + c * y + pl.y)
        })
        .collect()
}

fn o_bbox(v: &[P]) -> (f64, f64, f64, f64) {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in v {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    }
    b
}

fn inside(v: &[P], (px, py): P) -> bool {
    let mut c = false;
    let n = v.len();
    for i in 0..n {
        let (a, b) = (v[i], v[(i + 1) % n]);
        if (a.1 > py) != (b.1 > py) && px < (b.0 - a.0) * (py - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
    }
    c
}

fn right_angle(rot: f64) -> bool {
    let r = (rot / 90.0).round() * 90.0;
    (rot - r).abs() < 1e-9
}

/// Intervalles exacts pour deux rectangles tournés d'un multiple de 90° : (aire, distance).
fn exact_rect_pair(da: &OPiece, a: &OPl, db: &OPiece, b: &OPl) -> Option<(f64, f64)> {
    if da.rect.is_none() || db.rect.is_none() || !right_angle(a.rot) || !right_angle(b.rot) {
        return None;
    }
    let (ba, bb) = (o_bbox(&o_poly(da, a)), o_bbox(&o_poly(db, b)));
    let ox = (ba.2.min(bb.2) - ba.0.max(bb.0)).max(0.0);
    let oy = (ba.3.min(bb.3) - ba.1.max(bb.1)).max(0.0);
    let dx = (ba.0.max(bb.0) - ba.2.min(bb.2)).max(0.0);
    let dy = (ba.1.max(bb.1) - ba.3.min(bb.3)).max(0.0);
    Some((ox * oy, (dx * dx + dy * dy).sqrt()))
}

fn sample_area(va: &[P], vb: &[P]) -> f64 {
    let (ba, bb) = (o_bbox(va), o_bbox(vb));
    let (x0, x1) = (ba.0.max(bb.0), ba.2.min(bb.2));
    let (y0, y1) = (ba.1.max(bb.1), ba.3.min(bb.3));
    if x0 >= x1 || y0 >= y1 {
        return 0.0;
    }
    let step = 1.0;
    let mut n = 0u64;
    let mut x = x0 + step / 2.0;
    while x < x1 {
        let mut y = y0 + step / 2.0;
        while y < y1 {
            if inside(va, (x, y)) && inside(vb, (x, y)) {
                n += 1;
            }
            y += step;
        }
        x += step;
    }
    n as f64 * step * step
}

fn boundary_samples(v: &[P], spacing: f64) -> Vec<P> {
    let mut out = Vec::new();
    for i in 0..v.len() {
        let (a, b) = (v[i], v[(i + 1) % v.len()]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let n = (len / spacing).ceil().max(1.0) as usize;
        for k in 0..n {
            let t = k as f64 / n as f64;
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    out
}

fn sample_distance(va: &[P], vb: &[P]) -> f64 {
    let (sa, sb) = (boundary_samples(va, 1.0), boundary_samples(vb, 1.0));
    let mut best = f64::MAX;
    for a in &sa {
        for b in &sb {
            best = best.min(((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt());
        }
    }
    best
}

fn o_bands(pl: &OPlan) -> Vec<(f64, f64)> {
    if pl.fold {
        vec![(pl.sel, pl.w / 2.0), (pl.w / 2.0, pl.w - pl.sel)]
    } else {
        vec![(pl.sel, pl.w - pl.sel)]
    }
}

fn o_overflow(plan: &OPlan, bb: (f64, f64, f64, f64)) -> Vec<OverflowReason> {
    let mut r = Vec::new();
    let fits = |lo: f64, hi: f64| bb.0 >= lo - 1e-6 && bb.2 <= hi + 1e-6;
    if !o_bands(plan).iter().any(|&(lo, hi)| fits(lo, hi)) {
        r.push(if plan.fold && fits(plan.sel, plan.w - plan.sel) {
            OverflowReason::CrossesFold
        } else {
            OverflowReason::OutsideUseful
        });
    }
    if bb.1 < -1e-6 {
        r.push(OverflowReason::BeforeStart);
    }
    r
}

/// Droit-fil refusé ? (angle du segment posé modulo 180, écart à 0 de plus de 0,01°).
fn o_refused(d: &OPiece, plan: &OPlan, pl: &OPl) -> bool {
    let (Some(g), Some((a, b))) = (plan.grain, d.grain) else {
        return false;
    };
    let tr = |q: P| {
        let (s, c) = pl.rot.to_radians().sin_cos();
        let x = if pl.mir { -q.0 } else { q.0 };
        (c * x - s * q.1, s * x + c * q.1)
    };
    let (a, b) = (tr(a), tr(b));
    let ang = (b.1 - a.1).atan2(b.0 - a.0).to_degrees();
    let r = (ang - g).rem_euclid(180.0);
    r > 0.01 && r < 179.99
}

#[derive(Default)]
struct Cov {
    overlap_exact: u32,
    overlap_sampled: u32,
    gap_violations: u32,
    gap_ok_close: u32,
    out_useful: u32,
    before: u32,
    fold_cross: u32,
    refused: u32,
    mirror_warn: u32,
    over_placed: u32,
    unplaced: u32,
    poser_ok: u32,
    poser_refused: u32,
    undo_redo: u32,
    valid_plans: u32,
    concave_overlap: u32,
}

#[allow(clippy::too_many_arguments)]
fn check(
    p: &Project,
    plan_id: CuttingPlanId,
    ids: &[PieceId],
    defs: &[OPiece],
    plan: &OPlan,
    state: &[(PlacementId, OPl)],
    cov: &mut Cov,
    what: &str,
) {
    let an = p.analyze_cutting_plan(plan_id).unwrap();
    assert_eq!(an.outlines.len(), state.len(), "{what}");
    assert!(an.unreadable.is_empty(), "{what}");
    let polys: Vec<Vec<P>> = state
        .iter()
        .map(|(_, o)| o_poly(&defs[o.piece], o))
        .collect();
    // Débordement, rotation, miroir.
    let mut o_over = 0;
    for (i, (id, o)) in state.iter().enumerate() {
        let want = o_overflow(plan, o_bbox(&polys[i]));
        let got = an.overflows.iter().find(|f| f.placement == *id);
        assert_eq!(
            got.map(|f| f.reasons.clone()).unwrap_or_default(),
            want,
            "{what} débordement {i}"
        );
        if !want.is_empty() {
            o_over += 1;
        }
        for r in &want {
            match r {
                OverflowReason::OutsideUseful => cov.out_useful += 1,
                OverflowReason::BeforeStart => cov.before += 1,
                OverflowReason::CrossesFold => cov.fold_cross += 1,
            }
        }
        let refused = o_refused(&defs[o.piece], plan, o);
        assert_eq!(
            an.rotation_refused.iter().any(|r| r.placement == *id),
            refused,
            "{what} droit-fil {i}"
        );
        cov.refused += refused as u32;
        let warn = o.mir && !defs[o.piece].pair;
        assert_eq!(
            an.warnings
                .iter()
                .any(|w| w.placement == *id && w.kind.key() == "mirror_without_pair"),
            warn,
            "{what} miroir {i}"
        );
        cov.mirror_warn += warn as u32;
        // Aire de coupe exacte invariante : aire du polygone local (shoelace de l'oracle).
        let v = &defs[o.piece].verts;
        let area: f64 = (0..v.len())
            .map(|k| v[k].0 * v[(k + 1) % v.len()].1 - v[(k + 1) % v.len()].0 * v[k].1)
            .sum::<f64>()
            .abs()
            / 2.0;
        assert!(
            (an.outlines[i].area_mm2 - area).abs() < 1e-6,
            "{what} aire {i}"
        );
        // Contour posé : chaque sommet de l'oracle est un sommet du contour du cœur.
        for q in &polys[i] {
            assert!(
                an.outlines[i]
                    .polygon
                    .iter()
                    .any(|r| (r.x - q.0).abs() < 1e-6 && (r.y - q.1).abs() < 1e-6),
                "{what} sommet absent {i}"
            );
        }
    }
    assert_eq!(an.overflows.len(), o_over, "{what}");
    // Paires.
    let mut n_overlap = 0;
    let mut n_gap = 0;
    for i in 0..state.len() {
        for j in (i + 1)..state.len() {
            let (a, b) = (&state[i].1, &state[j].1);
            let (da, db) = (&defs[a.piece], &defs[b.piece]);
            let exact = exact_rect_pair(da, a, db, b);
            let ov = an
                .overlaps
                .iter()
                .find(|o| o.a == state[i].0 && o.b == state[j].0);
            let gp = an
                .gaps
                .iter()
                .find(|o| o.a == state[i].0 && o.b == state[j].0);
            let (area, dist) = match exact {
                Some((ar, ds)) => (ar, Some(ds)),
                None => {
                    let (ba, bb) = (o_bbox(&polys[i]), o_bbox(&polys[j]));
                    let near = ba.0 <= bb.2 + plan.gap + 2.0
                        && bb.0 <= ba.2 + plan.gap + 2.0
                        && ba.1 <= bb.3 + plan.gap + 2.0
                        && bb.1 <= ba.3 + plan.gap + 2.0;
                    (
                        sample_area(&polys[i], &polys[j]),
                        near.then(|| sample_distance(&polys[i], &polys[j])),
                    )
                }
            };
            let tol_area = if exact.is_some() { 1e-6 } else { 140.0 };
            match ov {
                Some(o) => {
                    assert!(
                        (o.area_mm2 - area).abs() <= tol_area,
                        "{what} aire {i}/{j}: {} vs {area}",
                        o.area_mm2
                    );
                    n_overlap += 1;
                    if exact.is_some() {
                        cov.overlap_exact += 1;
                    } else {
                        cov.overlap_sampled += 1;
                    }
                    if a.piece == 3 || b.piece == 3 {
                        cov.concave_overlap += 1;
                    }
                }
                None => assert!(
                    area <= 0.01 + tol_area,
                    "{what} chevauchement manqué {i}/{j}: {area}"
                ),
            }
            if ov.is_none() && area <= 0.01 + tol_area {
                // Écart : décidé seulement hors de la zone d'incertitude de l'échantillonnage.
                if let Some(ds) = dist {
                    let margin = if exact.is_some() { 1e-6 } else { 1.3 };
                    match gp {
                        Some(g) => {
                            assert!(ds <= plan.gap + margin, "{what} écart à tort {i}/{j}: {ds}");
                            assert!(
                                exact.is_none() || (g.distance_mm - ds).abs() < 1e-6,
                                "{what} distance"
                            );
                            n_gap += 1;
                            cov.gap_violations += 1;
                        }
                        None => assert!(
                            ds >= plan.gap - margin,
                            "{what} écart manqué {i}/{j}: {ds} < {}",
                            plan.gap
                        ),
                    }
                    if plan.gap > 0.0 && gp.is_none() && ds < plan.gap + 3.0 {
                        cov.gap_ok_close += 1;
                    }
                }
            }
        }
    }
    assert_eq!(an.overlaps.len(), n_overlap, "{what}");
    assert_eq!(an.gaps.len(), n_gap, "{what}");
    // Quantités.
    for (k, d) in defs.iter().enumerate() {
        let placed = state.iter().filter(|(_, o)| o.piece == k).count();
        let over = an.over_placed.iter().find(|o| o.piece == ids[k]);
        assert_eq!(over.is_some(), placed > d.expected, "{what} quantité {k}");
        cov.over_placed += over.is_some() as u32;
        assert_eq!(
            an.unplaced.contains(&ids[k]),
            placed == 0,
            "{what} non posée {k}"
        );
        cov.unplaced += (placed == 0) as u32;
    }
    let valid = o_over == 0
        && n_overlap == 0
        && n_gap == 0
        && !an.warnings.is_empty() == !an.warnings.is_empty()
        && an.rotation_refused.is_empty();
    assert_eq!(an.is_valid(), valid, "{what} validité");
    cov.valid_plans += (an.is_valid() && !state.is_empty()) as u32;
}

// ---------------------------------------------------------------------------
// Campagne
// ---------------------------------------------------------------------------

fn rand_pose(r: &mut Rng, plan: &OPlan, defs: &[OPiece]) -> OPl {
    let piece = r.below(defs.len() as u64) as usize;
    let rot = r.pick(&[0.0, 0.0, 90.0, 180.0, 270.0, 45.0, 30.0, 0.005, 0.02, 359.0]);
    let (x, y) = if r.chance(60) {
        // Voisinage d'un point de grille pour provoquer contacts et recouvrements.
        (
            5.0 * r.below(((plan.w / 5.0) as u64) + 8) as f64 - 20.0,
            5.0 * r.below(50) as f64 - 5.0,
        )
    } else {
        (
            r.below((plan.w * 10.0) as u64) as f64 / 10.0 - 30.0,
            r.below(3000) as f64 / 10.0 - 10.0,
        )
    };
    OPl {
        piece,
        x,
        y,
        rot,
        mir: r.chance(25),
    }
    .tap(|_| {
        let _ = defs;
    })
}

trait Tap: Sized {
    fn tap(self, f: impl FnOnce(&Self)) -> Self {
        f(&self);
        self
    }
}
impl Tap for OPl {}

#[test]
fn campagne_adverse_pose_et_diagnostics() {
    let defs = pieces_def();
    let mut cov = Cov::default();
    let mut steps = 0u32;
    let mut r = Rng(0x00C0_FFEE_2026_1125);
    for seq in 0..180u32 {
        let sc = build();
        let plan = OPlan {
            w: r.pick(&[600.0, 800.0, 1000.0, 140.0, 800.0]),
            sel: r.pick(&[0.0, 10.0, 25.0]),
            gap: r.pick(&[0.0, 5.0, 12.0]),
            fold: r.chance(35),
            grain: r.pick(&[Some(90.0), Some(0.0), None, Some(90.0)]),
        };
        let mut doc = Document::new(sc.p.clone());
        let plan_id = doc
            .execute(Command::AddCuttingPlan {
                spec: CuttingPlanSpec {
                    name: format!("P{seq}"),
                    material: None,
                    fabric_width_mm: plan.w,
                    fold: if plan.fold {
                        FabricFold::Folded
                    } else {
                        FabricFold::Flat
                    },
                    selvedge_mm: plan.sel,
                    min_gap_mm: plan.gap,
                    grain_direction: plan.grain,
                },
            })
            .unwrap()
            .cutting_plans[0];
        let mut state: Vec<(PlacementId, OPl)> = Vec::new();
        let mut hist: Vec<(Project, Vec<(PlacementId, OPl)>)> =
            vec![(doc.project().clone(), state.clone())];
        let mut redo: Vec<(Project, Vec<(PlacementId, OPl)>)> = Vec::new();
        let n_steps = 8 + r.below(7);
        for step in 0..n_steps {
            let what = format!("séquence {seq} pas {step}");
            match r.below(10) {
                0..=3 => {
                    let o = rand_pose(&mut r, &plan, &defs);
                    let id = doc
                        .execute(Command::AddPlacement {
                            plan: plan_id,
                            piece: sc.ids[o.piece],
                            size: None,
                            x_mm: o.x,
                            y_mm: o.y,
                            rotation_deg: o.rot,
                            mirrored: o.mir,
                        })
                        .unwrap()
                        .placements[0];
                    state.push((id, o));
                    hist.push((doc.project().clone(), state.clone()));
                    redo.clear();
                }
                4 | 5 if !state.is_empty() => {
                    let k = r.below(state.len() as u64) as usize;
                    let mut o = rand_pose(&mut r, &plan, &defs);
                    o.piece = state[k].1.piece;
                    let before = doc.project().clone();
                    doc.execute(Command::UpdatePlacement {
                        plan: plan_id,
                        placement: state[k].0,
                        size: None,
                        x_mm: o.x,
                        y_mm: o.y,
                        rotation_deg: o.rot,
                        mirrored: o.mir,
                    })
                    .unwrap();
                    state[k].1 = o;
                    if *doc.project() != before {
                        hist.push((doc.project().clone(), state.clone()));
                        redo.clear();
                    }
                }
                6 if !state.is_empty() => {
                    let k = r.below(state.len() as u64) as usize;
                    doc.execute(Command::RemovePlacement {
                        plan: plan_id,
                        placement: state[k].0,
                    })
                    .unwrap();
                    state.remove(k);
                    hist.push((doc.project().clone(), state.clone()));
                    redo.clear();
                }
                7 | 8 => {
                    // « Poser la sélection » : 1 à 6 pièces.
                    let wanted_idx: Vec<usize> = (0..1 + r.below(6))
                        .map(|_| r.below(defs.len() as u64) as usize)
                        .collect();
                    let wanted: Vec<_> = wanted_idx.iter().map(|&k| (sc.ids[k], None)).collect();
                    let (lo, hi) = if plan.fold {
                        (plan.sel, plan.w / 2.0)
                    } else {
                        (plan.sel, plan.w - plan.sel)
                    };
                    // Rotation choisie par l'oracle (0, 180, 90, 270 : première permise).
                    let rot_of = |k: usize| {
                        [0.0, 180.0, 90.0, 270.0].into_iter().find(|&rt| {
                            !o_refused(
                                &defs[k],
                                &plan,
                                &OPl {
                                    piece: k,
                                    x: 0.0,
                                    y: 0.0,
                                    rot: rt,
                                    mir: false,
                                },
                            )
                        })
                    };
                    let width_at = |k: usize, rt: f64| {
                        let bb = o_bbox(&o_poly(
                            &defs[k],
                            &OPl {
                                piece: k,
                                x: 0.0,
                                y: 0.0,
                                rot: rt,
                                mir: false,
                            },
                        ));
                        bb.2 - bb.0
                    };
                    let too_wide = wanted_idx
                        .iter()
                        .any(|&k| rot_of(k).is_none_or(|rt| width_at(k, rt) > hi - lo + 1e-6));
                    let before = doc.project().clone();
                    match doc.project().plan_row_layout(plan_id, &wanted) {
                        Err(_) => {
                            assert!(too_wide, "{what} refus injustifié");
                            assert_eq!(*doc.project(), before);
                            cov.poser_refused += 1;
                        }
                        Ok(cmd) => {
                            assert!(!too_wide, "{what} pose d'une pièce trop large");
                            let known: Vec<PlacementId> = doc
                                .project()
                                .cutting_plan(plan_id)
                                .unwrap()
                                .placements
                                .iter()
                                .map(|p| p.id)
                                .collect();
                            doc.execute(cmd).unwrap();
                            let ids: Vec<PlacementId> = doc
                                .project()
                                .cutting_plan(plan_id)
                                .unwrap()
                                .placements
                                .iter()
                                .map(|p| p.id)
                                .filter(|i| !known.contains(i))
                                .collect();
                            assert_eq!(ids.len(), wanted_idx.len(), "{what}");
                            let after = doc
                                .project()
                                .cutting_plan(plan_id)
                                .unwrap()
                                .placements
                                .clone();
                            for (id, &k) in ids.iter().zip(&wanted_idx) {
                                let pl = after.iter().find(|p| p.id == *id).unwrap();
                                assert!(!pl.mirrored);
                                state.push((
                                    *id,
                                    OPl {
                                        piece: k,
                                        x: pl.x_mm,
                                        y: pl.y_mm,
                                        rot: pl.rotation_deg,
                                        mir: false,
                                    },
                                ));
                                // Chaque pose tient dans sa bande et au-dessus du début.
                                let bb = o_bbox(&o_poly(&defs[k], &state.last().unwrap().1));
                                assert!(
                                    bb.0 >= lo - 1e-6 && bb.2 <= hi + 1e-6 && bb.1 >= -1e-6,
                                    "{what} hors bande"
                                );
                            }
                            // Les nouvelles poses ne se chevauchent pas entre elles et respectent l'écart.
                            let new = &state[state.len() - ids.len()..];
                            for i in 0..new.len() {
                                for j in (i + 1)..new.len() {
                                    let (a, b) = (&new[i].1, &new[j].1);
                                    if let Some((ar, ds)) =
                                        exact_rect_pair(&defs[a.piece], a, &defs[b.piece], b)
                                    {
                                        assert!(
                                            ar <= 1e-6 && ds >= plan.gap - 1e-6,
                                            "{what} pose: {ar} {ds}"
                                        );
                                    }
                                }
                            }
                            cov.poser_ok += 1;
                            hist.push((doc.project().clone(), state.clone()));
                            redo.clear();
                        }
                    }
                }
                9 => {
                    if r.chance(50) && hist.len() > 1 {
                        assert!(doc.undo());
                        redo.push(hist.pop().unwrap());
                        let (pj, st) = hist.last().unwrap();
                        assert_eq!(doc.project(), pj, "{what} annulation");
                        state = st.clone();
                        cov.undo_redo += 1;
                    } else if let Some(next) = redo.pop() {
                        assert!(doc.redo());
                        assert_eq!(*doc.project(), next.0, "{what} rétablissement");
                        state = next.1.clone();
                        hist.push(next);
                        cov.undo_redo += 1;
                    }
                }
                _ => {}
            }
            steps += 1;
            check(
                doc.project(),
                plan_id,
                &sc.ids,
                &defs,
                &plan,
                &state,
                &mut cov,
                &what,
            );
        }
    }
    assert!(steps >= 1200, "{steps}");
    assert!(cov.overlap_exact >= 60, "exact {}", cov.overlap_exact);
    assert!(
        cov.overlap_sampled >= 40,
        "échantillonné {}",
        cov.overlap_sampled
    );
    assert!(cov.concave_overlap >= 10, "L {}", cov.concave_overlap);
    assert!(cov.gap_violations >= 40, "écarts {}", cov.gap_violations);
    assert!(
        cov.gap_ok_close >= 5,
        "écarts conformes proches {}",
        cov.gap_ok_close
    );
    assert!(cov.out_useful >= 60, "hors zone {}", cov.out_useful);
    assert!(cov.before >= 20, "début {}", cov.before);
    assert!(cov.fold_cross >= 10, "pli {}", cov.fold_cross);
    assert!(cov.refused >= 40, "droit-fil {}", cov.refused);
    assert!(cov.mirror_warn >= 30, "miroir {}", cov.mirror_warn);
    assert!(cov.over_placed >= 10, "quantité {}", cov.over_placed);
    assert!(cov.unplaced >= 50, "non posées {}", cov.unplaced);
    assert!(cov.poser_ok >= 30, "poser {}", cov.poser_ok);
    assert!(cov.poser_refused >= 3, "poser refusé {}", cov.poser_refused);
    assert!(cov.undo_redo >= 30, "undo {}", cov.undo_redo);
    assert!(cov.valid_plans >= 30, "plans valides {}", cov.valid_plans);
}

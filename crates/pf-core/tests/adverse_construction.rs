//! PH-4a — campagne adverse de la construction rapide : 180 séquences à graine fixe de
//! rectangles, polygones étoilés et rectangles arrondis aléatoires (dimensions de 1e-4 mm à
//! 1e5 mm, rotations quelconques, rayon à la limite), mélangés à des déplacements de points, des
//! extractions de pièce, des annulations et des rétablissements, et des entrées invalides.
//!
//! Oracle **indépendant** : positions par rotation en radians (`sin` / `cos`), périmètres par
//! sommes de `hypot` et formule fermée `2(w−2r) + 2(h−2r) + 2πr`, aires par la formule des
//! lacets ou `w·h − (4−π)r²`, tangence par produit scalaire des tangentes calculées depuis les
//! centres et extrémités des arcs. Rien de `pf_geometry::quick` n'y sert à juger.
//!
//! Propriétés : jamais de panique ; entrée invalide (prédite par l'oracle) = refus et octets
//! identiques, historique inchangé ; entrée valide = acceptée ; mesures exactes ; raccords à
//! 1e-9 mm et tangence < 1e-6° ; `ExtractPiece` sans réglage ; une seule étape annulable ;
//! annuler / rétablir exacts ; relecture JSON identique ; compteurs prouvant que les contrôles
//! forts se sont exécutés.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, RectSpec};
use std::f64::consts::PI;
use std::panic::{catch_unwind, AssertUnwindSafe};

const SEQUENCES: usize = 180;

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
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

#[derive(Default, Debug)]
struct Counters {
    rects_ok: usize,
    rounded_ok: usize,
    polygons_ok: usize,
    refused_predicted: usize,
    undo_redo_checks: usize,
    extractions: usize,
    extraction_area_checks: usize,
    junctions: usize,
    moves_ok: usize,
    moves_refused: usize,
    json_roundtrips: usize,
    perimeter_checks: usize,
    corner_checks: usize,
    stadiums: usize,
    circles: usize,
    boundary_radii: usize,
    extremes: usize,
}

type V2 = (f64, f64);

fn pt(p: V2) -> PointMm {
    PointMm::new(p.0, p.1)
}

fn snap(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

/// Point du plan pour le point local `(x, y)` : rotation en radians autour de `origin`.
fn place(origin: V2, rot_deg: f64, x: f64, y: f64) -> V2 {
    let a = rot_deg.to_radians();
    (
        origin.0 + x * a.cos() - y * a.sin(),
        origin.1 + x * a.sin() + y * a.cos(),
    )
}

fn rect_corners(r: &RectSpec) -> [V2; 4] {
    let (ox, oy) = if r.centered {
        (-r.width_mm / 2.0, -r.height_mm / 2.0)
    } else {
        (0.0, 0.0)
    };
    let o = (r.origin.x, r.origin.y);
    let (w, h) = (r.width_mm, r.height_mm);
    [
        place(o, r.rotation_deg, ox, oy),
        place(o, r.rotation_deg, ox + w, oy),
        place(o, r.rotation_deg, ox + w, oy + h),
        place(o, r.rotation_deg, ox, oy + h),
    ]
}

fn shoelace(v: &[V2]) -> f64 {
    let n = v.len();
    let s: f64 = (0..n)
        .map(|i| v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1)
        .sum();
    s.abs() / 2.0
}

fn perimeter(v: &[V2], closed: bool) -> f64 {
    let n = v.len();
    let sides = if closed { n } else { n - 1 };
    (0..sides)
        .map(|i| (v[(i + 1) % n].0 - v[i].0).hypot(v[(i + 1) % n].1 - v[i].1))
        .sum()
}

/// Tolérance de position pour une échelle `s` : arrondis de `sin` / `cos` sur les coordonnées.
fn pos_tol(s: f64) -> f64 {
    1e-9 + 1e-12 * s
}

fn near(a: V2, b: PointMm, tol: f64) -> bool {
    (a.0 - b.x).hypot(a.1 - b.y) <= tol
}

// ------------------------------------------------------------------ lecture indépendante

fn position(p: &Project, s: SheetId, id: PointId) -> V2 {
    let q = p.sheet(s).unwrap().position(id).unwrap();
    (q.x, q.y)
}

/// (début, fin) d'un tracé ; tangentes unitaires (entrée, sortie) ; longueur lue sur le modèle.
fn ends(p: &Project, s: SheetId, e: EntityId) -> (PointId, PointId) {
    match &p.sheet(s).unwrap().entity(e).unwrap().kind {
        EntityKind::Segment { from, to, .. } => (*from, *to),
        EntityKind::Arc { start, end, .. } => (*start, *end),
        other => panic!("tracé inattendu {other:?}"),
    }
}

fn tangents(p: &Project, s: SheetId, e: EntityId) -> (V2, V2) {
    match &p.sheet(s).unwrap().entity(e).unwrap().kind {
        EntityKind::Segment { from, to, .. } => {
            let (a, b) = (position(p, s, *from), position(p, s, *to));
            let l = (b.0 - a.0).hypot(b.1 - a.1);
            let d = ((b.0 - a.0) / l, (b.1 - a.1) / l);
            (d, d)
        }
        EntityKind::Arc {
            center,
            start,
            end,
            ccw,
        } => {
            assert!(*ccw);
            let c = position(p, s, *center);
            let t = |q: PointId| {
                let v = position(p, s, q);
                let r = (v.0 - c.0).hypot(v.1 - c.1);
                (-(v.1 - c.1) / r, (v.0 - c.0) / r)
            };
            (t(*start), t(*end))
        }
        other => panic!("tracé inattendu {other:?}"),
    }
}

fn angle_deg(a: V2, b: V2) -> f64 {
    (a.0 * b.1 - a.1 * b.0)
        .abs()
        .atan2(a.0 * b.0 + a.1 * b.1)
        .to_degrees()
}

fn length_sum(p: &Project, s: SheetId, ents: &[EntityId]) -> f64 {
    let sh = p.sheet(s).unwrap();
    ents.iter().map(|e| sh.path_length_mm(*e).unwrap()).sum()
}

// ------------------------------------------------------------------ historique miroir

struct Hist {
    doc: Document,
    states: Vec<String>,
    cur: usize,
}

impl Hist {
    fn new(p: Project) -> Self {
        let first = snap(&p);
        Hist {
            doc: Document::new(p),
            states: vec![first],
            cur: 0,
        }
    }
    fn project(&self) -> &Project {
        self.doc.project()
    }
    /// Exécute ; refus = octets et historique identiques ; succès = nouvel état empilé.
    fn run(&mut self, c: Command, _counters: &mut Counters) -> Result<Outcome, CoreError> {
        let (undo, redo) = (self.doc.can_undo(), self.doc.can_redo());
        let before = snap(self.doc.project());
        assert_eq!(before, self.states[self.cur]);
        match self.doc.execute(c) {
            Ok(out) => {
                let after = snap(self.doc.project());
                if after != before {
                    self.states.truncate(self.cur + 1);
                    self.states.push(after);
                    self.cur += 1;
                }
                self.doc.project().validate().unwrap();
                Ok(out)
            }
            Err(e) => {
                assert_eq!(
                    snap(self.doc.project()),
                    before,
                    "un refus ne laisse aucune trace"
                );
                assert_eq!((self.doc.can_undo(), self.doc.can_redo()), (undo, redo));
                Err(e)
            }
        }
    }
    fn undo(&mut self, c: &mut Counters) {
        let ok = self.doc.undo();
        assert_eq!(ok, self.cur > 0);
        if ok {
            self.cur -= 1;
            assert_eq!(
                snap(self.doc.project()),
                self.states[self.cur],
                "annuler exact"
            );
            c.undo_redo_checks += 1;
        }
    }
    fn redo(&mut self, c: &mut Counters) {
        let ok = self.doc.redo();
        assert_eq!(ok, self.cur + 1 < self.states.len());
        if ok {
            self.cur += 1;
            assert_eq!(
                snap(self.doc.project()),
                self.states[self.cur],
                "rétablir exact"
            );
            c.undo_redo_checks += 1;
        }
    }
}

// ------------------------------------------------------------------ génération

struct Ctx {
    scale: f64,
    slot: usize,
    max_slots: usize,
    sheet: SheetId,
}

impl Ctx {
    fn next_origin(&mut self) -> Option<V2> {
        if self.slot >= self.max_slots {
            return None;
        }
        let o = (self.slot as f64 * 3.0 * self.scale, 0.0);
        self.slot += 1;
        Some(o)
    }
}

fn gen_rotation(rng: &mut Rng) -> f64 {
    if rng.chance(40) {
        [0.0, 90.0, 180.0, 270.0, 450.0, -90.0, -180.0][rng.below(7)]
    } else {
        rng.range(-720.0, 720.0)
    }
}

fn gen_dim(rng: &mut Rng, scale: f64) -> f64 {
    if rng.chance(20) {
        scale
    } else {
        rng.range(0.1, 1.0) * scale
    }
}

fn gen_rect(rng: &mut Rng, cx: &mut Ctx, square: bool) -> Option<RectSpec> {
    let o = cx.next_origin()?;
    let w = gen_dim(rng, cx.scale);
    let h = if square { w } else { gen_dim(rng, cx.scale) };
    Some(RectSpec {
        origin: pt(o),
        width_mm: w,
        height_mm: h,
        rotation_deg: gen_rotation(rng),
        centered: rng.chance(40),
    })
}

fn oriented_cmd(s: SheetId, r: RectSpec) -> Command {
    Command::AddOrientedRectangle {
        sheet: s,
        rect: r,
        role: PathRole::Construction,
        layer: None,
    }
}

/// Règle documentée du rayon (réécrite ici, indépendamment du code testé).
fn radius_valid(w: f64, h: f64, r: f64) -> bool {
    if !r.is_finite() || r <= 1e-6 {
        return false;
    }
    [w - 2.0 * r, h - 2.0 * r]
        .iter()
        .all(|l| *l >= -1e-9 && !(*l > 1e-9 && *l < 1e-6))
}

// ------------------------------------------------------------------ une séquence

fn one_sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed);
    let scale = [1e-3, 1e-2, 0.5, 5.0, 50.0, 500.0, 5e3, 1e4, 1e5][rng.below(9)];
    let extreme = scale == 1e-3 || scale == 1e5;
    if extreme {
        c.extremes += 1;
    }
    let project = Project::new("adverse");
    let sheet = project.sheets[0].id;
    let mut h = Hist::new(project);
    let mut cx = Ctx {
        scale,
        slot: 0,
        max_slots: if scale >= 5e4 { 2 } else { 6 },
        sheet,
    };
    let ops = 4 + rng.below(8);
    for _ in 0..ops {
        match rng.below(10) {
            0 | 1 => op_rect(&mut rng, &mut cx, &mut h, c),
            2..=4 => op_rounded(&mut rng, &mut cx, &mut h, c),
            5 | 6 => op_polygon(&mut rng, &mut cx, &mut h, c),
            7 => op_invalid(&mut rng, &mut cx, &mut h, c),
            8 => op_move(&mut rng, &mut cx, &mut h, c),
            _ => {
                if rng.chance(50) {
                    h.undo(c)
                } else {
                    h.redo(c)
                }
            }
        }
    }
    // Annuler tout, rétablir tout : états intermédiaires exacts.
    while h.cur > 0 {
        h.undo(c);
    }
    while h.cur + 1 < h.states.len() {
        h.redo(c);
    }
    let json = persist::to_json(h.project()).unwrap();
    let back = persist::from_json(&json).unwrap();
    assert_eq!(snap(&back), snap(h.project()), "relecture JSON identique");
    assert_eq!(persist::to_json(&back).unwrap(), json);
    c.json_roundtrips += 1;
}

fn check_undo_redo_of_last(h: &mut Hist, c: &mut Counters) {
    let after = h.states[h.cur].clone();
    let before = h.states[h.cur - 1].clone();
    assert!(h.doc.undo());
    assert_eq!(
        snap(h.project()),
        before,
        "annuler la création = état d'avant"
    );
    assert!(h.doc.redo());
    assert_eq!(snap(h.project()), after, "rétablir = état après");
    c.undo_redo_checks += 1;
}

fn extract_check(
    h: &mut Hist,
    sheet: SheetId,
    at: V2,
    expect: (f64, f64, usize),
    what: &str,
    c: &mut Counters,
) {
    let (area, per, edges) = expect;
    let out = h
        .run(
            Command::ExtractPiece {
                sheet,
                source: PieceSource::Click { at: pt(at) },
                name: "P".into(),
                detached: false,
            },
            c,
        )
        .unwrap_or_else(|e| panic!("ExtractPiece refusé : {e} ({what})"));
    c.extractions += 1;
    let piece = h.project().piece(out.pieces[0]).unwrap().outline.len();
    assert_eq!(piece, edges, "contour de la pièce");
    let ct = h.project().piece_contour(out.pieces[0]).unwrap();
    assert!(ct.closed && !ct.self_intersecting);
    assert!(
        (ct.area_mm2 - area).abs() <= 1e-6 * area,
        "aire {} attendue {area}",
        ct.area_mm2
    );
    assert!((ct.perimeter_mm - per).abs() <= 1e-6 * per);
    c.extraction_area_checks += 1;
}

fn op_rect(rng: &mut Rng, cx: &mut Ctx, h: &mut Hist, c: &mut Counters) {
    let Some(r) = gen_rect(rng, cx, false) else {
        return;
    };
    let out = h
        .run(oriented_cmd(cx.sheet, r), c)
        .expect("rectangle valide accepté");
    c.rects_ok += 1;
    assert_eq!((out.points.len(), out.entities.len()), (4, 4));
    let corners = rect_corners(&r);
    let tol = pos_tol(cx.scale);
    for (id, want) in out.points.iter().zip(corners) {
        let got = h.project().sheet(cx.sheet).unwrap().position(*id).unwrap();
        assert!(near(want, got, tol), "{got:?} != {want:?}");
    }
    let per = 2.0 * (r.width_mm + r.height_mm);
    assert!((length_sum(h.project(), cx.sheet, &out.entities) - per).abs() <= 1e-9 * per);
    c.perimeter_checks += 1;
    check_undo_redo_of_last(h, c);
    if r.width_mm.min(r.height_mm) >= 1.0 {
        let centre = place(
            (r.origin.x, r.origin.y),
            r.rotation_deg,
            if r.centered { 0.0 } else { r.width_mm / 2.0 },
            if r.centered { 0.0 } else { r.height_mm / 2.0 },
        );
        extract_check(
            h,
            cx.sheet,
            centre,
            (r.width_mm * r.height_mm, per, 4),
            &format!("rectangle {r:?}"),
            c,
        );
    }
}

fn op_rounded(rng: &mut Rng, cx: &mut Ctx, h: &mut Hist, c: &mut Counters) {
    let square = rng.chance(15);
    let Some(r) = gen_rect(rng, cx, square) else {
        return;
    };
    let (w, hh) = (r.width_mm, r.height_mm);
    let half = w.min(hh) / 2.0;
    let radius = match rng.below(20) {
        0..=8 => rng.range(0.05, 0.95) * half,
        9..=11 => half,
        12 => half * rng.range(0.999, 1.0),
        13 | 14 => (half * 1e-3).max(2e-6),
        15 | 16 => half * rng.range(1.001, 1.5),
        17 => [0.0, -1.0, f64::NAN, f64::INFINITY, 1e-7][rng.below(5)],
        _ => half,
    };
    let hide = rng.chance(50);
    let cmd = Command::AddRoundedRectangle {
        sheet: cx.sheet,
        rect: r,
        radius_mm: radius,
        hide_centers: hide,
        role: PathRole::Construction,
        layer: None,
    };
    if !radius_valid(w, hh, radius) {
        assert!(
            h.run(cmd, c).is_err(),
            "rayon {radius} sur {w} × {hh} doit être refusé"
        );
        c.refused_predicted += 1;
        return;
    }
    let out = h
        .run(cmd, c)
        .unwrap_or_else(|e| panic!("rayon {radius} sur {w}×{hh} refusé : {e}"));
    c.rounded_ok += 1;
    let (lw, lh) = (w - 2.0 * radius, hh - 2.0 * radius);
    let (zw, zh) = (lw <= 1e-9, lh <= 1e-9);
    let (arcs, lines, centres) = match (zw, zh) {
        (false, false) => (4, 4, 4),
        (true, true) => (2, 0, 1),
        _ => (2, 2, 2),
    };
    if zw || zh {
        c.boundary_radii += 1;
    }
    if zw != zh {
        c.stadiums += 1;
    }
    if zw && zh {
        c.circles += 1;
    }
    assert_eq!(out.entities.len(), arcs + lines);
    let npoints = match (zw, zh) {
        (false, false) => 12,
        (true, true) => 3,
        _ => 6,
    };
    assert_eq!(out.points.len(), npoints);
    let sh = h.project().sheet(cx.sheet).unwrap();
    let kinds_arc = out
        .entities
        .iter()
        .filter(|e| matches!(sh.entity(**e).unwrap().kind, EntityKind::Arc { .. }))
        .count();
    assert_eq!(kinds_arc, arcs);
    // Centres : au plus 4, masqués exactement quand demandé.
    let hidden = out
        .points
        .iter()
        .filter(|id| !sh.point_visible(**id))
        .count();
    assert_eq!(hidden, if hide { centres } else { 0 }, "centres masqués");
    // Rayon de chaque arc et angle de balayage par produit scalaire.
    for e in &out.entities {
        if let EntityKind::Arc {
            center, start, end, ..
        } = &sh.entity(*e).unwrap().kind
        {
            let cc = position(h.project(), cx.sheet, *center);
            for q in [start, end] {
                let v = position(h.project(), cx.sheet, *q);
                let d = (v.0 - cc.0).hypot(v.1 - cc.1);
                assert!(
                    (d - radius).abs() <= 1e-9 * radius.max(1.0) + 1e-9,
                    "rayon {d} != {radius}"
                );
            }
            c.corner_checks += 1;
        }
    }
    // Contour fermé, raccords exacts, tangence.
    let n = out.entities.len();
    for i in 0..n {
        let (a, b) = (out.entities[i], out.entities[(i + 1) % n]);
        let (_, ae) = ends(h.project(), cx.sheet, a);
        let (bs, _) = ends(h.project(), cx.sheet, b);
        let (pa, pb) = (
            position(h.project(), cx.sheet, ae),
            position(h.project(), cx.sheet, bs),
        );
        assert!((pa.0 - pb.0).hypot(pa.1 - pb.1) <= 1e-9, "raccord {i}");
        let ang = angle_deg(
            tangents(h.project(), cx.sheet, a).1,
            tangents(h.project(), cx.sheet, b).0,
        );
        assert!(
            ang < 1e-6,
            "tangence {ang}° au raccord {i} (r = {radius}, {w} × {hh})"
        );
        c.junctions += 1;
    }
    let per = 2.0 * lw + 2.0 * lh + 2.0 * PI * radius;
    let got = length_sum(h.project(), cx.sheet, &out.entities);
    assert!((got - per).abs() <= 1e-9 * per, "périmètre {got} != {per}");
    c.perimeter_checks += 1;
    check_undo_redo_of_last(h, c);
    if w.min(hh) >= 1.0 {
        let centre = place(
            (r.origin.x, r.origin.y),
            r.rotation_deg,
            if r.centered { 0.0 } else { w / 2.0 },
            if r.centered { 0.0 } else { hh / 2.0 },
        );
        let area = w * hh - (4.0 - PI) * radius * radius;
        extract_check(
            h,
            cx.sheet,
            centre,
            (area, per, n),
            &format!("arrondi {r:?} r={radius}"),
            c,
        );
    }
}

fn op_polygon(rng: &mut Rng, cx: &mut Ctx, h: &mut Hist, c: &mut Counters) {
    let Some(o) = cx.next_origin() else {
        return;
    };
    let n = 3 + rng.below(10);
    let closed = rng.chance(70);
    // Polygone étoilé autour de `o` : angles croissants, rayons dans [0,3 ; 1] × échelle.
    let step = 2.0 * PI / n as f64;
    let verts: Vec<V2> = (0..n)
        .map(|k| {
            let a = step * (k as f64 + rng.range(-0.2, 0.2));
            let r = rng.range(0.3, 1.0) * cx.scale;
            (o.0 + r * a.cos(), o.1 + r * a.sin())
        })
        .collect();
    // Oracle de validité : côtés consécutifs de plus de 1e-6 mm.
    let sides = if closed { n } else { n - 1 };
    let degenerate = (0..sides).any(|i| {
        (verts[(i + 1) % n].0 - verts[i].0).hypot(verts[(i + 1) % n].1 - verts[i].1) <= 1e-6
    });
    let cmd = Command::AddPolygon {
        sheet: cx.sheet,
        vertices: verts.iter().map(|v| PointRef::New(pt(*v))).collect(),
        closed,
        role: PathRole::Construction,
        layer: None,
    };
    if degenerate {
        assert!(h.run(cmd, c).is_err());
        c.refused_predicted += 1;
        return;
    }
    let out = h.run(cmd, c).expect("polygone valide accepté");
    c.polygons_ok += 1;
    assert_eq!(out.points.len(), n);
    assert_eq!(out.entities.len(), sides);
    for (id, want) in out.points.iter().zip(&verts) {
        let got = h.project().sheet(cx.sheet).unwrap().position(*id).unwrap();
        assert!(near(*want, got, 1e-12 * cx.scale.max(1.0)));
    }
    let per = perimeter(&verts, closed);
    let got = length_sum(h.project(), cx.sheet, &out.entities);
    assert!((got - per).abs() <= 1e-9 * per, "périmètre {got} != {per}");
    c.perimeter_checks += 1;
    check_undo_redo_of_last(h, c);
    if closed && cx.scale >= 1.0 {
        extract_check(
            h,
            cx.sheet,
            o,
            (shoelace(&verts), per, n),
            &format!("polygone {verts:?}"),
            c,
        );
    }
}

fn op_invalid(rng: &mut Rng, cx: &mut Ctx, h: &mut Hist, c: &mut Counters) {
    let s = cx.sheet;
    let bad_rect = |w: f64, hh: f64, rot: f64, o: V2| {
        oriented_cmd(
            s,
            RectSpec {
                origin: pt(o),
                width_mm: w,
                height_mm: hh,
                rotation_deg: rot,
                centered: false,
            },
        )
    };
    let v = |x: f64, y: f64| PointRef::New(PointMm::new(x, y));
    let sc = cx.scale;
    let cmd = match rng.below(12) {
        0 => bad_rect(0.0, sc, 0.0, (0.0, 0.0)),
        1 => bad_rect(sc, -sc, 0.0, (0.0, 0.0)),
        2 => bad_rect(f64::NAN, sc, 0.0, (0.0, 0.0)),
        3 => bad_rect(sc, sc, f64::INFINITY, (0.0, 0.0)),
        4 => bad_rect(sc, sc, 0.0, (f64::NAN, 0.0)),
        5 => bad_rect(5e-7, sc, 0.0, (0.0, 0.0)),
        6 => bad_rect(sc, sc, 10.0, (2e6, 0.0)),
        7 => Command::AddPolygon {
            sheet: s,
            vertices: vec![v(0.0, 0.0), v(sc, 0.0)],
            closed: true,
            role: PathRole::Construction,
            layer: None,
        },
        8 => Command::AddPolygon {
            sheet: s,
            vertices: vec![v(0.0, 0.0), v(sc, 0.0), v(sc, 0.0)],
            closed: false,
            role: PathRole::Construction,
            layer: None,
        },
        9 => Command::AddPolygon {
            sheet: s,
            vertices: (0..201)
                .map(|i| v(i as f64 * sc, (i % 2) as f64 * sc))
                .collect(),
            closed: false,
            role: PathRole::Construction,
            layer: None,
        },
        10 => Command::AddPolygon {
            sheet: s,
            vertices: vec![v(0.0, f64::NAN), v(sc, 0.0), v(sc, sc)],
            closed: true,
            role: PathRole::Construction,
            layer: None,
        },
        _ => Command::AddRoundedRectangle {
            sheet: s,
            rect: RectSpec {
                origin: PointMm::ORIGIN,
                width_mm: sc,
                height_mm: sc,
                rotation_deg: 0.0,
                centered: false,
            },
            radius_mm: sc * 0.51,
            hide_centers: true,
            role: PathRole::Construction,
            layer: None,
        },
    };
    assert!(h.run(cmd, c).is_err());
    c.refused_predicted += 1;
}

fn op_move(rng: &mut Rng, cx: &mut Ctx, h: &mut Hist, c: &mut Counters) {
    let pts: Vec<PointId> = h
        .project()
        .sheet(cx.sheet)
        .unwrap()
        .points
        .iter()
        .map(|p| p.id)
        .collect();
    if pts.is_empty() {
        return;
    }
    let id = pts[rng.below(pts.len())];
    let at = h.project().sheet(cx.sheet).unwrap().position(id).unwrap();
    let d = cx.scale / 50.0;
    let to = PointMm::new(at.x + rng.range(-d, d), at.y + rng.range(-d, d));
    match h.run(
        Command::MovePoint {
            sheet: cx.sheet,
            point: id,
            to,
        },
        c,
    ) {
        Ok(_) => c.moves_ok += 1,
        Err(_) => c.moves_refused += 1,
    }
}

#[test]
fn campagne_adverse_construction_rapide() {
    let mut c = Counters::default();
    for i in 0..SEQUENCES {
        let seed = 0xC0FF_EE00 + i as u64 * 7919;
        let r = catch_unwind(AssertUnwindSafe(|| one_sequence(seed, &mut c)));
        if let Err(e) = r {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            panic!("séquence {i} (graine {seed:#x}) : {msg}");
        }
    }
    eprintln!("{c:?}");
    // Les contrôles forts se sont réellement exécutés.
    assert!(c.rects_ok >= 40, "{c:?}");
    assert!(c.rounded_ok >= 100, "{c:?}");
    assert!(c.polygons_ok >= 60, "{c:?}");
    assert!(c.refused_predicted >= 60, "{c:?}");
    assert!(c.junctions >= 600, "{c:?}");
    assert!(c.corner_checks >= 300, "{c:?}");
    assert!(c.extraction_area_checks >= 100, "{c:?}");
    assert!(c.undo_redo_checks >= 400, "{c:?}");
    assert!(c.perimeter_checks >= 200, "{c:?}");
    assert!(
        c.stadiums >= 5 && c.circles >= 1 && c.boundary_radii >= 8,
        "{c:?}"
    );
    assert!(
        c.moves_ok >= 5 && c.moves_refused + c.moves_ok >= 20,
        "{c:?}"
    );
    assert!(c.extremes >= 10, "{c:?}");
    assert_eq!(c.json_roundtrips, SEQUENCES);
}

//! PH-5b — campagne adverse des bords « sur le pli » : 170 séquences à graine fixe sur des
//! rectangles (alignés ou tournés) et des polygones convexes (4 à 6 sommets, points d'une ellipse),
//! marges par bord aléatoires, bords déclarés ou retirés au hasard, marges réglées sur un bord déjà
//! sur le pli, sommets déplacés, annulations et rétablissements, relectures JSON, copies de feuille,
//! refus sans trace.
//!
//! **Oracle indépendant** : il ne lit rien du code testé. Chaque bord porte une marge effective
//! (nulle s'il est sur le pli, sinon sa surcharge ou la valeur par défaut) ; la droite de coupe de
//! chaque bord est la droite du bord décalée de cette marge vers l'extérieur ; chaque sommet de coupe
//! est l'**intersection** des droites décalées de ses deux bords (onglet). Aire (formule du lacet),
//! périmètre (somme des côtés), ligne de pliure (le côté de la coupe qui porte le bord sur le pli),
//! nombre de suites ouvertes (groupes cycliques de bords hors pli) sont recalculés ainsi. Les
//! séquences où une pointe d'onglet dépasserait la limite du moteur (biseau) ou où le polygone n'est
//! plus convexe sont écartées par l'oracle seul. Des **compteurs** prouvent que les contrôles forts
//! s'exécutent. Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 81).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::region::trace_geometry;
use pf_geometry::PointMm;

struct Rng(u64);

impl Rng {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f64 {
        (self.step() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    fn below(&mut self, n: u64) -> u64 {
        self.step() % n
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    rectangles: usize,
    polygons: usize,
    verified: usize,
    folds_declared: usize,
    folds_removed: usize,
    margins_set: usize,
    margin_ignored_checked: usize,
    runs_checked: usize,
    all_fold_checked: usize,
    moves: usize,
    moves_skipped: usize,
    undo_redo_checks: usize,
    json_roundtrips: usize,
    copies: usize,
    refusals: usize,
    scene_checks: usize,
}

// ----- oracle ---------------------------------------------------------------------------------

#[derive(Clone)]
struct Mirror {
    /// Sommets en anti-horaire.
    v: Vec<PointMm>,
    default_mm: f64,
    /// Surcharge de marge de chaque bord (bord `i` = sommet `i` → sommet `i + 1`).
    over: Vec<Option<f64>>,
    fold: Vec<bool>,
}

fn sub(a: PointMm, b: PointMm) -> PointMm {
    pt(a.x - b.x, a.y - b.y)
}
fn cross(a: PointMm, b: PointMm) -> f64 {
    a.x * b.y - a.y * b.x
}
fn dist(a: PointMm, b: PointMm) -> f64 {
    sub(a, b).x.hypot(sub(a, b).y)
}

impl Mirror {
    fn n(&self) -> usize {
        self.v.len()
    }

    /// Marge effective d'un bord : nulle sur le pli.
    fn margin(&self, i: usize) -> f64 {
        if self.fold[i] {
            0.0
        } else {
            self.over[i].unwrap_or(self.default_mm)
        }
    }

    fn convex(&self) -> bool {
        let n = self.n();
        (0..n).all(|i| {
            let (a, b, c) = (self.v[i], self.v[(i + 1) % n], self.v[(i + 2) % n]);
            cross(sub(b, a), sub(c, b)) > 1e-6
        })
    }

    /// Droite décalée du bord `i` : (point, direction).
    fn offset_line(&self, i: usize) -> (PointMm, PointMm) {
        let n = self.n();
        let (a, b) = (self.v[i], self.v[(i + 1) % n]);
        let d = sub(b, a);
        let len = d.x.hypot(d.y);
        // Normale extérieure d'un contour anti-horaire : (dy, −dx) / len.
        let normal = pt(d.y / len, -d.x / len);
        let m = self.margin(i);
        (pt(a.x + normal.x * m, a.y + normal.y * m), d)
    }

    /// Sommets de la coupe : intersection des droites décalées des bords `i − 1` et `i`.
    fn cut_vertices(&self) -> Vec<PointMm> {
        let n = self.n();
        (0..n)
            .map(|i| {
                let (p1, d1) = self.offset_line((i + n - 1) % n);
                let (p2, d2) = self.offset_line(i);
                let t = cross(sub(p2, p1), d2) / cross(d1, d2);
                pt(p1.x + d1.x * t, p1.y + d1.y * t)
            })
            .collect()
    }

    /// Une pointe d'onglet reste sous la limite du moteur (4 × la plus grande marge adjacente) :
    /// sinon le moteur coupe en biseau et la formule d'intersection ne s'applique plus.
    fn mitre_ok(&self) -> bool {
        let n = self.n();
        let cut = self.cut_vertices();
        (0..n).all(|i| {
            let big = self.margin((i + n - 1) % n).max(self.margin(i));
            big == 0.0 || dist(cut[i], self.v[i]) <= 3.5 * big
        })
    }

    fn area_and_perimeter(&self) -> (f64, f64) {
        let c = self.cut_vertices();
        let n = c.len();
        let area: f64 = (0..n).map(|i| cross(c[i], c[(i + 1) % n])).sum::<f64>() / 2.0;
        let per: f64 = (0..n).map(|i| dist(c[i], c[(i + 1) % n])).sum();
        (area.abs(), per)
    }

    fn fold_count(&self) -> usize {
        self.fold.iter().filter(|f| **f).count()
    }

    /// Suites ouvertes de la coupe : groupes cycliques de bords hors pli (1 chaîne fermée sans pli ;
    /// aucune suite si tout est pli).
    fn runs(&self) -> usize {
        let n = self.n();
        (0..n)
            .filter(|&i| !self.fold[i] && self.fold[(i + n - 1) % n])
            .count()
    }
}

// ----- scénario -------------------------------------------------------------------------------

struct H {
    doc: Document,
    piece: PieceId,
    sheet: SheetId,
    entities: Vec<EntityId>,
    m: Mirror,
    stray: EntityId,
    states: Vec<(String, Mirror)>,
    cur: usize,
}

fn entity_of(p: &Project, piece: PieceId, a: PointMm, b: PointMm) -> EntityId {
    let sheet = p.piece_sheet(piece).unwrap();
    p.piece(piece)
        .unwrap()
        .outline
        .iter()
        .find(|e| match sheet.shape(e.entity) {
            Ok(Shape::Line(x, y)) => {
                (dist(x, a) < 1e-6 && dist(y, b) < 1e-6) || (dist(x, b) < 1e-6 && dist(y, a) < 1e-6)
            }
            _ => false,
        })
        .expect("bord du contour")
        .entity
}

fn seam_of(m: &Mirror, entities: &[EntityId]) -> SeamAllowance {
    SeamAllowance {
        default_mm: m.default_mm,
        edges: (0..m.n())
            .filter_map(|i| {
                m.over[i].map(|v| EdgeAllowance {
                    entity: entities[i],
                    start_mm: v,
                    end_mm: v,
                })
            })
            .collect(),
        ..Default::default()
    }
}

fn random_polygon(rng: &mut Rng, c: &mut Counters) -> Vec<PointMm> {
    let kind = rng.below(10);
    if kind < 3 {
        c.rectangles += 1;
        let (w, h) = (rng.range(80.0, 260.0), rng.range(80.0, 260.0));
        let (w, h) = ((w * 4.0).round() / 4.0, (h * 4.0).round() / 4.0);
        let (x, y) = (
            (rng.range(-50.0, 50.0) * 4.0).round() / 4.0,
            (rng.range(-50.0, 50.0) * 4.0).round() / 4.0,
        );
        return vec![pt(x, y), pt(x + w, y), pt(x + w, y + h), pt(x, y + h)];
    }
    if kind < 5 {
        c.rectangles += 1;
        let (w, h) = (rng.range(80.0, 220.0), rng.range(80.0, 220.0));
        let a = rng.range(5.0, 85.0).to_radians();
        let (s, co) = (a.sin(), a.cos());
        let r = |x: f64, y: f64| {
            pt(
                ((x * co - y * s) * 100.0).round() / 100.0,
                ((x * s + y * co) * 100.0).round() / 100.0,
            )
        };
        return vec![r(0.0, 0.0), r(w, 0.0), r(w, h), r(0.0, h)];
    }
    c.polygons += 1;
    let n = 4 + rng.below(3) as usize;
    let (ra, rb) = (rng.range(70.0, 200.0), rng.range(70.0, 200.0));
    let phase = rng.range(0.0, std::f64::consts::TAU);
    (0..n)
        .map(|i| {
            let t = phase + (i as f64 + rng.range(-0.18, 0.18)) * std::f64::consts::TAU / n as f64;
            pt(
                (ra * t.cos() * 100.0).round() / 100.0,
                (rb * t.sin() * 100.0).round() / 100.0,
            )
        })
        .collect()
}

fn setup(rng: &mut Rng, c: &mut Counters) -> Option<H> {
    let v = random_polygon(rng, c);
    let n = v.len();
    let mut m = Mirror {
        v,
        default_mm: [5.0, 8.0, 10.0, 12.0][rng.below(4) as usize],
        over: vec![None; n],
        fold: vec![false; n],
    };
    // Sens anti-horaire exigé ; polygone convexe ; onglet sous la limite.
    if !m.convex() {
        return None;
    }
    for i in 0..n {
        if rng.chance(35) {
            m.over[i] = Some([0.0, 5.0, 10.0, 15.0, 20.0][rng.below(5) as usize]);
        }
    }
    for i in 0..n {
        m.fold[i] = rng.chance(35);
    }
    if !m.mitre_ok() {
        return None;
    }
    let mut p = Project::new("Campagne pliure");
    let s = p.sheets[0].id;
    for i in 0..n {
        p.apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(m.v[i]),
            to: PointRef::New(m.v[(i + 1) % n]),
        })
        .ok()?;
    }
    let cx = m.v.iter().map(|q| q.x).sum::<f64>() / n as f64;
    let cy = m.v.iter().map(|q| q.y).sum::<f64>() / n as f64;
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(cx, cy) },
            name: "Demi".into(),
            detached: false,
        })
        .ok()?
        .pieces[0];
    let entities: Vec<EntityId> = (0..n)
        .map(|i| entity_of(&p, piece, m.v[i], m.v[(i + 1) % n]))
        .collect();
    // Un tracé hors du contour, pour les refus.
    let stray = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(900.0, 900.0)),
            to: PointRef::New(pt(950.0, 900.0)),
        })
        .ok()?
        .entities[0];
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(seam_of(&m, &entities)),
    })
    .ok()?;
    // Les bords déclarés d'emblée : posés avant l'historique (l'état de départ n'a rien à annuler).
    for (i, e) in entities.iter().enumerate() {
        if m.fold[i] {
            p.apply(Command::SetPieceFold {
                piece,
                edge: *e,
                on: true,
            })
            .ok()?;
        }
    }
    let doc = Document::new(p);
    let state = (bytes(doc.project()), m.clone());
    Some(H {
        doc,
        piece,
        sheet: s,
        entities,
        m,
        stray,
        states: vec![state],
        cur: 0,
    })
}

// ----- vérifications --------------------------------------------------------------------------

fn verify(h: &H, c: &mut Counters, what: &str) {
    let m = &h.m;
    let p = h.doc.project();
    let n = m.n();
    let ctx = |s: &str| format!("{what} : {s}");
    // Coupe : sommets, aire, périmètre.
    let cut = p
        .piece_cut_contour(h.piece)
        .unwrap_or_else(|e| panic!("{}", ctx(&e.to_string())));
    let (area, per) = m.area_and_perimeter();
    assert!(
        (cut.area_mm2 - area).abs() < 1e-5 * area.max(1.0),
        "{}",
        ctx(&format!("aire {} ≠ {area}", cut.area_mm2))
    );
    assert!(
        (cut.perimeter_mm - per).abs() < 1e-5 * per.max(1.0),
        "{}",
        ctx(&format!("périmètre {} ≠ {per}", cut.perimeter_mm))
    );
    let want = m.cut_vertices();
    let got: Vec<PointMm> = cut
        .traces
        .iter()
        .flat_map(trace_geometry)
        .map(|g| g.start())
        .collect();
    assert_eq!(got.len(), n, "{}", ctx("nombre de sommets de coupe"));
    assert!(
        want.iter().all(|w| got.iter().any(|g| dist(*g, *w) < 1e-5)),
        "{}",
        ctx(&format!("sommets {got:?} ≠ {want:?}"))
    );
    // Pliures : une portion par bord sur le pli, entre les deux sommets de coupe du bord.
    let k = m.fold_count();
    assert_eq!(cut.fold_cuts.len(), k, "{}", ctx("portions de pliure"));
    let mut fold_len = 0.0;
    for i in 0..n {
        if !m.fold[i] {
            continue;
        }
        let f = cut
            .fold_cuts
            .iter()
            .find(|f| f.entity == h.entities[i])
            .unwrap_or_else(|| panic!("{}", ctx("bord sans portion de pliure")));
        let (a, b) = (want[i], want[(i + 1) % n]);
        match &cut.traces[f.trace] {
            pf_geometry::Trace::Line(x, y) => {
                assert!(
                    dist(*x, a) < 1e-5 && dist(*y, b) < 1e-5,
                    "{}",
                    ctx(&format!("pliure {x:?}→{y:?} ≠ {a:?}→{b:?}"))
                );
            }
            other => panic!("{}", ctx(&format!("pliure non rectiligne : {other:?}"))),
        }
        fold_len += dist(a, b);
    }
    // Lecture : résumé (sur les bords d'origine) et contrôle des marges ignorées.
    let sum = p.piece_fold_summary(h.piece);
    assert_eq!(sum.count, k, "{}", ctx("résumé"));
    let original: f64 = (0..n)
        .filter(|&i| m.fold[i])
        .map(|i| dist(m.v[i], m.v[(i + 1) % n]))
        .sum();
    assert!(
        (sum.length_mm - original).abs() < 1e-6,
        "{}",
        ctx("longueur")
    );
    let ignored = (0..n).filter(|&i| m.fold[i] && m.over[i].is_some()).count();
    let issues = p.piece_issues(h.piece);
    let flagged = issues.iter().find_map(|i| match i {
        PieceIssue::FoldMarginIgnored { count } => Some(*count),
        _ => None,
    });
    assert_eq!(flagged.unwrap_or(0), ignored, "{}", ctx("marges ignorées"));
    if ignored > 0 {
        c.margin_ignored_checked += 1;
    }
    assert!(
        !issues.iter().any(|i| matches!(
            i,
            PieceIssue::FoldOutsideContour { .. } | PieceIssue::CutContourFailed { .. }
        )),
        "{}",
        ctx(&format!("contrôle inattendu {issues:?}"))
    );
    // Export : une ligne de pliure par bord ; la coupe écrite = périmètre − pliures, en autant de
    // suites ouvertes que de groupes cycliques de bords hors pli ; les tracés des bords sur le pli
    // ne sont plus écrits comme contour. Longueurs recalculées par les sommets de l'oracle.
    check_scene(p, h, m, per, fold_len, &want, c, what);
    c.verified += 1;
}

#[allow(clippy::too_many_arguments)]
fn check_scene(
    p: &Project,
    h: &H,
    m: &Mirror,
    perimeter: f64,
    fold_len: f64,
    want: &[PointMm],
    c: &mut Counters,
    what: &str,
) {
    let n = m.n();
    let scene = pf_export::Scene::from_sheet_with(
        p,
        h.sheet,
        pf_export::SceneOptions {
            piece_mode: pf_export::PieceExportMode::Both,
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("{what} : scène : {e}"));
    let k = m.fold_count();
    assert_eq!(scene.folds.len(), k, "{what} : lignes de pliure");
    // Chaque ligne de pliure est le côté de coupe de son bord.
    for i in (0..n).filter(|&i| m.fold[i]) {
        let (a, b) = (want[i], want[(i + 1) % n]);
        assert!(
            scene
                .folds
                .iter()
                .any(|f| dist(f.from, a) < 1e-5 && dist(f.to, b) < 1e-5),
            "{what} : pliure {a:?}→{b:?} absente"
        );
    }
    // Tracés de contour : seulement les bords hors pli (le stray et les autres tracés mis à part).
    let drawn = scene
        .paths
        .iter()
        .filter(|q| q.role == PathRole::Construction)
        .count();
    assert_eq!(
        drawn,
        n - k + 1,
        "{what} : tracés de contour écrits (+ le tracé isolé)"
    );
    // Coupe : suites ouvertes, hors pliure.
    let runs = m.runs();
    if k == 0 {
        assert_eq!(scene.cut_contours.len(), 1);
        assert!(scene.cut_contours[0].closed);
    } else {
        assert_eq!(
            scene.cut_contours.len(),
            runs.max(1),
            "{what} : suites ouvertes"
        );
        assert!(scene.cut_contours.iter().all(|r| !r.closed));
    }
    let written: f64 = scene
        .cut_contours
        .iter()
        .flat_map(|r| &r.edges)
        .map(|e| match e {
            pf_export::PathGeom::Line(a, b) => dist(*a, *b),
            other => panic!("arête inattendue {other:?}"),
        })
        .sum();
    assert!(
        (written - (perimeter - fold_len)).abs() < 1e-5 * perimeter,
        "{what} : coupe écrite {written} ≠ {}",
        perimeter - fold_len
    );
    c.runs_checked += runs;
    if k == n {
        c.all_fold_checked += 1;
    }
    c.scene_checks += 1;
}

fn push_state(h: &mut H) {
    let b = bytes(h.doc.project());
    if b != h.states[h.cur].0 {
        h.states.truncate(h.cur + 1);
        h.states.push((b, h.m.clone()));
        h.cur += 1;
    }
}

fn op_toggle(h: &mut H, rng: &mut Rng, c: &mut Counters) {
    let i = rng.below(h.m.n() as u64) as usize;
    let on = !h.m.fold[i];
    h.doc
        .execute(Command::SetPieceFold {
            piece: h.piece,
            edge: h.entities[i],
            on,
        })
        .unwrap_or_else(|e| panic!("bascule du pli refusée : {e}"));
    let mut next = h.m.clone();
    next.fold[i] = on;
    if !next.mitre_ok() {
        // L'oracle écarte ce cas : on annule pour garder une séquence valide.
        assert!(h.doc.undo());
        return;
    }
    h.m = next;
    if on {
        c.folds_declared += 1;
    } else {
        c.folds_removed += 1;
    }
    push_state(h);
}

fn op_margin(h: &mut H, rng: &mut Rng, c: &mut Counters) {
    let i = rng.below(h.m.n() as u64) as usize;
    let mut next = h.m.clone();
    next.over[i] = if rng.chance(25) {
        None
    } else {
        Some([0.0, 5.0, 10.0, 15.0, 20.0][rng.below(5) as usize])
    };
    if !next.mitre_ok() {
        return;
    }
    h.doc
        .execute(Command::SetSeamAllowance {
            piece: h.piece,
            seam: Some(seam_of(&next, &h.entities)),
        })
        .unwrap_or_else(|e| panic!("marge refusée : {e}"));
    h.m = next;
    c.margins_set += 1;
    push_state(h);
}

fn op_move(h: &mut H, rng: &mut Rng, c: &mut Counters) {
    let n = h.m.n();
    let i = rng.below(n as u64) as usize;
    let centre = {
        let sx: f64 = h.m.v.iter().map(|q| q.x).sum();
        let sy: f64 = h.m.v.iter().map(|q| q.y).sum();
        pt(sx / n as f64, sy / n as f64)
    };
    let k = rng.range(0.88, 1.12);
    let q = h.m.v[i];
    let to = pt(
        ((centre.x + (q.x - centre.x) * k) * 100.0).round() / 100.0,
        ((centre.y + (q.y - centre.y) * k) * 100.0).round() / 100.0,
    );
    let mut next = h.m.clone();
    next.v[i] = to;
    if !next.convex() || !next.mitre_ok() {
        c.moves_skipped += 1;
        return;
    }
    let sheet = h.doc.project().sheet(h.sheet).unwrap();
    let id = sheet
        .points
        .iter()
        .find(|p| sheet.position(p.id).is_ok_and(|v| dist(v, q) < 1e-6))
        .expect("point du sommet")
        .id;
    match h.doc.execute(Command::MovePoint {
        sheet: h.sheet,
        point: id,
        to,
    }) {
        Ok(_) => {
            h.m = next;
            c.moves += 1;
            push_state(h);
        }
        Err(_) => {
            // Refus (ex. contour invalide) : aucune trace.
            assert_eq!(bytes(h.doc.project()), h.states[h.cur].0);
            c.moves_skipped += 1;
        }
    }
}

fn op_undo_redo(h: &mut H, rng: &mut Rng, c: &mut Counters) {
    let steps = 1 + rng.below(3);
    for _ in 0..steps {
        if rng.chance(55) {
            if h.cur > 0 {
                assert!(h.doc.undo(), "annuler");
                h.cur -= 1;
                h.m = h.states[h.cur].1.clone();
            } else {
                assert!(!h.doc.can_undo());
            }
        } else if h.cur + 1 < h.states.len() {
            assert!(h.doc.redo(), "rétablir");
            h.cur += 1;
            h.m = h.states[h.cur].1.clone();
        } else {
            assert!(!h.doc.can_redo());
        }
        assert_eq!(
            bytes(h.doc.project()),
            h.states[h.cur].0,
            "annuler / rétablir = octets exacts"
        );
        c.undo_redo_checks += 1;
    }
}

fn op_roundtrip(h: &mut H, c: &mut Counters) {
    let text = bytes(h.doc.project());
    let back = persist::from_json(&text).expect("relecture");
    assert_eq!(bytes(&back), text, "relecture JSON identique");
    let cut = back.piece_cut_contour(h.piece).expect("coupe relue");
    let cut0 = h.doc.project().piece_cut_contour(h.piece).unwrap();
    assert!((cut.area_mm2 - cut0.area_mm2).abs() < 1e-9);
    assert_eq!(cut.fold_cuts.len(), cut0.fold_cuts.len());
    c.json_roundtrips += 1;
}

fn op_refusals(h: &mut H, c: &mut Counters) {
    let before = bytes(h.doc.project());
    let p0 = h.doc.project().clone();
    let bad = [
        Command::SetPieceFold {
            piece: PieceId(999_999),
            edge: h.entities[0],
            on: true,
        },
        Command::SetPieceFold {
            piece: h.piece,
            edge: EntityId(999_999),
            on: true,
        },
        Command::SetPieceFold {
            piece: h.piece,
            edge: h.stray,
            on: true,
        },
    ];
    for cmd in bad {
        let mut p = h.doc.project().clone();
        assert!(p.apply(cmd).is_err());
        assert_eq!(p, p0, "refus sans trace");
        assert_eq!(bytes(&p), before, "refus : octets identiques");
        c.refusals += 1;
    }
}

fn op_copy(h: &mut H, c: &mut Counters) {
    let mut p = h.doc.project().clone();
    let out = p.apply(Command::DuplicateSheet { sheet: h.sheet }).unwrap();
    let ns = out.sheet.unwrap();
    let copy = p
        .pieces
        .iter()
        .find(|q| q.id != h.piece && matches!(q.link, PieceLink::Linked { sheet } if sheet == ns))
        .expect("pièce copiée");
    assert_eq!(copy.on_fold.len(), h.m.fold_count());
    let a = p.piece_cut_contour(copy.id).unwrap();
    let b = p.piece_cut_contour(h.piece).unwrap();
    assert!((a.area_mm2 - b.area_mm2).abs() < 1e-6);
    assert!((a.perimeter_mm - b.perimeter_mm).abs() < 1e-6);
    assert_eq!(a.fold_cuts.len(), b.fold_cuts.len());
    c.copies += 1;
}

#[test]
fn campagne_adverse_pliure_170_sequences() {
    let mut c = Counters::default();
    let mut seed = 0u64;
    while c.sequences < 170 {
        seed += 1;
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let Some(mut h) = setup(&mut rng, &mut c) else {
            continue;
        };
        c.sequences += 1;
        verify(&h, &mut c, &format!("graine {seed} : départ"));
        for step in 0..(3 + rng.below(5)) {
            match rng.below(10) {
                0..=2 => op_toggle(&mut h, &mut rng, &mut c),
                3..=4 => op_margin(&mut h, &mut rng, &mut c),
                5 => op_move(&mut h, &mut rng, &mut c),
                6 => op_undo_redo(&mut h, &mut rng, &mut c),
                7 => op_roundtrip(&mut h, &mut c),
                8 => op_copy(&mut h, &mut c),
                _ => op_refusals(&mut h, &mut c),
            }
            verify(&h, &mut c, &format!("graine {seed} : étape {step}"));
        }
    }
    // Les contrôles forts ont réellement tourné.
    assert!(c.sequences >= 150, "{c:?}");
    assert!(c.rectangles >= 40 && c.polygons >= 40, "{c:?}");
    assert!(c.verified >= 600, "{c:?}");
    assert!(c.folds_declared >= 60 && c.folds_removed >= 20, "{c:?}");
    assert!(c.margins_set >= 60, "{c:?}");
    assert!(c.margin_ignored_checked >= 30, "{c:?}");
    assert!(c.runs_checked >= 200, "{c:?}");
    assert!(c.all_fold_checked >= 1, "{c:?}");
    assert!(c.moves >= 20, "{c:?}");
    assert!(c.undo_redo_checks >= 60, "{c:?}");
    assert!(c.json_roundtrips >= 40 && c.copies >= 40, "{c:?}");
    assert!(c.refusals >= 60, "{c:?}");
    assert!(c.scene_checks >= 600, "{c:?}");
    eprintln!("{c:?}");
}

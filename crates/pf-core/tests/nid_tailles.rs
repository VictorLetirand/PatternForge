//! US-09.4a — nid de tailles : contours de chaque taille translatés pour que le point
//! d'empilement coïncide. Valeurs calculées **à la main** sur un rectangle 400 × 600 en base M,
//! gamme XS S M L XL ; écarts cumulés `S (−5 ; −10)`, `L (+5 ; +10)`, `XL (+10 ; +22)`,
//! `XS (−9 ; −20)` : A reste fixe, B reçoit `(dx ; 0)`, C `(dx ; dy)`, D `(0 ; dy)`.
//!
//! Dans la taille `T`, le rectangle est A `(0 ; 0)`, B `(400 + dx ; 0)`, C `(400 + dx ; 600 + dy)`,
//! D `(0 ; 600 + dy)`. Le point d'empilement ramène sa position en taille `T` à sa position en
//! base : décalage `(−dx ; −dy)` pour C, `(−dx ; 0)` pour B, `(0 ; −dy)` pour D, nul pour A.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

const TOL: f64 = 1e-9;
const SIZES: [&str; 5] = ["XS", "S", "M", "L", "XL"];
/// (taille, dx, dy) cumulés depuis la base M.
const DELTAS: [(&str, f64, f64); 5] = [
    ("XS", -9.0, -20.0),
    ("S", -5.0, -10.0),
    ("M", 0.0, 0.0),
    ("L", 5.0, 10.0),
    ("XL", 10.0, 22.0),
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn size(p: &Project, label: &str) -> SizeId {
    p.sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("taille {label}"))
        .id
}

fn point_at(p: &Project, sheet: SheetId, at: PointMm) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| pf_geometry::distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
        .unwrap_or_else(|| panic!("aucun point en {at:?}"))
        .id
}

fn seg(p: &mut Project, sheet: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

struct Fx {
    p: Project,
    sheet: SheetId,
    range: SizeRangeId,
    /// A, B, C, D (points libres).
    corners: [PointId; 4],
    /// AB (bas), BC (droite), CD (haut), DA (gauche).
    edges: [EntityId; 4],
}

fn rect() -> Fx {
    let mut p = Project::new("Nid de tailles");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: SIZES.iter().map(|s| s.to_string()).collect(),
        base: 2,
    })
    .unwrap();
    let range = p.sizes[0].id;
    let (a, b, c, d) = (
        pt(0.0, 0.0),
        pt(400.0, 0.0),
        pt(400.0, 600.0),
        pt(0.0, 600.0),
    );
    let ab = seg(&mut p, sheet, a, b);
    let bc = seg(&mut p, sheet, b, c);
    let cd = seg(&mut p, sheet, c, d);
    let da = seg(&mut p, sheet, d, a);
    let (ia, ib, ic, id) = (
        point_at(&p, sheet, a),
        point_at(&p, sheet, b),
        point_at(&p, sheet, c),
        point_at(&p, sheet, d),
    );
    for (label, dx, dy) in DELTAS.into_iter().filter(|t| t.0 != "M") {
        let s = size(&p, label);
        for (point, gx, gy) in [(ib, dx, 0.0), (ic, dx, dy), (id, 0.0, dy)] {
            p.apply(Command::SetPointGrading {
                sheet,
                point,
                size: s,
                dx_mm: gx,
                dy_mm: gy,
            })
            .unwrap();
        }
    }
    Fx {
        p,
        sheet,
        range,
        corners: [ia, ib, ic, id],
        edges: [ab, bc, cd, da],
    }
}

fn nest_size<'a>(n: &'a SizeNest, p: &Project, label: &str) -> &'a NestSize {
    let id = size(p, label);
    n.sizes
        .iter()
        .find(|s| s.size == id)
        .unwrap_or_else(|| panic!("taille {label} absente du nid"))
}

fn outline_of(s: &NestSize, e: EntityId) -> &NestOutline {
    s.outlines
        .iter()
        .find(|o| matches!(o.kind, NestOutlineKind::Path { entity, .. } if entity == e))
        .unwrap_or_else(|| panic!("contour du tracé {e:?} absent"))
}

fn same(got: &[PointMm], want: &[(f64, f64)], ctx: &str) {
    assert_eq!(got.len(), want.len(), "{ctx} : nombre de points");
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g.x - w.0).abs() <= TOL && (g.y - w.1).abs() <= TOL,
            "{ctx} : ({} ; {}) ≠ ({} ; {})",
            g.x,
            g.y,
            w.0,
            w.1
        );
    }
}

/// Contours attendus d'une taille de décalage `(sx ; sy)` : AB, BC, CD, DA calculés à la main
/// depuis le rectangle `(400 + dx) × (600 + dy)`.
fn expected(dx: f64, dy: f64, sx: f64, sy: f64) -> [[(f64, f64); 2]; 4] {
    let a = (sx, sy);
    let b = (400.0 + dx + sx, sy);
    let c = (400.0 + dx + sx, 600.0 + dy + sy);
    let d = (sx, 600.0 + dy + sy);
    [[a, b], [b, c], [c, d], [d, a]]
}

fn check_nest(f: &Fx, n: &SizeNest, shifts: impl Fn(f64, f64) -> (f64, f64)) {
    for (label, dx, dy) in DELTAS {
        let ns = nest_size(n, &f.p, label);
        let (sx, sy) = shifts(dx, dy);
        assert!(
            (ns.shift.0 - sx).abs() <= TOL && (ns.shift.1 - sy).abs() <= TOL,
            "{label} : décalage {:?} ≠ ({sx} ; {sy})",
            ns.shift
        );
        let want = expected(dx, dy, sx, sy);
        for (i, e) in f.edges.iter().enumerate() {
            same(
                &outline_of(ns, *e).points,
                &want[i],
                &format!("{label} bord {i}"),
            );
        }
        assert_eq!(
            ns.outlines.len(),
            4,
            "{label} : quatre tracés, rien d'autre"
        );
        assert_eq!(ns.is_base, label == "M");
    }
}

// ----- CA1 : translation par point d'empilement, calculée à la main ---------------------------

#[test]
fn ca1_ancre_a_origine_du_rectangle_aucune_translation() {
    let f = rect();
    let n =
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(f.corners[0]))
            .unwrap();
    // A ne bouge dans aucune taille : décalage nul, contours = rectangles de chaque taille.
    check_nest(&f, &n, |_, _| (0.0, 0.0));
    assert_eq!(n.base, size(&f.p, "M"));
    assert_eq!(n.anchor_position, pt(0.0, 0.0));
}

#[test]
fn ca1_ancre_coin_c_ramene_c_en_base() {
    let f = rect();
    let n =
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(f.corners[2]))
            .unwrap();
    // C en taille T = (400 + dx ; 600 + dy) → décalage (−dx ; −dy).
    check_nest(&f, &n, |dx, dy| (-dx, -dy));
    assert_eq!(n.anchor_position, pt(400.0, 600.0));
    // Valeurs écrites à la main pour S : A (5 ; 10), B (400 ; 10), C (400 ; 600), D (5 ; 600).
    let s = nest_size(&n, &f.p, "S");
    same(
        &outline_of(s, f.edges[0]).points,
        &[(5.0, 10.0), (400.0, 10.0)],
        "S AB",
    );
    same(
        &outline_of(s, f.edges[2]).points,
        &[(400.0, 600.0), (5.0, 600.0)],
        "S CD",
    );
    // XL : A (−10 ; −22), C (400 ; 600).
    let xl = nest_size(&n, &f.p, "XL");
    same(
        &outline_of(xl, f.edges[3]).points,
        &[(-10.0, 600.0), (-10.0, -22.0)],
        "XL DA",
    );
    // Dans toutes les tailles, C est au même endroit.
    for ns in &n.sizes {
        let c = &outline_of(ns, f.edges[1]).points[1];
        assert!((c.x - 400.0).abs() <= TOL && (c.y - 600.0).abs() <= TOL);
    }
}

#[test]
fn ca1_autres_coins_b_et_d() {
    let f = rect();
    // B : (400 + dx ; 0) → décalage (−dx ; 0).
    let nb =
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(f.corners[1]))
            .unwrap();
    check_nest(&f, &nb, |dx, _| (-dx, 0.0));
    // D : (0 ; 600 + dy) → décalage (0 ; −dy).
    let nd =
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(f.corners[3]))
            .unwrap();
    check_nest(&f, &nd, |_, dy| (0.0, -dy));
    // L, ancre D : A (0 ; −10), B (405 ; −10), C (405 ; 600), D (0 ; 600).
    let l = nest_size(&nd, &f.p, "L");
    same(
        &outline_of(l, f.edges[0]).points,
        &[(0.0, -10.0), (405.0, -10.0)],
        "L AB",
    );
}

#[test]
fn ca1_ancre_origine_de_la_feuille_aucune_translation() {
    let f = rect();
    let n = f.p.size_nest(f.sheet, f.range, NestAnchor::Origin).unwrap();
    check_nest(&f, &n, |_, _| (0.0, 0.0));
    assert_eq!(n.anchor_position, pt(0.0, 0.0));
    for s in &n.sizes {
        assert_eq!(s.shift, (0.0, 0.0));
    }
    // Sans translation, C reste à sa position propre dans chaque taille (XS : 391 ; 580).
    let xs = nest_size(&n, &f.p, "XS");
    same(
        &outline_of(xs, f.edges[1]).points,
        &[(391.0, 0.0), (391.0, 580.0)],
        "XS BC",
    );
}

#[test]
fn ca1_point_construit_comme_ancre() {
    let mut f = rect();
    // Point à 100 mm de C sur le bord droit BC (origine = fin du segment) : en taille T il est à
    // (400 + dx ; 600 + dy − 100) ; en base (400 ; 500) → décalage (−dx ; −dy).
    let id =
        f.p.apply(Command::AddPointOnSegment {
            sheet: f.sheet,
            segment: f.edges[1],
            distance_mm: 100.0,
            origin: SegmentEnd::End,
        })
        .unwrap()
        .points[0];
    let n =
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(id))
            .unwrap();
    assert_eq!(n.anchor_position, pt(400.0, 500.0));
    let xl = nest_size(&n, &f.p, "XL");
    assert_eq!(xl.anchor_position, pt(410.0, 522.0));
    assert_eq!(xl.shift, (-10.0, -22.0));
    same(
        &outline_of(xl, f.edges[1]).points,
        &[(400.0, -22.0), (400.0, 600.0)],
        "XL BC",
    );
}

// ----- CA2 : la base est la référence, la lecture n'a aucun effet -----------------------------

#[test]
fn ca2_taille_de_base_identique_a_la_geometrie_ecrite() {
    let f = rect();
    for anchor in [
        NestAnchor::Origin,
        NestAnchor::Point(f.corners[0]),
        NestAnchor::Point(f.corners[2]),
    ] {
        let n = f.p.size_nest(f.sheet, f.range, anchor).unwrap();
        let base = nest_size(&n, &f.p, "M");
        assert!(base.is_base);
        assert_eq!(base.shift, (0.0, 0.0));
        let s = f.p.sheet(f.sheet).unwrap();
        for e in f.edges {
            let (a, b) = s.segment_ends(e).unwrap();
            assert_eq!(outline_of(base, e).points, vec![a, b], "{anchor:?}");
        }
    }
}

#[test]
fn ca2_lecture_sans_effet_octets_identiques() {
    let f = rect();
    let before = bytes(&f.p);
    let snapshot = f.p.clone();
    for anchor in [NestAnchor::Origin, NestAnchor::Point(f.corners[2])] {
        f.p.size_nest(f.sheet, f.range, anchor).unwrap();
    }
    assert_eq!(bytes(&f.p), before);
    assert_eq!(f.p, snapshot);
    // Rien du nid n'est écrit dans le fichier.
    assert!(!before.contains("nest"));
    assert!(!before.contains("nid"));
}

// ----- CA3 : erreurs typées -------------------------------------------------------------------

#[test]
fn ca3_point_gamme_et_feuille_inconnus_refuses() {
    let mut f = rect();
    assert_eq!(
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(PointId(987_654))),
        Err(NestError::UnknownPoint(PointId(987_654)))
    );
    assert_eq!(
        f.p.size_nest(f.sheet, SizeRangeId(987_654), NestAnchor::Origin),
        Err(NestError::UnknownRange(SizeRangeId(987_654)))
    );
    assert_eq!(
        f.p.size_nest(SheetId(987_654), f.range, NestAnchor::Origin),
        Err(NestError::UnknownSheet(SheetId(987_654)))
    );
    // Un point d'une autre feuille est inconnu de celle-ci.
    f.p.apply(Command::CreateSheet {
        name: "Autre".into(),
    })
    .unwrap();
    let other = f.p.sheets[1].id;
    let q =
        f.p.apply(Command::AddPoint {
            sheet: other,
            at: pt(1.0, 1.0),
        })
        .unwrap()
        .points[0];
    assert_eq!(
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(q)),
        Err(NestError::UnknownPoint(q))
    );
    // Gamme vide (construite à la main : les commandes n'en créent pas).
    let mut empty = f.p.sizes[0].clone();
    empty.id = SizeRangeId(555);
    empty.sizes.clear();
    f.p.sizes.push(empty);
    assert_eq!(
        f.p.size_nest(f.sheet, SizeRangeId(555), NestAnchor::Origin),
        Err(NestError::EmptyRange(SizeRangeId(555)))
    );
    // Les messages sont des textes, jamais un panic.
    assert!(!NestError::UnknownPoint(q).to_string().is_empty());
}

// ----- CA4 : invariance sous changement de base -----------------------------------------------

#[test]
fn ca4_changement_de_base_contours_relatifs_au_point_identiques() {
    let mut f = rect();
    let anchor = NestAnchor::Point(f.corners[2]);
    let before = f.p.size_nest(f.sheet, f.range, anchor).unwrap();
    let l = size(&f.p, "L");
    f.p.apply(Command::SetBaseSize { size: l }).unwrap();
    let after = f.p.size_nest(f.sheet, f.range, anchor).unwrap();
    assert_eq!(after.base, l);
    // Lu sur la nouvelle base, le nid est le même, à la translation d'ensemble près : relativement
    // au point d'empilement, chaque contour de chaque taille est identique.
    for label in SIZES {
        let (b, a) = (
            nest_size(&before, &f.p, label),
            nest_size(&after, &f.p, label),
        );
        for e in f.edges {
            let pb = &outline_of(b, e).points;
            let pa = &outline_of(a, e).points;
            assert_eq!(pb.len(), pa.len());
            for (u, v) in pb.iter().zip(pa) {
                let ru = (
                    u.x - before.anchor_position.x,
                    u.y - before.anchor_position.y,
                );
                let rv = (v.x - after.anchor_position.x, v.y - after.anchor_position.y);
                assert!(
                    (ru.0 - rv.0).abs() <= 1e-6 && (ru.1 - rv.1).abs() <= 1e-6,
                    "{label} : {ru:?} ≠ {rv:?}"
                );
            }
        }
    }
    // La nouvelle base L est inchangée (référence) et n'est pas translatée.
    let nl = nest_size(&after, &f.p, "L");
    assert!(nl.is_base);
    assert_eq!(nl.shift, (0.0, 0.0));
}

// ----- CA5 : ce qui est lu --------------------------------------------------------------------

#[test]
fn ca5_point_le_plus_haut_pour_l_etiquette() {
    let f = rect();
    let n =
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(f.corners[2]))
            .unwrap();
    // Ancre C : D est le plus à gauche des deux points hauts (y = 600) : D = (−dx ; 600).
    for (label, dx, _) in DELTAS {
        let top = nest_size(&n, &f.p, label).top_point().unwrap();
        assert!(
            (top.x - (-dx)).abs() <= TOL && (top.y - 600.0).abs() <= TOL,
            "{label}"
        );
    }
    let empty = NestSize {
        size: size(&f.p, "M"),
        is_base: true,
        anchor_position: pt(0.0, 0.0),
        shift: (0.0, 0.0),
        outlines: Vec::new(),
        issues: Vec::new(),
    };
    assert_eq!(empty.top_point(), None);
}

#[test]
fn ca5_calque_masque_exclu_et_contour_de_coupe_translate() {
    let mut f = rect();
    // Pièce à marge de 12 mm : le contour de coupe calculé s'ajoute (rectangle élargi de 12 mm).
    let piece =
        f.p.apply(Command::ExtractPiece {
            sheet: f.sheet,
            source: PieceSource::Paths {
                entities: f.edges.to_vec(),
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    f.p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 12.0,
            ..Default::default()
        }),
    })
    .unwrap();
    let n =
        f.p.size_nest(f.sheet, f.range, NestAnchor::Point(f.corners[2]))
            .unwrap();
    for (label, dx, dy) in DELTAS {
        let ns = nest_size(&n, &f.p, label);
        let cut: Vec<&NestOutline> = ns
            .outlines
            .iter()
            .filter(|o| matches!(o.kind, NestOutlineKind::Cut { .. }))
            .collect();
        assert_eq!(cut.len(), 1, "{label}");
        assert!(cut[0].closed);
        // Rectangle de coupe en taille T : de (−12 ; −12) à (412 + dx ; 612 + dy), décalé de
        // (−dx ; −dy) : de (−12 − dx ; −12 − dy) à (412 ; 612), le coin C de coupe est commun.
        let xs: Vec<f64> = cut[0].points.iter().map(|p| p.x).collect();
        let ys: Vec<f64> = cut[0].points.iter().map(|p| p.y).collect();
        let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
        let max = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!((min(&xs) - (-12.0 - dx)).abs() <= 1e-6, "{label} xmin");
        assert!((min(&ys) - (-12.0 - dy)).abs() <= 1e-6, "{label} ymin");
        assert!((max(&xs) - 412.0).abs() <= 1e-6, "{label} xmax");
        assert!((max(&ys) - 612.0).abs() <= 1e-6, "{label} ymax");
    }
    // Un calque masqué n'est pas dessiné dans le nid.
    f.p.apply(Command::AddLayer {
        sheet: f.sheet,
        name: "Aide".into(),
    })
    .unwrap();
    let layer = f.p.sheet(f.sheet).unwrap().layers.last().unwrap().id;
    f.p.apply(Command::SetObjectsLayer {
        sheet: f.sheet,
        points: vec![],
        entities: vec![f.edges[0]],
        layer: Some(layer),
    })
    .unwrap();
    f.p.apply(Command::SetLayerVisible {
        sheet: f.sheet,
        layer,
        visible: false,
    })
    .unwrap();
    let n = f.p.size_nest(f.sheet, f.range, NestAnchor::Origin).unwrap();
    for ns in &n.sizes {
        assert!(!ns.outlines.iter().any(
            |o| matches!(o.kind, NestOutlineKind::Path { entity, .. } if entity == f.edges[0])
        ));
    }
}

//! US-08.3 — contrôle des raccords. Valeurs attendues calculées **à la main** (angles de polygones
//! à sommets entiers ou de sinus / cosinus écrits en littéraux), jamais relues du code testé.
//! CA1 angles et somme, CA2 sens de couture, CA3 statuts et tolérance, CA4 vue assemblée,
//! CA5 lecture en direct, CA6 fichiers (aucun champ nouveau).

use pf_core::contour::{contour_polyline, ContourEdge};
use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn nearp(a: PointMm, x: f64, y: f64, tol: f64) -> bool {
    near(a.x, x, tol) && near(a.y, y, tol)
}

/// Polygone en segments (`ents[k]` va de `v[k]` à `v[k+1]`) extrait en pièce ; les sommets sont
/// donnés dans le sens anti-horaire.
fn poly(p: &mut Project, s: SheetId, v: &[(f64, f64)], name: &str) -> (PieceId, Vec<EntityId>) {
    let n = v.len();
    let ents: Vec<EntityId> = (0..n)
        .map(|k| {
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(v[k].0, v[k].1)),
                to: PointRef::New(pt(v[(k + 1) % n].0, v[(k + 1) % n].1)),
            })
            .unwrap()
            .entities[0]
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: ents.clone(),
            },
            name: name.into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (piece, ents)
}

fn one(p: &Project, piece: PieceId, e: EntityId) -> SeamSide {
    p.seam_side_range(piece, e, 1).unwrap()
}

fn declare(p: &mut Project, a: SeamSide, b: SeamSide) -> SeamPairId {
    p.apply(Command::AddSeamPair {
        a,
        b,
        embu_mm: None,
        tolerance_mm: None,
    })
    .unwrap()
    .seam_pairs[0]
}

/// Même contour parcouru dans l'autre sens (sens horaire), mêmes tracés.
fn make_clockwise(p: &mut Project, piece: PieceId) {
    let outline: Vec<PieceEdge> = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .rev()
        .map(|e| PieceEdge {
            entity: e.entity,
            reversed: !e.reversed,
        })
        .collect();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: None,
    })
    .unwrap();
}

fn point_at(p: &Project, s: SheetId, x: f64, y: f64) -> PointId {
    let sheet = p.sheet(s).unwrap();
    sheet
        .points
        .iter()
        .map(|q| q.id)
        .find(|id| {
            let q = sheet.position(*id).unwrap();
            near(q.x, x, 1e-9) && near(q.y, y, 1e-9)
        })
        .expect("point introuvable")
}

/// A : quadrilatère dont le côté de couture est `(100,0) → (100,50)` ; le sommet précédent de
/// A est `(100 + back.0, back.1)` (donc l'angle intérieur en `(100,0)` vaut `ccw(90°, dir(back))`).
/// B : quadrilatère dont le côté de couture est `(300,50) → (300,0)` ; le sommet suivant de B est
/// `(300 + out.0, out.1)`. Retourne le projet, la feuille, les pièces et le couple.
struct Duo {
    p: Project,
    s: SheetId,
    a: PieceId,
    b: PieceId,
    pair: SeamPairId,
}

fn duo(back: (f64, f64), out: (f64, f64), b_top: (f64, f64)) -> Duo {
    let mut p = Project::new("duo");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[
            (100.0 + back.0, back.1),
            (100.0, 0.0),
            (100.0, 50.0),
            (0.0, 50.0),
        ],
        "Devant",
    );
    let (b, eb) = poly(
        &mut p,
        s,
        &[(300.0, 50.0), (300.0, 0.0), (300.0 + out.0, out.1), b_top],
        "Dos",
    );
    let (sa, sb) = (one(&p, a, ea[1]), one(&p, b, eb[0]));
    let pair = declare(&mut p, sa, sb);
    Duo { p, s, a, b, pair }
}

// Sinus et cosinus calculés à la main (15 chiffres).
const C10: f64 = 0.984_807_753_012_208; // cos 10°
const S10: f64 = 0.173_648_177_666_930; // sin 10°
const C20: f64 = 0.939_692_620_785_908; // cos 20°
const S20: f64 = 0.342_020_143_325_669; // sin 20°

fn rows(p: &Project) -> Vec<JunctionReading> {
    p.seam_junction_report()
}

// ====================================================================== CA1

#[test]
fn ca1_deux_rectangles_somme_180_ecart_0() {
    let mut p = Project::new("rects");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        "A",
    );
    let (b, eb) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (400.0, 0.0), (400.0, 50.0), (300.0, 50.0)],
        "B",
    );
    // A : bord droit (100,0)→(100,50) ; B : bord gauche (300,50)→(300,0) dans le contour.
    let (sa, sb) = (one(&p, a, ea[1]), one(&p, b, eb[3]));
    declare(&mut p, sa, sb);
    let r = rows(&p);
    assert_eq!(r.len(), 2);
    // Début de A (100,0) cousu à la FIN de B (300,0) : contours de même sens, sens opposés.
    assert_eq!(
        (r[0].end, r[0].partner_end),
        (JunctionEnd::Start, JunctionEnd::End)
    );
    assert_eq!(
        (r[1].end, r[1].partner_end),
        (JunctionEnd::End, JunctionEnd::Start)
    );
    assert!(nearp(r[0].point_a.unwrap(), 100.0, 0.0, 1e-9));
    assert!(nearp(r[0].point_b.unwrap(), 300.0, 0.0, 1e-9));
    assert!(nearp(r[1].point_a.unwrap(), 100.0, 50.0, 1e-9));
    assert!(nearp(r[1].point_b.unwrap(), 300.0, 50.0, 1e-9));
    for x in &r {
        assert!(near(x.angle_a_deg.unwrap(), 90.0, 1e-9), "{x:?}");
        assert!(near(x.angle_b_deg.unwrap(), 90.0, 1e-9));
        assert!(near(x.sum_deg.unwrap(), 180.0, 1e-9));
        assert!(near(x.deviation_deg.unwrap(), 0.0, 1e-9));
        assert_eq!(x.status, JunctionStatus::Smooth);
        assert_eq!(x.tolerance_deg, 2.0);
        assert_eq!(x.number, 1);
    }
}

#[test]
fn ca1_encolure_100_plus_70_casse_moins_10() {
    // A en (100,0) : arrivée depuis P0 = (100 − 100 cos10°, −100 sin10°), direction du retour 190°
    // → angle intérieur 190° − 90° = 100°. B en (300,0) : départ vers (300 + 100 cos20°,
    // 100 sin20°), direction 20° ; retour vers (300,50) à 90° → angle 70°.
    let d = duo(
        (-100.0 * C10, -100.0 * S10),
        (100.0 * C20, 100.0 * S20),
        (300.0 + 100.0 * C20, 50.0),
    );
    let r = rows(&d.p);
    assert!(near(r[0].angle_a_deg.unwrap(), 100.0, 1e-9), "{:?}", r[0]);
    assert!(near(r[0].angle_b_deg.unwrap(), 70.0, 1e-9));
    assert!(near(r[0].sum_deg.unwrap(), 170.0, 1e-9));
    assert!(near(r[0].deviation_deg.unwrap(), -10.0, 1e-9));
    assert_eq!(r[0].status, JunctionStatus::Broken);
    // L'autre jonction : (100,50) de A = 90°, (300,50) de B = 90° → lisse.
    assert!(near(r[1].angle_a_deg.unwrap(), 90.0, 1e-9));
    assert!(near(r[1].angle_b_deg.unwrap(), 90.0, 1e-9));
    assert!(near(r[1].deviation_deg.unwrap(), 0.0, 1e-9));
    assert_eq!(r[1].status, JunctionStatus::Smooth);
}

#[test]
fn ca1_bosse_110_plus_90_ecart_plus_20() {
    // Retour de A à 200° (cos200 = −cos20, sin200 = −sin20) → 110° ; B rectangulaire → 90°.
    let d = duo((-100.0 * C20, -100.0 * S20), (100.0, 0.0), (400.0, 50.0));
    let r = rows(&d.p);
    assert!(near(r[0].angle_a_deg.unwrap(), 110.0, 1e-9), "{:?}", r[0]);
    assert!(near(r[0].angle_b_deg.unwrap(), 90.0, 1e-9));
    assert!(near(r[0].sum_deg.unwrap(), 200.0, 1e-9));
    assert!(near(r[0].deviation_deg.unwrap(), 20.0, 1e-9));
    assert_eq!(r[0].status, JunctionStatus::Broken);
}

#[test]
fn ca1_arc_de_cercle_tangente_analytique() {
    // Quart de disque de rayon 100 : la couture est l'arc (100,0) → (0,100). La tangente en
    // (100,0) est verticale et le rayon horizontal : 90° ; en (0,100) : 90° aussi. Avec la corde
    // (45° / 135°) la somme serait fausse : la tangente est bien celle de la courbe.
    let mut p = Project::new("arc");
    let s = p.sheets[0].id;
    let arc = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 100.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    let mut ents = vec![arc];
    for (a, b) in [((0.0, 100.0), (0.0, 0.0)), ((0.0, 0.0), (100.0, 0.0))] {
        ents.push(
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(a.0, a.1)),
                to: PointRef::New(pt(b.0, b.1)),
            })
            .unwrap()
            .entities[0],
        );
    }
    let q = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: ents },
            name: "Quart".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let (t, et) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (457.0, 0.0), (457.0, 40.0), (300.0, 40.0)],
        "Bande",
    );
    let (sa, sb) = (one(&p, q, arc), one(&p, t, et[0]));
    declare(&mut p, sa, sb);
    let r = rows(&p);
    for x in &r {
        assert!(near(x.angle_a_deg.unwrap(), 90.0, 1e-9), "{x:?}");
        assert!(near(x.angle_b_deg.unwrap(), 90.0, 1e-9));
        assert_eq!(x.status, JunctionStatus::Smooth);
    }
    // Segment circulaire (arc + corde) : tangente / corde = 45° aux deux bouts (angle inscrit).
    let mut p = Project::new("segment circulaire");
    let s = p.sheets[0].id;
    let arc = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 100.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    let chord = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 100.0)),
            to: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let seg = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: vec![arc, chord],
            },
            name: "Segment".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    // B : trapèze isocèle dont la petite base (300,0)→(400,0) vaut 135° aux deux bouts.
    let (t, et) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (400.0, 0.0), (500.0, 100.0), (200.0, 100.0)],
        "Trapèze",
    );
    let (sa, sb) = (one(&p, seg, arc), one(&p, t, et[0]));
    declare(&mut p, sa, sb);
    for x in rows(&p) {
        assert!(near(x.angle_a_deg.unwrap(), 45.0, 1e-9), "{x:?}");
        assert!(near(x.angle_b_deg.unwrap(), 135.0, 1e-9));
        assert!(near(x.sum_deg.unwrap(), 180.0, 1e-9));
        assert_eq!(x.status, JunctionStatus::Smooth);
    }
}

#[test]
fn ca1_bezier_tangente_de_la_derivee() {
    // Bézier (100,0) c1 (130,30) c2 (50,130) → (0,100). Tangente de départ (30,30) = 45° ;
    // retour vers (0,0) à 180° : angle 135°. Tangente d'arrivée (0−50, 100−130) = (−50,−30) ;
    // retour = direction (50,30), à atan(30/50) = 30,96375653207352° ; départ vers (0,0) à 270° :
    // angle = 360° + 30,96375653207352° − 270° = 120,96375653207352°.
    let mut p = Project::new("bezier");
    let s = p.sheets[0].id;
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(100.0, 0.0)),
            c1: pt(130.0, 30.0),
            c2: pt(50.0, 130.0),
            end: PointRef::New(pt(0.0, 100.0)),
        })
        .unwrap()
        .entities[0];
    let mut ents = vec![c];
    for (a, b) in [((0.0, 100.0), (0.0, 0.0)), ((0.0, 0.0), (100.0, 0.0))] {
        ents.push(
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(a.0, a.1)),
                to: PointRef::New(pt(b.0, b.1)),
            })
            .unwrap()
            .entities[0],
        );
    }
    let q = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: ents },
            name: "Courbe".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let (t, et) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (457.0, 0.0), (457.0, 40.0), (300.0, 40.0)],
        "Bande",
    );
    let (sa, sb) = (one(&p, q, c), one(&p, t, et[0]));
    declare(&mut p, sa, sb);
    let r = rows(&p);
    assert!(near(r[0].angle_a_deg.unwrap(), 135.0, 1e-9), "{:?}", r[0]);
    assert!(
        near(r[1].angle_a_deg.unwrap(), 120.963_756_532_073_52, 1e-9),
        "{:?}",
        r[1]
    );
}

// ====================================================================== CA2

#[test]
fn ca2_sens_de_couture_inverse_l_appariement_des_extremites() {
    let mut d = duo(
        (-100.0 * C10, -100.0 * S10),
        (100.0 * C20, 100.0 * S20),
        (300.0 + 100.0 * C20, 50.0),
    );
    let same = rows(&d.p);
    // Contours de même sens : début de A (100,0) contre FIN de B (300,0).
    assert_eq!(same[0].partner_end, JunctionEnd::End);
    assert!(nearp(same[0].point_b.unwrap(), 300.0, 0.0, 1e-9));
    // B parcouru dans l'autre sens : sa couture va maintenant de (300,0) à (300,50) ; début
    // contre début. Mêmes angles aux mêmes sommets.
    make_clockwise(&mut d.p, d.b);
    let flipped = rows(&d.p);
    assert_eq!(flipped.len(), 2);
    assert_eq!(flipped[0].partner_end, JunctionEnd::Start);
    assert_eq!(flipped[1].partner_end, JunctionEnd::End);
    assert!(nearp(flipped[0].point_b.unwrap(), 300.0, 0.0, 1e-9));
    assert!(nearp(flipped[1].point_b.unwrap(), 300.0, 50.0, 1e-9));
    for k in 0..2 {
        assert!(
            near(
                flipped[k].angle_b_deg.unwrap(),
                same[k].angle_b_deg.unwrap(),
                1e-9
            ),
            "même angle au même sommet"
        );
        assert!(near(
            flipped[k].sum_deg.unwrap(),
            same[k].sum_deg.unwrap(),
            1e-9
        ));
        assert_eq!(flipped[k].status, same[k].status);
    }
    assert!(near(flipped[0].angle_b_deg.unwrap(), 70.0, 1e-9));
    // A aussi dans l'autre sens : retour à des contours de même sens (opposés de nouveau).
    make_clockwise(&mut d.p, d.a);
    let both = rows(&d.p);
    assert_eq!(both[0].partner_end, JunctionEnd::End);
    // La couture de A va désormais de (100,50) à (100,0) : son début est (100,50), cousu à la
    // fin de B qui est alors (300,50) ; angles inchangés aux mêmes sommets.
    assert!(nearp(both[0].point_a.unwrap(), 100.0, 50.0, 1e-9));
    assert!(nearp(both[0].point_b.unwrap(), 300.0, 50.0, 1e-9));
    assert!(near(both[0].angle_a_deg.unwrap(), 90.0, 1e-9));
    assert!(near(both[1].angle_a_deg.unwrap(), 100.0, 1e-9));
    assert!(near(both[1].angle_b_deg.unwrap(), 70.0, 1e-9));
    assert!(nearp(both[1].point_a.unwrap(), 100.0, 0.0, 1e-9));
}

// ====================================================================== CA3

/// A = 135° en (100,0) : sommet précédent (50,−50). B : départ vers (400,25), direction
/// (100,25) à atan(1/4) = 14,036243467926479° de +X ; retour à 90° → angle 75,96375653207352°.
/// Somme 210,96375653207352° : écart +30,96375653207352° (bosse).
fn limit_duo() -> Duo {
    duo((-50.0, -50.0), (100.0, 25.0), (400.0, 50.0))
}

#[test]
fn ca3_limite_incluse_et_bornes() {
    let d = limit_duo();
    let dev = 30.963_756_532_073_52;
    let r =
        d.p.seam_junction_report_with(30.963_756_532_073_52)
            .unwrap();
    assert!(near(r[0].angle_a_deg.unwrap(), 135.0, 1e-9));
    assert!(near(r[0].angle_b_deg.unwrap(), 75.963_756_532_073_52, 1e-9));
    assert!(near(r[0].deviation_deg.unwrap(), dev, 1e-9));
    assert_eq!(r[0].status, JunctionStatus::Smooth, "limite incluse");
    assert_eq!(r[0].tolerance_deg, 30.963_756_532_073_52);
    let r = d.p.seam_junction_report_with(30.96).unwrap();
    assert_eq!(r[0].status, JunctionStatus::Broken);
    let r = d.p.seam_junction_report_with(90.0).unwrap();
    assert_eq!(r[0].status, JunctionStatus::Smooth);
    // 0° : seule une somme de 180° exacte est lisse (jonction rectangulaire).
    let r = d.p.seam_junction_report_with(0.0).unwrap();
    assert_eq!(r[0].status, JunctionStatus::Broken);
    assert_eq!(r[1].status, JunctionStatus::Smooth, "90° + 90° = 180° à 0°");
}

#[test]
fn ca3_tolerance_invalide_refusee() {
    let d = limit_duo();
    for t in [
        f64::NAN,
        -0.001,
        -1.0,
        90.001,
        91.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        assert!(d.p.seam_junction_report_with(t).is_err(), "{t}");
    }
    assert!(d.p.seam_junction_report_with(0.0).is_ok());
    assert!(d.p.seam_junction_report_with(90.0).is_ok());
    // Le défaut est de 2°.
    assert_eq!(JUNCTION_DEFAULT_TOLERANCE_DEG, 2.0);
    assert_eq!(rows(&d.p)[0].tolerance_deg, 2.0);
}

#[test]
fn ca3_non_calculable_contour_ouvert_avec_raison() {
    let mut d = duo((-100.0, 0.0), (100.0, 0.0), (400.0, 50.0));
    // Contour de B ouvert : un côté non cité par le couple retiré.
    let keep: Vec<PieceEdge> = d.p.piece(d.b).unwrap().outline[..3].to_vec();
    let first = d.p.seams[0].b.edges[0];
    assert!(keep.iter().any(|e| e.entity == first));
    d.p.apply(Command::SetPieceOutline {
        piece: d.b,
        outline: keep,
        grainline: None,
    })
    .unwrap();
    assert_eq!(d.p.seams.len(), 1, "le couple reste déclaré");
    let r = rows(&d.p);
    assert_eq!(r.len(), 2);
    for x in &r {
        assert_eq!(
            x.status,
            JunctionStatus::NotComputable {
                side: SeamWhich::B,
                reason: JunctionIssue::Seam(SeamIssue::OutlineOpen)
            }
        );
        assert!(x.sum_deg.is_none() && x.deviation_deg.is_none());
    }
    assert!(d.p.seam_assembled_view(d.pair).is_err());
}

#[test]
fn ca3_non_calculable_pointe_bord_replie_sur_la_couture() {
    // (0,0) → (100,0) → (40,0) → (40,50) → (0,0) : le bord (0,0)→(100,0) arrive en (100,0) et la
    // couture (100,0)→(40,0) repart en sens inverse : angle de 0° (pointe), non calculable.
    let mut p = Project::new("pointe");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[(0.0, 0.0), (100.0, 0.0), (40.0, 0.0), (40.0, 50.0)],
        "Pointe",
    );
    let (b, eb) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (400.0, 0.0), (400.0, 50.0), (300.0, 50.0)],
        "B",
    );
    let (sa, sb) = (one(&p, a, ea[1]), one(&p, b, eb[0]));
    declare(&mut p, sa, sb);
    let r = rows(&p);
    assert_eq!(
        r[0].status,
        JunctionStatus::NotComputable {
            side: SeamWhich::A,
            reason: JunctionIssue::Cusp
        }
    );
    assert!(r[0].angle_a_deg.is_none() && r[0].angle_b_deg.is_some());
    // L'autre bout est lisible : (40,0) arrive de (100,0), repart vers (40,50).
    assert!(r[1].angle_a_deg.is_some());
}

// ====================================================================== CA4

#[test]
fn ca4_vue_assemblee_translation_et_embu() {
    // A : rectangle 100×50, couture (100,0)→(100,50) (50 mm) ; B : 100×40, couture
    // (300,40)→(300,0) dans le contour (40 mm). Début de A cousu à la fin de B.
    let mut p = Project::new("vue");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        "A",
    );
    let (b, eb) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (400.0, 0.0), (400.0, 40.0), (300.0, 40.0)],
        "B",
    );
    let (sa, sb) = (one(&p, a, ea[1]), one(&p, b, eb[3]));
    let pair = declare(&mut p, sa, sb);
    let v = p.seam_assembled_view(pair).unwrap();
    assert_eq!(v.number, 1);
    assert!(near(v.rotation_deg, 0.0, 1e-9));
    assert!(near(v.chord_a_mm, 50.0, 1e-9) && near(v.chord_b_mm, 40.0, 1e-9));
    // Début : coïncidence à 1e-6 ; fin : écart = 50 − 40 = 10 (embu affiché, jamais corrigé).
    assert!(v.junctions[0].gap_mm < 1e-6);
    assert!(nearp(v.junctions[0].point_a, 100.0, 0.0, 1e-9));
    assert!(nearp(v.junctions[0].point_b, 100.0, 0.0, 1e-6));
    assert!(near(v.end_gap_mm, 10.0, 1e-6));
    assert!(nearp(v.junctions[1].point_a, 100.0, 50.0, 1e-9));
    assert!(nearp(v.junctions[1].point_b, 100.0, 40.0, 1e-6));
    // B posé à droite de A : x de 100 à 200, y de 0 à 40.
    let (xmin, xmax) = v
        .contour_b
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), q| {
            (lo.min(q.x), hi.max(q.x))
        });
    assert!(near(xmin, 100.0, 1e-6) && near(xmax, 200.0, 1e-6));
    assert_eq!(v.contour_a.len(), 4);
    assert_eq!(v.contour_b.len(), 4);
    assert_eq!(v.seam_a.len(), 2);
    assert_eq!(v.seam_b.len(), 2);
    // Les lectures de la vue sont celles du tableau.
    assert_eq!(v.junctions[0].reading, rows(&p)[0]);
}

fn perimeter(poly: &[PointMm]) -> f64 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
        })
        .sum()
}

#[test]
fn ca4_vue_assemblee_rotation_cordes_alignees_longueurs_conservees() {
    // B : 40×100, couture = bord bas (300,0)→(340,0) (0°) ; A : couture montante (90°).
    // Contours de même sens : début de A (100,0) contre fin de B (340,0) ; corde de B de sa fin
    // vers son début = 180° → rotation de 90° − 180° = −90°.
    let mut p = Project::new("vue tournée");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        "A",
    );
    let (b, eb) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (340.0, 0.0), (340.0, 100.0), (300.0, 100.0)],
        "B",
    );
    let (sa, sb) = (one(&p, a, ea[1]), one(&p, b, eb[0]));
    let pair = declare(&mut p, sa, sb);
    let before = bytes(&p);
    let doc = Document::new(p.clone());
    let v = p.seam_assembled_view(pair).unwrap();
    assert!(near(v.rotation_deg, -90.0, 1e-9), "{}", v.rotation_deg);
    assert!(v.junctions[0].gap_mm < 1e-6);
    assert!(nearp(v.junctions[0].point_b, 100.0, 0.0, 1e-6));
    // Fin : corde de A = 50, corde de B = 40 → (100,40).
    assert!(nearp(v.junctions[1].point_b, 100.0, 40.0, 1e-6));
    assert!(near(v.end_gap_mm, 10.0, 1e-6));
    // Cordes alignées à 1e-9 rad.
    let dir = |u: PointMm, w: PointMm| (w.y - u.y).atan2(w.x - u.x);
    let da = dir(v.junctions[0].point_a, v.junctions[1].point_a);
    let db = dir(v.junctions[0].point_b, v.junctions[1].point_b);
    assert!((da - db).abs() < 1e-9, "{da} {db}");
    // Contour de B : (340,100) → (200,0), (300,100) → (200,40) ; périmètre 280 conservé.
    let has = |x: f64, y: f64| v.contour_b.iter().any(|q| nearp(*q, x, y, 1e-6));
    assert!(has(100.0, 0.0) && has(100.0, 40.0) && has(200.0, 0.0) && has(200.0, 40.0));
    assert!(near(perimeter(&v.contour_b), 280.0, 1e-6));
    assert!(near(perimeter(&v.contour_a), 300.0, 1e-6));
    // Lecture seule : octets identiques, aucune étape d'annulation, document intact.
    assert_eq!(bytes(&p), before);
    assert!(!doc.can_undo());
}

#[test]
fn ca4_vue_assemblee_courbe_longueur_conservee() {
    // B : Bézier fermée par un segment ; la polyligne de B (48 pas) a la même longueur avant et
    // après le déplacement rigide, à 1e-6 mm.
    let mut p = Project::new("vue courbe");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        "A",
    );
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(300.0, 0.0)),
            c1: pt(330.0, 40.0),
            c2: pt(380.0, 40.0),
            end: PointRef::New(pt(400.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let base = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(400.0, 0.0)),
            to: PointRef::New(pt(300.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let b = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: vec![c, base],
            },
            name: "Lentille".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let (sa, sb) = (one(&p, a, ea[1]), one(&p, b, base));
    let pair = declare(&mut p, sa, sb);
    let v = p.seam_assembled_view(pair).unwrap();
    let outline: Vec<_> = p
        .piece(b)
        .unwrap()
        .outline
        .iter()
        .map(|e| ContourEdge {
            entity: e.entity,
            reversed: e.reversed,
        })
        .collect();
    let original = contour_polyline(p.sheet(s).unwrap(), &outline, 48).unwrap();
    assert!(
        near(perimeter(&v.contour_b), perimeter(&original), 1e-6),
        "{} {}",
        perimeter(&v.contour_b),
        perimeter(&original)
    );
    assert!(v.junctions[0].gap_mm < 1e-6);
}

#[test]
fn ca4_vue_assemblee_refus_expliques() {
    let d = duo((-100.0, 0.0), (100.0, 0.0), (400.0, 50.0));
    assert!(d.p.seam_assembled_view(SeamPairId(987_654)).is_err());
    // Couture qui fait tout le tour d'un côté : corde nulle, pas de panique.
    let mut p = Project::new("tour");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        "A",
    );
    let (b, eb) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (400.0, 0.0), (400.0, 50.0), (300.0, 50.0)],
        "B",
    );
    let whole_a = p.seam_side_range(a, ea[0], 4).unwrap();
    let sb = p.seam_side_range(b, eb[0], 4).unwrap();
    let pair = declare(&mut p, whole_a, sb);
    assert!(p.seam_assembled_view(pair).is_err());
    // Le tableau reste lisible : 90° + 90° à chaque extrémité.
    for r in rows(&p) {
        assert!(near(r.angle_a_deg.unwrap(), 90.0, 1e-9));
    }
}

// ====================================================================== CA5

#[test]
fn ca5_deplacer_un_point_change_angle_et_statut() {
    let mut p = Project::new("direct");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        "A",
    );
    let (b, eb) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (400.0, 0.0), (400.0, 50.0), (300.0, 50.0)],
        "B",
    );
    let (sa, sb) = (one(&p, a, ea[1]), one(&p, b, eb[3]));
    declare(&mut p, sa, sb);
    assert_eq!(rows(&p)[0].status, JunctionStatus::Smooth);
    // (0,0) → (50,−50) : le bord précédent de (100,0) part à 225° → angle 135°, somme 225°.
    let corner = point_at(&p, s, 0.0, 0.0);
    p.apply(Command::MovePoint {
        sheet: s,
        point: corner,
        to: pt(50.0, -50.0),
    })
    .unwrap();
    let r = rows(&p);
    assert!(near(r[0].angle_a_deg.unwrap(), 135.0, 1e-9), "{:?}", r[0]);
    assert!(near(r[0].deviation_deg.unwrap(), 45.0, 1e-9));
    assert_eq!(r[0].status, JunctionStatus::Broken);
    assert_eq!(
        r[1].status,
        JunctionStatus::Smooth,
        "l'autre jonction n'a pas bougé"
    );
    // De retour : lisse.
    p.apply(Command::MovePoint {
        sheet: s,
        point: corner,
        to: pt(0.0, 0.0),
    })
    .unwrap();
    assert_eq!(rows(&p)[0].status, JunctionStatus::Smooth);
}

#[test]
fn ca5_couple_supprime_ou_piece_separee_ligne_retiree() {
    let mut d = duo((-100.0, 0.0), (100.0, 0.0), (400.0, 50.0));
    assert_eq!(rows(&d.p).len(), 2);
    // Couple supprimé : plus de ligne.
    d.p.apply(Command::RemoveSeamPair { id: d.pair }).unwrap();
    assert!(rows(&d.p).is_empty());
    // Pièce séparée : le couple est retiré (règle d'US-08.1), aucune référence morte.
    let mut d = duo((-100.0, 0.0), (100.0, 0.0), (400.0, 50.0));
    d.p.apply(Command::SplitPiece {
        piece: d.a,
        a: pt(50.0, -10.0),
        b: pt(50.0, 60.0),
    })
    .unwrap();
    assert!(d.p.seams.is_empty());
    assert!(rows(&d.p).is_empty());
    assert!(d.p.seam_assembled_view(d.pair).is_err());
    // Tracé de couture supprimé : couple retiré.
    let mut d = duo((-100.0, 0.0), (100.0, 0.0), (400.0, 50.0));
    let e = d.p.seams[0].b.edges[0];
    d.p.apply(Command::Delete {
        sheet: d.s,
        points: vec![],
        entities: vec![e],
    })
    .unwrap();
    assert!(d.p.seams.is_empty() && rows(&d.p).is_empty());
}

#[test]
fn ca5_meme_sommet_fin_d_une_couture_et_debut_d_une_autre_deux_lignes() {
    // A : rectangle, couture 1 = bord droit (100,0)→(100,50), couture 2 = bord haut
    // (100,50)→(0,50) : le sommet (100,50) est la fin de l'une et le début de l'autre.
    let mut p = Project::new("sommet partagé");
    let s = p.sheets[0].id;
    let (a, ea) = poly(
        &mut p,
        s,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        "A",
    );
    let (b, eb) = poly(
        &mut p,
        s,
        &[(300.0, 0.0), (400.0, 0.0), (400.0, 50.0), (300.0, 50.0)],
        "B",
    );
    let (c, ec) = poly(
        &mut p,
        s,
        &[(600.0, 0.0), (700.0, 0.0), (700.0, 50.0), (600.0, 50.0)],
        "C",
    );
    let (a1, b1) = (one(&p, a, ea[1]), one(&p, b, eb[3]));
    let (a2, c2) = (one(&p, a, ea[2]), one(&p, c, ec[3]));
    declare(&mut p, a1, b1);
    declare(&mut p, a2, c2);
    let r = rows(&p);
    assert_eq!(r.len(), 4);
    assert_eq!(
        (r[0].number, r[1].number, r[2].number, r[3].number),
        (1, 1, 2, 2)
    );
    let (end1, start2) = (&r[1], &r[2]);
    assert_eq!(
        (end1.end, start2.end),
        (JunctionEnd::End, JunctionEnd::Start)
    );
    assert!(nearp(end1.point_a.unwrap(), 100.0, 50.0, 1e-9));
    assert!(nearp(start2.point_a.unwrap(), 100.0, 50.0, 1e-9));
    assert_ne!(end1.pair, start2.pair);
    // Angle de A en (100,50) : 90° dans les deux lignes ; cote B / C : 90° → lisse.
    assert!(near(end1.angle_a_deg.unwrap(), 90.0, 1e-9));
    assert!(near(start2.angle_a_deg.unwrap(), 90.0, 1e-9));
}

// ====================================================================== CA6

#[test]
fn ca6_aucun_champ_nouveau_lecture_sans_ecriture() {
    // Sans couture : octets identiques, rapport vide.
    let mut p = Project::new("sans couture");
    let s = p.sheets[0].id;
    poly(
        &mut p,
        s,
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        "A",
    );
    let before = bytes(&p);
    assert!(rows(&p).is_empty());
    assert_eq!(bytes(&p), before);
    // Avec couture : lectures et vue ne changent aucun octet ; aucun mot nouveau dans le fichier.
    let d = duo(
        (-100.0 * C10, -100.0 * S10),
        (100.0 * C20, 100.0 * S20),
        (300.0 + 100.0 * C20, 50.0),
    );
    let before = bytes(&d.p);
    let _ = rows(&d.p);
    let _ = d.p.seam_junction_report_with(45.0).unwrap();
    let _ = d.p.seam_assembled_view(d.pair).unwrap();
    assert_eq!(bytes(&d.p), before);
    for word in ["junction", "raccord", "angle_a", "assembl"] {
        assert!(!before.contains(word), "{word}");
    }
    let reloaded = persist::from_json(&before).unwrap();
    assert_eq!(bytes(&reloaded), before);
    assert_eq!(rows(&reloaded), rows(&d.p));
}

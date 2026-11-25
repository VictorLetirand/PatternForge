//! US-09.6a — mesures par taille : tableau mesures × tailles lu sur le projet évalué de chaque
//! taille (aucun stockage), évolution entre tailles consécutives, alerte d'irrégularité. Valeurs
//! calculées **à la main** sur un rectangle 400 × 600 en base M, gamme XS S M L XL ; écarts
//! cumulés `S (−5 ; −10)`, `L (+5 ; +10)`, `XL (+10 ; +22)`, `XS (−9 ; −20)` : A reste fixe, B
//! reçoit `(dx ; 0)`, C `(dx ; dy)`, D `(0 ; dy)`, donc la taille `T` est un rectangle
//! `(400 + dx) × (600 + dy)` :
//!
//! | taille | largeur | hauteur | périmètre | aire    | coupe (marge 12) |
//! |--------|---------|---------|-----------|---------|------------------|
//! | XS     | 391     | 580     | 1942      | 226 780 | 2038             |
//! | S      | 395     | 590     | 1970      | 233 050 | 2066             |
//! | M      | 400     | 600     | 2000      | 240 000 | 2096             |
//! | L      | 405     | 610     | 2030      | 247 050 | 2126             |
//! | XL     | 410     | 622     | 2064      | 255 020 | 2160             |
//!
//! Pas du périmètre : 28, 30, 30, 34 (moyenne 30,5) ; de la hauteur : 10, 10, 10, 12.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};

const TOL: f64 = 1e-6;
const SIZES: [&str; 5] = ["XS", "S", "M", "L", "XL"];
const DELTAS: [(&str, f64, f64); 4] = [
    ("XS", -9.0, -20.0),
    ("S", -5.0, -10.0),
    ("L", 5.0, 10.0),
    ("XL", 10.0, 22.0),
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOL
}

fn num(a: f64, b: f64, ctx: &str) {
    assert!(near(a, b), "{ctx} : {a} ≠ {b}");
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
        .find(|q| distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
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

fn grade(p: &mut Project, sheet: SheetId, point: PointId, label: &str, dx: f64, dy: f64) {
    let size = size(p, label);
    p.apply(Command::SetPointGrading {
        sheet,
        point,
        size,
        dx_mm: dx,
        dy_mm: dy,
    })
    .unwrap_or_else(|e| panic!("{label} : {e}"));
}

fn add_range(p: &mut Project, name: &str, labels: &[&str], base: usize) -> SizeRangeId {
    p.apply(Command::AddSizeRange {
        name: name.into(),
        kind: SizeKind::Alphanumeric,
        labels: labels.iter().map(|s| s.to_string()).collect(),
        base,
    })
    .unwrap();
    p.sizes.last().unwrap().id
}

struct Fx {
    p: Project,
    sheet: SheetId,
    range: SizeRangeId,
    piece: PieceId,
    /// AB (bas), BC (droite), CD (haut), DA (gauche).
    edges: [EntityId; 4],
}

/// Rectangle 400 × 600, pièce « Devant » sans marge, coins gradués comme dans l'en-tête.
fn rect() -> Fx {
    let mut p = Project::new("Mesures par taille");
    let sheet = p.sheets[0].id;
    let range = add_range(&mut p, "Femme", &SIZES, 2);
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
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: vec![ab, bc, cd, da],
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let (ib, ic, id) = (
        point_at(&p, sheet, b),
        point_at(&p, sheet, c),
        point_at(&p, sheet, d),
    );
    for (label, dx, dy) in DELTAS {
        grade(&mut p, sheet, ib, label, dx, 0.0);
        grade(&mut p, sheet, ic, label, dx, dy);
        grade(&mut p, sheet, id, label, 0.0, dy);
    }
    Fx {
        p,
        sheet,
        range,
        piece,
        edges: [ab, bc, cd, da],
    }
}

fn row(t: &SizeMeasures, kind: MeasureKind) -> &SizeMeasureRow {
    t.rows
        .iter()
        .find(|r| r.kind == kind)
        .unwrap_or_else(|| panic!("ligne absente : {kind:?}"))
}

fn values(r: &SizeMeasureRow) -> Vec<f64> {
    r.values
        .iter()
        .map(|v| v.expect("valeur lisible"))
        .collect()
}

fn check(got: &[f64], want: &[f64], ctx: &str) {
    assert_eq!(got.len(), want.len(), "{ctx} : nombre de tailles");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        num(*g, *w, &format!("{ctx} [{}]", SIZES[i]));
    }
}

// ----- CA1 : valeurs par taille, calculées à la main ------------------------------------------

#[test]
fn ca1_longueurs_perimetre_et_aire_par_taille_a_la_main() {
    let f = rect();
    let t = f.p.size_measures(f.range).unwrap();
    // Colonnes = tailles de la gamme, dans l'ordre de la gamme.
    let order: Vec<SizeId> = SIZES.iter().map(|l| size(&f.p, l)).collect();
    assert_eq!(t.sizes, order);
    assert_eq!(t.base, size(&f.p, "M"));
    // Largeurs 391, 395, 400, 405, 410 ; hauteurs 580, 590, 600, 610, 622.
    let widths = [391.0, 395.0, 400.0, 405.0, 410.0];
    let heights = [580.0, 590.0, 600.0, 610.0, 622.0];
    let path = |e| MeasureKind::PathLength {
        sheet: f.sheet,
        entity: e,
    };
    check(&values(row(&t, path(f.edges[0]))), &widths, "AB");
    check(&values(row(&t, path(f.edges[1]))), &heights, "BC");
    check(&values(row(&t, path(f.edges[2]))), &widths, "CD");
    check(&values(row(&t, path(f.edges[3]))), &heights, "DA");
    check(
        &values(row(&t, MeasureKind::PiecePerimeter { piece: f.piece })),
        &[1942.0, 1970.0, 2000.0, 2030.0, 2064.0],
        "périmètre",
    );
    check(
        &values(row(&t, MeasureKind::PieceArea { piece: f.piece })),
        &[226_780.0, 233_050.0, 240_000.0, 247_050.0, 255_020.0],
        "aire",
    );
    // 1 périmètre + 1 aire (sans marge : pas de ligne de coupe) + 4 tracés.
    assert_eq!(t.rows.len(), 6);
    assert_eq!(
        row(&t, MeasureKind::PieceArea { piece: f.piece })
            .kind
            .unit(),
        MeasureUnit::Mm2
    );
    assert_eq!(
        row(&t, MeasureKind::PiecePerimeter { piece: f.piece })
            .kind
            .unit(),
        MeasureUnit::Mm
    );
}

#[test]
fn ca1_la_coupe_n_a_une_ligne_que_si_la_piece_a_une_marge() {
    let mut f = rect();
    let t = f.p.size_measures(f.range).unwrap();
    assert!(!t
        .rows
        .iter()
        .any(|r| matches!(r.kind, MeasureKind::PieceCutPerimeter { .. })));
    f.p.apply(Command::SetSeamAllowance {
        piece: f.piece,
        seam: Some(SeamAllowance {
            default_mm: 12.0,
            ..Default::default()
        }),
    })
    .unwrap();
    let t = f.p.size_measures(f.range).unwrap();
    // Coupe = périmètre + 8 × 12 (rectangle, coins à angle droit) : marge jamais graduée.
    check(
        &values(row(&t, MeasureKind::PieceCutPerimeter { piece: f.piece })),
        &[2038.0, 2066.0, 2096.0, 2126.0, 2160.0],
        "coupe",
    );
}

#[test]
fn ca1_la_taille_de_base_donne_les_valeurs_ecrites() {
    let f = rect();
    let t = f.p.size_measures(f.range).unwrap();
    let base = row(&t, MeasureKind::PiecePerimeter { piece: f.piece }).values[2].unwrap();
    // M = base : le périmètre écrit du projet (lecture ordinaire, hors tableau).
    num(
        base,
        f.p.piece_contour(f.piece).unwrap().perimeter_mm,
        "base",
    );
    num(base, 2000.0, "base à la main");
}

// ----- CA2 : évolution entre tailles ----------------------------------------------------------

#[test]
fn ca2_pas_en_mm_et_en_pourcent_et_pas_moyen() {
    let f = rect();
    let t = f.p.size_measures(f.range).unwrap();
    let r = row(&t, MeasureKind::PiecePerimeter { piece: f.piece });
    assert_eq!(r.steps.len(), 4);
    let deltas = [28.0, 30.0, 30.0, 34.0];
    let from = [1942.0, 1970.0, 2000.0, 2030.0];
    for i in 0..4 {
        let s = r.steps[i].expect("pas");
        num(s.delta, deltas[i], "delta");
        num(
            s.percent.unwrap(),
            deltas[i] / from[i] * 100.0,
            "pourcentage",
        );
    }
    num(r.mean_step.unwrap(), 30.5, "pas moyen");
    let mean_pct = (0..4).map(|i| deltas[i] / from[i] * 100.0).sum::<f64>() / 4.0;
    num(r.mean_step_percent.unwrap(), mean_pct, "pourcentage moyen");
    // Hauteur de BC : 580, 590, 600, 610, 622 → 10, 10, 10, 12 (moyenne 10,5).
    let bc = row(
        &t,
        MeasureKind::PathLength {
            sheet: f.sheet,
            entity: f.edges[1],
        },
    );
    let d: Vec<f64> = bc.steps.iter().map(|s| s.unwrap().delta).collect();
    check_steps(&d, &[10.0, 10.0, 10.0, 12.0]);
    num(bc.mean_step.unwrap(), 10.5, "pas moyen BC");
    // Un pas qui descend est négatif : l'ordre est celui de la gamme (jamais un tri).
    let ab = row(
        &t,
        MeasureKind::PathLength {
            sheet: f.sheet,
            entity: f.edges[0],
        },
    );
    let d: Vec<f64> = ab.steps.iter().map(|s| s.unwrap().delta).collect();
    check_steps(&d, &[4.0, 5.0, 5.0, 5.0]);
}

fn check_steps(got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        num(*g, *w, "pas");
    }
}

#[test]
fn ca2_un_tableau_regulier_n_a_aucune_alerte() {
    let f = rect();
    let t = f.p.size_measures(f.range).unwrap();
    assert_eq!(t.irregular_rows().count(), 0);
}

// ----- CA3 : alerte d'irrégularité ------------------------------------------------------------

/// Un segment horizontal de 100 mm (base M) dont l'extrémité reçoit ces écarts en X
/// (XS, S, L, XL) : longueurs XS = 100 + dx…
fn segment_with(dx: [f64; 4]) -> (Project, SizeRangeId, EntityId, SheetId) {
    let mut p = Project::new("Segment");
    let sheet = p.sheets[0].id;
    let range = add_range(&mut p, "Femme", &SIZES, 2);
    let e = seg(&mut p, sheet, pt(0.0, 0.0), pt(100.0, 0.0));
    let end = point_at(&p, sheet, pt(100.0, 0.0));
    for (label, d) in ["XS", "S", "L", "XL"].iter().zip(dx) {
        grade(&mut p, sheet, end, label, d, 0.0);
    }
    (p, range, e, sheet)
}

fn path_row(p: &Project, range: SizeRangeId, sheet: SheetId, e: EntityId) -> SizeMeasureRow {
    let t = p.size_measures(range).unwrap();
    row(&t, MeasureKind::PathLength { sheet, entity: e }).clone()
}

#[test]
fn ca3_pas_regulier_pas_d_alerte() {
    // Longueurs 90, 95, 100, 105, 110 : pas 5, 5, 5, 5.
    let (p, r, e, s) = segment_with([-10.0, -5.0, 5.0, 10.0]);
    let row = path_row(&p, r, s, e);
    check(
        &values(&row),
        &[90.0, 95.0, 100.0, 105.0, 110.0],
        "longueurs",
    );
    assert!(row.irregularity.is_none());
}

#[test]
fn ca3_pas_trop_eloigne_de_la_mediane_est_signale() {
    // Longueurs 90, 98, 100, 108, 120 : pas 8, 2, 8, 12 ; médiane 8, seuil 4 : le pas 2 (écart 6)
    // est signalé ; le pas 12 (écart 4, non supérieur au seuil) ne l'est pas.
    let (p, r, e, s) = segment_with([-10.0, -2.0, 8.0, 20.0]);
    let row = path_row(&p, r, s, e);
    check(
        &values(&row),
        &[90.0, 98.0, 100.0, 108.0, 120.0],
        "longueurs",
    );
    let irr = row.irregularity.clone().expect("alerte");
    assert!(irr.off_median);
    assert!(!irr.sign_change);
    assert_eq!(irr.steps, vec![1]);
}

#[test]
fn ca3_changement_de_signe_est_signale() {
    // Longueurs 100, 90, 100, 110, 120 : pas −10, +10, +10, +10.
    let (p, r, e, s) = segment_with([0.0, -10.0, 10.0, 20.0]);
    let row = path_row(&p, r, s, e);
    check(
        &values(&row),
        &[100.0, 90.0, 100.0, 110.0, 120.0],
        "longueurs",
    );
    let irr = row.irregularity.clone().expect("alerte");
    assert!(irr.sign_change);
    assert!(irr.steps.contains(&0) && irr.steps.contains(&1));
}

#[test]
fn ca3_seuil_exact_de_la_mediane_n_est_pas_signale() {
    // Longueurs 90, 96, 100, 106, 110 : pas 6, 4, 6, 4 ; médiane 5, seuil 2,5 ; écarts 1.
    let (p, r, e, s) = segment_with([-10.0, -4.0, 6.0, 10.0]);
    assert!(path_row(&p, r, s, e).irregularity.is_none());
    // Pas 2, 6, 2, 6 → médiane 4, seuil 2, écarts 2 (non supérieurs) : pas d'alerte (limite).
    // Longueurs 92, 94, 100, 102, 108 → dx = −8, −6, 0, +2, +8 (M = base).
    let (p, r, e, s) = segment_with([-8.0, -6.0, 2.0, 8.0]);
    let row = path_row(&p, r, s, e);
    check(
        &values(&row),
        &[92.0, 94.0, 100.0, 102.0, 108.0],
        "longueurs",
    );
    assert!(row.irregularity.is_none(), "{:?}", row.irregularity);
}

#[test]
fn ca3_moins_de_trois_tailles_ou_valeur_manquante_pas_d_alerte() {
    // La fonction pure : deux valeurs = un seul pas.
    let a = analyze_series(&[Some(100.0), Some(500.0)]);
    assert!(a.irregularity.is_none());
    assert_eq!(a.steps.len(), 1);
    // Une seule taille : aucun pas, aucun pas moyen.
    let a = analyze_series(&[Some(100.0)]);
    assert!(a.steps.is_empty() && a.mean_step.is_none() && a.irregularity.is_none());
    // Trois tailles très irrégulières : alerte.
    let a = analyze_series(&[Some(100.0), Some(90.0), Some(130.0)]);
    assert!(a.irregularity.is_some());
    // Une valeur manquante : pas de pas moyen ni d'alerte, jamais une valeur devinée.
    let a = analyze_series(&[Some(100.0), None, Some(130.0), Some(10.0)]);
    assert!(a.steps[0].is_none() && a.steps[1].is_none() && a.steps[2].is_some());
    assert!(a.mean_step.is_none() && a.irregularity.is_none());
    // Tous les pas nuls : pas d'alerte ; médiane nulle et un pas non nul : alerte.
    assert!(analyze_series(&[Some(5.0); 4]).irregularity.is_none());
    let a = analyze_series(&[Some(5.0), Some(5.0), Some(5.0), Some(8.0)]);
    assert!(a.irregularity.is_some());
    // Gamme de deux tailles dans un projet : jamais d'alerte.
    let mut p = Project::new("Deux tailles");
    let sheet = p.sheets[0].id;
    let range = add_range(&mut p, "Duo", &["S", "M"], 0);
    let e = seg(&mut p, sheet, pt(0.0, 0.0), pt(100.0, 0.0));
    let end = point_at(&p, sheet, pt(100.0, 0.0));
    grade(&mut p, sheet, end, "M", 400.0, 0.0);
    let r = path_row(&p, range, sheet, e);
    check_steps(&values(&r), &[100.0, 500.0]);
    assert!(r.irregularity.is_none());
}

// ----- CA4 : coutures appariées (E08) ---------------------------------------------------------

#[test]
fn ca4_longueurs_de_couture_et_ecart_par_taille() {
    let mut f = rect();
    // Seconde pièce 400 × 600 à droite, non graduée.
    let o =
        f.p.apply(Command::AddRectangle {
            sheet: f.sheet,
            corner: PointRef::New(pt(1000.0, 0.0)),
            width_mm: 400.0,
            height_mm: 600.0,
        })
        .unwrap();
    let second =
        f.p.apply(Command::ExtractPiece {
            sheet: f.sheet,
            source: PieceSource::Paths {
                entities: o.entities.clone(),
            },
            name: "Dos".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    // Côté A : BC du devant (hauteur 600 + dy) ; côté B : un côté vertical du dos (600).
    let vertical = o
        .entities
        .iter()
        .copied()
        .find(|e| {
            f.p.sheet(f.sheet)
                .unwrap()
                .segment_ends(*e)
                .is_ok_and(|(u, v)| (u.x - v.x).abs() < 1e-9 && (u.y - v.y).abs() > 1.0)
        })
        .unwrap();
    let a = f.p.seam_side_range(f.piece, f.edges[1], 1).unwrap();
    let b = f.p.seam_side_range(second, vertical, 1).unwrap();
    f.p.apply(Command::AddSeamPair {
        a,
        b,
        embu_mm: None,
        tolerance_mm: None,
    })
    .unwrap();
    let pair = f.p.seams[0].id;
    let t = f.p.size_measures(f.range).unwrap();
    check(
        &values(row(
            &t,
            MeasureKind::SeamLength {
                pair,
                side: SeamWhich::A,
            },
        )),
        &[580.0, 590.0, 600.0, 610.0, 622.0],
        "couture A",
    );
    check(
        &values(row(
            &t,
            MeasureKind::SeamLength {
                pair,
                side: SeamWhich::B,
            },
        )),
        &[600.0; 5],
        "couture B",
    );
    // Écart A − B : −20, −10, 0, +10, +22 (pas 10, 10, 10, 12 : régulier).
    let gap = row(&t, MeasureKind::SeamGap { pair });
    check(&values(gap), &[-20.0, -10.0, 0.0, 10.0, 22.0], "écart");
    assert!(gap.irregularity.is_none());
    assert_eq!(MeasureKind::SeamGap { pair }.group(), MeasureGroup::Seams);
    // Cohérence avec le rapport E08 de la taille de base (lecture ordinaire).
    let base_gap = f.p.seam_pair_report()[0].embu_measured_mm.unwrap();
    num(gap.values[2].unwrap(), base_gap, "écart de base");
}

// ----- CA5 : refus, lecture sans effet, gammes ------------------------------------------------

#[test]
fn ca5_gamme_inconnue_ou_vide_est_refusee_avec_une_erreur_typee() {
    let mut f = rect();
    assert_eq!(
        f.p.size_measures(SizeRangeId(987_654)),
        Err(SizeMeasuresError::UnknownRange(SizeRangeId(987_654)))
    );
    let before = bytes(&f.p);
    let _ = f.p.size_measures(SizeRangeId(987_654));
    assert_eq!(bytes(&f.p), before);
    let range = f.range;
    f.p.sizes[0].sizes.clear();
    assert_eq!(
        f.p.size_measures(range),
        Err(SizeMeasuresError::EmptyRange(range))
    );
}

#[test]
fn ca5_lecture_sans_effet_octets_identiques() {
    let f = rect();
    let before = bytes(&f.p);
    let copy = f.p.clone();
    let a = f.p.size_measures(f.range).unwrap();
    let b = f.p.size_measures(f.range).unwrap();
    assert_eq!(a, b, "déterministe");
    assert_eq!(bytes(&f.p), before);
    assert_eq!(f.p, copy);
    // Rien de plus dans le fichier : schéma toujours 4, aucune clé de mesures.
    assert!(!before.contains("measure"));
    assert!(before.contains("\"schema_version\": 6"));
}

#[test]
fn ca5_plusieurs_gammes_une_gamme_a_la_fois() {
    let mut f = rect();
    let other = add_range(&mut f.p, "Homme", &["P", "M", "G"], 1);
    let t = f.p.size_measures(other).unwrap();
    assert_eq!(t.sizes.len(), 3);
    // Les écarts de la gamme « Femme » ne touchent pas les tailles de « Homme ».
    check_steps(
        &values(row(&t, MeasureKind::PiecePerimeter { piece: f.piece })),
        &[2000.0; 3],
    );
    let t = f.p.size_measures(f.range).unwrap();
    assert_eq!(t.sizes.len(), 5);
}

// ----- CA6 : invariance sous changement de base -----------------------------------------------

#[test]
fn ca6_changer_la_taille_de_base_ne_change_aucune_valeur() {
    let mut f = rect();
    let before = f.p.size_measures(f.range).unwrap();
    for target in ["L", "XS", "S", "XL", "M"] {
        let id = size(&f.p, target);
        f.p.apply(Command::SetBaseSize { size: id }).unwrap();
        let after = f.p.size_measures(f.range).unwrap();
        assert_eq!(after.base, id);
        assert_eq!(after.rows.len(), before.rows.len(), "base {target}");
        for (r0, r1) in before.rows.iter().zip(&after.rows) {
            assert_eq!(r0.kind, r1.kind);
            for (i, (a, b)) in r0.values.iter().zip(&r1.values).enumerate() {
                num(
                    a.unwrap(),
                    b.unwrap(),
                    &format!("base {target}, taille {i}"),
                );
            }
            assert_eq!(
                r0.irregularity.is_some(),
                r1.irregularity.is_some(),
                "alerte, base {target}"
            );
        }
    }
}

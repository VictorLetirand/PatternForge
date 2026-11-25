//! US-10.3 (retouches nommées) et US-10.4 (mesures finies avec tolérances). Valeurs calculées
//! **à la main**.
//!
//! Fixture : rectangle « Devant » 400 × 600 en base M (gamme XS S M L XL), coins gradués
//! `XS (−9 ; −20)`, `S (−5 ; −10)`, `L (+5 ; +10)`, `XL (+10 ; +22)` : la largeur `AB` vaut
//! 391 / 395 / 400 / 405 / 410, le côté `BC` 580 / 590 / 600 / 610 / 622 ; « Dos » 400 × 600 fixe.
//!
//! | mesure du barème | unité | XS  | S   | M   | L   | XL  |
//! |------------------|-------|-----|-----|-----|-----|-----|
//! | Largeur bas      | mm    | 390 | 395 | 402 | 405 | 415 |
//! | Hauteur côté     | mm    | 580 | 590 | 600 | 610 | 620 |
//! | Angle coin       | deg   | 90  | 90  | 90  | 90  | 90  |
//!
//! Largeur bas (cote `AB`, tolérances −2 / +3) : écarts mesuré − barème = +1, 0, −2, 0, −5 →
//! conforme (bornes incluses : −2 est conforme) sauf XL (−5). Avec la retouche « Largeur −2 »
//! (barème 388, 393, 400, 403, 413) : +3, +2, 0, +2, −3 → conforme (+3 inclus) sauf XL.
//! Hauteur côté (couture `BC` / côté vertical du dos, tolérances 1 / 1) : 0, 0, 0, 0, +2 → hors
//! tolérance en XL seulement. Angle coin (cote d'angle A, 90°, tolérances 0,5 / 0,5) : écart 0.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};

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

struct Fx {
    p: Project,
    sheet: SheetId,
    range: SizeRangeId,
    /// AB (bas), BC (droite), CD (haut), DA (gauche).
    edges: [EntityId; 4],
    largeur: MeasurementId,
    hauteur: MeasurementId,
    angle: MeasurementId,
    /// Cote de longueur de `AB`.
    dim_w: FullDimensionId,
    /// Cote d'angle en A (de B vers D).
    dim_angle: FullDimensionId,
    pair: SeamPairId,
}

fn add_m(p: &mut Project, name: &str, unit: MeasurementUnit, vals: [f64; 5]) -> MeasurementId {
    let m = p
        .apply(Command::AddMeasurement {
            name: name.into(),
            unit,
        })
        .unwrap()
        .measurements[0];
    for (l, v) in SIZES.iter().zip(vals) {
        let size = size(p, l);
        p.apply(Command::SetMeasurementValue {
            measurement: m,
            size,
            value: v,
        })
        .unwrap();
    }
    m
}

fn fx() -> Fx {
    let mut p = Project::new("Retouches");
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
    let (ia, ib, ic, id) = (
        point_at(&p, sheet, a),
        point_at(&p, sheet, b),
        point_at(&p, sheet, c),
        point_at(&p, sheet, d),
    );
    for (label, dx, dy) in DELTAS {
        let size = size(&p, label);
        for (point, x, y) in [(ib, dx, 0.0), (ic, dx, dy), (id, 0.0, dy)] {
            p.apply(Command::SetPointGrading {
                sheet,
                point,
                size,
                dx_mm: x,
                dy_mm: y,
            })
            .unwrap();
        }
    }
    let o = p
        .apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(pt(1000.0, 0.0)),
            width_mm: 400.0,
            height_mm: 600.0,
        })
        .unwrap();
    let dos = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: o.entities.clone(),
            },
            name: "Dos".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let vertical = o
        .entities
        .iter()
        .copied()
        .find(|e| {
            p.sheet(sheet)
                .unwrap()
                .segment_ends(*e)
                .is_ok_and(|(u, v)| (u.x - v.x).abs() < 1e-9 && (u.y - v.y).abs() > 1.0)
        })
        .unwrap();
    let sa = p.seam_side_range(piece, bc, 1).unwrap();
    let sb = p.seam_side_range(dos, vertical, 1).unwrap();
    p.apply(Command::AddSeamPair {
        a: sa,
        b: sb,
        embu_mm: None,
        tolerance_mm: None,
    })
    .unwrap();
    let pair = p.seams[0].id;
    let dim_w = p
        .apply(Command::AddFullDimension {
            sheet,
            kind: FullDimensionKind::PathLength {
                entity: ab,
                offset_mm: 10.0,
            },
            label: None,
            text_shift_mm: 0.0,
        })
        .unwrap()
        .full_dimensions[0];
    let dim_angle = p
        .apply(Command::AddFullDimension {
            sheet,
            kind: FullDimensionKind::Angle {
                vertex: ia,
                first: ib,
                second: id,
                radius_mm: 20.0,
            },
            label: None,
            text_shift_mm: 0.0,
        })
        .unwrap()
        .full_dimensions[0];
    let largeur = add_m(
        &mut p,
        "Largeur bas",
        MeasurementUnit::Mm,
        [390.0, 395.0, 402.0, 405.0, 415.0],
    );
    let hauteur = add_m(
        &mut p,
        "Hauteur côté",
        MeasurementUnit::Mm,
        [580.0, 590.0, 600.0, 610.0, 620.0],
    );
    let angle = add_m(
        &mut p,
        "Angle coin",
        MeasurementUnit::Deg,
        [90.0, 90.0, 90.0, 90.0, 90.0],
    );
    Fx {
        p,
        sheet,
        range,
        edges: [ab, bc, cd, da],
        largeur,
        hauteur,
        angle,
        dim_w,
        dim_angle,
        pair,
    }
}

fn refused(p: &mut Project, c: Command) -> String {
    let before = bytes(p);
    let snapshot = p.clone();
    let e = p.apply(c).expect_err("doit être refusée").to_string();
    assert_eq!(bytes(p), before, "refus sans trace");
    assert_eq!(*p, snapshot);
    e
}

fn add_alt(p: &mut Project, name: &str, target: MeasurementId, delta: f64) -> AlterationId {
    p.apply(Command::AddAlteration {
        name: name.into(),
        target,
        delta,
    })
    .unwrap()
    .alterations[0]
}

fn add_fin(
    p: &mut Project,
    name: &str,
    m: MeasurementId,
    source: FinishedSource,
    tol: (f64, f64),
) -> FinishedMeasureId {
    p.apply(Command::AddFinishedMeasure {
        name: name.into(),
        measurement: m,
        source,
        tolerance_minus: tol.0,
        tolerance_plus: tol.1,
    })
    .unwrap()
    .finished_measures[0]
}

fn dim_source(sheet: SheetId, d: FullDimensionId) -> FinishedSource {
    FinishedSource::FullDimension {
        sheet,
        dimension: d,
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ---------- US-10.3 : commandes ----------

#[test]
fn ca1_ajouter_renommer_cibler_changer_le_delta_supprimer() {
    let mut f = fx();
    let a = add_alt(&mut f.p, "  Manche +15  ", f.largeur, 15.0);
    let r = f.p.alteration(a).unwrap();
    assert_eq!(r.name, "Manche +15", "nom rogné");
    assert_eq!((r.target, r.delta), (f.largeur, 15.0));
    f.p.apply(Command::RenameAlteration {
        alteration: a,
        name: "Manche +16".into(),
    })
    .unwrap();
    f.p.apply(Command::SetAlterationTarget {
        alteration: a,
        target: f.hauteur,
    })
    .unwrap();
    f.p.apply(Command::SetAlterationDelta {
        alteration: a,
        delta: -7.5,
    })
    .unwrap();
    let r = f.p.alteration(a).unwrap();
    assert_eq!(
        (r.name.as_str(), r.target, r.delta),
        ("Manche +16", f.hauteur, -7.5)
    );
    // Ids stables : la deuxième retouche reçoit un autre id, le premier n'est pas réutilisé.
    let b = add_alt(&mut f.p, "Autre", f.angle, 1.0);
    assert_ne!(a, b);
    f.p.apply(Command::RemoveAlteration { alteration: a })
        .unwrap();
    assert_eq!(f.p.alterations.len(), 1);
    assert!(f.p.alteration(a).is_err());
    let c = add_alt(&mut f.p, "Encore", f.angle, 2.0);
    assert!(c != a && c != b, "id jamais réutilisé");
    persist::from_json(&bytes(&f.p)).unwrap();
}

#[test]
fn ca2_refus_sans_trace() {
    let mut f = fx();
    let a = add_alt(&mut f.p, "Manche +15", f.largeur, 15.0);
    let ghost_m = MeasurementId(9_999_999);
    let cases: Vec<Command> = vec![
        Command::AddAlteration {
            name: "".into(),
            target: f.largeur,
            delta: 1.0,
        },
        Command::AddAlteration {
            name: "   ".into(),
            target: f.largeur,
            delta: 1.0,
        },
        Command::AddAlteration {
            name: "MANCHE +15".into(),
            target: f.largeur,
            delta: 1.0,
        },
        Command::AddAlteration {
            name: "x".repeat(500),
            target: f.largeur,
            delta: 1.0,
        },
        Command::AddAlteration {
            name: "Fantôme".into(),
            target: ghost_m,
            delta: 1.0,
        },
        Command::AddAlteration {
            name: "Nan".into(),
            target: f.largeur,
            delta: f64::NAN,
        },
        Command::AddAlteration {
            name: "Inf".into(),
            target: f.largeur,
            delta: f64::INFINITY,
        },
        Command::AddAlteration {
            name: "Énorme".into(),
            target: f.largeur,
            delta: 1_000_000.5,
        },
        Command::RenameAlteration {
            alteration: AlterationId(9_999_999),
            name: "x".into(),
        },
        Command::RenameAlteration {
            alteration: a,
            name: "".into(),
        },
        Command::SetAlterationTarget {
            alteration: a,
            target: ghost_m,
        },
        Command::SetAlterationTarget {
            alteration: AlterationId(9_999_999),
            target: f.largeur,
        },
        Command::SetAlterationDelta {
            alteration: a,
            delta: f64::NEG_INFINITY,
        },
        Command::SetAlterationDelta {
            alteration: a,
            delta: -1_000_001.0,
        },
        Command::RemoveAlteration {
            alteration: AlterationId(9_999_999),
        },
    ];
    for c in cases {
        refused(&mut f.p, c);
    }
    // Deuxième retouche au nom déjà pris (casse ignorée) lors d'un renommage.
    let b = add_alt(&mut f.p, "Autre", f.hauteur, 3.0);
    refused(
        &mut f.p,
        Command::RenameAlteration {
            alteration: b,
            name: "manche +15".into(),
        },
    );
}

#[test]
fn ca3_un_pas_d_annulation_et_aucun_pas_pour_un_no_op() {
    let mut f = fx();
    let a = add_alt(&mut f.p, "Manche", f.largeur, 15.0);
    let mut doc = Document::new(f.p.clone());
    let start = doc.project().clone();
    doc.execute(Command::SetAlterationDelta {
        alteration: a,
        delta: 16.0,
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.project(), start);
    assert!(!doc.can_undo());
    for c in [
        Command::SetAlterationDelta {
            alteration: a,
            delta: 15.0,
        },
        Command::SetAlterationTarget {
            alteration: a,
            target: f.largeur,
        },
        Command::RenameAlteration {
            alteration: a,
            name: "Manche".into(),
        },
    ] {
        doc.execute(c).unwrap();
        assert!(!doc.can_undo(), "no-op sans étape");
    }
}

// ---------- US-10.3 : lecture ----------

#[test]
fn ca4_valeur_retouchee_a_la_main_et_somme_des_retouches() {
    let mut f = fx();
    let m = size(&f.p, "M");
    // Sans retouche : valeur du barème.
    let v = f.p.altered_measurement_value(f.largeur, m).unwrap();
    assert_eq!(
        (v.base, v.altered, v.count, v.issue),
        (Some(402.0), Some(402.0), 0, None)
    );
    add_alt(&mut f.p, "Largeur -2", f.largeur, -2.0);
    add_alt(&mut f.p, "Largeur +10", f.largeur, 10.0);
    // 402 − 2 + 10 = 410 pour M ; XS : 390 + 8 = 398.
    let v = f.p.altered_measurement_value(f.largeur, m).unwrap();
    assert_eq!((v.base, v.altered, v.count), (Some(402.0), Some(410.0), 2));
    assert!(near(v.delta, 8.0));
    let xs = size(&f.p, "XS");
    assert_eq!(
        f.p.altered_measurement_value(f.largeur, xs)
            .unwrap()
            .altered,
        Some(398.0)
    );
    // Une mesure non ciblée est intacte.
    assert_eq!(
        f.p.altered_measurement_value(f.hauteur, m).unwrap().altered,
        Some(600.0)
    );
    // Le rapport par retouche.
    let rep = f.p.alteration_report(m).unwrap();
    assert_eq!(rep.len(), 2);
    assert!(rep
        .iter()
        .all(|r| r.status == AlterationStatus::Applied && r.altered == Some(410.0)));
    // Taille inconnue, mesure inconnue : refus de lecture.
    assert!(f.p.alteration_report(SizeId(9_999_999)).is_err());
    assert!(f
        .p
        .altered_measurement_value(MeasurementId(9_999_999), m)
        .is_err());
}

#[test]
fn ca5_la_lecture_ne_modifie_ni_le_barene_ni_la_geometrie_de_base() {
    let mut f = fx();
    let before_sheets = f.p.sheets.clone();
    let before_measurements = f.p.measurements.clone();
    add_alt(&mut f.p, "Largeur +15", f.largeur, 15.0);
    assert_eq!(f.p.sheets, before_sheets, "géométrie de base inchangée");
    assert_eq!(
        f.p.measurements, before_measurements,
        "barème stocké inchangé"
    );
    // Octets : seule la section `alterations` s'ajoute.
    let with = bytes(&f.p);
    let mut without = f.p.clone();
    without.alterations.clear();
    let json_without = bytes(&without);
    assert!(with.contains("\"alterations\""));
    assert!(!json_without.contains("\"alterations\""));
    let m = size(&f.p, "M");
    for l in SIZES {
        let s = size(&f.p, l);
        let g = f.p.graded_project(Some(s)).unwrap();
        let plain = without.graded_project(Some(s)).unwrap();
        // Géométrie lue identique avec et sans retouche (les retouches ne touchent jamais le tracé).
        assert_eq!(g.sheets, plain.sheets, "{l} : géométrie");
        // Barème de la copie lue : seule la valeur de la taille lue est retouchée.
        let lue = g
            .measurement(f.largeur)
            .unwrap()
            .values
            .iter()
            .find(|v| v.size == s)
            .map(|v| v.value);
        let stored =
            f.p.measurement(f.largeur)
                .unwrap()
                .values
                .iter()
                .find(|v| v.size == s)
                .unwrap()
                .value;
        assert_eq!(
            lue,
            Some(stored + 15.0),
            "{l} : valeur retouchée dans la copie lue"
        );
        let autre = g
            .measurement(f.hauteur)
            .unwrap()
            .values
            .iter()
            .find(|v| v.size == s)
            .map(|v| v.value);
        assert_eq!(
            autre,
            f.p.measurement(f.hauteur)
                .unwrap()
                .values
                .iter()
                .find(|v| v.size == s)
                .map(|v| v.value)
        );
    }
    let _ = m;
    // Le projet lui-même n'a pas bougé.
    assert_eq!(f.p.measurements, before_measurements);
}

#[test]
fn ca6_ordre_d_application_apres_gradation_valeurs_par_taille_et_crans() {
    // Les retouches s'appliquent en dernier : la géométrie graduée (écarts, valeurs de variables
    // par taille) est celle d'avant ; seule la valeur de barème change. Calcul à la main : en XL la
    // largeur graduée vaut 410 (400 + 10) avec ou sans retouche, la référence passe de 415 à 430.
    let mut f = fx();
    let xl = size(&f.p, "XL");
    add_alt(&mut f.p, "Largeur +15", f.largeur, 15.0);
    let g = f.p.graded_project(Some(xl)).unwrap();
    let len = g
        .sheet(f.sheet)
        .unwrap()
        .path_length_mm(f.edges[0])
        .unwrap();
    assert!(near(len, 410.0), "largeur graduée XL = {len}");
    let v = f.p.altered_measurement_value(f.largeur, xl).unwrap();
    assert_eq!((v.base, v.altered), (Some(415.0), Some(430.0)));
}

#[test]
fn ca7_taille_sans_valeur_ou_hors_bornes_jamais_devinee() {
    let mut f = fx();
    let l = size(&f.p, "L");
    f.p.apply(Command::ClearMeasurementValue {
        measurement: f.largeur,
        size: l,
    })
    .unwrap();
    let a = add_alt(&mut f.p, "Largeur +15", f.largeur, 15.0);
    let v = f.p.altered_measurement_value(f.largeur, l).unwrap();
    assert_eq!(
        (v.base, v.altered, v.issue),
        (None, None, Some(AlterationIssue::NoBaseValue))
    );
    let rep = f.p.alteration_report(l).unwrap();
    assert_eq!(rep[0].status, AlterationStatus::NoBaseValue);
    // Hors bornes : 390 − 400 < 0 en XS → pas de valeur retouchée (jamais négative).
    f.p.apply(Command::SetAlterationDelta {
        alteration: a,
        delta: -400.0,
    })
    .unwrap();
    let xs = size(&f.p, "XS");
    let v = f.p.altered_measurement_value(f.largeur, xs).unwrap();
    assert_eq!(
        (v.altered, v.issue),
        (None, Some(AlterationIssue::OutOfRange))
    );
    assert_eq!(
        f.p.alteration_report(xs).unwrap()[0].status,
        AlterationStatus::OutOfRange
    );
    // Dans la copie lue, la valeur impossible est retirée (jamais présentée comme retouchée).
    let g = f.p.graded_project(Some(xs)).unwrap();
    assert!(g
        .measurement(f.largeur)
        .unwrap()
        .values
        .iter()
        .all(|e| e.size != xs));
    // Une somme qui revient dans les bornes est acceptée : +400 −400 = 0.
    add_alt(&mut f.p, "Compense", f.largeur, 400.0);
    assert_eq!(
        f.p.altered_measurement_value(f.largeur, xs)
            .unwrap()
            .altered,
        Some(390.0)
    );
}

#[test]
fn ca8_cible_disparue_retire_la_retouche_avec_remarque() {
    let mut f = fx();
    add_alt(&mut f.p, "Largeur +15", f.largeur, 15.0);
    let keep = add_alt(&mut f.p, "Hauteur +1", f.hauteur, 1.0);
    let out =
        f.p.apply(Command::RemoveMeasurement {
            measurement: f.largeur,
        })
        .unwrap();
    assert_eq!(f.p.alterations.len(), 1);
    assert_eq!(f.p.alterations[0].id, keep);
    assert!(out.notes.iter().any(|n| n.contains("1 retouche(s)")));
    // Projet construit à la main avec une cible morte : la lecture le signale (pas de panique).
    let mut broken = f.p.clone();
    broken.alterations[0].target = MeasurementId(9_999_999);
    let m = size(&broken, "M");
    let rep = broken.alteration_report(m).unwrap();
    assert_eq!(rep[0].status, AlterationStatus::DeadTarget);
    // …et l'enregistrement le refuse (référence morte), le projet sain se relit.
    assert!(persist::to_json(&broken).is_err());
    let json = persist::to_json(&f.p).unwrap();
    persist::from_json(&json).unwrap();
}

// ---------- US-10.4 : commandes ----------

#[test]
fn cb1_ajouter_modifier_supprimer_une_mesure_finie() {
    let mut f = fx();
    let src = dim_source(f.sheet, f.dim_w);
    let id = add_fin(&mut f.p, "  Largeur finie  ", f.largeur, src, (2.0, 3.0));
    let r = f.p.finished_measure(id).unwrap();
    assert_eq!(r.name, "Largeur finie");
    assert_eq!((r.tolerance_minus, r.tolerance_plus), (2.0, 3.0));
    f.p.apply(Command::SetFinishedMeasure {
        finished: id,
        name: "Hauteur finie".into(),
        measurement: f.hauteur,
        source: FinishedSource::Seam { seam: f.pair },
        tolerance_minus: 1.0,
        tolerance_plus: 1.5,
    })
    .unwrap();
    let r = f.p.finished_measure(id).unwrap();
    assert_eq!(r.name, "Hauteur finie");
    assert_eq!(r.measurement, f.hauteur);
    assert_eq!(r.source, FinishedSource::Seam { seam: f.pair });
    assert_eq!((r.tolerance_minus, r.tolerance_plus), (1.0, 1.5));
    f.p.apply(Command::RemoveFinishedMeasure { finished: id })
        .unwrap();
    assert!(f.p.finished_measures.is_empty());
    let id2 = add_fin(
        &mut f.p,
        "Autre",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (0.0, 0.0),
    );
    assert_ne!(id, id2, "id jamais réutilisé");
    persist::from_json(&bytes(&f.p)).unwrap();
}

#[test]
fn cb2_refus_sans_trace() {
    let mut f = fx();
    let src = dim_source(f.sheet, f.dim_w);
    let id = add_fin(&mut f.p, "Largeur finie", f.largeur, src, (2.0, 3.0));
    let ghost_s = SeamPairId(9_999_999);
    let add = |name: &str, m, source, tm, tp| Command::AddFinishedMeasure {
        name: name.into(),
        measurement: m,
        source,
        tolerance_minus: tm,
        tolerance_plus: tp,
    };
    let cases: Vec<Command> = vec![
        add("", f.largeur, src, 1.0, 1.0),
        add("LARGEUR FINIE", f.largeur, src, 1.0, 1.0),
        add("x".repeat(500).as_str(), f.largeur, src, 1.0, 1.0),
        add("Fantôme", MeasurementId(9_999_999), src, 1.0, 1.0),
        add(
            "Couture fantôme",
            f.hauteur,
            FinishedSource::Seam { seam: ghost_s },
            1.0,
            1.0,
        ),
        add(
            "Cote fantôme",
            f.largeur,
            FinishedSource::FullDimension {
                sheet: f.sheet,
                dimension: FullDimensionId(9_999_999),
            },
            1.0,
            1.0,
        ),
        add(
            "Feuille fantôme",
            f.largeur,
            FinishedSource::FullDimension {
                sheet: SheetId(9_999_999),
                dimension: f.dim_w,
            },
            1.0,
            1.0,
        ),
        // Unités : longueur en degrés, angle en mm, couture en degrés.
        add("Longueur en deg", f.angle, src, 1.0, 1.0),
        add(
            "Angle en mm",
            f.largeur,
            dim_source(f.sheet, f.dim_angle),
            1.0,
            1.0,
        ),
        add(
            "Couture en deg",
            f.angle,
            FinishedSource::Seam { seam: f.pair },
            1.0,
            1.0,
        ),
        add("Tol négative", f.largeur, src, -0.1, 1.0),
        add("Tol négative +", f.largeur, src, 1.0, -0.1),
        add("Tol NaN", f.largeur, src, f64::NAN, 1.0),
        add("Tol inf", f.largeur, src, 1.0, f64::INFINITY),
        add("Tol énorme", f.largeur, src, 1.0, 1_000_001.0),
        Command::SetFinishedMeasure {
            finished: FinishedMeasureId(9_999_999),
            name: "x".into(),
            measurement: f.largeur,
            source: src,
            tolerance_minus: 1.0,
            tolerance_plus: 1.0,
        },
        Command::SetFinishedMeasure {
            finished: id,
            name: "".into(),
            measurement: f.largeur,
            source: src,
            tolerance_minus: 1.0,
            tolerance_plus: 1.0,
        },
        Command::SetFinishedMeasure {
            finished: id,
            name: "Largeur finie".into(),
            measurement: f.angle,
            source: src,
            tolerance_minus: 1.0,
            tolerance_plus: 1.0,
        },
        Command::RemoveFinishedMeasure {
            finished: FinishedMeasureId(9_999_999),
        },
    ];
    for c in cases {
        refused(&mut f.p, c);
    }
    let src2 = FinishedSource::Seam { seam: f.pair };
    let b = add_fin(&mut f.p, "Hauteur finie", f.hauteur, src2, (1.0, 1.0));
    refused(
        &mut f.p,
        Command::SetFinishedMeasure {
            finished: b,
            name: "largeur finie".into(),
            measurement: f.hauteur,
            source: src2,
            tolerance_minus: 1.0,
            tolerance_plus: 1.0,
        },
    );
}

#[test]
fn cb3_un_pas_d_annulation_et_aucun_pas_pour_un_no_op() {
    let mut f = fx();
    let src = dim_source(f.sheet, f.dim_w);
    let id = add_fin(&mut f.p, "Largeur finie", f.largeur, src, (2.0, 3.0));
    let mut doc = Document::new(f.p.clone());
    let start = doc.project().clone();
    let set = |tp: f64| Command::SetFinishedMeasure {
        finished: id,
        name: "Largeur finie".into(),
        measurement: f.largeur,
        source: src,
        tolerance_minus: 2.0,
        tolerance_plus: tp,
    };
    doc.execute(set(4.0)).unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.project(), start, "une seule annulation");
    assert!(!doc.can_undo());
    doc.execute(set(3.0)).unwrap();
    assert!(!doc.can_undo(), "mêmes valeurs : aucune étape");
}

// ---------- US-10.4 : lecture ----------

fn status_row(t: &FinishedTable, i: usize) -> Vec<FinishedStatus> {
    t.columns.iter().map(|c| c[i].status).collect()
}

#[test]
fn cb4_largeur_par_taille_et_sur_la_base_a_la_main() {
    let mut f = fx();
    let src = dim_source(f.sheet, f.dim_w);
    add_fin(&mut f.p, "Largeur finie", f.largeur, src, (2.0, 3.0));
    let t = f.p.finished_measure_table(f.range, false).unwrap();
    assert_eq!(t.sizes.len(), 5);
    let measured = [391.0, 395.0, 400.0, 405.0, 410.0];
    let reference = [390.0, 395.0, 402.0, 405.0, 415.0];
    let gaps = [1.0, 0.0, -2.0, 0.0, -5.0];
    for (i, col) in t.columns.iter().enumerate() {
        let r = &col[0];
        assert_eq!(r.measured, Some(measured[i]), "{} mesuré", SIZES[i]);
        assert_eq!(r.reference, Some(reference[i]), "{} référence", SIZES[i]);
        assert_eq!(r.gap, Some(gaps[i]), "{} écart", SIZES[i]);
        assert_eq!(r.reason, None);
        assert_eq!(r.alteration_delta, 0.0);
    }
    use FinishedStatus::*;
    // −2 est la borne basse incluse : conforme ; −5 est hors tolérance.
    assert_eq!(
        status_row(&t, 0),
        vec![
            Conforming,
            Conforming,
            Conforming,
            Conforming,
            OutOfTolerance
        ]
    );
    // Sur la base : la colonne de la taille de base (M) = géométrie écrite.
    let base =
        f.p.finished_measure_readings(Some(size(&f.p, "M")), false)
            .unwrap();
    assert_eq!(base[0].measured, Some(400.0));
    assert_eq!(base[0].gap, Some(-2.0));
    // Le projet n'a pas bougé (lecture pure).
    let before = bytes(&f.p);
    let _ = f.p.finished_measure_table(f.range, true).unwrap();
    assert_eq!(bytes(&f.p), before);
}

#[test]
fn cb5_couture_cote_d_angle_et_cote_horizontale_en_valeur_absolue() {
    let mut f = fx();
    add_fin(
        &mut f.p,
        "Hauteur finie",
        f.hauteur,
        FinishedSource::Seam { seam: f.pair },
        (1.0, 1.0),
    );
    add_fin(
        &mut f.p,
        "Angle fini",
        f.angle,
        dim_source(f.sheet, f.dim_angle),
        (0.5, 0.5),
    );
    // Cote horizontale de B vers A : ΔX = −largeur (signée) ; la lecture prend la valeur absolue.
    let sheet = f.sheet;
    let (ib, ia) = (
        point_at(&f.p, sheet, pt(400.0, 0.0)),
        point_at(&f.p, sheet, pt(0.0, 0.0)),
    );
    let dh =
        f.p.apply(Command::AddFullDimension {
            sheet,
            kind: FullDimensionKind::Horizontal {
                from: ib,
                to: ia,
                offset_mm: 10.0,
            },
            label: None,
            text_shift_mm: 0.0,
        })
        .unwrap()
        .full_dimensions[0];
    assert!(
        f.p.sheet(sheet)
            .unwrap()
            .full_dimension_geometry(dh)
            .unwrap()
            .value
            < 0.0
    );
    add_fin(
        &mut f.p,
        "Largeur H",
        f.largeur,
        dim_source(f.sheet, dh),
        (2.0, 3.0),
    );
    let t = f.p.finished_measure_table(f.range, false).unwrap();
    use FinishedStatus::*;
    // Hauteur côté : côté A = BC (580, 590, 600, 610, 622) contre 580, 590, 600, 610, 620.
    let h: Vec<Option<f64>> = t.columns.iter().map(|c| c[0].measured).collect();
    assert_eq!(
        h,
        vec![
            Some(580.0),
            Some(590.0),
            Some(600.0),
            Some(610.0),
            Some(622.0)
        ]
    );
    assert_eq!(
        status_row(&t, 0),
        vec![
            Conforming,
            Conforming,
            Conforming,
            Conforming,
            OutOfTolerance
        ]
    );
    assert_eq!(t.columns[4][0].gap, Some(2.0));
    // Angle : 90° partout, écart 0.
    assert!(t
        .columns
        .iter()
        .all(|c| c[1].measured == Some(90.0) && c[1].gap == Some(0.0)));
    assert_eq!(status_row(&t, 1), vec![Conforming; 5]);
    // Horizontale : valeurs absolues 391…410 (mêmes que la longueur de AB).
    let w: Vec<Option<f64>> = t.columns.iter().map(|c| c[2].measured).collect();
    assert_eq!(
        w,
        vec![
            Some(391.0),
            Some(395.0),
            Some(400.0),
            Some(405.0),
            Some(410.0)
        ]
    );
}

#[test]
fn cb6_valeur_du_bareme_absente_non_comparable_jamais_devinee() {
    let mut f = fx();
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    let l = size(&f.p, "L");
    f.p.apply(Command::ClearMeasurementValue {
        measurement: f.largeur,
        size: l,
    })
    .unwrap();
    let t = f.p.finished_measure_table(f.range, false).unwrap();
    let r = &t.columns[3][0];
    assert_eq!(r.status, FinishedStatus::NotComparable);
    assert_eq!(r.reason, Some(FinishedReason::NoReferenceValue));
    assert_eq!((r.reference, r.gap), (None, None));
    assert_eq!(r.measured, Some(405.0), "la valeur mesurée reste lue");
    // Les autres tailles ne sont pas affectées.
    assert_eq!(t.columns[2][0].status, FinishedStatus::Conforming);
    // Sans aucune taille (géométrie écrite, pas de gamme) : jamais de référence.
    let mut none = f.p.clone();
    none.apply(Command::RemoveSizeRange { range: f.range })
        .unwrap();
    let rs = none.finished_measure_readings(None, true).unwrap();
    assert_eq!(rs[0].status, FinishedStatus::NotComparable);
    assert_eq!(rs[0].reason, Some(FinishedReason::NoReferenceValue));
    assert!(none.finished_measure_base_readings()[0].reference.is_none());
}

#[test]
fn cb7_source_degeneree_non_comparable() {
    let mut f = fx();
    // Segment P(2000,0)→Q(2100,0), Q gradué de −100 en XS : le tracé est réduit à un point.
    let sheet = f.sheet;
    let e = seg(&mut f.p, sheet, pt(2000.0, 0.0), pt(2100.0, 0.0));
    let q = point_at(&f.p, sheet, pt(2100.0, 0.0));
    let xs = size(&f.p, "XS");
    f.p.apply(Command::SetPointGrading {
        sheet,
        point: q,
        size: xs,
        dx_mm: -100.0,
        dy_mm: 0.0,
    })
    .unwrap();
    let d =
        f.p.apply(Command::AddFullDimension {
            sheet,
            kind: FullDimensionKind::PathLength {
                entity: e,
                offset_mm: 5.0,
            },
            label: None,
            text_shift_mm: 0.0,
        })
        .unwrap()
        .full_dimensions[0];
    add_fin(
        &mut f.p,
        "Court",
        f.largeur,
        dim_source(f.sheet, d),
        (2.0, 3.0),
    );
    let t = f.p.finished_measure_table(f.range, false).unwrap();
    let r = &t.columns[0][0];
    assert_eq!(r.status, FinishedStatus::NotComparable, "{r:?}");
    assert_eq!(r.reason, Some(FinishedReason::SourceDegenerate));
    assert_eq!(r.measured, None);
    // Les autres tailles mesurent bien 100 mm (écart −290 …).
    assert_eq!(t.columns[2][0].measured, Some(100.0));
    assert_eq!(t.columns[2][0].gap, Some(100.0 - 402.0));
    assert_eq!(t.columns[2][0].status, FinishedStatus::OutOfTolerance);
}

#[test]
fn cb8_unites_differentes_non_comparable_apres_changement_de_la_mesure() {
    // Un fichier peut avoir une unité incohérente (la validation ne la contrôle pas) : lecture
    // « non comparable », jamais un calcul mélangeant mm et degrés.
    let mut f = fx();
    let id = add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    let mut p = f.p.clone();
    p.finished_measures
        .iter_mut()
        .find(|x| x.id == id)
        .unwrap()
        .measurement = f.angle;
    let r = &p
        .finished_measure_readings(Some(size(&p, "M")), false)
        .unwrap()[0];
    assert_eq!(r.status, FinishedStatus::NotComparable);
    assert_eq!(r.reason, Some(FinishedReason::UnitMismatch));
    assert_eq!(r.measured, None);
}

#[test]
fn cb9_avec_retouche_la_reference_change_et_l_ecart_est_signale() {
    let mut f = fx();
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    add_alt(&mut f.p, "Largeur -2", f.largeur, -2.0);
    let sans = f.p.finished_measure_table(f.range, false).unwrap();
    let avec = f.p.finished_measure_table(f.range, true).unwrap();
    // Sans retouche : barème brut. Avec : 388, 393, 400, 403, 413.
    let refs: Vec<Option<f64>> = avec.columns.iter().map(|c| c[0].reference).collect();
    assert_eq!(
        refs,
        vec![
            Some(388.0),
            Some(393.0),
            Some(400.0),
            Some(403.0),
            Some(413.0)
        ]
    );
    let raw: Vec<Option<f64>> = sans.columns.iter().map(|c| c[0].reference).collect();
    assert_eq!(
        raw,
        vec![
            Some(390.0),
            Some(395.0),
            Some(402.0),
            Some(405.0),
            Some(415.0)
        ]
    );
    let gaps: Vec<Option<f64>> = avec.columns.iter().map(|c| c[0].gap).collect();
    assert_eq!(
        gaps,
        vec![Some(3.0), Some(2.0), Some(0.0), Some(2.0), Some(-3.0)]
    );
    use FinishedStatus::*;
    // +3 est la borne haute incluse : conforme ; −3 est hors tolérance (−2 au plus).
    assert_eq!(
        status_row(&avec, 0),
        vec![
            Conforming,
            Conforming,
            Conforming,
            Conforming,
            OutOfTolerance
        ]
    );
    assert!(avec.columns.iter().all(|c| c[0].alteration_delta == -2.0));
    assert!(sans.columns.iter().all(|c| c[0].alteration_delta == 0.0));
    // Le rapport de contrôle lit la base de la première gamme (M) retouches comprises.
    let base = f.p.finished_measure_base_readings();
    assert_eq!(base[0].reference, Some(400.0));
    assert_eq!(base[0].size_label.as_deref(), Some("M"));
}

#[test]
fn cb10_retouche_hors_bornes_rend_la_mesure_non_comparable() {
    let mut f = fx();
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    add_alt(&mut f.p, "Trop", f.largeur, -395.0);
    let t = f.p.finished_measure_table(f.range, true).unwrap();
    // XS : 390 − 395 < 0 → hors bornes ; S : 395 − 395 = 0 (borne incluse), écart 395 − 0.
    assert_eq!(
        t.columns[0][0].reason,
        Some(FinishedReason::AlterationOutOfRange)
    );
    assert_eq!(t.columns[0][0].status, FinishedStatus::NotComparable);
    assert_eq!(t.columns[1][0].reference, Some(0.0));
    assert_eq!(t.columns[1][0].gap, Some(395.0));
    assert_eq!(t.columns[1][0].status, FinishedStatus::OutOfTolerance);
}

// ---------- US-10.4 : références ----------

#[test]
fn cb11_couture_cote_feuille_ou_mesure_supprimee_retire_la_mesure_finie() {
    // Couture supprimée.
    let mut f = fx();
    add_fin(
        &mut f.p,
        "Hauteur finie",
        f.hauteur,
        FinishedSource::Seam { seam: f.pair },
        (1.0, 1.0),
    );
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    let out = f.p.apply(Command::RemoveSeamPair { id: f.pair }).unwrap();
    assert_eq!(f.p.finished_measures.len(), 1);
    assert_eq!(f.p.finished_measures[0].name, "Largeur finie");
    assert!(
        out.notes.iter().any(|n| n.contains("Hauteur finie")),
        "{:?}",
        out.notes
    );
    persist::from_json(&bytes(&f.p)).unwrap();
    // Cote supprimée directement.
    let out =
        f.p.apply(Command::DeleteFullDimension {
            sheet: f.sheet,
            dimension: f.dim_w,
        })
        .unwrap();
    assert!(f.p.finished_measures.is_empty());
    assert!(out.notes.iter().any(|n| n.contains("Largeur finie")));
    persist::from_json(&bytes(&f.p)).unwrap();
    // Tracé mesuré supprimé : la cote disparaît, la mesure finie aussi (même étape).
    let mut f = fx();
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    f.p.apply(Command::Delete {
        sheet: f.sheet,
        points: vec![],
        entities: vec![f.edges[0]],
    })
    .unwrap();
    assert!(f.p.finished_measures.is_empty());
    persist::from_json(&bytes(&f.p)).unwrap();
    // Feuille supprimée.
    let mut f = fx();
    let second =
        f.p.apply(Command::CreateSheet {
            name: "Seconde".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let a =
        f.p.apply(Command::AddSegment {
            sheet: second,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(50.0, 0.0)),
        })
        .unwrap();
    let d =
        f.p.apply(Command::AddFullDimension {
            sheet: second,
            kind: FullDimensionKind::PathLength {
                entity: a.entities[0],
                offset_mm: 5.0,
            },
            label: None,
            text_shift_mm: 0.0,
        })
        .unwrap()
        .full_dimensions[0];
    add_fin(
        &mut f.p,
        "Sur seconde",
        f.largeur,
        FinishedSource::FullDimension {
            sheet: second,
            dimension: d,
        },
        (1.0, 1.0),
    );
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    f.p.apply(Command::DeleteSheet { sheet: second }).unwrap();
    assert_eq!(f.p.finished_measures.len(), 1);
    assert_eq!(f.p.finished_measures[0].name, "Largeur finie");
    persist::from_json(&bytes(&f.p)).unwrap();
    // Mesure du barème supprimée : déjà couvert par US-10.1, vérifié ici pour 10.4.
    let mut f = fx();
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    let out =
        f.p.apply(Command::RemoveMeasurement {
            measurement: f.largeur,
        })
        .unwrap();
    assert!(f.p.finished_measures.is_empty());
    assert!(out.notes.iter().any(|n| n.contains("1 mesure(s) finie(s)")));
    // Taille supprimée : les mesures finies ne citent aucune taille, rien à réécrire.
    let mut f = fx();
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    let xl = size(&f.p, "XL");
    f.p.apply(Command::RemoveSize { size: xl }).unwrap();
    assert_eq!(f.p.finished_measures.len(), 1);
    let t = f.p.finished_measure_table(f.range, false).unwrap();
    assert_eq!(t.sizes.len(), 4);
}

#[test]
fn cb12_refus_sans_trace_d_une_suppression_ne_laisse_aucune_mesure_finie_orpheline() {
    // Une commande refusée (verrou, validation…) ne retire rien.
    let mut f = fx();
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    let before = bytes(&f.p);
    let _ =
        f.p.apply(Command::RemoveSeamPair {
            id: SeamPairId(9_999_999),
        })
        .expect_err("inconnue");
    assert_eq!(bytes(&f.p), before);
}

// ---------- Format, rapport ----------

#[test]
fn cb13_sauvegarde_relecture_a_l_identique_et_schema_5() {
    let mut f = fx();
    add_alt(&mut f.p, "Largeur -2", f.largeur, -2.0);
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    add_fin(
        &mut f.p,
        "Hauteur finie",
        f.hauteur,
        FinishedSource::Seam { seam: f.pair },
        (1.0, 1.0),
    );
    assert_eq!(f.p.schema_version, 6);
    let json = bytes(&f.p);
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, f.p);
    assert_eq!(bytes(&back), json);
    assert_eq!(
        back.finished_measure_base_readings(),
        f.p.finished_measure_base_readings()
    );
}

#[test]
fn cb14_rapport_de_controle_lit_la_base_de_la_premiere_gamme() {
    let mut f = fx();
    assert!(
        f.p.control_report().finished.is_empty(),
        "sans mesure finie : section absente"
    );
    add_fin(
        &mut f.p,
        "Largeur finie",
        f.largeur,
        dim_source(f.sheet, f.dim_w),
        (2.0, 3.0),
    );
    let r = f.p.control_report();
    assert_eq!(r.finished.len(), 1);
    assert_eq!(r.finished[0].measured, Some(400.0));
    assert_eq!(r.finished[0].reference, Some(402.0));
    assert_eq!(r.finished[0].gap, Some(-2.0));
    assert_eq!(r.finished[0].status, FinishedStatus::Conforming);
    // Aucune anomalie n'est ajoutée au récapitulatif.
    let mut g = f.p.clone();
    g.finished_measures.clear();
    assert_eq!(r.anomalies, g.control_report().anomalies);
}

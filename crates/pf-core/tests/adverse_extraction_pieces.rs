//! Campagne adverse : extraction pieces (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource};
use pf_core::model::*;
use pf_core::{persist, Document, Project};
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

#[test]
fn extraction_de_pieces_campagne_aleatoire_graine_fixe() {
    let (mut found, mut refused, mut total, mut cuts) = (0usize, 0usize, 0usize, 0usize);
    for seq in 0..150u64 {
        let mut r = Rng(0x04_0001 + seq);
        let mut project = Project::new("Pièces aléa");
        let sheet = project.sheets[0].id;
        random_drawing(&mut r, &mut project, sheet);
        let mut doc = Document::new(project);
        for step in 0..6 {
            let label = format!("graine {seq} pas {step}");
            total += 1;
            let click = PointMm::new(r.unit() * 220.0 - 10.0, r.unit() * 220.0 - 10.0);
            let detached = r.chance(25);
            let cmd = Command::ExtractPiece {
                sheet,
                source: PieceSource::Click { at: click },
                name: format!("P{step}"),
                detached,
            };
            let before = doc.project().clone();
            let before_json = snap(&before);
            let predicted = before.sheet(sheet).unwrap().find_region_at(click);
            let result = catch_unwind(AssertUnwindSafe(|| doc.execute(cmd)));
            let Ok(result) = result else {
                panic!("panique : {label}");
            };
            match result {
                Err(e) => {
                    refused += 1;
                    assert!(!e.to_string().is_empty(), "{label}");
                    assert_eq!(
                        snap(doc.project()),
                        before_json,
                        "refus avec trace : {label}"
                    );
                    if predicted.is_ok() {
                        // La région existe : seul un cas géré (courbe lisse, cercle à crans,
                        // contour déjà pris) peut refuser, jamais en silence.
                        let m = e.to_string();
                        assert!(
                            m.contains("point de passage")
                                || m.contains("crans")
                                || m.contains("contour de la pièce")
                                || m.contains("chaîne")
                                || m.contains("même point"),
                            "refus inattendu {label} : {m}"
                        );
                    }
                }
                Ok(out) => {
                    found += 1;
                    let after = doc.project().clone();
                    after.validate().unwrap_or_else(|e| panic!("{label} : {e}"));
                    let id = out.pieces[0];
                    let c = after.piece_contour(id).unwrap();
                    assert!(c.area_mm2 > 0.0 && c.perimeter_mm > 0.0, "{label}");
                    assert!(c.closed && !c.clockwise, "{label} : {c:?}");
                    // Le clic est dans le contour (ou à moins de 0,05 mm de son bord).
                    let poly = piece_polygon(&after, id);
                    assert!(
                        pf_geometry::point_in_polygon(click, &poly)
                            || dist_to_polyline(&poly, click) < 0.05,
                        "{label} : clic {click:?} hors de la région renvoyée"
                    );
                    // La région annoncée avant et la pièce créée ont la même aire.
                    let region = predicted.expect("région prévue").region;
                    assert!(
                        (c.area_mm2 - region.area_mm2).abs() <= 1e-6 * region.area_mm2.max(1.0),
                        "{label} : {} <> {}",
                        c.area_mm2,
                        region.area_mm2
                    );
                    assert!(
                        (c.perimeter_mm - region.perimeter_mm).abs()
                            <= 1e-6 * region.perimeter_mm.max(1.0),
                        "{label}"
                    );
                    cuts += after.sheet(sheet).unwrap().entities.len()
                        - before.sheet(sheet).unwrap().entities.len();
                    // Annuler / rétablir : exact.
                    assert!(doc.undo(), "{label}");
                    assert_eq!(snap(doc.project()), before_json, "annuler : {label}");
                    assert!(doc.redo(), "{label}");
                    assert_eq!(snap(doc.project()), snap(&after), "rétablir : {label}");
                    // Aller-retour du fichier.
                    let json = persist::to_json(doc.project()).unwrap();
                    assert_eq!(
                        persist::to_json(&persist::from_json(&json).unwrap()).unwrap(),
                        json,
                        "{label}"
                    );
                }
            }
        }
    }
    eprintln!(
        "extraction de pièces : {found} créées sur {total} clics, {refused} refus, {cuts} tracés ajoutés par les coupes"
    );
    assert!(found >= 60, "seulement {found} extractions sur {total}");
    assert!(
        refused >= 20 && cuts >= 20,
        "refus {refused}, coupes {cuts}"
    );
}

#[test]
fn extraction_de_pieces_valeurs_extremes_sans_panique() {
    let (mut p, sheet, _pts, ents) = base_square();
    for &x in &extreme_numbers() {
        for source in [
            PieceSource::Click {
                at: PointMm::new(x, 5.0),
            },
            PieceSource::Click {
                at: PointMm::new(5.0, x),
            },
        ] {
            for detached in [false, true] {
                let cmd = Command::ExtractPiece {
                    sheet,
                    source: source.clone(),
                    name: "x".into(),
                    detached,
                };
                let before = snap(&p);
                let r = catch_unwind(AssertUnwindSafe(|| p.apply(cmd)));
                let Ok(r) = r else {
                    panic!("panique pour {x:e}");
                };
                if r.is_err() {
                    assert_eq!(snap(&p), before, "refus avec trace pour {x:e}");
                } else {
                    p.validate().unwrap();
                }
            }
        }
    }
    // Tracés choisis : doublons, vide, inconnus.
    for entities in [
        vec![],
        vec![ents[0], ents[0]],
        vec![ents[0], EntityId(u64::MAX)],
        ents.clone(),
    ] {
        let cmd = Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths { entities },
            name: "y".into(),
            detached: false,
        };
        let r = catch_unwind(AssertUnwindSafe(|| p.apply(cmd)));
        assert!(r.is_ok(), "panique sur des tracés choisis");
    }
    p.validate().unwrap();
}

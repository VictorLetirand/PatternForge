//! Campagne adverse : fiche piece (graine fixe).

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, PieceSource, PointRef};
use pf_core::model::*;
use pf_core::{persist, Document, Project};
use pf_geometry::PointMm;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// US-04.2 — fiche de pièce : campagne aléatoire à graine fixe
// ---------------------------------------------------------------------------

/// Texte de fiche tiré au hasard : valeurs normales, extrêmes, Unicode, contrôle, NUL.
fn card_text(r: &mut Rng) -> Option<String> {
    // Valeurs acceptées (bornes incluses) et valeurs à refuser (contrôle, NUL, trop long).
    let fine: Vec<String> = vec![
        String::new(),
        "A1".into(),
        "  B2  ".into(),
        "DV-01".into(),
        "Écharpe ✂".into(),
        "日本語のコード".into(),
        "👗🧵".into(),
        "l1\nl2\nl3".into(),
        "l1\r\nl2".into(),
        "x".repeat(MAX_NAME_LEN),
        "é".repeat(MAX_NAME_LEN),
        "m\n".repeat(MAX_COMMENT_LEN / 2),
    ];
    let hostile: Vec<String> = vec![
        " ".into(),
        "a\u{0}b".into(),
        "\u{0}".into(),
        "bad\nname".into(),
        "a\rb".into(),
        "tab\there".into(),
        "\u{1b}[31mrouge".into(),
        "\u{202e}inversé".into(),
        "x".repeat(MAX_NAME_LEN + 1),
        "m\n".repeat(MAX_COMMENT_LEN / 2 + 1),
        "z".repeat(10_000),
    ];
    if r.chance(15) {
        None
    } else if r.chance(12) {
        Some(hostile[r.below(hostile.len())].clone())
    } else {
        Some(fine[r.below(fine.len())].clone())
    }
}

fn card_quantity(r: &mut Rng) -> u32 {
    let pool = [1, 1, 2, 3, 12, 999, 1_000_000, u32::MAX - 1, u32::MAX];
    if r.chance(8) {
        0
    } else {
        pool[r.below(pool.len())]
    }
}

fn card_category(r: &mut Rng) -> Option<PieceCategory> {
    if r.chance(30) {
        None
    } else {
        Some(PieceCategory::ALL[r.below(PieceCategory::ALL.len())])
    }
}

#[test]
fn us042_campagne_aleatoire_de_la_fiche_de_piece_graine_fixe() {
    let (mut accepted, mut refused, mut noops, mut undone, mut reloaded) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut created = 0usize;
    for seq in 0..180u64 {
        let mut r = Rng(0x04_0200 + seq);
        let mut project = Project::new("Fiches aléa");
        let sheet = project.sheets[0].id;
        let mut doc = Document::new({
            let _ = project.apply(Command::AddRectangle {
                sheet,
                corner: PointRef::New(PointMm::new(0.0, 0.0)),
                width_mm: 100.0,
                height_mm: 60.0,
            });
            project
        });
        // Piles de références indépendantes de `Document` : un no-op ne doit rien empiler.
        let mut undo_stack: Vec<String> = Vec::new();
        let mut redo_stack: Vec<String> = Vec::new();
        for step in 0..16 {
            let label = format!("graine {seq} pas {step}");
            let before = doc.project().clone();
            let before_json = snap(&before);
            let ids: Vec<PieceId> = before.pieces.iter().map(|p| p.id).collect();
            let pick_piece = |r: &mut Rng| {
                if ids.is_empty() || r.chance(8) {
                    PieceId(BOGUS)
                } else {
                    ids[r.below(ids.len())]
                }
            };
            let op = r.below(100);
            if op < 12 || (ids.is_empty() && op < 60) {
                // Création : pièce-étiquette, ou extraction de la région du rectangle.
                let cmd = if r.chance(50) {
                    Command::AddPiece {
                        sheet,
                        name: format!("E{step}"),
                        quantity: 1,
                        material: None,
                        mirrored_pair: false,
                    }
                } else {
                    Command::ExtractPiece {
                        sheet,
                        source: PieceSource::Click {
                            at: PointMm::new(20.0 + r.unit() * 60.0, 10.0 + r.unit() * 40.0),
                        },
                        name: format!("P{step}"),
                        detached: r.chance(30),
                    }
                };
                if doc.execute(cmd).is_ok() {
                    created += 1;
                    undo_stack.push(before_json);
                    redo_stack.clear();
                } else {
                    assert_eq!(
                        snap(doc.project()),
                        before_json,
                        "refus avec trace : {label}"
                    );
                }
                continue;
            }
            if op < 72 {
                // Modification de la fiche (1 fois sur 6 : la fiche actuelle, sans changement).
                let piece = pick_piece(&mut r);
                let cmd = match before.piece(piece) {
                    Ok(q) if r.chance(17) => Command::SetPieceCard {
                        piece,
                        name: q.name.clone(),
                        code: q.code.clone(),
                        quantity: q.quantity,
                        mirrored_pair: q.mirrored_pair,
                        material: q.material.clone(),
                        category: q.category,
                        comment: q.comment.clone(),
                    },
                    _ => Command::SetPieceCard {
                        piece,
                        name: card_text(&mut r).unwrap_or_else(|| "Nom".into()),
                        code: card_text(&mut r),
                        quantity: card_quantity(&mut r),
                        mirrored_pair: r.chance(50),
                        material: card_text(&mut r),
                        category: card_category(&mut r),
                        comment: card_text(&mut r),
                    },
                };
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
                    }
                    Ok(_) => {
                        doc.project()
                            .validate()
                            .unwrap_or_else(|e| panic!("{label} : {e}"));
                        if snap(doc.project()) == before_json {
                            noops += 1; // aucune étape d'annulation
                        } else {
                            accepted += 1;
                            undo_stack.push(before_json);
                            redo_stack.clear();
                        }
                    }
                }
            } else if op < 80 {
                // Ancienne commande de fiche : ne doit pas toucher code/catégorie/commentaire.
                let piece = pick_piece(&mut r);
                let cmd = Command::SetPieceInfo {
                    piece,
                    name: card_text(&mut r).unwrap_or_else(|| "Nom".into()),
                    quantity: card_quantity(&mut r),
                    material: card_text(&mut r),
                    mirrored_pair: r.chance(50),
                };
                match doc.execute(cmd) {
                    Err(_) => assert_eq!(snap(doc.project()), before_json, "{label}"),
                    Ok(_) => {
                        if let (Ok(a), Ok(b)) = (before.piece(piece), doc.project().piece(piece)) {
                            assert_eq!(
                                (&a.code, a.category, &a.comment),
                                (&b.code, b.category, &b.comment),
                                "{label}"
                            );
                        }
                        if snap(doc.project()) != before_json {
                            undo_stack.push(before_json);
                            redo_stack.clear();
                        }
                    }
                }
            } else if op < 86 {
                let piece = pick_piece(&mut r);
                if doc.execute(Command::DeletePiece { piece }).is_ok() {
                    undo_stack.push(before_json);
                    redo_stack.clear();
                } else {
                    assert_eq!(snap(doc.project()), before_json, "{label}");
                }
            } else if op < 93 {
                // Annuler / rétablir : états exacts, étapes seulement pour les vrais changements.
                if r.chance(60) {
                    let expected = undo_stack.pop();
                    assert_eq!(doc.undo(), expected.is_some(), "{label}");
                    if let Some(state) = expected {
                        assert_eq!(snap(doc.project()), state, "annuler : {label}");
                        redo_stack.push(before_json);
                        undone += 1;
                    }
                } else {
                    let expected = redo_stack.pop();
                    assert_eq!(doc.redo(), expected.is_some(), "{label}");
                    if let Some(state) = expected {
                        assert_eq!(snap(doc.project()), state, "rétablir : {label}");
                        undo_stack.push(before_json);
                    }
                }
            } else {
                // Sauvegarde / relecture : identique, octets stables.
                let json = persist::to_json(doc.project())
                    .unwrap_or_else(|e| panic!("sauvegarde refusée : {label} : {e}"));
                let back = persist::from_json(&json)
                    .unwrap_or_else(|e| panic!("relecture refusée : {label} : {e}"));
                assert_eq!(&back, doc.project(), "relecture différente : {label}");
                assert_eq!(persist::to_json(&back).unwrap(), json, "octets : {label}");
                reloaded += 1;
            }
            // Lecture pure : jamais de panique, fiche cohérente avec la pièce.
            let current = doc.project().clone();
            for p in &current.pieces {
                let card = catch_unwind(AssertUnwindSafe(|| current.piece_card(p.id)))
                    .unwrap_or_else(|_| panic!("piece_card panique : {label}"))
                    .unwrap();
                assert_eq!(card.quantity, p.quantity, "{label}");
                assert!(card.quantity >= 1, "{label}");
                assert_eq!(card.code, p.code, "{label}");
                assert_eq!(card.issues, current.piece_issues(p.id), "{label}");
                if let Some(c) = &p.code {
                    assert!(
                        !c.trim().is_empty() && !c.chars().any(char::is_control),
                        "{label}"
                    );
                }
                if let Some(c) = &p.comment {
                    assert!(c.chars().count() <= MAX_COMMENT_LEN, "{label}");
                    assert!(
                        !c.chars().any(|ch| ch.is_control() && ch != '\n'),
                        "{label}"
                    );
                }
            }
        }
    }
    eprintln!(
        "fiches de pièces : {accepted} modifications, {refused} refus, {noops} sans changement, \
         {undone} annulations, {reloaded} relectures, {created} créations"
    );
    assert!(
        accepted >= 300,
        "seulement {accepted} modifications acceptées"
    );
    assert!(refused >= 300, "seulement {refused} refus");
    assert!(
        noops >= 1 && undone >= 100 && reloaded >= 100,
        "couverture insuffisante"
    );
}

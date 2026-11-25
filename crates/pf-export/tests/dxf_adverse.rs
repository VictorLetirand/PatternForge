//! US-12.1 — campagne adverse de l'export DXF : 160 séquences à graine fixe. Chaque séquence
//! construit un projet aléatoire (rectangles et pièces à courbes — Bézier, courbe lisse, arc —,
//! rôles de contour variés, pièces liées, détachées, dérivées, dépliées, pièces-étiquettes ou à
//! contour ouvert, droit-fil avec sens, crans simples et doubles des deux côtés, perçages,
//! pointages, croix, boutons, boutonnières de toutes orientations, éléments internes, fiches
//! avec code / matière / paire, noms accentués, placements manuels tournés) et l'exporte.
//!
//! L'oracle est indépendant : le fichier est relu par `common/dxf.rs` (aucun code partagé avec
//! l'écrivain) et comparé au modèle à 0,01 mm par `common/dxf_check.rs` : aire, périmètre, boîte
//! englobante, sommets sur le contour, crans (nombre, position, pied sur le bord), droit-fil,
//! perçages, boutons, boutonnières, éléments internes, textes, couches, emprise déclarée,
//! disjonction des pièces au placement automatique. Invariants par séquence : jamais de panique,
//! projet inchangé, octets identiques pour des entrées identiques (aussi sur une copie du projet
//! relue depuis son JSON), structure valide (paires, sections, EOF), aucun nombre non fini,
//! toute pièce soit écrite soit signalée dans le rapport.
//!
//! Rien de ceci ne prouve l'ouverture dans un logiciel métier : critère 3 de la story, humain.

mod common;

use common::dxf_check::*;
use common::dxf_models::*;
use common::Rng;
use pf_core::Project;
use pf_export::dxf::{export_dxf, DxfError, DxfIssueKind};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

const SEQUENCES: u64 = 160;

static EXPORTED: AtomicUsize = AtomicUsize::new(0);
static NOTCHES: AtomicUsize = AtomicUsize::new(0);
static GRAINS: AtomicUsize = AtomicUsize::new(0);
static CIRCLES: AtomicUsize = AtomicUsize::new(0);
static TEXTS: AtomicUsize = AtomicUsize::new(0);
static SKIPPED_EMPTY: AtomicUsize = AtomicUsize::new(0);
static SKIPPED_OPEN: AtomicUsize = AtomicUsize::new(0);
static REFUSED_NOTHING: AtomicUsize = AtomicUsize::new(0);
static MANUAL: AtomicUsize = AtomicUsize::new(0);
static DERIVED: AtomicUsize = AtomicUsize::new(0);
static SEWING: AtomicUsize = AtomicUsize::new(0);

macro_rules! ck {
    ($seq:expr, $cond:expr, $($arg:tt)+) => {
        if !$cond {
            panic!("séquence {} : {}", $seq, format!($($arg)+));
        }
    };
}

fn piece_level(kind: DxfIssueKind) -> bool {
    matches!(
        kind,
        DxfIssueKind::EmptyOutline
            | DxfIssueKind::OutlineUnreadable
            | DxfIssueKind::OutlineOpen
            | DxfIssueKind::DegenerateOutline
    )
}

fn project_of(seq: u64, rng: &mut Rng) -> Project {
    match seq % 16 {
        5 => common::dxf_models::rich_project().project,
        9 => pf_core::demo_piece_project().unwrap(),
        13 => pf_core::demo_project().unwrap(),
        _ => random_project(rng),
    }
}

fn run(seq: u64) {
    let mut rng = Rng(0xD0F5_1201_7C3B_99E1 ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    for _ in 0..4 {
        rng.next();
    }
    let project = project_of(seq, &mut rng);
    let before = project.clone();
    let json_before = pf_core::persist::to_json(&project).unwrap();
    let result = export_dxf(&project);
    ck!(seq, project == before, "le projet a changé");
    ck!(
        seq,
        pf_core::persist::to_json(&project).unwrap() == json_before,
        "octets du projet modifiés"
    );
    let out = match result {
        Ok(o) => o,
        Err(DxfError::NoPieces) => {
            ck!(seq, project.pieces.is_empty(), "NoPieces avec des pièces");
            return;
        }
        Err(DxfError::NothingExported(issues)) => {
            // Aucune pièce écrivable : chaque pièce est signalée, rien n'est produit.
            ck!(
                seq,
                issues.len() == project.pieces.len() && issues.iter().all(|i| piece_level(i.kind)),
                "refus sans détail complet : {issues:?}"
            );
            REFUSED_NOTHING.fetch_add(1, Relaxed);
            return;
        }
        Err(e) => panic!("séquence {seq} : erreur inattendue {e:?}"),
    };
    // Relecture indépendante et comparaison au modèle.
    let (_, done) = match verify_export(&project, &out.text, &out.report) {
        Ok(r) => r,
        Err(e) => panic!("séquence {seq} : {e}"),
    };
    // Déterminisme : même projet (copie, copie relue depuis son JSON), mêmes octets.
    ck!(
        seq,
        export_dxf(&before).unwrap().text == out.text,
        "export non déterministe"
    );
    let reread = pf_core::persist::from_json(&json_before).unwrap();
    ck!(
        seq,
        export_dxf(&reread).unwrap().text == out.text,
        "export différent après relecture du projet"
    );
    // Aucune perte silencieuse : chaque pièce est écrite ou signalée.
    let skipped = out
        .report
        .omissions()
        .filter(|i| piece_level(i.kind))
        .count();
    ck!(
        seq,
        out.report.pieces.len() + skipped == project.pieces.len(),
        "{} écrites + {} signalées ≠ {} pièces",
        out.report.pieces.len(),
        skipped,
        project.pieces.len()
    );
    for i in out.report.issues.iter() {
        ck!(seq, i.count > 0, "point signalé sans élément : {i:?}");
    }
    // Compteurs de couverture.
    EXPORTED.fetch_add(done.pieces, Relaxed);
    NOTCHES.fetch_add(done.notch_lines, Relaxed);
    GRAINS.fetch_add(done.grain_lines, Relaxed);
    CIRCLES.fetch_add(done.circles, Relaxed);
    TEXTS.fetch_add(done.texts, Relaxed);
    SKIPPED_EMPTY.fetch_add(
        out.report
            .issues
            .iter()
            .filter(|i| i.kind == DxfIssueKind::EmptyOutline)
            .count(),
        Relaxed,
    );
    SKIPPED_OPEN.fetch_add(
        out.report
            .issues
            .iter()
            .filter(|i| i.kind == DxfIssueKind::OutlineOpen)
            .count(),
        Relaxed,
    );
    MANUAL.fetch_add(
        project
            .pieces
            .iter()
            .filter(|p| p.placement.is_some())
            .count(),
        Relaxed,
    );
    DERIVED.fetch_add(
        project
            .pieces
            .iter()
            .filter(|p| p.derived.is_some())
            .count(),
        Relaxed,
    );
    SEWING.fetch_add(
        done.layer_counts.get(L_SEWING).copied().unwrap_or(0),
        Relaxed,
    );
}

#[test]
fn campagne_adverse_export_dxf_160_sequences() {
    for seq in 0..SEQUENCES {
        run(seq);
    }
    // Garde : la campagne a réellement exercé chaque famille d'éléments.
    let n = |a: &AtomicUsize| a.load(Relaxed);
    eprintln!(
        "pièces {} crans {} droit-fil {} cercles {} textes {} étiquettes {} ouvertes {} refus {} manuel {} dérivées {} couture {}",
        n(&EXPORTED),
        n(&NOTCHES),
        n(&GRAINS),
        n(&CIRCLES),
        n(&TEXTS),
        n(&SKIPPED_EMPTY),
        n(&SKIPPED_OPEN),
        n(&REFUSED_NOTHING),
        n(&MANUAL),
        n(&DERIVED),
        n(&SEWING)
    );
    assert!(n(&EXPORTED) > 200, "trop peu de pièces écrites");
    assert!(n(&NOTCHES) > 100, "trop peu de crans");
    assert!(n(&GRAINS) > 100, "trop peu de droit-fil");
    assert!(n(&CIRCLES) > 100, "trop peu de cercles");
    assert!(n(&TEXTS) > 400, "trop peu de textes");
    assert!(n(&SKIPPED_EMPTY) > 5, "pièces-étiquettes jamais exercées");
    assert!(n(&MANUAL) > 20, "placements manuels jamais exercés");
    assert!(n(&DERIVED) > 5, "pièces dérivées jamais exercées");
    assert!(n(&SEWING) > 5, "contours de couture jamais exercés");
}

/// Entrées hostiles au lecteur lui-même : le lecteur indépendant refuse ce qui n'est pas DXF.
#[test]
fn le_lecteur_refuse_les_fichiers_invalides() {
    use common::dxf::parse;
    let r = rich_project();
    let good = export_dxf(&r.project).unwrap().text;
    assert!(parse(&good).is_ok());
    // Paire coupée, EOF absent, section non refermée, nombre non fini, code non entier.
    assert!(parse(&good[..good.len() - 4]).is_err(), "EOF tronqué");
    assert!(parse(&good.replacen("ENDSEC", "ENDSE", 1)).is_err());
    assert!(parse(&good.replacen("10.00000", "NaN", 1)).is_err() || !good.contains("10.00000"));
    assert!(parse(&good.replacen("  0\nSECTION", "  x\nSECTION", 1)).is_err());
    assert!(parse(&format!("{good}  9\n")).is_err());
}

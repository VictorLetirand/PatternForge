//! US-12.2 - import DXF de type AAMA / ASTM : pièces, crans, droit-fil. Les fichiers sont **écrits
//! à la main** ici (blocs, couches de la table `LAYERS`, valeurs choisies dans le test) et les
//! valeurs attendues sont calculées dans le test (shoelace, géométrie du cran, arc analytique) ;
//! rien n'est comparé à la sortie du code testé lui-même.
//!
//! Honnêteté : aucun fichier de logiciel métier n'est lu ; l'ouverture d'un DXF réel dans un
//! logiciel tiers reste non prouvée ; la gradation (`.rul`) n'est pas lue (aucune spécification
//! dans le cahier) ; la table de couches n'est pas confirmée avec la norme.

mod common;

use common::dxf_models::*;
use pf_core::{
    locate_notch, AamaNotch, AamaPiece, Command, CoreError, Document, PathRole, Project,
    MAX_AAMA_PIECES, MAX_AAMA_VERTICES,
};
use pf_export::dxf::export_dxf;
use pf_export::dxf_aama_import::{AamaBuild, AamaDocument, AamaNoteKind, AamaReason};
use pf_export::dxf_import::{DeclaredUnit, DxfImportError, DxfUnit, UnitChoice, MAX_ENTITIES};
use pf_geometry::{PointMm, Side};

// ----- écriture à la main ----------------------------------------------------------------------

fn pair(s: &mut String, code: i32, v: impl std::fmt::Display) {
    s.push_str(&format!("{code:>3}\r\n{v}\r\n"));
}

fn lwpoly(layer: &str, pts: &[(f64, f64, f64)], closed: bool) -> String {
    let mut s = String::new();
    pair(&mut s, 0, "LWPOLYLINE");
    pair(&mut s, 8, layer);
    pair(&mut s, 90, pts.len());
    pair(&mut s, 70, i32::from(closed));
    for (x, y, b) in pts {
        pair(&mut s, 10, x);
        pair(&mut s, 20, y);
        if *b != 0.0 {
            pair(&mut s, 42, b);
        }
    }
    s
}

fn poly(layer: &str, pts: &[(f64, f64)], closed: bool) -> String {
    let mut s = String::new();
    pair(&mut s, 0, "POLYLINE");
    pair(&mut s, 8, layer);
    pair(&mut s, 66, 1);
    pair(&mut s, 70, i32::from(closed));
    for (x, y) in pts {
        pair(&mut s, 0, "VERTEX");
        pair(&mut s, 8, layer);
        pair(&mut s, 10, x);
        pair(&mut s, 20, y);
    }
    pair(&mut s, 0, "SEQEND");
    pair(&mut s, 8, layer);
    s
}

fn line(layer: &str, a: (f64, f64), b: (f64, f64)) -> String {
    let mut s = String::new();
    pair(&mut s, 0, "LINE");
    pair(&mut s, 8, layer);
    pair(&mut s, 10, a.0);
    pair(&mut s, 20, a.1);
    pair(&mut s, 11, b.0);
    pair(&mut s, 21, b.1);
    s
}

fn text(layer: &str, t: &str) -> String {
    let mut s = String::new();
    pair(&mut s, 0, "TEXT");
    pair(&mut s, 8, layer);
    pair(&mut s, 10, 0);
    pair(&mut s, 20, 0);
    pair(&mut s, 40, 4);
    pair(&mut s, 1, t);
    s
}

fn circle(layer: &str, c: (f64, f64), r: f64) -> String {
    let mut s = String::new();
    pair(&mut s, 0, "CIRCLE");
    pair(&mut s, 8, layer);
    pair(&mut s, 10, c.0);
    pair(&mut s, 20, c.1);
    pair(&mut s, 40, r);
    s
}

fn block(name: &str, body: &[String]) -> String {
    let mut s = String::new();
    pair(&mut s, 0, "BLOCK");
    pair(&mut s, 8, "0");
    pair(&mut s, 2, name);
    pair(&mut s, 70, 0);
    pair(&mut s, 10, 0);
    pair(&mut s, 20, 0);
    for b in body {
        s.push_str(b);
    }
    pair(&mut s, 0, "ENDBLK");
    pair(&mut s, 8, "0");
    s
}

fn insert(name: &str, at: (f64, f64)) -> String {
    let mut s = String::new();
    pair(&mut s, 0, "INSERT");
    pair(&mut s, 8, "0");
    pair(&mut s, 2, name);
    pair(&mut s, 10, at.0);
    pair(&mut s, 20, at.1);
    s
}

fn file(unit: Option<i64>, blocks: &[String], entities: &[String]) -> String {
    let mut s = String::new();
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "HEADER");
    pair(&mut s, 9, "$ACADVER");
    pair(&mut s, 1, "AC1009");
    if let Some(u) = unit {
        pair(&mut s, 9, "$INSUNITS");
        pair(&mut s, 70, u);
    }
    pair(&mut s, 0, "ENDSEC");
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "BLOCKS");
    for b in blocks {
        s.push_str(b);
    }
    pair(&mut s, 0, "ENDSEC");
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "ENTITIES");
    for e in entities {
        s.push_str(e);
    }
    pair(&mut s, 0, "ENDSEC");
    pair(&mut s, 0, "EOF");
    s
}

const RECT: [(f64, f64); 4] = [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)];

fn one_piece_file(extra: &[String]) -> String {
    let mut body = vec![poly("1", &RECT, true)];
    body.extend(extra.iter().cloned());
    file(Some(4), &[block("P1", &body)], &[insert("P1", (0.0, 0.0))])
}

fn build_mm(text: &str) -> AamaBuild {
    AamaDocument::parse(text.as_bytes())
        .expect("lecture")
        .build(DxfUnit::Millimetre)
}

fn shoelace(p: &[(f64, f64)]) -> f64 {
    let n = p.len();
    (0..n)
        .map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

fn import(b: &AamaBuild) -> (Project, pf_core::Outcome) {
    let mut p = Project::new("aama");
    let before = p.sheets.len();
    let out = p.apply(b.command().expect("commande")).expect("import");
    assert_eq!(p.sheets.len(), before + b.pieces.len());
    (p, out)
}

fn sum_ignored(b: &AamaBuild) -> usize {
    b.report.ignored_total()
}

// ----- CA : pièces, crans, droit-fil -----------------------------------------------------------

#[test]
fn piece_crans_et_droit_fil_ecrits_a_la_main() {
    let f = one_piece_file(&[
        text("84", "Piece Name: Devant"),
        text("84", "Code: D-01"),
        text("84", "Quantity: 2"),
        text("84", "Material: Coton"),
        text("84", "Mirrored pair: yes"),
        // pied (30 ; 0) sur le bord bas, pointe vers l'intérieur (côté gauche du sens +x).
        line("4", (30.0, 0.0), (30.0, 5.0)),
        line("6", (20.0, 25.0), (80.0, 25.0)),
        // pointes de flèche : comptées, pas importées comme droit-fil.
        line("6", (80.0, 25.0), (75.0, 27.0)),
        line("6", (80.0, 25.0), (75.0, 23.0)),
    ]);
    let b = build_mm(&f);
    assert_eq!(b.pieces.len(), 1);
    assert_eq!(b.report.inserts, 1);
    assert_eq!(b.report.ignored_of(AamaReason::GrainMark), 2);
    assert_eq!(sum_ignored(&b), 2);
    assert!(!b.report.rul_read(), "la gradation n'est pas lue");
    let (p, out) = import(&b);
    let piece = p.piece(out.pieces[0]).unwrap();
    assert_eq!(piece.name, "Devant");
    assert_eq!(piece.code.as_deref(), Some("D-01"));
    assert_eq!(piece.quantity, 2);
    assert_eq!(piece.material.as_deref(), Some("Coton"));
    assert!(piece.mirrored_pair);
    let c = p.piece_contour(piece.id).unwrap();
    assert!(c.closed && !c.self_intersecting);
    assert!(
        (c.area_mm2 - shoelace(&RECT)).abs() < 1e-6,
        "{}",
        c.area_mm2
    );
    assert!((c.perimeter_mm - 300.0).abs() < 1e-6);
    for e in &piece.outline {
        let sh = p.piece_sheet(piece.id).unwrap();
        assert_eq!(sh.entity(e.entity).unwrap().role, PathRole::CuttingContour);
    }
    // Cran relu comme traits : pied (30 ; 0), pointe (30 ; 5).
    let sh = p.piece_sheet(piece.id).unwrap();
    assert_eq!(sh.notches.len(), 1);
    let marks = sh.notch_marks(&sh.notches[0]).unwrap();
    assert_eq!(marks.len(), 1);
    let (a, z) = marks[0];
    assert!((a.x - 30.0).abs() < 0.01 && a.y.abs() < 0.01, "{a:?}");
    assert!(
        (z.x - 30.0).abs() < 0.01 && (z.y - 5.0).abs() < 0.01,
        "{z:?}"
    );
    // Droit-fil.
    let g = p.piece_grain_lines(piece.id);
    assert_eq!(g.len(), 1);
    assert!((g[0].from.x - 20.0).abs() < 0.01 && (g[0].from.y - 25.0).abs() < 0.01);
    assert!((g[0].to.x - 80.0).abs() < 0.01);
    // Le projet obtenu est valide (aller-retour par le fichier).
    let json = pf_core::persist::to_json(&p).unwrap();
    assert!(pf_core::persist::from_json(&json).is_ok());
}

#[test]
fn pied_et_pointe_dans_n_importe_quel_ordre_et_cote_droit() {
    // Pointe côté extérieur (y < 0) : côté droit du sens +x.
    let o: Vec<PointMm> = RECT.iter().map(|p| PointMm::new(p.0, p.1)).collect();
    let l = locate_notch(&o, PointMm::new(30.0, -5.0), PointMm::new(30.0, 0.0)).unwrap();
    assert_eq!(l.edge, 0);
    assert_eq!(l.side, Side::Right);
    assert!((l.distance_mm - 30.0).abs() < 1e-9 && (l.length_mm - 5.0).abs() < 1e-9);
    let l2 = locate_notch(&o, PointMm::new(30.0, 0.0), PointMm::new(30.0, -5.0)).unwrap();
    assert_eq!((l2.edge, l2.side), (0, Side::Right));
    // Ni pied ni pointe sur le contour.
    assert!(locate_notch(&o, PointMm::new(30.0, 10.0), PointMm::new(30.0, 15.0)).is_none());
    // Non fini, longueur nulle.
    assert!(locate_notch(&o, PointMm::new(f64::NAN, 0.0), PointMm::new(30.0, 5.0)).is_none());
    assert!(locate_notch(&o, PointMm::new(30.0, 0.0), PointMm::new(30.0, 0.0)).is_none());
    assert!(locate_notch(&o[..2], PointMm::new(30.0, 0.0), PointMm::new(30.0, 5.0)).is_none());
}

#[test]
fn cran_hors_contour_compte_pas_perdu_en_silence() {
    let f = one_piece_file(&[
        line("4", (30.0, 0.0), (30.0, 5.0)),
        line("4", (50.0, 20.0), (50.0, 25.0)),
        line("4", (100.0, 10.0), (95.0, 10.0)),
    ]);
    let b = build_mm(&f);
    assert_eq!(b.report.notches(), 2);
    assert_eq!(b.report.ignored_of(AamaReason::NotOnContour), 1);
    assert_eq!(sum_ignored(&b), 1);
}

#[test]
fn unites_declarees_choisies_et_refusees() {
    let inch: Vec<(f64, f64)> = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)];
    let f = |u| {
        file(
            u,
            &[block("P", &[poly("1", &inch, true)])],
            &[insert("P", (0.0, 0.0))],
        )
    };
    let d = AamaDocument::parse(f(Some(1)).as_bytes()).unwrap();
    assert_eq!(d.declared_unit(), DeclaredUnit::Known(DxfUnit::Inch));
    let b = d.build_choice(UnitChoice::Declared).unwrap();
    let (p, out) = import(&b);
    let c = p.piece_contour(out.pieces[0]).unwrap();
    assert!((c.area_mm2 - 254.0 * 127.0).abs() < 1e-6);
    // Sans déclaration : l'unité est exigée.
    let d = AamaDocument::parse(f(None).as_bytes()).unwrap();
    assert_eq!(d.declared_unit(), DeclaredUnit::Missing);
    assert_eq!(
        d.build_choice(UnitChoice::Declared),
        Err(DxfImportError::UnitRequired)
    );
    let b = d
        .build_choice(UnitChoice::Fixed(DxfUnit::Centimetre))
        .unwrap();
    let (p, out) = import(&b);
    assert!((p.piece_contour(out.pieces[0]).unwrap().area_mm2 - 5000.0).abs() < 1e-6);
}

#[test]
fn insertion_decalee_blocs_multiples_et_ce_qui_n_est_pas_importe() {
    let b1 = block(
        "A",
        &[
            poly("1", &RECT, true),
            text("84", "Piece Name: Pièce\\U+00E9"),
        ],
    );
    let b2 = block(
        "B",
        &[lwpoly(
            "1",
            &[(0.0, 0.0, 0.0), (30.0, 0.0, 0.0), (0.0, 40.0, 0.0)],
            true,
        )],
    );
    let b3 = block(
        "ORPHELIN",
        &[poly("1", &RECT, true), line("4", (1.0, 0.0), (1.0, 4.0))],
    );
    let f = file(
        Some(4),
        &[b1, b2, b3],
        &[
            insert("A", (200.0, 100.0)),
            insert("A", (400.0, 0.0)),
            insert("B", (0.0, 0.0)),
            insert("INCONNU", (0.0, 0.0)),
            line("1", (0.0, 0.0), (5.0, 5.0)),
        ],
    );
    let b = build_mm(&f);
    assert_eq!(b.pieces.len(), 3);
    assert_eq!(b.report.inserts, 4);
    assert_eq!(b.report.ignored_of(AamaReason::UnknownBlock), 1);
    assert_eq!(b.report.ignored_of(AamaReason::NoBlock), 1);
    assert_eq!(b.report.ignored_of(AamaReason::BlockNotInserted), 2);
    assert_eq!(sum_ignored(&b), 4);
    // Translation de l'insertion (oracle : coordonnées écrites dans le test).
    assert_eq!(b.pieces[0].outline[0], PointMm::new(200.0, 100.0));
    assert_eq!(b.pieces[1].outline[0], PointMm::new(400.0, 0.0));
    let (p, out) = import(&b);
    assert_eq!(out.pieces.len(), 3);
    let names: Vec<String> = p.pieces.iter().map(|x| x.name.clone()).collect();
    assert_eq!(names[0], "Pièceé");
    let mut sheets: Vec<&str> = p.sheets.iter().map(|s| s.name.as_str()).collect();
    sheets.sort();
    sheets.dedup();
    assert_eq!(sheets.len(), p.sheets.len(), "feuilles à noms uniques");
    assert!((p.piece_contour(out.pieces[2]).unwrap().area_mm2 - 600.0).abs() < 1e-6);
}

#[test]
fn coupe_prioritaire_couture_comptee_ou_seule() {
    let inner = [(5.0, 5.0), (95.0, 5.0), (95.0, 45.0), (5.0, 45.0)];
    let both = file(
        Some(4),
        &[block(
            "P",
            &[poly("1", &RECT, true), poly("15", &inner, true)],
        )],
        &[insert("P", (0.0, 0.0))],
    );
    let b = build_mm(&both);
    assert!(!b.pieces[0].sewing);
    assert_eq!(b.report.ignored_of(AamaReason::ExtraContour), 1);
    let only = file(
        Some(4),
        &[block("P", &[poly("15", &inner, true)])],
        &[insert("P", (0.0, 0.0))],
    );
    let b = build_mm(&only);
    assert!(b.pieces[0].sewing);
    let (p, out) = import(&b);
    let piece = p.piece(out.pieces[0]).unwrap();
    let sh = p.piece_sheet(piece.id).unwrap();
    assert_eq!(
        sh.entity(piece.outline[0].entity).unwrap().role,
        PathRole::SewingContour
    );
    assert!((p.piece_contour(piece.id).unwrap().area_mm2 - 90.0 * 40.0).abs() < 1e-6);
}

#[test]
fn comptabilite_complete_aucune_entite_perdue() {
    // 1 contour + 1 cran + 1 droit-fil importés ; le reste compté un par un.
    let f = one_piece_file(&[
        line("4", (30.0, 0.0), (30.0, 5.0)),
        line("6", (20.0, 25.0), (80.0, 25.0)),
        circle("13", (50.0, 25.0), 3.0),
        line("7", (10.0, 10.0), (90.0, 10.0)),
        line("ZZ", (0.0, 0.0), (1.0, 1.0)),
        text("84", "Category: lining"),
        circle("2", (0.0, 0.0), 1.0),
        line("4", (500.0, 500.0), (500.0, 510.0)),
    ]);
    let b = build_mm(&f);
    assert_eq!(b.report.ignored_of(AamaReason::NotImported), 3);
    assert_eq!(b.report.ignored_of(AamaReason::UnknownLayer), 1);
    assert_eq!(b.report.ignored_of(AamaReason::TextLine), 1);
    assert_eq!(b.report.ignored_of(AamaReason::NotOnContour), 1);
    // 9 éléments dans le bloc (contour compris) = 3 importés + 6 comptés.
    assert_eq!(sum_ignored(&b), 6);
    assert_eq!(b.report.pieces.len(), 1);
    assert_eq!(b.report.pieces[0].notches, 1);
    assert!(b.report.pieces[0].grain);
}

#[test]
fn arc_bulge_aplati_dans_la_tolerance() {
    // Carré 100 avec un demi-cercle (bulge 1) sur le côté bas, vers l'extérieur.
    let f = file(
        Some(4),
        &[block(
            "P",
            &[lwpoly(
                "1",
                &[
                    (0.0, 0.0, 1.0),
                    (100.0, 0.0, 0.0),
                    (100.0, 100.0, 0.0),
                    (0.0, 100.0, 0.0),
                ],
                true,
            )],
        )],
        &[insert("P", (0.0, 0.0))],
    );
    let b = build_mm(&f);
    assert_eq!(b.report.note(AamaNoteKind::ArcsFlattened), 1);
    assert!(b.pieces[0].outline.len() > 20);
    let (p, out) = import(&b);
    let area = p.piece_contour(out.pieces[0]).unwrap().area_mm2;
    let exact = 10_000.0 + std::f64::consts::PI * 50.0 * 50.0 / 2.0;
    assert!((area - exact).abs() < 2.0, "{area} vs {exact}");
    assert!(area < exact, "polygone inscrit");
}

#[test]
fn doublons_et_fermeture_par_repetition() {
    let pts = [
        (0.0, 0.0),
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 50.0),
        (0.0, 50.0),
        (0.0, 0.0),
    ];
    let f = file(
        Some(4),
        &[block("P", &[poly("1", &pts, false)])],
        &[insert("P", (0.0, 0.0))],
    );
    let b = build_mm(&f);
    assert_eq!(b.pieces[0].outline.len(), 4);
    assert_eq!(b.report.note(AamaNoteKind::ClosedByRepeat), 1);
    assert_eq!(b.report.note(AamaNoteKind::DuplicateVertices), 1);
}

// ----- une seule Command annulable, refus sans trace --------------------------------------------

#[test]
fn une_etape_annulable_et_refus_sans_trace() {
    let f = file(
        Some(4),
        &[
            block(
                "A",
                &[poly("1", &RECT, true), line("4", (30.0, 0.0), (30.0, 5.0))],
            ),
            block("B", &[poly("1", &RECT, true)]),
        ],
        &[insert("A", (0.0, 0.0)), insert("B", (300.0, 0.0))],
    );
    let b = build_mm(&f);
    let mut doc = Document::new(Project::new("x"));
    let before = doc.project().clone();
    doc.execute(b.command().unwrap()).unwrap();
    assert_eq!(doc.project().pieces.len(), 2);
    assert!(doc.undo());
    assert!(!doc.can_undo(), "un seul pas d'annulation");
    assert_eq!(*doc.project(), before);
    assert!(doc.redo());
    assert_eq!(doc.project().pieces.len(), 2);

    // Refus sans trace : une pièce invalide parmi de bonnes.
    let good = b.pieces[0].clone();
    let mut bad = b.pieces[1].clone();
    bad.outline[1] = PointMm::new(f64::NAN, 0.0);
    let mut p = Project::new("x");
    let before = p.clone();
    let r = p.apply(Command::ImportAamaPieces {
        pieces: vec![good.clone(), bad],
    });
    assert!(matches!(r, Err(CoreError::Invalid(_)) | Err(_)));
    assert_eq!(p, before);
    for mutate in [
        |q: &mut AamaPiece| q.outline.truncate(2),
        |q: &mut AamaPiece| q.outline.push(q.outline[0]),
        |q: &mut AamaPiece| q.quantity = 0,
        |q: &mut AamaPiece| q.name = "  ".into(),
        |q: &mut AamaPiece| q.name = "a\nb".into(),
        |q: &mut AamaPiece| {
            q.notches.push(AamaNotch {
                foot: PointMm::new(50.0, 25.0),
                tip: PointMm::new(50.0, 30.0),
            })
        },
        |q: &mut AamaPiece| q.grain = Some((PointMm::new(1.0, 1.0), PointMm::new(1.0, 1.0))),
        |q: &mut AamaPiece| q.outline[0] = PointMm::new(1e12, 0.0),
    ] {
        let mut q = good.clone();
        mutate(&mut q);
        let mut p = Project::new("x");
        let before = p.clone();
        assert!(p
            .apply(Command::ImportAamaPieces { pieces: vec![q] })
            .is_err());
        assert_eq!(p, before);
    }
    for pieces in [Vec::new(), vec![good.clone(); MAX_AAMA_PIECES + 1]] {
        let mut p = Project::new("x");
        let before = p.clone();
        assert!(p.apply(Command::ImportAamaPieces { pieces }).is_err());
        assert_eq!(p, before);
    }
}

// ----- adverse ---------------------------------------------------------------------------------

#[test]
fn fichier_tronque_a_chaque_longueur_jamais_de_panique() {
    let f = one_piece_file(&[
        line("4", (30.0, 0.0), (30.0, 5.0)),
        line("6", (20.0, 25.0), (80.0, 25.0)),
        text("84", "Piece Name: T"),
    ]);
    let bytes = f.as_bytes();
    let mut ok = 0;
    for n in (0..bytes.len() - 3).step_by(7).chain([bytes.len() - 4]) {
        if let Ok(d) = AamaDocument::parse(&bytes[..n]) {
            ok += 1;
            let _ = d.build(DxfUnit::Millimetre);
        }
    }
    assert!(AamaDocument::parse(bytes).is_ok());
    assert_eq!(ok, 0, "un fichier tronqué est refusé, jamais lu à moitié");
    assert!(matches!(
        AamaDocument::parse(&bytes[..bytes.len() - 5]),
        Err(DxfImportError::Truncated { .. })
    ));
}

#[test]
fn refus_de_lecture_binaire_nul_code_invalide_vide() {
    assert_eq!(
        AamaDocument::parse(b"AutoCAD Binary DXF\r\n\x1a\0"),
        Err(DxfImportError::BinaryDxf)
    );
    assert_eq!(
        AamaDocument::parse(b"0\nSEC\0TION"),
        Err(DxfImportError::NotDxf)
    );
    assert!(matches!(
        AamaDocument::parse(b"abc\nSECTION\n"),
        Err(DxfImportError::InvalidGroupCode { line: 1 })
    ));
    assert!(AamaDocument::parse(b"").is_err());
    assert!(AamaDocument::parse(b"  0\nEOF\n").is_err());
    assert!(matches!(
        AamaDocument::parse(b"  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n"),
        Err(DxfImportError::Truncated { .. })
    ));
    let big = vec![b'0'; pf_export::dxf_import::MAX_FILE_BYTES + 1];
    assert!(matches!(
        AamaDocument::parse(&big),
        Err(DxfImportError::TooLarge { .. })
    ));
}

#[test]
fn valeurs_non_finies_couches_inconnues_et_inserts_etranges() {
    let mut nan_poly = String::new();
    pair(&mut nan_poly, 0, "LWPOLYLINE");
    pair(&mut nan_poly, 8, "1");
    pair(&mut nan_poly, 70, 1);
    for (x, y) in [("0", "0"), ("nan", "0"), ("10", "inf"), ("1e999", "5")] {
        pair(&mut nan_poly, 10, x);
        pair(&mut nan_poly, 20, y);
    }
    let mut scaled = insert("P", (0.0, 0.0));
    pair(&mut scaled, 41, 2);
    let mut rotated = insert("P", (0.0, 0.0));
    pair(&mut rotated, 50, 90);
    let mut array = insert("P", (0.0, 0.0));
    pair(&mut array, 70, 3);
    let mut nested = String::new();
    nested.push_str(&insert("P", (1.0, 1.0)));
    let f = file(
        Some(4),
        &[
            block("P", &[poly("1", &RECT, true)]),
            block("N", &[nan_poly.clone(), nested]),
        ],
        &[
            insert("P", (0.0, 0.0)),
            scaled,
            rotated,
            array,
            insert("N", (0.0, 0.0)),
        ],
    );
    let b = build_mm(&f);
    assert_eq!(b.pieces.len(), 1);
    assert_eq!(b.report.ignored_of(AamaReason::Transformed), 3);
    assert_eq!(b.report.ignored_of(AamaReason::Invalid), 1);
    assert_eq!(b.report.ignored_of(AamaReason::Nested), 1);
    assert_eq!(b.report.ignored_of(AamaReason::NoContour), 1);
    for i in &b.report.ignored {
        assert!(i.count > 0);
    }
}

#[test]
fn contours_degeneres_ouverts_hors_espace_enormes_et_en_huit() {
    let huge: Vec<(f64, f64)> = vec![(0.0, 0.0), (1e12, 0.0), (1e12, 1e12)];
    let same: Vec<(f64, f64)> = vec![(5.0, 5.0); 6];
    let open: Vec<(f64, f64)> = RECT.to_vec();
    let bowtie: Vec<(f64, f64)> = vec![(0.0, 0.0), (100.0, 100.0), (100.0, 0.0), (0.0, 100.0)];
    let many: Vec<(f64, f64)> = (0..MAX_AAMA_VERTICES + 5)
        .map(|i| {
            let a = i as f64 / (MAX_AAMA_VERTICES + 5) as f64 * std::f64::consts::TAU;
            (500.0 * a.cos(), 500.0 * a.sin())
        })
        .collect();
    let f = file(
        Some(4),
        &[
            block("HUGE", &[poly("1", &huge, true)]),
            block("SAME", &[poly("1", &same, true)]),
            block("OPEN", &[poly("1", &open, false)]),
            block("BOW", &[poly("1", &bowtie, true)]),
            block("MANY", &[poly("1", &many, true)]),
        ],
        &["HUGE", "SAME", "OPEN", "BOW", "MANY"]
            .iter()
            .map(|n| insert(n, (0.0, 0.0)))
            .collect::<Vec<_>>(),
    );
    let b = build_mm(&f);
    assert_eq!(b.report.ignored_of(AamaReason::OutOfRange), 1);
    assert_eq!(b.report.ignored_of(AamaReason::Degenerate), 1);
    assert_eq!(b.report.ignored_of(AamaReason::OpenContour), 1);
    assert_eq!(b.report.ignored_of(AamaReason::TooComplex), 1);
    // Le nœud papillon est soit importé soit refusé par le modèle et compté : jamais perdu.
    let bow_imported = b.pieces.iter().any(|p| p.name == "BOW");
    assert!(bow_imported || b.report.ignored_of(AamaReason::Refused) == 1);
    assert_eq!(b.pieces.len(), usize::from(bow_imported));
    assert_eq!(b.report.ignored_of(AamaReason::NoContour), 4);
}

#[test]
fn trop_de_pieces_compte_et_trop_d_entites_refuse() {
    let mut blocks = Vec::new();
    let mut inserts = Vec::new();
    for i in 0..MAX_AAMA_PIECES + 3 {
        blocks.push(block(
            &format!("B{i}"),
            &[lwpoly(
                "1",
                &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (0.0, 10.0, 0.0)],
                true,
            )],
        ));
        inserts.push(insert(&format!("B{i}"), (i as f64 * 20.0, 0.0)));
    }
    let b = build_mm(&file(Some(4), &blocks, &inserts));
    assert_eq!(b.pieces.len(), MAX_AAMA_PIECES);
    assert_eq!(b.report.ignored_of(AamaReason::TooComplex), 3);

    let mut s = String::new();
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "ENTITIES");
    for _ in 0..=MAX_ENTITIES {
        s.push_str("0\nPOINT\n");
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    assert_eq!(
        AamaDocument::parse(s.as_bytes()),
        Err(DxfImportError::TooManyEntities)
    );
}

#[test]
fn doublons_de_blocs_et_noms_de_feuilles_existants() {
    let f = file(
        Some(4),
        &[
            block("P", &[poly("1", &RECT, true)]),
            block("P", &[poly("1", &RECT, true)]),
        ],
        &[insert("P", (0.0, 0.0)), insert("P", (200.0, 0.0))],
    );
    let b = build_mm(&f);
    assert_eq!(b.pieces.len(), 2);
    let mut p = Project::new("x");
    let first = p.apply(b.command().unwrap()).unwrap();
    // Réimporter le même fichier : noms de feuilles toujours uniques.
    let second = p.apply(b.command().unwrap()).unwrap();
    assert_eq!(first.pieces.len() + second.pieces.len(), 4);
    let mut names: Vec<&str> = p.sheets.iter().map(|s| s.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), p.sheets.len());
}

// ----- aller-retour avec l'export d'US-12.1 -----------------------------------------------------

#[test]
fn aller_retour_avec_l_export_du_projet_riche() {
    let r = rich_project();
    let out = export_dxf(&r.project).unwrap();
    let d = AamaDocument::parse(out.text.as_bytes()).unwrap();
    assert_eq!(d.declared_unit(), DeclaredUnit::Known(DxfUnit::Millimetre));
    let b = d.build_choice(UnitChoice::Declared).unwrap();
    assert_eq!(out.report.pieces.len(), 6);
    assert_eq!(b.report.inserts, 6);
    assert!(b.pieces.len() >= 5, "{} pièces", b.pieces.len());
    let (p, imported) = import(&b);
    // Chaque pièce importée a une aire et un périmètre proches de la pièce d'origine
    // (même nom), au millimètre carré près (courbes aplaties à 0,001 mm).
    let mut matched = 0;
    for pid in &imported.pieces {
        let new = p.piece(*pid).unwrap();
        let Some(old) = r.project.pieces.iter().find(|o| o.name == new.name) else {
            continue;
        };
        let (a, c) = (
            r.project.piece_contour(old.id).unwrap(),
            p.piece_contour(new.id).unwrap(),
        );
        assert!(
            (a.area_mm2 - c.area_mm2).abs() <= 0.5 + a.area_mm2 * 1e-6,
            "{}",
            new.name
        );
        assert!(
            (a.perimeter_mm - c.perimeter_mm).abs() <= 0.5,
            "{}",
            new.name
        );
        matched += 1;
    }
    assert!(matched >= 4, "{matched} pièces appariées par leur nom");
    // La pièce de devant : fiche, droit-fil, crans (1 simple + 1 double = 3 traits).
    let front = p
        .pieces
        .iter()
        .find(|x| x.name == "Devant é")
        .expect("devant");
    assert_eq!(front.code.as_deref(), Some("D-01"));
    assert_eq!(front.quantity, 2);
    assert!(front.mirrored_pair);
    assert_eq!(p.piece_grain_lines(front.id).len(), 1);
    assert_eq!(p.piece_sheet(front.id).unwrap().notches.len(), 3);
    // Tout ce qui n'est pas importé est compté : flèche du droit-fil, perçages, etc.
    assert!(b.report.ignored_of(AamaReason::GrainMark) >= 2);
    assert!(b.report.ignored_total() > 0);
}

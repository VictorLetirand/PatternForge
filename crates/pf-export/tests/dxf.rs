//! US-12.1 — export DXF de type AAMA / ASTM D6673 : un test au moins par critère. Le fichier est
//! relu par un lecteur DXF **indépendant** (`common/dxf.rs`, aucun code partagé avec l'écrivain)
//! et comparé à la géométrie du modèle à 0,01 mm (`common/dxf_check.rs`). L'ouverture dans un
//! logiciel métier tiers (critère 3 de la story, **humain**) n'est PAS testée : elle reste non
//! prouvée. Aucune règle de gradation (`.rul`) : la gradation n'existe pas.

mod common;

use common::dxf::*;
use common::dxf_check::*;
use common::dxf_models::*;
use common::pt;
use pf_core::{Command, PathRole, Placement, PointRef, Project};
use pf_export::dxf::{
    export_dxf, layer_of, DxfError, DxfIssueKind, LayerRole, DXF_VERSION, LAYERS,
    LAYER_TABLE_CONFIRMED,
};

fn export(p: &Project) -> pf_export::dxf::DxfExport {
    export_dxf(p).expect("export")
}

// ----- CA1 : écrivain DXF, relu par un lecteur indépendant, géométrie à 0,01 mm ----------------

#[test]
fn ca1_projet_riche_relu_par_le_lecteur_independant_a_0_01_mm() {
    let r = rich_project();
    let before = r.project.clone();
    let out = export(&r.project);
    assert_eq!(r.project, before, "le projet n'est jamais modifié");
    let (dxf, done) = verify_export(&r.project, &out.text, &out.report).expect("relecture");
    // 7 pièces au projet ; seule l'étiquette (sans contour) n'est pas écrite.
    assert_eq!(r.project.pieces.len(), 7);
    assert_eq!(done.pieces, 6);
    assert_eq!(out.report.pieces.len(), 6);
    assert!(done.notch_lines >= 7, "crans écrits : {}", done.notch_lines);
    assert!(done.grain_lines >= 1);
    assert!(done.circles >= 4, "perçage + 3 boutons + cercle interne");
    // Structure : version, sections, EOF final (lus par le lecteur) ; rien de non ASCII.
    assert_eq!(dxf.header_str("$ACADVER").as_deref(), Some(DXF_VERSION));
    assert!(out.text.ends_with("  0\nEOF\n"));
    assert!(out.text.is_ascii());
    // Déterminisme : mêmes entrées, mêmes octets.
    assert_eq!(out.bytes(), export(&r.project.clone()).bytes());
    // Le rapport signale l'étiquette (jamais de perte silencieuse).
    assert!(out
        .report
        .omissions()
        .any(|i| i.kind == DxfIssueKind::EmptyOutline && i.piece == "Étiquette"));
}

#[test]
fn ca1_courbes_discretisees_fleche_sous_le_centieme_de_millimetre() {
    let mut p = Project::new("Courbes");
    let s = p.sheets[0].id;
    let (piece, _) = curved_piece(
        &mut p,
        s,
        (0.0, 0.0),
        (300.0, 200.0),
        40.0,
        "Courbes",
        PathRole::CuttingContour,
        false,
    )
    .unwrap();
    let out = export(&p);
    let (dxf, _) = verify_export(&p, &out.text, &out.report).unwrap();
    // Plus de sommets que de tracés : les courbes sont réellement discrétisées.
    let blocks = dxf.inserted().unwrap();
    let Ent::Polyline { pts, .. } = blocks[0]
        .1
        .iter()
        .find(|e| matches!(e, Ent::Polyline { layer, .. } if layer == L_BOUNDARY))
        .unwrap()
    else {
        unreachable!()
    };
    assert!(pts.len() > 100, "{} sommets", pts.len());
    // Aire et périmètre exacts du modèle, relus à 0,01 mm.
    let c = p.piece_contour(piece).unwrap();
    assert!((perimeter(pts, true) - c.perimeter_mm).abs() <= TOL);
}

#[test]
fn ca1_nombres_sans_notation_ambigue() {
    let r = rich_project();
    let out = export(&r.project);
    for (i, line) in out.text.lines().enumerate() {
        if i % 2 == 1 {
            // Ligne de valeur : jamais d'exposant, ni NaN ni inf, dans une valeur numérique.
            let t = line.trim();
            if t.parse::<f64>().is_ok() {
                assert!(!t.contains(['e', 'E']), "exposant : {t}");
                assert!(!t.starts_with('+'));
                assert_ne!(t, "-0.00000");
            }
        }
    }
    let lower = out.text.to_lowercase();
    assert!(!lower.contains("nan") && !lower.contains("inf"));
}

// ----- CA2 : couches normalisées ----------------------------------------------------------------

#[test]
fn ca2_table_de_couches_unique_et_marquee_a_confirmer() {
    // Numéros attendus (réécrits ici) : 1 contour, 2 rotation, 3 courbe, 4 crans, 6 droit-fil,
    // 7 lignes internes, 8 découpes internes, 10 miroir, 13 perçages, 15 couture, 84 texte,
    // 90 jambes de pince et 91 bords sur le pli (numéros libres, déclarés seulement quand une
    // pince ou un bord sur le pli est écrit).
    let numbers: Vec<u32> = LAYERS.iter().map(|l| l.number).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4, 6, 7, 8, 10, 13, 15, 84, 90, 91]);
    for role in [
        LayerRole::Boundary,
        LayerRole::TurnPoints,
        LayerRole::CurvePoints,
        LayerRole::Notches,
        LayerRole::Grainline,
        LayerRole::InternalLines,
        LayerRole::InternalCutouts,
        LayerRole::Mirror,
        LayerRole::Drill,
        LayerRole::Sewing,
        LayerRole::Text,
        LayerRole::DartLegs,
        LayerRole::FoldLine,
    ] {
        assert_eq!(LAYERS.iter().filter(|l| l.role == role).count(), 1);
        let _ = layer_of(role);
    }
    // Les couches réservées (points de rotation / courbe, miroir) ne portent rien.
    let reserved: Vec<u32> = LAYERS
        .iter()
        .filter(|l| !l.written)
        .map(|l| l.number)
        .collect();
    assert_eq!(reserved, vec![2, 3, 10]);
    assert!(
        !std::hint::black_box(LAYER_TABLE_CONFIRMED),
        "la table n'est pas confirmée avec la spécification officielle"
    );
    // La table est déclarée dans le fichier : « 0 » et toutes les couches de la table.
    let r = rich_project();
    let out = export(&r.project);
    let dxf = parse(&out.text).unwrap();
    let names: Vec<&str> = dxf.layers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names[0], "0");
    // (sans pince, la couche 90 n'est pas déclarée : le fichier reste identique à l'octet).
    assert!(!names.contains(&"90") && !names.contains(&"91"));
    for n in numbers.iter().filter(|n| !matches!(**n, 90 | 91)) {
        assert!(names.contains(&n.to_string().as_str()), "couche {n}");
    }
}

#[test]
fn ca2_chaque_role_sur_sa_couche() {
    let r = rich_project();
    let out = export(&r.project);
    let (_, done) = verify_export(&r.project, &out.text, &out.report).unwrap();
    // Le projet riche remplit les couches 1, 4, 6, 7, 8, 13, 15, 84 et jamais les réservées.
    for used in ["1", "4", "6", "7", "8", "13", "15", "84"] {
        assert!(
            done.layer_counts.get(used).copied().unwrap_or(0) > 0,
            "couche {used} vide"
        );
    }
    for reserved in ["2", "3", "10"] {
        assert!(
            !done.layer_counts.contains_key(reserved),
            "couche {reserved} écrite"
        );
    }
    // Rapport : couches utilisées, dans l'ordre de la table.
    let used: Vec<u32> = out.report.layers.iter().map(|l| l.layer.number).collect();
    assert_eq!(used, vec![1, 4, 6, 7, 8, 13, 15, 84]);
}

#[test]
fn ca2_contour_de_coupe_jamais_invente() {
    // Rôle coupe → couche 1 seule ; rôle couture → couche 15 seule, remarque « aucun contour
    // de coupe » ; autre rôle → couche 1, remarque « pas un tracé de coupe ».
    for (role, layer, note) in [
        (PathRole::CuttingContour, "1", None),
        (
            PathRole::SewingContour,
            "15",
            Some(DxfIssueKind::NoCutContour),
        ),
        (
            PathRole::Construction,
            "1",
            Some(DxfIssueKind::BoundaryNotCut),
        ),
    ] {
        let mut p = Project::new("Rôles");
        let s = p.sheets[0].id;
        rect_piece(&mut p, s, (0.0, 0.0), (100.0, 60.0), "A", role, false);
        let out = export(&p);
        let dxf = parse(&out.text).unwrap();
        let ents = dxf.inserted().unwrap()[0].1;
        let polys: Vec<&str> = ents
            .iter()
            .filter(|e| matches!(e, Ent::Polyline { .. }))
            .map(Ent::layer)
            .collect();
        assert_eq!(polys, vec![layer], "{role:?}");
        let notes: Vec<_> = out.report.notes().map(|i| i.kind).collect();
        let got: Vec<_> = notes
            .into_iter()
            .filter(|k| *k != DxfIssueKind::MissingGrain)
            .collect();
        assert_eq!(got, note.into_iter().collect::<Vec<_>>(), "{role:?}");
    }
}

// ----- CA3 : pièces, placement ------------------------------------------------------------------

#[test]
fn ca3_toutes_les_sortes_de_pieces_avec_nom_code_quantite() {
    let r = rich_project();
    let out = export(&r.project);
    let (dxf, _) = verify_export(&r.project, &out.text, &out.report).unwrap();
    let names: Vec<&str> = out.report.pieces.iter().map(|p| p.name.as_str()).collect();
    for expected in ["Devant é", "Courbe", "Détachée", "Moitié"] {
        assert!(names.contains(&expected), "{expected} : {names:?}");
    }
    // Dérivée (doublure décalée) et dépliée : écrites comme les autres pièces.
    for id in [r.derived, r.unfolded, r.detached, r.curved] {
        assert!(out.report.pieces.iter().any(|p| p.id == id));
    }
    assert!(!out.report.pieces.iter().any(|p| p.id == r.label));
    // Un bloc par pièce, nommé d'après son identifiant.
    let blocks: Vec<&str> = dxf.blocks.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(blocks.len(), out.report.pieces.len());
    for info in &out.report.pieces {
        assert_eq!(info.block, format!("PIECE_{}", info.id.0));
        assert!(blocks.contains(&info.block.as_str()));
    }
    // Nom accentué (« é ») écrit en ASCII `\U+00E9`, relu à l'identique ; code et quantité.
    assert!(out.text.contains("Piece Name: Dev"));
    assert!(out.text.contains("\\U+00E9"));
    let texts: Vec<String> = dxf
        .blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for want in [
        "Piece Name: Devant é",
        "Code: D-01",
        "Quantity: 2",
        "Material: Coton",
        "Mirrored pair: yes",
    ] {
        assert!(
            texts.iter().any(|t| t == want),
            "{want} absent de {texts:?}"
        );
    }
}

#[test]
fn ca3_placement_manuel_garde_sa_position_et_sa_rotation() {
    let mut p = Project::new("Placement");
    let s = p.sheets[0].id;
    let (a, _) = rect_piece(
        &mut p,
        s,
        (0.0, 0.0),
        (100.0, 40.0),
        "A",
        PathRole::CuttingContour,
        false,
    );
    let (b, _) = rect_piece(
        &mut p,
        s,
        (300.0, 0.0),
        (60.0, 60.0),
        "B",
        PathRole::CuttingContour,
        false,
    );
    p.apply(Command::SetPiecePlacement {
        piece: a,
        placement: Some(Placement {
            x_mm: 500.0,
            y_mm: -250.0,
            rotation_deg: 0.0,
        }),
    })
    .unwrap();
    p.apply(Command::SetPiecePlacement {
        piece: b,
        placement: Some(Placement {
            x_mm: -80.0,
            y_mm: 40.0,
            rotation_deg: 90.0,
        }),
    })
    .unwrap();
    let out = export(&p);
    let (dxf, _) = verify_export(&p, &out.text, &out.report).unwrap();
    let boxes: Vec<(P, P)> = dxf
        .inserted()
        .unwrap()
        .iter()
        .map(|(_, e)| {
            let Some(Ent::Polyline { pts, .. }) = e
                .iter()
                .find(|e| matches!(e, Ent::Polyline { layer, .. } if layer == L_BOUNDARY))
            else {
                panic!("contour")
            };
            bbox(pts)
        })
        .collect();
    // A : 100 × 40 centré en (500 ; −250). B : 60 × 60 centré en (−80 ; 40).
    let center = |b: &(P, P)| ((b.0 .0 + b.1 .0) / 2.0, (b.0 .1 + b.1 .1) / 2.0);
    assert!(dist(center(&boxes[0]), (500.0, -250.0)) < 1e-3);
    assert!((boxes[0].1 .0 - boxes[0].0 .0 - 100.0).abs() < 1e-3);
    assert!(dist(center(&boxes[1]), (-80.0, 40.0)) < 1e-3);
    // Rotation de 90° : un rectangle 100 × 40 donnerait 40 × 100 (vérifié sur B en carré,
    // et sur A tournée ci-dessous).
    let mut q = p.clone();
    q.apply(Command::SetPiecePlacement {
        piece: a,
        placement: Some(Placement {
            x_mm: 0.0,
            y_mm: 0.0,
            rotation_deg: 90.0,
        }),
    })
    .unwrap();
    let out = export(&q);
    verify_export(&q, &out.text, &out.report).unwrap();
    let dxf = parse(&out.text).unwrap();
    let Some(Ent::Polyline { pts, .. }) = dxf.blocks[0]
        .1
        .iter()
        .find(|e| matches!(e, Ent::Polyline { layer, .. } if layer == L_BOUNDARY))
    else {
        panic!()
    };
    let (lo, hi) = bbox(pts);
    assert!((hi.0 - lo.0 - 40.0).abs() < 1e-3 && (hi.1 - lo.1 - 100.0).abs() < 1e-3);
}

#[test]
fn ca3_placement_automatique_sans_chevauchement() {
    let mut p = Project::new("Auto");
    let s = p.sheets[0].id;
    for i in 0..6 {
        let w = 80.0 + 40.0 * f64::from(i);
        rect_piece(
            &mut p,
            s,
            (f64::from(i) * 5.0, f64::from(i) * 5.0),
            (w, 50.0 + f64::from(i) * 10.0),
            &format!("P{i}"),
            PathRole::CuttingContour,
            false,
        );
    }
    let out = export(&p);
    // `verify_export` contrôle la disjonction des boîtes des pièces au placement automatique.
    let (_, done) = verify_export(&p, &out.text, &out.report).unwrap();
    assert_eq!(done.pieces, 6);
}

// ----- CA4 : unités et échelle ------------------------------------------------------------------

#[test]
fn ca4_unites_millimetres_et_echelle_1_pour_1() {
    let mut p = Project::new("Échelle");
    let s = p.sheets[0].id;
    // Un carré de 100 mm de côté, placé à la main en (0 ; 0).
    let (piece, _) = rect_piece(
        &mut p,
        s,
        (10.0, 10.0),
        (100.0, 100.0),
        "Carré",
        PathRole::CuttingContour,
        false,
    );
    p.apply(Command::SetPiecePlacement {
        piece,
        placement: Some(Placement {
            x_mm: 0.0,
            y_mm: 0.0,
            rotation_deg: 0.0,
        }),
    })
    .unwrap();
    let out = export(&p);
    let dxf = parse(&out.text).unwrap();
    assert_eq!(dxf.header_int("$INSUNITS"), Some(4), "millimètres");
    assert_eq!(dxf.header_int("$MEASUREMENT"), Some(1), "métrique");
    assert_eq!(dxf.header_str("$ACADVER").as_deref(), Some("AC1009"));
    // Échelle 1:1 : le carré de 100 mm occupe 100 unités de dessin, centré sur l'origine.
    let Some(Ent::Polyline { pts, .. }) = dxf.blocks[0]
        .1
        .iter()
        .find(|e| matches!(e, Ent::Polyline { layer, .. } if layer == L_BOUNDARY))
    else {
        panic!()
    };
    let (lo, hi) = bbox(pts);
    assert!((lo.0 + 50.0).abs() < 1e-4 && (hi.0 - 50.0).abs() < 1e-4);
    assert!((lo.1 + 50.0).abs() < 1e-4 && (hi.1 - 50.0).abs() < 1e-4);
    // Emprise déclarée : contient le carré (texte compris).
    let (emin, emax) = (
        dxf.header_point("$EXTMIN").unwrap(),
        dxf.header_point("$EXTMAX").unwrap(),
    );
    assert!(emin.0 <= -50.0 + 1e-4 && emin.1 <= -50.0 + 1e-4);
    assert!(emax.0 >= 50.0 - 1e-4 && emax.1 >= 50.0 - 1e-4);
}

// ----- CA6 : refus sans trace, rapport, jamais de perte silencieuse ------------------------------

#[test]
fn ca6_projet_sans_piece_refuse_proprement() {
    let p = Project::new("Vide");
    let before = p.clone();
    let err = export_dxf(&p).unwrap_err();
    assert_eq!(err, DxfError::NoPieces);
    assert!(err.to_string().contains("aucune pièce"));
    assert_eq!(p, before);
    // Construction sans pièce : refusé aussi.
    let mut q = Project::new("Construction");
    let s = q.sheets[0].id;
    q.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(0.0, 0.0)),
        to: PointRef::New(pt(10.0, 0.0)),
    })
    .unwrap();
    assert_eq!(export_dxf(&q).unwrap_err(), DxfError::NoPieces);
}

#[test]
fn ca6_pieces_non_ecrites_toujours_signalees() {
    let mut p = Project::new("Dégénérées");
    let s = p.sheets[0].id;
    let label = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Étiquette seule".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    // Aucune pièce écrivable : erreur qui porte le détail, rien n'est produit.
    let before = p.clone();
    match export_dxf(&p) {
        Err(DxfError::NothingExported(issues)) => {
            assert_eq!(issues.len(), 1);
            assert_eq!(issues[0].kind, DxfIssueKind::EmptyOutline);
            assert_eq!(issues[0].piece, "Étiquette seule");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(p, before);
    // Contour ouvert : une pièce valide à côté est écrite, l'autre est signalée.
    let e = seg(&mut p, s, (0.0, 0.0), (50.0, 50.0), PathRole::Construction);
    let open = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Ouverte".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    let _ = p.apply(Command::SetPieceOutline {
        piece: open,
        outline: vec![pf_core::PieceEdge {
            entity: e,
            reversed: false,
        }],
        grainline: None,
    });
    rect_piece(
        &mut p,
        s,
        (200.0, 0.0),
        (50.0, 50.0),
        "Bonne",
        PathRole::CuttingContour,
        false,
    );
    let before = p.clone();
    let out = export(&p);
    assert_eq!(p, before);
    verify_export(&p, &out.text, &out.report).unwrap();
    assert_eq!(out.report.pieces.len(), 1);
    let omitted: Vec<_> = out
        .report
        .omissions()
        .map(|i| (i.piece.as_str(), i.kind))
        .collect();
    assert!(omitted.contains(&("Étiquette seule", DxfIssueKind::EmptyOutline)));
    if p.piece(open).unwrap().outline.is_empty() {
        // Le contour ouvert a été refusé par la validation : rien d'autre à signaler.
        assert_eq!(omitted.len(), 1);
    } else {
        assert!(omitted.contains(&("Ouverte", DxfIssueKind::OutlineOpen)));
    }
    let _ = label;
}

#[test]
fn ca6_element_non_ecrit_est_dans_le_rapport() {
    // Un nom trop long est tronqué : signalé, jamais en silence.
    let mut p = Project::new("Long");
    let s = p.sheets[0].id;
    let long = "N".repeat(120);
    rect_piece(
        &mut p,
        s,
        (0.0, 0.0),
        (100.0, 60.0),
        &long,
        PathRole::CuttingContour,
        false,
    );
    let out = export(&p);
    assert!(out
        .report
        .omissions()
        .any(|i| i.kind == DxfIssueKind::TextTruncated));
    let dxf = parse(&out.text).unwrap();
    let texts: Vec<String> = dxf.blocks[0]
        .1
        .iter()
        .filter_map(|e| match e {
            Ent::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts[0].starts_with("Piece Name: NNN"));
    assert!(texts[0].len() < 100);
}

// ----- CA8 : honnêteté -------------------------------------------------------------------------

#[test]
fn ca8_le_fichier_ne_pretend_pas_etre_valide_ni_compatible() {
    let r = rich_project();
    let out = export(&r.project);
    let lower = out.text.to_lowercase();
    for word in ["compatible", "valid", "certif", "aama", "astm"] {
        assert!(!lower.contains(word), "le fichier contient « {word} »");
    }
    // Aucun fichier de règles de gradation : la taille unique est exportée sans règles.
    assert!(!lower.contains(".rul"));
}

// ----- second lecteur optionnel : ezdxf (Python) ------------------------------------------------

/// Relit le fichier avec `ezdxf` s'il est installé (sinon le test est ignoré **proprement** : il
/// affiche « ezdxf absent » et réussit sans rien vérifier). Compare, par bloc et par couche, le
/// nombre d'entités, et l'aire et le périmètre des polylignes, à ceux du lecteur indépendant.
#[test]
fn second_lecteur_ezdxf_optionnel() {
    use std::process::Command as Proc;
    let present = Proc::new("python")
        .args(["-I", "-c", "import ezdxf"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !present {
        eprintln!("ezdxf absent : second lecteur non exécuté (test ignoré)");
        return;
    }
    let r = rich_project();
    let out = export(&r.project);
    let dir = std::env::temp_dir().join(format!("pf-dxf-ezdxf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("export.dxf");
    std::fs::write(&file, out.bytes()).unwrap();
    const SCRIPT: &str = "
import sys, ezdxf
doc = ezdxf.readfile(sys.argv[1])
print('UNITS', doc.header['$INSUNITS'])
for b in doc.blocks:
    if not b.name.startswith('PIECE_'):
        continue
    counts = {}
    for e in b:
        counts[e.dxf.layer] = counts.get(e.dxf.layer, 0) + 1
        if e.dxftype() == 'POLYLINE':
            pts = [(v.dxf.location.x, v.dxf.location.y) for v in e.vertices]
            n = len(pts)
            seg = n if e.is_closed else n - 1
            per = sum(((pts[(i+1)%n][0]-pts[i][0])**2 + (pts[(i+1)%n][1]-pts[i][1])**2) ** 0.5 for i in range(seg))
            ar = abs(sum(pts[i][0]*pts[(i+1)%n][1] - pts[(i+1)%n][0]*pts[i][1] for i in range(n))) / 2
            print('POLY', b.name, e.dxf.layer, int(e.is_closed), ar, per)
    for k, v in sorted(counts.items()):
        print('COUNT', b.name, k, v)
";
    let res = Proc::new("python")
        .args(["-I", "-c", SCRIPT])
        .arg(&file)
        .output()
        .expect("python");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        res.status.success(),
        "ezdxf : {}",
        String::from_utf8_lossy(&res.stderr)
    );
    let text = String::from_utf8_lossy(&res.stdout).to_string();
    assert!(text.contains("UNITS 4"), "{text}");
    let mine = parse(&out.text).unwrap();
    for (name, ents) in &mine.blocks {
        for layer in ["1", "4", "6", "7", "8", "13", "15", "84"] {
            let n = ents.iter().filter(|e| e.layer() == layer).count();
            let line = format!("COUNT {name} {layer} {n}");
            let seen = text.lines().any(|l| l == line);
            assert_eq!(seen, n > 0, "{line} : {text}");
        }
        for e in ents {
            if let Ent::Polyline { layer, pts, closed } = e {
                let prefix = format!("POLY {name} {layer} {}", i32::from(*closed));
                let got = text
                    .lines()
                    .find(|l| l.starts_with(&prefix))
                    .expect("polyligne vue par ezdxf");
                let v: Vec<f64> = got.split(' ').skip(4).map(|x| x.parse().unwrap()).collect();
                assert!(
                    (v[0] - area(pts)).abs() < 0.01
                        && (v[1] - perimeter(pts, *closed)).abs() < 0.01
                );
            }
        }
    }
}

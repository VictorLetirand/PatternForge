//! US-17.2 - comparaison de versions (`pf_core::version_diff`) : valeurs **écrites à la main**.
//!
//! Fixture : feuille avec P1 (10 ; 20), P2 (100 ; 0), un segment P1-P2 ; une pièce « A » (rectangle
//! 100 x 50 en (0 ; 0)).

use pf_core::version_diff::*;
use pf_core::*;
use pf_geometry::PointMm;

struct Fx {
    p: Project,
    s: SheetId,
    p1: PointId,
    seg: EntityId,
}

fn fx() -> Fx {
    let mut p = Project::new("V");
    let s = p.sheets[0].id;
    let p1 = p
        .apply(Command::AddPoint {
            sheet: s,
            at: PointMm::new(10.0, 20.0),
        })
        .unwrap()
        .points[0];
    let p2 = p
        .apply(Command::AddPoint {
            sheet: s,
            at: PointMm::new(100.0, 0.0),
        })
        .unwrap()
        .points[0];
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::Existing(p1),
            to: PointRef::Existing(p2),
        })
        .unwrap()
        .entities[0];
    Fx { p, s, p1, seg }
}

#[test]
fn projets_identiques_aucune_difference() {
    let f = fx();
    let d = diff_projects(&f.p, &f.p.clone());
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn point_deplace_de_10_20_vers_13_24_vaut_5_mm() {
    let f = fx();
    let mut n = f.p.clone();
    n.apply(Command::MovePoint {
        sheet: f.s,
        point: f.p1,
        to: PointMm::new(13.0, 24.0),
    })
    .unwrap();
    let d = diff_projects(&f.p, &n);
    assert_eq!(d.sheets.len(), 1);
    let sd = &d.sheets[0];
    assert!(sd.points_added.is_empty() && sd.points_removed.is_empty());
    assert_eq!(sd.points_moved.len(), 1);
    let m = &sd.points_moved[0];
    // dx = 3, dy = 4, hypotenuse = 5 (triangle 3-4-5).
    assert!((m.distance_mm - 5.0).abs() < 1e-12, "{}", m.distance_mm);
    assert_eq!(
        (m.from, m.to),
        (PointMm::new(10.0, 20.0), PointMm::new(13.0, 24.0))
    );
    // Le segment n'est pas « modifié » : ses extrémités gardent leurs identifiants.
    assert!(sd.paths_changed.is_empty());
}

#[test]
fn deplacement_sous_le_seuil_est_ignore() {
    let f = fx();
    let mut n = f.p.clone();
    n.apply(Command::MovePoint {
        sheet: f.s,
        point: f.p1,
        to: PointMm::new(10.0 + 5e-7, 20.0),
    })
    .unwrap();
    assert!(diff_projects(&f.p, &n).is_empty());
}

#[test]
fn points_et_traces_ajoutes_supprimes() {
    let f = fx();
    let mut n = f.p.clone();
    let p3 = n
        .apply(Command::AddPoint {
            sheet: f.s,
            at: PointMm::new(5.0, 5.0),
        })
        .unwrap()
        .points[0];
    n.apply(Command::AddSegment {
        sheet: f.s,
        from: PointRef::Existing(p3),
        to: PointRef::Existing(f.p1),
    })
    .unwrap();
    let d = diff_projects(&f.p, &n);
    let sd = &d.sheets[0];
    assert_eq!(sd.points_added.len(), 1);
    assert_eq!(sd.points_added[0].position, PointMm::new(5.0, 5.0));
    assert_eq!(sd.paths_added.len(), 1);
    assert!(sd.paths_removed.is_empty());
    // Sens inverse : ce qui était ajouté devient supprimé.
    let back = diff_projects(&n, &f.p);
    assert_eq!(back.sheets[0].points_removed.len(), 1);
    assert_eq!(back.sheets[0].paths_removed.len(), 1);
    assert!(back.sheets[0].points_added.is_empty());
}

#[test]
fn trace_supprime_est_liste() {
    let f = fx();
    let mut n = f.p.clone();
    n.apply(Command::Delete {
        sheet: f.s,
        points: vec![],
        entities: vec![f.seg],
    })
    .unwrap();
    let d = diff_projects(&f.p, &n);
    assert_eq!(d.sheets[0].paths_removed.len(), 1);
    assert_eq!(d.sheets[0].paths_removed[0].id, f.seg);
}

#[test]
fn feuille_renommee_ajoutee_supprimee() {
    let f = fx();
    let mut n = f.p.clone();
    n.apply(Command::RenameSheet {
        sheet: f.s,
        name: "Dos".into(),
    })
    .unwrap();
    let new_sheet = n
        .apply(Command::CreateSheet {
            name: "Manche".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let d = diff_projects(&f.p, &n);
    assert_eq!(d.sheets[0].renamed.as_ref().unwrap().1, "Dos");
    assert_eq!(d.sheets_added, vec![(new_sheet, "Manche".to_string())]);
    let back = diff_projects(&n, &f.p);
    assert_eq!(back.sheets_removed, vec![(new_sheet, "Manche".to_string())]);
}

fn with_piece() -> (Project, PieceId) {
    let mut p = Project::new("P");
    let s = p.sheets[0].id;
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(0.0, 0.0)),
            width_mm: 100.0,
            height_mm: 50.0,
        })
        .unwrap();
    let id = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: r.entities,
            },
            name: "A".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, id)
}

#[test]
fn piece_changee_nom_matiere_marges() {
    let (p, id) = with_piece();
    let mut n = p.clone();
    n.apply(Command::SetPieceCard {
        piece: id,
        name: "Dos".into(),
        code: None,
        quantity: 1,
        mirrored_pair: false,
        material: Some("Cuir".into()),
        category: None,
        comment: None,
    })
    .unwrap();
    n.apply(Command::SetSeamAllowance {
        piece: id,
        seam: Some(pf_core::piece_seam::SeamAllowance {
            default_mm: 10.0,
            edges: vec![],
            hems: vec![],
            folds: vec![],
            corners: vec![],
            max_corner_mm: None,
        }),
    })
    .unwrap();
    let d = diff_projects(&p, &n);
    assert_eq!(d.pieces.len(), 1);
    assert_eq!(
        d.pieces[0].changes,
        vec![
            PieceChange::Name {
                from: "A".into(),
                to: "Dos".into()
            },
            PieceChange::Material {
                from: None,
                to: Some("Cuir".into())
            },
            PieceChange::Seam,
        ]
    );
    // Aucun point ni tracé ne bouge : seule la fiche change.
    assert!(d.sheets.is_empty());
}

#[test]
fn variables_et_tailles_changees() {
    let mut p = Project::new("V");
    p.apply(Command::AddVariable {
        name: "tour".into(),
        unit: pf_core::variable::VariableUnit::Mm,
        expr: "90".into(),
    })
    .unwrap();
    p.apply(Command::AddVariable {
        name: "gone".into(),
        unit: pf_core::variable::VariableUnit::Mm,
        expr: "1".into(),
    })
    .unwrap();
    let mut n = p.clone();
    n.apply(Command::SetVariableExpr {
        name: "tour".into(),
        expr: "92".into(),
    })
    .unwrap();
    n.apply(Command::AddVariable {
        name: "neuf".into(),
        unit: pf_core::variable::VariableUnit::Mm,
        expr: "2".into(),
    })
    .unwrap();
    n.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into()],
        base: 0,
    })
    .unwrap();
    let d = diff_projects(&p, &n);
    // Tri par nom : « neuf » < « tour ».
    let got: Vec<(String, Presence)> = d
        .variables
        .iter()
        .map(|c| (c.name.clone(), c.change.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("neuf".to_string(), Presence::Added),
            ("tour".to_string(), Presence::Changed),
        ]
    );
    let removed = diff_projects(&n, &p);
    assert!(removed
        .variables
        .iter()
        .any(|c| c.name == "neuf" && c.change == Presence::Removed));
    assert_eq!(
        d.sizes,
        vec![NamedChange {
            name: "Femme".into(),
            change: Presence::Added
        }]
    );
}

#[test]
fn sortie_deterministe_et_triee() {
    let f = fx();
    let mut n = f.p.clone();
    for i in 0..5 {
        n.apply(Command::AddPoint {
            sheet: f.s,
            at: PointMm::new(200.0 + f64::from(i), 7.0),
        })
        .unwrap();
    }
    let a = diff_projects(&f.p, &n);
    let b = diff_projects(&f.p, &n);
    assert_eq!(a, b);
    let ids: Vec<u64> = a.sheets[0].points_added.iter().map(|p| p.id.0).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
    assert_eq!(ids.len(), 5);
}

#[test]
fn superposition_ajoute_supprime_deplace() {
    let f = fx();
    let mut n = f.p.clone();
    n.apply(Command::MovePoint {
        sheet: f.s,
        point: f.p1,
        to: PointMm::new(13.0, 24.0),
    })
    .unwrap();
    let extra = n
        .apply(Command::AddSegmentPolar {
            sheet: f.s,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            length_mm: 30.0,
            angle_deg: 0.0,
        })
        .unwrap();
    assert_eq!(extra.entities.len(), 1);
    let o = overlay(&f.p, &n, f.s);
    let count = |k| o.lines.iter().filter(|l| l.kind == k).count();
    assert_eq!(count(OverlayKind::Kept), 1);
    assert_eq!(count(OverlayKind::Added), 1);
    assert_eq!(count(OverlayKind::Removed), 0);
    // Le segment conservé est dessiné dans le NOUVEL état : P1 en (13 ; 24).
    let kept = o
        .lines
        .iter()
        .find(|l| l.kind == OverlayKind::Kept)
        .unwrap();
    assert_eq!(kept.from, PointMm::new(13.0, 24.0));
    assert_eq!(o.moves.len(), 1);
    assert!((o.moves[0].distance_mm - 5.0).abs() < 1e-12);
    // À l'envers, le segment ajouté devient « supprimé » (dessiné dans l'ancien état).
    let back = overlay(&n, &f.p, f.s);
    assert_eq!(
        back.lines
            .iter()
            .filter(|l| l.kind == OverlayKind::Removed)
            .count(),
        1
    );
}

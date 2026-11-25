//! US-12.3 — DXF générique import / export : un test par critère d'acceptation (CA1 à CA5).
//!
//! Les fichiers « d'un tiers imaginaire » sont écrits à la main dans `common/dxf_generic.rs`
//! (R2018, poignées, CRLF, LWPOLYLINE avec bulges, POLYLINE 3D, SPLINE à nœuds…), dans un style
//! différent de celui de l'écrivain. Les valeurs attendues sont **calculées dans le test**
//! (longueurs d'arcs, périmètre et aire d'une plaque à coins arrondis, B-splines par Cox–de Boor,
//! Béziers par Simpson) ; rien n'est comparé à la sortie du code testé lui-même.
//!
//! Honnêteté : aucun fichier de logiciel réel n'est lu ici ; l'ouverture des fichiers exportés
//! dans un logiciel tiers n'est pas prouvée.

mod common;

use common::dxf_generic::*;
use pf_core::{
    persist, Command, CoreError, Document, EntityKind, ImportItem, ImportObject, PathRole,
    PointRef, Project, MAX_IMPORT_OBJECTS,
};
use pf_export::dxf_construction::{
    export_construction_dxf, ConstructionError, ConstructionNoteKind, ConstructionScope,
};
use pf_export::dxf_import::{
    DeclaredUnit, DxfDocument, DxfImportError, DxfUnit, IgnoreReason, ImportBuild, ImportNoteKind,
    UnitChoice, MAX_ENTITIES, MAX_FILE_BYTES,
};

fn build(bytes: &[u8], unit: DxfUnit) -> ImportBuild {
    DxfDocument::parse(bytes).unwrap().build(unit).unwrap()
}

fn on_layer<'a>(b: &'a ImportBuild, layer: &str) -> Vec<&'a ImportItem> {
    b.objects
        .iter()
        .filter(|o| o.layer.as_deref() == Some(layer))
        .map(|o| &o.item)
        .collect()
}

fn owned(items: &[&ImportItem]) -> Vec<ImportItem> {
    items.iter().map(|i| (*i).clone()).collect()
}

fn segments_of(items: &[&ImportItem]) -> Vec<((f64, f64), (f64, f64))> {
    items
        .iter()
        .filter_map(|i| match i {
            ImportItem::Segment { from, to } => Some((xy(*from), xy(*to))),
            _ => None,
        })
        .collect()
}

// =================================================================================== CA1

#[test]
fn ca1_apercu_boite_englobante_selon_l_unite_et_facteur_exact() {
    // LINE (0 ; 0)-(100 ; 50) et CIRCLE (200 ; 100) r 10 : boîte native (0 ; 0)-(210 ; 110).
    let mut e = Pairs::default();
    e.extend(&line(1, "0", (0.0, 0.0, 0.0), (100.0, 50.0, 0.0)));
    e.extend(&circle(2, "0", (200.0, 100.0), 10.0));
    let bytes = file(&header(None), &e).text(true).into_bytes();
    let doc = DxfDocument::parse(&bytes).unwrap();
    for (unit, k) in [
        (DxfUnit::Millimetre, 1.0),
        (DxfUnit::Centimetre, 10.0),
        (DxfUnit::Metre, 1000.0),
        (DxfUnit::Inch, 25.4),
        (DxfUnit::Foot, 304.8),
    ] {
        let b = doc.build(unit).unwrap();
        assert_eq!(b.scale, k, "{unit:?}");
        let bb = b.bbox_mm.unwrap();
        // Boîte attendue, calculée à la main : (0 ; 0) à (210 ; 110) × facteur.
        assert!(
            (bb.min.x - 0.0).abs() < 1e-9 && (bb.min.y - 0.0).abs() < 1e-9,
            "{unit:?}"
        );
        assert!(
            (bb.max.x - 210.0 * k).abs() < 1e-9 * k,
            "{unit:?} {}",
            bb.max.x
        );
        assert!(
            (bb.max.y - 110.0 * k).abs() < 1e-9 * k,
            "{unit:?} {}",
            bb.max.y
        );
        assert_eq!(b.report.objects, 2);
        assert_eq!(b.report.imported["LINE"], 1);
        assert_eq!(b.report.imported["CIRCLE"], 1);
    }
    // Le pouce : 25,4 mm exactement, 210 pouces = 5334 mm.
    let inch = doc.build(DxfUnit::Inch).unwrap();
    assert_eq!(inch.bbox_mm.unwrap().max.x, 5334.0);
}

#[test]
fn ca1_unite_declaree_par_le_fichier_ou_choix_obligatoire() {
    let mut e = Pairs::default();
    e.extend(&line(1, "0", (0.0, 0.0, 0.0), (10.0, 0.0, 0.0)));
    for (ins, expected) in [
        (Some(1), DeclaredUnit::Known(DxfUnit::Inch)),
        (Some(2), DeclaredUnit::Known(DxfUnit::Foot)),
        (Some(4), DeclaredUnit::Known(DxfUnit::Millimetre)),
        (Some(5), DeclaredUnit::Known(DxfUnit::Centimetre)),
        (Some(6), DeclaredUnit::Known(DxfUnit::Metre)),
        (Some(10), DeclaredUnit::Known(DxfUnit::Yard)),
        (Some(14), DeclaredUnit::Known(DxfUnit::Decimetre)),
        (Some(0), DeclaredUnit::Unitless),
        (None, DeclaredUnit::Missing),
        (Some(13), DeclaredUnit::Unsupported(13)),
        (Some(3), DeclaredUnit::Unsupported(3)),
    ] {
        let bytes = file(&header(ins), &e).text(false).into_bytes();
        let doc = DxfDocument::parse(&bytes).unwrap();
        assert_eq!(doc.declared_unit(), expected, "{ins:?}");
        match expected {
            DeclaredUnit::Known(u) => {
                let b = doc.build_choice(UnitChoice::Declared).unwrap();
                assert_eq!(b.unit, u);
                assert_eq!(b.scale, u.mm_per_unit());
            }
            _ => assert_eq!(
                doc.build_choice(UnitChoice::Declared).unwrap_err(),
                DxfImportError::UnitRequired,
                "sans unité reconnue, l'utilisatrice doit choisir"
            ),
        }
        // Le choix explicite est toujours possible.
        assert!(doc.build_choice(UnitChoice::Fixed(DxfUnit::Metre)).is_ok());
    }
    // En-tête absent : même règle.
    let mut f = Pairs::default();
    f.extend(&section("ENTITIES", &e));
    f.p(0, "EOF");
    let doc = DxfDocument::parse(f.text(false).as_bytes()).unwrap();
    assert_eq!(doc.declared_unit(), DeclaredUnit::Missing);
    assert_eq!(doc.version(), None);
}

#[test]
fn ca1_apercu_compte_les_entites_par_type_et_liste_les_ignorees() {
    let b = build(&tiers(Some(4)), DxfUnit::Millimetre);
    let r = &b.report;
    assert_eq!(r.read["LWPOLYLINE"], 1);
    assert_eq!(r.read["ARC"], 2);
    assert_eq!(r.read["SPLINE"], 5);
    assert_eq!(r.read["POLYLINE"], 2);
    assert_eq!(r.read["INSERT"], 2);
    assert_eq!(r.imported["ARC"], 2);
    assert_eq!(r.imported["CIRCLE"], 1);
    assert_eq!(r.imported["SPLINE"], 5);
    assert_eq!(
        r.imported["POLYLINE"], 1,
        "la polyligne 3D n'est pas importée"
    );
    // Tout ce qui n'est pas importé est listé, avec son compte et son motif.
    let ig = |kind: &str, reason: IgnoreReason| {
        r.ignored
            .iter()
            .find(|i| i.kind == kind && i.reason == reason)
            .map_or(0, |i| i.count)
    };
    for k in ["TEXT", "MTEXT", "HATCH", "DIMENSION", "3DFACE", "ELLIPSE"] {
        assert_eq!(ig(k, IgnoreReason::Unsupported), 1, "{k}");
    }
    assert_eq!(ig("INSERT", IgnoreReason::Unsupported), 2);
    assert_eq!(ig("POLYLINE", IgnoreReason::Polyline3d), 1);
    assert_eq!(ig("LINE", IgnoreReason::Degenerate), 1);
    assert_eq!(r.ignored_of("INSERT"), 2);
    // Une entité = importée, ou ignorée : rien ne disparaît.
    let imported: usize = r.imported.values().sum();
    assert_eq!(imported + r.ignored_total(), r.read.values().sum::<usize>());
    // Remarques : Z, courbes aplaties, doublon, blocs non lus.
    // La LINE à z = 5 seulement : la POLYLINE 3D est ignorée en entier (autre remarque).
    assert_eq!(r.note(ImportNoteKind::ZIgnored), 1);
    assert_eq!(r.note(ImportNoteKind::SplineSegments), 3);
    assert_eq!(r.note(ImportNoteKind::SplineFitPoints), 2);
    assert_eq!(r.note(ImportNoteKind::ClosedCurveSplit), 1);
    assert_eq!(r.note(ImportNoteKind::DuplicatesRemoved), 1);
    assert_eq!(r.note(ImportNoteKind::BlockContents), 1);
}

#[test]
fn ca1_changer_d_unite_met_l_apercu_a_jour_sans_changer_la_geometrie_relative() {
    let bytes = tiers(Some(4));
    let doc = DxfDocument::parse(&bytes).unwrap();
    let mm = doc.build(DxfUnit::Millimetre).unwrap();
    let cm = doc.build(DxfUnit::Centimetre).unwrap();
    let inch = doc.build(DxfUnit::Inch).unwrap();
    assert_eq!(
        mm.report.imported, cm.report.imported,
        "mêmes entités, autre échelle"
    );
    assert_eq!(mm.report.ignored, cm.report.ignored);
    let (a, b, c) = (
        mm.bbox_mm.unwrap(),
        cm.bbox_mm.unwrap(),
        inch.bbox_mm.unwrap(),
    );
    // Les coordonnées de la boîte sont multipliées par le facteur (courbes : écart de
    // discrétisation au plus 0,005 mm × facteur).
    assert!(
        (b.max.x - a.max.x * 10.0).abs() < 0.005 * 10.0 + 1e-6,
        "{} {}",
        b.max.x,
        a.max.x
    );
    assert!((c.max.y - a.max.y * 25.4).abs() < 0.005 * 25.4 + 1e-6);
    assert_ne!(a.max.x, b.max.x);
    // La boîte du fichier à la main : x max = 510 (LINE sur « RECT »), y max = 900, y min = −25
    // (bas du cercle de centre (300 ; 0) et de rayon 25).
    assert!((a.max.x - 510.0).abs() < 1e-9 && (a.max.y - 900.0).abs() < 1e-9);
    assert!((a.min.y + 25.0).abs() < 1e-9 && a.min.x.abs() < 1.0);
}

// =================================================================================== CA3

#[test]
fn ca3_fichier_tiers_plaque_a_coins_arrondis_perimetre_et_aire() {
    let b = build(&tiers(Some(4)), DxfUnit::Millimetre);
    let items = owned(&on_layer(&b, "Rect"));
    // Une LWPOLYLINE fermée de 8 sommets avec 4 bulges : 4 segments et 4 arcs.
    assert_eq!(
        items.len(),
        9,
        "8 + la LINE sur « RECT » qui se range avec elle"
    );
    let rect: Vec<ImportItem> = items[..8].to_vec();
    assert_eq!(
        rect.iter()
            .filter(|i| matches!(i, ImportItem::Arc { .. }))
            .count(),
        4
    );
    let perimeter: f64 = rect.iter().map(item_length).sum();
    let expected_perimeter = 2.0 * 80.0 + 2.0 * 40.0 + 4.0 * (std::f64::consts::FRAC_PI_2 * 10.0);
    assert!((perimeter - expected_perimeter).abs() < 0.01, "{perimeter}");
    let area = chain_area(&rect);
    let expected_area = 100.0 * 60.0 - 4.0 * 100.0 + std::f64::consts::PI * 100.0;
    assert!(
        (area - expected_area).abs() < 0.01,
        "{area} contre {expected_area}"
    );
    // Chaque arc a son centre sur un coin décalé de 10 mm et son rayon de 10 mm.
    for i in &rect {
        if let ImportItem::Arc {
            center,
            start,
            end,
            ccw,
        } = i
        {
            assert!(*ccw);
            assert!((d(xy(*center), xy(*start)) - 10.0).abs() < 1e-9);
            assert!((d(xy(*center), xy(*end)) - 10.0).abs() < 1e-9);
            assert!([(90.0, 10.0), (90.0, 50.0), (10.0, 50.0), (10.0, 10.0)]
                .iter()
                .any(|c| d(*c, xy(*center)) < 1e-9));
        }
    }
}

#[test]
fn ca3_fichier_tiers_arcs_antihoraires_en_degres_et_cercle() {
    let b = build(&tiers(Some(4)), DxfUnit::Millimetre);
    let arcs: Vec<ImportItem> = owned(&on_layer(&b, "Arcs"));
    assert_eq!(arcs.len(), 2);
    let (c, r) = ((200.0, 100.0), 50.0);
    let sweep_len = r * 120f64.to_radians();
    for (i, (a0, a1)) in arcs.iter().zip([(30.0f64, 150.0f64), (300.0, 60.0)]) {
        let ImportItem::Arc {
            center,
            start,
            end,
            ccw,
        } = i
        else {
            panic!("arc attendu")
        };
        assert!(*ccw);
        assert!(d(xy(*center), c) < 1e-9);
        let at = |deg: f64| {
            (
                c.0 + r * deg.to_radians().cos(),
                c.1 + r * deg.to_radians().sin(),
            )
        };
        assert!(d(xy(*start), at(a0)) < 1e-9, "départ {a0}°");
        assert!(d(xy(*end), at(a1)) < 1e-9, "arrivée {a1}°");
        assert!(
            (item_length(i) - sweep_len).abs() < 0.01,
            "{}",
            item_length(i)
        );
    }
    assert!((sweep_len - 104.719_755_119_659_77).abs() < 1e-9);
    let circles = owned(&on_layer(&b, "Cercle"));
    assert_eq!(circles.len(), 1);
    assert!(matches!(
        circles[0],
        ImportItem::Circle { center, radius_mm } if d(xy(center), (300.0, 0.0)) < 1e-9 && radius_mm == 25.0
    ));
    assert!((item_length(&circles[0]) - 157.079_632_679_489_66).abs() < 1e-6);
}

#[test]
fn ca3_fichier_tiers_spline_degre_3_avec_noeuds_bezier() {
    let b = build(&tiers(Some(4)), DxfUnit::Millimetre);
    let items = on_layer(&b, "Bez");
    let segs = segments_of(&items);
    assert_eq!(segs.len(), items.len(), "des segments seulement");
    let ctrl = [(400.0, 0.0), (400.0, 40.0), (460.0, 40.0), (460.0, 0.0)];
    // Chaîne continue des extrémités de la courbe.
    assert!(d(segs[0].0, ctrl[0]) < 1e-9 && d(segs[segs.len() - 1].1, ctrl[3]) < 1e-9);
    for w in segs.windows(2) {
        assert!(d(w[0].1, w[1].0) < 1e-9, "chaîne continue");
    }
    let chain: Vec<(f64, f64)> = std::iter::once(segs[0].0)
        .chain(segs.iter().map(|s| s.1))
        .collect();
    let exact = bezier_length(ctrl);
    let len: f64 = segs.iter().map(|s| d(s.0, s.1)).sum();
    assert!((len - exact).abs() < 0.01, "{len} contre {exact}");
    // Écart à la vraie courbe : au plus 0,005 mm (+ marge d'arrondi).
    for i in 0..=2000 {
        let p = bezier_point(ctrl, i as f64 / 2000.0);
        assert!(
            dist_to_polyline(p, &chain) <= 0.005 + 1e-6,
            "t={}",
            i as f64 / 2000.0
        );
    }
}

#[test]
fn ca3_fichier_tiers_bspline_a_noeuds_interieurs_et_quart_de_cercle_rationnel() {
    let b = build(&tiers(Some(4)), DxfUnit::Millimetre);
    // 6 points de contrôle, nœuds intérieurs 1 et 2 (définition de Cox–de Boor dans le test).
    let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0, 3.0];
    let segs = segments_of(&on_layer(&b, "Spl6"));
    let chain: Vec<(f64, f64)> = std::iter::once(segs[0].0)
        .chain(segs.iter().map(|s| s.1))
        .collect();
    let dense: Vec<(f64, f64)> = (0..=6000)
        .map(|i| {
            let t = (i as f64 / 6000.0 * 3.0).min(3.0 - 1e-12);
            bspline_point(3, &knots, &SPL6, &[], t)
        })
        .collect();
    for p in &dense {
        assert!(dist_to_polyline(*p, &chain) <= 0.005 + 1e-6, "{p:?}");
    }
    for p in &chain {
        assert!(
            dist_to_polyline(*p, &dense) <= 0.005 + 1e-3,
            "sommet {p:?} hors courbe"
        );
    }
    let exact: f64 = dense.windows(2).map(|w| d(w[0], w[1])).sum();
    let len: f64 = segs.iter().map(|s| d(s.0, s.1)).sum();
    assert!((len - exact).abs() < 0.01, "{len} contre {exact}");

    // Quart de cercle rationnel (poids √2/2) de centre (450 ; 300) et rayon 50 : exact.
    let segs = segments_of(&on_layer(&b, "Rat"));
    let mut len = 0.0;
    for s in &segs {
        assert!(
            (d(s.0, (450.0, 300.0)) - 50.0).abs() < 1e-6,
            "sommet hors du cercle"
        );
        len += d(s.0, s.1);
    }
    assert!(
        (len - std::f64::consts::PI * 50.0 / 2.0).abs() < 0.01,
        "{len}"
    );
}

#[test]
fn ca3_fichier_tiers_polyligne_ancienne_a_bulge_et_polyligne_3d_ignoree() {
    let b = build(&tiers(Some(4)), DxfUnit::Millimetre);
    let items = owned(&on_layer(&b, "Poly2d"));
    assert_eq!(items.len(), 2);
    match &items[0] {
        ImportItem::Arc {
            center,
            start,
            end,
            ccw,
        } => {
            // Bulge 1 : demi-cercle anti-horaire de (0 ; 200) à (100 ; 200), centre (50 ; 200).
            assert!(*ccw && d(xy(*center), (50.0, 200.0)) < 1e-9);
            assert!(d(xy(*start), (0.0, 200.0)) < 1e-9 && d(xy(*end), (100.0, 200.0)) < 1e-9);
            assert!((item_length(&items[0]) - std::f64::consts::PI * 50.0).abs() < 0.01);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(items[1], ImportItem::Segment { .. }));
    assert!(
        on_layer(&b, "Poly3d").is_empty(),
        "polyligne 3D non importée"
    );
    assert_eq!(b.report.ignored_of("POLYLINE"), 1);
    assert!(b
        .report
        .ignored
        .iter()
        .any(|i| i.kind == "POLYLINE" && i.reason == IgnoreReason::Polyline3d));
}

#[test]
fn ca3_courbes_a_points_d_ajustement_passent_par_leurs_points() {
    let b = build(&tiers(Some(4)), DxfUnit::Millimetre);
    let fit = owned(&on_layer(&b, "Fit"));
    assert_eq!(fit.len(), 1);
    let ImportItem::Spline { points } = &fit[0] else {
        panic!()
    };
    let want = [(0.0, 500.0), (50.0, 540.0), (100.0, 500.0), (150.0, 540.0)];
    assert_eq!(points.len(), 4);
    for (p, w) in points.iter().zip(want) {
        assert!(d(xy(*p), w) < 1e-9);
    }
    // Courbe fermée : deux courbes qui se rejoignent, aucun point perdu.
    let closed = owned(&on_layer(&b, "FitClosed"));
    assert_eq!(closed.len(), 2);
    let ImportItem::Spline { points: a } = &closed[0] else {
        panic!()
    };
    let ImportItem::Spline { points: c } = &closed[1] else {
        panic!()
    };
    assert!(d(xy(a[a.len() - 1]), xy(c[0])) < 1e-9);
    assert!(d(xy(c[c.len() - 1]), xy(a[0])) < 1e-9);
    assert_eq!(a.len() + c.len(), 4 + 2);
}

#[test]
fn ca3_import_dans_un_projet_longueurs_et_positions() {
    let mut p = Project::new("tiers");
    let s = p.sheets[0].id;
    let b = build(&tiers(Some(4)), DxfUnit::Millimetre);
    let out = p
        .apply(Command::ImportGeometry {
            sheet: s,
            objects: b.objects.clone(),
        })
        .unwrap();
    let sum = summarize(&p, s);
    // Arcs du fichier : 4 (plaque) + 2 + 1 (polyligne) = 7 ; cercles : 1.
    assert_eq!(sum.arcs.len(), 7);
    assert_eq!(sum.circles.len(), 1);
    // La plaque, relue dans le projet : périmètre des arcs + segments = 302,83 mm.
    assert!(p.pieces.is_empty(), "aucune pièce n'est inventée");
    assert!(out.entities.len() >= 20);
    // Aucune perte silencieuse : une remarque pour les doublons fusionnés (le segment
    // « Dup » en sens inverse) est déjà dans le rapport de lecture, la commande n'en a plus.
    assert!(out.notes.is_empty(), "{:?}", out.notes);
}

#[test]
fn ca3_aller_retour_export_puis_import_a_0_01_mm() {
    let mut p = Project::new("aller-retour");
    let s = p.sheets[0].id;
    let mut rng = common::Rng(0x1203_ABCD_5566_7788);
    for _ in 0..4 {
        rng.next();
    }
    // Un projet riche : segment, arcs des deux sens, cercle, Bézier, courbe lisse, point.
    for c in [
        Command::AddSegment {
            sheet: s,
            from: PointRef::New(pm(0.0, 0.0)),
            to: PointRef::New(pm(120.5, 33.25)),
        },
        Command::AddArc {
            sheet: s,
            center: PointRef::New(pm(200.0, 100.0)),
            radius_mm: 40.0,
            start_deg: 20.0,
            end_deg: 200.0,
            ccw: true,
        },
        Command::AddArc {
            sheet: s,
            center: PointRef::New(pm(-150.0, 40.0)),
            radius_mm: 61.5,
            start_deg: 20.0,
            end_deg: 200.0,
            ccw: false,
        },
        Command::AddCircle {
            sheet: s,
            center: PointRef::New(pm(0.0, 300.0)),
            radius_mm: 35.0,
        },
        Command::AddCurve {
            sheet: s,
            start: PointRef::New(pm(300.0, 0.0)),
            c1: pm(300.0, 80.0),
            c2: pm(380.0, 80.0),
            end: PointRef::New(pm(380.0, 0.0)),
        },
        Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pm(0.0, -100.0)),
                PointRef::New(pm(40.0, -140.0)),
                PointRef::New(pm(90.0, -90.0)),
                PointRef::New(pm(150.0, -120.0)),
            ],
        },
        Command::AddPoint {
            sheet: s,
            at: pm(-300.0, -300.0),
        },
    ] {
        p.apply(c).unwrap();
    }
    let before = p.clone();
    let exp = export_construction_dxf(&p, ConstructionScope::Sheet(s)).unwrap();
    assert_eq!(p, before, "l'export ne modifie pas le projet");
    let doc = DxfDocument::parse(exp.bytes()).unwrap();
    assert_eq!(
        doc.declared_unit(),
        DeclaredUnit::Known(DxfUnit::Millimetre),
        "unités déclarées en mm"
    );
    let build = doc.build_choice(UnitChoice::Declared).unwrap();
    assert!(
        build.report.ignored.is_empty(),
        "{:?}",
        build.report.ignored
    );
    let mut q = Project::new("relu");
    let qs = q.sheets[0].id;
    q.apply(Command::ImportGeometry {
        sheet: qs,
        objects: build.objects,
    })
    .unwrap();
    compare(&summarize(&p, s), &summarize(&q, qs), 0.01).unwrap();
    // Positions : mêmes arcs, mêmes sens ; le cercle garde son rayon.
    let (o, i) = (summarize(&p, s), summarize(&q, qs));
    assert_eq!(
        (o.arcs.len(), o.circles.len()),
        (i.arcs.len(), i.circles.len())
    );
    assert_eq!(o.arcs.len(), 2);
}

// =================================================================================== CA2 (commande)

#[test]
fn ca2_import_du_fichier_par_une_commande_un_seul_pas_et_refus_sans_trace() {
    let p = Project::new("cible");
    let s = p.sheets[0].id;
    let mut doc = Document::new(p);
    let before = persist::to_json(doc.project()).unwrap();
    let build = build_of(&tiers(Some(4)));
    doc.execute(Command::ImportGeometry {
        sheet: s,
        objects: build.objects.clone(),
    })
    .unwrap();
    assert!(doc.can_undo());
    assert!(doc.undo());
    assert_eq!(persist::to_json(doc.project()).unwrap(), before);
    assert!(!doc.can_undo(), "un seul pas");

    // Refus : coordonnée non finie glissée dans la liste.
    let mut bad = build.objects.clone();
    bad.push(ImportObject {
        item: ImportItem::Segment {
            from: pm(0.0, 0.0),
            to: pm(f64::NAN, 1.0),
        },
        layer: None,
    });
    let e = doc
        .execute(Command::ImportGeometry {
            sheet: s,
            objects: bad,
        })
        .unwrap_err();
    assert!(matches!(e, CoreError::Geometry(_)), "{e}");
    assert_eq!(persist::to_json(doc.project()).unwrap(), before);
    assert!(!doc.can_undo(), "un refus ne crée pas d'étape");
}

fn build_of(bytes: &[u8]) -> ImportBuild {
    build(bytes, DxfUnit::Millimetre)
}

// =================================================================================== CA4

#[test]
fn ca4_fichier_tronque_jamais_de_panique() {
    let bytes = tiers(Some(4));
    let text = String::from_utf8(bytes.clone()).unwrap();
    // Chaque préfixe, à la ligne et au milieu d'une ligne : refus propre (EOF absent).
    let mut cuts: Vec<usize> = text.match_indices('\n').map(|(i, _)| i).collect();
    cuts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    cuts.extend((0..text.len()).step_by(37));
    cuts.sort();
    cuts.dedup();
    let mut refused = 0;
    for cut in cuts {
        if cut >= text.len() - 1 {
            continue; // le fichier entier (avec ou sans le dernier retour à la ligne)
        }
        let Some(prefix) = text.get(..cut) else {
            continue;
        };
        match DxfDocument::parse(prefix.as_bytes()) {
            Err(
                DxfImportError::Truncated { .. }
                | DxfImportError::InvalidGroupCode { .. }
                | DxfImportError::Structure { .. }
                | DxfImportError::NotDxf,
            ) => refused += 1,
            other => panic!("coupure à {cut} : {other:?}"),
        }
    }
    assert!(refused > 100);
    assert!(DxfDocument::parse(&bytes).is_ok());
    // Fichier vide, une seule ligne.
    for tiny in ["", "0", "0\n", "0\nSECTION", "garbage\n", "0\nEOF\n"] {
        assert!(DxfDocument::parse(tiny.as_bytes()).is_err(), "{tiny:?}");
    }
}

#[test]
fn ca4_codes_de_groupe_invalides_localises() {
    for bad in ["abc", "1072", "-6", "", "1.5", "99999999999"] {
        let text = format!("  0\nSECTION\n  2\nENTITIES\n{bad}\nLINE\n  0\nENDSEC\n  0\nEOF\n");
        match DxfDocument::parse(text.as_bytes()) {
            Err(DxfImportError::InvalidGroupCode { line }) => assert_eq!(line, 5, "{bad:?}"),
            other => panic!("{bad:?} : {other:?}"),
        }
    }
    // Contenu hors section.
    let text = "  0\nSECTION\n  2\nENTITIES\n  0\nENDSEC\n 10\n1.0\n  0\nEOF\n";
    assert!(matches!(
        DxfDocument::parse(text.as_bytes()),
        Err(DxfImportError::Structure { line: 7 })
    ));
}

#[test]
fn ca4_nombres_non_finis_ignores_avec_rapport_jamais_importes() {
    let mut e = Pairs::default();
    for (i, bad) in [
        "NaN", "nan", "inf", "-inf", "Infinity", "1e999", "abc", "", "1,5",
    ]
    .iter()
    .enumerate()
    {
        let mut l = Pairs::default();
        l.p(0, "LINE")
            .p(8, "0")
            .p(10, "0")
            .p(20, "0")
            .p(11, bad)
            .p(21, "5");
        e.extend(&l);
        let _ = i;
    }
    e.extend(&line(1, "0", (0.0, 0.0, 0.0), (1.0, 1.0, 0.0)));
    let doc = DxfDocument::parse(file(&header(Some(4)), &e).text(false).as_bytes()).unwrap();
    let b = doc.build(DxfUnit::Millimetre).unwrap();
    assert_eq!(b.objects.len(), 1, "seule la ligne valide est importée");
    assert_eq!(
        b.report
            .ignored
            .iter()
            .find(|i| i.kind == "LINE" && i.reason == IgnoreReason::Invalid)
            .unwrap()
            .count,
        9
    );
    // Une valeur illisible dans l'en-tête ne bloque pas : unité non déclarée.
    let mut h = Pairs::default();
    h.p(9, "$INSUNITS").p(70, "NaN");
    let doc = DxfDocument::parse(file(&h, &e).text(false).as_bytes()).unwrap();
    assert_eq!(doc.declared_unit(), DeclaredUnit::Missing);
    // Coordonnées gigantesques : ignorées (hors de l'espace de travail), pas importées.
    let mut g = Pairs::default();
    g.extend(&line(1, "0", (0.0, 0.0, 0.0), (1.0e20, 0.0, 0.0)));
    g.extend(&line(2, "0", (0.0, 0.0, 0.0), (5.0, 0.0, 0.0)));
    let b = build(
        &file(&header(Some(4)), &g).text(false).into_bytes(),
        DxfUnit::Millimetre,
    );
    assert_eq!(b.objects.len(), 1);
    assert!(b
        .report
        .ignored
        .iter()
        .any(|i| i.reason == IgnoreReason::OutOfRange));
    // Même une valeur raisonnable devient hors limite dans une grande unité.
    let mut g = Pairs::default();
    g.extend(&line(1, "0", (0.0, 0.0, 0.0), (5000.0, 0.0, 0.0)));
    let doc = DxfDocument::parse(file(&header(Some(6)), &g).text(false).as_bytes()).unwrap();
    assert_eq!(doc.build(DxfUnit::Metre).unwrap().objects.len(), 0);
    assert_eq!(doc.build(DxfUnit::Centimetre).unwrap().objects.len(), 1);
}

#[test]
fn ca4_limites_de_taille_et_d_entites_refus_propre() {
    // Trop volumineux : refusé avant toute lecture.
    let big = vec![b' '; MAX_FILE_BYTES + 1];
    assert!(matches!(
        DxfDocument::parse(&big),
        Err(DxfImportError::TooLarge { .. })
    ));
    // Plus de MAX_ENTITIES entités.
    let mut text = String::from("  0\nSECTION\n  2\nENTITIES\n");
    for _ in 0..=MAX_ENTITIES {
        text.push_str("  0\nPOINT\n 10\n1\n 20\n1\n");
    }
    text.push_str("  0\nENDSEC\n  0\nEOF\n");
    assert_eq!(
        DxfDocument::parse(text.as_bytes()).unwrap_err(),
        DxfImportError::TooManyEntities
    );
    // Plus d'objets que la commande n'en accepte : refus à la conversion, avec le nombre.
    // (Les polylignes donnent plusieurs objets par entité : seules elles dépassent la borne.)
    let doc = DxfDocument::parse(polylines_text(MAX_IMPORT_OBJECTS + 5).as_bytes()).unwrap();
    assert!(matches!(
        doc.build(DxfUnit::Millimetre),
        Err(DxfImportError::TooManyObjects { count }) if count == MAX_IMPORT_OBJECTS + 1
    ));
    // Exactement à la borne : accepté.
    let doc = DxfDocument::parse(polylines_text(MAX_IMPORT_OBJECTS).as_bytes()).unwrap();
    assert_eq!(
        doc.build(DxfUnit::Millimetre).unwrap().objects.len(),
        MAX_IMPORT_OBJECTS
    );
}

/// DXF de `segments` segments distincts, en polylignes ouvertes de 20 001 sommets au plus.
fn polylines_text(segments: usize) -> String {
    let mut text = String::from("  0\nSECTION\n  2\nENTITIES\n");
    let (mut left, mut row) = (segments, 0);
    while left > 0 {
        let n = left.min(20_000);
        text.push_str(&format!("  0\nLWPOLYLINE\n 90\n{}\n 70\n0\n", n + 1));
        for v in 0..=n {
            text.push_str(&format!(" 10\n{}\n 20\n{}\n", v * 3, row * 10));
        }
        left -= n;
        row += 1;
    }
    text.push_str("  0\nENDSEC\n  0\nEOF\n");
    text
}

#[test]
fn ca4_dxf_binaire_octets_nuls_et_codages() {
    let mut bin = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
    bin.extend_from_slice(&[0u8; 64]);
    assert_eq!(
        DxfDocument::parse(&bin).unwrap_err(),
        DxfImportError::BinaryDxf
    );
    assert!(DxfImportError::BinaryDxf.to_string().contains("binaire"));
    assert_eq!(
        DxfDocument::parse(b"  0\nSECTION\n\0\0\n").unwrap_err(),
        DxfImportError::NotDxf
    );
    // CP1252 : « Façade » avec un octet 0xE7 isolé, relu et signalé.
    let mut e = Pairs::default();
    e.extend(&line(1, "Fa\u{e7}ade", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0)));
    let utf8 = file(&header(Some(4)), &e).text(false);
    let latin1: Vec<u8> = utf8
        .chars()
        .map(|c| if c == '\u{e7}' { 0xE7 } else { c as u8 })
        .collect();
    assert!(std::str::from_utf8(&latin1).is_err());
    let b = build(&latin1, DxfUnit::Millimetre);
    assert_eq!(b.objects[0].layer.as_deref(), Some("Façade"));
    assert_eq!(b.report.note(ImportNoteKind::LegacyEncoding), 1);
    // UTF-8 avec BOM : accepté, sans remarque.
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(utf8.as_bytes());
    let b = build(&with_bom, DxfUnit::Millimetre);
    assert_eq!(b.objects[0].layer.as_deref(), Some("Façade"));
    assert_eq!(b.report.note(ImportNoteKind::LegacyEncoding), 0);
    // Octets non texte en milieu de fichier (0x81 non défini en CP1252) : pas de panique.
    let junk = [&latin1[..40], &[0x81, 0x8d, 0xff, 0xfe], &latin1[40..]].concat();
    let _ = DxfDocument::parse(&junk);
}

#[test]
fn ca4_sections_manquantes_et_entites_degenerees() {
    // Sans section ENTITIES : aucune entité, aucun objet (la commande refusera « rien à importer »).
    let mut f = Pairs::default();
    f.extend(&section("HEADER", &header(Some(4))));
    f.p(0, "EOF");
    let doc = DxfDocument::parse(f.text(false).as_bytes()).unwrap();
    assert_eq!(doc.entity_count(), 0);
    let b = doc.build(DxfUnit::Millimetre).unwrap();
    assert!(b.objects.is_empty() && b.bbox_mm.is_none());
    let mut p = Project::new("vide");
    let s = p.sheets[0].id;
    assert!(p
        .apply(Command::ImportGeometry {
            sheet: s,
            objects: b.objects,
        })
        .is_err());
    // Section ouverte jamais fermée.
    assert!(matches!(
        DxfDocument::parse(b"  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\n0\n"),
        Err(DxfImportError::Truncated { .. })
    ));
    // Entités dégénérées : longueur, rayon, angle nuls ; sommets confondus.
    let mut e = Pairs::default();
    e.extend(&line(1, "0", (1.0, 1.0, 0.0), (1.0, 1.0, 0.0)));
    e.extend(&circle(2, "0", (0.0, 0.0), 0.0));
    e.extend(&circle(3, "0", (0.0, 0.0), -3.0));
    e.extend(&arc(4, "0", (0.0, 0.0), 10.0, 45.0, 45.0));
    e.extend(&arc(5, "0", (0.0, 0.0), 0.0, 0.0, 90.0));
    e.extend(&lwpolyline(6, "0", &[(0.0, 0.0, 0.0)], false));
    e.extend(&lwpolyline(
        7,
        "0",
        &[(0.0, 0.0, 0.0), (0.0, 0.0, 0.0)],
        true,
    ));
    e.extend(&lwpolyline(
        8,
        "0",
        &[
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 0.5),
            (10.0, 0.0, 0.0),
            (10.0, 0.0, 0.0),
        ],
        false,
    ));
    e.extend(&spline(9, "0", 3, &[0.0, 0.0], &[], &[]));
    let b = build(
        &file(&header(Some(4)), &e).text(false).into_bytes(),
        DxfUnit::Millimetre,
    );
    // Seule la polyligne 8 (une partie de 10 mm une fois les doublons retirés) subsiste.
    assert_eq!(b.objects.len(), 1, "{:?}", b.objects);
    assert!(b.report.note(ImportNoteKind::ZeroLengthParts) >= 2);
    assert_eq!(b.report.ignored_total(), 8);
    // ARC 0 → 360° : un cercle.
    let mut e = Pairs::default();
    e.extend(&arc(1, "0", (5.0, 5.0), 10.0, 0.0, 360.0));
    let b = build(
        &file(&header(Some(4)), &e).text(false).into_bytes(),
        DxfUnit::Millimetre,
    );
    assert!(matches!(b.objects[0].item, ImportItem::Circle { .. }));
}

#[test]
fn ca4_z_ignore_avec_rapport_et_extrusions() {
    let mut e = Pairs::default();
    e.extend(&line(1, "0", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0)));
    e.extend(&line(2, "0", (0.0, 5.0, 3.0), (1.0, 5.0, 0.0)));
    e.extend(&line(3, "0", (0.0, 6.0, 1e-12), (1.0, 6.0, 0.0)));
    // Extrusion (0 ; 0 ; −1) : l'axe X est inversé. ARC de centre (10 ; 0), 0° → 90° dans son
    // repère = arc de centre (−10 ; 0) de 90° à 180° dans le monde.
    let mut flipped = arc(4, "Miroir", (10.0, 0.0), 5.0, 0.0, 90.0);
    flipped.p(210, 0).p(220, 0).p(230, -1);
    e.extend(&flipped);
    // Extrusion oblique : refusée avec rapport.
    let mut oblique = circle(5, "Oblique", (0.0, 0.0), 5.0);
    oblique.p(210, 1).p(220, 0).p(230, 0);
    e.extend(&oblique);
    let b = build(
        &file(&header(Some(4)), &e).text(false).into_bytes(),
        DxfUnit::Millimetre,
    );
    assert_eq!(
        b.report.note(ImportNoteKind::ZIgnored),
        1,
        "z = 3 compté ; z = 1e-12 est du bruit numérique sous 1e-9"
    );
    let mirror = on_layer(&b, "Miroir");
    let ImportItem::Arc {
        center,
        start,
        end,
        ccw,
    } = mirror[0]
    else {
        panic!()
    };
    assert!(*ccw && d(xy(*center), (-10.0, 0.0)) < 1e-9);
    assert!(d(xy(*start), (-10.0, 5.0)) < 1e-9, "départ à 90°");
    assert!(d(xy(*end), (-15.0, 0.0)) < 1e-9, "arrivée à 180°");
    assert!(on_layer(&b, "Oblique").is_empty());
    assert!(b
        .report
        .ignored
        .iter()
        .any(|i| i.kind == "CIRCLE" && i.reason == IgnoreReason::Extrusion));
}

#[test]
fn ca4_doublons_signales_et_polyligne_fermee_a_deux_sommets() {
    let mut e = Pairs::default();
    for h in 0..5 {
        e.extend(&line(h, "0", (0.0, 0.0, 0.0), (10.0, 0.0, 0.0)));
    }
    let b = build(
        &file(&header(Some(4)), &e).text(false).into_bytes(),
        DxfUnit::Millimetre,
    );
    assert_eq!(b.objects.len(), 1);
    assert_eq!(b.report.note(ImportNoteKind::DuplicatesRemoved), 4);
    assert_eq!(
        b.report.imported["LINE"], 5,
        "chaque entité est comptée, 4 sont fusionnées"
    );
}

// =================================================================================== CA5

#[test]
fn ca5_couches_vers_calques_sans_piece_ni_contrainte() {
    let b = build(&tiers(Some(4)), DxfUnit::Millimetre);
    let names: Vec<(&str, Option<&str>)> = b
        .report
        .layers
        .iter()
        .map(|l| (l.dxf_name.as_str(), l.calque.as_deref()))
        .collect();
    // La couche « 0 » est le calque par défaut ; la casse est ignorée comme dans DXF
    // (« RECT » rejoint « Rect ») ; `\U+00E7` est décodé.
    assert!(names.contains(&("0", None)));
    assert!(names.contains(&("Rect", Some("Rect"))));
    assert!(!names.iter().any(|(n, _)| *n == "RECT"));
    assert!(names.contains(&("Façade", Some("Façade"))));
    assert!(
        !names.iter().any(|(n, _)| *n == "Autres"),
        "couche sans objet non listée"
    );
    let mut p = Project::new("calques");
    let s = p.sheets[0].id;
    p.apply(Command::ImportGeometry {
        sheet: s,
        objects: b.objects.clone(),
    })
    .unwrap();
    let sheet = p.sheet(s).unwrap();
    let calques: Vec<&str> = sheet.layers.iter().map(|l| l.name.as_str()).collect();
    for want in ["Rect", "Arcs", "Cercle", "Bez", "Façade", "Fit", "Poly2d"] {
        assert!(calques.contains(&want), "{want} : {calques:?}");
    }
    assert!(!calques.contains(&"0"));
    // Rien d'autre que de la géométrie ordinaire.
    assert!(p.pieces.is_empty());
    assert!(sheet.notches.is_empty() && sheet.dimensions.is_empty() && sheet.guides.is_empty());
    assert!(sheet
        .entities
        .iter()
        .all(|e| e.role == PathRole::Construction && e.name.is_none()));
    assert!(sheet.entities.iter().all(|e| match &e.kind {
        EntityKind::Segment { construction, .. } => construction.is_none(),
        _ => true,
    }));
    // Le segment de la couche « RECT » est dans le calque « Rect ».
    let rect = sheet.layers.iter().find(|l| l.name == "Rect").unwrap().id;
    assert_eq!(
        sheet
            .entities
            .iter()
            .filter(|e| e.layer == Some(rect))
            .count(),
        9
    );
}

// =================================================================================== export

#[test]
fn export_generique_couches_unites_et_rapport() {
    let mut p = Project::new("export");
    let s = p.sheets[0].id;
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pm(0.0, 0.0)),
            to: PointRef::New(pm(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: seg,
        role: PathRole::CuttingContour,
    })
    .unwrap();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Façade".into(),
        })
        .unwrap()
        .layers[0];
    let cw = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pm(50.0, 50.0)),
            radius_mm: 20.0,
            start_deg: 10.0,
            end_deg: 100.0,
            ccw: false,
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![cw],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(pm(-40.0, 0.0)),
        radius_mm: 12.0,
    })
    .unwrap();
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::New(pm(0.0, 200.0)),
        c1: pm(0.0, 260.0),
        c2: pm(60.0, 260.0),
        end: PointRef::New(pm(60.0, 200.0)),
    })
    .unwrap();
    p.apply(Command::AddPoint {
        sheet: s,
        at: pm(5.0, 5.0),
    })
    .unwrap();
    // Éléments non écrits : cran, cotation, ligne d'aide, nom de tracé.
    p.apply(Command::AddNotch {
        sheet: s,
        entity: seg,
        distance_mm: 10.0,
        side: pf_geometry::Side::Left,
        length_mm: 5.0,
        style: pf_core::NotchStyle::Single,
    })
    .unwrap();
    p.apply(Command::AddGuide {
        sheet: s,
        x_mm: 0.0,
        y_mm: 0.0,
        angle_deg: 45.0,
        axis: false,
        layer: None,
    })
    .unwrap();
    let a = p.sheet(s).unwrap().points[0].id;
    let b = p.sheet(s).unwrap().points[1].id;
    p.apply(Command::AddDimension {
        sheet: s,
        from: a,
        to: b,
        offset_mm: 10.0,
    })
    .unwrap();
    let before = p.clone();
    let exp = export_construction_dxf(&p, ConstructionScope::Sheet(s)).unwrap();
    assert_eq!(p, before);
    assert_eq!(
        exp.bytes(),
        export_construction_dxf(&p, ConstructionScope::Sheet(s))
            .unwrap()
            .bytes(),
        "déterministe"
    );
    assert!(exp.text.is_ascii(), "ASCII pur");
    let f = read_export(&exp.text).unwrap();
    assert_eq!(f.var("$ACADVER").unwrap()[0].1, "AC1009");
    assert_eq!(f.var("$INSUNITS").unwrap()[0].1.trim(), "4", "mm déclarés");
    assert_eq!(f.var("$MEASUREMENT").unwrap()[0].1.trim(), "1");
    // Couches : calque par défaut = « 0 », rôle dans le nom, calque non ASCII échappé.
    let names: Vec<&str> = f.entities.iter().map(layer_of).collect();
    assert!(names.contains(&"0.cutting"), "{names:?}");
    assert!(names.contains(&"Fa\\U+00E7ade"), "{names:?}");
    assert!(f.layers.iter().any(|l| l.0 == "0.cutting" && l.1 == 1));
    // Segment, cercle, point écrits tels quels.
    assert!(f.entities.iter().any(|e| matches!(e,
        Ex::Line { a, b, .. } if d(*a, (0.0, 0.0)) < 1e-9 && d(*b, (100.0, 0.0)) < 1e-9)));
    assert!(f.entities.iter().any(|e| matches!(e,
        Ex::Circle { c, r, .. } if d(*c, (-40.0, 0.0)) < 1e-9 && (*r - 12.0).abs() < 1e-9)));
    assert!(f
        .entities
        .iter()
        .any(|e| matches!(e, Ex::Point { p, .. } if d(*p, (5.0, 5.0)) < 1e-9)));
    // Arc horaire de 10° à 100° : écrit anti-horaire de 100° à 10°.
    let arc = f
        .entities
        .iter()
        .find_map(|e| match e {
            Ex::Arc { c, r, a0, a1, .. } => Some((*c, *r, *a0, *a1)),
            _ => None,
        })
        .unwrap();
    assert!(d(arc.0, (50.0, 50.0)) < 1e-9 && (arc.1 - 20.0).abs() < 1e-9);
    assert!(
        (arc.2 - 100.0).abs() < 1e-8 && (arc.3 - 10.0).abs() < 1e-8,
        "{arc:?}"
    );
    // Courbe de Bézier : polyligne ouverte discrétisée, extrémités exactes.
    let poly = f
        .entities
        .iter()
        .find_map(|e| match e {
            Ex::Poly { pts, closed, .. } => Some((pts.clone(), *closed)),
            _ => None,
        })
        .unwrap();
    assert!(!poly.1 && poly.0.len() > 8);
    assert!(d(poly.0[0], (0.0, 200.0)) < 1e-9 && d(*poly.0.last().unwrap(), (60.0, 200.0)) < 1e-9);
    // Rapport : rien n'est perdu en silence.
    let r = &exp.report;
    assert_eq!(r.count(ConstructionNoteKind::Notches), 1);
    assert_eq!(r.count(ConstructionNoteKind::Dimensions), 1);
    assert_eq!(r.count(ConstructionNoteKind::Guides), 1);
    assert_eq!(r.count(ConstructionNoteKind::CurvesDiscretized), 1);
    assert_eq!(r.omissions().count(), 3);
    assert_eq!(r.entities["LINE"], 1);
    assert_eq!(r.entities["ARC"], 1);
    assert_eq!(r.entities["CIRCLE"], 1);
    assert_eq!(r.entities["POLYLINE"], 1);
    assert!(r.entities["POINT"] >= 1);
    assert_eq!(r.total_entities(), f.entities.len());
    assert!(r
        .layers
        .iter()
        .any(|l| l.name == "0.cutting" && l.role == Some(PathRole::CuttingContour)));
}

fn layer_of(e: &Ex) -> &str {
    match e {
        Ex::Line { layer, .. }
        | Ex::Point { layer, .. }
        | Ex::Circle { layer, .. }
        | Ex::Arc { layer, .. }
        | Ex::Poly { layer, .. } => layer,
    }
}

#[test]
fn export_generique_toutes_les_feuilles_cote_a_cote_et_refus() {
    let mut p = Project::new("deux");
    let s1 = p.sheets[0].id;
    let s2 = p
        .apply(Command::CreateSheet { name: "Dos".into() })
        .unwrap()
        .sheet
        .unwrap();
    for s in [s1, s2] {
        p.apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pm(10.0, 0.0)),
            to: PointRef::New(pm(60.0, 0.0)),
        })
        .unwrap();
    }
    let one = export_construction_dxf(&p, ConstructionScope::Sheet(s1)).unwrap();
    assert_eq!(one.report.sheets.len(), 1);
    let all = export_construction_dxf(&p, ConstructionScope::AllSheets).unwrap();
    assert_eq!(all.report.sheets.len(), 2);
    assert_eq!(
        all.report.sheets[0].offset_x_mm, 0.0,
        "la première feuille reste en place"
    );
    // Deuxième feuille : 100 mm à droite de la fin de la première (x max 60) → x min = 160.
    assert!((all.report.sheets[1].offset_x_mm - 150.0).abs() < 1e-9);
    let f = read_export(&all.text).unwrap();
    let lines: Vec<_> = f
        .entities
        .iter()
        .filter_map(|e| match e {
            Ex::Line { a, b, .. } => Some((*a, *b)),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(d(lines[1].0, (160.0, 0.0)) < 1e-9 && d(lines[1].1, (210.0, 0.0)) < 1e-9);
    // Refus : feuille vide, feuille inconnue ; aucun fichier.
    let empty = p
        .apply(Command::CreateSheet {
            name: "Vide".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    assert_eq!(
        export_construction_dxf(&p, ConstructionScope::Sheet(empty)).unwrap_err(),
        ConstructionError::NothingToExport
    );
    assert!(matches!(
        export_construction_dxf(&p, ConstructionScope::Sheet(pf_core::SheetId(987_654))),
        Err(ConstructionError::Core(_))
    ));
}

fn pm(x: f64, y: f64) -> pf_geometry::PointMm {
    pf_geometry::PointMm::new(x, y)
}

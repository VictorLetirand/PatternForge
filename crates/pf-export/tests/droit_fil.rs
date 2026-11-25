//! US-06.5 — droit-fil avancé à l'export : la flèche **calculée** (parallèle, perpendiculaire,
//! biais, inversion) remplace le tracé dessiné ; le sens du tissu se lit sans la couleur
//! (poil = une seule pointe fermée, motif = pointes doublées, aucun = comme avant) ; les
//! lignes supplémentaires sont exportées ; sans nouveau réglage, rien ne change à l'octet.

#[path = "common/pdf_text.rs"]
mod pdf_text;

use pf_core::{
    Command, GrainDirection, GrainSense, PathRole, PieceGrain, PieceId, PieceSource, PointRef,
    Project, SheetId,
};
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{grain_arrow_marks, PathGeom, Scene, ARROW_LEN_MM, GRAIN_DOUBLE_HEAD_MM, MM_TO_PT};
use pf_geometry::{distance_mm, PointMm};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn seg(
    p: &mut Project,
    s: SheetId,
    a: (f64, f64),
    b: (f64, f64),
    role: PathRole,
) -> pf_core::EntityId {
    let e = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(a.0, a.1)),
            to: PointRef::New(pt(b.0, b.1)),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: e,
        role,
    })
    .unwrap();
    e
}

/// Pièce 100 × 60 ; droit-fil vertical (50 ; 10) → (50 ; 50) ; deuxième ligne
/// (10 ; 45) → (90 ; 45) ; tracé de référence horizontal (0 ; 100) → (40 ; 100).
struct Sc {
    p: Project,
    s: SheetId,
    piece: PieceId,
    extra: pf_core::EntityId,
    href: pf_core::EntityId,
}

fn project() -> Sc {
    let mut p = Project::new("Droit-fil export");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 60.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let main = seg(&mut p, s, (50.0, 10.0), (50.0, 50.0), PathRole::Grainline);
    let outline = p.piece(piece).unwrap().outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(main),
    })
    .unwrap();
    let extra = seg(&mut p, s, (10.0, 45.0), (90.0, 45.0), PathRole::Grainline);
    let href = seg(
        &mut p,
        s,
        (0.0, 100.0),
        (40.0, 100.0),
        PathRole::Construction,
    );
    Sc {
        p,
        s,
        piece,
        extra,
        href,
    }
}

fn with_grain(sc: &mut Sc, g: PieceGrain) {
    sc.p.apply(Command::SetPieceGrain {
        piece: sc.piece,
        grain: g,
    })
    .unwrap();
}

fn grain_paths(scene: &Scene) -> Vec<(PointMm, PointMm, GrainSense)> {
    scene
        .paths
        .iter()
        .filter(|p| p.role == PathRole::Grainline)
        .map(|p| match p.geom {
            PathGeom::Line(a, b) => (a, b, p.sense),
            _ => panic!("un droit-fil réglé est un segment"),
        })
        .collect()
}

/// Nombre de traits `<line … data-role="droit-fil"` du SVG.
fn svg_grain_lines(svg: &str) -> usize {
    svg.lines()
        .filter(|l| l.contains("<line") && l.contains("data-role=\"droit-fil\""))
        .count()
}

fn page_streams(bytes: &[u8]) -> Vec<String> {
    let text = pdf_text::readable(bytes);
    let mut pages = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        pages.push(body[..end].to_string());
        rest = &body[end + 9..];
    }
    pages
}

/// Segments (mm) « x y m x y l » d'un flux de page, texte et tirets ignorés.
fn pdf_segments(stream: &str) -> Vec<((f64, f64), (f64, f64))> {
    let mm = |v: f64| v / MM_TO_PT;
    let mut cleaned = String::new();
    let mut chars = stream.chars();
    while let Some(c) = chars.next() {
        match c {
            '(' => {
                while let Some(d) = chars.next() {
                    match d {
                        '\\' => {
                            chars.next();
                        }
                        ')' => break,
                        _ => {}
                    }
                }
            }
            '[' => {
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                }
            }
            _ => cleaned.push(c),
        }
    }
    let (mut out, mut start): (Vec<_>, Option<(f64, f64)>) = (Vec::new(), None);
    let mut stack: Vec<f64> = Vec::new();
    for tok in cleaned.split_whitespace() {
        if let Ok(v) = tok.parse::<f64>() {
            stack.push(v);
            continue;
        }
        let n = stack.len();
        match tok {
            "m" => start = Some((mm(stack[n - 2]), mm(stack[n - 1]))),
            "l" => {
                if let Some(s) = start {
                    out.push((s, (mm(stack[n - 2]), mm(stack[n - 1]))));
                }
            }
            _ => {}
        }
        stack.clear();
    }
    out
}

fn len(s: &((f64, f64), (f64, f64))) -> f64 {
    (s.1 .0 - s.0 .0).hypot(s.1 .1 - s.0 .1)
}

#[test]
fn sans_reglage_ou_avec_seulement_une_tolerance_l_export_est_identique_a_l_octet() {
    let plain = project();
    let mut tol = project();
    with_grain(
        &mut tol,
        PieceGrain {
            rotation_tolerance_deg: Some(15.0),
            ..PieceGrain::default()
        },
    );
    let (a, b) = (
        Scene::from_sheet(&plain.p, plain.s).unwrap(),
        Scene::from_sheet(&tol.p, tol.s).unwrap(),
    );
    // La tolérance n'est lue que par l'assemblage de pages (US-13.2) : seul `grain_limits`
    // la porte, tout ce qui se dessine est identique.
    assert_eq!(a.grain_limits[0].tolerance_deg, 0.0);
    assert_eq!(b.grain_limits[0].tolerance_deg, 15.0);
    let b_same = pf_export::Scene {
        grain_limits: a.grain_limits.clone(),
        ..b.clone()
    };
    assert_eq!(a, b_same);
    assert_eq!(export_svg(&a).unwrap(), export_svg(&b).unwrap());
    assert_eq!(
        export_pdf(std::slice::from_ref(&a)).unwrap().bytes,
        export_pdf(std::slice::from_ref(&b)).unwrap().bytes
    );
    // Aucun marquage de sens, aucune légende ajoutée.
    let svg = export_svg(&a).unwrap();
    assert!(!svg.contains("data-grain-sense"));
    let pdf = export_pdf(std::slice::from_ref(&a)).unwrap();
    assert!(!pdf_text::readable(&pdf.bytes).contains("Sens du droit-fil"));
    // Le droit-fil historique : tracé + deux pointes ouvertes (4 traits) = 5 traits, pour
    // chacune des deux lignes de rôle droit-fil de la feuille.
    assert_eq!(svg_grain_lines(&svg), 10);
}

#[test]
fn la_fleche_calculee_remplace_le_trace_dessine() {
    let mut sc = project();
    // Parallèle à l'horizontale : même longueur (40) et même milieu (50 ; 30).
    let g = PieceGrain {
        direction: GrainDirection::Parallel { reference: sc.href },
        ..PieceGrain::default()
    };
    with_grain(&mut sc, g);
    let scene = Scene::from_sheet(&sc.p, sc.s).unwrap();
    let g = grain_paths(&scene);
    assert_eq!(g.len(), 2, "le tracé dessiné est remplacé, pas doublé");
    let main = g.iter().find(|x| (x.0.x - 30.0).abs() < 1e-9).unwrap();
    assert!(distance_mm(main.0, pt(30.0, 30.0)) < 1e-9);
    assert!(distance_mm(main.1, pt(70.0, 30.0)) < 1e-9);
    // L'autre ligne (non réglée) est exportée comme avant.
    assert!(g.iter().any(
        |x| distance_mm(x.0, pt(10.0, 45.0)) < 1e-9 && distance_mm(x.1, pt(90.0, 45.0)) < 1e-9
    ));
    // Le tracé de référence reste un tracé de construction, inchangé.
    assert!(scene.paths.iter().any(|p| p.role == PathRole::Construction
        && matches!(p.geom, PathGeom::Line(a, b) if distance_mm(a, pt(0.0, 100.0)) < 1e-9
                && distance_mm(b, pt(40.0, 100.0)) < 1e-9)));
    // Les deux lignes (calculée et non réglée) ont chacune leurs 4 pointes de flèche.
    assert_eq!(svg_grain_lines(&export_svg(&scene).unwrap()), 10);
}

#[test]
fn lignes_supplementaires_exportees_avec_leur_direction_et_leur_sens() {
    let mut sc = project();
    let g = PieceGrain {
        sense: GrainSense::Nap,
        extra: vec![pf_core::ExtraGrainLine {
            entity: sc.extra,
            direction: GrainDirection::Bias { angle_deg: -45.0 },
            sense: GrainSense::Pattern,
            reversed: true,
        }],
        ..PieceGrain::default()
    };
    with_grain(&mut sc, g);
    let scene = Scene::from_sheet(&sc.p, sc.s).unwrap();
    let g = grain_paths(&scene);
    assert_eq!(g.len(), 2);
    let main = g.iter().find(|x| x.2 == GrainSense::Nap).unwrap();
    assert!(
        distance_mm(main.0, pt(50.0, 10.0)) < 1e-9 && distance_mm(main.1, pt(50.0, 50.0)) < 1e-9
    );
    // Ligne « motif » : biais −45°, inversée, longueur 80, milieu (50 ; 45).
    let h = 40.0 * std::f64::consts::FRAC_1_SQRT_2;
    let extra = g.iter().find(|x| x.2 == GrainSense::Pattern).unwrap();
    assert!(distance_mm(extra.0, pt(50.0 + h, 45.0 - h)) < 1e-9);
    assert!(distance_mm(extra.1, pt(50.0 - h, 45.0 + h)) < 1e-9);
    // SVG : l'attribut du sens, hors couleur, et le nombre de traits (poil : 1 + 3 ; motif :
    // 1 + 4 + 4).
    let svg = export_svg(&scene).unwrap();
    assert_eq!(svg.matches("data-grain-sense=\"nap\"").count(), 4);
    assert_eq!(svg.matches("data-grain-sense=\"pattern\"").count(), 9);
    assert_eq!(svg_grain_lines(&svg), 13);
}

#[test]
fn formes_des_fleches_distinctes_sans_la_couleur() {
    let (a, b) = (pt(0.0, 0.0), pt(40.0, 0.0));
    let none = grain_arrow_marks(a, b, GrainSense::Unmarked);
    let nap = grain_arrow_marks(a, b, GrainSense::Nap);
    let pattern = grain_arrow_marks(a, b, GrainSense::Pattern);
    // Aucun sens : les deux pointes ouvertes d'avant (quatre traits).
    assert_eq!(none, pf_export::grainline_arrow_marks(a, b));
    assert_eq!(none.len(), 4);
    // Poil : une seule pointe, fermée (triangle : trois traits), au bout `b`.
    assert_eq!(nap.len(), 3);
    assert!(
        nap[0].0 == b && nap[1].0 == b,
        "les deux branches partent de b"
    );
    for (p, q) in &nap[..2] {
        assert!((distance_mm(*p, *q) - ARROW_LEN_MM).abs() < 1e-9);
        assert!(q.x < b.x, "la pointe regarde vers l'extérieur du trait");
    }
    assert!(
        nap[2].0 == nap[0].1 && nap[2].1 == nap[1].1,
        "le triangle est fermé"
    );
    assert!(
        nap.iter().all(|(p, q)| p.x > a.x + 1.0 && q.x > a.x + 1.0),
        "rien à l'autre bout"
    );
    // Motif : les quatre traits d'avant et quatre autres, pointes doublées à 2 mm en retrait.
    assert_eq!(pattern.len(), 8);
    assert_eq!(&pattern[..4], &none[..]);
    let tips: Vec<PointMm> = pattern[4..].iter().map(|m| m.0).collect();
    assert!(tips.contains(&pt(GRAIN_DOUBLE_HEAD_MM, 0.0)));
    assert!(tips
        .iter()
        .any(|t| (t.x - (40.0 - GRAIN_DOUBLE_HEAD_MM)).abs() < 1e-9));
    // Trois formes deux à deux différentes.
    assert!(none != nap && none != pattern && nap != pattern);
    // Trop court pour porter des pointes : aucune, quel que soit le sens.
    for sense in [GrainSense::Unmarked, GrainSense::Nap, GrainSense::Pattern] {
        assert!(grain_arrow_marks(a, pt(8.0, 0.0), sense).is_empty());
    }
}

#[test]
fn pdf_formes_mesurees_et_legende_seulement_si_un_sens_existe() {
    let count_for = |sense: GrainSense| -> (usize, String) {
        let mut sc = project();
        with_grain(
            &mut sc,
            PieceGrain {
                sense,
                rotation_tolerance_deg: Some(5.0),
                ..PieceGrain::default()
            },
        );
        let scene = Scene::from_sheet(&sc.p, sc.s).unwrap();
        let pdf = export_pdf(std::slice::from_ref(&scene)).unwrap();
        let pages = page_streams(&pdf.bytes);
        // Traits de la feuille (page 2) qui ont une extrémité sur le droit-fil réglé (x = 50).
        let segs = pdf_segments(&pages[1]);
        let n = segs.len();
        (n, pages[0].clone())
    };
    let (base, legend0) = count_for(GrainSense::Unmarked);
    let (nap, legend1) = count_for(GrainSense::Nap);
    let (pattern, legend2) = count_for(GrainSense::Pattern);
    // Aucun sens : 4 branches ; poil : 3 traits ; motif : 8 traits (la ligne non réglée garde
    // ses 4 branches dans tous les cas).
    assert_eq!(nap, base - 1);
    assert_eq!(pattern, base + 4);
    assert!(!legend0.contains("Sens du droit-fil"));
    assert!(legend1.contains("Sens du droit-fil") && legend2.contains("Sens du droit-fil"));
    // Longueur d'une branche relue : 4 mm.
    let mut sc = project();
    with_grain(
        &mut sc,
        PieceGrain {
            sense: GrainSense::Nap,
            ..PieceGrain::default()
        },
    );
    let scene = Scene::from_sheet(&sc.p, sc.s).unwrap();
    let pdf = export_pdf(std::slice::from_ref(&scene)).unwrap();
    let segs = pdf_segments(&page_streams(&pdf.bytes)[1]);
    let branches = segs
        .iter()
        .filter(|s| (len(s) - ARROW_LEN_MM).abs() < 1e-3)
        .count();
    assert!(branches >= 2, "branches de 4 mm relues : {branches}");
}

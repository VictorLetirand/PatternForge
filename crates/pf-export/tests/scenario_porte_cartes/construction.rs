//! Scénario PH-3b : porte-cartes en cuir (fond, poche, patte) construit **uniquement** par
//! `Project::apply(Command)`. Partagé par le test `scenario_porte_cartes.rs` et par
//! `examples/generer_exemples.rs` (inclusion par `#[path]`).
//!
//! Une feuille « Porte-cartes », trois pièces (mm, Y vers le haut) :
//! - fond : rectangle (0 ; 0)-(100 ; 70), coins arrondis de rayon 5 (congés), coupe nette,
//!   23 perçages de 1,2 mm à y = 3, x = 6 + 4k (k = 0 à 22) ;
//! - poche : rectangle (120 ; 0)-(210 ; 50), bord haut en arc de flèche 5 (rayon 205), marge de
//!   cuir 6 mm, rempli de 8 mm sur l'arc ;
//! - patte : (0 ; -20)-(0 ; -50), demi-cercle de rayon 40 (sommet (40 ; -90)), (80 ; -50)-(80 ; -20),
//!   coupe nette.
#![allow(dead_code)]

use pf_core::{
    Command, CoreError, EntityId, FoldAllowance, MarkingAnchor, MarkingShape, NotchStyle, PathRole,
    PieceCategory, PieceId, PieceSource, PointId, PointRef, Project, SeamAllowance, SheetId,
};
use pf_geometry::{PointMm, Side};

pub struct PorteCartes {
    pub project: Project,
    pub sheet: SheetId,
    pub body: PieceId,
    pub pocket: PieceId,
    pub flap: PieceId,
    /// Bord bas droit du fond (de x = 5 à x = 95) et bord bas de la poche.
    pub body_bottom: EntityId,
    pub pocket_bottom: EntityId,
    /// Arc du bord haut de la poche (porte le rempli).
    pub pocket_arc: EntityId,
    /// Bord haut de la patte.
    pub flap_top: EntityId,
    pub holes: Vec<PointId>,
}

/// Page du traceur HPGL : l'emprise de la feuille (210 × 160 mm) tient sur une A4 paysage.
pub fn hpgl_page() -> pf_export::PageSetup {
    pf_export::PageSetup {
        orientation: pf_export::Orientation::Landscape,
        ..Default::default()
    }
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

pub fn build() -> Result<PorteCartes, CoreError> {
    let mut p = Project::new("Porte-cartes en cuir");
    let sheet = p.sheets[0].id;
    p.apply(Command::RenameSheet {
        sheet,
        name: "Porte-cartes".into(),
    })?;
    let seg = |p: &mut Project, a: PointMm, b: PointMm| -> Result<EntityId, CoreError> {
        Ok(p.apply(Command::AddSegment {
            sheet,
            from: PointRef::New(a),
            to: PointRef::New(b),
        })?
        .entities[0])
    };
    let role = |p: &mut Project, entity: EntityId| -> Result<(), CoreError> {
        p.apply(Command::SetPathRole {
            sheet,
            entity,
            role: PathRole::SewingContour,
        })?;
        Ok(())
    };
    let notch = |p: &mut Project, entity: EntityId, distance_mm: f64| -> Result<(), CoreError> {
        p.apply(Command::AddNotch {
            sheet,
            entity,
            distance_mm,
            side: Side::Right,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })?;
        Ok(())
    };
    let card = |p: &mut Project, piece: PieceId, name: &str, code: &str| {
        p.apply(Command::SetPieceCard {
            piece,
            name: name.into(),
            code: Some(code.into()),
            quantity: 1,
            mirrored_pair: false,
            material: Some("Cuir".into()),
            category: Some(PieceCategory::Leather),
            comment: None,
        })
    };

    // --- Fond : rectangle 100 × 70, quatre congés de 5 mm -------------------------------------
    let bottom = seg(&mut p, pt(0.0, 0.0), pt(100.0, 0.0))?;
    let right = seg(&mut p, pt(100.0, 0.0), pt(100.0, 70.0))?;
    let top = seg(&mut p, pt(100.0, 70.0), pt(0.0, 70.0))?;
    let left = seg(&mut p, pt(0.0, 70.0), pt(0.0, 0.0))?;
    let fillet = |p: &mut Project,
                  first: EntityId,
                  second: EntityId,
                  a: PointMm,
                  b: PointMm|
     -> Result<EntityId, CoreError> {
        Ok(*p
            .apply(Command::FilletPaths {
                sheet,
                first,
                second,
                radius_mm: 5.0,
                pick_first: a,
                pick_second: b,
            })?
            .entities
            .last()
            .expect("arc du congé"))
    };
    let a01 = fillet(&mut p, bottom, right, pt(50.0, 0.0), pt(100.0, 35.0))?;
    let a12 = fillet(&mut p, right, top, pt(100.0, 35.0), pt(50.0, 70.0))?;
    let a23 = fillet(&mut p, top, left, pt(50.0, 70.0), pt(0.0, 35.0))?;
    let a30 = fillet(&mut p, left, bottom, pt(0.0, 35.0), pt(50.0, 0.0))?;
    let body_edges = vec![bottom, a01, right, a12, top, a23, left, a30];
    for e in &body_edges {
        role(&mut p, *e)?;
    }
    let body = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: body_edges,
            },
            name: "Fond".into(),
            detached: false,
        })?
        .pieces[0];
    p.apply(Command::SetNetCut {
        piece: body,
        net_cut: true,
    })?;
    // Perçages de couture sellier : 3 mm du bord bas, pas de 4 mm, de x = 6 à x = 94.
    let mut holes = Vec::new();
    for k in 0..23 {
        let point = p
            .apply(Command::AddPoint {
                sheet,
                at: pt(6.0 + 4.0 * k as f64, 3.0),
            })?
            .points[0];
        p.apply(Command::AddPieceMarking {
            piece: body,
            at: MarkingAnchor::Point { point },
            shape: MarkingShape::Drill { diameter_mm: 1.2 },
        })?;
        holes.push(point);
    }
    // Crans de repère d'assemblage avec la poche : à 20 et 70 mm du début du bord droit du bas.
    notch(&mut p, bottom, 20.0)?;
    notch(&mut p, bottom, 70.0)?;
    card(&mut p, body, "Fond", "PC-01")?;

    // --- Poche : rectangle 90 × 50, bord haut en arc de flèche 5 -------------------------------
    let pocket_bottom = seg(&mut p, pt(120.0, 0.0), pt(210.0, 0.0))?;
    let pocket_right = seg(&mut p, pt(210.0, 0.0), pt(210.0, 50.0))?;
    let pocket_arc = p
        .apply(Command::AddArcThreePoints {
            sheet,
            start: PointRef::New(pt(210.0, 50.0)),
            through: PointRef::New(pt(165.0, 55.0)),
            end: PointRef::New(pt(120.0, 50.0)),
        })?
        .entities[0];
    let pocket_left = seg(&mut p, pt(120.0, 50.0), pt(120.0, 0.0))?;
    let pocket_edges = vec![pocket_bottom, pocket_right, pocket_arc, pocket_left];
    for e in &pocket_edges {
        role(&mut p, *e)?;
    }
    let pocket = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: pocket_edges,
            },
            name: "Poche".into(),
            detached: false,
        })?
        .pieces[0];
    notch(&mut p, pocket_bottom, 20.0)?;
    notch(&mut p, pocket_bottom, 70.0)?;
    card(&mut p, pocket, "Poche", "PC-02")?;
    // Marge de la catégorie « Cuir » (jeu par défaut de l'atelier) ; rempli de 8 mm sur l'arc.
    let leather_mm = p.category_seam_mm(pocket)?;
    p.apply(Command::SetSeamAllowance {
        piece: pocket,
        seam: Some(SeamAllowance {
            default_mm: leather_mm,
            folds: vec![FoldAllowance {
                entity: pocket_arc,
                value_mm: 8.0,
                notch_step_mm: None,
                notch_count: None,
            }],
            ..Default::default()
        }),
    })?;

    // --- Patte : bords droits de 30 mm et demi-cercle de rayon 40, coupe nette -----------------
    let flap_left = seg(&mut p, pt(0.0, -20.0), pt(0.0, -50.0))?;
    let flap_arc = p
        .apply(Command::AddArcThreePoints {
            sheet,
            start: PointRef::New(pt(0.0, -50.0)),
            through: PointRef::New(pt(40.0, -90.0)),
            end: PointRef::New(pt(80.0, -50.0)),
        })?
        .entities[0];
    let flap_right = seg(&mut p, pt(80.0, -50.0), pt(80.0, -20.0))?;
    let flap_top = seg(&mut p, pt(80.0, -20.0), pt(0.0, -20.0))?;
    let flap_edges = vec![flap_left, flap_arc, flap_right, flap_top];
    for e in &flap_edges {
        role(&mut p, *e)?;
    }
    let flap = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: flap_edges,
            },
            name: "Patte".into(),
            detached: false,
        })?
        .pieces[0];
    p.apply(Command::SetNetCut {
        piece: flap,
        net_cut: true,
    })?;
    // Cran de repère au milieu du bord haut (assemblage avec le dos du fond).
    notch(&mut p, flap_top, 40.0)?;
    card(&mut p, flap, "Patte", "PC-03")?;
    Ok(PorteCartes {
        project: p,
        sheet,
        body,
        pocket,
        flap,
        body_bottom: bottom,
        pocket_bottom,
        pocket_arc,
        flap_top,
        holes,
    })
}

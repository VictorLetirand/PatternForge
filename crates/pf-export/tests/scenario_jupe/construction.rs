//! Scénario PH-3a : jupe droite de base (devant, sur le pli), construite **uniquement** par
//! `Project::apply(Command)`. Partagé par le test `scenario_jupe_droite.rs` et par
//! `examples/generer_exemples.rs` (inclusion par `#[path]`), pour que le fichier d'exemple et le
//! test décrivent le même projet.
//!
//! Mesures d'exemple (mm, Y vers le haut), contour anti-horaire A → D → C → H → B :
//! A (0 ; 600) milieu devant en taille, D (0 ; 0) milieu devant en bas, C (240 ; 0) côté en bas,
//! H (240 ; 360) côté à la hanche (hauteur de hanches 240 sous la taille), B (170 ; 600) côté en
//! taille. 1/4 de hanches 240, 1/4 de taille 150 + pince 20 = 170, longueur 600.
#![allow(dead_code)]

use pf_core::{
    Command, CoreError, DartId, EdgeAllowance, EntityId, HemAllowance, MarkingAnchor, MarkingShape,
    NotchStyle, PathRole, PieceCategory, PieceId, PieceSource, PointId, PointRef, Project,
    SeamAllowance, SheetId,
};
use pf_geometry::{PointMm, Side};

pub struct Jupe {
    pub project: Project,
    pub sheet: SheetId,
    pub piece: PieceId,
    /// Milieu devant (pli), ourlet (bas), côté bas, côté haut, taille.
    pub cf: EntityId,
    pub hem: EntityId,
    pub side_low: EntityId,
    pub side_up: EntityId,
    pub waist: EntityId,
    pub dart: DartId,
    pub apex: PointId,
    pub axis: PointId,
    pub grain: EntityId,
}

/// Page du traceur HPGL : la jupe (255 × 650 mm) ne tient pas sur une A4, le HPGL n'est pas
/// tuilé ; une feuille A0 la contient.
pub fn hpgl_page() -> pf_export::PageSetup {
    pf_export::PageSetup {
        format: pf_export::PageFormat::A0,
        ..Default::default()
    }
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

pub fn build() -> Result<Jupe, CoreError> {
    let mut p = Project::new("Jupe droite de base");
    let sheet = p.sheets[0].id;
    p.apply(Command::RenameSheet {
        sheet,
        name: "Jupe devant".into(),
    })?;
    // Contour : cinq points, cinq segments (rôle couture), sens anti-horaire.
    let corners = [
        (0.0, 600.0),
        (0.0, 0.0),
        (240.0, 0.0),
        (240.0, 360.0),
        (170.0, 600.0),
    ];
    let mut ids = Vec::new();
    for (x, y) in corners {
        ids.push(
            p.apply(Command::AddPoint {
                sheet,
                at: pt(x, y),
            })?
            .points[0],
        );
    }
    let mut edges = Vec::new();
    for i in 0..5 {
        let e = p
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::Existing(ids[i]),
                to: PointRef::Existing(ids[(i + 1) % 5]),
            })?
            .entities[0];
        p.apply(Command::SetPathRole {
            sheet,
            entity: e,
            role: PathRole::SewingContour,
        })?;
        edges.push(e);
    }
    let (cf, hem, side_low, side_up, waist) = (edges[0], edges[1], edges[2], edges[3], edges[4]);
    // Pièce extraite par ses cinq tracés.
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: edges.clone(),
            },
            name: "Jupe devant".into(),
            detached: false,
        })?
        .pieces[0];
    // Pince de taille : sommet (85 ; 500), axe vers (85 ; 600), longueur 100, ouverture 20.
    let dart = p
        .apply(Command::AddPieceDart {
            piece,
            apex: PointRef::New(pt(85.0, 500.0)),
            toward: PointRef::New(pt(85.0, 600.0)),
            length_mm: 100.0,
            opening_mm: 20.0,
            fixed: Side::Left,
        })?
        .darts[0];
    let (apex, axis) = {
        let d = &p.piece(piece)?.darts[0];
        (d.apex, d.toward)
    };
    // Pointage à 10 mm de la pointe de pince.
    p.apply(Command::AddPieceMarking {
        piece,
        at: MarkingAnchor::DartTip {
            tip: apex,
            toward: axis,
            setback_mm: 10.0,
        },
        shape: MarkingShape::Punch,
    })?;
    // Crans (côté extérieur = droite du sens de parcours) : hanche sur le côté et sur le pli,
    // pieds de la pince sur la taille.
    for (entity, distance_mm) in [(side_low, 360.0), (cf, 240.0), (waist, 75.0), (waist, 95.0)] {
        p.apply(Command::AddNotch {
            sheet,
            entity,
            distance_mm,
            side: Side::Right,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })?;
    }
    // Droit-fil parallèle au milieu devant.
    let grain = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(120.0, 100.0)),
            to: PointRef::New(pt(120.0, 400.0)),
        })?
        .entities[0];
    p.apply(Command::SetPathRole {
        sheet,
        entity: grain,
        role: PathRole::Grainline,
    })?;
    let outline = p.piece(piece)?.outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(grain),
    })?;
    // Marges : 10 mm partout, 0 sur le pli, ourlet de 40 mm en bas.
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            edges: vec![EdgeAllowance {
                entity: cf,
                start_mm: 0.0,
                end_mm: 0.0,
            }],
            hems: vec![HemAllowance {
                entity: hem,
                height_mm: 40.0,
            }],
            ..Default::default()
        }),
    })?;
    // Étiquette de pièce.
    p.apply(Command::SetPieceCard {
        piece,
        name: "Jupe devant".into(),
        code: Some("JD-01".into()),
        quantity: 1,
        mirrored_pair: false,
        material: Some("Tissu".into()),
        category: Some(PieceCategory::Fabric),
        comment: Some("Couper au pli, milieu devant sur la pliure".into()),
    })?;
    Ok(Jupe {
        project: p,
        sheet,
        piece,
        cf,
        hem,
        side_low,
        side_up,
        waist,
        dart,
        apex,
        axis,
        grain,
    })
}

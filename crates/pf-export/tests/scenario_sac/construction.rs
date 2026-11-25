//! Exemple « sac cabas » (US-16.9) : deux pièces (corps, anse) sur deux feuilles, construites
//! **uniquement** par `Project::apply(Command)`. Partagé par `tests/exemple_sac.rs` et
//! `examples/generer_exemple_sac.rs` (inclusion par `#[path]`).
//!
//! Mesures d'exemple (mm) : corps 400 x 380, anse 700 x 40. Contour anti-horaire.
#![allow(dead_code)]

use pf_core::{
    Command, CoreError, EdgeAllowance, HemAllowance, NotchStyle, PathRole, PieceCategory,
    PieceSource, PointRef, Project, SeamAllowance, SheetId,
};
use pf_geometry::{PointMm, Side};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Description d'une pièce rectangulaire w x h.
struct Rect<'a> {
    size: (f64, f64),
    name: &'a str,
    code: &'a str,
    comment: &'a str,
    notch_at: &'a [(usize, f64)],
    hem_top: Option<f64>,
}

/// Ajoute la pièce avec droit-fil, crans et fiche.
fn rectangle_piece(p: &mut Project, sheet: SheetId, spec: Rect) -> Result<(), CoreError> {
    let Rect {
        size: (w, h),
        name,
        code,
        comment,
        notch_at,
        hem_top,
    } = spec;
    let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
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
    for i in 0..4 {
        let e = p
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::Existing(ids[i]),
                to: PointRef::Existing(ids[(i + 1) % 4]),
            })?
            .entities[0];
        p.apply(Command::SetPathRole {
            sheet,
            entity: e,
            role: PathRole::SewingContour,
        })?;
        edges.push(e);
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: edges.clone(),
            },
            name: name.into(),
            detached: false,
        })?
        .pieces[0];
    for &(edge, distance_mm) in notch_at {
        p.apply(Command::AddNotch {
            sheet,
            entity: edges[edge],
            distance_mm,
            side: Side::Right,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })?;
    }
    let grain = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(w / 2.0, h * 0.2)),
            to: PointRef::New(pt(w / 2.0, h * 0.8)),
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
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            edges: Vec::<EdgeAllowance>::new(),
            hems: hem_top
                .map(|height_mm| {
                    vec![HemAllowance {
                        entity: edges[2],
                        height_mm,
                    }]
                })
                .unwrap_or_default(),
            ..Default::default()
        }),
    })?;
    p.apply(Command::SetPieceCard {
        piece,
        name: name.into(),
        code: Some(code.into()),
        quantity: 2,
        mirrored_pair: false,
        material: Some("Toile".into()),
        category: Some(PieceCategory::Fabric),
        comment: Some(comment.into()),
    })?;
    Ok(())
}

pub fn build() -> Result<Project, CoreError> {
    let mut p = Project::new("Sac cabas");
    let body = p.sheets[0].id;
    p.apply(Command::RenameSheet {
        sheet: body,
        name: "Corps".into(),
    })?;
    // Le bord haut (arête 2) porte l'ourlet de 30 mm ; crans de milieu en bas et en haut.
    rectangle_piece(
        &mut p,
        body,
        Rect {
            size: (400.0, 380.0),
            name: "Corps",
            code: "SC-01",
            comment: "Couper deux fois, ourlet de 30 mm en haut",
            notch_at: &[(0, 200.0), (2, 200.0)],
            hem_top: Some(30.0),
        },
    )?;
    let strap = p
        .apply(Command::CreateSheet {
            name: "Anse".into(),
        })?
        .sheet
        .expect("feuille créée");
    rectangle_piece(
        &mut p,
        strap,
        Rect {
            size: (700.0, 40.0),
            name: "Anse",
            code: "SC-02",
            comment: "Couper deux fois dans le droit-fil",
            notch_at: &[(0, 350.0)],
            hem_top: None,
        },
    )?;
    Ok(p)
}

//! Contrôle d'une [`Scene`] avant tout export : une valeur non finie donnerait un SVG ou un PDF
//! invalide, ou une image fausse, sans erreur. Chaque écrivain appelle
//! [`Scene::check_finite`] avant de produire quoi que ce soit.
//!
//! Les structures sont déconstruites **sans `..`** : un champ ajouté à la scène ne compile pas
//! tant que son contrôle n'est pas décidé ici.

use crate::{
    ExportError, PathGeom, Scene, SceneButton, SceneCutContour, SceneDart, SceneDimension,
    SceneFold, SceneFoldOver, SceneFullDimension, SceneMarking, ScenePath, ScenePieceLabel,
    ScenePoint, SceneStitchHole, SceneZone,
};
use pf_core::{ButtonShape, MarkingShape, StitchHole};
use pf_geometry::{CubicBezier, PointMm};

fn nums(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn pt(p: &PointMm) -> bool {
    nums(&[p.x, p.y])
}

fn pts(ps: &[PointMm]) -> bool {
    ps.iter().all(pt)
}

fn pairs(ps: &[(PointMm, PointMm)]) -> bool {
    ps.iter().all(|(a, b)| pt(a) && pt(b))
}

fn cubic(c: &CubicBezier) -> bool {
    pts(&[c.p0, c.c1, c.c2, c.p3])
}

fn geom(g: &PathGeom) -> bool {
    match g {
        PathGeom::Line(a, b) => pt(a) && pt(b),
        PathGeom::Cubic(c) => cubic(c),
        PathGeom::Arc(a) => pt(&a.center) && nums(&[a.radius_mm, a.start_rad, a.sweep_rad]),
        PathGeom::Circle(c) => pt(&c.circle.center) && nums(&[c.circle.radius_mm, c.start_deg]),
        PathGeom::Spline(s) => s.iter().all(cubic),
    }
}

fn path(p: &ScenePath) -> bool {
    let ScenePath {
        geom: g,
        role: _,
        sense: _,
    } = p;
    geom(g)
}

fn point(p: &ScenePoint) -> bool {
    let ScenePoint {
        pos,
        label: _,
        show_label: _,
    } = p;
    pt(pos)
}

fn dimension(d: &SceneDimension) -> bool {
    let SceneDimension {
        from,
        to,
        from_offset,
        to_offset,
        text: _,
    } = d;
    pts(&[*from, *to, *from_offset, *to_offset])
}

fn full_dimension(d: &SceneFullDimension) -> bool {
    let SceneFullDimension {
        kind: _,
        value,
        marks,
        text_center,
        text_angle_deg,
        text: _,
    } = d;
    nums(&[*value, *text_angle_deg]) && pairs(marks) && pt(text_center)
}

fn marking(m: &SceneMarking) -> bool {
    let SceneMarking { pos, shape } = m;
    pt(pos)
        && match *shape {
            MarkingShape::Punch => true,
            MarkingShape::Drill { diameter_mm } => diameter_mm.is_finite(),
            MarkingShape::Cross { size_mm } => size_mm.is_finite(),
            MarkingShape::Slot {
                length_mm,
                width_mm,
                angle_deg,
            } => nums(&[length_mm, width_mm, angle_deg]),
        }
}

fn stitch_hole(h: &SceneStitchHole) -> bool {
    let SceneStitchHole {
        pos,
        hole,
        tangent_deg,
    } = h;
    pt(pos)
        && tangent_deg.is_finite()
        && match *hole {
            StitchHole::Round { diameter_mm } => diameter_mm.is_finite(),
            StitchHole::Flat {
                length_mm,
                width_mm,
            }
            | StitchHole::French {
                length_mm,
                width_mm,
            } => nums(&[length_mm, width_mm]),
            StitchHole::Japanese {
                length_mm,
                width_mm,
                slant_deg,
            } => nums(&[length_mm, width_mm, slant_deg]),
        }
}

fn zone(z: &SceneZone) -> bool {
    let SceneZone {
        kind: _,
        polygon,
        width_mm,
    } = z;
    pts(polygon) && width_mm.is_none_or(f64::is_finite)
}

fn button(b: &SceneButton) -> bool {
    let SceneButton { pos, shape } = b;
    pt(pos)
        && match *shape {
            ButtonShape::Button { diameter_mm } => diameter_mm.is_finite(),
            ButtonShape::Buttonhole {
                length_mm,
                width_mm,
                angle_deg,
            } => nums(&[length_mm, width_mm, angle_deg]),
        }
}

fn cut_contour(c: &SceneCutContour) -> bool {
    let SceneCutContour {
        piece: _,
        edges,
        closed: _,
        notch_marks,
        cut_notch_marks,
    } = c;
    edges.iter().all(geom) && pairs(notch_marks) && pairs(cut_notch_marks)
}

fn dart(d: &SceneDart) -> bool {
    let SceneDart {
        piece: _,
        apex,
        left_foot,
        right_foot,
    } = d;
    pts(&[*apex, *left_foot, *right_foot])
}

fn piece_label(l: &ScenePieceLabel) -> bool {
    let ScenePieceLabel {
        piece: _,
        center,
        lines: _,
    } = l;
    pt(center)
}

fn fold(f: &SceneFold) -> bool {
    let SceneFold {
        piece: _,
        from,
        to,
        label: _,
    } = f;
    pt(from) && pt(to)
}

fn fold_over(f: &SceneFoldOver) -> bool {
    let SceneFoldOver {
        piece: _,
        at,
        inward,
        label: _,
    } = f;
    pt(at) && nums(&[inward.x, inward.y])
}

impl Scene {
    /// Refuse (`ExportError::NotFinite`, avec le nom de la feuille) une scène dont une valeur
    /// numérique n'est pas finie, ou dont l'emprise déborde les nombres représentables.
    pub fn check_finite(&self) -> Result<(), ExportError> {
        let Scene {
            project_name: _,
            sheet_name,
            paths,
            points,
            notch_marks,
            dimensions,
            full_dimensions,
            markings,
            stitch_holes,
            zones,
            buttons,
            // Jamais écrites : une tolérance non finie interdit seulement la rotation.
            grain_limits: _,
            cut_contours,
            cut_warnings: _,
            darts,
            dart_warnings: _,
            piece_labels,
            folds,
            fold_overs,
        } = self;
        let ok = paths.iter().all(path)
            && points.iter().all(point)
            && pairs(notch_marks)
            && dimensions.iter().all(dimension)
            && full_dimensions.iter().all(full_dimension)
            && markings.iter().all(marking)
            && stitch_holes.iter().all(stitch_hole)
            && zones.iter().all(zone)
            && buttons.iter().all(button)
            && cut_contours.iter().all(cut_contour)
            && darts.iter().all(dart)
            && piece_labels.iter().all(piece_label)
            && folds.iter().all(fold)
            && fold_overs.iter().all(fold_over);
        // Des valeurs finies mais démesurées peuvent encore déborder en les soustrayant.
        if ok && self.bbox().is_none_or(|b| nums(&[b.width(), b.height()])) {
            Ok(())
        } else {
            Err(ExportError::NotFinite(sheet_name.clone()))
        }
    }
}

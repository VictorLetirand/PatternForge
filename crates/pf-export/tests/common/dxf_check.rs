//! Vérification d'un export DXF contre le modèle (US-12.1), partagée par les tests de critères
//! et la campagne adverse. Le fichier est relu par `common::dxf` (lecteur indépendant) ; le
//! modèle est lu par `pf_core` (mesures exactes, positions calculées). Les numéros de couches
//! sont réécrits ici en dur : ils ne viennent pas de la table de l'écrivain.
#![allow(dead_code)]

use super::dxf::*;
use pf_core::piece_internal::InternalGeometry;
use pf_core::{ButtonShape, MarkingShape, PieceId, Project, Shape};
use pf_export::dxf::DxfReport;
use pf_geometry::PointMm;

pub const L_BOUNDARY: &str = "1";
pub const L_NOTCH: &str = "4";
pub const L_GRAIN: &str = "6";
pub const L_INTERNAL: &str = "7";
pub const L_CUTOUT: &str = "8";
pub const L_DRILL: &str = "13";
pub const L_SEWING: &str = "15";
pub const L_TEXT: &str = "84";
/// Écart accepté entre le fichier relu et la géométrie du modèle (critère d'acceptation).
pub const TOL: f64 = 0.01;

pub fn placed(project: &Project, piece: PieceId) -> impl Fn(PointMm) -> P {
    let item = project
        .plan_layout()
        .into_iter()
        .find(|i| i.piece == piece)
        .expect("pièce du plan");
    let (pl, o) = (item.placement.unwrap(), item.origin.unwrap());
    move |p: PointMm| {
        let q = pl.apply(o, p);
        (q.x, q.y)
    }
}

fn matches_pair(a: P, b: P, c: P, d: P) -> bool {
    dist(a, c) <= TOL && dist(b, d) <= TOL
}

/// Associe chaque segment attendu à un segment distinct du fichier (extrémités dans l'ordre).
fn match_segments(expected: &[(P, P)], found: &[(P, P)], what: &str) -> Result<(), String> {
    let mut used = vec![false; found.len()];
    for (k, (a, b)) in expected.iter().enumerate() {
        let hit = found
            .iter()
            .enumerate()
            .position(|(i, (c, d))| !used[i] && matches_pair(*a, *b, *c, *d));
        match hit {
            Some(i) => used[i] = true,
            None => return Err(format!("{what} {k} ({a:?} → {b:?}) absent du fichier")),
        }
    }
    Ok(())
}

fn centroid(pts: &[P]) -> P {
    let n = pts.len();
    let (mut a2, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        let cr = p.0 * q.1 - q.0 * p.1;
        a2 += cr;
        cx += (p.0 + q.0) * cr;
        cy += (p.1 + q.1) * cr;
    }
    (cx / (3.0 * a2), cy / (3.0 * a2))
}

/// Résumé de ce qui a été relu (pour les assertions des tests de critères).
#[derive(Debug, Default)]
pub struct Checked {
    pub pieces: usize,
    pub notch_lines: usize,
    pub grain_lines: usize,
    pub circles: usize,
    pub points: usize,
    pub texts: usize,
    pub layer_counts: std::collections::BTreeMap<String, usize>,
}

/// Vérifie tout le fichier. `Err` = description de la première divergence.
pub fn verify_export(
    project: &Project,
    text: &str,
    report: &DxfReport,
) -> Result<(Dxf, Checked), String> {
    let dxf = parse(text)?;
    let mut done = Checked::default();
    // En-tête : version, unités, mesure, emprise.
    dxf.header_str("$ACADVER").ok_or("$ACADVER absent")?;
    if dxf.header_int("$INSUNITS") != Some(4) {
        return Err("$INSUNITS différent de 4 (mm)".into());
    }
    if dxf.header_int("$MEASUREMENT") != Some(1) {
        return Err("$MEASUREMENT différent de 1 (métrique)".into());
    }
    let (emin, emax) = (
        dxf.header_point("$EXTMIN").ok_or("$EXTMIN absent")?,
        dxf.header_point("$EXTMAX").ok_or("$EXTMAX absent")?,
    );
    // Tables : type de ligne et couches déclarés.
    if !dxf.ltypes.iter().any(|l| l == "CONTINUOUS") {
        return Err("type de ligne CONTINUOUS absent".into());
    }
    let inserted = dxf.inserted()?;
    if inserted.len() != report.pieces.len() {
        return Err(format!(
            "{} insertions pour {} pièces du rapport",
            inserted.len(),
            report.pieces.len()
        ));
    }
    if dxf.blocks.len() != inserted.len() {
        return Err("bloc non inséré".into());
    }
    let mut auto_boxes: Vec<(P, P)> = Vec::new();
    for (info, (block, ents)) in report.pieces.iter().zip(&inserted) {
        if *block != info.block {
            return Err(format!("bloc {block} au lieu de {}", info.block));
        }
        if ents.len() != info.entities {
            return Err(format!("{block} : nombre d'entités différent du rapport"));
        }
        let b = verify_piece(project, info.id, ents, &dxf, (emin, emax), &mut done)?;
        if project.piece(info.id).unwrap().placement.is_none() {
            auto_boxes.push(b);
        }
        done.pieces += 1;
    }
    // Placement automatique : jamais de chevauchement (les pièces placées à la main sont libres).
    for (i, a) in auto_boxes.iter().enumerate() {
        for b in &auto_boxes[i + 1..] {
            let apart =
                a.1 .0 <= b.0 .0 || b.1 .0 <= a.0 .0 || a.1 .1 <= b.0 .1 || b.1 .1 <= a.0 .1;
            if !apart {
                return Err("deux pièces au placement automatique se chevauchent".into());
            }
        }
    }
    // Le rapport compte exactement les entités par couche.
    for l in &report.layers {
        let name = l.layer.number.to_string();
        let n = done.layer_counts.get(&name).copied().unwrap_or(0);
        if n != l.entities {
            return Err(format!(
                "couche {name} : {n} entités relues, {} dans le rapport",
                l.entities
            ));
        }
    }
    let total: usize = done.layer_counts.values().sum();
    let reported: usize = report.layers.iter().map(|l| l.entities).sum();
    if total != reported {
        return Err("entité sur une couche absente du rapport".into());
    }
    Ok((dxf, done))
}

fn verify_piece(
    project: &Project,
    id: PieceId,
    ents: &[Ent],
    dxf: &Dxf,
    (emin, emax): (P, P),
    done: &mut Checked,
) -> Result<(P, P), String> {
    let piece = project.piece(id).unwrap();
    let tr = placed(project, id);
    let sheet = project.piece_sheet(id).unwrap();
    let contour = project.piece_contour(id).unwrap();
    let name = &piece.name;
    let known = |layer: &str| dxf.layers.iter().any(|(n, _)| n == layer);
    for e in ents {
        *done.layer_counts.entry(e.layer().to_string()).or_default() += 1;
        if !known(e.layer()) {
            return Err(format!("{name} : couche {} absente de la table", e.layer()));
        }
        // Toute coordonnée est dans l'emprise déclarée.
        let pts: Vec<P> = match e {
            Ent::Line { a, b, .. } => vec![*a, *b],
            Ent::Point { p, .. } => vec![*p],
            Ent::Circle { c, r, .. } => vec![(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)],
            Ent::Polyline { pts, .. } => pts.clone(),
            Ent::Text { at, .. } => vec![*at],
            Ent::Insert { .. } => Vec::new(),
        };
        for p in pts {
            if p.0 < emin.0 - 1e-4
                || p.1 < emin.1 - 1e-4
                || p.0 > emax.0 + 1e-4
                || p.1 > emax.1 + 1e-4
            {
                return Err(format!("{name} : {p:?} hors de $EXTMIN/$EXTMAX"));
            }
        }
    }
    fn layer_ents<'a>(ents: &'a [Ent], layer: &'a str) -> impl Iterator<Item = &'a Ent> {
        ents.iter().filter(move |e| e.layer() == layer)
    }
    let on = |layer: &'static str| layer_ents(ents, layer);

    // ----- contour : une polyligne fermée, couche de contour ou de couture -----
    let boundary: Vec<&Ent> = on(L_BOUNDARY)
        .chain(on(L_SEWING))
        .filter(|e| matches!(e, Ent::Polyline { .. }))
        .collect();
    if boundary.len() != 1 {
        return Err(format!(
            "{name} : {} contours écrits au lieu d'un",
            boundary.len()
        ));
    }
    let Ent::Polyline { pts, closed, .. } = boundary[0] else {
        unreachable!()
    };
    if !closed || pts.len() < 3 {
        return Err(format!("{name} : contour non fermé"));
    }
    let perim = perimeter(pts, true);
    if (perim - contour.perimeter_mm).abs() > TOL {
        return Err(format!(
            "{name} : périmètre {perim} ≠ {} (mesure du modèle)",
            contour.perimeter_mm
        ));
    }
    let a = area(pts);
    if (a - contour.area_mm2).abs() > TOL * contour.perimeter_mm + 1e-6 {
        return Err(format!(
            "{name} : aire {a} ≠ {} (mesure du modèle)",
            contour.area_mm2
        ));
    }
    // Boîte englobante : échantillon dense du modèle, placé (un tracé après l'autre).
    let mut dense_edges: Vec<Vec<P>> = Vec::new();
    for e in &piece.outline {
        let shape = sheet.shape(e.entity).unwrap();
        dense_edges.push(shape.sample(3_000).into_iter().map(&tr).collect());
    }
    let dense: Vec<P> = dense_edges.iter().flatten().copied().collect();
    let (dlo, dhi) = bbox(&dense);
    let (flo, fhi) = bbox(pts);
    for (x, y) in [
        (dlo.0, flo.0),
        (dlo.1, flo.1),
        (dhi.0, fhi.0),
        (dhi.1, fhi.1),
    ] {
        if (x - y).abs() > TOL {
            return Err(format!(
                "{name} : boîte {flo:?}-{fhi:?} ≠ {dlo:?}-{dhi:?} (modèle placé)"
            ));
        }
    }
    // Chaque sommet du fichier est à moins de TOL du contour du modèle (échantillon dense).
    for p in pts.iter().step_by((pts.len() / 120).max(1)) {
        let d = dense_edges
            .iter()
            .map(|e| dist_to_polyline(*p, e, false))
            .fold(f64::INFINITY, f64::min);
        if d > TOL {
            return Err(format!(
                "{name} : sommet {p:?} à {d} mm du contour du modèle"
            ));
        }
    }

    // ----- crans -----
    let mut expected: Vec<(P, P)> = Vec::new();
    for n in sheet
        .notches
        .iter()
        .filter(|n| piece.outline.iter().any(|e| e.entity == n.entity))
    {
        for (a, b) in sheet.notch_marks(n).map_err(|e| e.to_string())? {
            expected.push((tr(a), tr(b)));
        }
    }
    let found: Vec<(P, P)> = on(L_NOTCH)
        .map(|e| match e {
            Ent::Line { a, b, .. } => Ok((*a, *b)),
            other => Err(format!("{name} : entité {other:?} sur la couche des crans")),
        })
        .collect::<Result<_, _>>()?;
    if found.len() != expected.len() {
        return Err(format!(
            "{name} : {} traits de crans, {} attendus",
            found.len(),
            expected.len()
        ));
    }
    match_segments(&expected, &found, &format!("{name} : cran"))?;
    for (foot, _) in &found {
        // Le pied d'un cran est sur le bord (à la discrétisation près).
        if dist_to_polyline(*foot, pts, true) > TOL {
            return Err(format!("{name} : pied de cran {foot:?} hors du contour"));
        }
    }
    done.notch_lines += found.len();

    // ----- droit-fil -----
    let grain: Vec<(P, P)> = on(L_GRAIN)
        .map(|e| match e {
            Ent::Line { a, b, .. } => Ok((*a, *b)),
            other => Err(format!(
                "{name} : entité {other:?} sur la couche du droit-fil"
            )),
        })
        .collect::<Result<_, _>>()?;
    let lines = project.piece_grain_lines(id);
    if piece.grainline.is_none() && !grain.is_empty() {
        return Err(format!("{name} : droit-fil écrit sans droit-fil"));
    }
    let want: Vec<(P, P)> = lines.iter().map(|l| (tr(l.from), tr(l.to))).collect();
    match_segments(&want, &grain, &format!("{name} : droit-fil"))?;
    let mut rest = grain.clone();
    for w in &want {
        let i = rest
            .iter()
            .position(|(c, d)| matches_pair(w.0, w.1, *c, *d))
            .unwrap();
        rest.remove(i);
    }
    for (a, b) in rest {
        // Pointes de flèche : branches de 4 mm au plus.
        if dist(a, b) > 4.001 {
            return Err(format!(
                "{name} : trait de droit-fil de {} mm hors flèche",
                dist(a, b)
            ));
        }
    }
    done.grain_lines += grain.len();

    // ----- perçages, pointages, croix, boutons (couche 13), boutonnières (couche 8) -----
    let mut circles: Vec<(P, f64)> = Vec::new();
    let mut points: Vec<P> = Vec::new();
    let mut crosses: Vec<(P, f64)> = Vec::new();
    let mut drill_slots: Vec<(P, f64, f64)> = Vec::new(); // centre, longueur, largeur
    for m in project.piece_markings(id) {
        match m.marking.shape {
            MarkingShape::Slot {
                length_mm,
                width_mm,
                ..
            } => drill_slots.push((tr(m.at), length_mm, width_mm)),
            MarkingShape::Drill { diameter_mm } => circles.push((tr(m.at), diameter_mm / 2.0)),
            MarkingShape::Punch => points.push(tr(m.at)),
            MarkingShape::Cross { size_mm } => crosses.push((tr(m.at), size_mm)),
        }
    }
    let mut holes: Vec<(P, f64, f64)> = Vec::new(); // centre, longueur, largeur
    for g in project.piece_buttons(id) {
        for el in g.elements {
            match el.shape {
                ButtonShape::Button { diameter_mm } => {
                    circles.push((tr(el.center), diameter_mm / 2.0))
                }
                ButtonShape::Buttonhole {
                    length_mm,
                    width_mm,
                    ..
                } => holes.push((tr(el.center), length_mm, width_mm)),
            }
        }
    }
    let mut internal_circles: Vec<(P, f64)> = Vec::new();
    let mut internal_paths: Vec<f64> = Vec::new();
    let mut internal_points: Vec<P> = Vec::new();
    for r in project.piece_internals(id) {
        match (r.element.target, r.geometry) {
            (pf_core::InternalTarget::Path { entity }, InternalGeometry::Path { length_mm }) => {
                let shape = project
                    .sheets
                    .iter()
                    .find_map(|s| s.shape(entity).ok())
                    .unwrap();
                match shape {
                    Shape::Circle(c) => {
                        internal_circles.push((tr(c.circle.center), c.circle.radius_mm))
                    }
                    _ => internal_paths.push(length_mm),
                }
            }
            (_, InternalGeometry::Position { at }) => internal_points.push(tr(at)),
            _ => {
                return Err(format!(
                    "{name} : élément interne illisible dans un projet valide"
                ))
            }
        }
    }
    let mut f_circles: Vec<(P, f64)> = Vec::new();
    let mut f_points: Vec<P> = Vec::new();
    let mut f_lines: Vec<(P, P)> = Vec::new();
    let mut f_slots: Vec<&Vec<P>> = Vec::new();
    for e in on(L_DRILL) {
        match e {
            Ent::Polyline {
                pts, closed: true, ..
            } => f_slots.push(pts),
            Ent::Circle { c, r, .. } => f_circles.push((*c, *r)),
            Ent::Point { p, .. } => f_points.push(*p),
            Ent::Line { a, b, .. } => f_lines.push((*a, *b)),
            other => return Err(format!("{name} : {other:?} sur la couche des perçages")),
        }
    }
    if f_circles.len() != circles.len() || f_points.len() != points.len() {
        return Err(format!(
            "{name} : {} cercles / {} points de perçage, {} / {} attendus",
            f_circles.len(),
            f_points.len(),
            circles.len(),
            points.len()
        ));
    }
    let mut used = vec![false; f_circles.len()];
    for (c, r) in &circles {
        let i = f_circles
            .iter()
            .enumerate()
            .position(|(i, (fc, fr))| !used[i] && dist(*c, *fc) <= TOL && (r - fr).abs() <= TOL)
            .ok_or(format!("{name} : cercle {c:?} r={r} absent"))?;
        used[i] = true;
    }
    for p in &points {
        if !f_points.iter().any(|q| dist(*p, *q) <= TOL) {
            return Err(format!("{name} : pointage {p:?} absent"));
        }
    }
    if f_lines.len() != 2 * crosses.len() {
        return Err(format!(
            "{name} : {} traits de croix pour {} croix",
            f_lines.len(),
            crosses.len()
        ));
    }
    for (c, size) in &crosses {
        let n = f_lines
            .iter()
            .filter(|(a, b)| {
                ((a.0 + b.0) / 2.0 - c.0).abs() <= TOL
                    && ((a.1 + b.1) / 2.0 - c.1).abs() <= TOL
                    && (dist(*a, *b) - size).abs() <= TOL
            })
            .count();
        if n < 2 {
            return Err(format!("{name} : croix {c:?} de {size} mm absente"));
        }
    }
    if f_slots.len() != drill_slots.len() {
        return Err(format!(
            "{name} : {} fentes de poinçon écrites, {} attendues",
            f_slots.len(),
            drill_slots.len()
        ));
    }
    let mut used_slots = vec![false; f_slots.len()];
    for (c, l, w) in &drill_slots {
        let want_area = (l - w) * w + std::f64::consts::PI * (w / 2.0) * (w / 2.0);
        let want_perim = 2.0 * (l - w) + std::f64::consts::PI * w;
        let i = f_slots
            .iter()
            .enumerate()
            .position(|(i, poly)| {
                !used_slots[i]
                    && dist(centroid(poly), *c) <= TOL
                    && (area(poly) - want_area).abs() <= TOL * want_perim
                    && (perimeter(poly, true) - want_perim).abs() <= TOL
            })
            .ok_or(format!("{name} : fente {l} × {w} en {c:?} absente"))?;
        used_slots[i] = true;
    }
    done.circles += f_circles.len();
    done.points += f_points.len();

    // Couche des découpes : boutonnières (polylignes fermées) et cercles internes.
    let cut_poly: Vec<&Vec<P>> = on(L_CUTOUT)
        .filter_map(|e| match e {
            Ent::Polyline {
                pts, closed: true, ..
            } => Some(pts),
            _ => None,
        })
        .collect();
    let cut_circles: Vec<(P, f64)> = on(L_CUTOUT)
        .filter_map(|e| match e {
            Ent::Circle { c, r, .. } => Some((*c, *r)),
            _ => None,
        })
        .collect();
    if on(L_CUTOUT).count() != cut_poly.len() + cut_circles.len() {
        return Err(format!(
            "{name} : entité inattendue sur la couche des découpes"
        ));
    }
    if cut_poly.len() != holes.len() || cut_circles.len() != internal_circles.len() {
        return Err(format!(
            "{name} : {} boutonnières et {} cercles internes écrits, {} et {} attendus",
            cut_poly.len(),
            cut_circles.len(),
            holes.len(),
            internal_circles.len()
        ));
    }
    let mut used = vec![false; cut_poly.len()];
    for (c, l, w) in &holes {
        // Fente à bouts arrondis : aire = (L − w)·w + π·(w/2)².
        let want_area = (l - w) * w + std::f64::consts::PI * (w / 2.0) * (w / 2.0);
        let want_perim = 2.0 * (l - w) + std::f64::consts::PI * w;
        let i = cut_poly
            .iter()
            .enumerate()
            .position(|(i, poly)| {
                !used[i]
                    && dist(centroid(poly), *c) <= TOL
                    && (area(poly) - want_area).abs() <= TOL * want_perim
                    && (perimeter(poly, true) - want_perim).abs() <= TOL
            })
            .ok_or(format!("{name} : boutonnière {l} × {w} en {c:?} absente"))?;
        used[i] = true;
    }
    for (c, r) in &internal_circles {
        if !cut_circles
            .iter()
            .any(|(fc, fr)| dist(*c, *fc) <= TOL && (r - fr).abs() <= TOL)
        {
            return Err(format!("{name} : cercle interne {c:?} absent"));
        }
    }

    // Lignes internes (couche 7) : polylignes ouvertes et repères.
    let f_open: Vec<&Vec<P>> = on(L_INTERNAL)
        .filter_map(|e| match e {
            Ent::Polyline {
                pts, closed: false, ..
            } => Some(pts),
            _ => None,
        })
        .collect();
    let f_ipoints: Vec<P> = on(L_INTERNAL)
        .filter_map(|e| match e {
            Ent::Point { p, .. } => Some(*p),
            _ => None,
        })
        .collect();
    if on(L_INTERNAL).count() != f_open.len() + f_ipoints.len() {
        return Err(format!(
            "{name} : entité inattendue sur la couche des lignes internes"
        ));
    }
    if f_open.len() != internal_paths.len() || f_ipoints.len() != internal_points.len() {
        return Err(format!(
            "{name} : lignes internes {} / {} attendues",
            f_open.len(),
            internal_paths.len()
        ));
    }
    let mut used = vec![false; f_open.len()];
    for len in &internal_paths {
        let i = f_open
            .iter()
            .enumerate()
            .position(|(i, poly)| !used[i] && (perimeter(poly, false) - len).abs() <= TOL)
            .ok_or(format!("{name} : ligne interne de {len} mm absente"))?;
        used[i] = true;
    }
    for p in &internal_points {
        if !f_ipoints.iter().any(|q| dist(*p, *q) <= TOL) {
            return Err(format!("{name} : repère interne {p:?} absent"));
        }
    }

    // ----- texte -----
    let mut want_text = vec![format!("Piece Name: {}", piece.name)];
    if let Some(c) = &piece.code {
        want_text.push(format!("Code: {c}"));
    }
    want_text.push(format!("Quantity: {}", piece.quantity));
    if let Some(m) = &piece.material {
        want_text.push(format!("Material: {m}"));
    }
    if piece.mirrored_pair {
        want_text.push("Mirrored pair: yes".to_string());
    }
    if let Some(c) = piece.category {
        // Noms écrits ici (indépendants de `PieceCategory::key`).
        let word = match c {
            pf_core::PieceCategory::Fabric => "fabric",
            pf_core::PieceCategory::Lining => "lining",
            pf_core::PieceCategory::Interfacing => "interfacing",
            pf_core::PieceCategory::Leather => "leather",
            pf_core::PieceCategory::Reinforcement => "reinforcement",
        };
        want_text.push(format!("Category: {word}"));
    }
    if let Some(c) = &piece.comment {
        // Une ligne de texte par ligne non vide du commentaire, six au plus.
        for l in c.lines().map(str::trim).filter(|l| !l.is_empty()).take(6) {
            want_text.push(format!("Comment: {l}"));
        }
    }
    let texts: Vec<&str> = on(L_TEXT)
        .map(|e| match e {
            Ent::Text { text, height, .. } if *height > 0.0 => Ok(text.as_str()),
            other => Err(format!("{name} : {other:?} sur la couche du texte")),
        })
        .collect::<Result<_, _>>()?;
    if texts != want_text.iter().map(String::as_str).collect::<Vec<_>>() {
        return Err(format!("{name} : textes {texts:?} ≠ {want_text:?}"));
    }
    done.texts += texts.len();

    // Aucune entité hors des couches connues de la table normalisée.
    for e in ents {
        if !["1", "4", "6", "7", "8", "13", "15", "84"].contains(&e.layer()) {
            return Err(format!("{name} : couche {} inattendue", e.layer()));
        }
    }
    Ok((flo, fhi))
}

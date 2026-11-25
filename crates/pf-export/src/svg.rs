//! Export SVG : `width`/`height` en mm et `viewBox` de même taille, donc
//! 1 unité utilisateur = 1 mm. L'axe Y du modèle (vers le haut) est retourné.

use crate::{num, stroke_style, ExportError, PathGeom, Scene};
use pf_core::PathRole;
use pf_geometry::PointMm;

/// Marge blanche autour du contenu, en mm.
pub const SVG_MARGIN_MM: f64 = 10.0;

fn role_id(role: PathRole) -> &'static str {
    match role {
        PathRole::Construction => "construction",
        PathRole::SewingContour => "couture",
        PathRole::CuttingContour => "coupe",
        PathRole::Grainline => "droit-fil",
        PathRole::Fold => "pliure",
        PathRole::AnnotationAxis => "axe-repere",
        PathRole::Topstitch => "surpiqure",
        PathRole::CenterFront => "milieu-devant",
        PathRole::CenterBack => "milieu-dos",
        PathRole::WaistLine => "ligne-taille",
        PathRole::HipLine => "ligne-hanche",
        PathRole::BustLine => "ligne-poitrine",
        PathRole::SkivingLine => "ligne-parage",
        PathRole::GlueLine => "ligne-collage",
    }
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Commande d'arc SVG vers `to` (déjà converti en coordonnées SVG, Y vers le bas :
/// le sens anti-horaire du modèle devient `sweep-flag = 0`).
fn svg_arc(a: &pf_geometry::Arc, to: (f64, f64)) -> String {
    let large = u8::from(a.sweep_rad.abs() > std::f64::consts::PI);
    let sweep = u8::from(!a.is_ccw());
    format!(
        "A {r} {r} 0 {large} {sweep} {} {}",
        num(to.0),
        num(to.1),
        r = num(a.radius_mm)
    )
}

/// Attribut `d` d'une chaîne fermée d'arêtes (M, L, A, C puis Z) ; `None` si la chaîne est vide.
fn cut_path_d(
    edges: &[PathGeom],
    tx: &impl Fn(PointMm) -> (f64, f64),
    closed: bool,
) -> Option<String> {
    let start = match edges.first()? {
        PathGeom::Line(a, _) => *a,
        PathGeom::Cubic(k) => k.p0,
        PathGeom::Arc(a) => a.start_point(),
        PathGeom::Circle(_) | PathGeom::Spline(_) => return None,
    };
    let s = tx(start);
    let mut d = format!("M {} {}", num(s.0), num(s.1));
    for e in edges {
        match e {
            &PathGeom::Line(_, b) => {
                let b = tx(b);
                d.push_str(&format!(" L {} {}", num(b.0), num(b.1)));
            }
            PathGeom::Cubic(k) => {
                let (c1, c2, p3) = (tx(k.c1), tx(k.c2), tx(k.p3));
                d.push_str(&format!(
                    " C {} {} {} {} {} {}",
                    num(c1.0),
                    num(c1.1),
                    num(c2.0),
                    num(c2.1),
                    num(p3.0),
                    num(p3.1)
                ));
            }
            PathGeom::Arc(a) => d.push_str(&format!(" {}", svg_arc(a, tx(a.end_point())))),
            PathGeom::Circle(_) | PathGeom::Spline(_) => {}
        }
    }
    if closed {
        d.push_str(" Z");
    }
    Some(d)
}

pub fn export_svg(scene: &Scene) -> Result<String, ExportError> {
    export_svg_lang(scene, pf_i18n::Lang::Fr)
}

/// SVG d'une scène avec le titre et la description dans la langue `lang` (US-19.2). Les clés
/// `data-role` restent celles du format (stables) ; en français, identique à `export_svg`.
pub fn export_svg_lang(scene: &Scene, lang: pf_i18n::Lang) -> Result<String, ExportError> {
    scene.check_finite()?;
    let bb = scene
        .bbox()
        .ok_or_else(|| ExportError::EmptySheet(scene.sheet_name.clone()))?;
    let m = SVG_MARGIN_MM;
    let (w, h) = (bb.width() + 2.0 * m, bb.height() + 2.0 * m);
    let tx = |p: PointMm| (p.x - bb.min.x + m, bb.max.y - p.y + m);
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" \
         viewBox=\"0 0 {} {}\">\n",
        num(w),
        num(h),
        num(w),
        num(h)
    ));
    let kind = pf_i18n::lookup(
        lang,
        if scene.is_construction_only() {
            "pdfx.svg.construction"
        } else {
            "pdfx.svg.piece"
        },
    );
    let desc = pf_i18n::lookup(lang, "pdfx.svg.desc").replace("{kind}", &kind);
    out.push_str(&format!(
        "  <title>{} — {}</title>\n  <desc>{}</desc>\n",
        esc(&scene.project_name),
        esc(&scene.sheet_name),
        esc(&desc)
    ));
    out.push_str("  <g fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\">\n");
    for path in &scene.paths {
        let st = stroke_style(path.role);
        let dash = if st.dash_mm.is_empty() {
            String::new()
        } else {
            let d: Vec<String> = st.dash_mm.iter().map(|v| num(*v)).collect();
            format!(" stroke-dasharray=\"{}\"", d.join(" "))
        };
        let sense = if path.sense == pf_core::GrainSense::Unmarked {
            String::new()
        } else {
            format!(" data-grain-sense=\"{}\"", path.sense.key())
        };
        let attrs = format!(
            "data-role=\"{}\" stroke-width=\"{}\"{dash}{sense}",
            role_id(path.role),
            num(st.width_mm)
        );
        match &path.geom {
            &PathGeom::Line(a, b) => {
                let (a_w, b_w) = (a, b);
                let (a, b) = (tx(a), tx(b));
                out.push_str(&format!(
                    "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" {attrs}/>\n",
                    num(a.0),
                    num(a.1),
                    num(b.0),
                    num(b.1)
                ));
                if path.role == PathRole::Grainline {
                    for (p, q) in crate::grain_arrow_marks(a_w, b_w, path.sense) {
                        let (p, q) = (tx(p), tx(q));
                        out.push_str(&format!(
                            "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" {attrs}/>\n",
                            num(p.0),
                            num(p.1),
                            num(q.0),
                            num(q.1)
                        ));
                    }
                }
            }
            PathGeom::Spline(segments) => {
                let s = tx(segments[0].p0);
                let mut d = format!("M {} {}", num(s.0), num(s.1));
                for c in segments {
                    let (c1, c2, p3) = (tx(c.c1), tx(c.c2), tx(c.p3));
                    d.push_str(&format!(
                        " C {} {} {} {} {} {}",
                        num(c1.0),
                        num(c1.1),
                        num(c2.0),
                        num(c2.1),
                        num(p3.0),
                        num(p3.1)
                    ));
                }
                out.push_str(&format!("    <path d=\"{d}\" {attrs}/>\n"));
            }
            PathGeom::Arc(a) => {
                let (s, e) = (tx(a.start_point()), tx(a.end_point()));
                out.push_str(&format!(
                    "    <path d=\"M {} {} {}\" {attrs}/>\n",
                    num(s.0),
                    num(s.1),
                    svg_arc(a, e)
                ));
            }
            PathGeom::Circle(c) => {
                let [h1, h2] = c.halves();
                let s = tx(h1.start_point());
                let m = tx(h1.end_point());
                let e = tx(h2.end_point());
                out.push_str(&format!(
                    "    <path d=\"M {} {} {} {} Z\" {attrs}/>\n",
                    num(s.0),
                    num(s.1),
                    svg_arc(&h1, m),
                    svg_arc(&h2, e)
                ));
            }
            PathGeom::Cubic(c) => {
                let (p0, c1, c2, p3) = (tx(c.p0), tx(c.c1), tx(c.c2), tx(c.p3));
                out.push_str(&format!(
                    "    <path d=\"M {} {} C {} {} {} {} {} {}\" {attrs}/>\n",
                    num(p0.0),
                    num(p0.1),
                    num(c1.0),
                    num(c1.1),
                    num(c2.0),
                    num(c2.1),
                    num(p3.0),
                    num(p3.1)
                ));
            }
        }
    }
    out.push_str("  </g>\n");
    // Contours de coupe calculés (US-05.4) : un chemin fermé par pièce, trait long / trait court.
    if !scene.cut_contours.is_empty() {
        let dash: Vec<String> = crate::COMPUTED_CUT_DASH_MM
            .iter()
            .map(|v| num(*v))
            .collect();
        out.push_str(&format!(
            "  <g fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\" \
             stroke-width=\"{}\" stroke-dasharray=\"{}\">\n",
            num(crate::COMPUTED_CUT_STROKE_MM),
            dash.join(" ")
        ));
        for cut in &scene.cut_contours {
            if let Some(d) = cut_path_d(&cut.edges, &tx, cut.closed) {
                out.push_str(&format!(
                    "    <path data-role=\"coupe-calculee\" data-piece=\"{}\" d=\"{d}\"/>\n",
                    esc(&cut.piece)
                ));
            }
        }
        for cut in &scene.cut_contours {
            for (p, q) in &cut.notch_marks {
                let (p, q) = (tx(*p), tx(*q));
                out.push_str(&format!(
                    "    <line data-role=\"cran-rempli\" x1=\"{}\" y1=\"{}\" x2=\"{}\" \
                     y2=\"{}\" stroke-dasharray=\"none\"/>\n",
                    num(p.0),
                    num(p.1),
                    num(q.0),
                    num(q.1)
                ));
            }
            // Crans sur la coupe (US-06.1) : rôle distinct du cran de rempli.
            for (p, q) in &cut.cut_notch_marks {
                let (p, q) = (tx(*p), tx(*q));
                out.push_str(&format!(
                    "    <line data-role=\"cran-coupe\" x1=\"{}\" y1=\"{}\" x2=\"{}\" \
                     y2=\"{}\" stroke-dasharray=\"none\"/>\n",
                    num(p.0),
                    num(p.1),
                    num(q.0),
                    num(q.1)
                ));
            }
        }
        out.push_str("  </g>\n");
    }
    if !scene.notch_marks.is_empty() {
        let st = stroke_style(PathRole::CuttingContour);
        out.push_str(&format!(
            "  <g fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\" data-role=\"cran\" \
             stroke-width=\"{}\">\n",
            num(st.width_mm)
        ));
        for (p, q) in &scene.notch_marks {
            let (p, q) = (tx(*p), tx(*q));
            out.push_str(&format!(
                "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
                num(p.0),
                num(p.1),
                num(q.0),
                num(q.1)
            ));
        }
        out.push_str("  </g>\n");
    }
    if !scene.darts.is_empty() {
        // Jambes de pince : motif de trait propre (jamais la couleur seule), rôle `pince`. La
        // ligne pliée / le capuchon ne sont pas exportés.
        let dash: Vec<String> = crate::DART_DASH_MM.iter().map(|v| num(*v)).collect();
        out.push_str(&format!(
            "  <g fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\" stroke-width=\"{}\" \
             stroke-dasharray=\"{}\">\n",
            num(crate::DART_STROKE_MM),
            dash.join(" ")
        ));
        for d in &scene.darts {
            for (p, q) in d.legs() {
                let (p, q) = (tx(p), tx(q));
                out.push_str(&format!(
                    "    <line data-role=\"pince\" data-piece=\"{}\" x1=\"{}\" y1=\"{}\" \
                     x2=\"{}\" y2=\"{}\"/>\n",
                    esc(&d.piece),
                    num(p.0),
                    num(p.1),
                    num(q.0),
                    num(q.1)
                ));
            }
        }
        out.push_str("  </g>\n");
    }
    if !scene.folds.is_empty() {
        // Bords sur le pli (PH-5b) : ligne de pliure au motif propre (jamais la couleur seule),
        // rôle `sur-le-pli`, puis libellé horizontal côté intérieur. Ni couture ni coupe.
        let dash: Vec<String> = crate::FOLD_EDGE_DASH_MM.iter().map(|v| num(*v)).collect();
        out.push_str(&format!(
            "  <g fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\" stroke-width=\"{}\" \
             stroke-dasharray=\"{}\">\n",
            num(crate::FOLD_EDGE_STROKE_MM),
            dash.join(" ")
        ));
        for f in &scene.folds {
            let (p, q) = (tx(f.from), tx(f.to));
            out.push_str(&format!(
                "    <line data-role=\"sur-le-pli\" data-piece=\"{}\" x1=\"{}\" y1=\"{}\" \
                 x2=\"{}\" y2=\"{}\"/>\n",
                esc(&f.piece),
                num(p.0),
                num(p.1),
                num(q.0),
                num(q.1)
            ));
        }
        out.push_str("  </g>\n");
        out.push_str(&format!(
            "  <g font-family=\"Helvetica, Arial, sans-serif\" font-size=\"{}\" fill=\"#000000\" \
             stroke=\"none\">\n",
            num(crate::FOLD_LABEL_TEXT_MM)
        ));
        for f in &scene.folds {
            let t = tx(f.baseline_start());
            let w = crate::fold_label_width_mm(&f.label);
            out.push_str(&format!(
                "    <text data-role=\"libelle-pli\" data-piece=\"{}\" x=\"{}\" y=\"{}\" \
                 text-anchor=\"middle\">{}</text>\n",
                esc(&f.piece),
                num(t.0 + w / 2.0),
                num(t.1),
                esc(&f.label)
            ));
        }
        out.push_str("  </g>\n");
    }
    if !scene.markings.is_empty() {
        // La légende ne change pas (octets des exemples) tant qu'aucune fente n'est exportée.
        let slot_legend = if scene.markings.iter().any(|m| m.slot().is_some()) {
            ", fente (rectangle à bouts arrondis)"
        } else {
            ""
        };
        out.push_str(&format!(
            "  <g id=\"marquages\" fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\" \
             data-role=\"marquage\" stroke-width=\"{}\">\n    <title>Marquages de pièce : \
             pointage (disque plein), perçage (cercle), croix{slot_legend}</title>\n",
            num(crate::MARKING_STROKE_MM)
        ));
        for m in &scene.markings {
            let c = tx(m.pos);
            let form = m.form_id();
            match (m.circle(), m.shape) {
                (Some((r, true)), _) => out.push_str(&format!(
                    "    <circle data-form=\"{form}\" cx=\"{}\" cy=\"{}\" r=\"{}\" \
                     fill=\"#000000\" stroke=\"none\"/>\n",
                    num(c.0),
                    num(c.1),
                    num(r)
                )),
                (Some((r, false)), _) => out.push_str(&format!(
                    "    <circle data-form=\"{form}\" data-diameter-mm=\"{}\" cx=\"{}\" \
                     cy=\"{}\" r=\"{}\"/>\n",
                    num(2.0 * r),
                    num(c.0),
                    num(c.1),
                    num(r)
                )),
                (None, pf_core::MarkingShape::Cross { size_mm }) => {
                    let mut d = String::new();
                    for (p, q) in m.cross_marks() {
                        let (p, q) = (tx(p), tx(q));
                        if !d.is_empty() {
                            d.push(' ');
                        }
                        d.push_str(&format!(
                            "M {} {} L {} {}",
                            num(p.0),
                            num(p.1),
                            num(q.0),
                            num(q.1)
                        ));
                    }
                    out.push_str(&format!(
                        "    <path data-form=\"{form}\" data-size-mm=\"{}\" d=\"{d}\"/>\n",
                        num(size_mm)
                    ));
                }
                (None, pf_core::MarkingShape::Slot { .. }) => {
                    if let Some(slot) = m.slot() {
                        // Côté, bout arrondi, côté, bout arrondi : le contour d'une fente.
                        use pf_geometry::region::EdgeGeom;
                        let mut d = String::new();
                        for (i, edge) in slot.outline().iter().enumerate() {
                            if i == 0 {
                                let s = tx(edge.start());
                                d.push_str(&format!("M {} {}", num(s.0), num(s.1)));
                            }
                            let e = tx(edge.end());
                            match edge {
                                EdgeGeom::Arc(a) => d.push_str(&format!(" {}", svg_arc(a, e))),
                                _ => d.push_str(&format!(" L {} {}", num(e.0), num(e.1))),
                            }
                        }
                        d.push_str(" Z");
                        out.push_str(&format!(
                            "    <path data-form=\"{form}\" data-length-mm=\"{}\" \
                             data-width-mm=\"{}\" data-angle-deg=\"{}\" d=\"{d}\"/>\n",
                            num(slot.length_mm),
                            num(slot.width_mm),
                            num(slot.angle_deg)
                        ));
                    }
                }
                (None, _) => {}
            }
        }
        out.push_str("  </g>\n");
    }
    if !scene.stitch_holes.is_empty() {
        out.push_str(&format!(
            "  <g id=\"trous-de-piqure\" fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\"              data-role=\"piqure\" stroke-width=\"{}\">
    <title>Trous de piqure : rond              (cercle), plat (rectangle), francais (losange), oblique (rectangle incline)</title>
",
            num(crate::MARKING_STROKE_MM)
        ));
        for m in &scene.stitch_holes {
            let form = m.form_id();
            if let Some(r) = m.circle_radius() {
                let c = tx(m.pos);
                out.push_str(&format!(
                    "    <circle data-form=\"{form}\" data-diameter-mm=\"{}\" cx=\"{}\"                      cy=\"{}\" r=\"{}\"/>
",
                    num(2.0 * r),
                    num(c.0),
                    num(c.1),
                    num(r)
                ));
            } else if let Some(v) = m.vertices() {
                let pts: Vec<String> = v
                    .iter()
                    .map(|p| {
                        let q = tx(*p);
                        format!("{} {}", num(q.0), num(q.1))
                    })
                    .collect();
                out.push_str(&format!(
                    "    <path data-form=\"{form}\" data-angle-deg=\"{}\" d=\"M {} L {} L {} L {} Z\"/>
",
                    num(m.tangent_deg),
                    pts[0],
                    pts[1],
                    pts[2],
                    pts[3]
                ));
            }
        }
        out.push_str(
            "  </g>
",
        );
    }
    if !scene.zones.is_empty() {
        // Zones (US-06.10) : un groupe par zone (type, largeur saisie), contour + motif + étiquette
        // texte ; la couleur ne porte jamais seule le type.
        out.push_str(&format!(
            "  <g id=\"zones\" fill=\"none\" stroke-linecap=\"round\" data-role=\"zone\" \
             stroke-width=\"{}\">\n    <title>Zones : parage (hachures), collage (points), \
             decoupe interne (croisillons)</title>\n",
            num(crate::zone::ZONE_STROKE_MM)
        ));
        for z in &scene.zones {
            let pat = z.pattern();
            let pts: Vec<String> = z
                .polygon
                .iter()
                .map(|p| {
                    let q = tx(*p);
                    format!("{} {}", num(q.0), num(q.1))
                })
                .collect();
            let width = z
                .width_mm
                .map(|w| format!(" data-width-mm=\"{}\"", num(w)))
                .unwrap_or_default();
            out.push_str(&format!(
                "    <g data-zone=\"{}\"{width} stroke=\"{}\">\n",
                z.id(),
                z.color()
            ));
            let dash = if z.dash().is_empty() {
                String::new()
            } else {
                let v: Vec<String> = z.dash().iter().map(|d| num(*d)).collect();
                format!(" stroke-dasharray=\"{}\"", v.join(" "))
            };
            out.push_str(&format!(
                "      <path data-part=\"contour\"{dash} d=\"M {} Z\"/>\n",
                pts.join(" L ")
            ));
            if !pat.hatch.is_empty() {
                let mut d = String::new();
                for (p, q) in &pat.hatch {
                    let (p, q) = (tx(*p), tx(*q));
                    d.push_str(&format!(
                        "M {} {} L {} {} ",
                        num(p.0),
                        num(p.1),
                        num(q.0),
                        num(q.1)
                    ));
                }
                out.push_str(&format!(
                    "      <path data-part=\"motif\" stroke-width=\"{}\" d=\"{}\"/>\n",
                    num(crate::zone::ZONE_HATCH_MM),
                    d.trim_end()
                ));
            }
            for p in &pat.dots {
                let c = tx(*p);
                out.push_str(&format!(
                    "      <circle data-part=\"motif\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" \
                     stroke=\"none\"/>\n",
                    num(c.0),
                    num(c.1),
                    num(crate::zone::ZONE_DOT_RADIUS_MM),
                    z.color()
                ));
            }
            if let Some(at) = pat.label_at {
                let t = tx(at);
                out.push_str(&format!(
                    "      <text data-part=\"etiquette\" x=\"{}\" y=\"{}\" text-anchor=\"middle\" \
                     font-family=\"Helvetica, Arial, sans-serif\" font-size=\"{}\" fill=\"{}\" \
                     stroke=\"none\">{}</text>\n",
                    num(t.0),
                    num(t.1),
                    num(crate::zone::ZONE_LABEL_MM),
                    z.color(),
                    z.label()
                ));
            }
            out.push_str("    </g>\n");
        }
        out.push_str("  </g>\n");
    }
    if !scene.buttons.is_empty() {
        out.push_str(&format!(
            "  <g id=\"boutons\" fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\" \
             data-role=\"bouton\" stroke-width=\"{}\">\n    <title>Boutons et boutonnières : \
             bouton (cercle), boutonnière (fente à bouts arrondis)</title>\n",
            num(crate::BUTTON_STROKE_MM)
        ));
        for b in &scene.buttons {
            let form = b.form_id();
            match (b.circle_radius(), b.slot()) {
                (Some(r), _) => {
                    let c = tx(b.pos);
                    out.push_str(&format!(
                        "    <circle data-form=\"{form}\" data-diameter-mm=\"{}\" cx=\"{}\" \
                         cy=\"{}\" r=\"{}\"/>\n",
                        num(2.0 * r),
                        num(c.0),
                        num(c.1),
                        num(r)
                    ));
                }
                (None, Some(slot)) => {
                    // Côté, bout arrondi, côté, bout arrondi : le contour d'une fente.
                    use pf_geometry::region::EdgeGeom;
                    let mut d = String::new();
                    for (i, edge) in slot.outline().iter().enumerate() {
                        if i == 0 {
                            let s = tx(edge.start());
                            d.push_str(&format!("M {} {}", num(s.0), num(s.1)));
                        }
                        let e = tx(edge.end());
                        match edge {
                            EdgeGeom::Arc(a) => d.push_str(&format!(" {}", svg_arc(a, e))),
                            _ => d.push_str(&format!(" L {} {}", num(e.0), num(e.1))),
                        }
                    }
                    d.push_str(" Z");
                    out.push_str(&format!(
                        "    <path data-form=\"{form}\" data-length-mm=\"{}\" \
                         data-width-mm=\"{}\" data-angle-deg=\"{}\" d=\"{d}\"/>\n",
                        num(slot.length_mm),
                        num(slot.width_mm),
                        num(slot.angle_deg)
                    ));
                }
                (None, None) => {}
            }
        }
        out.push_str("  </g>\n");
    }
    if !scene.dimensions.is_empty() {
        out.push_str(
            "  <g fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\" data-role=\"cotation\" \
             stroke-width=\"0.2\">\n",
        );
        for d in &scene.dimensions {
            for (p, q) in d.marks() {
                let (p, q) = (tx(p), tx(q));
                out.push_str(&format!(
                    "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
                    num(p.0),
                    num(p.1),
                    num(q.0),
                    num(q.1)
                ));
            }
            let t = tx(d.text_anchor());
            out.push_str(&format!(
                "    <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-family=\"Helvetica, Arial, \
                 sans-serif\" font-size=\"{}\" fill=\"#000000\" stroke=\"none\">{}</text>\n",
                num(t.0),
                num(t.1),
                num(crate::DIMENSION_TEXT_MM),
                esc(&d.text)
            ));
        }
        out.push_str("  </g>\n");
    }
    if !scene.full_dimensions.is_empty() {
        // Cotes complètes (US-06.7) : une cote = un groupe (type, valeur) avec ses traits et
        // son texte réel, orienté le long de la ligne (rotation autour du début de la ligne de
        // base ; le sens trigonométrique du modèle devient `rotate(-angle)` Y vers le bas).
        out.push_str(
            "  <g fill=\"none\" stroke=\"#000000\" stroke-linecap=\"round\" data-role=\"cote\" \
             stroke-width=\"0.2\">\n",
        );
        for d in &scene.full_dimensions {
            out.push_str(&format!(
                "    <g data-kind=\"{}\" data-value=\"{}\">\n",
                d.kind,
                num(d.value)
            ));
            for (p, q) in &d.marks {
                let (p, q) = (tx(*p), tx(*q));
                out.push_str(&format!(
                    "      <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
                    num(p.0),
                    num(p.1),
                    num(q.0),
                    num(q.1)
                ));
            }
            let t = tx(d.baseline_center());
            out.push_str(&format!(
                "      <text x=\"{x}\" y=\"{y}\" text-anchor=\"middle\" transform=\"rotate({} {x} {y})\" \
                 font-family=\"Helvetica, Arial, sans-serif\" font-size=\"{}\" fill=\"#000000\" \
                 stroke=\"none\">{}</text>\n",
                num(-d.text_angle_deg),
                num(crate::DIMENSION_TEXT_MM),
                esc(&d.text),
                x = num(t.0),
                y = num(t.1),
            ));
            out.push_str("    </g>\n");
        }
        out.push_str("  </g>\n");
    }
    if !scene.piece_labels.is_empty() {
        // Étiquettes de pièce : une ligne de texte par `<text>`, centrée sur le bloc ; les
        // accents du français sont écrits tels quels (UTF-8).
        out.push_str(&format!(
            "  <g font-family=\"Helvetica, Arial, sans-serif\" font-size=\"{}\" fill=\"#000000\" \
             stroke=\"none\">\n",
            num(crate::LABEL_TEXT_MM)
        ));
        for l in &scene.piece_labels {
            for (i, line) in l.lines.iter().enumerate() {
                let t = tx(l.baseline_center(i));
                out.push_str(&format!(
                    "    <text data-role=\"etiquette-piece\" data-piece=\"{}\" x=\"{}\" y=\"{}\" \
                     text-anchor=\"middle\">{}</text>\n",
                    esc(&l.piece),
                    num(t.0),
                    num(t.1),
                    esc(line)
                ));
            }
        }
        out.push_str("  </g>\n");
    }
    if !scene.fold_overs.is_empty() {
        // Libellés « Rempli 8 mm » (PH-5c) : texte horizontal côté intérieur, rôle `libelle-rempli`.
        out.push_str(&format!(
            "  <g font-family=\"Helvetica, Arial, sans-serif\" font-size=\"{}\" fill=\"#000000\" \
             stroke=\"none\">\n",
            num(crate::FOLD_LABEL_TEXT_MM)
        ));
        for f in &scene.fold_overs {
            let t = tx(f.baseline_start());
            let w = crate::fold_label_width_mm(&f.label);
            out.push_str(&format!(
                "    <text data-role=\"libelle-rempli\" data-piece=\"{}\" x=\"{}\" y=\"{}\" \
                 text-anchor=\"middle\">{}</text>\n",
                esc(&f.piece),
                num(t.0 + w / 2.0),
                num(t.1),
                esc(&f.label)
            ));
        }
        out.push_str("  </g>\n");
    }
    if !scene.points.is_empty() {
        out.push_str(
            "  <g font-family=\"Helvetica, Arial, sans-serif\" font-size=\"3\" fill=\"#000000\">\n",
        );
        for p in &scene.points {
            let (x, y) = tx(p.pos);
            out.push_str(&format!(
                "    <circle cx=\"{}\" cy=\"{}\" r=\"0.6\"/>\n",
                num(x),
                num(y)
            ));
            if p.show_label {
                out.push_str(&format!(
                    "    <text x=\"{}\" y=\"{}\">{}</text>\n",
                    num(x + 1.5),
                    num(y - 1.5),
                    esc(&p.label)
                ));
            }
        }
        out.push_str("  </g>\n");
    }
    out.push_str("</svg>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;

    fn attr(tag: &str, name: &str) -> f64 {
        let key = format!("{name}=\"");
        let start = tag
            .find(&key)
            .unwrap_or_else(|| panic!("{name} absent de {tag}"))
            + key.len();
        let end = start + tag[start..].find('"').unwrap();
        tag[start..end].trim_end_matches("mm").parse().unwrap()
    }

    #[test]
    fn etalon_svg_en_millimetres() {
        let svg = export_svg(&Scene::calibration_square()).unwrap();
        let root = svg.lines().find(|l| l.starts_with("<svg")).unwrap();
        assert!(
            root.contains("width=\"120mm\"") && root.contains("height=\"120mm\""),
            "{root}"
        );
        assert!(root.contains("viewBox=\"0 0 120 120\""), "{root}");
        let lines: Vec<&str> = svg.lines().filter(|l| l.contains("<line")).collect();
        assert_eq!(lines.len(), 4);
        for l in lines {
            let (dx, dy) = (attr(l, "x2") - attr(l, "x1"), attr(l, "y2") - attr(l, "y1"));
            assert!((dx.hypot(dy) - 100.0).abs() < 1e-9, "côté ≠ 100 mm : {l}");
            assert!(l.contains("data-role=\"coupe\""));
        }
    }

    #[test]
    fn exercice_de_reference_cotes_conservees() {
        let project = pf_core::demo_project().unwrap();
        let scene = Scene::from_sheet(&project, project.sheets[0].id).unwrap();
        let svg = export_svg(&scene).unwrap();
        let lens: Vec<f64> = svg
            .lines()
            .filter(|l| l.contains("<line"))
            .map(|l| (attr(l, "x2") - attr(l, "x1")).hypot(attr(l, "y2") - attr(l, "y1")))
            .collect();
        assert_eq!(lens.len(), 2);
        assert!(
            (lens[0] - 120.0).abs() < 1e-9 && (lens[1] - 80.0).abs() < 1e-9,
            "{lens:?}"
        );
        assert!(svg.contains("<path d=\"M "));
        assert!(svg.contains("tracé de construction"));
        // Y retourné : B (120, 0) est en bas, C (120, 80) en haut.
        let bc = svg.lines().filter(|l| l.contains("<line")).nth(1).unwrap();
        assert!(attr(bc, "y1") > attr(bc, "y2"));
    }

    #[test]
    fn caracteres_speciaux_echappes() {
        let mut scene = Scene::calibration_square();
        scene.sheet_name = "Poche <A&B>".into();
        let svg = export_svg(&scene).unwrap();
        assert!(svg.contains("Poche &lt;A&amp;B&gt;"));
    }

    #[test]
    fn titre_et_description_localises_francais_inchange() {
        let fr = export_svg(&Scene::calibration_square()).unwrap();
        assert!(fr.contains(
            "<title>PatternForge — Carré étalon 100 mm</title>\n  <desc>PatternForge. Unités : \
             1 unité = 1 mm, échelle 1:1. Contenu : pièce avec ligne de coupe.</desc>\n"
        ));
        let fr_scene = Scene::calibration_square_lang(pf_i18n::Lang::Fr);
        assert_eq!(fr, export_svg_lang(&fr_scene, pf_i18n::Lang::Fr).unwrap());
        let en_scene = Scene::calibration_square_lang(pf_i18n::Lang::En);
        let en = export_svg_lang(&en_scene, pf_i18n::Lang::En).unwrap();
        assert!(en.contains(
            "<title>PatternForge — 100 mm calibration square</title>\n  <desc>PatternForge. \
             Units: 1 unit = 1 mm, scale 1:1. Content: piece with a cutting line.</desc>\n"
        ));
        let mut sketch = en_scene.clone();
        for p in &mut sketch.paths {
            p.role = PathRole::Construction;
        }
        let en2 = export_svg_lang(&sketch, pf_i18n::Lang::En).unwrap();
        assert!(en2.contains("Content: construction drawing, no cutting line.</desc>"));
        assert_eq!(
            fr.matches("data-role=\"coupe\"").count(),
            en.matches("data-role=\"coupe\"").count()
        );
        assert_eq!(fr.matches("data-role=\"coupe\"").count(), 4);
    }
}

//! US-06.7 — export des cotes complètes (longueur développée, angle, rayon, diamètre,
//! projections) : lignes et **texte réel orienté le long de la ligne**, jamais à l'envers ;
//! légende avec accents. Les fichiers sont relus par des lecteurs indépendants du code
//! d'export. Le PDF embarque un sous-ensemble de Noto Sans (US-06.6) : le texte est relu par le
//! `ToUnicode` du fichier ; un caractère absent de la police est remplacé par « ? » ; les
//! largeurs de centrage sont celles de la police, lues dans le fichier de police du dépôt ; le
//! SVG garde tout caractère Unicode.

#[path = "common/pdf_text.rs"]
mod pdf_text;

use pf_core::{Command, FullDimensionKind, PointRef, Project, SheetId};
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{Scene, DIMENSION_TEXT_MM, MM_TO_PT};
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

struct Ids {
    sheet: SheetId,
}

/// Une cote de chaque type, dont deux textes obliques (montant et descendant) et une légende
/// accentuée.
fn project(with_cotes: bool) -> (Project, Ids) {
    let mut p = Project::new("Cotes export");
    let s = p.sheets[0].id;
    let seg = |p: &mut Project, a: PointMm, b: PointMm| {
        let o = p
            .apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(a),
                to: PointRef::New(b),
            })
            .unwrap();
        (o.entities[0], o.points[0], o.points[1])
    };
    let (_, a, b) = seg(&mut p, pt(0., 0.), pt(100., 0.));
    let (up, ..) = seg(&mut p, pt(0., 60.), pt(30., 100.));
    let (down, ..) = seg(&mut p, pt(200., 60.), pt(170., 100.));
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(200., 200.)),
            radius_mm: 12.5,
        })
        .unwrap()
        .entities[0];
    let v = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(300., 0.),
        })
        .unwrap()
        .points[0];
    let e = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(350., 0.),
        })
        .unwrap()
        .points[0];
    let n = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(300., 50.),
        })
        .unwrap()
        .points[0];
    if with_cotes {
        let mut cote = |kind: FullDimensionKind, label: Option<&str>| {
            p.apply(Command::AddFullDimension {
                sheet: s,
                kind,
                label: label.map(String::from),
                text_shift_mm: 0.0,
            })
            .unwrap();
        };
        cote(
            FullDimensionKind::Horizontal {
                from: a,
                to: b,
                offset_mm: 10.0,
            },
            Some("Épaule — côté œil"),
        );
        cote(
            FullDimensionKind::PathLength {
                entity: up,
                offset_mm: 5.0,
            },
            None,
        );
        cote(
            FullDimensionKind::PathLength {
                entity: down,
                offset_mm: 5.0,
            },
            Some("A&B <1>"),
        );
        cote(
            FullDimensionKind::Radius {
                entity: circle,
                angle_deg: 30.0,
            },
            None,
        );
        cote(
            FullDimensionKind::Diameter {
                entity: circle,
                angle_deg: 120.0,
            },
            None,
        );
        cote(
            FullDimensionKind::Angle {
                vertex: v,
                first: e,
                second: n,
                radius_mm: 20.0,
            },
            Some("Pince"),
        );
        cote(
            FullDimensionKind::Vertical {
                from: n,
                to: v,
                offset_mm: -8.0,
            },
            None,
        );
    }
    (p, Ids { sheet: s })
}

fn scene(with_cotes: bool) -> Scene {
    let (p, ids) = project(with_cotes);
    Scene::from_sheet(&p, ids.sheet).unwrap()
}

// --- lecteur SVG indépendant -------------------------------------------------------

fn attr(tag: &str, name: &str) -> String {
    let key = format!(" {name}=\"");
    let start = tag
        .find(&key)
        .unwrap_or_else(|| panic!("{name} absent de {tag}"))
        + key.len();
    let end = start + tag[start..].find('"').unwrap();
    tag[start..end].to_string()
}

struct SvgCote {
    kind: String,
    value: f64,
    lines: Vec<((f64, f64), (f64, f64))>,
    text: String,
    text_xy: (f64, f64),
    rotate_deg: f64,
}

fn svg_cotes(svg: &str) -> Vec<SvgCote> {
    let (_, after) = svg
        .split_once("data-role=\"cote\"")
        .expect("groupe des cotes");
    let mut out = Vec::new();
    for block in after.split("<g data-kind=\"").skip(1) {
        let block = block.split("</g>").next().unwrap();
        let kind = block.split('"').next().unwrap().to_string();
        let value: f64 = {
            let rest = block.split_once("data-value=\"").unwrap().1;
            rest.split('"').next().unwrap().parse().unwrap()
        };
        let mut lines = Vec::new();
        for l in block.split("<line").skip(1) {
            let l = l.split("/>").next().unwrap();
            let f = |n: &str| attr(l, n).parse::<f64>().unwrap();
            lines.push(((f("x1"), f("y1")), (f("x2"), f("y2"))));
        }
        let t = block.split("<text").nth(1).expect("texte réel");
        let head = t.split('>').next().unwrap();
        let text = t
            .split_once('>')
            .unwrap()
            .1
            .split("</text>")
            .next()
            .unwrap()
            .to_string();
        let rotate = attr(head, "transform");
        let nums: Vec<f64> = rotate
            .trim_start_matches("rotate(")
            .trim_end_matches(')')
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        out.push(SvgCote {
            kind,
            value,
            lines,
            text,
            text_xy: (
                attr(head, "x").parse().unwrap(),
                attr(head, "y").parse().unwrap(),
            ),
            rotate_deg: nums[0],
        });
    }
    out
}

fn len(l: &((f64, f64), (f64, f64))) -> f64 {
    (l.1 .0 - l.0 .0).hypot(l.1 .1 - l.0 .1)
}

#[test]
fn svg_texte_reel_oriente_legende_accentuee_et_traits_mesures() {
    let sc = scene(true);
    assert_eq!(sc.full_dimensions.len(), 7);
    let svg = export_svg(&sc).unwrap();
    let cotes = svg_cotes(&svg);
    assert_eq!(cotes.len(), 7);
    let kinds: Vec<&str> = cotes.iter().map(|c| c.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "horizontal",
            "path_length",
            "path_length",
            "radius",
            "diameter",
            "angle",
            "vertical"
        ]
    );
    // Horizontale : texte accentué intact, échappement XML, ligne de cote de 100 mm.
    let h = &cotes[0];
    assert_eq!(h.text, "Épaule — côté œil : 100.00 mm");
    assert!(h.rotate_deg.abs() < 1e-9);
    assert!(
        h.lines.iter().any(|l| (len(l) - 100.0).abs() < 1e-3),
        "ligne de cote de 100 mm"
    );
    assert!((h.value - 100.0).abs() < 1e-9);
    assert_eq!(cotes[2].text, "A&amp;B &lt;1&gt; : 50.00 mm");
    // Segment montant (3-4-5 × 10) : direction 53,13° ; descendant vers la gauche : 126,87°
    // ramené à −53,13° (jamais à l'envers). La rotation SVG est de signe opposé (Y vers le bas).
    let a = (4.0f64 / 3.0).atan().to_degrees();
    assert!(
        (cotes[1].rotate_deg + a).abs() < 1e-3,
        "{}",
        cotes[1].rotate_deg
    );
    assert!(
        (cotes[2].rotate_deg - a).abs() < 1e-3,
        "{}",
        cotes[2].rotate_deg
    );
    assert!(cotes[1].text == "50.00 mm");
    // Rayon, diamètre, angle, verticale.
    assert_eq!(cotes[3].text, "R 12.50 mm");
    assert_eq!(cotes[4].text, "Ø 25.00 mm");
    assert_eq!(cotes[5].text, "Pince : 90.00°");
    assert_eq!(cotes[6].text, "50.00 mm");
    // Verticale : le texte monte (90° dans le modèle, −90° en SVG).
    assert!((cotes[6].rotate_deg + 90.0).abs() < 1e-9);
    // Diamètre : trait de 25 mm.
    assert!(cotes[4].lines.iter().any(|l| (len(l) - 25.0).abs() < 1e-3));
    // Rayon : trait de 12,5 mm.
    assert!(cotes[3].lines.iter().any(|l| (len(l) - 12.5).abs() < 1e-3));
    // Position : le texte est juste au-dessus de la ligne de cote horizontale, centré dessus.
    let line = h
        .lines
        .iter()
        .find(|l| (len(l) - 100.0).abs() < 1e-3)
        .unwrap();
    let mid_x = (line.0 .0 + line.1 .0) / 2.0;
    assert!((h.text_xy.0 - mid_x).abs() < 1e-3);
    let lift = pf_core::full_dimension::TEXT_LIFT_MM - 0.35 * DIMENSION_TEXT_MM;
    // SVG : Y vers le bas, la ligne de base est `lift` mm au-dessus de la ligne de cote.
    assert!(
        (line.0 .1 - h.text_xy.1 - lift).abs() < 1e-3,
        "{} {}",
        line.0 .1,
        h.text_xy.1
    );
    // Les cotations d'US-02 et les autres groupes ne sont pas affectés.
    assert!(!svg.contains("data-role=\"cotation\""));
}

#[test]
fn svg_garde_tout_caractere_unicode() {
    let (mut p, ids) = project(true);
    let d = p.sheet(ids.sheet).unwrap().full_dimensions[0].id;
    p.apply(Command::SetFullDimensionLabel {
        sheet: ids.sheet,
        dimension: d,
        label: Some("Łódź 日本".into()),
    })
    .unwrap();
    let sc = Scene::from_sheet(&p, ids.sheet).unwrap();
    let svg = export_svg(&sc).unwrap();
    assert!(svg.contains(">Łódź 日本 : 100.00 mm</text>"));
}

// --- lecteur PDF indépendant -------------------------------------------------------

fn page_streams(bytes: &[u8]) -> Vec<String> {
    let readable = pdf_text::readable(bytes);
    let text = readable.as_str();
    let mut pages = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        pages.push(body[..end].to_string());
        rest = &body[end + 9..];
    }
    pages
}

struct PdfText {
    angle_deg: f64,
    origin: (f64, f64),
    size_pt: f64,
    raw: String,
}

/// Textes à matrice `Tm` : `BT /F1 s Tf 0 g a b c d e f Tm (raw) Tj ET`.
fn pdf_texts(stream: &str) -> Vec<PdfText> {
    let mut out = Vec::new();
    for op in stream.split("BT ").skip(1) {
        let op = op.split(" ET").next().unwrap();
        if !op.contains(" Tm ") {
            continue;
        }
        let (head, rest) = op.split_once(" Tm ").unwrap();
        let toks: Vec<&str> = head.split_whitespace().collect();
        let n = toks.len();
        let v: Vec<f64> = toks[n - 6..].iter().map(|t| t.parse().unwrap()).collect();
        let size_pt: f64 = toks[toks.iter().position(|t| *t == "Tf").unwrap() - 1]
            .parse()
            .unwrap();
        let raw = rest
            .strip_prefix('(')
            .unwrap()
            .rsplit_once(") Tj")
            .unwrap()
            .0
            .to_string();
        // Matrice de rotation pure : (cos, sin, −sin, cos).
        assert!(
            (v[0] - v[3]).abs() < 1e-9 && (v[1] + v[2]).abs() < 1e-9,
            "{v:?}"
        );
        out.push(PdfText {
            angle_deg: v[1].atan2(v[0]).to_degrees(),
            origin: (v[4] / MM_TO_PT, v[5] / MM_TO_PT),
            size_pt,
            raw,
        });
    }
    out
}

/// Segments `x y m x y l S` en mm.
fn pdf_lines(stream: &str) -> Vec<((f64, f64), (f64, f64))> {
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
    let toks: Vec<&str> = cleaned.split_whitespace().collect();
    let mut out = Vec::new();
    for i in 0..toks.len() {
        if toks[i] == "l" && i >= 5 && toks[i - 3] == "m" && toks.get(i + 1) == Some(&"S") {
            let f = |k: usize| toks[k].parse::<f64>().unwrap() / MM_TO_PT;
            out.push(((f(i - 5), f(i - 4)), (f(i - 2), f(i - 1))));
        }
    }
    out
}

#[test]
fn pdf_texte_reel_oriente_positions_mesurees_et_accents_fideles() {
    let sc = scene(true);
    let pdf = export_pdf(std::slice::from_ref(&sc)).unwrap();
    let pages: Vec<String> = page_streams(&pdf.bytes);
    // Toutes les cotes tiennent sur une seule fenêtre ? Sinon on les rassemble page par page.
    // Chaque fenêtre de la découpe porte les sept textes (la page de fenêtre les rogne) :
    // on lit la première page qui en a.
    let all_texts: Vec<PdfText> = pages
        .iter()
        .map(|p| pdf_texts(p))
        .find(|t| !t.is_empty())
        .unwrap();
    assert_eq!(all_texts.len(), 7, "{} textes", all_texts.len());
    // Texte relu par le `ToUnicode` du fichier : accents, tiret cadratin, Ø et ° fidèles.
    let raws: Vec<&str> = all_texts.iter().map(|t| t.raw.as_str()).collect();
    assert_eq!(raws[0], "Épaule — côté œil : 100.00 mm");
    assert_eq!(raws[1], "50.00 mm");
    assert_eq!(raws[2], "A&B <1> : 50.00 mm");
    assert_eq!(raws[3], "R 12.50 mm");
    assert_eq!(raws[4], "Ø 25.00 mm");
    assert_eq!(raws[5], "Pince : 90.00°");
    for t in &all_texts {
        assert!((t.size_pt - DIMENSION_TEXT_MM * MM_TO_PT).abs() < 1e-3);
        assert!(
            t.angle_deg > -90.0 - 1e-9 && t.angle_deg <= 90.0 + 1e-9,
            "{}",
            t.angle_deg
        );
    }
    // Angles : 0°, ±53,13°, verticale 90°.
    let a = (4.0f64 / 3.0).atan().to_degrees();
    assert!(all_texts[0].angle_deg.abs() < 1e-6);
    assert!(
        (all_texts[1].angle_deg - a).abs() < 1e-2,
        "{}",
        all_texts[1].angle_deg
    );
    assert!(
        (all_texts[2].angle_deg + a).abs() < 1e-2,
        "{}",
        all_texts[2].angle_deg
    );
    assert!((all_texts[6].angle_deg - 90.0).abs() < 1e-6);

    // Positions mesurées : le milieu de la ligne de base du texte horizontal est à
    // (milieu de la ligne de cote ; ordonnée de la ligne + remontée − 0,35 corps).
    let stream = pages.iter().find(|p| !pdf_texts(p).is_empty()).unwrap();
    let lines = pdf_lines(stream);
    let len = |l: &((f64, f64), (f64, f64))| (l.1 .0 - l.0 .0).hypot(l.1 .1 - l.0 .1);
    // Les lignes de 100 mm : le segment dessiné et la ligne de cote (la plus haute).
    let long: Vec<_> = lines
        .iter()
        .filter(|l| (len(l) - 100.0).abs() < 1e-2)
        .collect();
    assert!(long.len() >= 2);
    let cote = long
        .iter()
        .max_by(|a, b| a.0 .1.total_cmp(&b.0 .1))
        .unwrap();
    let mid = ((cote.0 .0 + cote.1 .0) / 2.0, cote.0 .1);
    // Largeurs réelles : somme des largeurs d'avance de la police (lue sur disque).
    let font = pdf_text::repo_font();
    let width = |s: &str| font.width_mm(s, DIMENSION_TEXT_MM * MM_TO_PT);
    // Valeur écrite à la main : « 50.00 mm » = 4 × 572 (chiffres) + 268 (point) + 260 (espace)
    // + 2 × 935 (m) = 4686 / 1000 em × 3 mm.
    assert!((width("50.00 mm") - 4.686 * 3.0).abs() < 1e-9);
    let w = width("Épaule — côté œil : 100.00 mm");
    let t = &all_texts[0];
    let lift = pf_core::full_dimension::TEXT_LIFT_MM - 0.35 * DIMENSION_TEXT_MM;
    assert!(
        (t.origin.0 + w / 2.0 - mid.0).abs() < 1e-2,
        "{:?} {mid:?}",
        t.origin
    );
    assert!(
        (t.origin.1 - (mid.1 + lift)).abs() < 1e-2,
        "{:?} {mid:?}",
        t.origin
    );
    // Texte incliné (53,13°) : milieu de base = milieu de la ligne de cote + lift × normale
    // gauche (−sin, cos).
    let t1 = &all_texts[1];
    let (s1, c1) = a.to_radians().sin_cos();
    let candidates: Vec<_> = lines
        .iter()
        .filter(|l| (len(l) - 50.0).abs() < 1e-2)
        .collect();
    let line1 = candidates
        .iter()
        .max_by(|p, q| {
            let m = |l: &&&((f64, f64), (f64, f64))| {
                ((l.0 .0 + l.1 .0) / 2.0) * -s1 + ((l.0 .1 + l.1 .1) / 2.0) * c1
            };
            m(p).total_cmp(&m(q))
        })
        .unwrap();
    let mid1 = (
        (line1.0 .0 + line1.1 .0) / 2.0,
        (line1.0 .1 + line1.1 .1) / 2.0,
    );
    let w1 = width("50.00 mm");
    let base = (t1.origin.0 + c1 * w1 / 2.0, t1.origin.1 + s1 * w1 / 2.0);
    assert!(
        (base.0 - (mid1.0 - s1 * lift)).abs() < 1e-2,
        "{base:?} {mid1:?}"
    );
    assert!(
        (base.1 - (mid1.1 + c1 * lift)).abs() < 1e-2,
        "{base:?} {mid1:?}"
    );
    // Rayon : le trait part du centre et fait 12,5 mm ; diamètre : un trait de 25 mm.
    assert!(lines.iter().any(|l| (len(l) - 12.5).abs() < 1e-2));
    assert!(lines.iter().any(|l| (len(l) - 25.0).abs() < 1e-2));
}

#[test]
fn pdf_latin_etendu_fidele_et_caractere_absent_remplace_par_point_d_interrogation() {
    let (mut p, ids) = project(true);
    let d = p.sheet(ids.sheet).unwrap().full_dimensions[0].id;
    let first_text = |p: &Project, ids: &Ids| -> String {
        let sc = Scene::from_sheet(p, ids.sheet).unwrap();
        let pdf = export_pdf(&[sc]).unwrap();
        let texts: Vec<PdfText> = page_streams(&pdf.bytes)
            .iter()
            .map(|s| pdf_texts(s))
            .find(|t| !t.is_empty())
            .unwrap();
        texts[0].raw.clone()
    };
    p.apply(Command::SetFullDimensionLabel {
        sheet: ids.sheet,
        dimension: d,
        label: Some("Łódź".into()),
    })
    .unwrap();
    // Ł et ź (Latin étendu A) sont dans la police embarquée : fidèles.
    assert_eq!(first_text(&p, &ids), "Łódź : 100.00 mm");
    p.apply(Command::SetFullDimensionLabel {
        sheet: ids.sheet,
        dimension: d,
        label: Some("Ж日".into()),
    })
    .unwrap();
    // Absents de la police : « ? » documenté, sans panic.
    assert_eq!(first_text(&p, &ids), "?? : 100.00 mm");
}

#[test]
fn sans_cote_complete_aucun_octet_ajoute() {
    let without = scene(false);
    assert!(without.full_dimensions.is_empty());
    let svg = export_svg(&without).unwrap();
    assert!(!svg.contains("data-role=\"cote\"") && !svg.contains("data-kind"));
    let pdf = export_pdf(std::slice::from_ref(&without)).unwrap();
    assert!(!page_streams(&pdf.bytes).iter().any(|s| s.contains(" Tm ")));
    // La même scène avec cotes ne change pas le reste : mêmes tracés.
    let with = scene(true);
    assert_eq!(with.paths, without.paths);
    assert_eq!(with.points, without.points);
    // Exports déterministes.
    assert_eq!(svg, export_svg(&without).unwrap());
    assert_eq!(
        export_pdf(std::slice::from_ref(&with)).unwrap().bytes,
        export_pdf(std::slice::from_ref(&with)).unwrap().bytes
    );
}

#[test]
fn calque_masque_et_cote_non_calculable_ne_sont_pas_exportes() {
    let (mut p, ids) = project(true);
    let s = ids.sheet;
    // Cotes non calculables : le point `second` se superpose au côté de l'angle (0°) et se
    // retrouve à la même ordonnée que le sommet (projection verticale nulle).
    let angle = p
        .sheet(s)
        .unwrap()
        .full_dimensions
        .iter()
        .find(|d| matches!(d.kind, FullDimensionKind::Angle { .. }))
        .unwrap()
        .kind;
    let FullDimensionKind::Angle { second, .. } = angle else {
        unreachable!()
    };
    p.apply(Command::MovePoint {
        sheet: s,
        point: second,
        to: pt(380., 0.),
    })
    .unwrap();
    let sc = Scene::from_sheet(&p, s).unwrap();
    assert_eq!(sc.full_dimensions.len(), 5);
    // Calque masqué : les deux cotes du cercle disparaissent avec lui.
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Cercle".into(),
        })
        .unwrap()
        .layers[0];
    let circle = p
        .sheet(s)
        .unwrap()
        .entities
        .iter()
        .find(|e| {
            e.kind.all_points().len() == 1 && matches!(e.kind, pf_core::EntityKind::Circle { .. })
        })
        .unwrap()
        .id;
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![circle],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer,
        visible: false,
    })
    .unwrap();
    let sc = Scene::from_sheet(&p, s).unwrap();
    assert_eq!(sc.full_dimensions.len(), 3);
}

#[test]
fn l_emprise_inclut_le_texte_des_cotes() {
    let (mut p, ids) = project(true);
    let s = ids.sheet;
    let d = p.sheet(s).unwrap().full_dimensions[0].id;
    // Texte décalé loin à droite de la ligne : l'emprise s'agrandit, rien n'est coupé.
    let before = Scene::from_sheet(&p, s).unwrap().bbox().unwrap();
    p.apply(Command::SetFullDimensionTextShift {
        sheet: s,
        dimension: d,
        text_shift_mm: 900.0,
    })
    .unwrap();
    let after = Scene::from_sheet(&p, s).unwrap().bbox().unwrap();
    assert!(
        after.max.x > before.max.x + 100.0,
        "{} {}",
        after.max.x,
        before.max.x
    );
    let sc = Scene::from_sheet(&p, s).unwrap();
    for c in sc.full_dimensions[0].text_corners() {
        assert!(after.contains(c));
    }
}

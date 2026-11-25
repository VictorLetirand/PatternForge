//! Outils des tests du contenu de la sortie (US-13.4) : projets variés et lecteurs de SVG et de
//! PDF **indépendants** du code d'export (aucune fonction de `pf_export` n'y est réutilisée pour
//! lire ; les rôles et leurs identifiants SVG sont réécrits ici).
#![allow(dead_code)]

use super::{pt, Rng};
use pf_core::{
    ButtonElement, Command, FullDimensionKind, GrainSense, MarkingAnchor, MarkingShape, NotchStyle,
    PathRole, PieceGrain, PieceSource, PointRef, Project, SheetId, TopstitchSide, TopstitchSource,
};
use pf_export::OutputCategory;
use pf_geometry::Side;

/// Éléments présents dans un projet de test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    pub cutting: bool,
    pub sewing: bool,
    pub construction: bool,
    pub notches: bool,
    pub markings: bool,
    pub buttons: bool,
    pub grain: bool,
    pub topstitch: bool,
    pub special: bool,
    pub axes: bool,
    pub dims: bool,
    pub full_dims: bool,
    pub extra_points: bool,
    /// Une pince de 30 × 10 mm sur le bord bas de la pièce.
    pub darts: bool,
    /// Fiche de pièce renseignée (étiquette écrite).
    pub label: bool,
}

impl Features {
    pub fn all() -> Features {
        Features {
            cutting: true,
            sewing: true,
            construction: true,
            notches: true,
            markings: true,
            buttons: true,
            grain: true,
            topstitch: true,
            special: true,
            axes: true,
            dims: true,
            full_dims: true,
            extra_points: true,
            darts: true,
            label: true,
        }
    }

    pub fn random(rng: &mut Rng) -> Features {
        let mut b = || rng.below(4) != 0;
        Features {
            cutting: b(),
            sewing: b(),
            construction: b(),
            notches: b(),
            markings: b(),
            buttons: b(),
            grain: b(),
            topstitch: b(),
            special: b(),
            axes: b(),
            dims: b(),
            full_dims: b(),
            extra_points: b(),
            darts: b(),
            label: b(),
        }
    }
}

/// Projet d'une feuille : une pièce `w × h` mm (w ≥ 60, h ≥ 40) et les éléments demandés.
pub fn project(f: Features, w: f64, h: f64) -> (Project, SheetId) {
    let mut p = Project::new("Contenu");
    let s = p.sheets[0].id;
    let outer = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    let role = |p: &mut Project, e, role| {
        p.apply(Command::SetPathRole {
            sheet: s,
            entity: e,
            role,
        })
        .unwrap();
    };
    if f.cutting {
        for &e in &outer.entities {
            role(&mut p, e, PathRole::CuttingContour);
        }
    }
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click {
                at: pt(w / 2.0, h / 2.0),
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let point = |p: &mut Project, x: f64, y: f64| {
        p.apply(Command::AddPoint {
            sheet: s,
            at: pt(x, y),
        })
        .unwrap()
        .points[0]
    };
    let seg = |p: &mut Project, a: (f64, f64), b: (f64, f64), r: PathRole| {
        let e = p
            .apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(a.0, a.1)),
                to: PointRef::New(pt(b.0, b.1)),
            })
            .unwrap()
            .entities[0];
        role(p, e, r);
        e
    };
    if f.markings {
        let a = point(&mut p, w * 0.5, h * 0.5);
        let b = point(&mut p, w * 0.25, h * 0.4);
        let c = point(&mut p, w * 0.75, h * 0.45);
        for (at, shape) in [
            (a, MarkingShape::Drill { diameter_mm: 6.0 }),
            (b, MarkingShape::Punch),
            (c, MarkingShape::Cross { size_mm: 8.0 }),
        ] {
            p.apply(Command::AddPieceMarking {
                piece,
                at: MarkingAnchor::Point { point: at },
                shape,
            })
            .unwrap();
        }
    }
    if f.darts {
        p.apply(Command::AddPieceDart {
            piece,
            apex: PointRef::New(pt(w * 0.4, 30.0)),
            toward: PointRef::New(pt(w * 0.4, 0.0)),
            length_mm: 30.0,
            opening_mm: 10.0,
            fixed: Side::Left,
        })
        .unwrap();
    }
    if f.label {
        p.apply(Command::SetPieceCard {
            piece,
            name: "Devant".into(),
            code: Some("D-1".into()),
            quantity: 2,
            mirrored_pair: true,
            material: Some("Coton".into()),
            category: Some(pf_core::PieceCategory::Fabric),
            comment: Some("Couper deux fois\nsens du fil".into()),
        })
        .unwrap();
    }
    if f.buttons {
        let a = point(&mut p, w * 0.2, h * 0.8);
        let b = point(&mut p, w * 0.8, h * 0.8);
        p.apply(Command::AddButtonGroup {
            piece,
            from: a,
            to: b,
            count: 3,
            element: ButtonElement::Button { diameter_mm: 10.0 },
            edge_margin_mm: 0.0,
        })
        .unwrap();
    }
    if f.grain {
        let main = seg(
            &mut p,
            (w * 0.6, h * 0.15),
            (w * 0.6, h * 0.35),
            PathRole::Grainline,
        );
        let outline = p.piece(piece).unwrap().outline.clone();
        p.apply(Command::SetPieceOutline {
            piece,
            outline,
            grainline: Some(main),
        })
        .unwrap();
        p.apply(Command::SetPieceGrain {
            piece,
            grain: PieceGrain {
                sense: GrainSense::Nap,
                ..PieceGrain::default()
            },
        })
        .unwrap();
    }
    if f.sewing {
        let inner = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(pt(6.0, 6.0)),
                width_mm: w - 12.0,
                height_mm: h - 12.0,
            })
            .unwrap();
        for &e in &inner.entities {
            role(&mut p, e, PathRole::SewingContour);
        }
    }
    if f.construction {
        // Sous la pièce : exclure la construction change la boîte englobante.
        seg(&mut p, (0.0, -15.0), (w, -15.0), PathRole::Construction);
        p.apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(w * 0.5, -40.0)),
            radius_mm: 9.0,
        })
        .unwrap();
    }
    if f.special {
        for (i, r) in [
            PathRole::Fold,
            PathRole::CenterFront,
            PathRole::WaistLine,
            PathRole::GlueLine,
        ]
        .into_iter()
        .enumerate()
        {
            let y = h * (0.52 + 0.04 * i as f64);
            seg(&mut p, (8.0, y), (w - 8.0, y), r);
        }
    }
    if f.axes {
        seg(
            &mut p,
            (w * 0.5, 4.0),
            (w * 0.5, h - 4.0),
            PathRole::AnnotationAxis,
        );
    }
    if f.topstitch {
        p.apply(Command::AddTopstitch {
            sheet: s,
            source: TopstitchSource::Paths {
                paths: outer.entities.clone(),
                from: None,
            },
            distance_mm: 3.0,
            side: TopstitchSide::Inside,
        })
        .unwrap();
    }
    if f.notches {
        for (i, style) in [NotchStyle::Single, NotchStyle::Double]
            .into_iter()
            .enumerate()
        {
            p.apply(Command::AddNotch {
                sheet: s,
                entity: outer.entities[i],
                distance_mm: 15.0,
                side: Side::Left,
                length_mm: 5.0,
                style,
            })
            .unwrap();
        }
    }
    if f.dims {
        p.apply(Command::AddDimension {
            sheet: s,
            from: outer.points[0],
            to: outer.points[1],
            offset_mm: 8.0,
        })
        .unwrap();
    }
    if f.full_dims {
        p.apply(Command::AddFullDimension {
            sheet: s,
            kind: FullDimensionKind::Horizontal {
                from: outer.points[0],
                to: outer.points[1],
                offset_mm: -10.0,
            },
            label: Some("Largeur".into()),
            text_shift_mm: 0.0,
        })
        .unwrap();
    }
    if f.extra_points {
        point(&mut p, w * 0.35, h * 0.9);
        point(&mut p, w * 0.9, h * 0.1);
    }
    (p, s)
}

/// Projet aléatoire : caractéristiques, taille (d'une page à plusieurs).
pub fn random_project(rng: &mut Rng) -> (Project, SheetId, Features) {
    let f = Features::random(rng);
    let (w, h) = match rng.below(4) {
        0 => (rng.range(60.0, 150.0), rng.range(40.0, 120.0)),
        1 => (rng.range(150.0, 400.0), rng.range(100.0, 300.0)),
        2 => (rng.range(300.0, 700.0), rng.range(200.0, 500.0)),
        _ => (rng.range(60.0, 100.0), rng.range(40.0, 70.0)),
    };
    let (p, s) = project(f, w, h);
    (p, s, f)
}

// ---------------------------------------------------------------------------
// Lecteur SVG indépendant
// ---------------------------------------------------------------------------

/// Famille d'un élément du SVG, d'après le groupe qui le porte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Paths,
    Notch,
    Marking,
    Button,
    Dim,
    Cote,
    Points,
}

#[derive(Debug, Clone)]
pub struct Elem {
    pub group: Group,
    pub tag: &'static str,
    /// `data-role` d'un tracé.
    pub role: Option<String>,
    /// `data-form` d'un marquage ou d'un bouton.
    pub form: Option<String>,
    /// Points de l'élément (coordonnées SVG) : seuls ceux qui se translatent avec le dessin.
    pub pts: Vec<(f64, f64)>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Svg {
    pub width: f64,
    pub height: f64,
    pub elems: Vec<Elem>,
    /// Nombre de groupes `<g data-kind=…>` (cotes complètes).
    pub cotes: usize,
}

pub fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = start + tag[start..].find('"')?;
    Some(tag[start..end].to_string())
}

fn num_attr(tag: &str, name: &str) -> f64 {
    attr(tag, name)
        .unwrap_or_else(|| panic!("{name} absent de {tag}"))
        .parse()
        .unwrap()
}

/// Points d'un attribut `d` : M, L, C (paires) et A (x y seulement ; rayons et drapeaux ne se
/// translatent pas).
fn path_points(d: &str) -> Vec<(f64, f64)> {
    let toks: Vec<&str> = d.split_whitespace().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            "M" | "L" => {
                out.push((toks[i + 1].parse().unwrap(), toks[i + 2].parse().unwrap()));
                i += 3;
            }
            "C" => {
                for k in 0..3 {
                    out.push((
                        toks[i + 1 + 2 * k].parse().unwrap(),
                        toks[i + 2 + 2 * k].parse().unwrap(),
                    ));
                }
                i += 7;
            }
            "A" => {
                out.push((toks[i + 6].parse().unwrap(), toks[i + 7].parse().unwrap()));
                i += 8;
            }
            _ => i += 1,
        }
    }
    out
}

fn unescape(t: &str) -> String {
    t.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

pub fn read_svg(svg: &str) -> Svg {
    let mut out = Svg::default();
    let mut stack: Vec<Group> = Vec::new();
    for raw in svg.lines() {
        let l = raw.trim();
        if l.starts_with("<svg") {
            // « 120mm » : retirer l'unité.
            let wh = |name: &str| {
                attr(l, name)
                    .unwrap()
                    .trim_end_matches("mm")
                    .parse::<f64>()
                    .unwrap()
            };
            out.width = wh("width");
            out.height = wh("height");
        } else if l.starts_with("<g ") {
            let g = if l.contains("data-role=\"cran\"") {
                Group::Notch
            } else if l.contains("id=\"marquages\"") {
                Group::Marking
            } else if l.contains("id=\"boutons\"") {
                Group::Button
            } else if l.contains("data-role=\"cotation\"") {
                Group::Dim
            } else if l.contains("data-role=\"cote\"") {
                Group::Cote
            } else if l.contains("data-kind=") {
                out.cotes += 1;
                Group::Cote
            } else if l.contains("font-size=\"3\"") {
                Group::Points
            } else {
                Group::Paths
            };
            stack.push(g);
        } else if l == "</g>" {
            stack.pop();
        } else if let Some(&group) = stack.last() {
            let tag = if l.starts_with("<line") {
                "line"
            } else if l.starts_with("<path") {
                "path"
            } else if l.starts_with("<circle") {
                "circle"
            } else if l.starts_with("<text") {
                "text"
            } else {
                continue;
            };
            let mut e = Elem {
                group,
                tag,
                role: attr(l, "data-role"),
                form: attr(l, "data-form"),
                pts: Vec::new(),
                text: None,
            };
            match tag {
                "line" => {
                    e.pts = vec![
                        (num_attr(l, "x1"), num_attr(l, "y1")),
                        (num_attr(l, "x2"), num_attr(l, "y2")),
                    ]
                }
                "circle" => e.pts = vec![(num_attr(l, "cx"), num_attr(l, "cy"))],
                "text" => {
                    e.pts = vec![(num_attr(l, "x"), num_attr(l, "y"))];
                    let inner = l.split_once('>').unwrap().1;
                    e.text = Some(unescape(inner.rsplit_once("</text>").unwrap().0));
                }
                _ => e.pts = path_points(&attr(l, "d").unwrap()),
            }
            out.elems.push(e);
        }
    }
    out
}

/// Catégorie d'un élément du SVG, d'après les identifiants écrits dans le fichier (table
/// réécrite ici, indépendante de `OutputCategory::of_role`).
pub fn category_of(e: &Elem) -> OutputCategory {
    use OutputCategory as C;
    match e.group {
        Group::Paths => match e.role.as_deref().expect("tracé sans data-role") {
            "construction" => C::Construction,
            "couture" => C::SewingContour,
            "coupe" => C::CuttingContour,
            "droit-fil" => C::Grainline,
            "axe-repere" => C::Axes,
            "pince" => C::Darts,
            "etiquette-piece" => C::PieceLabels,
            "pliure" | "surpiqure" | "milieu-devant" | "milieu-dos" | "ligne-taille"
            | "ligne-hanche" | "ligne-poitrine" | "ligne-parage" | "ligne-collage" => {
                C::SpecialLines
            }
            other => panic!("rôle SVG inconnu : {other}"),
        },
        Group::Notch => C::Notches,
        Group::Marking => C::Markings,
        Group::Button => C::Buttons,
        Group::Dim => C::Dimensions,
        Group::Cote => C::FullDimensions,
        Group::Points => {
            if e.tag == "text" {
                C::PointLabels
            } else {
                C::Points
            }
        }
    }
}

impl Svg {
    pub fn count(&self, c: OutputCategory) -> usize {
        self.elems.iter().filter(|e| category_of(e) == c).count()
    }

    pub fn of(&self, c: OutputCategory) -> Vec<&Elem> {
        self.elems.iter().filter(|e| category_of(e) == c).collect()
    }

    /// Textes d'une catégorie (cotations, cotes complètes, noms de points).
    pub fn texts(&self, c: OutputCategory) -> Vec<String> {
        self.of(c).iter().filter_map(|e| e.text.clone()).collect()
    }
}

/// Les éléments `kept` de `full`, dans l'ordre, doivent se retrouver dans `filtered` aux mêmes
/// coordonnées à une translation constante près (la boîte englobante a pu changer). Renvoie la
/// translation, ou `None` si rien n'est retenu.
pub fn same_up_to_shift(
    full: &Svg,
    filtered: &Svg,
    kept: impl Fn(OutputCategory) -> bool,
) -> Result<Option<(f64, f64)>, String> {
    let expected: Vec<&Elem> = full.elems.iter().filter(|e| kept(category_of(e))).collect();
    if expected.len() != filtered.elems.len() {
        return Err(format!(
            "{} éléments attendus, {} trouvés",
            expected.len(),
            filtered.elems.len()
        ));
    }
    let mut shift: Option<(f64, f64)> = None;
    for (a, b) in expected.iter().zip(&filtered.elems) {
        if a.tag != b.tag || a.role != b.role || a.form != b.form || a.text != b.text {
            return Err(format!("élément différent : {a:?} / {b:?}"));
        }
        if a.pts.len() != b.pts.len() {
            return Err(format!("nombre de points différent : {a:?} / {b:?}"));
        }
        for (p, q) in a.pts.iter().zip(&b.pts) {
            let d = (q.0 - p.0, q.1 - p.1);
            match shift {
                None => shift = Some(d),
                Some(s) => {
                    if (d.0 - s.0).abs() > 2e-3 || (d.1 - s.1).abs() > 2e-3 {
                        return Err(format!("translation non constante : {s:?} puis {d:?}"));
                    }
                }
            }
        }
    }
    Ok(shift)
}

// ---------------------------------------------------------------------------
// Lecteur PDF indépendant (flux de contenu en lignes)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct TilePage {
    /// Lignes du flux entre le rectangle de découpe et `Q` : tout le patron de la page.
    pub clip: Vec<String>,
    /// Texte de l'en-tête de page.
    pub header: String,
}

#[derive(Debug, Clone, Default)]
pub struct Pdf {
    /// `/Count` du catalogue.
    pub pages: usize,
    pub tiles: Vec<TilePage>,
    /// Tous les flux (une chaîne par page).
    pub streams: Vec<String>,
}

pub fn read_pdf_streams(bytes: &[u8]) -> Pdf {
    let readable = super::pdf_text::readable(bytes);
    let text = readable.as_str();
    let mut out = Pdf {
        pages: text[text.find("/Count ").unwrap() + 7..]
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap(),
        ..Pdf::default()
    };
    let mut rest = text;
    while let Some(start) = rest.find("stream\n") {
        let body = &rest[start + 7..];
        let end = body.find("endstream").unwrap();
        let stream = &body[..end];
        out.streams.push(stream.to_string());
        let header_line = stream
            .lines()
            .find(|l| l.contains("100 % taille r") && l.contains("sur "))
            .map(str::to_string);
        if let Some(header) = header_line {
            let lines: Vec<&str> = stream.lines().collect();
            let from = lines
                .iter()
                .position(|l| l.ends_with(" re W n"))
                .expect("rectangle de découpe");
            let to = lines[from..]
                .iter()
                .position(|l| *l == "Q")
                .map(|i| from + i)
                .expect("fin de la découpe");
            out.tiles.push(TilePage {
                clip: lines[from + 1..to].iter().map(|s| s.to_string()).collect(),
                header,
            });
        }
        rest = &body[end + 9..];
    }
    out
}

impl TilePage {
    /// Opérations de dessin et de texte (une par élément écrit).
    pub fn ops(&self) -> usize {
        self.clip
            .iter()
            .filter(|l| l.ends_with(" S") || l.ends_with(" f") || l.starts_with("BT "))
            .count()
    }

    pub fn texts(&self) -> usize {
        self.clip.iter().filter(|l| l.starts_with("BT ")).count()
    }
}

impl Pdf {
    pub fn ops(&self) -> usize {
        self.tiles.iter().map(TilePage::ops).sum()
    }
}

/// `small` est une sous-suite de `big` (mêmes lignes, même ordre).
pub fn is_subsequence(small: &[String], big: &[String]) -> bool {
    let mut it = big.iter();
    small.iter().all(|s| it.any(|b| b == s))
}

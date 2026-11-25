//! PH-5c — campagne adverse : « Rempli … mm » et « Coupe nette » dans les quatre exports.
//!
//! 160 séquences à graine fixe (xorshift) : un rectangle de taille et d'origine aléatoires, tracés de
//! rôle couture, avec un rempli aléatoire sur un à quatre bords (valeurs au demi-millimètre ou à
//! 0,125 mm), ou en coupe nette, ou ordinaire ; fiche renseignée ou non ; langue, mode couture /
//! coupe et filtre de contenu aléatoires ; feuille tournée d'un quart de tour (PDF) dans un quart
//! des cas ; pièces minces (14 à 24 mm) où le libellé doit glisser le long du bord.
//!
//! **Oracle indépendant** (rien n'est lu du code d'export) : texte attendu recalculé en entiers
//! (valeur au millième, zéros finaux retirés, séparateur de la langue), position du milieu du bord,
//! normale intérieure, décalage au centre du libellé (1,5 mm + demi-extension du texte), glissement
//! le long du bord (pas de 3,5 mm, tangente (−ny ; nx), demi-longueur du bord), emplacement du bloc
//! d'étiquette de la pièce, passage modèle → fichier lu sur les points des sommets (SVG, PDF) ou sur
//! le coin bas-gauche du contour (DXF, HPGL). Lecteurs : `lecteurs_libelles/`.
//!
//! Chaque séquence relit les quatre formats et vérifie : textes, nombre, position, absence totale
//! sans déclaration, absence sous le filtre « étiquettes de pièce », indépendance du mode couture /
//! coupe, langue (DXF toujours anglais).

mod lecteurs_libelles;

use lecteurs_libelles::*;
use pf_core::{Command, EntityId, FoldAllowance, PathRole, Project, SeamAllowance, SheetId};
use pf_export::dxf::export_dxf_with;
use pf_export::hpgl::{export_hpgl, HpglError};
use pf_export::pdf::{export_pdf_with, plan_tiles_with};
use pf_export::svg::export_svg;
use pf_export::{
    AssemblyOptions, OutputCategory as C, OutputContent, PageFormat, PageSetup, PieceExportMode,
    SceneOptions,
};
use pf_i18n::Lang;

const SEQUENCES: u64 = 160;

// ---------------------------------------------------------------------------------------------
// Générateur
// ---------------------------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 11) % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Rempli,
    NetCut,
    Plain,
}

#[derive(Clone, Debug)]
struct Case {
    kind: Kind,
    lang: Lang,
    rotated: bool,
    thin: bool,
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
    name: String,
    code: Option<String>,
    qty: u32,
    /// (bord 0 bas, 1 droite, 2 haut, 3 gauche ; valeur du rempli en mm).
    folds: Vec<(usize, f64)>,
    margin: f64,
    content: usize,
}

fn gen_case(rng: &mut Rng, i: u64) -> Case {
    let kind = match rng.below(100) {
        0..=54 => Kind::Rempli,
        55..=84 => Kind::NetCut,
        _ => Kind::Plain,
    };
    let rotated = rng.chance(25);
    let thin = !rotated && rng.chance(25);
    let (w, h) = if rotated {
        (200.0 + rng.below(41) as f64, 60.0 + rng.below(91) as f64)
    } else if thin {
        (120.0 + rng.below(41) as f64, 14.0 + rng.below(11) as f64)
    } else {
        (120.0 + rng.below(41) as f64, 60.0 + rng.below(121) as f64)
    };
    let mut folds = Vec::new();
    if kind == Kind::Rempli {
        let mut edges: Vec<usize> = if thin { vec![0, 2] } else { vec![0, 1, 2, 3] };
        let count = 1 + rng.below(edges.len() as u64) as usize;
        for _ in 0..count {
            let e = edges.remove(rng.below(edges.len() as u64) as usize);
            let v = if rng.chance(20) {
                (1 + rng.below(96)) as f64 * 0.125
            } else {
                (1 + rng.below(24)) as f64 * 0.5
            };
            folds.push((e, v));
        }
    }
    let code = rng.chance(60).then(|| format!("K{}", rng.below(1000)));
    Case {
        kind,
        lang: if rng.chance(50) { Lang::Fr } else { Lang::En },
        rotated,
        thin,
        x0: rng.below(401) as f64 - 100.0,
        y0: rng.below(401) as f64 - 100.0,
        w,
        h,
        name: format!("Pc{i}"),
        code,
        qty: 1 + rng.below(3) as u32,
        folds,
        margin: if rng.chance(50) {
            0.0
        } else {
            (1 + rng.below(6)) as f64
        },
        content: rng.below(7) as usize,
    }
}

fn content_of(id: usize) -> OutputContent {
    let mut c = match id {
        1 => OutputContent::default().without(C::PieceLabels),
        2 => OutputContent::sewing_only(),
        3 => OutputContent::cutting_only(),
        4 => OutputContent::default().without(C::SpecialLines),
        _ => OutputContent::default(),
    };
    match id {
        5 => c.piece_mode = PieceExportMode::SewingOnly,
        6 => c.piece_mode = PieceExportMode::CutOnly,
        _ => {}
    }
    c
}

fn build(c: &Case) -> (Project, SheetId, [EntityId; 4]) {
    let mut p = Project::new("Campagne");
    let s = p.sheets[0].id;
    let (piece, e) = rect(
        &mut p,
        s,
        (c.x0, c.y0),
        (c.w, c.h),
        &c.name,
        PathRole::SewingContour,
    );
    let edges = [e.bottom, e.right, e.top, e.left];
    match c.kind {
        Kind::Rempli => {
            p.apply(Command::SetSeamAllowance {
                piece,
                seam: Some(SeamAllowance {
                    default_mm: c.margin,
                    folds: c
                        .folds
                        .iter()
                        .map(|(i, v)| FoldAllowance {
                            entity: edges[*i],
                            value_mm: *v,
                            notch_step_mm: None,
                            notch_count: None,
                        })
                        .collect(),
                    ..Default::default()
                }),
            })
            .unwrap();
        }
        Kind::NetCut => {
            p.apply(Command::SetNetCut {
                piece,
                net_cut: true,
            })
            .unwrap();
        }
        Kind::Plain => {}
    }
    if c.code.is_some() || c.qty != 1 {
        p.apply(Command::SetPieceCard {
            piece,
            name: c.name.clone(),
            code: c.code.clone(),
            quantity: c.qty,
            mirrored_pair: false,
            material: None,
            category: None,
            comment: None,
        })
        .unwrap();
    }
    (p, s, edges)
}

// ---------------------------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------------------------

fn corners(c: &Case) -> [(f64, f64); 4] {
    [
        (c.x0, c.y0),
        (c.x0 + c.w, c.y0),
        (c.x0 + c.w, c.y0 + c.h),
        (c.x0, c.y0 + c.h),
    ]
}

/// Valeur au plus court, en entiers : millièmes, zéros finaux retirés, séparateur de la langue.
fn value_text(v: f64, lang: Lang) -> String {
    let m = (v * 1000.0).round() as i64;
    let (ip, fp) = (m / 1000, m % 1000);
    let sep = if lang == Lang::Fr { ',' } else { '.' };
    if fp == 0 {
        ip.to_string()
    } else {
        let f = format!("{fp:03}");
        format!("{ip}{sep}{}", f.trim_end_matches('0'))
    }
}

fn rempli_text(lang: Lang, v: f64) -> String {
    let head = if lang == Lang::Fr {
        "Rempli"
    } else {
        "Fold-over"
    };
    format!("{head} {} mm", value_text(v, lang))
}

fn mention(lang: Lang) -> &'static str {
    if lang == Lang::Fr {
        "Coupe nette"
    } else {
        "Net cut"
    }
}

/// Lignes de l'étiquette de pièce des formats à scène (français, mention dans la langue).
fn piece_lines(c: &Case, lang: Lang) -> Vec<String> {
    let mut l = Vec::new();
    if c.code.is_some() || c.qty != 1 {
        l.push(c.name.clone());
        if let Some(k) = &c.code {
            l.push(format!("Code : {k}"));
        }
        l.push(format!("Quantité : {}", c.qty));
    }
    if c.kind == Kind::NetCut {
        if l.is_empty() {
            l.push(c.name.clone());
        }
        l.push(mention(lang).to_string());
    }
    l
}

/// Lignes du bloc de texte du DXF (anglais, toujours écrites).
fn dxf_lines(c: &Case) -> Vec<String> {
    let mut l = vec![format!("Piece Name: {}", c.name)];
    if let Some(k) = &c.code {
        l.push(format!("Code: {k}"));
    }
    l.push(format!("Quantity: {}", c.qty));
    if c.kind == Kind::NetCut {
        l.push("Net cut".to_string());
    }
    l
}

type P = (f64, f64);
type Rect = (P, P);
/// Point du bord retenu, normale intérieure, centre du texte, nombre de glissements.
type Placed = (P, P, P, usize);
/// Texte, point du bord retenu, normale intérieure, centre du texte.
type Expected = (String, P, P, P);

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.0 .0 < b.1 .0 && b.0 .0 < a.1 .0 && a.0 .1 < b.1 .1 && b.0 .1 < a.1 .1
}

/// Milieu, sortant, longueur du bord `i` (0 bas, 1 droite, 2 haut, 3 gauche).
fn edge(c: &Case, i: usize) -> ((f64, f64), (f64, f64), f64) {
    let (xm, ym) = (c.x0 + c.w / 2.0, c.y0 + c.h / 2.0);
    match i {
        0 => ((xm, c.y0), (0.0, -1.0), c.w),
        1 => ((c.x0 + c.w, ym), (1.0, 0.0), c.h),
        2 => ((xm, c.y0 + c.h), (0.0, 1.0), c.w),
        _ => ((c.x0, ym), (-1.0, 0.0), c.h),
    }
}

/// Position attendue du libellé du bord `i` : point du bord retenu et centre du texte, dans le
/// repère du modèle, pour un texte de `chars` caractères estimé à 0,62 em de 3,5 mm, hauteur 3,5 mm
/// et les obstacles donnés. Retourne aussi le nombre de glissements.
fn expected_center(c: &Case, i: usize, chars: usize, obstacles: &[Rect]) -> Placed {
    let (mid, out, len) = edge(c, i);
    let inward = (-out.0, -out.1);
    let hw = chars as f64 * 3.5 * 0.62 / 2.0;
    // Centre : écart 1,5 mm + demi-extension du texte selon la normale (verticale : hauteur 1,75).
    let centre = |at: (f64, f64)| -> (f64, f64) {
        let off = 1.5 + if inward.0 != 0.0 { hw } else { 1.75 };
        (at.0 + inward.0 * off, at.1 + inward.1 * off)
    };
    let rect = |m: (f64, f64)| -> Rect { ((m.0 - hw, m.1 - 1.75), (m.0 + hw, m.1 + 1.75)) };
    let free = |at: (f64, f64)| !obstacles.iter().any(|o| overlap(&rect(centre(at)), o));
    if free(mid) {
        return (mid, inward, centre(mid), 0);
    }
    let tangent = (-out.1, out.0);
    let steps = (((len / 2.0 - hw).max(0.0)) / 3.5).floor() as i64;
    for j in 1..=steps {
        for sign in [1.0, -1.0] {
            let s = sign * j as f64 * 3.5;
            let at = (mid.0 + tangent.0 * s, mid.1 + tangent.1 * s);
            if free(at) {
                return (at, inward, centre(at), 1);
            }
        }
    }
    (mid, inward, centre(mid), 0)
}

/// Bloc d'étiquette de pièce des formats à scène : centré sur l'emprise du contour, 2,2 mm par
/// caractère, 5 mm par ligne.
fn scene_label_rect(c: &Case, lines: &[String]) -> Option<Rect> {
    if lines.is_empty() {
        return None;
    }
    let (cx, cy) = (c.x0 + c.w / 2.0, c.y0 + c.h / 2.0);
    let widest = lines.iter().map(|l| l.chars().count()).max().unwrap() as f64 * 2.2;
    let hh = lines.len() as f64 * 2.5;
    Some(((cx - widest / 2.0, cy - hh), (cx + widest / 2.0, cy + hh)))
}

/// Libellés attendus (texte, point du bord, normale intérieure, centre) des formats à scène.
fn expected_scene_labels(c: &Case, lang: Lang, shifts: &mut usize) -> Vec<Expected> {
    let obstacles: Vec<Rect> = scene_label_rect(c, &piece_lines(c, lang))
        .into_iter()
        .collect();
    c.folds
        .iter()
        .map(|(i, v)| {
            let text = rempli_text(lang, *v);
            let (at, inward, centre, moved) =
                expected_center(c, *i, text.chars().count(), &obstacles);
            *shifts += moved;
            (text, at, inward, centre)
        })
        .collect()
}

fn baseline(c: &Case, n: usize, i: usize) -> f64 {
    c.y0 + c.h / 2.0 + ((n as f64 - 1.0) / 2.0 - i as f64) * 5.0 - 1.4
}

fn ascii(s: &str) -> String {
    s.replace('é', "e")
}

// ---------------------------------------------------------------------------------------------
// Vérifications par format
// ---------------------------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    rempli: usize,
    net_cut: usize,
    plain: usize,
    rotated_pdf: usize,
    shifted: usize,
    filtered_out: usize,
    hpgl_empty: usize,
    labels_checked: usize,
    langs_en: usize,
    modes_cut_only: usize,
    modes_sewing_only: usize,
    thin: usize,
}

fn setup_pdf(rotated: bool) -> PageSetup {
    if rotated {
        PageSetup {
            overlap_mm: 10.0,
            assembly: AssemblyOptions {
                marks: false,
                plan: false,
                save_paper: true,
            },
            ..PageSetup::default()
        }
    } else {
        PageSetup::default()
    }
}

fn run(c: &Case, n: &mut Counters) {
    let (p, s, _) = build(c);
    let content = content_of(c.content);
    let labels_on = content.includes(C::PieceLabels);
    let scene = content
        .scene(
            &p,
            s,
            SceneOptions {
                lang: c.lang,
                ..SceneOptions::default()
            },
        )
        .unwrap();
    let lines = piece_lines(c, c.lang);
    let mut shifts = 0;
    let rempli = if c.kind == Kind::Rempli {
        expected_scene_labels(c, c.lang, &mut shifts)
    } else {
        Vec::new()
    };
    let (cx, cy) = (c.x0 + c.w / 2.0, c.y0 + c.h / 2.0);
    let _ = cy;

    // --- Scène : contenu du filtre --------------------------------------------------------
    if labels_on {
        assert_eq!(scene.fold_overs.len(), rempli.len(), "{c:?}");
        assert_eq!(
            scene.piece_labels.len(),
            usize::from(!lines.is_empty()),
            "{c:?}"
        );
        n.labels_checked += rempli.len() + lines.len();
        if shifts > 0 {
            n.shifted += 1;
        }
    } else {
        assert!(
            scene.fold_overs.is_empty() && scene.piece_labels.is_empty(),
            "{c:?}"
        );
        if c.kind != Kind::Plain {
            n.filtered_out += 1;
        }
    }
    let want_rempli: Vec<_> = if labels_on {
        rempli.clone()
    } else {
        Vec::new()
    };
    let want_lines: Vec<String> = if labels_on { lines.clone() } else { Vec::new() };

    // --- SVG ------------------------------------------------------------------------------
    {
        let svg = export_svg(&scene).unwrap();
        let (dx, dy) = svg_frame(&svg, &corners(c));
        let texts = svg_texts(&svg);
        let over: Vec<&SvgText> = texts
            .iter()
            .filter(|t| t.role == "libelle-rempli")
            .collect();
        assert_eq!(over.len(), want_rempli.len(), "{c:?}\n{over:?}");
        for (text, _, _, (mx, my)) in &want_rempli {
            let found = over.iter().any(|t| {
                t.text == *text && near(t.x, mx + dx, 2e-3) && near(t.y, dy - (my - 1.225), 2e-3)
            });
            assert!(
                found,
                "{c:?}\nSVG : « {text} » attendu en ({mx} ; {my}) : {over:?}"
            );
        }
        let etiq: Vec<&SvgText> = texts
            .iter()
            .filter(|t| t.role == "etiquette-piece")
            .collect();
        assert_eq!(etiq.len(), want_lines.len(), "{c:?}\n{etiq:?}");
        for (k, t) in etiq.iter().enumerate() {
            assert_eq!(t.text, want_lines[k], "{c:?}");
            assert!(near(t.x, cx + dx, 2e-3), "{c:?}\n{t:?}");
            assert!(
                near(t.y, dy - baseline(c, want_lines.len(), k), 2e-3),
                "{c:?}\n{t:?} / {}",
                dy - baseline(c, want_lines.len(), k)
            );
        }
    }

    // --- PDF (A4 ; feuille tournée dans un quart des cas) ----------------------------------
    {
        let setup = setup_pdf(c.rotated);
        let tiles = plan_tiles_with(&scene, &setup).unwrap();
        assert_eq!(tiles.len(), 1, "{c:?}");
        let turns = tiles[0].turns;
        if c.rotated {
            assert_eq!(turns, 1, "{c:?}");
            n.rotated_pdf += 1;
        } else {
            assert_eq!(turns, 0, "{c:?}");
        }
        let pdf = export_pdf_with(std::slice::from_ref(&scene), &setup).unwrap();
        let texts = pdf_texts(&pdf.bytes);
        // Page du patron : celle qui porte les croix des quatre sommets.
        let page = pdf_segments(&pdf.bytes)
            .iter()
            .find(|(_, s)| ((s[2] - s[0]).hypot(s[3] - s[1]) - 2.0).abs() < 1e-3)
            .map(|(p, _)| *p)
            .expect("croix de point");
        let (px, py) = pdf_frame(&pdf.bytes, page, &corners(c), turns);
        let on_page: Vec<&PdfText> = texts.iter().filter(|t| t.page == page).collect();
        // Libellés du rempli : texte horizontal, repère de la page.
        let over: Vec<&&PdfText> = on_page
            .iter()
            .filter(|t| t.text.starts_with("Rempli ") || t.text.starts_with("Fold-over "))
            .collect();
        assert_eq!(over.len(), want_rempli.len(), "{c:?}\n{over:?}");
        for (text, at, inward, _) in &want_rempli {
            let a = rot(*at, turns);
            let v = rot(*inward, turns);
            let hw = text.chars().count() as f64 * 3.5 * 0.62 / 2.0;
            let off = 1.5 + if v.0.abs() > 0.5 { hw } else { 1.75 };
            let centre = (px + a.0 + v.0 * off, py + a.1 + v.1 * off);
            let width = pdf_text_width_mm(&pdf.bytes, text, 3.5 * 72.0 / 25.4);
            let found = over.iter().any(|t| {
                t.text == *text
                    && near(t.angle_deg, 0.0, 1e-9)
                    && near(t.x_mm, centre.0 - width / 2.0, 0.03)
                    && near(t.y_mm, centre.1 - 1.225, 0.03)
            });
            assert!(
                found,
                "{c:?}\nPDF : « {text} » attendu au centre {centre:?} : {over:?}"
            );
        }
        // Étiquette de pièce : une ligne par texte (corps 4 mm), tournée de 90° sur feuille tournée.
        let name_lines: Vec<&&PdfText> = on_page
            .iter()
            .filter(|t| (t.size_pt - 4.0 * 72.0 / 25.4).abs() < 0.01)
            .collect();
        assert_eq!(name_lines.len(), want_lines.len(), "{c:?}\n{name_lines:?}");
        let centre = rot((cx, c.y0 + c.h / 2.0), turns);
        let centre = (px + centre.0, py + centre.1);
        let n_lines = want_lines.len();
        for (k, line) in want_lines.iter().enumerate() {
            let up = ((n_lines as f64 - 1.0) / 2.0 - k as f64) * 5.0 - 1.4;
            let half = pdf_text_width_mm(&pdf.bytes, line, 4.0 * 72.0 / 25.4) / 2.0;
            // Sens de lecture (c ; s) et normale (−s ; c) : angle 0 ou 90°.
            let (s, co) = if turns == 1 { (1.0, 0.0) } else { (0.0, 1.0) };
            let (x, y) = (centre.0 - s * up - co * half, centre.1 + co * up - s * half);
            let found = name_lines.iter().any(|t| {
                t.text == *line
                    && near(t.angle_deg, if turns == 1 { 90.0 } else { 0.0 }, 1e-6)
                    && near(t.x_mm, x, 0.03)
                    && near(t.y_mm, y, 0.03)
            });
            assert!(
                found,
                "{c:?}\nPDF : ligne « {line} » attendue en ({x} ; {y}) : {name_lines:?}"
            );
        }
        // Rien n'est écrit sous le filtre : aucune trace des libellés.
        if !labels_on && c.kind != Kind::Plain {
            let all: Vec<&str> = texts.iter().map(|t| t.text.as_str()).collect();
            assert!(
                !all.iter().any(|t| t.starts_with("Rempli ")
                    || t.starts_with("Fold-over ")
                    || *t == "Coupe nette"
                    || *t == "Net cut"),
                "{c:?}\n{all:?}"
            );
        }
    }

    // --- HPGL (rouleau : aucune limite de taille) --------------------------------------------
    {
        let page = PageSetup {
            format: PageFormat::Roll { width_mm: 914.0 },
            ..PageSetup::default()
        };
        // Tracés écrits : couture (si le mode et le filtre la gardent), coupe calculée (pièce à
        // marge) ou coupe nette (rôle coupe en mode « avec coupe », couture sinon).
        let mode = content.piece_mode;
        let (sewing, cut) = match c.kind {
            Kind::Rempli => (
                mode.includes_sewing() && content.includes(C::SewingContour),
                mode.includes_cut() && content.includes(C::CuttingContour),
            ),
            Kind::NetCut if mode.includes_cut() => (false, content.includes(C::CuttingContour)),
            Kind::NetCut => (content.includes(C::SewingContour), false),
            Kind::Plain => (content.includes(C::SewingContour), false),
        };
        match export_hpgl(&scene, &page) {
            Err(HpglError::Nothing) => {
                assert!(!sewing && !cut, "{c:?}");
                n.hpgl_empty += 1;
            }
            Err(e) => panic!("{c:?}\nHPGL : {e:?}"),
            Ok(plot) => {
                assert!(sewing || cut, "{c:?}");
                let text = &plot.text;
                // Origine du plot = coin bas-gauche des traits : coupe calculée (marge de chaque
                // bord = rempli du bord ou marge par défaut) sinon contour de la pièce.
                let margin = |i: usize| {
                    c.folds
                        .iter()
                        .find(|(e, _)| *e == i)
                        .map_or(c.margin, |(_, v)| *v)
                };
                let contour_min = if cut && c.kind == Kind::Rempli {
                    (c.x0 - margin(3), c.y0 - margin(0))
                } else {
                    (c.x0, c.y0)
                };
                // L'emprise du plot compte aussi le bloc d'étiquette (pièce mince : il dépasse).
                let block = scene_label_rect(c, &want_lines);
                let origin = block.map_or(contour_min, |b| {
                    (contour_min.0.min(b.0 .0), contour_min.1.min(b.0 .1))
                });
                let strokes: Vec<(i64, i64)> = (1..=9)
                    .flat_map(|pen| hpgl_strokes(text, pen))
                    .flatten()
                    .collect();
                let (sx, sy) = (
                    strokes.iter().map(|p| p.0).min().unwrap() as f64,
                    strokes.iter().map(|p| p.1).min().unwrap() as f64,
                );
                assert!(
                    (sx - (contour_min.0 - origin.0) * 40.0).abs() <= 1.01
                        && (sy - (contour_min.1 - origin.1) * 40.0).abs() <= 1.01,
                    "{c:?}\n{text}"
                );
                let labels = hpgl_labels(text);
                let over: Vec<&HpglLabel> = labels
                    .iter()
                    .filter(|l| l.text.starts_with("Rempli ") || l.text.starts_with("Fold-over "))
                    .collect();
                assert_eq!(over.len(), want_rempli.len(), "{c:?}\n{labels:?}");
                let u = |m: f64, o: f64| (m - o) * 40.0;
                for (label, _, _, (mx, my)) in &want_rempli {
                    let hw = label.chars().count() as f64 * 3.5 * 0.62 / 2.0;
                    let (ex, ey) = (u(mx - hw, origin.0), u(my - 1.225, origin.1));
                    let found = over.iter().any(|l| {
                        l.text == *label
                            && l.pen == 7
                            && (l.x as f64 - ex).abs() <= 1.01
                            && (l.y as f64 - ey).abs() <= 1.01
                    });
                    assert!(
                        found,
                        "{c:?}\nHPGL : « {label} » attendu en ({ex} ; {ey}) : {over:?}"
                    );
                }
                let etiq: Vec<&HpglLabel> = labels
                    .iter()
                    .filter(|l| {
                        !(l.text.starts_with("Rempli ") || l.text.starts_with("Fold-over "))
                    })
                    .collect();
                assert_eq!(etiq.len(), want_lines.len(), "{c:?}\n{labels:?}");
                for (k, line) in want_lines.iter().enumerate() {
                    let l = etiq[k];
                    let start = cx - line.chars().count() as f64 * 2.2 / 2.0;
                    assert_eq!(l.text, ascii(line), "{c:?}");
                    assert_eq!(l.pen, 7);
                    assert!(
                        (l.x as f64 - u(start, origin.0)).abs() <= 1.01
                            && (l.y as f64 - u(baseline(c, want_lines.len(), k), origin.1)).abs()
                                <= 1.01,
                        "{c:?}\nHPGL : {l:?}"
                    );
                }
            }
        }
    }

    // --- DXF (anglais, bloc de texte toujours écrit : aucun filtre de contenu) ---------------
    {
        let dxf = export_dxf_with(&p, content.piece_mode).unwrap().text;
        let all = (1..=99)
            .map(|l| dxf_layer_vertices(&dxf, &l.to_string()))
            .filter(|v| !v.is_empty());
        let verts: Vec<(f64, f64)> = all.flatten().collect();
        let (min_x, min_y) = (
            verts.iter().map(|p| p.0).fold(f64::MAX, f64::min),
            verts.iter().map(|p| p.1).fold(f64::MAX, f64::min),
        );
        let mode = content.piece_mode;
        let margin = |i: usize| {
            c.folds
                .iter()
                .find(|(e, _)| *e == i)
                .map_or(c.margin, |(_, v)| *v)
        };
        let origin = if c.kind == Kind::Rempli && mode.includes_cut() {
            (c.x0 - margin(3), c.y0 - margin(0))
        } else {
            (c.x0, c.y0)
        };
        let to_file = |m: (f64, f64)| (m.0 - origin.0 + min_x, m.1 - origin.1 + min_y);
        let texts = dxf_texts(&dxf);
        // Bloc de texte : n lignes à 6 mm, texte de 4 mm, 0,6 em par caractère.
        let lines = dxf_lines(c);
        let nl = lines.len() as f64;
        let widths: Vec<f64> = lines
            .iter()
            .map(|l| l.chars().count() as f64 * 2.4)
            .collect();
        let block = |k: usize| {
            to_file((
                cx - widths[k] / 2.0,
                cy + (nl - 1.0) * 3.0 - k as f64 * 6.0 - 2.0,
            ))
        };
        let by_text: Vec<&DxfText> = texts.iter().filter(|t| t.layer == "84").collect();
        for (k, line) in lines.iter().enumerate() {
            let (ex, ey) = block(k);
            assert!(
                by_text
                    .iter()
                    .any(|t| t.text == *line && near(t.x, ex, 2e-3) && near(t.y, ey, 2e-3)),
                "{c:?}\nDXF : « {line} » attendu en ({ex} ; {ey}) : {by_text:?}"
            );
        }
        // Rempli : texte anglais au milieu du bord, glissement calculé avec le bloc de texte (largeur
        // 2,4 mm par caractère, 6 mm par ligne) pour obstacle.
        let widest = widths.iter().cloned().fold(0.0, f64::max);
        let obstacle: Rect = (
            (cx - widest / 2.0, cy - nl * 3.0),
            (cx + widest / 2.0, cy + nl * 3.0),
        );
        let en: Vec<_> = c
            .folds
            .iter()
            .map(|(i, v)| {
                let text = rempli_text(Lang::En, *v);
                let (_, _, centre, _) = expected_center(c, *i, text.chars().count(), &[obstacle]);
                (text, centre)
            })
            .collect();
        let over: Vec<&&DxfText> = by_text
            .iter()
            .filter(|t| t.text.starts_with("Fold-over "))
            .collect();
        assert_eq!(over.len(), en.len(), "{c:?}\n{by_text:?}");
        for (text, centre) in &en {
            let (ex, ey) = to_file((
                centre.0 - text.chars().count() as f64 * 2.4 / 2.0,
                centre.1 - 2.0,
            ));
            assert!(
                over.iter()
                    .any(|t| t.text == *text && near(t.x, ex, 2e-3) && near(t.y, ey, 2e-3)),
                "{c:?}\nDXF : « {text} » attendu en ({ex} ; {ey}) : {over:?}"
            );
        }
        assert!(!dxf.contains("Rempli") && !dxf.contains("Coupe nette"));
    }

    n.sequences += 1;
    match c.kind {
        Kind::Rempli => n.rempli += 1,
        Kind::NetCut => n.net_cut += 1,
        Kind::Plain => n.plain += 1,
    }
    n.langs_en += usize::from(c.lang == Lang::En);
    n.thin += usize::from(c.thin);
    n.modes_cut_only += usize::from(content.piece_mode == PieceExportMode::CutOnly);
    n.modes_sewing_only += usize::from(content.piece_mode == PieceExportMode::SewingOnly);
}

#[test]
fn campagne_adverse_libelles_rempli_et_coupe_nette() {
    let mut rng = Rng(0x5EED_0C5C_A11E_B0B5);
    let mut n = Counters::default();
    for i in 0..SEQUENCES {
        let case = gen_case(&mut rng, i);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&case, &mut n)));
        if let Err(e) = result {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default();
            panic!("séquence {i} : {msg}");
        }
    }
    eprintln!("{n:#?}");
    // Les contrôles forts se sont réellement exécutés.
    assert_eq!(n.sequences as u64, SEQUENCES);
    assert!(n.rempli >= 60 && n.net_cut >= 30 && n.plain >= 10, "{n:?}");
    assert!(n.rotated_pdf >= 20 && n.thin >= 10, "{n:?}");
    assert!(n.shifted >= 5, "glissements : {n:?}");
    assert!(n.filtered_out >= 10 && n.langs_en >= 40, "{n:?}");
    assert!(n.modes_cut_only >= 10 && n.modes_sewing_only >= 10, "{n:?}");
    assert!(n.labels_checked >= 300, "{n:?}");
}

//! Campagne adverse (graine fixe) : export des pinces et de l'étiquette de pièce dans les quatre
//! formats. Chaque séquence construit une pièce rectangulaire au hasard (position, taille, 0 à 3
//! pinces, fiche vide, partielle ou complète, accents, ponctuation, droit-fil, pince non
//! calculable, calque masqué, filtre de contenu) par `Project::apply`, l'exporte, puis relit les
//! fichiers avec les lecteurs indépendants de `common/`.
//!
//! Oracle indépendant du code d'export :
//! - jambes : recalculées par Pythagore √(longueur² + (ouverture / 2)²) à partir des **paramètres
//!   tirés au hasard** ; extrémités attendues par rapport au coin bas-gauche du contour relu **dans
//!   le même fichier** (jamais d'après la scène) ;
//! - étiquette : lignes réécrites ici (`expected_lines`), translittération HPGL et caractères de la police du PDF
//!   réécrites ici ; lecture du texte dans chaque format, présence et ordre.
//!
//! Des compteurs prouvent que chaque contrôle fort a réellement été exécuté.
//!
//! Honnêteté : aucune ouverture dans un logiciel tiers, aucun traçage ni impression réels.

mod common;

use common::coupe::{attr, mm_pt, read_pdf as read_strokes, read_svg, PdfStroke};
use common::dxf::{self as rd, Ent};
use common::dxf_models::rect_piece;
use common::hpgl::read_hpgl;
use common::Rng;
use pf_core::{Command, PathRole, PieceCategory, PieceId, PointRef, Project, SheetId};
use pf_export::dxf::{export_dxf, DxfIssueKind};
use pf_export::hpgl::export_hpgl;
use pf_export::pdf::export_pdf;
use pf_export::svg::export_svg;
use pf_export::{OutputCategory as C, OutputContent, PageFormat, PageSetup, Scene};
use pf_geometry::{PointMm, Side};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

const SEQUENCES: u64 = 180;
const _: () = assert!(SEQUENCES >= 150);

/// Segment (sommet, pied) en mm.
type Seg = ((f64, f64), (f64, f64));

static LEG_CHECKS: AtomicUsize = AtomicUsize::new(0);
static LABEL_CHECKS: AtomicUsize = AtomicUsize::new(0);
static DXF_CHECKS: AtomicUsize = AtomicUsize::new(0);
static HPGL_CHECKS: AtomicUsize = AtomicUsize::new(0);
static PDF_CHECKS: AtomicUsize = AtomicUsize::new(0);
static WARNING_CHECKS: AtomicUsize = AtomicUsize::new(0);
static HIDDEN_CHECKS: AtomicUsize = AtomicUsize::new(0);
static EMPTY_CARD_CHECKS: AtomicUsize = AtomicUsize::new(0);
static FILTER_CHECKS: AtomicUsize = AtomicUsize::new(0);
static LEGS_SEEN: AtomicUsize = AtomicUsize::new(0);
static LINES_SEEN: AtomicUsize = AtomicUsize::new(0);

macro_rules! ck {
    ($seq:expr, $cond:expr, $($arg:tt)+) => {
        if !$cond {
            panic!("séquence {} : {}", $seq, format!($($arg)+));
        }
    };
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ---------------------------------------------------------------------------
// Tirage
// ---------------------------------------------------------------------------

/// Pince tirée : abscisse du sommet relative au coin, longueur, ouverture (mm).
#[derive(Debug, Clone, Copy)]
struct DartSpec {
    x: f64,
    len: f64,
    open: f64,
}

impl DartSpec {
    /// Jambe : √(longueur² + (ouverture / 2)²).
    fn leg(&self) -> f64 {
        (self.len * self.len + (self.open / 2.0) * (self.open / 2.0)).sqrt()
    }
}

#[derive(Debug, Clone)]
struct Card {
    name: String,
    code: Option<String>,
    quantity: u32,
    pair: bool,
    material: Option<String>,
    category: Option<PieceCategory>,
    comment: Option<String>,
}

/// Mots de l'alphabet de test : accents du français, « Ž » (Latin étendu A : dans la police
/// du PDF), « Ж » (cyrillique : absent de la police du PDF, écrit « ? »), ponctuation d'XML
/// (& < >), parenthèses (PDF) et point-virgule (HPGL).
const WORDS: [&str; 13] = [
    "Dos", "é", "è", "à", "ç", "ü", "Ž", "Ж", "a&b", "x;y", "(a)", "<b>", "ô",
];

fn phrase(rng: &mut Rng, max_words: u64) -> String {
    let n = 1 + rng.below(max_words);
    (0..n)
        .map(|_| rng.pick(&WORDS))
        .collect::<Vec<_>>()
        .join(" ")
}

fn random_card(rng: &mut Rng) -> Option<Card> {
    match rng.below(4) {
        // Fiche jamais touchée : nom seul.
        0 => None,
        // Champs vides ou blancs, quantité 1 : rien à écrire.
        1 => Some(Card {
            name: "Dos".into(),
            code: Some("  ".into()),
            quantity: 1,
            pair: false,
            material: Some(String::new()),
            category: None,
            comment: Some(" \n \t\n".into()),
        }),
        // Quantité seule.
        2 => Some(Card {
            name: phrase(rng, 3),
            code: None,
            quantity: 2 + rng.below(5) as u32,
            pair: rng.below(2) == 0,
            material: None,
            category: None,
            comment: None,
        }),
        _ => {
            let long = |rng: &mut Rng| {
                if rng.below(4) == 0 {
                    // Ligne de plus de 60 caractères : coupée par « … ».
                    format!("{} {}", phrase(rng, 6), "mot ".repeat(20))
                } else {
                    phrase(rng, 5)
                }
            };
            let comment = if rng.below(3) == 0 {
                None
            } else {
                let n = rng.below(9);
                let lines: Vec<String> = (0..n)
                    .map(|_| match rng.below(4) {
                        0 => String::new(),
                        1 => format!("   {}  ", phrase(rng, 3)),
                        _ => long(rng),
                    })
                    .collect();
                Some(lines.join("\n"))
            };
            Some(Card {
                name: phrase(rng, 3),
                code: (rng.below(3) != 0).then(|| phrase(rng, 2)),
                quantity: 1 + rng.below(4) as u32,
                pair: rng.below(2) == 0,
                material: (rng.below(3) != 0).then(|| phrase(rng, 3)),
                category: (rng.below(3) != 0).then(|| rng.pick(&PieceCategory::ALL)),
                comment,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Oracle : lignes d'étiquette, réécrites ici
// ---------------------------------------------------------------------------

fn clip(t: &str) -> String {
    let t = t.trim();
    if t.chars().count() <= 60 {
        t.to_string()
    } else {
        format!("{}…", t.chars().take(59).collect::<String>())
    }
}

fn category_fr(c: PieceCategory) -> &'static str {
    match c {
        PieceCategory::Fabric => "Tissu",
        PieceCategory::Lining => "Doublure",
        PieceCategory::Interfacing => "Entoilage",
        PieceCategory::Leather => "Cuir",
        PieceCategory::Reinforcement => "Renfort",
    }
}

fn category_en(c: PieceCategory) -> &'static str {
    match c {
        PieceCategory::Fabric => "fabric",
        PieceCategory::Lining => "lining",
        PieceCategory::Interfacing => "interfacing",
        PieceCategory::Leather => "leather",
        PieceCategory::Reinforcement => "reinforcement",
    }
}

fn filled(o: &Option<String>) -> Option<&str> {
    o.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn expected_lines(c: &Card) -> Vec<String> {
    let (code, material, comment) = (filled(&c.code), filled(&c.material), filled(&c.comment));
    if code.is_none()
        && material.is_none()
        && comment.is_none()
        && c.category.is_none()
        && c.quantity == 1
        && !c.pair
    {
        return Vec::new();
    }
    let mut v = vec![clip(&c.name)];
    if let Some(x) = code {
        v.push(clip(&format!("Code : {x}")));
    }
    v.push(format!(
        "Quantité : {}{}",
        c.quantity,
        if c.pair {
            " (paire, ×2 en miroir)"
        } else {
            ""
        }
    ));
    if let Some(x) = material {
        v.push(clip(&format!("Matière : {x}")));
    }
    if let Some(k) = c.category {
        v.push(format!("Catégorie : {}", category_fr(k)));
    }
    if let Some(x) = comment {
        v.extend(
            x.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .take(6)
                .map(clip),
        );
    }
    v
}

/// HPGL : ASCII ; accents retirés, `;` → `,`, `×` → `x`, `…` → `...`, autre → `?`.
fn hpgl_ascii(t: &str) -> String {
    t.chars()
        .map(|ch| match ch {
            ';' => ",".to_string(),
            'é' | 'è' => "e".into(),
            'à' => "a".into(),
            'ç' => "c".into(),
            'ü' => "u".into(),
            'ô' => "o".into(),
            '×' => "x".into(),
            '…' => "...".into(),
            c if c.is_ascii() => c.to_string(),
            _ => "?".into(),
        })
        .collect()
}

/// PDF à police embarquée : un caractère absent du fichier de police du dépôt (lu par un lecteur
/// de test) est écrit « ? ».
fn pdf_text(t: &str) -> String {
    static FONT: std::sync::OnceLock<common::pdf_text::Ttf> = std::sync::OnceLock::new();
    let font = FONT.get_or_init(common::pdf_text::repo_font);
    t.chars()
        .map(|c| if font.advance_of(c).is_some() { c } else { '?' })
        .collect()
}

fn unescape(t: &str) -> String {
    t.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

struct Built {
    project: Project,
    sheet: SheetId,
    piece: PieceId,
    darts: Vec<DartSpec>,
    /// Pinces non calculables (sommet sorti de la pièce) : indices dans `darts`.
    invalid: Vec<usize>,
    card: Card,
    hidden: bool,
}

fn build(rng: &mut Rng, seq: u64) -> Built {
    let (w, h) = (rng.range(120.0, 400.0), rng.range(90.0, 300.0));
    let (ox, oy) = (rng.range(-300.0, 300.0), rng.range(-300.0, 300.0));
    let mut p = Project::new("Adverse pince étiquette");
    let s = p.sheets[0].id;
    let (piece, outline) = rect_piece(
        &mut p,
        s,
        (ox, oy),
        (w, h),
        "Dos",
        PathRole::CuttingContour,
        false,
    );
    // Un point libre visible : la feuille n'est jamais vide, même contour masqué.
    p.apply(Command::AddPoint {
        sheet: s,
        at: PointMm::new(ox - 60.0, oy - 60.0),
    })
    .unwrap();
    // Droit-fil au hasard : le centre de l'étiquette en dépend.
    if rng.below(3) == 0 {
        let g = p
            .apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(PointMm::new(ox + w * 0.9, oy + h * 0.2)),
                to: PointRef::New(PointMm::new(ox + w * 0.9, oy + h * 0.8)),
            })
            .unwrap()
            .entities[0];
        p.apply(Command::SetPathRole {
            sheet: s,
            entity: g,
            role: PathRole::Grainline,
        })
        .unwrap();
        let outline_edges = p.piece(piece).unwrap().outline.clone();
        p.apply(Command::SetPieceOutline {
            piece,
            outline: outline_edges,
            grainline: Some(g),
        })
        .unwrap();
    }
    // Pinces sur le bord bas, réparties sur la largeur.
    let n = rng.below(4) as usize;
    let mut darts = Vec::new();
    let mut apexes = Vec::new();
    for i in 0..n {
        let slot = w / n as f64;
        let open = rng.range(2.0, (slot / 2.5).clamp(2.5, 18.0));
        let len = rng.range(open.max(8.0), (h * 0.6).min(95.0));
        let x = slot * (i as f64 + 0.5);
        let apex = p
            .apply(Command::AddPoint {
                sheet: s,
                at: PointMm::new(ox + x, oy + len),
            })
            .unwrap()
            .points[0];
        let toward = p
            .apply(Command::AddPoint {
                sheet: s,
                at: PointMm::new(ox + x, oy),
            })
            .unwrap()
            .points[0];
        p.apply(Command::AddPieceDart {
            piece,
            apex: PointRef::Existing(apex),
            toward: PointRef::Existing(toward),
            length_mm: len,
            opening_mm: open,
            fixed: if rng.below(2) == 0 {
                Side::Left
            } else {
                Side::Right
            },
        })
        .unwrap();
        darts.push(DartSpec { x, len, open });
        apexes.push(apex);
    }
    // Une pince sur six devient non calculable : son sommet sort de la pièce.
    let mut invalid = Vec::new();
    if n > 0 && rng.below(6) == 0 {
        let k = rng.below(n as u64) as usize;
        p.apply(Command::MovePoint {
            sheet: s,
            point: apexes[k],
            to: PointMm::new(ox - 400.0, oy + h + 400.0),
        })
        .unwrap();
        invalid.push(k);
    }
    // Fiche.
    let card = match random_card(rng) {
        None => Card {
            name: "Dos".into(),
            code: None,
            quantity: 1,
            pair: false,
            material: None,
            category: None,
            comment: None,
        },
        Some(c) => {
            p.apply(Command::SetPieceCard {
                piece,
                name: c.name.clone(),
                code: c.code.clone(),
                quantity: c.quantity,
                mirrored_pair: c.pair,
                material: c.material.clone(),
                category: c.category,
                comment: c.comment.clone(),
            })
            .unwrap();
            c
        }
    };
    // Calque masqué : la pièce n'est plus exportée avec sa pince ni son étiquette.
    let hidden = rng.below(8) == 0;
    if hidden {
        let layer = p
            .apply(Command::AddLayer {
                sheet: s,
                name: format!("Masqué {seq}"),
            })
            .unwrap()
            .layers[0];
        p.apply(Command::SetObjectsLayer {
            sheet: s,
            points: Vec::new(),
            entities: outline,
            layer: Some(layer),
        })
        .unwrap();
        p.apply(Command::SetLayerVisible {
            sheet: s,
            layer,
            visible: false,
        })
        .unwrap();
    }
    Built {
        project: p,
        sheet: s,
        piece,
        darts,
        invalid,
        card,
        hidden,
    }
}

// ---------------------------------------------------------------------------
// Contrôles
// ---------------------------------------------------------------------------

/// Jambes attendues par rapport au coin bas-gauche du contour (mm) : (sommet, pied), pieds à
/// ± ouverture / 2 de l'abscisse du sommet, sur le bord bas (y = 0).
fn expected_legs(darts: &[DartSpec], invalid: &[usize]) -> Vec<Seg> {
    let mut v = Vec::new();
    for (i, d) in darts.iter().enumerate() {
        if invalid.contains(&i) {
            continue;
        }
        let apex = (d.x, d.len);
        for sgn in [-1.0, 1.0] {
            v.push((apex, (d.x + sgn * d.open / 2.0, 0.0)));
        }
    }
    v
}

/// Chaque jambe attendue se retrouve (extrémités non orientées) parmi les jambes lues, une fois.
fn legs_match(seq: u64, what: &str, want: &[Seg], found: &[Seg], tol: f64) {
    ck!(
        seq,
        want.len() == found.len(),
        "{what} : {} jambes lues, {} attendues ({found:?})",
        found.len(),
        want.len()
    );
    let close = |a: (f64, f64), b: (f64, f64)| near(a.0, b.0, tol) && near(a.1, b.1, tol);
    let mut used = vec![false; found.len()];
    for (a, b) in want {
        let hit = found.iter().enumerate().position(|(i, (c, d))| {
            !used[i] && ((close(*a, *c) && close(*b, *d)) || (close(*a, *d) && close(*b, *c)))
        });
        match hit {
            Some(i) => used[i] = true,
            None => panic!("séquence {seq} : {what} : jambe {a:?} → {b:?} absente de {found:?}"),
        }
    }
    LEGS_SEEN.fetch_add(want.len(), Relaxed);
}

fn run(seq: u64) {
    let mut rng = Rng(0x7A11_E5C0_FFEE_0069 ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    for _ in 0..4 {
        rng.next();
    }
    let b = build(&mut rng, seq);
    let content = OutputContent::default()
        .with(C::Darts, rng.below(4) != 0)
        .with(C::PieceLabels, rng.below(4) != 0);
    let want_lines = expected_lines(&b.card);
    let want_legs = expected_legs(&b.darts, &b.invalid);
    let before = b.project.clone();

    // ----- scène -----
    let full = Scene::from_sheet(&b.project, b.sheet).unwrap();
    ck!(seq, b.project == before, "le projet a changé");
    if b.hidden {
        ck!(
            seq,
            full.darts.is_empty() && full.piece_labels.is_empty() && full.dart_warnings.is_empty(),
            "pièce masquée exportée"
        );
        let svg = export_svg(&full).unwrap();
        ck!(
            seq,
            !svg.contains("data-role=\"pince\"") && !svg.contains("data-role=\"etiquette-piece\""),
            "pièce masquée écrite dans le SVG"
        );
        // Le DXF ne suit pas les calques : la pièce y est, avec sa pince et sa fiche.
        let dxf = export_dxf(&b.project).unwrap();
        let d = rd::parse(&dxf.text).unwrap();
        ck!(
            seq,
            dxf_dart_lines(&d).len() == want_legs.len(),
            "DXF d'une pièce masquée"
        );
        HIDDEN_CHECKS.fetch_add(1, Relaxed);
        return;
    }
    ck!(
        seq,
        full.piece_labels.len() == usize::from(!want_lines.is_empty()),
        "{} étiquettes en scène, {} attendues",
        full.piece_labels.len(),
        usize::from(!want_lines.is_empty())
    );
    if let Some(l) = full.piece_labels.first() {
        ck!(seq, l.lines == want_lines, "{:?} ≠ {want_lines:?}", l.lines);
    }
    if want_lines.is_empty() {
        EMPTY_CARD_CHECKS.fetch_add(1, Relaxed);
    }
    ck!(
        seq,
        full.dart_warnings.len() == b.invalid.len(),
        "{} avertissements, {} pinces non calculables",
        full.dart_warnings.len(),
        b.invalid.len()
    );
    if !b.invalid.is_empty() {
        ck!(
            seq,
            full.dart_warnings
                .iter()
                .all(|w| !w.reason.is_empty() && w.to_string().contains("non exportée")),
            "avertissement illisible"
        );
        WARNING_CHECKS.fetch_add(1, Relaxed);
    }
    let scene = content.apply(&full).unwrap();
    let keep_darts = content.includes(C::Darts);
    let keep_labels = content.includes(C::PieceLabels);
    let w_legs: &[Seg] = if keep_darts { &want_legs } else { &[] };
    let w_lines: &[String] = if keep_labels && !want_lines.is_empty() {
        &want_lines
    } else {
        &[]
    };
    ck!(
        seq,
        scene.darts.is_empty() == !(keep_darts && !want_legs.is_empty()),
        "filtre des pinces"
    );
    FILTER_CHECKS.fetch_add(1, Relaxed);

    // ----- SVG -----
    let svg = export_svg(&scene).unwrap();
    let doc = read_svg(&svg);
    let cut = doc.with_role("coupe");
    ck!(
        seq,
        cut.len() == 4,
        "contour de coupe : {} côtés",
        cut.len()
    );
    let min_x = cut
        .iter()
        .flat_map(|i| i.pts.iter().map(|p| p.0))
        .fold(f64::INFINITY, f64::min);
    let max_y = cut
        .iter()
        .flat_map(|i| i.pts.iter().map(|p| p.1))
        .fold(f64::NEG_INFINITY, f64::max);
    let svg_legs: Vec<_> = doc
        .with_role("pince")
        .iter()
        .map(|i| {
            let m = |p: (f64, f64)| (p.0 - min_x, max_y - p.1);
            (m(i.pts[0]), m(i.pts[1]))
        })
        .collect();
    legs_match(seq, "SVG", w_legs, &svg_legs, 2e-3);
    for (i, l) in doc.with_role("pince").iter().enumerate() {
        let d = (l.pts[1].0 - l.pts[0].0).hypot(l.pts[1].1 - l.pts[0].1);
        let want = b
            .darts
            .iter()
            .enumerate()
            .filter(|(k, _)| !b.invalid.contains(k));
        let want: Vec<f64> = want.map(|(_, d)| d.leg()).collect();
        // Chaque jambe lue a la longueur de Pythagore d'une des pinces tirées.
        ck!(
            seq,
            want.iter().any(|w| near(d, *w, 2e-3)),
            "jambe {i} de {d} mm : Pythagore attend {want:?}"
        );
    }
    LEG_CHECKS.fetch_add(1, Relaxed);
    let svg_lines: Vec<String> = svg
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("<text") && l.contains("data-role=\"etiquette-piece\""))
        .map(|l| {
            let inner = l.split_once('>').unwrap().1;
            unescape(inner.rsplit_once("</text>").unwrap().0)
        })
        .collect();
    ck!(
        seq,
        svg_lines == w_lines,
        "SVG : {svg_lines:?} ≠ {w_lines:?}"
    );
    LINES_SEEN.fetch_add(w_lines.len(), Relaxed);
    // Le centre du bloc est le même pour toutes les lignes (texte centré).
    let xs: Vec<String> = svg
        .lines()
        .filter(|l| l.contains("etiquette-piece"))
        .map(|l| attr(l, "x").unwrap())
        .collect();
    ck!(
        seq,
        xs.windows(2).all(|w| w[0] == w[1]),
        "lignes non alignées"
    );
    LABEL_CHECKS.fetch_add(1, Relaxed);

    // ----- PDF -----
    let pdf = export_pdf(std::slice::from_ref(&scene)).unwrap();
    let strokes = read_strokes(&pdf.bytes);
    let solid: Vec<&PdfStroke> = strokes
        .iter()
        .filter(|s| s.dash_pt.is_empty() && near(s.width_pt, mm_pt(0.6), 1e-3) && s.pts.len() == 2)
        .collect();
    ck!(seq, solid.len() >= 4, "contour du PDF");
    let (px, py) = solid
        .iter()
        .flat_map(|s| s.pts.iter())
        .fold((f64::INFINITY, f64::INFINITY), |a, p| {
            (a.0.min(p.0), a.1.min(p.1))
        });
    let dash = [8.0, 1.5, 1.5, 1.5, 1.5, 1.5].map(mm_pt);
    let pdf_legs: Vec<_> = strokes
        .iter()
        .filter(|s| {
            s.dash_pt.len() == 6 && s.dash_pt.iter().zip(&dash).all(|(a, b)| near(*a, *b, 1e-3))
        })
        .map(|s| {
            let m = |p: (f64, f64)| ((p.0 - px) * 25.4 / 72.0, (p.1 - py) * 25.4 / 72.0);
            (m(s.pts[0]), m(s.pts[1]))
        })
        .collect();
    legs_match(seq, "PDF", w_legs, &pdf_legs, 2e-2);
    let pages = common::read_pdf(&pdf.bytes);
    let pdf_lines: Vec<String> = w_lines.iter().map(|l| pdf_text(l)).collect();
    if !pdf_lines.is_empty() {
        ck!(
            seq,
            pages
                .iter()
                .any(|pg| pg.strings.windows(pdf_lines.len()).any(|w| w == pdf_lines)),
            "PDF : bloc d'étiquette {pdf_lines:?} absent"
        );
    } else {
        ck!(
            seq,
            pages
                .iter()
                .all(|pg| pg.strings.iter().all(|t| !t.starts_with("Quantité"))),
            "PDF : étiquette écrite à tort"
        );
    }
    PDF_CHECKS.fetch_add(1, Relaxed);

    // ----- HPGL -----
    let page = PageSetup {
        format: PageFormat::A0,
        ..Default::default()
    };
    let plot = export_hpgl(&scene, &page).unwrap();
    ck!(seq, plot.text.is_ascii(), "HPGL non ASCII");
    let r = read_hpgl(&plot.text).unwrap();
    let cut_pts: Vec<(i64, i64)> = r
        .strokes
        .iter()
        .filter(|s| s.pen == 1)
        .flat_map(|s| s.pts.clone())
        .collect();
    let (hx, hy) = (
        cut_pts.iter().map(|p| p.0).min().unwrap(),
        cut_pts.iter().map(|p| p.1).min().unwrap(),
    );
    let hpgl_legs: Vec<_> = r
        .strokes
        .iter()
        .filter(|s| s.pen == 9)
        .map(|s| {
            let m = |p: (i64, i64)| ((p.0 - hx) as f64 / 40.0, (p.1 - hy) as f64 / 40.0);
            (m(s.pts[0]), m(*s.pts.last().unwrap()))
        })
        .collect();
    legs_match(seq, "HPGL", w_legs, &hpgl_legs, 0.03);
    let hpgl_lines: Vec<String> = r.labels.iter().map(|l| l.text.clone()).collect();
    let want_hpgl: Vec<String> = w_lines.iter().map(|l| hpgl_ascii(l)).collect();
    ck!(
        seq,
        hpgl_lines == want_hpgl,
        "HPGL : {hpgl_lines:?} ≠ {want_hpgl:?}"
    );
    ck!(
        seq,
        r.labels
            .iter()
            .all(|l| l.pen == 7 && l.at.0 >= 0 && l.at.1 >= 0),
        "étiquette HPGL hors emprise ou mauvaise plume"
    );
    ck!(seq, plot.report.labels == want_hpgl.len(), "rapport HPGL");
    HPGL_CHECKS.fetch_add(1, Relaxed);

    // ----- DXF (pas de filtre de contenu : toutes les pinces calculables, toute la fiche) -----
    let dxf = export_dxf(&b.project).unwrap();
    let d = rd::parse(&dxf.text).unwrap();
    let dxf_legs = dxf_dart_lines(&d);
    let boundary: Vec<(f64, f64)> = d
        .blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Polyline { layer, pts, .. } if layer == "1" => Some(pts.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    ck!(seq, !boundary.is_empty(), "contour DXF");
    let (dx, dy) = (
        boundary.iter().map(|p| p.0).fold(f64::INFINITY, f64::min),
        boundary.iter().map(|p| p.1).fold(f64::INFINITY, f64::min),
    );
    let rel: Vec<_> = dxf_legs
        .iter()
        .map(|(a, b)| ((a.0 - dx, a.1 - dy), (b.0 - dx, b.1 - dy)))
        .collect();
    legs_match(seq, "DXF", &want_legs, &rel, 2e-3);
    ck!(
        seq,
        dxf.report
            .issues
            .iter()
            .filter(|i| i.kind == DxfIssueKind::DartUnreadable)
            .map(|i| i.count)
            .sum::<usize>()
            == b.invalid.len(),
        "avertissements DXF des pinces non calculables"
    );
    ck!(
        seq,
        dxf.text.contains("\n90\n") == !want_legs.is_empty(),
        "couche 90 déclarée à tort ou absente"
    );
    // Texte de la fiche, tel qu'il est stocké dans le modèle (ordre : nom, code, quantité,
    // matière, paire, catégorie, commentaire ; 80 caractères au plus par ligne).
    let stored = b.project.piece(b.piece).unwrap();
    let mut want_dxf = vec![format!("Piece Name: {}", stored.name)];
    if let Some(c) = &stored.code {
        want_dxf.push(format!("Code: {c}"));
    }
    want_dxf.push(format!("Quantity: {}", stored.quantity));
    if let Some(m) = &stored.material {
        want_dxf.push(format!("Material: {m}"));
    }
    if stored.mirrored_pair {
        want_dxf.push("Mirrored pair: yes".into());
    }
    if let Some(k) = stored.category {
        want_dxf.push(format!("Category: {}", category_en(k)));
    }
    if let Some(c) = &stored.comment {
        want_dxf.extend(
            c.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .take(6)
                .map(|l| format!("Comment: {l}")),
        );
    }
    let want_dxf: Vec<String> = want_dxf
        .into_iter()
        .map(|l| l.chars().take(80).collect())
        .collect();
    let got: Vec<String> = d
        .blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Text { layer, text, .. } if layer == "84" => Some(text.clone()),
            _ => None,
        })
        .collect();
    ck!(seq, got == want_dxf, "DXF : {got:?} ≠ {want_dxf:?}");
    DXF_CHECKS.fetch_add(1, Relaxed);
}

fn dxf_dart_lines(d: &rd::Dxf) -> Vec<Seg> {
    d.blocks
        .iter()
        .flat_map(|(_, e)| e)
        .filter_map(|e| match e {
            Ent::Line { layer, a, b } if layer == "90" => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

#[test]
fn campagne_adverse_export_pince_etiquette() {
    for seq in 0..SEQUENCES {
        run(seq);
    }
    // Les contrôles forts ont réellement été exécutés (garde contre une campagne creuse).
    let n = |a: &AtomicUsize| a.load(Relaxed);

    assert!(n(&LEG_CHECKS) >= 120, "SVG : {}", n(&LEG_CHECKS));
    assert!(n(&LABEL_CHECKS) >= 120);
    assert!(n(&DXF_CHECKS) >= 120);
    assert!(n(&HPGL_CHECKS) >= 120);
    assert!(n(&PDF_CHECKS) >= 120);
    assert!(n(&FILTER_CHECKS) >= 120);
    assert!(n(&WARNING_CHECKS) >= 3, "pinces non calculables");
    assert!(n(&HIDDEN_CHECKS) >= 5, "pièces masquées");
    assert!(n(&EMPTY_CARD_CHECKS) >= 20, "fiches sans étiquette");
    assert!(n(&LEGS_SEEN) >= 200, "jambes vérifiées : {}", n(&LEGS_SEEN));
    assert!(n(&LINES_SEEN) >= 200, "lignes lues : {}", n(&LINES_SEEN));
}

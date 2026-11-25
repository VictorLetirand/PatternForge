//! US-09.8b — export des mesures par taille (CSV, PDF). Rectangle 400 × 600 en base M, gamme
//! XS S M L XL ; écarts cumulés `S (−5 ; −10)`, `L (+5 ; +10)`, `XL (+10 ; +22)`, `XS (−9 ; −20)` :
//! A fixe, B `(dx ; 0)`, C `(dx ; dy)`, D `(0 ; dy)`, donc la taille `T` est un rectangle
//! `(400 + dx) × (600 + dy)` : périmètres 1942 / 1970 / 2000 / 2030 / 2064, aires 226 780 /
//! 233 050 / 240 000 / 247 050 / 255 020. Les CSV attendus sont **écrits à la main** (jamais relus
//! du code testé) ; les pas moyens et pourcentages ont été calculés une fois à partir des
//! définitions (`(b − a) / a × 100`, moyenne des pas) ; les lecteurs PDF et CSV sont ceux,
//! indépendants, de `common/`.

mod common;

use common::csv_reader::parse;
use common::report_pdf::{media_boxes, pages};
use pf_core::*;
use pf_export::report::{csv_text, ReportFormat};
use pf_export::size_measures_export::{export_size_measures, ALERT_MARK};
use pf_export::ExportError;
use pf_geometry::PointMm;
use pf_i18n::Lang;
use std::fs;
use std::path::PathBuf;

const DELTAS: [(&str, f64, f64); 5] = [
    ("XS", -9.0, -20.0),
    ("S", -5.0, -10.0),
    ("M", 0.0, 0.0),
    ("L", 5.0, 10.0),
    ("XL", 10.0, 22.0),
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Dossier de travail **dans `target/`** (jamais à la racine du disque ni dans /tmp).
fn work_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mesures_export_{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn listing(dir: &PathBuf) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn json(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn seg(p: &mut Project, sheet: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

fn point_at(p: &Project, sheet: SheetId, at: PointMm) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| pf_geometry::distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
        .unwrap_or_else(|| panic!("aucun point en {at:?}"))
        .id
}

fn grade(p: &mut Project, sheet: SheetId, point: PointId, size_index: usize, dx: f64, dy: f64) {
    let size = p.sizes[0].sizes[size_index].id;
    p.apply(Command::SetPointGrading {
        sheet,
        point,
        size,
        dx_mm: dx,
        dy_mm: dy,
    })
    .unwrap();
}

fn add_range(p: &mut Project, name: &str, labels: &[&str], base: usize) -> SizeRangeId {
    p.apply(Command::AddSizeRange {
        name: name.into(),
        kind: SizeKind::Alphanumeric,
        labels: labels.iter().map(|s| s.to_string()).collect(),
        base,
    })
    .unwrap();
    p.sizes.last().unwrap().id
}

struct Fx {
    p: Project,
    range: SizeRangeId,
}

/// Rectangle 400 × 600, pièce « Devant », coins gradués comme dans l'en-tête ; `labels` = noms
/// des tailles (dans l'ordre XS S M L XL des écarts).
fn rect_with(name: &str, range_name: &str, labels: &[&str; 5]) -> Fx {
    let mut p = Project::new(name);
    let sheet = p.sheets[0].id;
    let range = add_range(&mut p, range_name, labels, 2);
    let ab = seg(&mut p, sheet, pt(0.0, 0.0), pt(400.0, 0.0));
    let bc = seg(&mut p, sheet, pt(400.0, 0.0), pt(400.0, 600.0));
    let cd = seg(&mut p, sheet, pt(400.0, 600.0), pt(0.0, 600.0));
    let da = seg(&mut p, sheet, pt(0.0, 600.0), pt(0.0, 0.0));
    p.apply(Command::ExtractPiece {
        sheet,
        source: PieceSource::Paths {
            entities: vec![ab, bc, cd, da],
        },
        name: "Devant".into(),
        detached: false,
    })
    .unwrap();
    let (b, c, d) = (
        point_at(&p, sheet, pt(400.0, 0.0)),
        point_at(&p, sheet, pt(400.0, 600.0)),
        point_at(&p, sheet, pt(0.0, 600.0)),
    );
    for (i, (_, dx, dy)) in DELTAS.iter().enumerate().filter(|(i, _)| *i != 2) {
        grade(&mut p, sheet, b, i, *dx, 0.0);
        grade(&mut p, sheet, c, i, *dx, *dy);
        grade(&mut p, sheet, d, i, 0.0, *dy);
    }
    Fx { p, range }
}

fn rect() -> Fx {
    rect_with("Export mesures", "Femme", &["XS", "S", "M", "L", "XL"])
}

fn csv_of(f: &Fx, lang: Lang) -> String {
    let out = export_size_measures(&f.p, f.range, ReportFormat::Csv, lang).unwrap();
    assert_eq!(out.format, ReportFormat::Csv);
    assert_eq!(out.page_count, None);
    String::from_utf8(out.bytes).expect("UTF-8")
}

fn pdf_of(f: &Fx, lang: Lang) -> Vec<u8> {
    let out = export_size_measures(&f.p, f.range, ReportFormat::Pdf, lang).unwrap();
    assert!(out.page_count.unwrap() >= 1);
    out.bytes
}

// ----- CSV : contenu complet écrit à la main --------------------------------------------------

#[test]
fn csv_fr_complet_ecrit_a_la_main() {
    let f = rect();
    let expected = [
        "\u{FEFF}#resume;Élément;Valeur",
        "resume;Projet;Export mesures",
        "resume;Gamme;Femme",
        "resume;Taille de base;M",
        "resume;Mesures;6",
        "resume;Évolutions irrégulières;0",
        "resume;Unités;Longueurs en mm, aires en mm². Pas moyen : moyenne des écarts entre tailles consécutives.",
        "resume;Précision;\"2 décimales ; cellule vide : non calculable dans cette taille\"",
        "resume;Avertissement;Mesures observées, lecture seule, non validées par une modéliste",
        "#mesure;Mesure;Unité;XS;S;M;L;XL;Pas moyen;Pas moyen (%);Alerte",
        "mesure;Périmètre · Devant;mm;1942.00;1970.00;2000.00;2030.00;2064.00;30.50;1.53;",
        "mesure;Aire (mm²) · Devant;mm²;226780.00;233050.00;240000.00;247050.00;255020.00;7060.00;2.98;",
        "mesure;Segment AB;mm;391.00;395.00;400.00;405.00;410.00;4.75;1.19;",
        "mesure;Segment BC;mm;580.00;590.00;600.00;610.00;622.00;10.50;1.76;",
        "mesure;Segment CD;mm;391.00;395.00;400.00;405.00;410.00;4.75;1.19;",
        "mesure;Segment DA;mm;580.00;590.00;600.00;610.00;622.00;10.50;1.76;",
        "",
    ]
    .join("\r\n");
    assert_eq!(csv_of(&f, Lang::Fr), expected);
}

#[test]
fn csv_en_complet_ecrit_a_la_main() {
    let f = rect();
    let expected = [
        "\u{FEFF}#resume;Item;Value",
        "resume;Project;Export mesures",
        "resume;Range;Femme",
        "resume;Base size;M",
        "resume;Measures;6",
        "resume;Irregular trends;0",
        "resume;Units;Lengths in mm, areas in mm². Mean step: average of the differences between consecutive sizes.",
        "resume;Precision;\"2 decimals; empty cell: cannot be computed in this size\"",
        "resume;Notice;Observed measurements, read-only, not validated by a pattern maker",
        "#mesure;Measure;Unit;XS;S;M;L;XL;Mean step;Mean step (%);Alert",
        "mesure;Perimeter · Devant;mm;1942.00;1970.00;2000.00;2030.00;2064.00;30.50;1.53;",
        "mesure;Area (mm²) · Devant;mm²;226780.00;233050.00;240000.00;247050.00;255020.00;7060.00;2.98;",
        "mesure;Segment AB;mm;391.00;395.00;400.00;405.00;410.00;4.75;1.19;",
        "mesure;Segment BC;mm;580.00;590.00;600.00;610.00;622.00;10.50;1.76;",
        "mesure;Segment CD;mm;391.00;395.00;400.00;405.00;410.00;4.75;1.19;",
        "mesure;Segment DA;mm;580.00;590.00;600.00;610.00;622.00;10.50;1.76;",
        "",
    ]
    .join("\r\n");
    assert_eq!(csv_of(&f, Lang::En), expected);
}

#[test]
fn csv_chaque_taille_lit_sa_propre_geometrie_pas_la_base() {
    // Garde-fou contre « la valeur de la base recopiée dans toutes les colonnes » : les cinq
    // périmètres sont distincts et dans l'ordre de la gamme.
    let csv = csv_of(&rect(), Lang::Fr);
    let line = csv
        .lines()
        .find(|l| l.starts_with("mesure;Périmètre"))
        .unwrap();
    let cells: Vec<&str> = line.split(';').collect();
    assert_eq!(
        &cells[3..8],
        &["1942.00", "1970.00", "2000.00", "2030.00", "2064.00"]
    );
}

// ----- Cellules illisibles ---------------------------------------------------------------------

/// Rectangle dont le coin C, en XS, passe sous AB : le contour se recoupe, l'aire de XS n'est
/// pas lisible (les autres tailles et les longueurs le restent).
fn bowtie_in_xs() -> Fx {
    let mut f = rect();
    let sheet = f.p.sheets[0].id;
    let c = point_at(&f.p, sheet, pt(400.0, 600.0));
    // XS : C passe de (391 ; 580) à (400 ; −100) (écart cumulé (0 ; −700)).
    grade(&mut f.p, sheet, c, 0, 0.0, -700.0);
    f
}

#[test]
fn cellule_illisible_vide_en_csv_et_tiret_en_pdf() {
    let f = bowtie_in_xs();
    let csv = csv_of(&f, Lang::Fr);
    assert!(
        csv.lines()
            .any(|l| l
                == "mesure;Aire (mm²) · Devant;mm²;;233050.00;240000.00;247050.00;255020.00;;;"),
        "{csv}"
    );
    // Pas de pas moyen ni de pourcentage devinés, jamais NaN.
    assert!(!csv.contains("NaN") && !csv.contains("inf"));
    // Le PDF écrit « — » à la place, sur la ligne de l'aire, à la colonne de XS.
    let bytes = pdf_of(&f, Lang::Fr);
    let all: Vec<_> = pages(&bytes).into_iter().flat_map(|p| p.texts).collect();
    let area = all
        .iter()
        .find(|t| t.text == "Aire (mm²) · Devant")
        .expect("ligne de l'aire");
    let on_line: Vec<_> = all
        .iter()
        .filter(|t| (t.y_mm - area.y_mm).abs() < 1e-6)
        .collect();
    let dashes: Vec<_> = on_line.iter().filter(|t| t.text == "—").collect();
    // XS, pas moyen, pas moyen en % : trois cellules illisibles ; les quatre autres valeurs écrites.
    assert_eq!(dashes.len(), 3, "{on_line:?}");
    for v in ["233050", "240000", "247050", "255020"] {
        assert!(on_line.iter().any(|t| t.text == v), "{v} : {on_line:?}");
    }
    let xs_x = on_line
        .iter()
        .filter(|t| t.text != "Aire (mm²) · Devant" && t.text != "mm²")
        .map(|t| t.x_mm)
        .fold(f64::MAX, f64::min);
    assert!(dashes.iter().any(|t| (t.x_mm - xs_x).abs() < 1e-6));
}

// ----- Injection de formule --------------------------------------------------------------------

#[test]
fn injection_de_formule_neutralisee_dans_tous_les_libelles() {
    let mut f = rect_with("=Projet", "@Gamme", &["=XS", "+S", "-M", "@L", "XL"]);
    // Base : « -M » (index 2). Deux feuilles : le libellé d'une ligne de tracé commence par le nom
    // de la feuille (« -Feuille 1 : Segment AB »).
    f.p.apply(Command::RenameSheet {
        sheet: f.p.sheets[0].id,
        name: "-Feuille 1".into(),
    })
    .unwrap();
    f.p.apply(Command::CreateSheet {
        name: "+Feuille 2".into(),
    })
    .unwrap();
    let s2 = f.p.sheets[1].id;
    seg(&mut f.p, s2, pt(0.0, 0.0), pt(10.0, 0.0));
    let csv = csv_of(&f, Lang::Fr);
    let rows = parse(csv.as_bytes());
    let mut checked = 0;
    for row in &rows {
        for field in row {
            // Aucun champ ne commence par un caractère de formule sans l'apostrophe devant.
            assert!(
                !field.starts_with(['=', '+', '-', '@', '\t', '\r']),
                "champ non protégé : {field:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 40);
    let find = |needle: &str| rows.iter().flatten().any(|f| f == needle);
    assert!(find("'=Projet"), "projet");
    assert!(find("'@Gamme"), "gamme");
    assert!(find("'-M"), "taille de base");
    let header = rows.iter().find(|r| r[0] == "#mesure").unwrap();
    assert_eq!(
        &header[3..8],
        &["'=XS", "'+S", "'-M", "'@L", "XL"],
        "en-tête des tailles"
    );
    assert!(find("'-Feuille 1 : Segment AB"), "ligne de tracé");
    assert!(
        find("'+Feuille 2 : Segment AB"),
        "ligne de tracé, feuille 2"
    );
    // Les nombres, eux, ne reçoivent jamais d'apostrophe.
    assert!(rows.iter().any(|r| r.len() > 3 && r[3] == "1942.00"));
}

#[test]
fn csv_text_public_garde_les_octets_de_l_ancienne_copie_de_l_interface() {
    // L'ancienne copie (apps/pf-desktop/src/size_measures.rs) réécrite ici comme oracle.
    fn old(s: &str) -> String {
        let mut field = String::with_capacity(s.len() + 2);
        if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
            field.push('\'');
        }
        field.push_str(s);
        if field.contains([';', '"', '\r', '\n']) {
            format!("\"{}\"", field.replace('"', "\"\""))
        } else {
            field
        }
    }
    for s in [
        "",
        "Périmètre · Devant",
        "=1+1",
        "+x",
        "-x",
        "@x",
        "\tx",
        "\rx",
        "a;b",
        "a\"b",
        "a\nb",
        "=a;b\"c",
        "-",
    ] {
        assert_eq!(csv_text(s), old(s), "{s:?}");
    }
    assert_eq!(csv_text("=1"), "'=1");
}

// ----- PDF -------------------------------------------------------------------------------------

fn lengths_project(range_name: &str) -> Fx {
    // Un segment de 100 mm en M ; XS 90, S 98, L 108, XL 120 : pas 8, 2, 8, 12 (médiane 8) ; le
    // pas S-M s'écarte de 6 > 4 (50 % de la médiane).
    let mut p = Project::new("Longueurs");
    let sheet = p.sheets[0].id;
    let range = add_range(&mut p, range_name, &["XS", "S", "M", "L", "XL"], 2);
    seg(&mut p, sheet, pt(0.0, 0.0), pt(100.0, 0.0));
    let b = point_at(&p, sheet, pt(100.0, 0.0));
    for (i, dx) in [(0, -10.0), (1, -2.0), (3, 8.0), (4, 20.0)] {
        grade(&mut p, sheet, b, i, dx, 0.0);
    }
    Fx { p, range }
}

#[test]
fn pdf_deterministe_octet_a_octet() {
    let f = rect();
    assert_eq!(pdf_of(&f, Lang::Fr), pdf_of(&f, Lang::Fr));
    assert_eq!(pdf_of(&f, Lang::En), pdf_of(&f, Lang::En));
    assert_ne!(pdf_of(&f, Lang::Fr), pdf_of(&f, Lang::En));
    // Un projet reconstruit à l'identique donne les mêmes octets (aucune horloge, aucun hasard).
    assert_eq!(pdf_of(&f, Lang::Fr), pdf_of(&rect(), Lang::Fr));
    assert_eq!(csv_of(&f, Lang::Fr), csv_of(&rect(), Lang::Fr));
}

#[test]
fn pdf_contient_titre_gamme_valeurs_et_evolution() {
    let f = rect();
    let bytes = pdf_of(&f, Lang::Fr);
    assert!(bytes.starts_with(b"%PDF"));
    let pg = pages(&bytes);
    assert_eq!(pg.len(), 1);
    let all = pg[0].joined();
    // Titre et nom de la gamme dans l'en-tête ; unités explicites.
    assert_eq!(pg[0].texts[0].text, "Mesures par taille · Femme");
    assert!((pg[0].texts[0].x_mm - 15.0).abs() < 1e-3, "marge 15 mm");
    assert!(all.contains("Longueurs en mm, aires en mm²"), "{all}");
    assert!(all.contains("Mesures observées, lecture seule"), "{all}");
    // Les valeurs de la ligne du périmètre, dans l'ordre des tailles, sur la même ligne.
    let line = |label: &str| -> Vec<(f64, String)> {
        let first = pg[0].texts.iter().find(|t| t.text == label).expect(label);
        let mut v: Vec<(f64, String)> = pg[0]
            .texts
            .iter()
            .filter(|t| (t.y_mm - first.y_mm).abs() < 1e-6)
            .map(|t| (t.x_mm, t.text.clone()))
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v
    };
    let texts = |v: Vec<(f64, String)>| v.into_iter().map(|(_, t)| t).collect::<Vec<_>>();
    assert_eq!(
        texts(line("Périmètre · Devant")),
        [
            "Périmètre · Devant",
            "mm",
            "1942",
            "1970",
            "2000",
            "2030",
            "2064",
            "30.50",
            "1.53"
        ]
    );
    assert_eq!(
        texts(line("Aire (mm²) · Devant")),
        [
            "Aire (mm²) · Devant",
            "mm²",
            "226780",
            "233050",
            "240000",
            "247050",
            "255020",
            "7060.00",
            "2.98"
        ]
    );
    // En-tête de colonnes : les tailles dans l'ordre.
    // (« Pas moyen (%) » est coupé en deux lignes dans sa colonne étroite.)
    assert_eq!(
        texts(line("Mesure")),
        [
            "Mesure",
            "Unité",
            "XS",
            "S",
            "M",
            "L",
            "XL",
            "Pas moyen",
            "Pas moyen"
        ]
    );
}

#[test]
fn pdf_page_a4_et_marges_en_mm_vers_pt() {
    let bytes = pdf_of(&rect(), Lang::Fr);
    let text = String::from_utf8(bytes.clone()).expect("PDF ASCII");
    // A4 = 210 × 297 mm = 595,2756 × 841,8898 pt (72 / 25,4).
    assert!(text.contains("/MediaBox [0 0 595.2756 841.8898]"));
    let boxes = media_boxes(&bytes);
    assert!((boxes[0].0 - 210.0).abs() < 1e-3 && (boxes[0].1 - 297.0).abs() < 1e-3);
    // Filets entre marges de 15 mm.
    for page in pages(&bytes) {
        assert!(!page.rules.is_empty());
        for r in page.rules {
            assert!((r.x1_mm - 15.0).abs() < 1e-3 && (r.x2_mm - 195.0).abs() < 1e-3);
        }
    }
}

#[test]
fn alerte_d_irregularite_en_texte_et_symbole_dans_csv_et_pdf() {
    let f = lengths_project("Femme");
    let table = f.p.size_measures(f.range).unwrap();
    assert_eq!(table.irregular_rows().count(), 1);
    // CSV : colonne d'alerte en texte.
    let csv = csv_of(&f, Lang::Fr);
    assert!(
        csv.lines().any(|l| l
            == "mesure;Segment AB;mm;90.00;98.00;100.00;108.00;120.00;7.50;7.51;Irrégulier : pas à plus de 50 % de la médiane (S-M)"),
        "{csv}"
    );
    assert!(csv.contains("resume;Évolutions irrégulières;1"));
    // PDF : symbole ET texte, sur une ligne placée sous la rangée.
    let bytes = pdf_of(&f, Lang::Fr);
    let all = pages(&bytes)[0].joined();
    assert_eq!(ALERT_MARK, "(!)");
    assert!(
        all.lines()
            .any(|l| l == "(!) Irrégulier : pas à plus de 50 % de la médiane (S-M)"),
        "{all}"
    );
    for v in ["90", "98", "100", "108", "120", "7.50", "7.51"] {
        assert!(all.lines().any(|l| l == v), "{v} : {all}");
    }
    // En anglais.
    let en = pages(&pdf_of(&f, Lang::En))[0].joined();
    assert!(
        en.lines()
            .any(|l| l == "(!) Irregular: step more than 50 % from the median (S-M)"),
        "{en}"
    );
    // Sans irrégularité : aucune ligne d'alerte.
    let calm = pages(&pdf_of(&rect(), Lang::Fr))[0].joined();
    assert!(!calm.contains("(!)"));
}

#[test]
fn beaucoup_de_tailles_le_tableau_pdf_est_coupe_en_blocs_sans_perdre_une_valeur() {
    // Douze tailles : les colonnes ne tiennent pas sur 180 mm ; chaque taille reste écrite, dans
    // le bloc de ses voisines, et la page n'est jamais dépassée.
    let mut p = Project::new("Douze");
    let sheet = p.sheets[0].id;
    let labels: Vec<String> = (0..12).map(|i| format!("{}", 34 + 2 * i)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let range = add_range(&mut p, "Grande gamme", &refs, 5);
    seg(&mut p, sheet, pt(0.0, 0.0), pt(100000.0, 0.0));
    let b = point_at(&p, sheet, pt(100000.0, 0.0));
    for i in (0..12).filter(|i| *i != 5) {
        grade(&mut p, sheet, b, i, (i as f64 - 5.0) * 1111.0, 0.0);
    }
    let out = export_size_measures(&p, range, ReportFormat::Pdf, Lang::Fr).unwrap();
    // Copie dans `target/` pour relire le fichier avec un lecteur tiers (MuPDF).
    fs::write(work_dir("douze").join("douze.pdf"), &out.bytes).unwrap();
    let pg = pages(&out.bytes);
    let all: Vec<_> = pg.iter().flat_map(|p| p.texts.iter()).collect();
    for (i, label) in labels.iter().enumerate() {
        assert!(all.iter().any(|t| &t.text == label), "taille {label}");
        let v = format!("{}", 100000 + (i as i64 - 5) * 1111);
        assert!(all.iter().any(|t| t.text == v), "valeur {v}");
    }
    for t in &all {
        assert!(t.x_mm >= 14.99 && t.x_mm <= 195.0, "{t:?}");
        assert!(t.y_mm >= 0.0 && t.y_mm <= 297.0, "{t:?}");
    }
    let blocks = all
        .iter()
        .filter(|t| t.text.contains(" · tailles "))
        .count();
    assert!(blocks >= 2, "tableau non coupé");
}

// ----- Refus, projet inchangé, écriture --------------------------------------------------------

#[test]
fn gamme_inconnue_refusee_sans_rien_produire() {
    let f = rect();
    for format in [ReportFormat::Csv, ReportFormat::Pdf] {
        let r = export_size_measures(&f.p, SizeRangeId(9999), format, Lang::Fr);
        assert!(matches!(r, Err(ExportError::Core(_))), "{r:?}");
    }
    // Un projet sans gamme : refus aussi.
    let empty = Project::new("Vide");
    assert!(export_size_measures(&empty, f.range, ReportFormat::Csv, Lang::Fr).is_err());
}

#[test]
fn projet_inchange_a_l_octet_et_aucune_ecriture_hors_du_chemin_choisi() {
    let f = rect();
    let before = json(&f.p);
    let dir = work_dir("ecriture");
    let other = work_dir("ecriture_autre");
    let cwd_before = listing(&std::env::current_dir().unwrap());
    for (format, ext) in [(ReportFormat::Csv, "csv"), (ReportFormat::Pdf, "pdf")] {
        let out = export_size_measures(&f.p, f.range, format, Lang::Fr).unwrap();
        // Produire les octets n'écrit rien.
        assert!(listing(&dir).len() <= 1);
        let path = dir.join(format!("mesures.{ext}"));
        out.write_to(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), out.bytes);
    }
    // Seuls les deux fichiers choisis existent : ni temporaire, ni voisin ; l'autre dossier est vide.
    assert_eq!(listing(&dir), ["mesures.csv", "mesures.pdf"]);
    assert!(listing(&other).is_empty());
    assert_eq!(listing(&std::env::current_dir().unwrap()), cwd_before);
    // Destination illisible : erreur propre, aucun fichier.
    let out = export_size_measures(&f.p, f.range, ReportFormat::Csv, Lang::Fr).unwrap();
    assert!(out.write_to(&dir.join("absent").join("x.csv")).is_err());
    assert_eq!(listing(&dir), ["mesures.csv", "mesures.pdf"]);
    assert_eq!(json(&f.p), before, "projet modifié");
}

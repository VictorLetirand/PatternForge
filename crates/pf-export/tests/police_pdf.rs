//! US-06.6 — texte du PDF avec une police **embarquée** (sous-ensemble de Noto Sans Regular,
//! SIL OFL 1.1) : accents fidèles, texte copiable (`ToUnicode`), largeurs de centrage tirées des
//! vraies métriques.
//!
//! Les fichiers sont relus par des lecteurs écrits dans `common/pdf_text.rs`, indépendants du
//! code d'export : table `xref`, objets de police, `ToUnicode`, `CIDToGIDMap`, `W`, police
//! intégrée (largeurs d'avance, longueur des glyphes) et police du dépôt lue sur disque. Les
//! chaînes et largeurs attendues sont écrites à la main (largeurs d'avance de Noto Sans Regular :
//! chiffre 572, espace 260, « m » 935, « Ø » 781, « ° » 428, « ? » 434, points 268, sur 1000).

mod common;

use common::pdf_text::{parse_ttf, read_font, repo_font, repo_font_bytes};
use pf_core::PathRole;
use pf_export::pdf::{export_calibration_pdf, export_pdf};
use pf_export::svg::export_svg;
use pf_export::{PathGeom, Scene, SceneDimension, ScenePath, ScenePieceLabel, ScenePoint};
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Pièce de 200 × 150 mm portant une étiquette, deux cotations et un point nommé.
fn scene(label_lines: &[&str], dims: &[&str], point: &str) -> Scene {
    let c = [
        pt(0.0, 0.0),
        pt(200.0, 0.0),
        pt(200.0, 150.0),
        pt(0.0, 150.0),
    ];
    Scene {
        project_name: "Écharpe".into(),
        sheet_name: "Pièce".into(),
        paths: (0..4)
            .map(|i| ScenePath {
                geom: PathGeom::Line(c[i], c[(i + 1) % 4]),
                role: PathRole::CuttingContour,
                sense: pf_core::GrainSense::Unmarked,
            })
            .collect(),
        points: vec![ScenePoint {
            pos: c[0],
            label: point.into(),
            show_label: true,
        }],
        notch_marks: Vec::new(),
        dimensions: dims
            .iter()
            .enumerate()
            .map(|(i, d)| SceneDimension {
                from: pt(0.0, 0.0),
                to: pt(200.0, 0.0),
                from_offset: pt(0.0, -10.0 - 8.0 * i as f64),
                to_offset: pt(200.0, -10.0 - 8.0 * i as f64),
                text: (*d).into(),
            })
            .collect(),
        full_dimensions: Vec::new(),
        markings: Vec::new(),
        stitch_holes: Vec::new(),
        zones: Vec::new(),
        buttons: Vec::new(),
        grain_limits: Vec::new(),
        cut_contours: Vec::new(),
        cut_warnings: Vec::new(),
        darts: Vec::new(),
        dart_warnings: Vec::new(),
        piece_labels: vec![ScenePieceLabel {
            piece: "Écharpe".into(),
            center: pt(100.0, 75.0),
            lines: label_lines.iter().map(|l| (*l).into()).collect(),
        }],
        folds: Vec::new(),
        fold_overs: Vec::new(),
    }
}

fn base_scene() -> Scene {
    scene(
        &["Écharpe à pli", "Matière : Cuir", "Quantité : 2"],
        &["Ø 12,0 mm", "120,00 mm"],
        "P1",
    )
}

fn all_strings(bytes: &[u8]) -> Vec<String> {
    common::read_pdf(bytes)
        .into_iter()
        .flat_map(|p| p.strings)
        .collect()
}

// --- CA 1 : police embarquée, aucune Helvetica ---------------------------------------------

#[test]
fn ca1_la_police_est_embarquee_avec_un_prefixe_de_sous_ensemble_et_sans_helvetica() {
    let out = export_pdf(&[base_scene()]).unwrap();
    let pdf = std::str::from_utf8(&out.bytes).expect("PDF ASCII");
    let font = read_font(&out.bytes);
    // Préfixe de sous-ensemble : six capitales, « + », nom PostScript de la police.
    let (prefix, name) = font
        .base_font
        .split_once('+')
        .expect("préfixe de sous-ensemble");
    assert_eq!(prefix.len(), 6);
    assert!(prefix.chars().all(|c| c.is_ascii_uppercase()), "{prefix}");
    assert_eq!(name, "NotoSans-Regular");
    // Le même nom partout.
    assert!(font
        .cid_font
        .contains(&format!("/BaseFont /{}", font.base_font)));
    assert!(font
        .descriptor
        .contains(&format!("/FontName /{}", font.base_font)));
    // Police TrueType intégrée : tables requises, longueur annoncée exacte.
    assert!(font.descriptor.contains("/FontFile2 "));
    let ttf = parse_ttf(&font.font_file);
    for tag in ["glyf", "loca", "head", "hhea", "hmtx", "maxp"] {
        assert!(ttf.tags.iter().any(|t| t == tag), "table {tag} absente");
    }
    let len1 = format!("/Length1 {}", font.font_file.len());
    assert!(pdf.contains(&len1), "{len1} absent");
    // Aucune police Helvetica ni Type1 ni WinAnsi pour le texte.
    assert!(!pdf.contains("Helvetica") && !pdf.contains("/Type1") && !pdf.contains("WinAnsi"));
    // Le sous-ensemble est petit (quelques dizaines de glyphes), jamais la police complète.
    assert!(
        font.font_file.len() < 60_000,
        "{} octets",
        font.font_file.len()
    );
    // Le fichier reste de l'ASCII pur, en-tête PDF 1.4.
    assert!(out.bytes.is_ascii() && pdf.starts_with("%PDF-1.4\n"));
}

#[test]
fn ca1_le_pdf_reste_a4_a_l_echelle_1_pour_1_avec_l_etalon_de_100_mm() {
    let out = export_pdf(&[base_scene()]).unwrap();
    let pages = common::read_pdf(&out.bytes);
    assert_eq!(pages.len(), out.page_count);
    for p in &pages {
        let (w, h) = p.media_mm();
        assert!(
            (w - 210.0).abs() < 0.01 && (h - 297.0).abs() < 0.01,
            "{w} x {h}"
        );
    }
    // Carré étalon de 100 × 100 mm en page 1 (pt → mm : 25,4 / 72).
    assert!(pages[0]
        .rects
        .iter()
        .any(|r| (r.2 - 100.0).abs() < 1e-3 && (r.3 - 100.0).abs() < 1e-3));
    // La cotation du bord de 200 mm est une ligne de 200 mm exactement (aucun trait modifié).
    assert!(pages[1..]
        .iter()
        .flat_map(|p| p.lines.iter())
        .any(|l| (l.len() - 200.0).abs() < 1e-3));
}

// --- CA 2 : texte relu avec ses accents ---------------------------------------------------

#[test]
fn ca2_les_textes_sont_relus_exactement_avec_leurs_accents() {
    let out = export_pdf(&[base_scene()]).unwrap();
    let strings = all_strings(&out.bytes);
    for want in [
        "Écharpe à pli",
        "Matière : Cuir",
        "Quantité : 2",
        "Ø 12,0 mm",
        "120,00 mm",
        "P1",
        // Page étalon : consignes avec guillemets, tiret cadratin, signes.
        "PatternForge — page étalon",
        "100 × 100 mm",
        "Imprimer à 100 % (taille réelle). Désactiver « Ajuster à la page ».",
    ] {
        assert!(
            strings.iter().any(|s| s == want),
            "« {want} » absent : {strings:?}"
        );
    }
}

#[test]
fn ca2_tous_les_signes_demandes_sont_fideles() {
    let line = "é è ê à ç œ ù ï ô É Ç Œ « » “ ” ’ — – … ° ² ³ Ø × ± € Ž ł ő";
    let out = export_pdf(&[scene(&[line], &[], "A")]).unwrap();
    assert!(
        all_strings(&out.bytes).iter().any(|s| s == line),
        "ligne relue ≠ ligne écrite"
    );
}

#[test]
fn ca2_le_texte_est_copiable_ligne_a_ligne_par_le_to_unicode_du_fichier() {
    let out = export_pdf(&[base_scene()]).unwrap();
    let font = read_font(&out.bytes);
    // Chaque CID utilisé est décrit : lettres accentuées lues par la table du fichier.
    let texts: Vec<&String> = font.to_unicode.values().collect();
    for ch in ["É", "à", "è", "Ø", "—", "«"] {
        assert!(
            texts.iter().any(|t| t.as_str() == ch),
            "{ch} absent du ToUnicode"
        );
    }
    // Aucun CID n'est décrit deux fois pour deux textes différents (table fonctionnelle).
    assert_eq!(
        font.to_unicode.len(),
        font.to_unicode
            .values()
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
}

// --- CA 3 : la police embarquée est cohérente avec la police du dépôt -----------------------

#[test]
fn ca3_cidtogid_w_et_glyphes_du_sous_ensemble_concordent_avec_la_police_du_depot() {
    let repo = repo_font();
    // Tous les caractères de la police du dépôt, en lignes de 40 caractères.
    let chars: Vec<char> = repo
        .cmap
        .keys()
        .filter_map(|c| char::from_u32(*c))
        .filter(|c| !c.is_control())
        .collect();
    let lines: Vec<String> = chars.chunks(40).map(|c| c.iter().collect()).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let out = export_pdf(&[scene(&refs, &[], "A")]).unwrap();
    let font = read_font(&out.bytes);
    let embedded = parse_ttf(&font.font_file);
    assert!(
        font.widths.len() >= 390,
        "{} CID décrits",
        font.widths.len()
    );
    for (cid, width) in &font.widths {
        let text = &font.to_unicode[cid];
        let ch = text.chars().next().unwrap();
        let gid = repo.cmap[&(ch as u32)];
        assert_eq!(
            font.cid_to_gid[usize::from(*cid)],
            gid,
            "CIDToGIDMap de {ch:?}"
        );
        let adv = i64::from(repo.advance_of(ch).unwrap());
        assert_eq!(*width, adv * 1000 / i64::from(repo.upem), "/W de {ch:?}");
        assert_eq!(
            embedded.advance_of_gid(gid),
            adv as u16,
            "hmtx intégré de {ch:?}"
        );
        assert_eq!(
            embedded.glyph_len[usize::from(gid)],
            repo.glyph_len[usize::from(gid)],
            "glyphe {ch:?} absent ou tronqué du sous-ensemble"
        );
    }
}

#[test]
fn ca3_un_texte_ascii_seul_n_embarque_que_les_glyphes_utilises() {
    let out = export_pdf(&[scene(&["AB"], &[], "A")]).unwrap();
    let font = read_font(&out.bytes);
    let embedded = parse_ttf(&font.font_file);
    let repo = repo_font();
    let a_gid = repo.cmap[&('A' as u32)];
    let z_gid = repo.cmap[&('z' as u32)];
    assert!(embedded.glyph_len[usize::from(a_gid)] > 0, "« A » présent");
    // « z » n'est écrit nulle part : son glyphe est vide (ou hors du sous-ensemble).
    assert!(
        usize::from(z_gid) >= embedded.glyph_len.len()
            || embedded.glyph_len[usize::from(z_gid)] == 0,
        "glyphe inutilisé embarqué"
    );
}

// --- CA 4 : largeurs = somme des largeurs d'avance de la police ------------------------------

#[test]
fn ca4_largeurs_lues_dans_la_police_par_un_lecteur_independant() {
    let repo = repo_font();
    // Valeurs écrites à la main (Noto Sans Regular, 1000 unités par em).
    for (ch, adv) in [
        ('0', 572),
        ('5', 572),
        (' ', 260),
        ('m', 935),
        ('Ø', 781),
        ('°', 428),
        ('?', 434),
        ('.', 268),
    ] {
        assert_eq!(repo.advance_of(ch), Some(adv), "{ch:?}");
    }
    // « 0 m » = 572 + 260 + 935 = 1767 ; « Ø° » = 781 + 428 = 1209.
    let mm = |units: f64, pt: f64| units / 1000.0 * pt * 25.4 / 72.0;
    assert!((pf_export::font::text_width_mm("0 m", 10.0) - mm(1767.0, 10.0)).abs() < 1e-9);
    assert!((pf_export::font::text_width_mm("Ø°", 8.5) - mm(1209.0, 8.5)).abs() < 1e-9);
    // « 50.00 mm » : 4 × 572 + 268 + 260 + 2 × 935 = 4686.
    assert!((pf_export::font::text_width_mm("50.00 mm", 12.0) - mm(4686.0, 12.0)).abs() < 1e-9);
    // Texte vide : largeur nulle. Caractère absent de la police : largeur de « ? » (434).
    assert_eq!(pf_export::font::text_width_mm("", 10.0), 0.0);
    assert!((pf_export::font::text_width_mm("日", 10.0) - mm(434.0, 10.0)).abs() < 1e-9);
}

#[test]
fn ca4_l_etiquette_est_centree_avec_les_vraies_largeurs() {
    // Une ligne de largeur connue : « 0000 » = 4 × 572 = 2288 / 1000 em. Corps de l'étiquette :
    // `LABEL_TEXT_MM`. Centre de l'étiquette à x = 100 mm : le texte commence à
    // 100 − largeur / 2 (en mm de page, marge gauche de 10 mm, fenêtre à 0).
    let sc = scene(&["0000"], &[], "A");
    let out = export_pdf(&[sc]).unwrap();
    let readable = common::pdf_text::readable(&out.bytes);
    let op = readable
        .lines()
        .find(|l| l.ends_with("(0000) Tj ET"))
        .expect("ligne « 0000 »");
    // `BT /F1 s Tf 0 g x y Td (0000) Tj ET` : x en points.
    let toks: Vec<&str> = op.split_whitespace().collect();
    let td = toks.iter().position(|t| *t == "Td").unwrap();
    let x: f64 = toks[td - 2].parse::<f64>().unwrap() * 25.4 / 72.0;
    let width = 2.288 * pf_export::LABEL_TEXT_MM;
    // Marge de page de 10 mm + marge de contenu de 5 mm (le patron commence à x = 0).
    let expected = 10.0 + 5.0 + 100.0 - width / 2.0;
    assert!(
        (x - expected).abs() < 0.01,
        "origine x = {x}, attendu {expected}"
    );
}

// --- Robustesse -------------------------------------------------------------------------------

#[test]
fn texte_vide_absent_de_la_police_controle_et_tres_long_sans_panic() {
    let long: String = "é".repeat(5000);
    let lines = ["", "日本語", "😀", "A\nB\tC", "Ж", "x ≤ y", long.as_str()];
    let out = export_pdf(&[scene(&lines, &["", "Ø ?"], "日本")]).unwrap();
    let strings = all_strings(&out.bytes);
    for want in [
        "",
        "???",
        "?",
        "A?B?C",
        "?",
        "x ? y",
        long.as_str(),
        "Ø ?",
        "??",
    ] {
        assert!(
            strings.iter().any(|s| s == want),
            "« {} » absent",
            want.chars().take(20).collect::<String>()
        );
    }
}

#[test]
fn deux_exports_identiques_donnent_les_memes_octets() {
    let a = export_pdf(&[base_scene()]).unwrap();
    let b = export_pdf(&[base_scene()]).unwrap();
    assert_eq!(a.bytes, b.bytes);
    assert_eq!(
        export_calibration_pdf().bytes,
        export_calibration_pdf().bytes
    );
}

#[test]
fn la_page_etalon_seule_embarque_la_police_et_relit_ses_accents() {
    let out = export_calibration_pdf();
    let font = read_font(&out.bytes);
    assert!(font.base_font.ends_with("+NotoSans-Regular"));
    let strings = all_strings(&out.bytes);
    assert!(strings.iter().any(|s| s == "PatternForge — page étalon"));
    assert!(strings.iter().any(|s| s == "Légende des traits"));
}

// --- SVG : texte UTF-8 réel, police non embarquée ---------------------------------------------

#[test]
fn le_svg_garde_le_texte_utf8_reel_sans_police_embarquee() {
    let svg = export_svg(&base_scene()).unwrap();
    assert!(svg.contains("Écharpe à pli") && svg.contains("Ø 12,0 mm"));
    // Choix documenté : la liste de familles du SVG n'est pas modifiée (octets des exemples).
    assert!(svg.contains("font-family=\"Helvetica, Arial, sans-serif\""));
    assert!(!svg.contains("base64") && !svg.contains("@font-face"));
}

// --- Licence et provenance de la police du dépôt ---------------------------------------------

#[test]
fn la_police_du_depot_est_petite_sous_licence_ofl_avec_son_avis() {
    let bytes = repo_font_bytes();
    assert!(bytes.len() <= 250_000, "{} octets", bytes.len());
    // Avis de copyright conservé dans la table `name` (UTF-16BE).
    let needle: Vec<u8> = "Noto Project Authors"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    assert!(bytes.windows(needle.len()).any(|w| w == needle.as_slice()));
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/");
    let ofl = std::fs::read_to_string(format!("{dir}OFL.txt")).expect("OFL.txt");
    assert!(ofl.contains("SIL OPEN FONT LICENSE Version 1.1"));
    assert!(ofl.contains("Copyright 2022 The Noto Project Authors"));
    let readme = std::fs::read_to_string(format!("{dir}README.md")).expect("README.md");
    assert!(readme.contains("SHA-256") && readme.contains("raw.githubusercontent.com"));
}

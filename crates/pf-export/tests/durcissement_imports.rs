//! Durcissement 1, fichiers piégés à l'import : PDF (A3 budget décompressé, A4 unité et
//! rotation de page), DXF AAMA (A6 recherche des blocs), SVG et DXF (A7 plafonds à la lecture).
//! Les variantes d'erreur sont comparées par leur écriture `Debug` : ces tests compilent aussi
//! sur le code d'avant la correction, où ils échouaient.

use pf_core::ImportItem;
use pf_export::dxf_aama_import::{AamaDocument, AamaReason};
use pf_export::dxf_import::{DxfDocument, DxfUnit};
use pf_export::pdf_import::PdfDocument;
use pf_export::svg_import::SvgDocument;
use std::sync::mpsc;
use std::time::Duration;

fn bounded<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok(v) => v,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("gel : rien après {secs} s"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(worker.join().unwrap_err())
        }
    }
}

fn refused<T, E: std::fmt::Debug>(r: Result<T, E>, expected: &str) {
    match r {
        Ok(_) => panic!("accepté, refus {expected} attendu"),
        Err(e) => assert_eq!(format!("{e:?}"), expected),
    }
}

// ---------------------------------------------------------------- PDF

/// PDF à `pages` pages qui citent **toutes** le même flux de contenu (objet 3).
/// `tree` : clés ajoutées au nœud `/Pages` ; `page` : clés ajoutées à chaque page.
fn pdf(pages: usize, stream: &[u8], filter: &str, tree: &str, page: &str) -> Vec<u8> {
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 10 + i)).collect();
    let mut out = b"%PDF-1.6\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_vec();
    out.extend(
        format!(
            "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {pages} {tree} >>\nendobj\n",
            kids.join(" ")
        )
        .into_bytes(),
    );
    out.extend(format!("3 0 obj\n<< /Length {} {filter} >>\nstream\n", stream.len()).into_bytes());
    out.extend(stream);
    out.extend(b"\nendstream\nendobj\n");
    for i in 0..pages {
        out.extend(
            format!(
                "{} 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 3 0 R {page} >>\nendobj\n",
                10 + i
            )
            .into_bytes(),
        );
    }
    out.extend(b"trailer\n<< /Root 1 0 R >>\n%%EOF\n");
    out
}

/// 72 points = 25,4 mm.
const LINE: &str = "0 0 m 72 0 l S";

fn padded(total: usize) -> Vec<u8> {
    let mut v = LINE.as_bytes().to_vec();
    v.resize(total, b' ');
    v
}

fn deflated(total: usize) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(&padded(total), 6)
}

const MIB: usize = 1024 * 1024;

#[test]
fn a3_flux_compresse_cite_par_beaucoup_de_pages_refuse() {
    // Un fichier de quelques dizaines de Ko qui demanderait 40 × 10 Mio = 400 Mio.
    let bytes = pdf(40, &deflated(10 * MIB), "/Filter /FlateDecode", "", "");
    assert!(bytes.len() < 60 * 1024, "{}", bytes.len());
    refused(PdfDocument::parse(&bytes), "Unsupported(\"total_too_big\")");
}

#[test]
fn a3_flux_non_compresse_cite_par_beaucoup_de_pages_refuse() {
    let bytes = pdf(100, &padded(3 * MIB), "", "", "");
    refused(PdfDocument::parse(&bytes), "Unsupported(\"total_too_big\")");
}

/// Des milliers de références à un gros flux qui ne décompresse presque rien : le temps de
/// lecture est borné aussi (chaque référence consomme au moins sa taille compressée).
#[test]
fn a3_gros_flux_compresse_qui_ne_donne_rien_refuse() {
    // Blocs « stored » vides répétés, puis un bloc final : 2 Mio compressés, 0 octet décodé.
    let mut z = vec![0x78, 0x01];
    for _ in 0..(2 * MIB / 5) {
        z.extend([0x00, 0x00, 0x00, 0xFF, 0xFF]);
    }
    z.extend([0x01, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01]);
    let bytes = pdf(20_000, &z, "/Filter /FlateDecode", "", "");
    let r = bounded(60, move || {
        PdfDocument::parse(&bytes).map(|d| d.page_count())
    });
    refused(r, "Unsupported(\"total_too_big\")");
}

#[test]
fn a3_document_ordinaire_a_flux_partage_accepte() {
    // 30 pages × 1 Mio = 30 Mio : sous le budget.
    let bytes = pdf(30, &deflated(MIB), "/Filter /FlateDecode", "", "");
    let doc = PdfDocument::parse(&bytes).unwrap();
    assert_eq!(doc.page_count(), 30);
    assert_eq!(doc.build(30, 1.0).unwrap().objects.len(), 1);
}

fn segment_length_mm(bytes: &[u8]) -> f64 {
    let b = PdfDocument::parse(bytes).unwrap().build(1, 1.0).unwrap();
    let ImportItem::Segment { from, to } = b.objects[0].item.clone() else {
        panic!("pas un segment")
    };
    pf_geometry::distance_mm(from, to)
}

fn notes(bytes: &[u8]) -> Vec<(&'static str, usize)> {
    let b = PdfDocument::parse(bytes).unwrap().build(1, 1.0).unwrap();
    b.report
        .notes
        .iter()
        .map(|n| (n.kind.key(), n.count))
        .collect()
}

#[test]
fn a4_user_unit_applique_et_signale() {
    // UserUnit 10 : une unité vaut 10/72 de pouce, 72 unités = 254 mm.
    let bytes = pdf(1, LINE.as_bytes(), "", "", "/UserUnit 10");
    assert!(
        (segment_length_mm(&bytes) - 254.0).abs() < 1e-9,
        "{}",
        segment_length_mm(&bytes)
    );
    assert_eq!(notes(&bytes), vec![("user_unit", 1)]);
    let b = PdfDocument::parse(&bytes).unwrap().build(1, 0.5).unwrap();
    assert!((b.mm_per_unit - 25.4 / 72.0 * 10.0 * 0.5).abs() < 1e-12);
    // UserUnit 2.5, cité par référence indirecte : 63,5 mm.
    let mut indirect = pdf(1, LINE.as_bytes(), "", "", "/UserUnit 5 0 R");
    indirect.extend(b"5 0 obj\n2.5\nendobj\n");
    assert!((segment_length_mm(&indirect) - 63.5).abs() < 1e-9);
}

#[test]
fn a4_user_unit_un_ou_absent_inchange() {
    for page in ["", "/UserUnit 1", "/UserUnit 1.0"] {
        let bytes = pdf(1, LINE.as_bytes(), "", "", page);
        assert!((segment_length_mm(&bytes) - 25.4).abs() < 1e-9, "{page}");
        assert_eq!(notes(&bytes), vec![], "{page}");
    }
}

#[test]
fn a4_user_unit_illisible_refuse() {
    for page in [
        "/UserUnit 0",
        "/UserUnit -2",
        "/UserUnit /Dix",
        "/UserUnit (10)",
        "/UserUnit 9 0 R",
        "/UserUnit 100000000",
    ] {
        let bytes = pdf(1, LINE.as_bytes(), "", "", page);
        refused(PdfDocument::parse(&bytes), "Structure(\"UserUnit\")");
    }
}

#[test]
fn a4_rotation_heritee_du_parent_signalee() {
    let bytes = pdf(1, LINE.as_bytes(), "", "/Rotate 90", "");
    assert_eq!(notes(&bytes), vec![("page_rotation", 1)]);
    // La page redéfinit la rotation héritée : plus de remarque.
    let bytes = pdf(1, LINE.as_bytes(), "", "/Rotate 90", "/Rotate 0");
    assert_eq!(notes(&bytes), vec![]);
    // Héritage à travers deux niveaux, par référence indirecte.
    let mut deep = b"%PDF-1.6\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [4 0 R] /Rotate 6 0 R >>\nendobj\n\
3 0 obj\n<< /Length 14 >>\nstream\n0 0 m 72 0 l S\nendstream\nendobj\n\
4 0 obj\n<< /Type /Pages /Parent 2 0 R /Kids [5 0 R] >>\nendobj\n\
5 0 obj\n<< /Type /Page /Parent 4 0 R /Contents 3 0 R >>\nendobj\n\
6 0 obj\n270\nendobj\ntrailer\n<< /Root 1 0 R >>\n"
        .to_vec();
    deep.extend(b"%%EOF\n");
    assert_eq!(notes(&deep), vec![("page_rotation", 1)]);
}

/// `MediaBox` (hérité ou non) délimite la page : il ne change pas l'échelle de lecture.
#[test]
fn a4_media_box_heritee_sans_effet_sur_l_echelle() {
    let bytes = pdf(1, LINE.as_bytes(), "", "/MediaBox [100 100 7200 7200]", "");
    assert!((segment_length_mm(&bytes) - 25.4).abs() < 1e-9);
    assert_eq!(notes(&bytes), vec![]);
}

/// La décompression bornée ne suffit pas : la **lecture** d'une page l'est aussi, sinon une page
/// de quelques Ko compressés fait construire des millions de formes.
fn page_refused(content: &str, expected: &str) {
    let z = miniz_oxide::deflate::compress_to_vec_zlib(content.as_bytes(), 6);
    let bytes = pdf(1, &z, "/Filter /FlateDecode", "", "");
    assert!(bytes.len() < 100 * 1024, "{}", bytes.len());
    refused(PdfDocument::parse(&bytes).unwrap().build(1, 1.0), expected);
}

#[test]
fn a3_page_de_formes_sans_objet_refusee_avant_de_tout_construire() {
    page_refused(
        &"0 0 m S ".repeat(400_001),
        "TooManyElements { max: 400000 }",
    );
}

#[test]
fn a3_page_de_sommets_sans_fin_refusee() {
    // Un chemin jamais peint s'accumulait sans limite.
    let content = format!("0 0 m {}", "1 1 l ".repeat(600_001));
    page_refused(&content, "TooManyElements { max: 600000 }");
}

#[test]
fn a3_page_d_operateurs_inconnus_en_masse_refusee() {
    let content: String = (0..10_001).map(|i| format!("zz{i} ")).collect();
    page_refused(&content, "TooManyElements { max: 10000 }");
}

#[test]
fn a3_page_au_plafond_d_objets_acceptee() {
    let content = "0 0 m 72 0 l S ".repeat(200_000);
    let z = miniz_oxide::deflate::compress_to_vec_zlib(content.as_bytes(), 6);
    let bytes = pdf(1, &z, "/Filter /FlateDecode", "", "");
    let build = PdfDocument::parse(&bytes).unwrap().build(1, 1.0).unwrap();
    assert_eq!(build.objects.len(), 200_000);
}

// ---------------------------------------------------------------- DXF AAMA

fn aama(blocks: &[String], inserts: &[String]) -> Vec<u8> {
    let mut s = String::from("0\nSECTION\n2\nBLOCKS\n");
    for name in blocks {
        s.push_str(&format!("0\nBLOCK\n2\n{name}\n10\n0\n20\n0\n0\nENDBLK\n"));
    }
    s.push_str("0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    for name in inserts {
        s.push_str(&format!("0\nINSERT\n2\n{name}\n10\n0\n20\n0\n"));
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s.into_bytes()
}

#[test]
fn a6_beaucoup_de_blocs_et_d_insertions_sans_gel() {
    let blocks: Vec<String> = (0..49_000).map(|i| format!("B{i}")).collect();
    let inserts = vec!["inconnu".to_string(); 100_000];
    let bytes = aama(&blocks, &inserts);
    let (inserts, unknown) = bounded(20, move || {
        let build = AamaDocument::parse(&bytes)
            .unwrap()
            .build(DxfUnit::Millimetre);
        (
            build.report.inserts,
            build.report.ignored_of(AamaReason::UnknownBlock),
        )
    });
    assert_eq!((inserts, unknown), (100_000, 100_000));
}

/// La recherche reste insensible à la casse (ASCII) et retient le premier bloc d'un nom.
#[test]
fn a6_recherche_des_blocs_inchangee() {
    let blocks = ["Devant", "DEVANT", "Dos", "Été"].map(String::from);
    let inserts = ["devant", "dOs", "été", "Été", "Manche"].map(String::from);
    let build = AamaDocument::parse(&aama(&blocks, &inserts))
        .unwrap()
        .build(DxfUnit::Millimetre);
    assert_eq!(build.report.inserts, 5);
    // « été » ne diffère de « Été » que par une majuscule **non ASCII** : bloc inconnu.
    assert_eq!(build.report.ignored_of(AamaReason::UnknownBlock), 2);
    let no_contour: Vec<(&str, usize)> = build
        .report
        .ignored
        .iter()
        .filter(|i| i.reason == AamaReason::NoContour)
        .map(|i| (i.piece.as_str(), i.count))
        .collect();
    assert_eq!(no_contour, vec![("Devant", 1), ("Dos", 1), ("Été", 1)]);
}

// ---------------------------------------------------------------- SVG

fn svg(body: &str) -> Vec<u8> {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{body}</svg>").into_bytes()
}

#[test]
fn a7_svg_trop_d_elements_refuse_a_la_lecture() {
    // Éléments sans aucun sommet (rayon nul) : seul le plafond d'éléments joue.
    let bytes = svg(&"<circle r='0'/>".repeat(400_001));
    refused(
        SvgDocument::parse(&bytes),
        "TooManyElements { max: 400000 }",
    );
}

#[test]
fn a7_svg_trop_d_elements_ignores_refuse_a_la_lecture() {
    let bytes = svg(&"<x/>".repeat(400_001));
    refused(
        SvgDocument::parse(&bytes),
        "TooManyElements { max: 400000 }",
    );
}

#[test]
fn a7_svg_trop_de_sommets_refuse_a_la_lecture() {
    let max = "TooManyElements { max: 600000 }";
    let poly = svg(&format!("<polyline points='{}'/>", "1 2 ".repeat(600_001)));
    refused(SvgDocument::parse(&poly), max);
    let path = svg(&format!("<path d='M0 0{}'/>", "h1".repeat(600_000)));
    refused(SvgDocument::parse(&path), max);
    // Le plafond porte sur le fichier entier, pas sur un seul élément.
    let element = format!("<polyline points='{}'/>", "1 2 ".repeat(1000));
    refused(SvgDocument::parse(&svg(&element.repeat(601))), max);
}

#[test]
fn a7_svg_trop_d_attributs_refuse_a_la_lecture() {
    let attrs: String = (0..10_001).map(|i| format!(" a{i}='1'")).collect();
    refused(
        SvgDocument::parse(&svg(&format!("<line{attrs}/>"))),
        "TooManyElements { max: 10000 }",
    );
}

/// Au plafond d'objets du modèle, rien n'est refusé.
#[test]
fn a7_svg_au_plafond_d_objets_accepte() {
    let bytes = svg(&"<line x2='1'/>".repeat(200_000));
    let doc = SvgDocument::parse(&bytes).unwrap();
    assert_eq!(doc.element_count(), 200_000);
    assert_eq!(doc.build(1.0, 1.0).unwrap().objects.len(), 200_000);
    // Une polyligne de 200 001 sommets donne 200 000 côtés.
    let poly = svg(&format!(
        "<polyline points='{}'/>",
        (0..=200_000)
            .map(|i| format!("{i} {} ", i % 2))
            .collect::<String>()
    ));
    let doc = SvgDocument::parse(&poly).unwrap();
    assert_eq!(doc.build(1.0, 1.0).unwrap().objects.len(), 200_000);
}

/// Un seul chemin d'arcs elliptiques très aplatis donnerait des millions de segments : la
/// conversion s'arrête au plafond au lieu de tout construire.
#[test]
fn a7_svg_arcs_aplatis_en_masse_refuses_avant_de_tout_construire() {
    // Chaque demi-ellipse de 1000 × 500 mm demande des centaines de côtés à 0,05 mm.
    let d = format!(
        "M0 0{}",
        "a1000 500 0 0 1 2000 0 a1000 500 0 0 1 -2000 0".repeat(50_000)
    );
    let bytes = svg(&format!("<path d='{d}'/>"));
    let count = bounded(60, move || {
        match SvgDocument::parse(&bytes).unwrap().build(1.0, 1.0) {
            Err(pf_export::vector_import::VectorImportError::TooManyObjects { count }) => count,
            other => panic!("{:?}", other.map(|b| b.objects.len())),
        }
    });
    // Arrêt dès le dépassement : au plus un arc de trop (20 000 côtés).
    assert!((200_001..=220_001).contains(&count), "{count}");
}

// ---------------------------------------------------------------- DXF

fn dxf(entities: &str) -> Vec<u8> {
    format!("0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n").into_bytes()
}

fn polyline(n: usize) -> String {
    let mut s = String::from("0\nPOLYLINE\n8\n0\n");
    for i in 0..n {
        s.push_str(&format!("0\nVERTEX\n10\n{i}\n20\n{}\n", i % 2));
    }
    s.push_str("0\nSEQEND\n");
    s
}

fn lwpolyline(n: usize) -> String {
    let mut s = String::from("0\nLWPOLYLINE\n8\n0\n");
    for i in 0..n {
        s.push_str(&format!("10\n{i}\n20\n{}\n", i % 2));
    }
    s
}

#[test]
fn a7_dxf_trop_de_sommets_refuse_a_la_lecture() {
    let e = "TooManyVertices { max: 400000 }";
    refused(DxfDocument::parse(&dxf(&polyline(400_001))), e);
    refused(DxfDocument::parse(&dxf(&lwpolyline(400_001))), e);
    // Le plafond porte sur le fichier entier.
    let several = lwpolyline(1_000).repeat(401);
    refused(DxfDocument::parse(&dxf(&several)), e);
}

#[test]
fn a7_dxf_enregistrement_demesure_refuse_a_la_lecture() {
    // Un seul enregistrement de plus de 4 millions de paires.
    let huge = format!("0\nSPLINE\n{}", "40\n1\n".repeat(4_000_001));
    refused(
        DxfDocument::parse(&dxf(&huge)),
        "TooManyVertices { max: 4000000 }",
    );
}

#[test]
fn a7_dxf_gros_trace_ordinaire_accepte() {
    for body in [polyline(150_000), lwpolyline(150_000)] {
        let doc = DxfDocument::parse(&dxf(&body)).unwrap();
        let build = doc.build(DxfUnit::Millimetre).unwrap();
        assert_eq!(build.objects.len(), 149_999);
    }
}

//! US-12.4 - Import de PDF vectoriel : chemins seulement. PDF ecrits a la main (un par
//! fonctionnalite) ; valeurs calculees a la main : 72 pt = 25,4 mm.

use pf_core::ImportItem;
use pf_export::pdf_import::PdfDocument;
use pf_export::vector_import::{IgnoreReason, NoteKind, VectorBuild, VectorImportError};
use pf_geometry::PointMm;

const MM: f64 = 25.4 / 72.0;

/// PDF minimal : catalogue, arbre de pages, une page et un flux de contenu par entree.
fn pdf_of(pages: &[Vec<u8>], stream_dict: &str, trailer: &str) -> Vec<u8> {
    let n = pages.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + 2 * i)).collect();
    let mut out = b"%PDF-1.4\n".to_vec();
    out.extend(
        format!(
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [{}] /Count {n} >>\nendobj\n",
            kids.join(" ")
        )
        .into_bytes(),
    );
    for (i, data) in pages.iter().enumerate() {
        let (p, c) = (3 + 2 * i, 4 + 2 * i);
        out.extend(
            format!("{p} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents {c} 0 R >>\nendobj\n")
                .into_bytes(),
        );
        out.extend(
            format!(
                "{c} 0 obj\n<< /Length {} {stream_dict} >>\nstream\n",
                data.len()
            )
            .into_bytes(),
        );
        out.extend(data);
        out.extend(b"\nendstream\nendobj\n");
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R {trailer} >>\n%%EOF\n",
            3 + 2 * n
        )
        .into_bytes(),
    );
    out
}

fn one(content: &str) -> Vec<u8> {
    pdf_of(&[content.as_bytes().to_vec()], "", "")
}

fn build(bytes: &[u8]) -> VectorBuild {
    PdfDocument::parse(bytes).unwrap().build(1, 1.0).unwrap()
}

fn items(b: &VectorBuild) -> Vec<ImportItem> {
    b.objects.iter().map(|o| o.item.clone()).collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9
}

fn close(p: PointMm, x: f64, y: f64) -> bool {
    near(p.x, x) && near(p.y, y)
}

#[test]
fn segment_de_72_pt_fait_25_4_mm() {
    let b = build(&one("0 0 m 72 0 l S"));
    let ImportItem::Segment { from, to } = &items(&b)[0] else {
        panic!()
    };
    assert!(close(*from, 0.0, 0.0) && close(*to, 25.4, 0.0));
    assert!(near(b.mm_per_unit, MM));
    assert_eq!(b.report.imported["path"], 1);
}

#[test]
fn rectangle_re_et_pile_de_transformations_q_cm_q() {
    // cm 2 0 0 2 72 0 : x' = 72 + 2 x ; rect (0 0 36 18) -> (72 0)-(144 36) pt = 25,4 .. 50,8 mm x 0 .. 12,7 mm.
    let b = build(&one("q 2 0 0 2 72 0 cm 0 0 36 18 re f Q 0 0 m 72 0 l S"));
    assert_eq!(b.report.objects, 5);
    let bb = b.bbox_mm.unwrap();
    assert!(near(bb.min.x, 0.0) && near(bb.max.x, 50.8));
    assert!(near(bb.max.y, 12.7));
    // Apres Q, la matrice redevient l'identite : le dernier segment fait 25,4 mm.
    let ImportItem::Segment { from, to } = items(&b).last().unwrap().clone() else {
        panic!()
    };
    assert!(close(from, 0.0, 0.0) && close(to, 25.4, 0.0));
    // cm imbriques : translation 72 puis echelle 2 = x' = 72 + 2 x.
    let b = build(&one(
        "q 1 0 0 1 72 0 cm q 2 0 0 2 0 0 cm 0 0 m 36 0 l S Q Q",
    ));
    let ImportItem::Segment { from, to } = &items(&b)[0] else {
        panic!()
    };
    assert!(close(*from, 25.4, 0.0) && close(*to, 50.8, 0.0));
}

#[test]
fn courbes_c_v_y_et_fermeture_h() {
    // v : 1er controle = point courant ; y : 2e controle = extremite.
    let b = build(&one(
        "0 0 m 72 0 72 72 144 72 c 144 72 m 216 72 288 0 v 288 0 m 300 100 360 0 y h S",
    ));
    let it = items(&b);
    let ImportItem::Bezier { c1, c2, end, .. } = &it[0] else {
        panic!()
    };
    assert!(close(*c1, 25.4, 0.0) && close(*c2, 25.4, 25.4) && close(*end, 50.8, 25.4));
    let ImportItem::Bezier { start, c1, c2, .. } = &it[1] else {
        panic!()
    };
    assert!(close(*start, 50.8, 25.4) && close(*c1, 50.8, 25.4));
    // "216 72 288 0 v" : 2e controle (216 ; 72), extremite (288 ; 0).
    assert!(close(*c2, 216.0 * MM, 72.0 * MM));
    let ImportItem::Bezier { c1, c2, end, .. } = &it[2] else {
        panic!()
    };
    assert!(near(c1.x, 300.0 * MM) && close(*c2, 360.0 * MM, 0.0) && close(*end, 360.0 * MM, 0.0));
    // h ferme le sous-chemin (288 0) -> (360 0) : segment de fermeture de 72 pt.
    assert!(matches!(it[3], ImportItem::Segment { .. }));
    assert_eq!(it.len(), 4);
}

#[test]
fn texte_image_et_decoupe_sont_listes() {
    let b = build(&one(
        "BT /F1 12 Tf 10 10 Td (Bonjour) Tj ET /Im1 Do 0 0 m 10 0 l S \
         BI /W 1 /H 1 /CS /G /BPC 8 ID \x01 EI \
         q 0 0 5 5 re W n Q /Sh sh 1 w 0 0 m 36 0 l S",
    ));
    assert_eq!(b.report.imported["path"], 2);
    assert_eq!(b.report.ignored_of("text"), 1);
    assert_eq!(b.report.ignored_of("xobject"), 1);
    assert_eq!(b.report.ignored_of("image"), 1);
    assert_eq!(b.report.ignored_of("shading"), 1);
    assert_eq!(b.report.note(NoteKind::ClipPathDropped), 1);
    // `w` (epaisseur de trait) n'a aucun effet : le segment fait toujours 36 pt = 12,7 mm.
    let ImportItem::Segment { to, .. } = items(&b).last().unwrap().clone() else {
        panic!()
    };
    assert!(near(to.x, 12.7));
}

#[test]
fn chemin_non_termine_est_liste() {
    let b = build(&one("0 0 m 10 0 l S 0 0 m 5 5 l"));
    assert_eq!(b.report.objects, 1);
    assert_eq!(b.report.ignored_of("path"), 1);
    assert_eq!(b.report.ignored[0].reason, IgnoreReason::Unterminated);
}

#[test]
fn flux_flate_et_page_choisie() {
    let p1 = miniz_oxide::deflate::compress_to_vec_zlib(b"0 0 m 72 0 l S", 6);
    let p2 = b"0 0 m 144 0 l S".to_vec();
    let bytes = pdf_of(&[p1, p2], "", "");
    // Le premier flux n'est pas declare Flate : lu tel quel -> rejet ou ignore, jamais silencieux.
    assert!(
        PdfDocument::parse(&bytes).is_err() || {
            let d = PdfDocument::parse(&bytes).unwrap();
            d.build(1, 1.0)
                .map(|b| b.report.objects == 0)
                .unwrap_or(true)
        }
    );
    // Page 1 declaree Flate (tous les flux de ce fichier le sont ici).
    let p1 = miniz_oxide::deflate::compress_to_vec_zlib(b"0 0 m 72 0 l S", 6);
    let p2 = miniz_oxide::deflate::compress_to_vec_zlib(b"0 0 m 144 0 l S", 6);
    let bytes = pdf_of(&[p1, p2], "/Filter /FlateDecode", "");
    let d = PdfDocument::parse(&bytes).unwrap();
    assert_eq!(d.page_count(), 2);
    let l = |page| {
        let b = d.build(page, 1.0).unwrap();
        b.bbox_mm.unwrap().width()
    };
    assert!(near(l(1), 25.4) && near(l(2), 50.8));
    assert_eq!(
        d.build(3, 1.0).unwrap_err(),
        VectorImportError::PageOutOfRange { page: 3, pages: 2 }
    );
    assert!(matches!(
        d.build(0, 1.0),
        Err(VectorImportError::PageOutOfRange { .. })
    ));
    // Echelle : 50 % divise les longueurs par deux.
    assert!(near(
        d.build(2, 0.5).unwrap().bbox_mm.unwrap().width(),
        25.4
    ));
    // [/FlateDecode] en tableau accepte aussi.
    let bytes = pdf_of(
        &[miniz_oxide::deflate::compress_to_vec_zlib(
            b"0 0 m 72 0 l S",
            6,
        )],
        "/Filter [/FlateDecode]",
        "",
    );
    assert!(near(build(&bytes).bbox_mm.unwrap().width(), 25.4));
}

#[test]
fn refus_clairs_chiffre_filtre_flux_d_objets_tronque() {
    let good = one("0 0 m 72 0 l S");
    assert!(PdfDocument::parse(&good).is_ok());
    assert_eq!(
        PdfDocument::parse(&pdf_of(&[b"0 0 m 1 1 l S".to_vec()], "", "/Encrypt 9 0 R"))
            .unwrap_err(),
        VectorImportError::Encrypted
    );
    assert_eq!(
        PdfDocument::parse(&pdf_of(&[b"x".to_vec()], "/Filter /LZWDecode", "")).unwrap_err(),
        VectorImportError::Unsupported("filter")
    );
    assert_eq!(
        PdfDocument::parse(&pdf_of(
            &[b"x".to_vec()],
            "/Filter [/ASCII85Decode /FlateDecode]",
            ""
        ))
        .unwrap_err(),
        VectorImportError::Unsupported("filter")
    );
    assert_eq!(
        PdfDocument::parse(&pdf_of(
            &[b"x".to_vec()],
            "/Filter /FlateDecode /DecodeParms << /Predictor 12 >>",
            ""
        ))
        .unwrap_err(),
        VectorImportError::Unsupported("predictor")
    );
    let mut objstm = good.clone();
    objstm.extend(
        b"9 0 obj\n<< /Type /ObjStm /N 1 /First 4 /Length 0 >>\nstream\n\nendstream\nendobj\n",
    );
    assert_eq!(
        PdfDocument::parse(&objstm).unwrap_err(),
        VectorImportError::Unsupported("object_streams")
    );
    // Tronque : plus de trailer, puis coupe au milieu du flux, puis au milieu d'un objet.
    let cut = |n: usize| PdfDocument::parse(&good[..n]).unwrap_err();
    let t = String::from_utf8_lossy(&good).find("trailer").unwrap();
    assert_eq!(cut(t), VectorImportError::Truncated);
    let s = String::from_utf8_lossy(&good).find("stream").unwrap();
    assert_eq!(cut(s + 12), VectorImportError::Truncated);
    assert!(matches!(
        cut(40),
        VectorImportError::Truncated | VectorImportError::Structure(_)
    ));
    assert_eq!(
        PdfDocument::parse(b"hello").unwrap_err(),
        VectorImportError::NotPdf
    );
    // Flate corrompu, operandes absurdes, Q sans q.
    let bad_flate = pdf_of(&[b"not zlib at all".to_vec()], "/Filter /FlateDecode", "");
    assert!(matches!(
        PdfDocument::parse(&bad_flate),
        Err(VectorImportError::Structure(_))
    ));
    for bad in [
        "0 0 l",
        "1 2 m 3 l S",
        "Q",
        "1 0 0 1 cm",
        "0 0 m 1 1 l h h S x",
    ] {
        let d = PdfDocument::parse(&one(bad)).unwrap();
        let r = d.build(1, 1.0);
        match bad {
            "0 0 m 1 1 l h h S x" => assert!(r.is_ok()),
            _ => assert!(matches!(r, Err(VectorImportError::Structure(_))), "{bad}"),
        }
    }
}

#[test]
fn rotation_de_page_et_operateur_inconnu_sont_notes() {
    let bytes = String::from_utf8(one("0 0 m 72 0 l S zz")).unwrap();
    let bytes = bytes.replace("/Type /Page /Parent", "/Type /Page /Rotate 90 /Parent");
    let b = build(bytes.as_bytes());
    assert_eq!(b.report.note(NoteKind::PageRotation), 1);
    assert_eq!(b.report.ignored_of("op zz"), 1);
}

#[test]
fn echelle_et_bornes_pdf() {
    let d = PdfDocument::parse(&one("0 0 m 1000000000 0 l S 0 0 m 72 0 l S")).unwrap();
    let b = d.build(1, 1.0).unwrap();
    assert_eq!(b.report.objects, 1);
    assert_eq!(b.report.ignored[0].reason, IgnoreReason::OutOfRange);
    assert_eq!(d.build(1, 0.0).unwrap_err(), VectorImportError::BadScale);
}

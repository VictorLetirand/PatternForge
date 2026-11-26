//! US-18.2 / US-18.4 / US-12.6 : le vrai binaire `patternforge-cli` sur des fichiers temporaires.
//! Valeurs attendues calculées à la main (commentaires) ou lues dans les fichiers d'exemple.

mod support;

use pf_core::persist;
use pf_export::{dxf, pdf, svg, Scene};
use support::*;

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

// ----- valider -----------------------------------------------------------------------------

#[test]
fn valider_codes_0_1_2() {
    let dir = temp_dir("valider");
    let ok = dir.join("a.pforge.json");
    save(&square_project("Carre", 80.0, false), &ok);
    let o = run(&["valider", &p(&ok)]);
    assert_eq!(code(&o), 0);
    assert!(stdout(&o).contains("valide") && stdout(&o).contains("Carre"));
    assert!(stdout(&o).contains("1 feuille(s)"));

    let bad = dir.join("b.pforge.json");
    std::fs::write(&bad, "{ \"schema_version\": 5, ").unwrap();
    let o = run(&["valider", &p(&bad)]);
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    assert!(stderr(&o).contains("invalide"));

    let other = dir.join("c.pforge.json");
    std::fs::write(&other, "[1, 2]").unwrap();
    assert_eq!(code(&run(&["valider", &p(&other)])), 1);

    let o = run(&["valider", &p(&dir.join("absent.pforge.json"))]);
    assert_eq!(code(&o), 2);
}

#[test]
fn valider_ne_modifie_pas_le_fichier() {
    let dir = temp_dir("valider-ro");
    let f = dir.join("a.pforge.json");
    save(&square_project("Carre", 80.0, false), &f);
    let before = std::fs::read(&f).unwrap();
    run(&["valider", &p(&f)]);
    assert_eq!(std::fs::read(&f).unwrap(), before);
}

#[test]
fn langue_en_messages_anglais() {
    let dir = temp_dir("langue");
    let f = dir.join("a.pforge.json");
    save(&square_project("Carre", 80.0, false), &f);
    let o = run(&["valider", &p(&f), "--langue", "en"]);
    assert_eq!(code(&o), 0);
    assert!(stdout(&o).contains("valid - project") && stdout(&o).contains("1 sheet(s)"));
    assert!(!stdout(&o).contains("feuille"));
    let o = run(&["valider", &p(&dir.join("x.pforge.json")), "--langue", "en"]);
    assert_eq!(code(&o), 2);
    let o = run(&["exporter", &p(&f), "--format", "zip", "--langue", "en"]);
    assert_eq!(code(&o), 3);
    assert!(
        stderr(&o).contains("format not supported"),
        "{}",
        stderr(&o)
    );
    let o = run(&["exporter", &p(&f), "--format", "zip"]);
    assert!(stderr(&o).contains("format non pris en charge"));
}

// ----- exporter : identité à l'octet avec la bibliothèque et les exemples -----------------

#[test]
fn exports_identiques_aux_exemples_du_depot() {
    let dir = temp_dir("exemples");
    let src = example("jupe-droite.pforge");
    for (fmt, ext, name) in [
        ("svg", "svg", "jupe-droite.svg"),
        ("pdf", "pdf", "jupe-droite.pdf"),
        ("dxf", "dxf", "jupe-droite.dxf"),
    ] {
        let out = dir.join(fmt);
        let o = run(&["exporter", &p(&src), "--format", fmt, "--sortie", &p(&out)]);
        assert_eq!(code(&o), 0, "{fmt} : {}", stderr(&o));
        let got = std::fs::read(out.join(format!("jupe-droite.{ext}"))).unwrap();
        let want = std::fs::read(example(name)).unwrap();
        assert!(got == want, "{fmt} : octets différents de examples/{name}");
    }
}

#[test]
fn hpgl_identique_a_la_bibliotheque_page_par_page() {
    let dir = temp_dir("hpgl");
    let src = example("jupe-droite.pforge");
    let project = persist::load(&src).unwrap();
    let scene = Scene::from_sheet(&project, project.sheets[0].id).unwrap();
    let tiles = pf_export::hpgl::export_hpgl_pages(
        &scene,
        &pf_export::PageSetup::default(),
        pf_export::hpgl::HpglOptions::default(),
    )
    .unwrap();
    assert!(tiles.pages.len() > 1);
    let o = run(&[
        "exporter",
        &p(&src),
        "--format",
        "hpgl",
        "--sortie",
        &p(&dir),
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    for page in &tiles.pages {
        let got = std::fs::read(dir.join(format!("jupe-droite-p{}.plt", page.number))).unwrap();
        assert!(got == page.bytes(), "page {}", page.number);
    }
    assert_eq!(names(&dir).len(), tiles.pages.len());
}

#[test]
fn exports_identiques_a_la_bibliotheque() {
    let dir = temp_dir("biblio");
    let project = square_project("Carre", 80.0, false);
    let f = dir.join("carre.pforge.json");
    save(&project, &f);
    let loaded = persist::load(&f).unwrap();
    let scene = Scene::from_sheet(&loaded, loaded.sheets[0].id).unwrap();
    let out = dir.join("out");
    assert_eq!(
        code(&run(&[
            "exporter",
            &p(&f),
            "--format",
            "svg",
            "--sortie",
            &p(&out)
        ])),
        0
    );
    assert_eq!(
        std::fs::read(out.join("carre.svg")).unwrap(),
        svg::export_svg(&scene).unwrap().into_bytes()
    );
    assert_eq!(
        code(&run(&[
            "convertir",
            &p(&f),
            "--vers",
            "pdf",
            "--sortie",
            &p(&out)
        ])),
        0
    );
    assert_eq!(
        std::fs::read(out.join("carre.pdf")).unwrap(),
        pdf::export_pdf(&[scene]).unwrap().bytes
    );
    // Aucune pièce : le DXF est refusé par la bibliothèque, donc code 1 sans fichier.
    assert!(dxf::export_dxf(&loaded).is_err());
    let o = run(&["exporter", &p(&f), "--format", "dxf", "--sortie", &p(&out)]);
    assert_eq!(code(&o), 1);
    assert!(!out.join("carre.dxf").exists());
}

#[test]
fn convertir_refuse_png_et_exige_vers() {
    let dir = temp_dir("convertir");
    let f = dir.join("a.pforge.json");
    save(&square_project("Carre", 80.0, false), &f);
    assert_eq!(code(&run(&["convertir", &p(&f), "--vers", "png"])), 3);
    assert_eq!(code(&run(&["convertir", &p(&f)])), 3);
}

// ----- écrasement, sortie, arguments -------------------------------------------------------

#[test]
fn jamais_d_ecrasement_silencieux() {
    let dir = temp_dir("ecraser");
    let f = dir.join("a.pforge.json");
    save(&square_project("Carre", 80.0, false), &f);
    let target = dir.join("a.svg");
    std::fs::write(&target, "ancien").unwrap();
    let o = run(&["exporter", &p(&f), "--format", "svg"]);
    assert_eq!(code(&o), 2);
    assert!(stderr(&o).contains("--ecraser"));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "ancien");
    let o = run(&["exporter", &p(&f), "--format", "svg", "--ecraser"]);
    assert_eq!(code(&o), 0);
    assert_ne!(std::fs::read_to_string(&target).unwrap(), "ancien");
}

#[test]
fn sortie_fichier_et_sortie_dossier() {
    let dir = temp_dir("sortie");
    let f = dir.join("a.pforge.json");
    save(&square_project("Carre", 80.0, false), &f);
    let file = dir.join("mon-plan.svg");
    assert_eq!(
        code(&run(&[
            "exporter",
            &p(&f),
            "--format",
            "svg",
            "--sortie",
            &p(&file)
        ])),
        0
    );
    assert!(file.exists());
    let sub = dir.join("nouveau").join("dossier");
    assert_eq!(
        code(&run(&[
            "exporter",
            &p(&f),
            "--format",
            "svg",
            "--sortie",
            &p(&sub)
        ])),
        0
    );
    assert!(sub.join("a.svg").exists());
}

#[test]
fn arguments_invalides_code_3() {
    let dir = temp_dir("args");
    let f = dir.join("a.pforge.json");
    save(&square_project("Carre", 80.0, false), &f);
    let f = p(&f);
    for args in [
        vec!["exporter", &f],
        vec!["exporter", &f, "--format", "zip"],
        vec!["exporter", &f, "--format", "svg", "--option-inconnue"],
        vec!["exporter", &f, "--format", "svg", "--dpi", "300"],
        vec!["exporter", &f, "--format", "png", "--dpi", "35"],
        vec!["exporter", &f, "--format", "png", "--dpi", "1201"],
        vec!["exporter", &f, "--format", "png", "--dpi", "abc"],
        vec!["exporter", &f, "--format", "png", "--dpi", "300.5"],
        vec![
            "exporter",
            &f,
            "--format",
            "svg",
            "--taille",
            "S",
            "--tailles-toutes",
        ],
        vec!["exporter", &f, "--format", "svg", "--sortie"],
        vec!["exporter", &f, "--format", "svg", "--langue", "de"],
        vec!["lot"],
        vec!["inconnue"],
        vec![],
    ] {
        assert_eq!(code(&run(&args)), 3, "{args:?}");
    }
    assert_eq!(code(&run(&["--aide"])), 0);
}

// ----- tailles -----------------------------------------------------------------------------

#[test]
fn tailles_inconnue_nommee_et_toutes() {
    let dir = temp_dir("tailles");
    let f = dir.join("a.pforge.json");
    save(&square_project("Carre", 80.0, true), &f);
    let out = dir.join("out");
    let o = run(&[
        "exporter",
        &p(&f),
        "--format",
        "svg",
        "--taille",
        "XXL",
        "--sortie",
        &p(&out),
    ]);
    assert_eq!(code(&o), 4);
    assert!(stderr(&o).contains("S, M, L"), "{}", stderr(&o));
    assert!(!out.exists());

    let o = run(&[
        "exporter",
        &p(&f),
        "--format",
        "svg",
        "--taille",
        "l",
        "--sortie",
        &p(&out),
    ]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    assert_eq!(names(&out), vec!["a-L.svg"]);
    // La base M garde le nom simple.
    let o = run(&[
        "exporter",
        &p(&f),
        "--format",
        "svg",
        "--taille",
        "M",
        "--sortie",
        &p(&out),
    ]);
    assert_eq!(code(&o), 0);
    assert_eq!(names(&out), vec!["a-L.svg", "a.svg"]);
    let l = std::fs::read_to_string(out.join("a-L.svg")).unwrap();
    // L : carré 80 -> sommets 0 / 90 sur x et y sauf le premier : emprise 90 x 90 + 2 x 10 de marge.
    assert!(l.contains("width=\"110mm\""), "{}", &l[..200.min(l.len())]);
    let m = std::fs::read_to_string(out.join("a.svg")).unwrap();
    assert!(m.contains("width=\"100mm\""));

    let all = dir.join("all");
    let o = run(&[
        "exporter",
        &p(&f),
        "--format",
        "svg",
        "--tailles-toutes",
        "--sortie",
        &p(&all),
    ]);
    assert_eq!(code(&o), 0);
    assert_eq!(names(&all), vec!["a-L.svg", "a-M.svg", "a-S.svg"]);
    // Projet sans taille : --taille est refusé (code 4), --tailles-toutes exporte la base.
    let g = dir.join("g.pforge.json");
    save(&square_project("Sans", 80.0, false), &g);
    assert_eq!(
        code(&run(&[
            "exporter",
            &p(&g),
            "--format",
            "svg",
            "--taille",
            "M"
        ])),
        4
    );
    let o = run(&["exporter", &p(&g), "--format", "svg", "--tailles-toutes"]);
    assert_eq!(code(&o), 0);
    assert!(dir.join("g.svg").exists());
}

// ----- PNG / JPEG (US-12.6) ----------------------------------------------------------------

fn decode_png(path: &std::path::Path) -> (png::OutputInfo, Vec<u8>, png::Info<'static>) {
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = dec.read_info().unwrap();
    let info = reader.info().clone();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let out = reader.next_frame(&mut buf).unwrap();
    (out, buf, info)
}

#[test]
fn png_dimensions_resolution_et_pixels() {
    let dir = temp_dir("png");
    let f = dir.join("c.pforge.json");
    // 80 mm + 2 x 10 mm de marge = 100 mm ; 100 / 25,4 x 300 = 1181,10 -> 1181 px.
    save(&square_project("Carre", 80.0, false), &f);
    let o = run(&["exporter", &p(&f), "--format", "png", "--dpi", "300"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let (out, buf, info) = decode_png(&dir.join("c.png"));
    assert_eq!((out.width, out.height), (1181, 1181));
    assert_eq!(out.color_type, png::ColorType::Grayscale);
    // pHYs : 300 / 0,0254 = 11 811,02 -> 11 811 pixels par mètre.
    let dims = info.pixel_dims.expect("chunk pHYs");
    assert_eq!((dims.xppu, dims.yppu), (11_811, 11_811));
    assert_eq!(dims.unit, png::Unit::Meter);
    let px = |x: usize, y: usize| buf[y * 1181 + x];
    // Coin du dessin : trait gauche à x = 10 mm = 118,1 px (épaisseur 0,25 mm = 2,95 px).
    assert!(px(118, 600) < 40, "trait gauche : {}", px(118, 600));
    assert!(px(1181 - 119, 600) < 40, "trait droit : {}", px(1062, 600));
    assert!(px(600, 118) < 40 && px(600, 1062) < 40);
    // Fond blanc : centre de l'image, et marge.
    assert_eq!(px(600, 600), 255);
    assert_eq!(px(5, 5), 255);
    assert_eq!(px(1175, 1175), 255);
    // Trois pixels de large au plus de part et d'autre du trait.
    assert_eq!(px(110, 600), 255);
    assert_eq!(px(126, 600), 255);
}

#[test]
fn png_autres_resolutions_dimensions_a_la_main() {
    let dir = temp_dir("png-dpi");
    let f = dir.join("c.pforge.json");
    save(&square_project("Carre", 80.0, false), &f);
    // 100 mm : 72 dpi -> 283,46 = 283 ; 36 dpi -> 141,73 = 142 ; 1200 dpi -> 4724,41 = 4724.
    for (dpi, px, ppm) in [(72u32, 283u32, 2835u32), (36, 142, 1417)] {
        let out = dir.join(format!("o{dpi}"));
        let o = run(&[
            "exporter",
            &p(&f),
            "--format",
            "png",
            "--dpi",
            &dpi.to_string(),
            "--sortie",
            &p(&out),
        ]);
        assert_eq!(code(&o), 0, "{}", stderr(&o));
        let (info, _, i2) = decode_png(&out.join("c.png"));
        assert_eq!((info.width, info.height), (px, px), "{dpi} dpi");
        assert_eq!(i2.pixel_dims.unwrap().xppu, ppm, "{dpi} dpi");
    }
    // Défaut : 300 dpi.
    let o = run(&[
        "exporter",
        &p(&f),
        "--format",
        "png",
        "--sortie",
        &p(&dir.join("d")),
    ]);
    assert_eq!(code(&o), 0);
    assert_eq!(decode_png(&dir.join("d").join("c.png")).0.width, 1181);
}

#[test]
fn jpeg_dimensions_densite_et_pixels() {
    let dir = temp_dir("jpeg");
    let f = dir.join("c.pforge.json");
    save(&square_project("Carre", 80.0, false), &f);
    let o = run(&["exporter", &p(&f), "--format", "jpeg", "--dpi", "300"]);
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let bytes = std::fs::read(dir.join("c.jpg")).unwrap();
    // JFIF : FF D8 FF E0, longueur, "JFIF\0", version (2), unité 1 = points par pouce, 300 x 300.
    assert_eq!(&bytes[..4], &[0xFF, 0xD8, 0xFF, 0xE0]);
    assert_eq!(&bytes[6..11], b"JFIF\0");
    assert_eq!(bytes[13], 1);
    assert_eq!(u16::from_be_bytes([bytes[14], bytes[15]]), 300);
    assert_eq!(u16::from_be_bytes([bytes[16], bytes[17]]), 300);
    let mut dec = zune_jpeg::JpegDecoder::new(&bytes);
    let pixels = dec.decode().unwrap();
    let info = dec.info().unwrap();
    assert_eq!((info.width, info.height), (1181, 1181));
    let comps = pixels.len() / (1181 * 1181);
    let px = |x: usize, y: usize| pixels[(y * 1181 + x) * comps];
    assert!(px(118, 600) < 90, "trait : {}", px(118, 600));
    assert!(px(600, 600) > 220, "fond : {}", px(600, 600));
}

#[test]
fn png_refuse_image_trop_grande_sans_ecrire() {
    let dir = temp_dir("png-grand");
    let f = dir.join("g.pforge.json");
    // 5000 mm a 1200 dpi : 236 220 px de côté (> 65 535) : refus propre.
    save(&square_project("Grand", 5000.0, false), &f);
    let o = run(&["exporter", &p(&f), "--format", "png", "--dpi", "1200"]);
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    assert!(!dir.join("g.png").exists());
}

// ----- lot ---------------------------------------------------------------------------------

#[test]
fn lot_rapport_ordre_et_code() {
    let dir = temp_dir("lot");
    let input = dir.join("in");
    std::fs::create_dir_all(input.join("sous")).unwrap();
    save(
        &square_project("B", 80.0, false),
        &input.join("b.pforge.json"),
    );
    save(
        &square_project("A", 80.0, false),
        &input.join("a.pforge.json"),
    );
    std::fs::write(input.join("c.pforge.json"), "{ pas du json").unwrap();
    std::fs::write(input.join("notes.txt"), "ignoré").unwrap();
    save(
        &square_project("S", 80.0, false),
        &input.join("sous").join("a.pforge.json"),
    );
    let before = names(&input);
    let out = dir.join("out");
    let o = run(&[
        "lot",
        &p(&input),
        "--format",
        "svg",
        "--sortie",
        &p(&out),
        "--rapport-csv",
        "rapport.csv",
    ]);
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    let text = stdout(&o);
    let (ia, ib, ic) = (
        text.find("a.pforge.json").unwrap(),
        text.find("b.pforge.json").unwrap(),
        text.find("c.pforge.json").unwrap(),
    );
    assert!(ia < ib && ib < ic, "ordre trié : {text}");
    assert!(
        text.contains("2 projet(s) exporté(s), 1 en échec, 3 traité(s)."),
        "{text}"
    );
    assert_eq!(names(&out), vec!["a.svg", "b.svg", "rapport.csv"]);
    let csv = std::fs::read_to_string(out.join("rapport.csv")).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "fichier;statut;code;fichiers;message");
    assert_eq!(lines.len(), 4);
    assert!(lines[1].starts_with("a.pforge.json;ok;0;1;"));
    assert!(lines[3].starts_with("c.pforge.json;echec;1;0;"));
    // Rien n'est écrit dans le dossier d'entrée.
    assert_eq!(names(&input), before);

    // Récursif : le sous-dossier est traité, noms rendus uniques (a, a-2).
    let rec = dir.join("rec");
    let o = run(&[
        "lot",
        &p(&input),
        "--format",
        "svg",
        "--sortie",
        &p(&rec),
        "--recursif",
    ]);
    assert_eq!(code(&o), 1);
    assert_eq!(names(&rec), vec!["a-2.svg", "a.svg", "b.svg"]);
    // Les projets sont triés par chemin relatif : « a.pforge.json » avant « sous/a.pforge.json ».
    let t = stdout(&o);
    assert!(t.find("sous/a.pforge.json").unwrap() > t.find("c.pforge.json").unwrap());
}

#[test]
fn lot_sans_echec_code_0_et_refus_d_ecrasement() {
    let dir = temp_dir("lot-ok");
    let input = dir.join("in");
    std::fs::create_dir_all(&input).unwrap();
    save(
        &square_project("A", 80.0, true),
        &input.join("a.pforge.json"),
    );
    let out = dir.join("out");
    let o = run(&[
        "lot",
        &p(&input),
        "--format",
        "png",
        "--dpi",
        "72",
        "--tailles-toutes",
        "--sortie",
        &p(&out),
    ]);
    assert_eq!(code(&o), 0, "{}{}", stdout(&o), stderr(&o));
    assert_eq!(names(&out), vec!["a-L.png", "a-M.png", "a-S.png"]);
    // Deuxième passage sans --ecraser : le projet est en échec (code 2 listé), code global 1.
    let o = run(&[
        "lot",
        &p(&input),
        "--format",
        "png",
        "--dpi",
        "72",
        "--tailles-toutes",
        "--sortie",
        &p(&out),
    ]);
    assert_eq!(code(&o), 1);
    assert!(stdout(&o).contains("code 2"), "{}", stdout(&o));
    let o = run(&[
        "lot",
        &p(&input),
        "--format",
        "png",
        "--dpi",
        "72",
        "--tailles-toutes",
        "--sortie",
        &p(&out),
        "--ecraser",
    ]);
    assert_eq!(code(&o), 0);
}

#[test]
fn lot_dossier_absent_vide_et_rapport_hors_sortie() {
    let dir = temp_dir("lot-err");
    let out = dir.join("out");
    let o = run(&[
        "lot",
        &p(&dir.join("absent")),
        "--format",
        "svg",
        "--sortie",
        &p(&out),
    ]);
    assert_eq!(code(&o), 2);
    let empty = dir.join("vide");
    std::fs::create_dir_all(&empty).unwrap();
    let o = run(&["lot", &p(&empty), "--format", "svg", "--sortie", &p(&out)]);
    assert_eq!(code(&o), 0);
    assert!(stdout(&o).contains("aucun fichier"));
    for bad in ["../x.csv", "sub/x.csv", "C:\\x.csv", ""] {
        let o = run(&[
            "lot",
            &p(&empty),
            "--format",
            "svg",
            "--sortie",
            &p(&out),
            "--rapport-csv",
            bad,
        ]);
        assert_eq!(code(&o), 3, "{bad}");
    }
    assert_eq!(code(&run(&["lot", &p(&empty), "--format", "svg"])), 3);
}

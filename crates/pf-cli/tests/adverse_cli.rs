//! US-18.2 / US-18.4 : campagne adverse de la ligne de commande, 160 invocations à graine fixe
//! (+ 40 lots). Chaque invocation tire une commande, un fichier (valide, tronqué, binaire, vide,
//! absent, mauvaise version, clé dupliquée) et des options (valides ou non). L'**oracle** prédit
//! le code de retour par une table écrite ici, sans lire le code de la CLI : arguments invalides
//! (3) avant fichier (2 absent ou octets non UTF-8, 1 invalide) avant taille (4) avant export (DXF sans pièce : 1) ;
//! refus d'écraser (2). Jamais de panique (code 101, message « panicked »).

mod support;

use support::*;

const SEQUENCES: u64 = 160;
const LOTS: u64 = 40;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Valid,
    ValidSizes,
    Truncated,
    Garbage,
    Missing,
    WrongVersion,
    Empty,
    DuplicateKey,
}

const KINDS: [Kind; 8] = [
    Kind::Valid,
    Kind::ValidSizes,
    Kind::Truncated,
    Kind::Garbage,
    Kind::Missing,
    Kind::WrongVersion,
    Kind::Empty,
    Kind::DuplicateKey,
];

fn valid_json(rng: &mut Rng, sizes: bool) -> String {
    let name = format!("P{}", rng.below(1000));
    pf_core::persist::to_json(&square_project(&name, 80.0, sizes)).expect("sérialisation")
}

fn write_kind(kind: Kind, path: &std::path::Path, rng: &mut Rng) {
    match kind {
        Kind::Valid => std::fs::write(path, valid_json(rng, false)).unwrap(),
        Kind::ValidSizes => std::fs::write(path, valid_json(rng, true)).unwrap(),
        Kind::Truncated => {
            let t = valid_json(rng, false);
            let cut = 1 + rng.below(t.len() as u64 - 2) as usize;
            // Coupe sur une frontière de caractère.
            let mut c = cut;
            while !t.is_char_boundary(c) {
                c -= 1;
            }
            std::fs::write(path, &t[..c.max(1)]).unwrap();
        }
        Kind::Garbage => {
            let bytes: Vec<u8> = (0..64 + rng.below(200))
                .map(|_| rng.below(256) as u8)
                .collect();
            std::fs::write(path, bytes).unwrap();
        }
        Kind::Missing => {}
        Kind::WrongVersion => {
            let t = valid_json(rng, false);
            assert!(t.contains("\"schema_version\": 6") || t.contains("\"schema_version\":6"));
            let t = t
                .replace("\"schema_version\": 6", "\"schema_version\": 99")
                .replace("\"schema_version\":6", "\"schema_version\":99");
            std::fs::write(path, t).unwrap();
        }
        Kind::Empty => std::fs::write(path, "").unwrap(),
        Kind::DuplicateKey => {
            let t = valid_json(rng, false);
            let t = t.replacen('{', "{\"schema_version\": 6,", 1);
            std::fs::write(path, t).unwrap();
        }
    }
}

#[test]
fn campagne_adverse_invocations() {
    let mut seen = [0usize; 5];
    let mut refusals = 0;
    let mut by_kind = std::collections::BTreeMap::<String, usize>::new();
    for seq in 0..SEQUENCES {
        let mut rng = Rng(seq.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for _ in 0..3 {
            rng.next();
        }
        let dir = temp_dir("adv");
        let file = dir.join("f.pforge.json");
        let kind = if rng.below(2) == 0 {
            KINDS[rng.below(2) as usize]
        } else {
            KINDS[rng.below(KINDS.len() as u64) as usize]
        };
        write_kind(kind, &file, &mut rng);
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();

        let cmd = ["valider", "exporter", "convertir"][rng.below(3) as usize];
        let fmt_pool = [
            "pdf", "svg", "dxf", "hpgl", "png", "jpeg", "PNG", "jpg", "zip", "",
        ];
        let fmt = fmt_pool[rng.below(fmt_pool.len() as u64) as usize];
        let mut args: Vec<String> = vec![cmd.into(), p(&file)];
        let mut arg_error = false;
        // Prédiction du nom de fichier (pour le refus d'écraser) et du format effectif.
        let mut format_ok = None;
        match cmd {
            "valider" => {
                if rng.below(5) == 0 {
                    args.push(p(&dir.join("deuxieme.pforge.json")));
                    arg_error = true;
                }
            }
            _ => {
                let flag = if cmd == "exporter" {
                    "--format"
                } else {
                    "--vers"
                };
                if rng.below(12) == 0 {
                    arg_error = true; // option absente
                } else {
                    args.push(flag.into());
                    args.push(fmt.into());
                    let norm = fmt.to_lowercase();
                    let known = ["pdf", "svg", "dxf", "hpgl", "png", "jpeg", "jpg"];
                    if !known.contains(&norm.as_str())
                        || (cmd == "convertir" && ["png", "jpeg", "jpg"].contains(&norm.as_str()))
                    {
                        arg_error = true;
                    } else {
                        format_ok = Some(norm);
                    }
                }
            }
        }
        args.push("--sortie".into());
        args.push(p(&out));
        let (mut size_flag, mut all) = (None, false);
        let mut ecraser = false;
        if cmd != "valider" {
            if rng.below(3) == 0 {
                let dpi =
                    ["36", "300", "1200", "35", "1201", "abc", "-5", ""][rng.below(8) as usize];
                args.push("--dpi".into());
                args.push(dpi.into());
                let in_range = dpi.parse::<u32>().is_ok_and(|d| (36..=1200).contains(&d));
                let raster = matches!(format_ok.as_deref(), Some("png" | "jpeg" | "jpg"));
                if !in_range || (format_ok.is_some() && !raster) {
                    arg_error = true;
                }
            }
            if rng.below(2) == 0 {
                let s = ["S", "M", "l", "XXL", ""][rng.below(5) as usize];
                args.push("--taille".into());
                args.push(s.into());
                size_flag = Some(s);
            }
            if rng.below(4) == 0 {
                args.push("--tailles-toutes".into());
                all = true;
            }
            if size_flag.is_some() && all {
                arg_error = true;
            }
            if rng.below(6) == 0 {
                args.push("--ecraser".into());
                ecraser = true;
            }
        }
        if rng.below(10) == 0 {
            args.push("--langue".into());
            let l = ["fr", "en", "de"][rng.below(3) as usize];
            args.push(l.into());
            if l == "de" {
                arg_error = true;
            }
        }
        if rng.below(15) == 0 {
            args.push("--option-inconnue".into());
            arg_error = true;
        }
        if rng.below(15) == 0 {
            args.push("--dpi".into()); // valeur manquante en fin de ligne
            arg_error = true;
        }

        // Fichier préexistant (refus d'écraser) : seulement quand le nom est prévisible.
        let mut preexisting = false;
        if !arg_error && cmd != "valider" && size_flag.is_none() && !all {
            let ext = match format_ok.as_deref() {
                Some("jpeg" | "jpg") => "jpg",
                Some("hpgl") => "plt",
                Some(e) => e,
                None => "x",
            };
            std::fs::write(out.join(format!("f.{ext}")), "ancien").unwrap();
            preexisting = true;
        }

        // Oracle.
        let has_sizes = kind == Kind::ValidSizes;
        let loadable = matches!(kind, Kind::Valid | Kind::ValidSizes);
        let expected = if arg_error {
            3
        } else if matches!(kind, Kind::Missing | Kind::Garbage) {
            2
        } else if !loadable {
            1
        } else if cmd == "valider" {
            0
        } else if size_flag
            .is_some_and(|s| !has_sizes || !["S", "M", "L"].contains(&s.to_uppercase().as_str()))
        {
            4
        } else if format_ok.as_deref() == Some("dxf") {
            1
        } else if preexisting && !ecraser {
            2
        } else {
            0
        };

        let o = run_s(&args);
        let c = code(&o);
        let msg = format!(
            "{args:?} ({kind:?}) stdout={} stderr={}",
            stdout(&o),
            stderr(&o)
        );
        assert!(!stderr(&o).contains("panicked"), "panique : {msg}");
        assert!((0..=4).contains(&c), "code hors table {c} : {msg}");
        assert_eq!(c, expected, "{msg}");
        seen[c as usize] += 1;
        *by_kind.entry(format!("{kind:?}")).or_default() += 1;
        if preexisting && !ecraser && expected == 2 {
            refusals += 1;
            assert_eq!(
                std::fs::read_to_string(
                    out.join(
                        std::fs::read_dir(&out)
                            .unwrap()
                            .next()
                            .unwrap()
                            .unwrap()
                            .file_name()
                    )
                )
                .unwrap(),
                "ancien"
            );
        }
        // Un refus n'écrit rien (hors fichier préexistant) ; le fichier source n'est jamais touché.
        if expected != 0 && !preexisting {
            assert_eq!(
                std::fs::read_dir(&out).unwrap().count(),
                0,
                "écriture malgré le refus : {msg}"
            );
        }
        if expected == 0 && cmd != "valider" {
            assert!(std::fs::read_dir(&out).unwrap().count() >= 1, "{msg}");
        }
    }
    for (code, n) in seen.iter().enumerate() {
        assert!(*n >= 5, "code {code} vu {n} fois seulement : {seen:?}");
    }
    assert!(refusals >= 3, "refus d'écraser : {refusals}");
    assert!(by_kind.len() == KINDS.len(), "{by_kind:?}");
}

#[test]
fn campagne_adverse_lots() {
    let (mut ok_lots, mut failed_lots) = (0, 0);
    for seq in 0..LOTS {
        let mut rng = Rng(seq.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
        for _ in 0..3 {
            rng.next();
        }
        let dir = temp_dir("adv-lot");
        let input = dir.join("in");
        std::fs::create_dir_all(&input).unwrap();
        let n = rng.below(5);
        let mut expected_ok = 0;
        let mut any_bad = false;
        for i in 0..n {
            let kind = [
                Kind::Valid,
                Kind::ValidSizes,
                Kind::Truncated,
                Kind::Garbage,
                Kind::WrongVersion,
                Kind::Empty,
            ][rng.below(6) as usize];
            write_kind(kind, &input.join(format!("p{i}.pforge.json")), &mut rng);
            if matches!(kind, Kind::Valid | Kind::ValidSizes) {
                expected_ok += 1;
            } else {
                any_bad = true;
            }
        }
        std::fs::write(input.join("ignore.json"), "{}").unwrap();
        let out = dir.join("out");
        let fmt = ["svg", "pdf", "png", "dxf"][rng.below(4) as usize];
        let mut args: Vec<String> = vec![
            "lot".into(),
            p(&input),
            "--format".into(),
            fmt.into(),
            "--sortie".into(),
            p(&out),
        ];
        if fmt == "png" {
            args.extend(["--dpi".into(), "36".into()]);
        }
        let o = run_s(&args);
        let msg = format!("{args:?} stdout={} stderr={}", stdout(&o), stderr(&o));
        assert!(!stderr(&o).contains("panicked"), "{msg}");
        // DXF : aucun projet n'a de pièce, donc tous échouent.
        let ok_count = if fmt == "dxf" { 0 } else { expected_ok };
        let want = if ok_count < n { 1 } else { 0 };
        assert_eq!(code(&o), want, "{msg}");
        let written = std::fs::read_dir(&out).map(|d| d.count()).unwrap_or(0);
        assert_eq!(written, ok_count as usize, "{msg}");
        assert!(stdout(&o).contains(&format!("{n} traité(s)")), "{msg}");
        if want == 0 {
            ok_lots += 1;
        } else {
            failed_lots += 1;
        }
        let _ = any_bad;
        // Entrée intacte : n projets + ignore.json.
        assert_eq!(std::fs::read_dir(&input).unwrap().count(), n as usize + 1);
    }
    assert!(
        ok_lots >= 5 && failed_lots >= 5,
        "{ok_lots} / {failed_lots}"
    );
}

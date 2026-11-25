//! US-10.1 (barème de mesures × tailles, CSV) et US-10.2 (fiche client, mesures manquantes,
//! taille la plus proche). Valeurs calculées à la main dans chaque test.
//!
//! Fixture : gamme « Femme » S M L (base M) ;
//!
//! | mesure           | unité | S   | M    | L   |
//! |------------------|-------|-----|------|-----|
//! | Tour de poitrine | mm    | 880 | 920  | 960 |
//! | Longueur dos     | mm    | 400 | 410  | 420 |
//! | Angle d'épaule   | deg   | 20  | 21,5 | 22  |
//!
//! Fiche « Alice » : poitrine 930, dos 408, épaule 20 → écarts (client − taille) :
//! S +50 / +8 / 0 (score mm 58), M +10 / −2 / −1,5 (score 12), L −30 / −12 / −2 (score 42).

use pf_core::measurement::MAX_MEASUREMENTS;
use pf_core::persist;
use pf_core::*;

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

struct F {
    p: Project,
    s: SizeId,
    m: SizeId,
    l: SizeId,
    range: SizeRangeId,
    poitrine: MeasurementId,
    dos: MeasurementId,
    epaule: MeasurementId,
}

fn add_m(p: &mut Project, name: &str, unit: MeasurementUnit) -> MeasurementId {
    p.apply(Command::AddMeasurement {
        name: name.into(),
        unit,
    })
    .unwrap()
    .measurements[0]
}

fn set(p: &mut Project, m: MeasurementId, s: SizeId, v: f64) {
    p.apply(Command::SetMeasurementValue {
        measurement: m,
        size: s,
        value: v,
    })
    .unwrap();
}

fn fixture() -> F {
    let mut p = Project::new("Bareme");
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let r = p.sizes[0].clone();
    let (s, m, l) = (r.sizes[0].id, r.sizes[1].id, r.sizes[2].id);
    let poitrine = add_m(&mut p, "Tour de poitrine", MeasurementUnit::Mm);
    let dos = add_m(&mut p, "Longueur dos", MeasurementUnit::Mm);
    let epaule = add_m(&mut p, "Angle d'épaule", MeasurementUnit::Deg);
    for (me, vals) in [
        (poitrine, [880.0, 920.0, 960.0]),
        (dos, [400.0, 410.0, 420.0]),
        (epaule, [20.0, 21.5, 22.0]),
    ] {
        for (sz, v) in [s, m, l].into_iter().zip(vals) {
            set(&mut p, me, sz, v);
        }
    }
    F {
        p,
        s,
        m,
        l,
        range: r.id,
        poitrine,
        dos,
        epaule,
    }
}

fn client(p: &mut Project, name: &str, vals: &[(MeasurementId, f64)]) -> ClientId {
    let c = p
        .apply(Command::AddClient { name: name.into() })
        .unwrap()
        .clients[0];
    for (m, v) in vals {
        p.apply(Command::SetClientValue {
            client: c,
            measurement: *m,
            value: *v,
        })
        .unwrap();
    }
    c
}

fn value(p: &Project, m: MeasurementId, s: SizeId) -> Option<f64> {
    p.measurement(m)
        .unwrap()
        .values
        .iter()
        .find(|v| v.size == s)
        .map(|v| v.value)
}

/// Refus sans trace : octets du projet identiques.
fn refused(p: &mut Project, c: Command) -> String {
    let before = bytes(p);
    let snapshot = p.clone();
    let e = p.apply(c).expect_err("doit être refusée").to_string();
    assert_eq!(bytes(p), before, "refus sans trace");
    assert_eq!(*p, snapshot);
    e
}

// ---------- US-10.1 : commandes ----------

#[test]
fn ca1_ajouter_renommer_supprimer_une_mesure() {
    let mut f = fixture();
    assert_eq!(f.p.measurements.len(), 3);
    assert_eq!(f.p.measurements[0].name, "Tour de poitrine");
    assert_eq!(f.p.measurements[2].unit, MeasurementUnit::Deg);
    // Les ids sont stables et jamais réutilisés.
    let id = add_m(&mut f.p, "  Hanche  ", MeasurementUnit::Mm);
    assert_eq!(f.p.measurement(id).unwrap().name, "Hanche", "nom rogné");
    f.p.apply(Command::RenameMeasurement {
        measurement: id,
        name: "Tour de hanche".into(),
    })
    .unwrap();
    assert_eq!(f.p.measurement(id).unwrap().name, "Tour de hanche");
    f.p.apply(Command::RemoveMeasurement { measurement: id })
        .unwrap();
    assert!(f.p.measurement(id).is_err());
    let again = add_m(&mut f.p, "Hanche", MeasurementUnit::Mm);
    assert_ne!(again, id, "id jamais réutilisé");
    // Renommer en changeant seulement la casse de son propre nom est permis.
    f.p.apply(Command::RenameMeasurement {
        measurement: f.dos,
        name: "LONGUEUR DOS".into(),
    })
    .unwrap();
    assert_eq!(f.p.measurement(f.dos).unwrap().name, "LONGUEUR DOS");
}

#[test]
fn ca2_valeur_par_taille_definir_remplacer_effacer() {
    let mut f = fixture();
    assert_eq!(value(&f.p, f.poitrine, f.s), Some(880.0));
    set(&mut f.p, f.poitrine, f.s, 881.5);
    assert_eq!(value(&f.p, f.poitrine, f.s), Some(881.5));
    assert_eq!(f.p.measurement(f.poitrine).unwrap().values.len(), 3);
    f.p.apply(Command::ClearMeasurementValue {
        measurement: f.poitrine,
        size: f.s,
    })
    .unwrap();
    assert_eq!(value(&f.p, f.poitrine, f.s), None, "non renseignée");
    assert_eq!(value(&f.p, f.poitrine, f.m), Some(920.0), "autres intactes");
    // Zéro est une valeur valide, distincte de « non renseignée ».
    set(&mut f.p, f.poitrine, f.s, 0.0);
    assert_eq!(value(&f.p, f.poitrine, f.s), Some(0.0));
}

#[test]
fn ca3_refus_sans_trace() {
    let mut f = fixture();
    let c = client(&mut f.p, "Alice", &[(f.poitrine, 930.0)]);
    let ghost_m = MeasurementId(999_999);
    let ghost_s = SizeId(999_999);
    let cases: Vec<(Command, &str)> = vec![
        (
            Command::AddMeasurement {
                name: "".into(),
                unit: MeasurementUnit::Mm,
            },
            "nom vide",
        ),
        (
            Command::AddMeasurement {
                name: "   ".into(),
                unit: MeasurementUnit::Mm,
            },
            "nom blanc",
        ),
        (
            Command::AddMeasurement {
                name: "tour de POITRINE".into(),
                unit: MeasurementUnit::Mm,
            },
            "nom pris, casse ignorée",
        ),
        (
            Command::RenameMeasurement {
                measurement: f.dos,
                name: "Tour de poitrine".into(),
            },
            "renommer en nom pris",
        ),
        (
            Command::RenameMeasurement {
                measurement: ghost_m,
                name: "X".into(),
            },
            "mesure inconnue",
        ),
        (
            Command::RemoveMeasurement {
                measurement: ghost_m,
            },
            "suppression inconnue",
        ),
        (
            Command::SetMeasurementValue {
                measurement: f.dos,
                size: f.s,
                value: -1.0,
            },
            "négative",
        ),
        (
            Command::SetMeasurementValue {
                measurement: f.dos,
                size: f.s,
                value: f64::NAN,
            },
            "NaN",
        ),
        (
            Command::SetMeasurementValue {
                measurement: f.dos,
                size: f.s,
                value: f64::INFINITY,
            },
            "infinie",
        ),
        (
            Command::SetMeasurementValue {
                measurement: f.dos,
                size: f.s,
                value: 1_000_000.5,
            },
            "hors bornes",
        ),
        (
            Command::SetMeasurementValue {
                measurement: f.dos,
                size: ghost_s,
                value: 1.0,
            },
            "taille inconnue",
        ),
        (
            Command::SetMeasurementValue {
                measurement: ghost_m,
                size: f.s,
                value: 1.0,
            },
            "mesure inconnue",
        ),
        (
            Command::ClearMeasurementValue {
                measurement: f.dos,
                size: ghost_s,
            },
            "effacer : taille inconnue",
        ),
        (
            Command::AddClient {
                name: "alice".into(),
            },
            "fiche : nom pris",
        ),
        (
            Command::SetClientValue {
                client: c,
                measurement: f.dos,
                value: -0.001,
            },
            "fiche : négative",
        ),
        (
            Command::SetClientValue {
                client: c,
                measurement: ghost_m,
                value: 1.0,
            },
            "fiche : mesure inconnue",
        ),
        (
            Command::SetClientValue {
                client: ClientId(999_999),
                measurement: f.dos,
                value: 1.0,
            },
            "fiche inconnue",
        ),
        (
            Command::ImportMeasurements {
                rows: vec![],
                clear_empty: false,
            },
            "import vide",
        ),
    ];
    for (cmd, what) in cases {
        let msg = refused(&mut f.p, cmd);
        assert!(!msg.is_empty(), "{what}");
    }
    // 1_000_000 exactement est accepté (borne incluse).
    set(&mut f.p, f.dos, f.s, 1_000_000.0);
}

#[test]
fn ca4_un_pas_d_annulation_et_aucun_pas_pour_un_no_op() {
    let f = fixture();
    let mut doc = Document::new(f.p.clone());
    let start = doc.project().clone();
    doc.execute(Command::SetMeasurementValue {
        measurement: f.poitrine,
        size: f.s,
        value: 881.0,
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.project(), start, "une seule annulation");
    assert!(!doc.can_undo());
    // Même valeur, effacer une valeur absente, même nom : aucune étape.
    for c in [
        Command::SetMeasurementValue {
            measurement: f.poitrine,
            size: f.s,
            value: 880.0,
        },
        Command::RenameMeasurement {
            measurement: f.poitrine,
            name: "Tour de poitrine".into(),
        },
    ] {
        doc.execute(c).unwrap();
        assert!(!doc.can_undo(), "no-op sans étape");
    }
    let mut p = f.p.clone();
    p.apply(Command::ClearMeasurementValue {
        measurement: f.poitrine,
        size: f.s,
    })
    .unwrap();
    let mut doc = Document::new(p);
    doc.execute(Command::ClearMeasurementValue {
        measurement: f.poitrine,
        size: f.s,
    })
    .unwrap();
    assert!(!doc.can_undo());
}

#[test]
fn ca5_retirer_une_taille_ou_une_gamme_retire_ses_valeurs() {
    let mut f = fixture();
    let out = f.p.apply(Command::RemoveSize { size: f.l }).unwrap();
    assert!(out
        .notes
        .iter()
        .any(|n| n.contains("3 valeur(s) du barème")));
    for m in [f.poitrine, f.dos, f.epaule] {
        assert_eq!(value(&f.p, m, f.l), None);
        assert!(value(&f.p, m, f.s).is_some() && value(&f.p, m, f.m).is_some());
    }
    assert_eq!(f.p.measurement(f.poitrine).unwrap().values.len(), 2);
    // Le projet reste valide et s'enregistre.
    persist::from_json(&bytes(&f.p)).unwrap();
    let mut doc = Document::new(f.p.clone());
    let before = doc.project().clone();
    doc.execute(Command::RemoveSizeRange { range: f.range })
        .unwrap();
    assert!(doc
        .project()
        .measurements
        .iter()
        .all(|m| m.values.is_empty()));
    assert_eq!(doc.project().measurements.len(), 3, "les mesures restent");
    assert!(doc.undo());
    assert_eq!(*doc.project(), before, "une seule annulation");
}

#[test]
fn ca6_retirer_une_mesure_retire_ses_references() {
    let mut f = fixture();
    let a = client(&mut f.p, "Alice", &[(f.poitrine, 930.0), (f.dos, 408.0)]);
    let b = client(&mut f.p, "Bob", &[(f.dos, 400.0)]);
    f.p.alterations.push(Alteration {
        id: AlterationId(f.p.next_id),
        name: "Dos +15".into(),
        target: f.dos,
        delta: 15.0,
    });
    f.p.next_id += 1;
    f.p.alterations.push(Alteration {
        id: AlterationId(f.p.next_id),
        name: "Poitrine +5".into(),
        target: f.poitrine,
        delta: 5.0,
    });
    f.p.next_id += 1;
    let out =
        f.p.apply(Command::RemoveMeasurement { measurement: f.dos })
            .unwrap();
    assert_eq!(
        out.notes,
        vec![
            "Mesure « Longueur dos » retirée avec ses références : 2 valeur(s) de fiche client, \
             1 retouche(s), 0 mesure(s) finie(s)."
                .to_string()
        ]
    );
    let ca = f.p.client(a).unwrap();
    assert_eq!(ca.values.len(), 1);
    assert_eq!(ca.values[0].measurement, f.poitrine);
    assert!(f.p.client(b).unwrap().values.is_empty());
    assert_eq!(f.p.alterations.len(), 1);
    assert_eq!(f.p.alterations[0].target, f.poitrine);
    persist::from_json(&bytes(&f.p)).unwrap();
}

#[test]
fn ca7_sauvegarde_relecture_a_l_identique() {
    let mut f = fixture();
    client(&mut f.p, "Alice", &[(f.poitrine, 930.0), (f.epaule, 20.25)]);
    let text = bytes(&f.p);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, f.p);
    assert_eq!(bytes(&back), text);
    assert!(text.contains("\"measurements\""));
    // Sans mesure ni fiche, aucune clé n'est écrite.
    let empty = Project::new("Vide");
    assert!(!bytes(&empty).contains("measurements"));
    assert!(!bytes(&empty).contains("clients"));
}

// ---------- US-10.1 : CSV ----------

/// Lecteur CSV indépendant : octets → (en-tête, lignes) ; ne partage rien avec le cœur.
fn read_csv_independent(bytes: &[u8], sep: char) -> Vec<Vec<String>> {
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
    let text = std::str::from_utf8(body).expect("UTF-8");
    let mut records = Vec::new();
    for line in text.split("\r\n").filter(|l| !l.is_empty()) {
        let mut rec = Vec::new();
        let mut cur = String::new();
        let mut in_q = false;
        let cs: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if in_q {
                if c == '"' && cs.get(i + 1) == Some(&'"') {
                    cur.push('"');
                    i += 1;
                } else if c == '"' {
                    in_q = false;
                } else {
                    cur.push(c);
                }
            } else if c == '"' {
                in_q = true;
            } else if c == sep {
                rec.push(std::mem::take(&mut cur));
            } else {
                cur.push(c);
            }
            i += 1;
        }
        rec.push(cur);
        records.push(rec);
    }
    records
}

#[test]
fn ca8_export_csv_octets_attendus() {
    let f = fixture();
    let out = export_measurement_csv(&f.p, f.range, &CsvFormat::default()).unwrap();
    let expected = "mesure;unite;S;M;L\r\n\
                    Tour de poitrine;mm;880;920;960\r\n\
                    Longueur dos;mm;400;410;420\r\n\
                    Angle d'épaule;deg;20;21,5;22\r\n";
    let mut want = vec![0xEF, 0xBB, 0xBF];
    want.extend_from_slice(expected.as_bytes());
    assert_eq!(out.bytes, want);
    assert_eq!((out.measurements, out.omitted_values), (3, 0));
    // Autres réglages : point décimal, virgule séparatrice, UTF-8 sans marque.
    let fmt = CsvFormat {
        separator: ',',
        decimal: '.',
        encoding: CsvEncoding::Utf8,
    };
    let out = export_measurement_csv(&f.p, f.range, &fmt).unwrap();
    assert_eq!(
        String::from_utf8(out.bytes).unwrap(),
        "mesure,unite,S,M,L\r\nTour de poitrine,mm,880,920,960\r\n\
         Longueur dos,mm,400,410,420\r\nAngle d'épaule,deg,20,21.5,22\r\n"
    );
    // Windows-1252 : « é » = 0xE9, apostrophe droite = 0x27.
    let fmt = CsvFormat {
        encoding: CsvEncoding::Windows1252,
        ..CsvFormat::default()
    };
    let out = export_measurement_csv(&f.p, f.range, &fmt).unwrap();
    assert!(!out.bytes.contains(&0xC3), "aucun octet UTF-8");
    assert!(out.bytes.windows(3).any(|w| w == [b'\'', 0xE9, b'p']));
}

#[test]
fn ca9_export_csv_cellules_vides_guillemets_et_valeurs_hors_gamme() {
    let mut f = fixture();
    f.p.apply(Command::ClearMeasurementValue {
        measurement: f.dos,
        size: f.m,
    })
    .unwrap();
    let odd = add_m(&mut f.p, "Haut \"x\"; y", MeasurementUnit::Mm);
    // Seconde gamme : ses valeurs ne sont pas dans le fichier de la première, et sont comptées.
    f.p.apply(Command::AddSizeRange {
        name: "Homme".into(),
        kind: SizeKind::Numeric,
        labels: vec!["46".into(), "48".into()],
        base: 0,
    })
    .unwrap();
    let h48 = f.p.sizes[1].sizes[1].id;
    set(&mut f.p, odd, h48, 3.0);
    let out = export_measurement_csv(&f.p, f.range, &CsvFormat::default()).unwrap();
    assert_eq!(out.omitted_values, 1);
    let rows = read_csv_independent(&out.bytes, ';');
    assert_eq!(rows[0], ["mesure", "unite", "S", "M", "L"]);
    assert_eq!(rows[2], ["Longueur dos", "mm", "400", "", "420"]);
    assert_eq!(rows[4], ["Haut \"x\"; y", "mm", "", "", ""]);
    let text = String::from_utf8(out.bytes[3..].to_vec()).unwrap();
    assert!(text.contains("\"Haut \"\"x\"\"; y\";mm;;;\r\n"));
    // Erreurs d'export : réglage incohérent, caractère hors Windows-1252.
    let same = CsvFormat {
        separator: ',',
        decimal: ',',
        encoding: CsvEncoding::Utf8,
    };
    assert!(export_measurement_csv(&f.p, f.range, &same).is_err());
    add_m(&mut f.p, "Ligne ≈ 1", MeasurementUnit::Mm);
    let w = CsvFormat {
        encoding: CsvEncoding::Windows1252,
        ..CsvFormat::default()
    };
    let e = export_measurement_csv(&f.p, f.range, &w).unwrap_err();
    assert!(e.contains('≈'), "{e}");
}

#[test]
fn ca10_aller_retour_csv_sans_perte() {
    let mut f = fixture();
    set(&mut f.p, f.poitrine, f.s, 880.125);
    for fmt in [
        CsvFormat::default(),
        CsvFormat {
            separator: '\t',
            decimal: '.',
            encoding: CsvEncoding::Utf8,
        },
        CsvFormat {
            separator: ',',
            decimal: '.',
            encoding: CsvEncoding::Windows1252,
        },
    ] {
        let out = export_measurement_csv(&f.p, f.range, &fmt).unwrap();
        let mut target = Project::new("Cible");
        target
            .apply(Command::AddSizeRange {
                name: "Femme".into(),
                kind: SizeKind::Alphanumeric,
                labels: vec!["S".into(), "M".into(), "L".into()],
                base: 1,
            })
            .unwrap();
        let read = read_measurement_csv(&target, target.sizes[0].id, &out.bytes, &fmt).unwrap();
        assert_eq!(read.rows.len(), 3);
        assert_eq!(read.blank_lines, 0);
        target
            .apply(Command::ImportMeasurements {
                rows: read.rows,
                clear_empty: false,
            })
            .unwrap();
        let ts = target.sizes[0]
            .sizes
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>();
        for (name, vals) in [
            ("Tour de poitrine", [880.125, 920.0, 960.0]),
            ("Longueur dos", [400.0, 410.0, 420.0]),
            ("Angle d'épaule", [20.0, 21.5, 22.0]),
        ] {
            let m = target.measurements.iter().find(|m| m.name == name).unwrap();
            for (sz, v) in ts.iter().zip(vals) {
                assert_eq!(value(&target, m.id, *sz), Some(v), "{name}");
            }
        }
        assert_eq!(target.measurements[2].unit, MeasurementUnit::Deg);
    }
}

fn target_with(labels: &[&str]) -> (Project, SizeRangeId) {
    let mut p = Project::new("Cible");
    p.apply(Command::AddSizeRange {
        name: "G".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels.iter().map(|s| s.to_string()).collect(),
        base: 0,
    })
    .unwrap();
    let r = p.sizes[0].id;
    (p, r)
}

fn read(p: &Project, r: SizeRangeId, text: &str, fmt: &CsvFormat) -> Vec<CsvLineError> {
    read_measurement_csv(p, r, text.as_bytes(), fmt).expect_err("doit échouer")
}

#[test]
fn ca11_lecture_csv_erreurs_explicites_avec_ligne_et_colonne() {
    let (p, r) = target_with(&["S", "M"]);
    let fmt = CsvFormat {
        separator: ';',
        decimal: ',',
        encoding: CsvEncoding::Utf8,
    };
    // Plusieurs erreurs rapportées ensemble, tout est rejeté.
    let text = "mesure;unite;S;M\n\
                A;mm;1,5;2\n\
                B;cm;3;4\n\
                C;mm;x;5\n\
                D;mm;-3;1\n\
                E;mm;1.5;2\n\
                F;mm;1\n\
                A;mm;1;1\n\
                \n\
                G;deg;1 000;2\n";
    let errs = read(&p, r, text, &fmt);
    let at: Vec<(usize, Option<usize>)> = errs.iter().map(|e| (e.line, e.column)).collect();
    assert_eq!(
        at,
        vec![
            (3, Some(2)),  // unité cm
            (4, Some(3)),  // x
            (5, Some(3)),  // -3
            (6, Some(3)),  // 1.5 avec décimale ','
            (7, None),     // 3 champs au lieu de 4
            (8, Some(1)),  // A répétée
            (10, Some(3)), // 1 000
        ]
    );
    assert!(errs[0].message.contains("cm"));
    assert!(errs[2].message.contains("négative"));
    assert!(errs[3].message.contains("séparateur décimal"));
    assert!(errs[4].message.contains("3 champ(s) au lieu de 4"));
    assert!(errs[5].message.contains("ligne 2"));
    assert_eq!(
        errs[0].to_string(),
        "ligne 3, colonne 2 : unité « cm » inconnue (mm ou deg)"
    );
}

#[test]
fn ca12_en_tete_et_encodage() {
    let (p, r) = target_with(&["S", "M"]);
    let fmt = CsvFormat {
        separator: ';',
        decimal: ',',
        encoding: CsvEncoding::Utf8,
    };
    // Taille inconnue, taille en double, en-tête incorrect.
    let e = read(&p, r, "mesure;unite;S;XL\nA;mm;1;2\n", &fmt);
    assert_eq!((e[0].line, e[0].column), (1, Some(4)));
    let e = read(&p, r, "mesure;unite;S;S\nA;mm;1;2\n", &fmt);
    assert!(e[0].message.contains("en double"));
    let e = read(&p, r, "nom;unite;S\nA;mm;1\n", &fmt);
    assert!(e[0].message.contains("en-tête attendu"));
    assert_eq!(read(&p, r, "", &fmt)[0].line, 0);
    let e = read(&p, r, "mesure;unite;S\n", &fmt);
    assert!(e[0].message.contains("aucune ligne"));
    // Réglage incohérent.
    let same = CsvFormat {
        separator: ',',
        decimal: ',',
        ..fmt
    };
    assert!(read(&p, r, "mesure,unite,S\nA,mm,1\n", &same)[0]
        .message
        .contains("identiques"));
    // Octets invalides : jamais de caractère de remplacement.
    let bad = read_measurement_csv(&p, r, b"mesure;unite;S\nCh\xE9;mm;1\n", &fmt).unwrap_err();
    assert!(bad[0].message.contains("position 17"), "{:?}", bad[0]);
    // Windows-1252 : « é » (0xE9) lu ; 0x81 (indéfini) refusé ; marque UTF-8 refusée.
    let w = CsvFormat {
        encoding: CsvEncoding::Windows1252,
        ..fmt
    };
    let ok = read_measurement_csv(&p, r, b"mesure;unite;S\nCh\xE9;mm;1\n", &w).unwrap();
    assert_eq!(ok.rows[0].name, "Ché");
    let e = read_measurement_csv(&p, r, b"mesure;unite;S\nA\x81;mm;1\n", &w).unwrap_err();
    assert!(e[0].message.contains("0x81"));
    let e = read_measurement_csv(&p, r, b"\xEF\xBB\xBFmesure;unite;S\nA;mm;1\n", &w).unwrap_err();
    assert!(e[0].message.contains("UTF-8"));
    // UTF-8 avec marque lu sans marque, CRLF accepté, lignes vides comptées.
    let u = read_measurement_csv(
        &p,
        r,
        "\u{FEFF}mesure;unite;S;M\r\nÉpaule;deg;1,25;\r\n\r\n".as_bytes(),
        &fmt,
    )
    .unwrap();
    assert_eq!(u.blank_lines, 1);
    assert_eq!(u.rows[0].name, "Épaule");
    assert_eq!(u.rows[0].unit, MeasurementUnit::Deg);
    assert_eq!(u.rows[0].cells[0].value, Some(1.25));
    assert_eq!(u.rows[0].cells[1].value, None);
}

#[test]
fn ca13_guillemets_mal_formes_et_unite_differente() {
    let (mut p, r) = target_with(&["S"]);
    let fmt = CsvFormat {
        separator: ';',
        decimal: '.',
        encoding: CsvEncoding::Utf8,
    };
    let e = read(&p, r, "mesure;unite;S\n\"A;mm;1\n", &fmt);
    assert!(e[0].message.contains("jamais refermé"));
    let e = read(&p, r, "mesure;unite;S\nA\"b;mm;1\n", &fmt);
    assert!(e[0].message.contains("guillemet au milieu"));
    let e = read(&p, r, "mesure;unite;S\n\"A\"x;mm;1\n", &fmt);
    assert!(e[0].message.contains("après le guillemet"));
    let ok = read_measurement_csv(
        &p,
        r,
        "mesure;unite;S\n\"A;\"\"b\"\"\";mm;1.5\n".as_bytes(),
        &fmt,
    )
    .unwrap();
    assert_eq!(ok.rows[0].name, "A;\"b\"");
    // Mesure existante avec une autre unité : erreur de ligne, projet intact.
    add_m(&mut p, "Épaule", MeasurementUnit::Deg);
    let e = read(&p, r, "mesure;unite;S\népaule;mm;1\n", &fmt);
    assert_eq!((e[0].line, e[0].column), (2, Some(2)));
    assert!(e[0].message.contains("autre unité"));
}

#[test]
fn ca14_import_fusion_cellule_vide_et_une_etape() {
    let mut f = fixture();
    let fmt = CsvFormat {
        separator: ';',
        decimal: ',',
        encoding: CsvEncoding::Utf8,
    };
    let text = "mesure;unite;S;M;L\n\
                tour de poitrine;mm;900;;\n\
                Hanche;mm;;1000,5;1020\n";
    let rows = read_measurement_csv(&f.p, f.range, text.as_bytes(), &fmt)
        .unwrap()
        .rows;
    // Cellule vide : valeur existante conservée.
    let mut keep = f.p.clone();
    let out = keep
        .apply(Command::ImportMeasurements {
            rows: rows.clone(),
            clear_empty: false,
        })
        .unwrap();
    assert_eq!(
        out.measurements.len(),
        1,
        "Hanche créée, poitrine mise à jour"
    );
    assert_eq!(
        out.notes,
        vec!["Import du barème : 1 mesure(s) créée(s), 1 mise(s) à jour.".to_string()]
    );
    assert_eq!(value(&keep, f.poitrine, f.s), Some(900.0));
    assert_eq!(value(&keep, f.poitrine, f.m), Some(920.0));
    assert_eq!(value(&keep, f.poitrine, f.l), Some(960.0));
    assert_eq!(
        keep.measurements[0].name, "Tour de poitrine",
        "nom d'origine"
    );
    let h = out.measurements[0];
    assert_eq!(value(&keep, h, f.s), None);
    assert_eq!(value(&keep, h, f.m), Some(1000.5));
    // `clear_empty` : la cellule vide efface.
    let mut clear = f.p.clone();
    clear
        .apply(Command::ImportMeasurements {
            rows,
            clear_empty: true,
        })
        .unwrap();
    assert_eq!(value(&clear, f.poitrine, f.s), Some(900.0));
    assert_eq!(value(&clear, f.poitrine, f.m), None);
    assert_eq!(value(&clear, f.poitrine, f.l), None);
    // Une seule étape annulable.
    let mut doc = Document::new(f.p.clone());
    let rows = read_measurement_csv(&f.p, f.range, text.as_bytes(), &fmt)
        .unwrap()
        .rows;
    doc.execute(Command::ImportMeasurements {
        rows,
        clear_empty: false,
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.project(), f.p);
    assert!(!doc.can_undo());
    // Refus atomique : une ligne d'unité contraire n'écrit rien du tout.
    let bad = vec![
        MeasurementImportRow {
            name: "Nouvelle".into(),
            unit: MeasurementUnit::Mm,
            cells: vec![],
        },
        MeasurementImportRow {
            name: "angle d'épaule".into(),
            unit: MeasurementUnit::Mm,
            cells: vec![],
        },
    ];
    let msg = refused(
        &mut f.p,
        Command::ImportMeasurements {
            rows: bad,
            clear_empty: false,
        },
    );
    assert!(msg.contains("existe en deg"), "{msg}");
}

#[test]
fn ca15_limite_de_mesures_dans_l_import() {
    let (mut p, r) = target_with(&["S"]);
    for i in 0..MAX_MEASUREMENTS {
        add_m(&mut p, &format!("m{i}"), MeasurementUnit::Mm);
    }
    let fmt = CsvFormat {
        separator: ';',
        decimal: '.',
        encoding: CsvEncoding::Utf8,
    };
    let e = read(&p, r, "mesure;unite;S\nnouvelle;mm;1\n", &fmt);
    assert!(e[0].message.contains("500"));
    let msg = refused(
        &mut p,
        Command::AddMeasurement {
            name: "de trop".into(),
            unit: MeasurementUnit::Mm,
        },
    );
    assert!(msg.contains("500"), "{msg}");
}

// ---------- US-10.2 : fiche client ----------

#[test]
fn ca20_fiche_client_commandes() {
    let mut f = fixture();
    let c = client(&mut f.p, "Alice", &[(f.poitrine, 930.0)]);
    f.p.apply(Command::RenameClient {
        client: c,
        name: "Alice M.".into(),
    })
    .unwrap();
    assert_eq!(f.p.client(c).unwrap().name, "Alice M.");
    f.p.apply(Command::SetClientValue {
        client: c,
        measurement: f.poitrine,
        value: 931.0,
    })
    .unwrap();
    f.p.apply(Command::SetClientValue {
        client: c,
        measurement: f.dos,
        value: 408.0,
    })
    .unwrap();
    assert_eq!(f.p.client(c).unwrap().values.len(), 2);
    assert_eq!(f.p.client(c).unwrap().values[0].value, 931.0);
    f.p.apply(Command::ClearClientValue {
        client: c,
        measurement: f.poitrine,
    })
    .unwrap();
    assert_eq!(f.p.client(c).unwrap().values.len(), 1);
    f.p.apply(Command::RemoveClient { client: c }).unwrap();
    assert!(f.p.clients.is_empty());
    persist::from_json(&bytes(&f.p)).unwrap();
    // Annulation en une étape, no-op sans étape.
    let mut doc = Document::new(f.p.clone());
    doc.execute(Command::AddClient {
        name: "Zoé".into()
    })
    .unwrap();
    assert!(doc.undo());
    assert!(!doc.can_undo());
    let id = {
        let mut p = f.p.clone();
        let c = client(&mut p, "Zoé", &[]);
        let mut doc = Document::new(p);
        doc.execute(Command::ClearClientValue {
            client: c,
            measurement: f.dos,
        })
        .unwrap();
        doc.execute(Command::RenameClient {
            client: c,
            name: "Zoé".into(),
        })
        .unwrap();
        assert!(!doc.can_undo(), "no-ops sans étape");
        c
    };
    assert_ne!(id, c);
}

#[test]
fn ca21_mesures_manquantes_jamais_devinees() {
    let mut f = fixture();
    let b = client(&mut f.p, "Bob", &[(f.poitrine, 900.0)]);
    let before = f.p.clone();
    let missing = f.p.client_missing_measurements(b).unwrap();
    assert_eq!(missing, vec![f.dos, f.epaule], "ordre du barème");
    assert_eq!(f.p, before, "lecture pure, rien n'est rempli");
    assert_eq!(f.p.client(b).unwrap().values.len(), 1);
    let c = client(&mut f.p, "Vide", &[]);
    assert_eq!(
        f.p.client_missing_measurements(c).unwrap(),
        vec![f.poitrine, f.dos, f.epaule]
    );
    assert!(f.p.client_missing_measurements(ClientId(9999)).is_err());
    // Une nouvelle mesure du barème apparaît manquante pour toutes les fiches.
    let tour = add_m(&mut f.p, "Tour de cou", MeasurementUnit::Mm);
    assert_eq!(
        f.p.client_missing_measurements(b).unwrap().last(),
        Some(&tour)
    );
}

#[test]
fn ca22_taille_la_plus_proche_et_ecarts() {
    let mut f = fixture();
    let a = client(
        &mut f.p,
        "Alice",
        &[(f.poitrine, 930.0), (f.dos, 408.0), (f.epaule, 20.0)],
    );
    let before = f.p.clone();
    let prop = f.p.client_size_proposal(a, f.range).unwrap();
    assert_eq!(
        f.p, before,
        "la proposition n'est jamais appliquée ni stockée"
    );
    assert!(prop.missing.is_empty());
    assert_eq!(prop.nearest, Some(f.m));
    assert!(prop.tied.is_empty());
    let scores: Vec<Option<f64>> = prop.candidates.iter().map(|c| c.score_mm).collect();
    assert_eq!(scores, vec![Some(58.0), Some(12.0), Some(42.0)]);
    let gaps = |sz: SizeId| -> Vec<f64> {
        prop.candidates
            .iter()
            .find(|c| c.size == sz)
            .unwrap()
            .gaps
            .iter()
            .map(|g| g.gap)
            .collect()
    };
    assert_eq!(gaps(f.s), vec![50.0, 8.0, 0.0]);
    assert_eq!(gaps(f.m), vec![10.0, -2.0, -1.5]);
    assert_eq!(gaps(f.l), vec![-30.0, -12.0, -2.0]);
    let g = &prop.candidates[1].gaps[0];
    assert_eq!(
        (g.measurement, g.client_value, g.size_value),
        (f.poitrine, 930.0, 920.0)
    );
    // Le degré n'entre pas dans le score : sans la poitrine ni le dos, plus rien à comparer.
    let d = client(&mut f.p, "Deg", &[(f.epaule, 20.0)]);
    let prop = f.p.client_size_proposal(d, f.range).unwrap();
    assert_eq!(prop.nearest, None);
    assert!(prop.candidates.iter().all(|c| c.score_mm.is_none()));
    assert_eq!(prop.candidates[0].gaps.len(), 1);
}

#[test]
fn ca23_egalite_taille_non_comparable_et_gamme_sans_valeur() {
    let mut f = fixture();
    // Bob : poitrine 900 → S 20, M 20, L 60 : égalité S / M, la première de la gamme gagne.
    let b = client(&mut f.p, "Bob", &[(f.poitrine, 900.0)]);
    let prop = f.p.client_size_proposal(b, f.range).unwrap();
    assert_eq!(prop.nearest, Some(f.s));
    assert_eq!(prop.tied, vec![f.m]);
    assert_eq!(prop.missing, vec![f.dos, f.epaule]);
    // Alice sans valeur de dos en L : L n'est pas comparable (jamais complétée ni interpolée).
    let a = client(&mut f.p, "Alice", &[(f.poitrine, 960.0), (f.dos, 420.0)]);
    f.p.apply(Command::ClearMeasurementValue {
        measurement: f.dos,
        size: f.l,
    })
    .unwrap();
    let prop = f.p.client_size_proposal(a, f.range).unwrap();
    let l = &prop.candidates[2];
    assert_eq!(l.score_mm, None);
    assert_eq!(l.missing_in_size, vec![f.dos]);
    assert_eq!(l.gaps.len(), 1, "l'écart de poitrine reste affiché");
    // Poitrine 960 / dos 420 : S 80 + 20 = 100, M 40 + 10 = 50 → M.
    assert_eq!(prop.candidates[0].score_mm, Some(100.0));
    assert_eq!(prop.candidates[1].score_mm, Some(50.0));
    assert_eq!(prop.nearest, Some(f.m));
    // Fiche vide : aucune proposition, tout manque.
    let e = client(&mut f.p, "Vide", &[]);
    let prop = f.p.client_size_proposal(e, f.range).unwrap();
    assert_eq!(prop.nearest, None);
    assert_eq!(prop.missing.len(), 3);
    // Gamme ou fiche inconnue : erreur.
    assert!(f.p.client_size_proposal(e, SizeRangeId(9999)).is_err());
    assert!(f.p.client_size_proposal(ClientId(9999), f.range).is_err());
}

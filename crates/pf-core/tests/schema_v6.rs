//! Décisions 455 à 459 — schéma 6 (montée groupée) : migration v5 → v6 sur chaque fixture v5
//! (`tests/fixtures_v5`, octets exacts), clés réservées, client d'origine d'une taille, identifiant
//! d'entrée de variante, options renommables / déplaçables, image de fond (E14) et peau de cuir
//! (E15) en stockage seulement, schéma JSON et `pf-validate`.
//!
//! **Valeurs et JSON écrits à la main**, jamais relus du code testé. Un test par critère.

#[path = "format_ouvert/validateur.rs"]
mod validateur;

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command as Proc;

const SCHEMA_TEXT: &str = include_str!("../../../docs/pforge-v6.schema.json");

fn schema() -> Value {
    serde_json::from_str(SCHEMA_TEXT).expect("le schéma est du JSON")
}

/// Dossier temporaire propre au fil d'exécution, vidé avant usage.
fn tmp(name: &str) -> PathBuf {
    let thread = format!("{:?}", std::thread::current().id()).replace(['(', ')'], "");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("schema_v6-{name}-{thread}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures_v5")
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_dir().join(name)).expect(name)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn load(text: &str) -> Result<Project, CoreError> {
    persist::from_json(text)
}

fn refused(text: &str, needle: &str) {
    let err = load(text).expect_err(&format!("doit être refusé ({needle})"));
    let msg = err.to_string();
    assert!(msg.contains(needle), "message « {msg} » sans « {needle} »");
}

struct Run {
    code: i32,
    stdout: String,
}

fn validate_cli(text: &str, name: &str) -> Run {
    let path = tmp("cli").join(name);
    std::fs::write(&path, text).unwrap();
    let out = Proc::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&path)
        .output()
        .expect("pf-validate");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

// ---------------------------------------------------------------------------
// CA1 — migration v5 → v6 : octets attendus pour chaque fixture v5
// ---------------------------------------------------------------------------

const FIXTURES_WITHOUT_VARIANTS: [&str; 8] = [
    "v5-geometrie-exercice.pforge.json",
    "v5-grande-piece-600x450.pforge.json",
    "v5-jupe-droite.pforge.json",
    "v5-piece-crans-droit-fil.pforge.json",
    "v5-porte-cartes.pforge.json",
    "v5-sac-cabas.pforge.json",
    "v5-jupe-droite-tailles.pforge.json",
    "v5-porte-cartes-piqure.pforge.json",
];

#[test]
fn ca1_chaque_fixture_v5_sans_variante_migre_avec_la_seule_ligne_de_version() {
    for name in FIXTURES_WITHOUT_VARIANTS {
        let v5 = fixture(name);
        assert_eq!(v5.matches("\"schema_version\": 5,").count(), 1, "{name}");
        assert!(!v5.contains("\"variants\""), "{name}");
        let p = load(&v5).unwrap_or_else(|e| panic!("{name} : {e}"));
        assert_eq!(p.schema_version, 6, "{name}");
        let expected = v5.replacen("\"schema_version\": 5,", "\"schema_version\": 6,", 1);
        assert_eq!(bytes(&p), expected, "{name} : octets attendus");
        // Idempotence : relire la sortie donne les mêmes octets.
        assert_eq!(bytes(&load(&expected).unwrap()), expected, "{name}");
    }
}

#[test]
fn ca1_fixture_avec_variantes_les_entrees_recoivent_les_identifiants_29_30_31() {
    // Fichier v5 écrit à la main : variantes 26, 27, 28 ; next_id 29 ; entrées sans id.
    let v5 = fixture("v5-variantes-tailles-fiche.pforge.json");
    assert!(v5.contains("\"next_id\": 29,") && !v5.contains("\"id\": 29"));
    let p = load(&v5).unwrap();
    assert_eq!(p.next_id, 32, "trois entrées : 29, 30, 31");
    let ids = |v: usize| -> Vec<u64> { p.variants[v].entries.iter().map(|e| e.id.0).collect() };
    assert_eq!(ids(0), vec![29, 30], "Robe : ordre du fichier");
    assert_eq!(ids(1), Vec::<u64>::new(), "Vide : aucune entrée");
    assert_eq!(ids(2), vec![31], "Haut");
    // Octets attendus : la version, le compteur et trois lignes `id`, rien d'autre.
    let mut expected = v5
        .replacen("\"schema_version\": 5,", "\"schema_version\": 6,", 1)
        .replacen("\"next_id\": 29,", "\"next_id\": 32,", 1);
    for (anchor, id) in [
        (
            "        {\n          \"piece\": 10,\n          \"quantity\": 1\n        },",
            29,
        ),
        ("        {\n          \"piece\": 19,\n", 30),
        (
            "        {\n          \"piece\": 10,\n          \"quantity\": 1\n        }\n",
            31,
        ),
    ] {
        assert_eq!(expected.matches(anchor).count(), 1, "ancre {id}");
        let with_id = anchor.replacen("{\n", &format!("{{\n          \"id\": {id},\n"), 1);
        expected = expected.replacen(anchor, &with_id, 1);
    }
    assert_eq!(bytes(&p), expected);
    assert_eq!(
        bytes(&load(&expected).unwrap()),
        expected,
        "aller-retour octet"
    );
    // Les tailles existantes n'ont aucun client ; la clé n'est pas écrite.
    assert!(p.sizes[0].sizes.iter().all(|s| s.client.is_none()));
    assert!(!bytes(&p).contains("\"client\""));
}

#[test]
fn ca1_fixture_de_la_recette_trois_entrees_34_35_36_et_compteur_37() {
    let v5 = fixture("v5-ensemble-variantes-plan.pforge.json");
    assert!(v5.contains("\"next_id\": 34,"));
    let p = load(&v5).unwrap();
    let ids: Vec<u64> = p.variants[0].entries.iter().map(|e| e.id.0).collect();
    assert_eq!(ids, vec![34, 35, 36]);
    assert_eq!(p.next_id, 37);
    // Valeurs relues à l'identique : pièce, quantité, option, matière.
    let e = &p.variants[0].entries;
    assert_eq!((e[1].piece, e[1].quantity), (PieceId(19), 2));
    assert_eq!(e[1].option.as_deref(), Some("avec poche"));
    assert_eq!(e[2].material.as_deref(), Some("Lin"));
    let out = bytes(&p);
    assert_eq!(bytes(&load(&out).unwrap()), out);
}

#[test]
fn ca1_la_migration_est_deterministe_et_ne_depend_pas_de_la_lecture() {
    let v5 = fixture("v5-variantes-tailles-fiche.pforge.json");
    let a = bytes(&load(&v5).unwrap());
    let b = bytes(&load(&v5).unwrap());
    assert_eq!(a, b);
    // Même résultat par pf-validate --json (fichier intact, migré en mémoire).
    let run = validate_cli(&v5, "v5.pforge.json");
    assert_eq!(run.code, 0, "{}", run.stdout);
}

// ---------------------------------------------------------------------------
// CA2 — clés réservées à la v6 refusées dans un fichier antérieur
// ---------------------------------------------------------------------------

/// Texte v6 minimal : une feuille, un plan, une peau, une image de fond, une taille citant une fiche
/// et une variante (une entrée). Écrit à la main (ordre des clés du code).
fn rich_v6() -> Value {
    json!({
        "format": "patternforge",
        "schema_version": 6,
        "name": "Riche",
        "next_id": 30,
        "sheets": [{
            "id": 1, "name": "Feuille 1", "points": [], "entities": [],
            "background": background_two_points()
        }],
        "pieces": [{"id": 2, "name": "Dos", "quantity": 1, "mirrored_pair": false, "outline": [], "link": {"type": "linked", "sheet": 1}}],
        "sizes": [{"id": 3, "name": "G", "kind": "alphanumeric", "base": 4,
            "sizes": [{"id": 4, "label": "M"}, {"id": 5, "label": "Alice", "client": 6}]}],
        "clients": [{"id": 6, "name": "Alice"}],
        "cutting_plans": [{
            "id": 7, "name": "Cuir", "fabric_width_mm": 1500.0, "fold": "flat",
            "selvedge_mm": 0.0, "min_gap_mm": 0.0,
            "hide": {
                "contour": rect(0.0, 0.0, 2000.0, 1500.0),
                "defects": [{"id": 8, "kind": "hole", "polygon": rect(100.0, 100.0, 150.0, 140.0)}],
                "quality_zones": [{"id": 9, "grade": 2, "polygon": rect(0.0, 0.0, 500.0, 300.0)}]
            }
        }],
        "variants": [{"id": 10, "name": "Robe", "entries": [{"id": 11, "piece": 2, "quantity": 1}]}]
    })
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([{"x": x0, "y": y0}, {"x": x1, "y": y0}, {"x": x1, "y": y1}, {"x": x0, "y": y1}])
}

fn background_two_points() -> Value {
    json!({
        "image": "fonds/scan.png",
        "sha256": "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        "width_px": 2480, "height_px": 3508,
        "calibration": {"kind": "two_points",
            "points_px": [{"x": 100.0, "y": 200.0}, {"x": 900.0, "y": 200.0}],
            "known_distance_mm": 200.0, "residual_mm": 0.5},
        "opacity": 0.5, "visible": true, "locked": false
    })
}

#[test]
fn ca2_le_fichier_v6_riche_est_accepte_et_relu_a_l_octet() {
    let doc = rich_v6();
    let p = load(&doc.to_string()).expect("fichier v6 riche");
    assert_eq!(p.sizes[0].sizes[1].client, Some(ClientId(6)));
    assert_eq!(p.variants[0].entries[0].id, VariantEntryId(11));
    let out = bytes(&p);
    assert_eq!(bytes(&load(&out).unwrap()), out);
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), doc);
}

#[test]
fn ca2_chaque_cle_v6_est_refusee_dans_un_fichier_declare_v5_ou_plus_ancien() {
    let cases: [(&str, &str); 4] = [
        ("/sheets/0/background", "background"),
        ("/cutting_plans/0/hide", "hide"),
        ("/sizes/0/sizes/1/client", "client"),
        ("/variants/0/entries/0/id", "id (entrée de variante)"),
    ];
    for (pointer, key) in cases {
        // Un fichier qui ne porte QUE cette clé v6 (les autres retirées) et se déclare v5.
        let mut doc = rich_v6();
        for (other, _) in cases.iter().filter(|(p, _)| *p != pointer) {
            remove_pointer(&mut doc, other);
        }
        // Rend le reste cohérent sans clé v6 : plan sans peau etc. sont retirés ci-dessus.
        for version in [5, 4, 3] {
            let mut d = doc.clone();
            d["schema_version"] = json!(version);
            if version < 5 {
                // Les clés v5 du document (plans, variantes, clients) sont refusées en premier :
                // on ne garde que ce qui existe avant la v5 pour isoler la clé v6.
                for k in ["cutting_plans", "variants", "clients"] {
                    d.as_object_mut().unwrap().remove(k);
                }
                if pointer != "/sheets/0/background" && pointer != "/sizes/0/sizes/1/client" {
                    continue;
                }
            }
            if version < 4 {
                d.as_object_mut().unwrap().remove("sizes");
                if pointer == "/sizes/0/sizes/1/client" {
                    continue;
                }
            }
            let text = d.to_string();
            let err = load(&text).expect_err(&format!("{key} dans un fichier v{version}"));
            let msg = err.to_string();
            assert!(
                msg.contains("réservée") || msg.contains("unknown field"),
                "{key} v{version} : {msg}"
            );
            if version == 5 {
                assert!(
                    msg.contains(&format!(
                        "la clé « {key} » est réservée au format version 6"
                    )),
                    "{msg}"
                );
            }
        }
    }
}

fn remove_pointer(doc: &mut Value, pointer: &str) {
    let (parent, last) = pointer.rsplit_once('/').unwrap();
    let parent = doc.pointer_mut(parent).unwrap();
    match parent {
        Value::Object(o) => {
            o.remove(last);
        }
        _ => panic!("parent non objet : {pointer}"),
    }
}

#[test]
fn ca2_reserved_v6_key_lit_les_quatre_chemins_et_rien_d_autre() {
    let doc = rich_v6();
    let obj = doc.as_object().unwrap();
    assert_eq!(persist::reserved_v6_key(obj).as_deref(), Some("background"));
    let mut d = doc.clone();
    remove_pointer(&mut d, "/sheets/0/background");
    assert_eq!(
        persist::reserved_v6_key(d.as_object().unwrap()).as_deref(),
        Some("hide")
    );
    remove_pointer(&mut d, "/cutting_plans/0/hide");
    assert_eq!(
        persist::reserved_v6_key(d.as_object().unwrap()).as_deref(),
        Some("client")
    );
    remove_pointer(&mut d, "/sizes/0/sizes/1/client");
    assert_eq!(
        persist::reserved_v6_key(d.as_object().unwrap()).as_deref(),
        Some("id (entrée de variante)")
    );
    remove_pointer(&mut d, "/variants/0/entries/0/id");
    assert_eq!(persist::reserved_v6_key(d.as_object().unwrap()), None);
}

// ---------------------------------------------------------------------------
// CA3 — client d'origine d'une taille
// ---------------------------------------------------------------------------

struct Sc {
    p: Project,
    alice: ClientId,
    bob: ClientId,
    range: SizeRangeId,
}

fn sc() -> Sc {
    let mut p = Project::new("Clients");
    let range = p
        .apply(Command::AddSizeRange {
            name: "G".into(),
            kind: SizeKind::Alphanumeric,
            labels: vec!["M".into(), "L".into()],
            base: 0,
        })
        .unwrap()
        .size_ranges[0];
    let m = p
        .apply(Command::AddMeasurement {
            name: "Poitrine".into(),
            unit: MeasurementUnit::Mm,
        })
        .unwrap()
        .measurements[0];
    let mut client = |name: &str, v: f64| {
        let c = p
            .apply(Command::AddClient { name: name.into() })
            .unwrap()
            .clients[0];
        p.apply(Command::SetClientValue {
            client: c,
            measurement: m,
            value: v,
        })
        .unwrap();
        c
    };
    let alice = client("Alice", 930.0);
    let bob = client("Bob", 1010.0);
    Sc {
        p,
        alice,
        bob,
        range,
    }
}

fn generate(f: &mut Sc, who: &str, label: &str, replace: bool) -> Result<Outcome, CoreError> {
    let client = if who == "alice" { f.alice } else { f.bob };
    f.p.apply(Command::GenerateClientSize {
        client,
        range: f.range,
        label: label.into(),
        replace,
    })
}

#[test]
fn ca3_generate_client_size_memorise_la_fiche_et_l_ecrit() {
    let mut f = sc();
    let size = generate(&mut f, "alice", "T40", false).unwrap().sizes[0];
    let s = f.p.sizes[0].sizes.iter().find(|s| s.id == size).unwrap();
    assert_eq!(s.client, Some(f.alice));
    // Les tailles d'origine (M, L) n'ont pas de client.
    assert!(f.p.sizes[0].sizes[..2].iter().all(|s| s.client.is_none()));
    let text = bytes(&f.p);
    assert_eq!(text.matches("\"client\":").count(), 1);
    let line = format!("\"client\": {}", f.alice.0);
    assert!(text.contains(&line), "{line}");
    // Aller-retour octet.
    assert_eq!(bytes(&load(&text).unwrap()), text);
}

#[test]
fn ca3_remplacer_une_taille_change_la_fiche_d_origine_et_une_seule_etape() {
    let mut f = sc();
    let size = generate(&mut f, "alice", "T40", false).unwrap().sizes[0];
    let mut doc = Document::new(f.p.clone());
    let before = bytes(doc.project());
    doc.execute(Command::GenerateClientSize {
        client: f.bob,
        range: f.range,
        label: "t40".into(),
        replace: true,
    })
    .unwrap();
    let s = doc.project().sizes[0]
        .sizes
        .iter()
        .find(|s| s.id == size)
        .unwrap();
    assert_eq!(
        s.client,
        Some(f.bob),
        "la dernière fiche est celle d'origine"
    );
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before, "annulation exacte à l'octet");
}

#[test]
fn ca3_supprimer_une_fiche_citee_est_refuse_en_nommant_la_taille_sans_trace() {
    let mut f = sc();
    generate(&mut f, "alice", "T40", false).unwrap();
    generate(&mut f, "alice", "T42", false).unwrap();
    let before = bytes(&f.p);
    let err = f.p.apply(Command::RemoveClient { client: f.alice });
    let msg = err.unwrap_err().to_string();
    assert_eq!(
        msg,
        "la fiche « Alice » est citée par la taille « T40 » de la gamme « G » : supprimez d'abord cette taille",
        "la première taille citante est nommée"
    );
    assert_eq!(bytes(&f.p), before, "refus sans trace");
    // Une fiche non citée se supprime.
    f.p.apply(Command::RemoveClient { client: f.bob }).unwrap();
    assert_eq!(f.p.clients.len(), 1);
}

#[test]
fn ca3_retirer_la_taille_ou_la_gamme_retire_la_reference_puis_la_fiche_se_supprime() {
    let mut f = sc();
    let t40 = generate(&mut f, "alice", "T40", false).unwrap().sizes[0];
    generate(&mut f, "alice", "T42", false).unwrap();
    f.p.apply(Command::RemoveSize { size: t40 }).unwrap();
    assert!(f
        .p
        .apply(Command::RemoveClient { client: f.alice })
        .is_err());
    f.p.apply(Command::RemoveSizeRange { range: f.range })
        .unwrap();
    f.p.apply(Command::RemoveClient { client: f.alice })
        .unwrap();
    assert_eq!(f.p.clients.len(), 1);
    assert!(f.p.sizes.is_empty());
    assert!(!bytes(&f.p).contains("\"client\":"));
}

#[test]
fn ca3_renommer_la_fiche_ou_la_taille_ne_touche_pas_la_reference() {
    let mut f = sc();
    let t40 = generate(&mut f, "alice", "T40", false).unwrap().sizes[0];
    f.p.apply(Command::RenameClient {
        client: f.alice,
        name: "Alice B.".into(),
    })
    .unwrap();
    f.p.apply(Command::RenameSize {
        size: t40,
        label: "T41".into(),
    })
    .unwrap();
    let s = f.p.sizes[0].sizes.iter().find(|s| s.id == t40).unwrap();
    assert_eq!((s.label.as_str(), s.client), ("T41", Some(f.alice)));
    // Le message de refus suit les nouveaux noms.
    let msg =
        f.p.apply(Command::RemoveClient { client: f.alice })
            .unwrap_err()
            .to_string();
    assert!(
        msg.contains("« Alice B. »") && msg.contains("« T41 »"),
        "{msg}"
    );
}

#[test]
fn ca3_fichier_dont_la_taille_cite_une_fiche_absente_est_refuse() {
    let mut doc = rich_v6();
    doc["sizes"][0]["sizes"][1]["client"] = json!(99);
    refused(
        &doc.to_string(),
        "cite la fiche client 99 qui n'existe plus",
    );
    // Un id de fiche qui est en fait celui d'une taille : toujours une référence morte.
    let mut doc = rich_v6();
    doc["sizes"][0]["sizes"][1]["client"] = json!(4);
    refused(&doc.to_string(), "qui n'existe plus");
    // Clé en trop dans une taille : refusée.
    let mut doc = rich_v6();
    doc["sizes"][0]["sizes"][1]["zz"] = json!(1);
    refused(&doc.to_string(), "unknown field");
}

// ---------------------------------------------------------------------------
// CA4 — identifiant d'entrée de variante, RenameVariantOption, MoveVariantOption
// ---------------------------------------------------------------------------

struct Vs {
    p: Project,
    v: VariantId,
    dos: PieceId,
    poche: PieceId,
    col: PieceId,
}

fn piece(p: &mut Project, x: f64, name: &str) -> PieceId {
    let s = p.sheets[0].id;
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(x, 0.0)),
            width_mm: 100.0,
            height_mm: 40.0,
        })
        .unwrap();
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Paths {
            entities: r.entities,
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

fn vs() -> Vs {
    let mut p = Project::new("Options");
    let dos = piece(&mut p, 0.0, "Dos");
    let poche = piece(&mut p, 200.0, "Poche");
    let col = piece(&mut p, 400.0, "Col");
    let v = p
        .apply(Command::AddVariant {
            name: "Robe".into(),
        })
        .unwrap()
        .variants[0];
    for o in ["poche", "col", "manche"] {
        p.apply(Command::AddVariantOption {
            variant: v,
            option: o.into(),
        })
        .unwrap();
    }
    p.apply(Command::SetVariantOptionActive {
        variant: v,
        option: "poche".into(),
        active: true,
    })
    .unwrap();
    let mut add = |piece: PieceId, option: Option<&str>| {
        p.apply(Command::AddVariantEntry {
            variant: v,
            piece,
            quantity: 1,
            material: None,
            option: option.map(str::to_string),
        })
        .unwrap();
    };
    add(dos, None);
    add(poche, Some("poche"));
    add(col, Some("col"));
    Vs {
        p,
        v,
        dos,
        poche,
        col,
    }
}

fn options(p: &Project, v: VariantId) -> Vec<String> {
    p.variant(v).unwrap().options.clone()
}

fn entry_options(p: &Project, v: VariantId) -> Vec<Option<String>> {
    p.variant(v)
        .unwrap()
        .entries
        .iter()
        .map(|e| e.option.clone())
        .collect()
}

#[test]
fn ca4_les_commandes_creent_les_identifiants_d_entree_sans_reutilisation() {
    let mut f = vs();
    let ids = |p: &Project| -> Vec<u64> {
        p.variant(f.v)
            .unwrap()
            .entries
            .iter()
            .map(|e| e.id.0)
            .collect()
    };
    let first = ids(&f.p);
    assert_eq!(first.len(), 3);
    assert!(first.windows(2).all(|w| w[1] == w[0] + 1), "{first:?}");
    // Retirer l'entrée du milieu puis en ajouter une : l'identifiant n'est pas réutilisé.
    f.p.apply(Command::RemoveVariantEntry {
        variant: f.v,
        piece: f.poche,
        option: Some("poche".into()),
    })
    .unwrap();
    f.p.apply(Command::AddVariantEntry {
        variant: f.v,
        piece: f.poche,
        quantity: 2,
        material: None,
        option: Some("poche".into()),
    })
    .unwrap();
    let after = ids(&f.p);
    assert_eq!(after[..2], [first[0], first[2]]);
    assert_eq!(after[2], first[2] + 1, "jamais réutilisé");
    // Modifier une entrée garde son identifiant.
    f.p.apply(Command::UpdateVariantEntry {
        variant: f.v,
        piece: f.dos,
        option: None,
        quantity: 5,
        material: Some("Lin".into()),
        new_option: Some("manche".into()),
    })
    .unwrap();
    assert_eq!(ids(&f.p), after);
    // Dupliquer : des identifiants neufs et distincts.
    let copy =
        f.p.apply(Command::DuplicateVariant {
            variant: f.v,
            name: "Copie".into(),
        })
        .unwrap()
        .variants[0];
    let dup: Vec<u64> =
        f.p.variant(copy)
            .unwrap()
            .entries
            .iter()
            .map(|e| e.id.0)
            .collect();
    assert_eq!(dup.len(), 3);
    assert!(
        dup.iter().all(|d| !after.contains(d)),
        "{dup:?} vs {after:?}"
    );
    assert!(dup.windows(2).all(|w| w[1] == w[0] + 1));
    let text = bytes(&f.p);
    assert_eq!(bytes(&load(&text).unwrap()), text);
}

#[test]
fn ca4_deux_entrees_de_meme_identifiant_ou_hors_compteur_sont_refusees() {
    let mut doc = rich_v6();
    doc["variants"][0]["entries"] = json!([
        {"id": 11, "piece": 2, "quantity": 1},
        {"id": 11, "piece": 2, "quantity": 2, "option": "x"}
    ]);
    doc["variants"][0]["options"] = json!(["x"]);
    refused(
        &doc.to_string(),
        "identifiant en double dans le projet (11)",
    );
    let mut doc = rich_v6();
    doc["variants"][0]["entries"][0]["id"] = json!(30); // = next_id
    refused(&doc.to_string(), "incohérent avec le compteur");
    let mut doc = rich_v6();
    doc["variants"][0]["entries"][0]["id"] = json!(10); // = id de la variante
    refused(
        &doc.to_string(),
        "identifiant en double dans le projet (10)",
    );
    let mut doc = rich_v6();
    doc["variants"][0]["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("id");
    refused(&doc.to_string(), "missing field `id`");
}

#[test]
fn ca4_rename_option_reecrit_option_etat_actif_et_entrees_en_une_etape() {
    let f = vs();
    let mut doc = Document::new(f.p.clone());
    let before = bytes(doc.project());
    doc.execute(Command::RenameVariantOption {
        variant: f.v,
        option: "POCHE".into(), // casse ignorée à la résolution
        name: "  poche cousue ".into(),
    })
    .unwrap();
    let p = doc.project();
    assert_eq!(options(p, f.v), ["poche cousue", "col", "manche"]);
    assert_eq!(p.variant(f.v).unwrap().options_active, ["poche cousue"]);
    assert_eq!(
        entry_options(p, f.v),
        [None, Some("poche cousue".into()), Some("col".into())]
    );
    // Les commandes existantes adressent l'entrée par (pièce, option) avec le nouveau nom.
    doc.execute(Command::UpdateVariantEntry {
        variant: f.v,
        piece: f.poche,
        option: Some("poche cousue".into()),
        quantity: 3,
        material: None,
        new_option: Some("poche cousue".into()),
    })
    .unwrap();
    assert!(doc
        .execute(Command::UpdateVariantEntry {
            variant: f.v,
            piece: f.poche,
            option: Some("poche".into()),
            quantity: 1,
            material: None,
            new_option: None,
        })
        .is_err());
    // Annulation : une étape pour la mise à jour, une pour le renommage, octets d'avant.
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(doc.redo());
    assert_eq!(options(doc.project(), f.v)[0], "poche cousue");
}

#[test]
fn ca4_rename_option_refus_nommes_sans_trace() {
    let f = vs();
    let cases: [(&str, &str, &str); 6] = [
        ("poche", "", "ne peut pas être vide"),
        ("poche", "   ", "ne peut pas être vide"),
        ("poche", "COL", "existe déjà (casse ignorée)"),
        ("poche", " col ", "existe déjà (casse ignorée)"),
        ("inconnue", "x", "option « inconnue » inconnue"),
        ("poche", &"x".repeat(121), "dépasse 120 caractères"),
    ];
    for (option, name, needle) in cases {
        let mut p = f.p.clone();
        let before = bytes(&p);
        let err = p
            .apply(Command::RenameVariantOption {
                variant: f.v,
                option: option.into(),
                name: name.into(),
            })
            .expect_err(name);
        let msg = err.to_string();
        assert!(msg.contains(needle), "« {name} » : {msg}");
        assert_eq!(bytes(&p), before, "refus sans trace : {name}");
    }
    // Variante inconnue.
    let mut p = f.p.clone();
    assert!(p
        .apply(Command::RenameVariantOption {
            variant: VariantId(999),
            option: "poche".into(),
            name: "x".into(),
        })
        .is_err());
}

#[test]
fn ca4_rename_option_meme_nom_aucune_etape_et_changement_de_casse_permis() {
    let f = vs();
    let mut doc = Document::new(f.p.clone());
    let steps_before = doc.can_undo();
    doc.execute(Command::RenameVariantOption {
        variant: f.v,
        option: "poche".into(),
        name: "poche".into(),
    })
    .unwrap();
    assert_eq!(doc.can_undo(), steps_before);
    assert_eq!(
        bytes(doc.project()),
        bytes(&f.p),
        "même nom : rien ne change"
    );
    // Seule la casse change : permis (ce n'est pas un doublon).
    let mut p = f.p.clone();
    p.apply(Command::RenameVariantOption {
        variant: f.v,
        option: "poche".into(),
        name: "POCHE".into(),
    })
    .unwrap();
    assert_eq!(options(&p, f.v)[0], "POCHE");
    assert_eq!(p.variant(f.v).unwrap().options_active, ["POCHE"]);
    assert_eq!(entry_options(&p, f.v)[1].as_deref(), Some("POCHE"));
}

#[test]
fn ca4_move_option_reordonne_et_refuse_hors_liste() {
    let f = vs();
    let mut p = f.p.clone();
    let mv = |p: &mut Project, o: &str, to: usize| {
        p.apply(Command::MoveVariantOption {
            variant: f.v,
            option: o.into(),
            to,
        })
    };
    mv(&mut p, "poche", 2).unwrap();
    assert_eq!(options(&p, f.v), ["col", "manche", "poche"]);
    mv(&mut p, "MANCHE", 0).unwrap();
    assert_eq!(options(&p, f.v), ["manche", "col", "poche"]);
    // Entrées, état actif et identifiants ne bougent pas.
    assert_eq!(p.variant(f.v).unwrap().options_active, ["poche"]);
    assert_eq!(
        entry_options(&p, f.v),
        [None, Some("poche".into()), Some("col".into())]
    );
    // Refus nommés sans trace.
    let before = bytes(&p);
    let e = mv(&mut p, "col", 3).unwrap_err().to_string();
    assert!(
        e.contains("position 3 hors de la liste de 3 option(s)"),
        "{e}"
    );
    let e = mv(&mut p, "zzz", 0).unwrap_err().to_string();
    assert!(e.contains("option « zzz » inconnue"), "{e}");
    assert_eq!(bytes(&p), before);
    // Même position : aucune étape ; sinon une étape annulable exactement.
    let mut doc = Document::new(p.clone());
    doc.execute(Command::MoveVariantOption {
        variant: f.v,
        option: "col".into(),
        to: 1,
    })
    .unwrap();
    assert_eq!(bytes(doc.project()), before);
    doc.execute(Command::MoveVariantOption {
        variant: f.v,
        option: "col".into(),
        to: 0,
    })
    .unwrap();
    assert_eq!(options(doc.project(), f.v), ["col", "manche", "poche"]);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    let _ = f.col;
}

// ---------------------------------------------------------------------------
// CA5 — image de fond (E14) : JSON exact écrit à la main
// ---------------------------------------------------------------------------

const SHA: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn bg_text(calibration: &str) -> String {
    format!(
        r#"{{
  "format": "patternforge",
  "schema_version": 6,
  "name": "Fond",
  "next_id": 2,
  "sheets": [
    {{
      "id": 1,
      "name": "Feuille 1",
      "points": [],
      "entities": [],
      "background": {{
        "image": "fonds/scan.png",
        "sha256": "{SHA}",
        "width_px": 2480,
        "height_px": 3508,
        "calibration": {calibration},
        "opacity": 0.5,
        "visible": true,
        "locked": false
      }}
    }}
  ]
}}
"#
    )
}

const TWO_POINTS: &str = r#"{
          "kind": "two_points",
          "points_px": [
            {
              "x": 100.0,
              "y": 200.0
            },
            {
              "x": 900.0,
              "y": 200.0
            }
          ],
          "known_distance_mm": 200.0,
          "residual_mm": 0.5
        }"#;

const FOUR_POINTS: &str = r#"{
          "kind": "four_points",
          "points_px": [
            {
              "x": 0.0,
              "y": 0.0
            },
            {
              "x": 1000.0,
              "y": 0.0
            },
            {
              "x": 1000.0,
              "y": 500.0
            },
            {
              "x": 0.0,
              "y": 500.0
            }
          ],
          "known_width_mm": 400.0,
          "known_height_mm": 200.0,
          "residual_mm": 0.0
        }"#;

#[test]
fn ca5_fond_deux_points_et_quatre_points_aller_retour_octet() {
    for (name, cal) in [("deux points", TWO_POINTS), ("quatre points", FOUR_POINTS)] {
        let text = bg_text(cal);
        let p = load(&text).unwrap_or_else(|e| panic!("{name} : {e}"));
        let b = p.sheets[0].background.as_ref().unwrap();
        assert_eq!((b.width_px, b.height_px, b.opacity), (2480, 3508, 0.5));
        assert!(b.visible && !b.locked);
        assert_eq!(bytes(&p), text, "{name} : octets");
    }
    match load(&bg_text(FOUR_POINTS)).unwrap().sheets[0]
        .background
        .as_ref()
        .unwrap()
        .calibration
    {
        Calibration::FourPoints {
            known_width_mm,
            known_height_mm,
            residual_mm,
            ..
        } => assert_eq!(
            (known_width_mm, known_height_mm, residual_mm),
            (400.0, 200.0, 0.0)
        ),
        _ => panic!("quatre points attendus"),
    }
}

#[test]
fn ca5_sans_fond_rien_n_est_ecrit_et_l_image_n_est_jamais_integree() {
    let p = Project::new("Sans fond");
    assert!(!bytes(&p).contains("background"));
    let text = bg_text(TWO_POINTS);
    let out = bytes(&load(&text).unwrap());
    // Seulement la référence : le fichier ne grossit pas avec l'image.
    assert!(out.len() < 1000, "{}", out.len());
    assert!(!out.contains("base64") && !out.contains("data:"));
}

fn bg_doc() -> Value {
    serde_json::from_str(&bg_text(TWO_POINTS)).unwrap()
}

fn set(doc: &mut Value, pointer: &str, v: Value) {
    *doc.pointer_mut(pointer).unwrap() = v;
}

const BG: &str = "/sheets/0/background";

#[test]
fn ca5_chemins_d_image_dangereux_refuses_avec_la_raison() {
    let cases: [(&str, &str); 14] = [
        ("/etc/passwd.png", "chemin absolu"),
        ("\\\\serveur\\partage\\a.png", "séparateur"),
        ("C:/fonds/a.png", "« : » refusé"),
        ("C:\\fonds\\a.png", "séparateur"),
        ("../a.png", "segment « .. » refusé"),
        ("fonds/../../a.png", "segment « .. » refusé"),
        ("fonds/..", "segment « .. » refusé"),
        ("./a.png", "segment « . » refusé"),
        ("fonds//a.png", "segment vide"),
        ("fonds/", "segment vide"),
        ("", "vide"),
        (" a.png", "vide ou avec des espaces"),
        ("fonds\\a.png", "séparateur"),
        ("a\u{0}b.png", "caractère de contrôle"),
    ];
    for (path, needle) in cases {
        let mut doc = bg_doc();
        set(&mut doc, &format!("{BG}/image"), json!(path));
        refused(&doc.to_string(), needle);
    }
    let mut doc = bg_doc();
    set(&mut doc, &format!("{BG}/image"), json!("a".repeat(261)));
    refused(&doc.to_string(), "trop long");
    // Des chemins relatifs sûrs sont acceptés.
    for ok in ["a.png", "fonds/sous dossier/a b.PNG", "a..b/c.png", "...x"] {
        let mut doc = bg_doc();
        set(&mut doc, &format!("{BG}/image"), json!(ok));
        load(&doc.to_string()).unwrap_or_else(|e| panic!("{ok} : {e}"));
    }
}

#[test]
fn ca5_empreinte_dimensions_opacite_et_calibration_invalides_refusees() {
    let sha_cases = [
        "00".repeat(31),                // 62 caractères
        "0".repeat(63),                 // 63
        "0".repeat(65),                 // 65
        "G".repeat(64),                 // non hexadécimal
        "A".repeat(64),                 // majuscules : forme canonique = minuscules
        format!("{}é", "0".repeat(63)), // multi-octets
        String::new(),
    ];
    for sha in sha_cases {
        let mut doc = bg_doc();
        set(&mut doc, &format!("{BG}/sha256"), json!(sha));
        refused(
            &doc.to_string(),
            "l'empreinte doit avoir 64 chiffres hexadécimaux minuscules",
        );
    }
    for (field, v, needle) in [
        (
            "width_px",
            json!(0),
            "largeur en pixels hors de [1 ; 100000]",
        ),
        (
            "height_px",
            json!(0),
            "hauteur en pixels hors de [1 ; 100000]",
        ),
        ("width_px", json!(100_001), "largeur en pixels hors de"),
        (
            "opacity",
            json!(-0.1),
            "l'opacité doit être comprise entre 0 et 1",
        ),
        (
            "opacity",
            json!(1.1),
            "l'opacité doit être comprise entre 0 et 1",
        ),
    ] {
        let mut doc = bg_doc();
        set(&mut doc, &format!("{BG}/{field}"), v);
        refused(&doc.to_string(), needle);
    }
    // 1e999 (hors de f64) n'est pas un nombre JSON lisible : refusé, jamais accepté en silence.
    let big = bg_text(TWO_POINTS).replace("\"opacity\": 0.5", "\"opacity\": 1e999");
    assert!(load(&big).is_err());
    // Bornes incluses : opacité 0 et 1, dimensions 1 et 100000.
    for (field, v) in [
        ("opacity", json!(0.0)),
        ("opacity", json!(1.0)),
        ("width_px", json!(1)),
        ("height_px", json!(100_000)),
    ] {
        let mut doc = bg_doc();
        set(&mut doc, &format!("{BG}/{field}"), v);
        load(&doc.to_string()).unwrap();
    }
    // Calibration : distance nulle ou négative, résidu négatif, points confondus.
    let cal = format!("{BG}/calibration");
    for (path, v, needle) in [
        (
            "known_distance_mm",
            json!(0.0),
            "distance connue : doit être > 0",
        ),
        (
            "known_distance_mm",
            json!(-5.0),
            "distance connue : doit être > 0",
        ),
        ("known_distance_mm", json!(1e8), "hors bornes"),
        ("residual_mm", json!(-0.001), "doit être ≥ 0"),
    ] {
        let mut doc = bg_doc();
        set(&mut doc, &format!("{cal}/{path}"), v);
        refused(&doc.to_string(), needle);
    }
    let mut doc = bg_doc();
    set(
        &mut doc,
        &format!("{cal}/points_px/1"),
        json!({"x": 100.0, "y": 200.0}),
    );
    refused(&doc.to_string(), "est confondu avec un autre");
    let mut doc: Value = serde_json::from_str(&bg_text(FOUR_POINTS)).unwrap();
    set(&mut doc, &format!("{cal}/known_height_mm"), json!(0.0));
    refused(&doc.to_string(), "hauteur connue : doit être > 0");
    set(&mut doc, &format!("{cal}/known_height_mm"), json!(200.0));
    set(
        &mut doc,
        &format!("{cal}/points_px/3"),
        json!({"x": 1000.0, "y": 500.0}),
    );
    refused(&doc.to_string(), "est confondu avec un autre");
}

#[test]
fn ca5_formes_invalides_refusees_par_le_chargeur() {
    // Clé en trop (fond, calibration, point), clé manquante, type faux, genre de calibration inconnu.
    let mut doc = bg_doc();
    doc.pointer_mut(BG).unwrap()["zz"] = json!(1);
    refused(&doc.to_string(), "unknown field");
    let mut doc = bg_doc();
    doc.pointer_mut(&format!("{BG}/calibration")).unwrap()["zz"] = json!(1);
    refused(&doc.to_string(), "unknown field");
    let mut doc = bg_doc();
    doc.pointer_mut(&format!("{BG}/calibration/points_px/0"))
        .unwrap()["z"] = json!(1);
    refused(&doc.to_string(), "unknown field");
    let mut doc = bg_doc();
    set(
        &mut doc,
        &format!("{BG}/calibration/kind"),
        json!("three_points"),
    );
    refused(&doc.to_string(), "unknown variant");
    // Trois points pour « deux points », deux pour « quatre points ».
    let mut doc = bg_doc();
    let pts = doc
        .pointer_mut(&format!("{BG}/calibration/points_px"))
        .unwrap();
    pts.as_array_mut()
        .unwrap()
        .push(json!({"x": 1.0, "y": 1.0}));
    assert!(load(&doc.to_string()).is_err());
    let mut doc = bg_doc();
    doc.pointer_mut(BG)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("locked");
    refused(&doc.to_string(), "missing field `locked`");
    let mut doc = bg_doc();
    set(&mut doc, &format!("{BG}/visible"), json!("oui"));
    assert!(load(&doc.to_string()).is_err());
    let mut doc = bg_doc();
    set(&mut doc, &format!("{BG}/width_px"), json!(10.5));
    assert!(load(&doc.to_string()).is_err());
}

// ---------------------------------------------------------------------------
// CA6 — peau de cuir (E15) : JSON exact écrit à la main
// ---------------------------------------------------------------------------

fn hide_text(contour: &str, defects: &str, zones: &str) -> String {
    format!(
        r#"{{
  "format": "patternforge",
  "schema_version": 6,
  "name": "Cuir",
  "next_id": 10,
  "sheets": [
    {{
      "id": 1,
      "name": "Feuille 1",
      "points": [],
      "entities": []
    }}
  ],
  "cutting_plans": [
    {{
      "id": 2,
      "name": "Peau",
      "fabric_width_mm": 1500.0,
      "fold": "flat",
      "selvedge_mm": 0.0,
      "min_gap_mm": 0.0,
      "hide": {{
        "contour": {contour}{defects}{zones}
      }}
    }}
  ]
}}
"#
    )
}

const CONTOUR_4: &str = r#"[
          {
            "x": 0.0,
            "y": 0.0
          },
          {
            "x": 2000.0,
            "y": 0.0
          },
          {
            "x": 2000.0,
            "y": 1500.0
          },
          {
            "x": 0.0,
            "y": 1500.0
          }
        ]"#;

const DEFECTS_1: &str = r#",
        "defects": [
          {
            "id": 3,
            "kind": "hole",
            "polygon": [
              {
                "x": 100.0,
                "y": 100.0
              },
              {
                "x": 150.0,
                "y": 100.0
              },
              {
                "x": 125.0,
                "y": 140.0
              }
            ],
            "note": "trou de piqure"
          }
        ]"#;

const ZONES_1: &str = r#",
        "quality_zones": [
          {
            "id": 4,
            "grade": 2,
            "polygon": [
              {
                "x": 0.0,
                "y": 0.0
              },
              {
                "x": 500.0,
                "y": 0.0
              },
              {
                "x": 500.0,
                "y": 300.0
              }
            ]
          }
        ]"#;

#[test]
fn ca6_peau_complete_et_peau_sans_defaut_aller_retour_octet() {
    for (name, text) in [
        ("complète", hide_text(CONTOUR_4, DEFECTS_1, ZONES_1)),
        ("contour seul", hide_text(CONTOUR_4, "", "")),
        ("défauts seuls", hide_text(CONTOUR_4, DEFECTS_1, "")),
        ("zones seules", hide_text(CONTOUR_4, "", ZONES_1)),
    ] {
        let p = load(&text).unwrap_or_else(|e| panic!("{name} : {e}"));
        assert_eq!(bytes(&p), text, "{name} : octets");
    }
    let p = load(&hide_text(CONTOUR_4, DEFECTS_1, ZONES_1)).unwrap();
    let h = p.cutting_plans[0].hide.as_ref().unwrap();
    assert_eq!(h.contour.len(), 4);
    assert_eq!(
        (h.defects[0].id, h.defects[0].kind),
        (HideDefectId(3), DefectKind::Hole)
    );
    assert_eq!(h.defects[0].note.as_deref(), Some("trou de piqure"));
    assert_eq!(
        (h.quality_zones[0].id, h.quality_zones[0].grade),
        (QualityZoneId(4), 2)
    );
}

#[test]
fn ca6_un_plan_sans_peau_s_ecrit_sans_cle_hide() {
    let mut p = Project::new("Plan");
    p.apply(Command::AddCuttingPlan {
        spec: CuttingPlanSpec {
            name: "Tissu".into(),
            material: None,
            fabric_width_mm: 1500.0,
            fold: FabricFold::Flat,
            selvedge_mm: 0.0,
            min_gap_mm: 0.0,
            grain_direction: None,
        },
    })
    .unwrap();
    assert!(!bytes(&p).contains("hide"));
    // Mettre à jour le plan ne touche pas une peau posée.
    let mut doc: Value = serde_json::from_str(&hide_text(CONTOUR_4, DEFECTS_1, ZONES_1)).unwrap();
    doc["cutting_plans"][0]["name"] = json!("Peau");
    let mut p = load(&doc.to_string()).unwrap();
    let id = p.cutting_plans[0].id;
    p.apply(Command::UpdateCuttingPlan {
        plan: id,
        spec: CuttingPlanSpec {
            name: "Peau bis".into(),
            material: Some("Cuir".into()),
            fabric_width_mm: 1400.0,
            fold: FabricFold::Flat,
            selvedge_mm: 0.0,
            min_gap_mm: 0.0,
            grain_direction: None,
        },
    })
    .unwrap();
    assert!(p.cutting_plans[0].hide.is_some());
    assert_eq!(p.cutting_plans[0].hide.as_ref().unwrap().defects.len(), 1);
}

fn hide_doc() -> Value {
    serde_json::from_str(&hide_text(CONTOUR_4, DEFECTS_1, ZONES_1)).unwrap()
}

const HIDE: &str = "/cutting_plans/0/hide";

fn poly(points: &[(f64, f64)]) -> Value {
    Value::Array(
        points
            .iter()
            .map(|(x, y)| json!({"x": x, "y": y}))
            .collect(),
    )
}

#[test]
fn ca6_contour_invalide_refuse_avec_la_raison() {
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "deux sommets",
            poly(&[(0.0, 0.0), (10.0, 0.0)]),
            "un polygone a de 3 à 1000 sommets",
        ),
        (
            "papillon",
            poly(&[(0.0, 0.0), (100.0, 100.0), (100.0, 0.0), (0.0, 100.0)]),
            "le polygone n'est pas simple",
        ),
        (
            "sommet de fermeture répété",
            poly(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 0.0)]),
            "le polygone n'est pas simple",
        ),
        (
            "sommets consécutifs confondus",
            poly(&[(0.0, 0.0), (100.0, 0.0), (100.0, 0.0), (100.0, 100.0)]),
            "le polygone n'est pas simple",
        ),
        (
            "tous alignés",
            poly(&[(0.0, 0.0), (50.0, 0.0), (100.0, 0.0)]),
            "le polygone n'est pas simple",
        ),
        (
            "coordonnée hors bornes",
            poly(&[(0.0, 0.0), (2e7, 0.0), (2e7, 100.0)]),
            "hors de ±",
        ),
    ];
    for (name, contour, needle) in cases {
        let mut doc = hide_doc();
        doc["cutting_plans"][0]["hide"]["contour"] = contour;
        // Les défauts ne peuvent plus être contenus : on les retire pour isoler le contour.
        let h = doc.pointer_mut(HIDE).unwrap().as_object_mut().unwrap();
        h.remove("defects");
        refused(&doc.to_string(), needle);
        let _ = name;
    }
    // Contour trop long : 1001 sommets.
    let mut pts = Vec::new();
    for i in 0..1001 {
        let a = i as f64 * std::f64::consts::TAU / 1001.0;
        pts.push((1000.0 + 500.0 * a.cos(), 1000.0 + 500.0 * a.sin()));
    }
    let mut doc = hide_doc();
    doc["cutting_plans"][0]["hide"]["contour"] = poly(&pts);
    doc.pointer_mut(HIDE)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("defects");
    doc.pointer_mut(HIDE)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("quality_zones");
    refused(&doc.to_string(), "un polygone a de 3 à 1000 sommets");
}

#[test]
fn ca6_defaut_hors_du_contour_ou_qui_le_traverse_refuse_en_le_nommant() {
    // Sommet dehors.
    let mut doc = hide_doc();
    doc["cutting_plans"][0]["hide"]["defects"][0]["polygon"] =
        poly(&[(1990.0, 100.0), (2010.0, 100.0), (2000.0, 140.0)]);
    refused(
        &doc.to_string(),
        "défaut 3 : le défaut sort du contour de la peau",
    );
    // Contour en U : les deux bras sont dedans, le défaut les relie en passant par le vide.
    let u = poly(&[
        (0.0, 0.0),
        (300.0, 0.0),
        (300.0, 300.0),
        (200.0, 300.0),
        (200.0, 100.0),
        (100.0, 100.0),
        (100.0, 300.0),
        (0.0, 300.0),
    ]);
    let mut doc = hide_doc();
    doc["cutting_plans"][0]["hide"]["contour"] = u.clone();
    doc["cutting_plans"][0]["hide"]["defects"][0]["polygon"] =
        poly(&[(50.0, 250.0), (250.0, 250.0), (250.0, 280.0), (50.0, 280.0)]);
    refused(&doc.to_string(), "le défaut sort du contour de la peau");
    // Dans un bras du U : accepté (sommets sur le bord compris).
    let mut doc = hide_doc();
    doc["cutting_plans"][0]["hide"]["contour"] = u;
    doc["cutting_plans"][0]["hide"]["defects"][0]["polygon"] =
        poly(&[(0.0, 200.0), (100.0, 200.0), (50.0, 280.0)]);
    doc["cutting_plans"][0]["hide"]["quality_zones"] = json!([]);
    load(&doc.to_string()).expect("défaut dans un bras, bord compris");
    // Un défaut qui épouse tout le contour (bord compris) est contenu.
    let mut doc = hide_doc();
    doc["cutting_plans"][0]["hide"]["defects"][0]["polygon"] = rect(0.0, 0.0, 2000.0, 1500.0);
    load(&doc.to_string()).expect("défaut égal au contour");
}

#[test]
fn ca6_defauts_zones_notes_genres_et_identifiants_invalides_refuses() {
    let d = format!("{HIDE}/defects/0");
    let z = format!("{HIDE}/quality_zones/0");
    for (pointer, v, needle) in [
        (format!("{d}/kind"), json!("tear"), "unknown variant"),
        (format!("{d}/kind"), json!("Hole"), "unknown variant"),
        (format!("{d}/note"), json!(""), "la note doit être non vide"),
        (
            format!("{d}/note"),
            json!(" x"),
            "la note doit être non vide",
        ),
        (
            format!("{d}/note"),
            json!("a\nb"),
            "la note doit être non vide",
        ),
        (
            format!("{d}/note"),
            json!("n".repeat(501)),
            "la note doit être non vide",
        ),
        (format!("{d}/id"), json!(10), "incohérent avec le compteur"),
        (
            format!("{d}/id"),
            json!(2),
            "identifiant en double dans le projet (2)",
        ),
        (
            format!("{z}/grade"),
            json!(0),
            "la note de qualité doit être 1, 2 ou 3 (reçu 0)",
        ),
        (
            format!("{z}/grade"),
            json!(4),
            "la note de qualité doit être 1, 2 ou 3 (reçu 4)",
        ),
        (
            format!("{z}/id"),
            json!(3),
            "identifiant en double dans le projet (3)",
        ),
        (
            format!("{z}/polygon"),
            poly(&[(0.0, 0.0), (10.0, 0.0), (5.0, 5.0), (5.0, -5.0)]),
            "le polygone n'est pas simple",
        ),
    ] {
        let mut doc = hide_doc();
        set(&mut doc, &pointer, v);
        refused(&doc.to_string(), needle);
    }
    // Les 5 genres sont acceptés.
    for kind in ["hole", "scar", "stain", "thin", "other"] {
        let mut doc = hide_doc();
        set(&mut doc, &format!("{d}/kind"), json!(kind));
        load(&doc.to_string()).unwrap_or_else(|e| panic!("{kind} : {e}"));
    }
    // Clés en trop.
    for pointer in [HIDE.to_string(), d.clone(), z.clone()] {
        let mut doc = hide_doc();
        doc.pointer_mut(&pointer).unwrap()["zz"] = json!(1);
        refused(&doc.to_string(), "unknown field");
    }
    let mut doc = hide_doc();
    doc.pointer_mut(&format!("{d}/polygon/0")).unwrap()["z"] = json!(1);
    refused(&doc.to_string(), "unknown field");
    // Contour obligatoire.
    let mut doc = hide_doc();
    doc.pointer_mut(HIDE)
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("contour");
    refused(&doc.to_string(), "missing field `contour`");
}

#[test]
fn ca6_nombre_de_defauts_borne_a_500() {
    let tri = |i: usize| {
        let x = 10.0 + (i % 40) as f64 * 40.0;
        let y = 10.0 + (i / 40) as f64 * 40.0;
        poly(&[(x, y), (x + 10.0, y), (x + 5.0, y + 8.0)])
    };
    let defects = |n: usize| -> Value {
        Value::Array(
            (0..n)
                .map(|i| json!({"id": 100 + i, "kind": "other", "polygon": tri(i)}))
                .collect(),
        )
    };
    let mut doc = hide_doc();
    doc["next_id"] = json!(10_000);
    doc["cutting_plans"][0]["hide"]["defects"] = defects(500);
    doc["cutting_plans"][0]["hide"]["quality_zones"] = json!([]);
    load(&doc.to_string()).expect("500 défauts : la limite est incluse");
    doc["cutting_plans"][0]["hide"]["defects"] = defects(501);
    refused(&doc.to_string(), "trop de défauts (500 au plus)");
}

// ---------------------------------------------------------------------------
// CA7 — pf-validate et schéma JSON couvrent les clés v6
// ---------------------------------------------------------------------------

#[test]
fn ca7_pf_validate_0_sur_les_fichiers_v6_et_1_sur_les_abimes() {
    let ok = validate_cli(&rich_v6().to_string(), "riche.pforge.json");
    assert_eq!(ok.code, 0, "{}", ok.stdout);
    let cases: Vec<(&str, Value, &str)> = {
        let mut v = Vec::new();
        let mut d = rich_v6();
        set(&mut d, "/sheets/0/background/image", json!("../a.png"));
        v.push(("chemin", d, "segment « .. » refusé"));
        let mut d = rich_v6();
        set(&mut d, "/sheets/0/background/sha256", json!("zz"));
        v.push(("empreinte", d, "empreinte"));
        let mut d = rich_v6();
        set(
            &mut d,
            "/cutting_plans/0/hide/defects/0/polygon",
            poly(&[(5000.0, 0.0), (5100.0, 0.0), (5050.0, 90.0)]),
        );
        v.push(("défaut", d, "sort du contour"));
        let mut d = rich_v6();
        set(&mut d, "/sizes/0/sizes/1/client", json!(77));
        v.push(("fiche", d, "n'existe plus"));
        let mut d = rich_v6();
        set(&mut d, "/schema_version", json!(5));
        v.push(("clé v6 dans v5", d, "réservée au format version 6"));
        v
    };
    for (name, doc, needle) in cases {
        let run = validate_cli(
            &doc.to_string(),
            &format!("{}.pforge.json", name.replace(' ', "_")),
        );
        assert_eq!(run.code, 1, "{name} : {}", run.stdout);
        assert!(run.stdout.contains(needle), "{name} : {}", run.stdout);
    }
}

fn schema_ok(doc: &Value) -> bool {
    validateur::validate(&schema(), doc).is_empty()
}

#[test]
fn ca7_le_schema_v6_accepte_les_fichiers_v6_et_refuse_ce_que_le_chargeur_refuse() {
    assert_eq!(schema()["properties"]["schema_version"]["const"], json!(6));
    for (name, doc) in [
        ("riche", rich_v6()),
        ("fond deux points", bg_doc()),
        (
            "fond quatre points",
            serde_json::from_str(&bg_text(FOUR_POINTS)).unwrap(),
        ),
        ("peau", hide_doc()),
    ] {
        let errors = validateur::validate(&schema(), &doc);
        assert!(errors.is_empty(), "{name} : {errors:?}");
        assert!(load(&doc.to_string()).is_ok(), "{name}");
    }
    // Mutations refusées par le schéma ET le chargeur.
    let cases: Vec<(&str, Value)> = {
        let mut v: Vec<(&str, Value)> = Vec::new();
        let mut d = rich_v6();
        set(&mut d, "/sheets/0/background/opacity", json!(1.5));
        v.push(("opacité", d));
        let mut d = rich_v6();
        set(&mut d, "/sheets/0/background/sha256", json!("abc"));
        v.push(("empreinte", d));
        let mut d = rich_v6();
        set(&mut d, "/sheets/0/background/width_px", json!(0));
        v.push(("largeur", d));
        let mut d = rich_v6();
        set(&mut d, "/sheets/0/background/calibration/kind", json!("x"));
        v.push(("genre de calibration", d));
        let mut d = rich_v6();
        d.pointer_mut("/sheets/0/background").unwrap()["zz"] = json!(1);
        v.push(("clé en trop du fond", d));
        let mut d = rich_v6();
        d.pointer_mut("/sheets/0/background/calibration/points_px")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .pop();
        v.push(("un seul point", d));
        let mut d = rich_v6();
        set(
            &mut d,
            "/cutting_plans/0/hide/defects/0/kind",
            json!("tear"),
        );
        v.push(("genre de défaut", d));
        let mut d = rich_v6();
        set(
            &mut d,
            "/cutting_plans/0/hide/quality_zones/0/grade",
            json!(4),
        );
        v.push(("note de qualité", d));
        let mut d = rich_v6();
        set(
            &mut d,
            "/cutting_plans/0/hide/contour",
            poly(&[(0.0, 0.0), (1.0, 0.0)]),
        );
        v.push(("contour de deux sommets", d));
        let mut d = rich_v6();
        d.pointer_mut("/cutting_plans/0/hide/defects/0")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("id");
        v.push(("défaut sans id", d));
        let mut d = rich_v6();
        d.pointer_mut("/variants/0/entries/0")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("id");
        v.push(("entrée sans id", d));
        let mut d = rich_v6();
        d.pointer_mut("/sizes/0/sizes/1").unwrap()["client"] = json!("Alice");
        v.push(("client non entier", d));
        let mut d = rich_v6();
        set(&mut d, "/schema_version", json!(5));
        v.push(("version 5 écrite avec clés v6", d));
        v
    };
    for (name, doc) in cases {
        assert!(!schema_ok(&doc), "{name} : le schéma accepte");
        assert!(
            load(&doc.to_string()).is_err(),
            "{name} : le chargeur accepte"
        );
    }
    // Écarts connus et déclarés (FORMAT_PFORGE_V6 § 9, § 4.14, § 4.15) : le schéma accepte, le chargeur refuse.
    let known: Vec<(&str, Value)> = {
        let mut v: Vec<(&str, Value)> = Vec::new();
        let mut d = rich_v6();
        set(&mut d, "/sheets/0/background/image", json!("../a.png"));
        v.push(("chemin dangereux", d));
        let mut d = rich_v6();
        set(
            &mut d,
            "/sheets/0/background/calibration/known_distance_mm",
            json!(0.0),
        );
        v.push(("distance nulle", d));
        let mut d = rich_v6();
        set(
            &mut d,
            "/cutting_plans/0/hide/contour",
            poly(&[(0.0, 0.0), (100.0, 100.0), (100.0, 0.0), (0.0, 100.0)]),
        );
        v.push(("contour auto-croisé", d));
        let mut d = rich_v6();
        set(
            &mut d,
            "/cutting_plans/0/hide/defects/0/polygon",
            poly(&[(5000.0, 0.0), (5100.0, 0.0), (5050.0, 90.0)]),
        );
        v.push(("défaut hors contour", d));
        v
    };
    for (name, doc) in known {
        assert!(schema_ok(&doc), "{name} : écart déclaré, le schéma accepte");
        assert!(load(&doc.to_string()).is_err(), "{name}");
    }
}

#[test]
fn ca7_la_specification_v6_decrit_les_nouvelles_cles() {
    let spec = include_str!("../../../docs/FORMAT_PFORGE_V6.md");
    for needle in [
        "version de schéma 6",
        "sheets[].background",
        "cutting_plans[].hide",
        "sizes[].sizes[].client",
        "variants[].entries[].id",
        "RenameVariantOption",
        "MoveVariantOption",
        "Table de migration v5 → v6",
        "clé réservée au format version 6",
        "pforge-v6.schema.json",
    ] {
        assert!(
            spec.contains(needle),
            "FORMAT_PFORGE_V6.md sans « {needle} »"
        );
    }
    let adr = include_str!("../../../docs/ADR_V6_STOCKAGE.md");
    assert!(adr.contains("455") && adr.contains("459"));
}

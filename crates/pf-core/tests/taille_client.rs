//! US-10.2 (reste) — « taille sur-mesure générée depuis la fiche client » (`GenerateClientSize`).
//! Valeurs calculées à la main dans chaque test.
//!
//! Fixture : gamme « Femme » S M L (base M) ;
//!
//! | mesure           | unité | S   | M    | L   |
//! |------------------|-------|-----|------|-----|
//! | Tour de poitrine | mm    | 880 | 920  | 960 |
//! | Longueur dos     | mm    | 400 | 410  | 420 |
//! | Angle d'épaule   | deg   | 20  | 21,5 | 22  |
//!
//! Fiche « Alice » : poitrine 930, dos 408, épaule **manquante**.

use pf_core::persist;
use pf_core::*;

struct F {
    p: Project,
    range: SizeRangeId,
    sizes: [SizeId; 3],
    poitrine: MeasurementId,
    dos: MeasurementId,
    epaule: MeasurementId,
    alice: ClientId,
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn fixture() -> F {
    let mut p = Project::new("Sur-mesure");
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let r = p.sizes[0].clone();
    let sizes = [r.sizes[0].id, r.sizes[1].id, r.sizes[2].id];
    let mut ms = Vec::new();
    for (name, unit, vals) in [
        (
            "Tour de poitrine",
            MeasurementUnit::Mm,
            [880.0, 920.0, 960.0],
        ),
        ("Longueur dos", MeasurementUnit::Mm, [400.0, 410.0, 420.0]),
        ("Angle d'épaule", MeasurementUnit::Deg, [20.0, 21.5, 22.0]),
    ] {
        let id = p
            .apply(Command::AddMeasurement {
                name: name.into(),
                unit,
            })
            .unwrap()
            .measurements[0];
        for (s, v) in sizes.iter().zip(vals) {
            p.apply(Command::SetMeasurementValue {
                measurement: id,
                size: *s,
                value: v,
            })
            .unwrap();
        }
        ms.push(id);
    }
    let alice = p
        .apply(Command::AddClient {
            name: "Alice".into(),
        })
        .unwrap()
        .clients[0];
    for (m, v) in [(ms[0], 930.0), (ms[1], 408.0)] {
        p.apply(Command::SetClientValue {
            client: alice,
            measurement: m,
            value: v,
        })
        .unwrap();
    }
    F {
        p,
        range: r.id,
        sizes,
        poitrine: ms[0],
        dos: ms[1],
        epaule: ms[2],
        alice,
    }
}

fn value(p: &Project, m: MeasurementId, s: SizeId) -> Option<f64> {
    p.measurement(m)
        .unwrap()
        .values
        .iter()
        .find(|v| v.size == s)
        .map(|v| v.value)
}

fn generate(f: &F, label: &str, replace: bool) -> Command {
    Command::GenerateClientSize {
        client: f.alice,
        range: f.range,
        label: label.into(),
        replace,
    }
}

fn refused(p: &mut Project, c: Command) -> String {
    let before = bytes(p);
    let snapshot = p.clone();
    let e = p.apply(c).expect_err("doit être refusée").to_string();
    assert_eq!(bytes(p), before, "refus sans trace");
    assert_eq!(*p, snapshot);
    e
}

#[test]
fn ca1_la_colonne_vient_de_la_fiche_et_le_manquant_reste_vide() {
    let mut f = fixture();
    let before = f.p.clone();
    let out =
        f.p.apply(generate(&f, "  Alice sur-mesure ", false))
            .unwrap();
    let new = out.sizes[0];
    let r = f.p.size_range(f.range).unwrap();
    // Ajoutée à la fin, libellé rogné, base inchangée.
    assert_eq!(r.sizes.len(), 4);
    assert_eq!(r.sizes[3].id, new);
    assert_eq!(r.sizes[3].label, "Alice sur-mesure");
    assert_eq!(r.base, f.sizes[1]);
    // Colonne : 930 / 408 ; épaule vide (jamais 22 de L, ni 21,5 de la base, ni une moyenne).
    assert_eq!(value(&f.p, f.poitrine, new), Some(930.0));
    assert_eq!(value(&f.p, f.dos, new), Some(408.0));
    assert_eq!(value(&f.p, f.epaule, new), None);
    // Rapport : une seule mesure manquante, nommée dans la remarque.
    assert_eq!(
        f.p.client_missing_measurements(f.alice).unwrap(),
        vec![f.epaule]
    );
    assert!(out.notes.iter().any(|n| n.contains("Angle d'épaule")
        && n.contains("2 mesure(s) renseignée(s)")
        && n.contains("1 manquante(s)")));
    // Les autres colonnes et la fiche sont intactes.
    for s in f.sizes {
        for m in [f.poitrine, f.dos, f.epaule] {
            assert_eq!(value(&f.p, m, s), value(&before, m, s));
        }
    }
    assert_eq!(f.p.clients, before.clients);
    persist::from_json(&bytes(&f.p)).unwrap();
}

#[test]
fn ca2_refus_nommes_sans_trace() {
    let mut f = fixture();
    let c = |client, range, label: &str, replace| Command::GenerateClientSize {
        client,
        range,
        label: label.into(),
        replace,
    };
    let e = refused(&mut f.p, c(ClientId(9999), f.range, "X", false));
    assert!(e.contains("fiche client introuvable"), "{e}");
    let e = refused(&mut f.p, c(f.alice, SizeRangeId(9999), "X", false));
    assert!(e.contains("gamme"), "{e}");
    for vide in ["", "   "] {
        refused(&mut f.p, c(f.alice, f.range, vide, false));
    }
    let e = refused(&mut f.p, c(f.alice, f.range, &"x".repeat(21), false));
    assert!(e.contains("20"), "{e}");
    // Doublon (casse ignorée), avec ou sans confirmation de remplacement inutile : sans = refusé.
    let e = refused(&mut f.p, c(f.alice, f.range, "m", false));
    assert!(e.contains('m'), "{e}");
    // Limite de 60 tailles par gamme : 3 + 57 = 60, la suivante est refusée.
    for i in 0..57 {
        f.p.apply(Command::AddSize {
            range: f.range,
            label: format!("T{i}"),
            at: None,
        })
        .unwrap();
    }
    assert_eq!(f.p.size_range(f.range).unwrap().sizes.len(), 60);
    let e = refused(&mut f.p, c(f.alice, f.range, "Alice", false));
    assert!(e.contains("60"), "{e}");
}

#[test]
fn ca3_une_etape_annulable_et_rejouer_ne_fait_rien() {
    let f = fixture();
    let mut doc = Document::new(f.p.clone());
    doc.execute(generate(&f, "Alice", false)).unwrap();
    assert!(doc.can_undo());
    assert_eq!(doc.project().size_range(f.range).unwrap().sizes.len(), 4);
    // Rejoué avec confirmation et colonne identique : aucune étape de plus.
    doc.execute(generate(&f, "Alice", true)).unwrap();
    assert!(doc.undo());
    assert!(!doc.can_undo(), "une seule étape pour deux commandes");
    assert_eq!(*doc.project(), f.p);
    // Rejoué sans confirmation : refusé (le libellé existe).
    let mut p = f.p.clone();
    p.apply(generate(&f, "Alice", false)).unwrap();
    let e = refused(&mut p, generate(&f, "Alice", false));
    assert!(e.contains("Alice"), "{e}");
}

#[test]
fn ca4_regenerer_remplace_toute_la_colonne_apres_confirmation() {
    let mut f = fixture();
    let new = f.p.apply(generate(&f, "Alice", false)).unwrap().sizes[0];
    // La fiche change : épaule 19 renseignée, dos effacé.
    f.p.apply(Command::SetClientValue {
        client: f.alice,
        measurement: f.epaule,
        value: 19.0,
    })
    .unwrap();
    f.p.apply(Command::ClearClientValue {
        client: f.alice,
        measurement: f.dos,
    })
    .unwrap();
    // Rien ne suit tout seul : la taille garde l'ancienne colonne.
    assert_eq!(value(&f.p, f.dos, new), Some(408.0));
    assert_eq!(value(&f.p, f.epaule, new), None);
    let mut doc = Document::new(f.p.clone());
    let out = doc.execute(generate(&f, "alice", true)).unwrap();
    assert!(out.sizes.is_empty(), "aucune taille créée");
    let p = doc.project();
    assert_eq!(p.size_range(f.range).unwrap().sizes.len(), 4);
    assert_eq!(value(p, f.poitrine, new), Some(930.0));
    assert_eq!(value(p, f.dos, new), None, "ancienne cellule effacée");
    assert_eq!(value(p, f.epaule, new), Some(19.0));
    assert!(doc.undo());
    assert!(!doc.can_undo());
    assert_eq!(value(doc.project(), f.dos, new), Some(408.0));
}

#[test]
fn ca5_la_taille_et_la_fiche_sont_independantes() {
    let mut f = fixture();
    let new = f.p.apply(generate(&f, "Alice", false)).unwrap().sizes[0];
    let client_before = f.p.client(f.alice).unwrap().clone();
    f.p.apply(Command::SetMeasurementValue {
        measurement: f.poitrine,
        size: new,
        value: 1000.0,
    })
    .unwrap();
    assert_eq!(f.p.client(f.alice).unwrap(), &client_before);
    f.p.apply(Command::SetClientValue {
        client: f.alice,
        measurement: f.poitrine,
        value: 905.0,
    })
    .unwrap();
    assert_eq!(value(&f.p, f.poitrine, new), Some(1000.0));
    // Schéma 6 (décision 456) : seule la référence « taille générée depuis la fiche » est stockée.
    // Supprimer une fiche citée est refusé en nommant la taille ; les valeurs, elles, restent
    // indépendantes (la taille ne suit pas la fiche).
    let before = bytes(&f.p);
    let err = f.p.apply(Command::RemoveClient { client: f.alice });
    let msg = err.unwrap_err().to_string();
    assert!(msg.contains("par la taille « Alice » de la gamme"), "{msg}");
    assert_eq!(bytes(&f.p), before, "refus sans trace");
    // La taille retirée, la fiche peut l'être ; la valeur de barème d'une autre taille ne bouge pas.
    f.p.apply(Command::RemoveSize { size: new }).unwrap();
    f.p.apply(Command::RemoveClient { client: f.alice })
        .unwrap();
    assert_eq!(f.p.size_range(f.range).unwrap().sizes.len(), 3);
    persist::from_json(&bytes(&f.p)).unwrap();
}

#[test]
fn ca6_la_taille_se_lit_comme_les_autres_dans_le_projet_evalue() {
    let mut f = fixture();
    f.p.apply(Command::AddVariable {
        name: "largeur".into(),
        unit: VariableUnit::Mm,
        expr: "100".into(),
    })
    .unwrap();
    let new = f.p.apply(generate(&f, "Alice", false)).unwrap().sizes[0];
    // Sans valeur propre : la variable vaut sa valeur de base ; avec : celle de la taille.
    let g = f.p.graded_project(Some(new)).unwrap();
    assert_eq!(g.variable_values_for_size(Some(new)).unwrap()[0].1, 100.0);
    f.p.apply(Command::SetVariableSizeExpr {
        name: "largeur".into(),
        size: new,
        expr: "150".into(),
    })
    .unwrap();
    let g = f.p.graded_project(Some(new)).unwrap();
    assert_eq!(g.variable_values_for_size(Some(new)).unwrap()[0].1, 150.0);
    // La colonne du barème est lisible dans le projet évalué ; aucun lien mesure → géométrie.
    assert_eq!(value(&g, f.poitrine, new), Some(930.0));
    assert_eq!(g.sheets, f.p.sheets);
}

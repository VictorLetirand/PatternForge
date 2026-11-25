//! US-04.2 — fiche de pièce : champs (code, catégorie, commentaire, quantité, paire,
//! matière), commande `SetPieceCard`, points d'attention (`piece_issues`) et fiche en
//! lecture seule (`piece_card`). Un test par critère d'acceptation.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};
use std::path::PathBuf;

const FIXTURES: [&str; 4] = [
    "v1-exercice.pforge.json",
    "v1-grande-piece.pforge.json",
    "v2-piece-contour-droit-fil.pforge.json",
    "v2-piece-sans-contour.pforge.json",
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn fixture(name: &str) -> Project {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    persist::from_json(&text).unwrap_or_else(|e| panic!("{name} : {e}"))
}

/// Rectangle de 100 × 60 mm et pièce liée extraite de sa région (aire 6000, périmètre 320).
fn with_piece() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Fiche");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 60.0,
    })
    .unwrap();
    let id = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, sheet, id)
}

fn full_card(piece: PieceId) -> Command {
    Command::SetPieceCard {
        piece,
        name: "Devant gauche".into(),
        code: Some("DV-01".into()),
        quantity: 2,
        mirrored_pair: true,
        material: Some("Drap de laine".into()),
        category: Some(PieceCategory::Interfacing),
        comment: Some("Couper dans le sens\ndu droit-fil".into()),
    }
}

fn to_value(p: &Project) -> Value {
    serde_json::from_str(&persist::to_json(p).unwrap()).unwrap()
}

/// Droit-fil : un segment de rôle `Grainline` ajouté à la feuille.
fn add_grainline(p: &mut Project, sheet: SheetId) -> EntityId {
    let e = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(10.0, 10.0)),
            to: PointRef::New(pt(10.0, 50.0)),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetPathRole {
        sheet,
        entity: e,
        role: PathRole::Grainline,
    })
    .unwrap();
    e
}

// ----- CA1 : champs de la fiche -------------------------------------------------

#[test]
fn ca1_champs_absents_octets_inchanges_apres_relecture() {
    for name in FIXTURES {
        let first = persist::to_json(&fixture(name)).unwrap();
        let second = persist::to_json(&persist::from_json(&first).unwrap()).unwrap();
        assert_eq!(first, second, "{name}");
        for key in ["\"code\"", "\"category\"", "\"comment\""] {
            assert!(!first.contains(key), "{name} : {key} écrit sans valeur");
        }
    }
    let (p, _, _) = with_piece();
    let text = persist::to_json(&p).unwrap();
    assert_eq!(
        text,
        persist::to_json(&persist::from_json(&text).unwrap()).unwrap()
    );
    for key in ["\"code\"", "\"category\"", "\"comment\""] {
        assert!(!text.contains(key));
    }
    // Fichier livré (schéma 3) : relu puis réécrit, il est identique à l'octet.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/piece-crans-droit-fil.pforge.json");
    let shipped = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    let again = persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap();
    assert_eq!(shipped, again);
}

#[test]
fn ca1_champs_presents_relus_a_l_identique_schema_inchange() {
    let (mut p, _, id) = with_piece();
    p.apply(full_card(id)).unwrap();
    let text = persist::to_json(&p).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6, "schéma 4 (US-09.1)");
    let piece = &v["pieces"][0];
    assert_eq!(piece["code"], "DV-01");
    assert_eq!(piece["category"], "interfacing", "snake_case");
    assert_eq!(piece["comment"], "Couper dans le sens\ndu droit-fil");
    assert_eq!(piece["quantity"], 2);
    assert_eq!(piece["mirrored_pair"], true);
    assert_eq!(piece["material"], "Drap de laine");
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), text);
    // Les nouveaux champs sont de vrais champs typés, pas des clés inconnues.
    assert!(back.pieces[0].extra.is_empty());
    // Chaque catégorie a sa clé stable, écrite telle quelle.
    for c in PieceCategory::ALL {
        let mut q = p.clone();
        q.pieces[0].category = Some(c);
        assert_eq!(to_value(&q)["pieces"][0]["category"], c.key());
        assert_eq!(
            persist::from_json(&persist::to_json(&q).unwrap()).unwrap(),
            q
        );
    }
    assert_eq!(
        PieceCategory::ALL.map(PieceCategory::key),
        [
            "fabric",
            "lining",
            "interfacing",
            "leather",
            "reinforcement"
        ]
    );
}

#[test]
fn ca1_valeurs_inconnues_ou_mal_typees_refusees_proprement() {
    let (mut p, _, id) = with_piece();
    p.apply(full_card(id)).unwrap();
    let base = to_value(&p);
    let bad = [
        ("category", json!("soie")),
        ("category", json!(3)),
        ("category", json!("Fabric")),
        ("code", json!(12)),
        ("code", json!("")),
        ("code", json!("a\u{0}b")),
        ("comment", json!(["x"])),
        ("comment", json!("")),
        ("comment", json!("x".repeat(MAX_COMMENT_LEN + 1))),
        ("quantity", json!(0)),
    ];
    for (key, value) in bad {
        let mut v = base.clone();
        v["pieces"][0][key] = value.clone();
        let err = persist::from_json(&v.to_string());
        assert!(err.is_err(), "{key} = {value} aurait dû être refusé");
    }
    // Le fichier d'origine reste lisible : aucun refus ne dépend d'un état global.
    assert!(persist::from_json(&base.to_string()).is_ok());
}

#[test]
fn ca1_champ_homonyme_dans_extra_jamais_perdu_en_silence() {
    let (p, _, _) = with_piece();
    let mut v = to_value(&p);
    // Fichier écrit par une version qui connaît déjà ces champs, valeurs conformes :
    // elles deviennent les champs typés (aucune perte, clés écrites une seule fois).
    v["pieces"][0]["code"] = json!("X9");
    v["pieces"][0]["category"] = json!("leather");
    v["pieces"][0]["comment"] = json!("ancien");
    let q = persist::from_json(&v.to_string()).unwrap();
    assert_eq!(q.pieces[0].code.as_deref(), Some("X9"));
    assert_eq!(q.pieces[0].category, Some(PieceCategory::Leather));
    assert_eq!(q.pieces[0].comment.as_deref(), Some("ancien"));
    assert!(q.pieces[0].extra.is_empty());
    let out = to_value(&q);
    assert_eq!(out["pieces"][0]["code"], "X9");
    assert_eq!(out["pieces"][0]["category"], "leather");
    // Un champ inconnu voisin reste, lui, conservé dans `extra`.
    v["pieces"][0]["zz_futur"] = json!({"a": 1});
    let q = persist::from_json(&v.to_string()).unwrap();
    assert_eq!(to_value(&q)["pieces"][0]["zz_futur"], json!({"a": 1}));
    // Construit à la main, un homonyme dans `extra` est refusé (jamais deux clés
    // identiques écrites) : validation et écriture échouent avec un message.
    for key in ["code", "category", "comment"] {
        let mut q = p.clone();
        q.pieces[0].extra.insert(key.into(), json!("x"));
        let e = q.validate().unwrap_err().to_string();
        assert!(e.contains(key), "{e}");
        assert!(persist::to_json(&q).is_err());
    }
}

// ----- CA2 : commande SetPieceCard ----------------------------------------------

#[test]
fn ca2_chaque_champ_par_la_commande_une_etape_annulable() {
    let (p, _, id) = with_piece();
    let mut doc = Document::new(p);
    type Setter = Box<dyn Fn(&mut Card)>;
    let setters: Vec<(&str, Setter)> = vec![
        ("name", Box::new(|f| f.name = "Dos".into())),
        ("code", Box::new(|f| f.code = Some("  D-1  ".into()))),
        ("quantity", Box::new(|f| f.quantity = 4)),
        ("pair", Box::new(|f| f.mirrored_pair = true)),
        ("material", Box::new(|f| f.material = Some("Coton".into()))),
        (
            "category",
            Box::new(|f| f.category = Some(PieceCategory::Lining)),
        ),
        ("comment", Box::new(|f| f.comment = Some("a\nb".into()))),
    ];
    for (field, setter) in setters {
        let before = doc.project().clone();
        let steps = doc.can_undo();
        let mut fields = Card::of(doc.project(), id);
        setter(&mut fields);
        doc.execute(fields.command(id))
            .unwrap_or_else(|e| panic!("{field} : {e}"));
        let after = doc.project().clone();
        assert_ne!(after, before, "{field}");
        // Une seule étape : un undo ramène exactement l'état précédent.
        assert!(doc.undo(), "{field}");
        assert_eq!(doc.project(), &before, "{field} : undo exact");
        assert_eq!(doc.can_undo(), steps, "{field} : une étape, pas plus");
        assert!(doc.redo());
        assert_eq!(doc.project(), &after, "{field} : redo exact");
    }
    let q = doc.project().piece(id).unwrap();
    assert_eq!(q.name, "Dos");
    assert_eq!(q.code.as_deref(), Some("D-1"), "espaces rognés");
    assert_eq!(q.quantity, 4);
    assert!(q.mirrored_pair);
    assert_eq!(q.material.as_deref(), Some("Coton"));
    assert_eq!(q.category, Some(PieceCategory::Lining));
    assert_eq!(q.comment.as_deref(), Some("a\nb"));
    // Le contour n'est pas touché par la fiche.
    assert_eq!(q.outline.len(), 4);
}

/// Champs de la fiche, pour construire une commande en modifiant un seul champ.
struct Card {
    name: String,
    code: Option<String>,
    quantity: u32,
    mirrored_pair: bool,
    material: Option<String>,
    category: Option<PieceCategory>,
    comment: Option<String>,
}

impl Card {
    /// Fiche vierge (nom « Devant », quantité 1).
    fn blank() -> Card {
        Card {
            name: "Devant".into(),
            code: None,
            quantity: 1,
            mirrored_pair: false,
            material: None,
            category: None,
            comment: None,
        }
    }

    /// Fiche actuelle d'une pièce.
    fn of(p: &Project, id: PieceId) -> Card {
        let q = p.piece(id).unwrap();
        Card {
            name: q.name.clone(),
            code: q.code.clone(),
            quantity: q.quantity,
            mirrored_pair: q.mirrored_pair,
            material: q.material.clone(),
            category: q.category,
            comment: q.comment.clone(),
        }
    }

    fn command(self, piece: PieceId) -> Command {
        Command::SetPieceCard {
            piece,
            name: self.name,
            code: self.code,
            quantity: self.quantity,
            mirrored_pair: self.mirrored_pair,
            material: self.material,
            category: self.category,
            comment: self.comment,
        }
    }
}

#[test]
fn ca2_refus_sans_trace() {
    let (p, _, id) = with_piece();
    let mut doc = Document::new(p);
    doc.execute(full_card(id)).unwrap();
    let before = doc.project().clone();
    let text_before = persist::to_json(&before).unwrap();
    let long_code = "c".repeat(MAX_NAME_LEN + 1);
    let long_comment = "m".repeat(MAX_COMMENT_LEN + 1);
    let refusals: Vec<(&str, Command)> = vec![
        ("quantité 0", with(id, |f| f.quantity = 0)),
        (
            "nom trop long",
            with(id, |f| f.name = "n".repeat(MAX_NAME_LEN + 1)),
        ),
        ("nom vide", with(id, |f| f.name = "   ".into())),
        ("nom NUL", with(id, |f| f.name = "a\u{0}b".into())),
        (
            "code trop long",
            with(id, |f| f.code = Some(long_code.clone())),
        ),
        (
            "code avec saut de ligne",
            with(id, |f| f.code = Some("a\nb".into())),
        ),
        ("code NUL", with(id, |f| f.code = Some("\u{0}".into()))),
        (
            "matière trop longue",
            with(id, |f| f.material = Some(long_code.clone())),
        ),
        (
            "commentaire trop long",
            with(id, |f| f.comment = Some(long_comment.clone())),
        ),
        (
            "commentaire NUL",
            with(id, |f| f.comment = Some("a\u{0}".into())),
        ),
        (
            "commentaire tabulation",
            with(id, |f| f.comment = Some("a\tb".into())),
        ),
        (
            "commentaire retour chariot isolé",
            with(id, |f| f.comment = Some("a\rb".into())),
        ),
        (
            "commentaire échappement",
            with(id, |f| f.comment = Some("a\u{1b}[0m".into())),
        ),
        (
            "pièce inexistante",
            Command::SetPieceCard {
                piece: PieceId(987_654),
                name: "x".into(),
                code: None,
                quantity: 1,
                mirrored_pair: false,
                material: None,
                category: None,
                comment: None,
            },
        ),
    ];
    for (what, cmd) in refusals {
        assert!(doc.execute(cmd).is_err(), "{what} aurait dû être refusé");
        assert_eq!(doc.project(), &before, "{what} : projet modifié");
        assert_eq!(
            persist::to_json(doc.project()).unwrap(),
            text_before,
            "{what} : octets différents après refus"
        );
    }
    // Aucune étape d'annulation ajoutée par les refus : un seul undo (la fiche complète).
    assert!(doc.undo());
    assert!(!doc.can_undo());
    // Limites exactes acceptées (borne incluse), caractères Unicode comptés en caractères.
    let ok_code = "é".repeat(MAX_NAME_LEN);
    let ok_comment = "ü\n".repeat(MAX_COMMENT_LEN / 2 - 1) + "fi";
    assert_eq!(ok_comment.chars().count(), MAX_COMMENT_LEN);
    let mut p = doc.project().clone();
    p.apply(with(id, |f| {
        f.code = Some(ok_code.clone());
        f.comment = Some(ok_comment.clone());
        f.quantity = u32::MAX;
    }))
    .unwrap();
    assert_eq!(p.piece(id).unwrap().quantity, u32::MAX);
    let back = persist::from_json(&persist::to_json(&p).unwrap()).unwrap();
    assert_eq!(back, p);
}

fn with(id: PieceId, f: impl FnOnce(&mut Card)) -> Command {
    let mut c = Card::blank();
    f(&mut c);
    c.command(id)
}

#[test]
fn ca2_sans_changement_aucune_etape_et_vide_efface() {
    let (p, _, id) = with_piece();
    let mut doc = Document::new(p);
    doc.execute(full_card(id)).unwrap();
    assert!(doc.undo());
    assert!(!doc.can_undo());
    doc.execute(full_card(id)).unwrap();
    // Même commande une seconde fois : aucun changement, donc aucune étape.
    let before = doc.project().clone();
    doc.execute(full_card(id)).unwrap();
    assert_eq!(doc.project(), &before);
    assert!(doc.undo());
    assert!(!doc.can_undo(), "le no-op n'a pas créé d'étape");
    doc.redo();
    // Code vide ou blanc = effacé ; idem matière et commentaire.
    doc.execute(with(id, |f| {
        f.code = Some("   ".into());
        f.material = Some(String::new());
        f.comment = Some(" \n ".into());
        f.name = "Devant".into();
        f.category = None;
    }))
    .unwrap();
    let q = doc.project().piece(id).unwrap();
    assert_eq!(q.code, None);
    assert_eq!(q.material, None);
    assert_eq!(q.comment, None);
    assert_eq!(q.category, None);
    let v = to_value(doc.project());
    for key in ["code", "material", "category", "comment"] {
        assert!(v["pieces"][0].get(key).is_none(), "{key} omis quand vide");
    }
    // Fins de ligne Windows ramenées à « \n ».
    doc.execute(with(id, |f| f.comment = Some("a\r\nb".into())))
        .unwrap();
    assert_eq!(
        doc.project().piece(id).unwrap().comment.as_deref(),
        Some("a\nb")
    );
}

#[test]
fn ca2_set_piece_info_conserve_les_champs_de_la_fiche() {
    let (mut p, _, id) = with_piece();
    p.apply(full_card(id)).unwrap();
    p.apply(Command::SetPieceInfo {
        piece: id,
        name: "Autre".into(),
        quantity: 3,
        material: None,
        mirrored_pair: false,
    })
    .unwrap();
    let q = p.piece(id).unwrap();
    assert_eq!(q.name, "Autre");
    assert_eq!(q.code.as_deref(), Some("DV-01"));
    assert_eq!(q.category, Some(PieceCategory::Interfacing));
    assert!(q.comment.is_some());
    // La commande se sérialise et se relit à l'identique.
    let c = full_card(id);
    let back: Command = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back, c);
}

// ----- CA3 : points d'attention ---------------------------------------------------

#[test]
fn ca3_droit_fil_manquant_signale_puis_assigne_pas_une_erreur() {
    let (mut p, sheet, id) = with_piece();
    assert_eq!(p.piece_issues(id), vec![PieceIssue::MissingGrainline]);
    // Valide et sauvegardable malgré l'avertissement.
    p.validate().unwrap();
    let text = persist::to_json(&p).unwrap();
    assert_eq!(persist::from_json(&text).unwrap(), p);
    // Assigner un droit-fil : l'avertissement disparaît.
    let g = add_grainline(&mut p, sheet);
    let outline = p.piece(id).unwrap().outline.clone();
    p.apply(Command::SetPieceOutline {
        piece: id,
        outline,
        grainline: Some(g),
    })
    .unwrap();
    assert!(p.piece_issues(id).is_empty());
    assert!(p.piece_card(id).unwrap().has_grainline);
    // Supprimer le tracé du droit-fil : la pièce garde la référence ou non, mais jamais
    // de référence cassée acceptée par la validation.
    p.validate().unwrap();
}

#[test]
fn ca3_contour_vide_ouvert_illisible_et_auto_intersection() {
    // Pièce-étiquette : pas de contour.
    let mut p = Project::new("Étiquette");
    let sheet = p.sheets[0].id;
    let id = p
        .apply(Command::AddPiece {
            sheet,
            name: "Étiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    assert_eq!(
        p.piece_issues(id),
        vec![PieceIssue::MissingGrainline, PieceIssue::EmptyOutline]
    );
    p.validate().unwrap();

    // Contour ouvert : trois côtés d'un rectangle.
    let (mut q, _, id) = with_piece();
    let mut outline = q.piece(id).unwrap().outline.clone();
    let last = outline.pop().unwrap();
    q.apply(Command::SetPieceOutline {
        piece: id,
        outline,
        grainline: None,
    })
    .unwrap();
    let issues = q.piece_issues(id);
    assert!(issues.contains(&PieceIssue::OutlineOpen), "{issues:?}");
    q.validate().unwrap();
    persist::to_json(&q).unwrap();
    let _ = last;

    // Contour illisible : un tracé supprimé en douce du modèle (hors commande).
    let (mut r, _, id) = with_piece();
    r.pieces[0].outline[0].entity = EntityId(777_777);
    assert_eq!(r.piece_issues(id).len(), 2);
    assert!(r.piece_issues(id).contains(&PieceIssue::OutlineUnreadable));

    // Contour qui se recoupe : papillon A(0,0) B(100,100) C(100,0) D(0,100).
    let mut b = Project::new("Papillon");
    let sheet = b.sheets[0].id;
    let mut edges = Vec::new();
    for (from, to) in [
        (pt(0.0, 0.0), pt(100.0, 100.0)),
        (pt(100.0, 100.0), pt(100.0, 0.0)),
        (pt(100.0, 0.0), pt(0.0, 100.0)),
        (pt(0.0, 100.0), pt(0.0, 0.0)),
    ] {
        let e = b
            .apply(Command::AddSegment {
                sheet,
                from: PointRef::New(from),
                to: PointRef::New(to),
            })
            .unwrap()
            .entities[0];
        edges.push(PieceEdge {
            entity: e,
            reversed: false,
        });
    }
    let id = b
        .apply(Command::AddPiece {
            sheet,
            name: "Papillon".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    b.apply(Command::SetPieceOutline {
        piece: id,
        outline: edges,
        grainline: None,
    })
    .unwrap();
    assert!(b
        .piece_issues(id)
        .contains(&PieceIssue::OutlineSelfIntersecting));
    b.validate().unwrap();
}

#[test]
fn ca3_droit_fil_inexistant_et_nom_vide_signales_hors_validation() {
    let (mut p, _, id) = with_piece();
    // Hors commande (la validation refuserait ces états) : la fonction pure les signale.
    p.pieces[0].grainline = Some(EntityId(424_242));
    p.pieces[0].name = "  ".into();
    let issues = p.piece_issues(id);
    assert!(
        issues.contains(&PieceIssue::GrainlineMissingEntity),
        "{issues:?}"
    );
    assert!(issues.contains(&PieceIssue::NoName), "{issues:?}");
    assert!(!issues.contains(&PieceIssue::MissingGrainline));
    assert!(
        p.validate().is_err(),
        "la validation reste stricte sur les références"
    );
    // Identifiant inconnu : liste vide, pas de panique.
    assert!(p.piece_issues(PieceId(1)).is_empty());
}

#[test]
fn ca3_code_duplique_signale_et_non_refuse() {
    let (mut p, sheet, a) = with_piece();
    let b = p
        .apply(Command::AddPiece {
            sheet,
            name: "Dos".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    let set_code = |id, code: Option<&str>| Command::SetPieceCard {
        piece: id,
        name: "P".into(),
        code: code.map(String::from),
        quantity: 1,
        mirrored_pair: false,
        material: None,
        category: None,
        comment: None,
    };
    p.apply(set_code(a, Some("A1"))).unwrap();
    assert!(!p
        .piece_issues(a)
        .iter()
        .any(|i| matches!(i, PieceIssue::DuplicateCode { .. })));
    // Même code (espaces rognés) : accepté, signalé sur les deux pièces.
    p.apply(set_code(b, Some(" A1 "))).unwrap();
    for id in [a, b] {
        assert!(
            p.piece_issues(id)
                .contains(&PieceIssue::DuplicateCode { code: "A1".into() }),
            "{id:?}"
        );
    }
    // Sauvegardable.
    let text = persist::to_json(&p).unwrap();
    assert_eq!(persist::from_json(&text).unwrap(), p);
    // Codes différents, ou absents (deux pièces sans code ne sont pas des doublons).
    p.apply(set_code(b, Some("B2"))).unwrap();
    p.apply(set_code(a, None)).unwrap();
    p.apply(set_code(b, None)).unwrap();
    for id in [a, b] {
        assert!(!p
            .piece_issues(id)
            .iter()
            .any(|i| matches!(i, PieceIssue::DuplicateCode { .. })));
    }
    // Clés stables, indépendantes de la langue.
    let keys: Vec<&str> = [
        PieceIssue::NoName,
        PieceIssue::MissingGrainline,
        PieceIssue::GrainlineMissingEntity,
        PieceIssue::EmptyOutline,
        PieceIssue::OutlineOpen,
        PieceIssue::OutlineUnreadable,
        PieceIssue::OutlineSelfIntersecting,
        PieceIssue::DuplicateCode { code: "x".into() },
    ]
    .iter()
    .map(PieceIssue::key)
    .collect();
    assert_eq!(
        keys,
        [
            "no_name",
            "missing_grainline",
            "grainline_missing_entity",
            "empty_outline",
            "outline_open",
            "outline_unreadable",
            "outline_self_intersecting",
            "duplicate_code"
        ]
    );
}

// ----- CA4 : fiche en lecture seule pour les exports ------------------------------

#[test]
fn ca4_piece_card_valeurs_et_mesures_coherentes_avec_le_contour() {
    let (mut p, _, id) = with_piece();
    p.apply(full_card(id)).unwrap();
    let c = p.piece_card(id).unwrap();
    let m = p.piece_contour(id).unwrap();
    assert_eq!(c.id, id);
    assert_eq!(c.name, "Devant gauche");
    assert_eq!(c.code.as_deref(), Some("DV-01"));
    assert_eq!(c.quantity, 2);
    assert!(c.mirrored_pair);
    assert_eq!(c.material.as_deref(), Some("Drap de laine"));
    assert_eq!(c.category, Some(PieceCategory::Interfacing));
    assert_eq!(
        c.comment.as_deref(),
        Some("Couper dans le sens\ndu droit-fil")
    );
    assert!(!c.has_grainline);
    assert_eq!(c.issues, vec![PieceIssue::MissingGrainline]);
    assert_eq!(c.area_mm2, Some(m.area_mm2));
    assert_eq!(c.perimeter_mm, Some(m.perimeter_mm));
    assert!((c.area_mm2.unwrap() - 6000.0).abs() < 1e-6);
    assert!((c.perimeter_mm.unwrap() - 320.0).abs() < 1e-6);
    // La fiche suit la construction (mesures observées, pas stockées).
    let mut q = p.clone();
    q.apply(Command::AddPiece {
        sheet: q.sheets[0].id,
        name: "Étiquette".into(),
        quantity: 1,
        material: None,
        mirrored_pair: false,
    })
    .unwrap();
    let tag = q.pieces[1].id;
    let t = q.piece_card(tag).unwrap();
    assert_eq!((t.area_mm2, t.perimeter_mm), (None, None));
    assert_eq!(t.code, None);
    assert_eq!(t.category, None);
    assert!(matches!(
        q.piece_card(PieceId(5)),
        Err(CoreError::UnknownPiece(_))
    ));
    // Lecture pure : rien n'a changé dans le projet.
    let before = p.clone();
    let _ = p.piece_card(id).unwrap();
    assert_eq!(p, before);
}

// ----- Rétrocompatibilité, copies, presse-papiers ----------------------------------

#[test]
fn compat_fixtures_v1_v2_ouvertes_comme_avant_sans_champs_de_fiche() {
    for name in FIXTURES {
        let p = fixture(name);
        p.validate().unwrap();
        for piece in &p.pieces {
            assert!(piece.code.is_none() && piece.category.is_none() && piece.comment.is_none());
            assert!(piece.extra.is_empty(), "{name}");
            let card = p.piece_card(piece.id).unwrap();
            assert_eq!(card.name, piece.name);
            assert_eq!(card.quantity, piece.quantity);
        }
        let v: Value = serde_json::from_str(&persist::to_json(&p).unwrap()).unwrap();
        assert_eq!(v["schema_version"], 6);
    }
}

#[test]
fn copie_de_feuille_emporte_la_fiche_et_signale_le_code_duplique() {
    let (mut p, sheet, id) = with_piece();
    p.apply(full_card(id)).unwrap();
    p.apply(Command::DuplicateSheet { sheet }).unwrap();
    assert_eq!(p.pieces.len(), 2);
    let copy = &p.pieces[1];
    assert_eq!(copy.code.as_deref(), Some("DV-01"));
    assert_eq!(copy.category, Some(PieceCategory::Interfacing));
    assert_eq!(copy.comment, p.pieces[0].comment);
    // Même code sur les deux pièces : signalé, pas refusé.
    for piece in &p.pieces {
        assert!(p
            .piece_issues(piece.id)
            .iter()
            .any(|i| matches!(i, PieceIssue::DuplicateCode { .. })));
    }
    p.validate().unwrap();
}

#[test]
fn presse_papiers_ne_transporte_pas_les_pieces() {
    // Le fragment copié ne contient que des tracés : coller ne crée aucune pièce
    // (et donc aucune fiche) ; documenté dans docs/interface-contract.md.
    let (mut p, sheet, id) = with_piece();
    p.apply(full_card(id)).unwrap();
    let entities: Vec<EntityId> = p
        .sheet(sheet)
        .unwrap()
        .entities
        .iter()
        .map(|e| e.id)
        .collect();
    let fragment = p.copy_fragment(sheet, &[], &entities, &[], &[]).unwrap();
    p.apply(Command::PasteFragment {
        sheet,
        fragment: Box::new(fragment),
        layer: None,
    })
    .unwrap();
    assert_eq!(p.pieces.len(), 1);
    assert_eq!(p.pieces[0].code.as_deref(), Some("DV-01"));
}

//! US-11.4 (tranche 129) — bibliothèque de bases : enregistrer une pièce, un bloc ou un modèle,
//! l'insérer avec ses variables. Valeurs attendues calculées à la main (commentaires).
//!
//! Projet source (`fixture`) : rectangle 170 × 120 de (0 ; 0), 4 points, 4 côtés ; droit-fil de
//! (85 ; 20) à (85 ; 100) : 2 points, 1 tracé ; 3 crans ; 2 cotations ; plus un point sur le
//! premier côté à la distance `L` du départ (formule maintenue) : 7 points, 5 tracés.

mod bibliotheque_support;

use bibliotheque_support::*;
use pf_core::persist;
use pf_core::*;
use serde_json::Value;

fn save_piece(lib: &Library, f: &Fx, name: &str) -> LibraryEntry {
    lib.save(name, f.p.library_piece(f.piece).unwrap()).unwrap()
}

fn var(p: &Project, name: &str) -> Option<Variable> {
    p.variables.iter().find(|v| v.name == name).cloned()
}

// --- Enregistrer ------------------------------------------------------------------------------

#[test]
fn enregistrer_une_piece_comptes_et_fichier() {
    let t = Tmp::new("piece");
    let lib = Library::new(t.path());
    let f = fixture("50");
    let e = save_piece(&lib, &f, "Rabat cuir");
    assert_eq!(e.id, 1);
    assert!(t.file(1).is_file(), "lib-000001.pforge-lib.json");
    let c = &e.content;
    assert_eq!(c.kind, LibraryKind::Piece);
    let s = &c.sheets[0];
    // 4 coins + 2 points de droit-fil + 1 point à L = 7 points ; 4 côtés + 1 droit-fil = 5 tracés.
    assert_eq!((s.points.len(), s.entities.len()), (7, 5));
    assert_eq!((s.notches.len(), s.dimensions.len()), (3, 2));
    assert_eq!(s.formulas.len(), 1);
    assert_eq!(c.variables.len(), 1);
    assert_eq!(c.variables[0].name, "L");
    assert_eq!(c.variables[0].expr, "50");
    let p = &c.pieces[0];
    assert_eq!(p.name, "Rabat");
    assert_eq!(p.quantity, 2);
    assert_eq!(p.material.as_deref(), Some("Cuir"));
    assert!(p.grainline.is_some());
    assert_eq!(p.outline.len(), 4);
    // Relecture : identique à ce qui a été écrit.
    assert_eq!(lib.load(1).unwrap(), e);
    assert_eq!(lib.list().unwrap().entries, vec![e]);
}

#[test]
fn enregistrer_ne_touche_pas_au_projet_ni_au_format() {
    let t = Tmp::new("format");
    let lib = Library::new(t.path());
    let f = fixture("50");
    let before = bytes(&f.p);
    save_piece(&lib, &f, "A");
    assert_eq!(bytes(&f.p), before);
    let json: Value = serde_json::from_str(&before).unwrap();
    assert_eq!(json["schema_version"], 6);
    assert!(!before.contains("library"));
    // Le fichier de bibliothèque est un autre format, avec sa propre version.
    let file: Value = serde_json::from_str(&std::fs::read_to_string(t.file(1)).unwrap()).unwrap();
    assert_eq!(file["format"], "patternforge-library");
    assert_eq!(file["version"], 1);
    assert_eq!(file["id"], 1);
    assert_eq!(file["name"], "A");
}

#[test]
fn refus_nommes_du_nom_sans_rien_ecrire() {
    let t = Tmp::new("noms");
    let lib = Library::new(t.path());
    let f = fixture("50");
    let c = f.p.library_piece(f.piece).unwrap();
    assert_eq!(
        lib.save("", c.clone()).unwrap_err(),
        LibraryError::NameEmpty
    );
    assert_eq!(
        lib.save("  \t ", c.clone()).unwrap_err(),
        LibraryError::NameEmpty
    );
    assert_eq!(
        lib.save("a\u{7}b", c.clone()).unwrap_err(),
        LibraryError::NameControl
    );
    assert_eq!(
        lib.save("a\nb", c.clone()).unwrap_err(),
        LibraryError::NameControl
    );
    let long = "x".repeat(81);
    assert_eq!(
        lib.save(&long, c.clone()).unwrap_err(),
        LibraryError::NameTooLong { max: 80 }
    );
    assert!(!t.path().exists() || lib.list().unwrap().entries.is_empty());
    // 80 caractères : accepté (limite incluse), espaces de bord retirés.
    let ok = "y".repeat(80);
    let e = lib.save(&format!("  {ok}  "), c.clone()).unwrap();
    assert_eq!(e.name, ok);
    // Doublon : casse ignorée, après rognage.
    assert_eq!(
        lib.save(&ok.to_uppercase(), c.clone()).unwrap_err(),
        LibraryError::NameDuplicate {
            name: ok.to_uppercase()
        }
    );
    assert_eq!(lib.list().unwrap().entries.len(), 1);
}

// --- Insérer une pièce ------------------------------------------------------------------------

#[test]
fn inserer_une_piece_nouveaux_ids_une_etape_copie_independante() {
    let t = Tmp::new("ins");
    let lib = Library::new(t.path());
    let f = fixture("50");
    let entry = save_piece(&lib, &f, "Rabat");
    let source_ids = project_ids(&f.p, &[f.sheet]);

    // Cible dont le compteur dépasse ceux de la source : aucun recouvrement fortuit de numéros.
    let mut base = target(None);
    for i in 0..80 {
        base.apply(Command::CreateSheet {
            name: format!("F{i}"),
        })
        .unwrap();
    }
    let next_before = base.next_id;
    assert!(next_before > f.p.next_id);
    let mut doc = Document::new(base);
    let before = bytes(doc.project());
    let out = doc.execute(insert(&entry.content)).unwrap();
    let p = doc.project();
    // Feuille par défaut + 80 + feuille insérée.
    assert_eq!(p.sheets.len(), 82);
    let sid = out.sheet.unwrap();
    let s = p.sheet(sid).unwrap();
    assert_eq!(s.name, "Rabat");
    assert_eq!((s.points.len(), s.entities.len()), (7, 5));
    assert_eq!((s.notches.len(), s.dimensions.len()), (3, 2));
    assert_eq!(p.pieces.len(), 1);
    let pc = &p.pieces[0];
    assert_eq!(pc.link, PieceLink::Linked { sheet: sid });
    assert_eq!(pc.material.as_deref(), Some("Cuir"));
    assert_eq!(pc.quantity, 2);
    assert_eq!(pc.outline.len(), 4);
    assert!(pc.grainline.is_some());
    // Aucun identifiant de la source ni de l'entrée n'est conservé.
    let new_ids = project_ids(p, &[sid]);
    assert!(new_ids.is_disjoint(&source_ids), "ids recopiés");
    let entry_ids = project_ids(&f.p, &[f.sheet]);
    assert!(new_ids.is_disjoint(&entry_ids));
    assert!(
        new_ids.iter().all(|i| *i >= next_before),
        "ids pris au compteur du projet"
    );
    // La géométrie est celle de l'entrée : le coin haut-droit est à (170 ; 120).
    let corners: Vec<(f64, f64)> = s.points.iter().map(|q| pos(p, sid, q.id)).collect();
    assert!(corners.contains(&(170.0, 120.0)));
    // Une seule étape annulable, exacte à l'octet.
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.undo());
    assert!(doc.redo());
    assert_eq!(doc.project().sheets.len(), 82);
}

#[test]
fn inserer_deux_fois_donne_deux_copies_independantes() {
    let t = Tmp::new("deux");
    let lib = Library::new(t.path());
    let f = fixture("50");
    let entry = save_piece(&lib, &f, "Rabat");
    let mut p = target(None);
    let a = p.apply(insert(&entry.content)).unwrap();
    let b = p.apply(insert(&entry.content)).unwrap();
    // Variable réutilisée (même valeur) : une seule.
    assert_eq!(p.variables.len(), 1);
    assert_eq!(p.sheets.len(), 3);
    let (sa, sb) = (a.sheet.unwrap(), b.sheet.unwrap());
    assert_ne!(p.sheet(sa).unwrap().name, p.sheet(sb).unwrap().name);
    assert_eq!(p.pieces.len(), 2);
    assert_ne!(p.pieces[0].id, p.pieces[1].id);
    assert!(project_ids(&p, &[sa]).is_disjoint(&project_ids(&p, &[sb])));
    // Modifier une copie ne touche ni l'autre ni l'entrée.
    let corner = p.sheet(sa).unwrap().points[0].id;
    let other_before = pos(&p, sb, p.sheet(sb).unwrap().points[0].id);
    p.apply(Command::MovePoint {
        sheet: sa,
        point: corner,
        to: pf_geometry::PointMm::new(999.0, 888.0),
    })
    .unwrap();
    assert_eq!(pos(&p, sb, p.sheet(sb).unwrap().points[0].id), other_before);
    assert_eq!(lib.load(entry.id).unwrap(), entry);
}

#[test]
fn piece_riche_marge_et_marquage_suivent_les_nouveaux_objets() {
    let t = Tmp::new("riche");
    let lib = Library::new(t.path());
    let mut f = fixture("50");
    f.p.apply(Command::SetSeamAllowance {
        piece: f.piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            edges: vec![EdgeAllowance {
                entity: f.base,
                start_mm: 15.0,
                end_mm: 15.0,
            }],
            hems: Vec::new(),
            folds: Vec::new(),
            corners: Vec::new(),
            max_corner_mm: None,
        }),
    })
    .unwrap();
    f.p.apply(Command::AddPieceMarking {
        piece: f.piece,
        at: MarkingAnchor::Point { point: f.point },
        shape: MarkingShape::Drill { diameter_mm: 4.0 },
    })
    .unwrap();
    // Coupe de la source : [-10 ; 180] x [-15 ; 130] = 190 x 145 = 27550 mm², périmètre 670.
    let cut = f.p.piece_cut_contour(f.piece).unwrap();
    assert!((cut.area_mm2 - 27550.0).abs() < 1e-6);
    let entry = save_piece(&lib, &f, "Rabat marges");
    let mut p = target(None);
    p.apply(insert(&entry.content)).unwrap();
    let pc = &p.pieces[0];
    let seam = pc.seam.as_ref().unwrap();
    assert_eq!(seam.default_mm, 10.0);
    // La surcharge pointe un tracé de la nouvelle feuille, pas de la source.
    let sid = match pc.link {
        PieceLink::Linked { sheet } => sheet,
        PieceLink::Detached => panic!("liée"),
    };
    let sheet = p.sheet(sid).unwrap();
    assert!(sheet.entity(seam.edges[0].entity).is_ok());
    let MarkingAnchor::Point { point } = pc.markings[0].at else {
        panic!("ancrage point")
    };
    assert!(sheet.point(point).is_ok());
    assert_eq!(pos(&p, sid, point), (50.0, 0.0));
    let cut2 = p.piece_cut_contour(pc.id).unwrap();
    assert!((cut2.area_mm2 - 27550.0).abs() < 1e-6);
    assert!((cut2.perimeter_mm - cut.perimeter_mm).abs() < 1e-9);
}

// --- Variables --------------------------------------------------------------------------------

#[test]
fn variable_absente_copiee_et_formule_maintenue() {
    let t = Tmp::new("var");
    let lib = Library::new(t.path());
    let entry = save_piece(&lib, &fixture("50"), "Rabat");
    let mut p = target(None);
    let out = p.apply(insert(&entry.content)).unwrap();
    let v = var(&p, "L").unwrap();
    assert_eq!((v.expr.as_str(), v.unit), ("50", VariableUnit::Mm));
    // Le repère est à 50 mm du départ du premier côté, qui part de (0 ; 0) vers (170 ; 0).
    let sid = out.sheet.unwrap();
    let sheet = p.sheet(sid).unwrap();
    assert_eq!(sheet.formulas.len(), 1);
    let point = sheet.formulas[0].point;
    assert_eq!(pos(&p, sid, point), (50.0, 0.0));
    // Maintenue : la variable du projet déplace le repère à 80 mm.
    p.apply(Command::SetVariableExpr {
        name: "L".into(),
        expr: "80".into(),
    })
    .unwrap();
    assert_eq!(pos(&p, sid, point), (80.0, 0.0));
}

#[test]
fn variable_identique_reutilisee_meme_si_le_texte_differe() {
    let t = Tmp::new("same");
    let lib = Library::new(t.path());
    let entry = save_piece(&lib, &fixture("50"), "Rabat");
    let mut p = target(Some("25*2"));
    assert!(p.library_conflicts(&entry.content).unwrap().is_empty());
    p.apply(insert(&entry.content)).unwrap();
    // 25 × 2 = 50 : la variable du projet est gardée telle quelle.
    assert_eq!(p.variables.len(), 1);
    assert_eq!(var(&p, "L").unwrap().expr, "25*2");
}

#[test]
fn variable_de_valeur_differente_refus_nomme_sans_trace() {
    let t = Tmp::new("conf");
    let lib = Library::new(t.path());
    let entry = save_piece(&lib, &fixture("50"), "Rabat");
    let mut p = target(Some("60"));
    let before = bytes(&p);
    let conflicts = p.library_conflicts(&entry.content).unwrap();
    assert_eq!(
        conflicts,
        vec![VariableConflict {
            name: "L".into(),
            existing_name: "L".into(),
            incoming_value: 50.0,
            existing_value: 60.0
        }]
    );
    let err = p.apply(insert(&entry.content)).unwrap_err().to_string();
    assert!(
        err.contains("« L »") && err.contains("50") && err.contains("60"),
        "{err}"
    );
    assert_eq!(bytes(&p), before, "refus sans trace");
}

#[test]
fn conflit_d_unite_et_de_casse() {
    let t = Tmp::new("unit");
    let lib = Library::new(t.path());
    let entry = save_piece(&lib, &fixture("50"), "Rabat");
    // Même valeur 50 mais unité « degrés » : conflit.
    let mut p = Project::new("Cible");
    p.apply(Command::AddVariable {
        name: "L".into(),
        unit: VariableUnit::Deg,
        expr: "50".into(),
    })
    .unwrap();
    assert_eq!(p.library_conflicts(&entry.content).unwrap().len(), 1);
    assert!(p.apply(insert(&entry.content)).is_err());
    // Même valeur, nom qui ne diffère que par la casse : conflit (les noms sont uniques sans casse).
    let mut q = Project::new("Cible");
    q.apply(Command::AddVariable {
        name: "l".into(),
        unit: VariableUnit::Mm,
        expr: "50".into(),
    })
    .unwrap();
    let c = q.library_conflicts(&entry.content).unwrap();
    assert_eq!(
        (c[0].name.as_str(), c[0].existing_name.as_str()),
        ("L", "l")
    );
    assert!(q.apply(insert(&entry.content)).is_err());
}

#[test]
fn option_renommer_insere_sous_un_nom_libre() {
    let t = Tmp::new("ren");
    let lib = Library::new(t.path());
    let entry = save_piece(&lib, &fixture("50"), "Rabat");
    let mut p = target(Some("60"));
    // « L_2 » déjà pris : le premier nom libre est « L_3 ».
    p.apply(Command::AddVariable {
        name: "L_2".into(),
        unit: VariableUnit::Mm,
        expr: "1".into(),
    })
    .unwrap();
    let out = p
        .apply(Command::InsertLibrary {
            content: Box::new(entry.content.clone()),
            sheet: None,
            layer: None,
            rename_variables: true,
        })
        .unwrap();
    assert_eq!(
        var(&p, "L").unwrap().expr,
        "60",
        "la variable du projet est intacte"
    );
    assert_eq!(var(&p, "L_3").unwrap().expr, "50");
    assert!(out.notes.iter().any(|n| n.contains("« L_3 »")));
    let sid = out.sheet.unwrap();
    let sheet = p.sheet(sid).unwrap();
    assert_eq!(sheet.formulas[0].expr, "L_3");
    assert_eq!(pos(&p, sid, sheet.formulas[0].point), (50.0, 0.0));
    // Le renommage suit : L_3 = 20 déplace le repère à 20 mm, L (60) n'y est pour rien.
    p.apply(Command::SetVariableExpr {
        name: "L_3".into(),
        expr: "20".into(),
    })
    .unwrap();
    assert_eq!(
        pos(&p, sid, p.sheet(sid).unwrap().formulas[0].point),
        (20.0, 0.0)
    );
}

#[test]
fn variable_qui_en_cite_une_autre_les_deux_sont_copiees() {
    let t = Tmp::new("chaine");
    let lib = Library::new(t.path());
    let mut f = fixture("50");
    // M = L * 2 ; la formule du repère cite M : L doit suivre, sans que le repère la cite.
    f.p.apply(Command::AddVariable {
        name: "M".into(),
        unit: VariableUnit::Mm,
        expr: "L*2".into(),
    })
    .unwrap();
    f.p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet: f.sheet,
            point: f.point,
            param: ParamKind::SegmentDistance,
        },
        expr: Some("M".into()),
    })
    .unwrap();
    // Variable inutile : n'est pas copiée.
    f.p.apply(Command::AddVariable {
        name: "Z".into(),
        unit: VariableUnit::Mm,
        expr: "7".into(),
    })
    .unwrap();
    let c = f.p.library_piece(f.piece).unwrap();
    assert_eq!(c.variable_names(), vec!["L".to_string(), "M".to_string()]);
    let entry = lib.save("Rabat", c).unwrap();
    let mut p = target(None);
    let out = p.apply(insert(&entry.content)).unwrap();
    assert_eq!(var(&p, "M").unwrap().expr, "L*2");
    assert!(var(&p, "Z").is_none());
    // M = 100 : le repère est à 100 mm.
    let sid = out.sheet.unwrap();
    let pt = p.sheet(sid).unwrap().formulas[0].point;
    assert_eq!(pos(&p, sid, pt), (100.0, 0.0));
}

// --- Bloc et modèle ---------------------------------------------------------------------------

#[test]
fn bloc_insere_dans_une_feuille_existante_avec_decalage_au_second_collage() {
    let t = Tmp::new("bloc");
    let lib = Library::new(t.path());
    let f = fixture("50");
    // Sélection : le premier côté et le repère construit dessus.
    let c =
        f.p.library_block(f.sheet, &[f.point], &[f.base], &[])
            .unwrap();
    assert_eq!(c.kind, LibraryKind::Block);
    assert_eq!(c.sheets[0].points.len(), 3);
    assert_eq!(c.sheets[0].formulas.len(), 1);
    assert_eq!(c.variable_names(), vec!["L".to_string()]);
    let entry = lib.save("Côté et repère", c).unwrap();

    let mut p = target(None);
    let s = p.sheets[0].id;
    let cmd = |p: &mut Project| {
        p.apply(Command::InsertLibrary {
            content: Box::new(entry.content.clone()),
            sheet: Some(s),
            layer: None,
            rename_variables: false,
        })
        .unwrap()
    };
    let o1 = cmd(&mut p);
    assert_eq!(p.sheets.len(), 1, "pas de nouvelle feuille");
    let sheet = p.sheet(s).unwrap();
    assert_eq!((sheet.points.len(), sheet.entities.len()), (3, 1));
    // Extrémités (0 ; 0) et (170 ; 0), repère à 50 mm.
    let relative = sheet.formulas[0].point;
    assert_eq!(pos(&p, s, relative), (50.0, 0.0));
    assert_eq!(o1.points.len(), 3);
    // Deuxième collage : les points libres tombent sur les premiers, décalage de 5 mm sur x et y.
    cmd(&mut p);
    let sheet = p.sheet(s).unwrap();
    assert_eq!((sheet.points.len(), sheet.entities.len()), (6, 2));
    assert_eq!(sheet.formulas.len(), 2);
    let second = sheet.formulas[1].point;
    assert_eq!(pos(&p, s, second), (55.0, 5.0));
    assert_eq!(var(&p, "L").unwrap().expr, "50");
}

#[test]
fn bloc_sans_feuille_cible_et_piece_avec_feuille_refuses() {
    let t = Tmp::new("cible");
    let lib = Library::new(t.path());
    let f = fixture("50");
    let block = lib
        .save(
            "B",
            f.p.library_block(f.sheet, &[], &[f.base], &[]).unwrap(),
        )
        .unwrap();
    let piece = save_piece(&lib, &f, "P");
    let mut p = target(None);
    let before = bytes(&p);
    assert!(p.apply(insert(&block.content)).is_err());
    let s = p.sheets[0].id;
    let bad = Command::InsertLibrary {
        content: Box::new(piece.content.clone()),
        sheet: Some(s),
        layer: None,
        rename_variables: false,
    };
    assert!(p.apply(bad).is_err());
    let bad_layer = Command::InsertLibrary {
        content: Box::new(piece.content),
        sheet: None,
        layer: Some(LayerId(1)),
        rename_variables: false,
    };
    assert!(p.apply(bad_layer).is_err());
    assert_eq!(bytes(&p), before);
}

#[test]
fn modele_complet_feuilles_pieces_variables() {
    let t = Tmp::new("modele");
    let lib = Library::new(t.path());
    let mut f = fixture("50");
    f.p.apply(Command::CreateSheet {
        name: "Annexe".into(),
    })
    .unwrap();
    let annexe = f.p.sheets[1].id;
    f.p.apply(Command::AddSegment {
        sheet: annexe,
        from: PointRef::New(pf_geometry::PointMm::new(0.0, 0.0)),
        to: PointRef::New(pf_geometry::PointMm::new(30.0, 40.0)),
    })
    .unwrap();
    let c = f.p.library_model().unwrap();
    assert_eq!(c.kind, LibraryKind::Model);
    assert_eq!(
        (c.sheets.len(), c.pieces.len(), c.variables.len()),
        (2, 1, 1)
    );
    let entry = lib.save("Tout", c).unwrap();
    let mut p = target(None);
    let mut doc = Document::new(p.clone());
    let out = doc.execute(insert(&entry.content)).unwrap();
    p = doc.project().clone();
    // Feuille par défaut + 2 feuilles du modèle ; la pièce suit sa feuille.
    assert_eq!(p.sheets.len(), 3);
    assert_eq!(p.pieces.len(), 1);
    assert_eq!(out.pieces.len(), 1);
    let names: Vec<&str> = p.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(&names[1..], ["Rabat", "Annexe"]);
    assert_eq!(p.sheets[2].entities.len(), 1);
    assert!(doc.undo() && !doc.undo(), "une seule étape");
}

#[test]
fn modele_dit_ce_qui_n_est_pas_repris() {
    let mut f = fixture("50");
    f.p.apply(Command::AddVariant {
        name: "Chemise".into(),
    })
    .unwrap();
    let c = f.p.library_model().unwrap();
    assert!(
        c.notes.iter().any(|n| n == "1 variante(s) non repris."),
        "{:?}",
        c.notes
    );
    // Insérée : les remarques reviennent à l'utilisatrice, aucune variante n'apparaît.
    let mut p = target(None);
    let out = p.apply(insert(&c)).unwrap();
    assert!(out.notes.iter().any(|n| n.contains("variante")));
    assert!(p.variants.is_empty());
}

#[test]
fn bloc_dont_le_parent_n_est_pas_choisi_fige_le_point_et_le_dit() {
    let f = fixture("50");
    // Le repère seul : son tracé parent n'est pas dans la sélection, il est figé en (50 ; 0).
    let c = f.p.library_block(f.sheet, &[f.point], &[], &[]).unwrap();
    assert_eq!(c.sheets[0].points.len(), 1);
    assert!(c.sheets[0].formulas.is_empty());
    assert!(c.variables.is_empty(), "pas de formule, pas de variable");
    assert!(
        c.notes
            .iter()
            .any(|n| n.starts_with("1 formule(s) non reprise(s)")),
        "{:?}",
        c.notes
    );
    let mut p = target(None);
    let s = p.sheets[0].id;
    p.apply(Command::InsertLibrary {
        content: Box::new(c),
        sheet: Some(s),
        layer: None,
        rename_variables: false,
    })
    .unwrap();
    assert!(p.variables.is_empty());
    let sheet = p.sheet(s).unwrap();
    assert!(matches!(
        sheet.points[0].definition,
        PointDefinition::Free { .. }
    ));
    assert_eq!(pos(&p, s, sheet.points[0].id), (50.0, 0.0));
    // Une pièce dont les formules sont reprises ne dit pas qu'elles sont figées.
    let piece = f.p.library_piece(f.piece).unwrap();
    assert!(
        piece.notes.iter().all(|n| !n.contains("formule")),
        "{:?}",
        piece.notes
    );
}

// --- Gestion de la bibliothèque ---------------------------------------------------------------

#[test]
fn lister_renommer_supprimer_identifiants_stables() {
    let t = Tmp::new("gestion");
    let lib = Library::new(t.path());
    let f = fixture("50");
    let a = save_piece(&lib, &f, "A");
    let b = save_piece(&lib, &f, "B");
    assert_eq!((a.id, b.id), (1, 2));
    let r = lib.rename(1, "  Alpha ").unwrap();
    assert_eq!((r.id, r.name.as_str()), (1, "Alpha"));
    assert_eq!(r.content, a.content, "contenu inchangé");
    assert!(t.file(1).is_file(), "le fichier garde son nom");
    // Refus : doublon (casse ignorée), nom vide, entrée inconnue.
    assert_eq!(
        lib.rename(2, "ALPHA").unwrap_err(),
        LibraryError::NameDuplicate {
            name: "ALPHA".into()
        }
    );
    assert_eq!(lib.rename(2, "").unwrap_err(), LibraryError::NameEmpty);
    assert_eq!(
        lib.rename(9, "Z").unwrap_err(),
        LibraryError::NotFound { id: 9 }
    );
    // Renommer en soi-même (autre casse) est permis.
    assert_eq!(lib.rename(2, "b").unwrap().name, "b");
    let names: Vec<String> = lib
        .list()
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["Alpha", "b"]);
    lib.delete(1).unwrap();
    assert!(!t.file(1).exists());
    assert_eq!(lib.delete(1).unwrap_err(), LibraryError::NotFound { id: 1 });
    // Le nom libéré peut servir de nouveau ; l'entrée 2 est intacte.
    let c = save_piece(&lib, &f, "Alpha");
    assert_eq!(c.id, 3);
    assert_eq!(lib.load(2).unwrap().name, "b");
}

#[test]
fn dossier_absent_est_une_bibliotheque_vide() {
    let t = Tmp::new("absent");
    let lib = Library::new(t.path().join("nulle-part"));
    let l = lib.list().unwrap();
    assert!(l.entries.is_empty() && l.problems.is_empty());
    assert_eq!(lib.load(1).unwrap_err(), LibraryError::NotFound { id: 1 });
}

// --- Fichiers corrompus -----------------------------------------------------------------------

fn corrupt_cases(good: &str) -> Vec<(&'static str, Vec<u8>)> {
    let mut cases: Vec<(&'static str, Vec<u8>)> = vec![
        ("tronque", good.as_bytes()[..good.len() / 2].to_vec()),
        ("vide", Vec::new()),
        ("utf8", vec![0xff, 0xfe, 0x00, 0xc3]),
        ("texte", b"pas du json".to_vec()),
    ];
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut v: Value = serde_json::from_str(good).unwrap();
        f(&mut v);
        serde_json::to_vec(&v).unwrap()
    };
    cases.push((
        "champ inconnu",
        edit(&|v| {
            v["intrus"] = Value::from(1);
        }),
    ));
    cases.push((
        "champ inconnu imbrique",
        edit(&|v| {
            v["content"]["variables"][0]["intrus"] = Value::from(1);
        }),
    ));
    cases.push((
        "champ inconnu de feuille",
        edit(&|v| {
            v["content"]["sheets"][0]["intrus"] = Value::from(1);
        }),
    ));
    cases.push((
        "version",
        edit(&|v| {
            v["version"] = Value::from(2);
        }),
    ));
    cases.push((
        "format",
        edit(&|v| {
            v["format"] = Value::from("autre");
        }),
    ));
    cases.push((
        "id",
        edit(&|v| {
            v["id"] = Value::from(7);
        }),
    ));
    cases.push((
        "nom vide",
        edit(&|v| {
            v["name"] = Value::from("");
        }),
    ));
    cases.push((
        "type",
        edit(&|v| {
            v["content"]["kind"] = Value::from("modele");
        }),
    ));
    cases.push((
        "reference morte",
        edit(&|v| {
            // Retire un tracé : le contour de la pièce le cite encore.
            v["content"]["sheets"][0]["entities"]
                .as_array_mut()
                .unwrap()
                .remove(0);
        }),
    ));
    cases.push((
        "variable manquante",
        edit(&|v| {
            v["content"]["variables"] = Value::Array(Vec::new());
        }),
    ));
    cases.push((
        "id en double",
        edit(&|v| {
            let id = v["content"]["sheets"][0]["points"][0]["id"].clone();
            v["content"]["sheets"][0]["points"][1]["id"] = id;
        }),
    ));
    cases.push((
        "compteur trop bas",
        edit(&|v| {
            v["content"]["next_id"] = Value::from(3);
        }),
    ));
    let dup = good.replacen("\"version\": 1,", "\"version\": 1,\n  \"version\": 1,", 1);
    assert_ne!(dup, good);
    cases.push(("cle en double", dup.into_bytes()));
    cases
}

#[test]
fn fichiers_corrompus_listes_jamais_touches_jamais_ecrases() {
    let t = Tmp::new("corrompus");
    let lib = Library::new(t.path());
    let f = fixture("50");
    let good = save_piece(&lib, &f, "Bonne");
    let text = std::fs::read_to_string(t.file(1)).unwrap();
    let cases = corrupt_cases(&text);
    assert!(cases.len() >= 15);
    for (name, data) in cases {
        let _ = std::fs::remove_dir_all(t.path());
        let lib = Library::new(t.path());
        lib.save("Bonne", good.content.clone()).unwrap();
        // Le fichier 2 est abîmé ; le 1 reste bon.
        std::fs::write(t.file(2), &data).unwrap();
        let l = lib.list().unwrap();
        assert_eq!(l.entries.len(), 1, "{name} : l'entrée 1 reste lisible");
        assert_eq!(l.problems.len(), 1, "{name} : listé comme problème");
        assert_eq!(l.problems[0].file, "lib-000002.pforge-lib.json", "{name}");
        assert!(!l.problems[0].message.is_empty(), "{name}");
        assert!(
            matches!(lib.load(2), Err(LibraryError::Corrupt { .. })),
            "{name}"
        );
        // Ni renommé, ni supprimé, ni écrasé.
        assert!(lib.rename(2, "X").is_err(), "{name}");
        assert!(lib.delete(2).is_err(), "{name}");
        let new = lib.save("Nouvelle", good.content.clone()).unwrap();
        assert_eq!(
            new.id, 3,
            "{name} : l'identifiant du fichier abîmé est réservé"
        );
        assert_eq!(
            std::fs::read(t.file(2)).unwrap(),
            data,
            "{name} : octets intacts"
        );
    }
}

#[test]
fn fichiers_etrangers_temporaires_et_noms_non_canoniques() {
    let t = Tmp::new("etrangers");
    let lib = Library::new(t.path());
    let f = fixture("50");
    save_piece(&lib, &f, "A");
    std::fs::write(t.path().join("notes.txt"), "x").unwrap();
    std::fs::write(t.path().join("lib-000001.pforge-lib.json.tmp123"), "x").unwrap();
    std::fs::write(t.path().join("lib-1.pforge-lib.json"), "{}").unwrap();
    std::fs::write(t.path().join("autre.pforge-lib.json"), "{}").unwrap();
    let l = lib.list().unwrap();
    assert_eq!(l.entries.len(), 1);
    let mut files: Vec<String> = l.problems.iter().map(|p| p.file.clone()).collect();
    files.sort();
    assert_eq!(files, ["autre.pforge-lib.json", "lib-1.pforge-lib.json"]);
}

#[test]
fn contenu_falsifie_en_memoire_refuse_sans_trace() {
    let t = Tmp::new("falsifie");
    let lib = Library::new(t.path());
    let entry = save_piece(&lib, &fixture("50"), "Rabat");
    let mut p = target(None);
    let before = bytes(&p);
    // Référence manquante : le contour cite un tracé retiré.
    let mut c = entry.content.clone();
    c.sheets[0].entities.remove(0);
    let e = p.apply(insert(&c)).unwrap_err();
    assert!(
        e.to_string().contains("bibliothèque") || e.to_string().contains("référence"),
        "{e}"
    );
    assert_eq!(bytes(&p), before);
    // Pièce liée à aucune feuille de l'entrée.
    let mut c = entry.content.clone();
    c.pieces[0].link = PieceLink::Detached;
    assert!(p.apply(insert(&c)).is_err());
    // Type incohérent : une « pièce » sans pièce.
    let mut c = entry.content.clone();
    c.pieces.clear();
    assert!(p.apply(insert(&c)).is_err());
    // Variable citée par une formule mais absente de l'entrée.
    let mut c = entry.content.clone();
    c.variables.clear();
    assert!(p.apply(insert(&c)).is_err());
    assert_eq!(bytes(&p), before);
}

#[test]
fn le_projet_reste_valide_et_relisible_apres_insertion() {
    let t = Tmp::new("valide");
    let lib = Library::new(t.path());
    let entry = save_piece(&lib, &fixture("50"), "Rabat");
    let mut p = target(None);
    p.apply(insert(&entry.content)).unwrap();
    let json = bytes(&p);
    let back = persist::from_json(&json).unwrap();
    assert_eq!(bytes(&back), json);
    assert_eq!(back.schema_version, 6);
}

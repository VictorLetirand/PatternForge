//! US-03.1b — valeurs maintenues par une formule : calculs faits à la main, refus sans trace,
//! une seule étape annulable, délier, renommage, copies, ouverture d'un fichier, `pf-validate`.
//!
//! Paramètres retenus : distance et angle d'un point en direction (`Polar`), dx et dy d'un point
//! décalé (`Offset`). Les valeurs attendues sont écrites en commentaire (calcul à la main).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};

const TOL: f64 = 1e-9;

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn var(p: &mut Project, name: &str, expr: &str) {
    p.apply(Command::AddVariable {
        name: name.into(),
        unit: VariableUnit::Mm,
        expr: expr.into(),
    })
    .unwrap();
}

fn set_var(name: &str, expr: &str) -> Command {
    Command::SetVariableExpr {
        name: name.into(),
        expr: expr.into(),
    }
}

fn free(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: PointMm::new(x, y),
    })
    .unwrap()
    .points[0]
}

fn derived(p: &mut Project, s: SheetId, d: PointDefinition) -> PointId {
    p.apply(Command::AddPointAt {
        sheet: s,
        point: PointRef::Derived(d),
    })
    .unwrap()
    .points[0]
}

fn offset(p: &mut Project, s: SheetId, from: PointId, dx: f64, dy: f64) -> PointId {
    derived(
        p,
        s,
        PointDefinition::Offset {
            from,
            dx_mm: dx,
            dy_mm: dy,
        },
    )
}

fn polar(p: &mut Project, s: SheetId, from: PointId, d: f64, a: f64) -> PointId {
    derived(
        p,
        s,
        PointDefinition::Polar {
            from,
            distance_mm: d,
            angle_deg: a,
            reference: None,
        },
    )
}

fn target(s: SheetId, point: PointId, param: ParamKind) -> ParamTarget {
    ParamTarget {
        sheet: s,
        point,
        param,
    }
}

fn bind(s: SheetId, point: PointId, param: ParamKind, expr: &str) -> Command {
    Command::SetParameterExpr {
        target: target(s, point, param),
        expr: Some(expr.into()),
    }
}

fn unbind(s: SheetId, point: PointId, param: ParamKind) -> Command {
    Command::SetParameterExpr {
        target: target(s, point, param),
        expr: None,
    }
}

fn pos(p: &Project, s: SheetId, id: PointId) -> PointMm {
    p.sheet(s).unwrap().position(id).unwrap()
}

fn at(p: &Project, s: SheetId, id: PointId, x: f64, y: f64) {
    let q = pos(p, s, id);
    assert!(
        (q.x - x).abs() < TOL && (q.y - y).abs() < TOL,
        "position {q:?}, attendu ({x} ; {y})"
    );
}

fn value(p: &Project, s: SheetId, id: PointId, k: ParamKind) -> f64 {
    p.sheet(s).unwrap().parameter_value(id, k).unwrap()
}

/// Commande refusée : projet **identique à l'octet** ; renvoie le message.
fn refused(p: &mut Project, cmd: Command) -> String {
    let before = bytes(p);
    let before_p = p.clone();
    let e = p.apply(cmd).expect_err("devrait être refusée").to_string();
    assert_eq!(bytes(p), before, "trace laissée par un refus : {e}");
    assert_eq!(*p, before_p);
    e
}

/// A (0 ; 0), variable `L = 120`, B = A + (1 ; 0) lié à `L/2+10`.
fn base() -> (Project, SheetId, PointId, PointId) {
    let mut p = Project::new("Formules");
    let s = p.sheets[0].id;
    var(&mut p, "L", "120");
    let a = free(&mut p, s, 0.0, 0.0);
    let b = offset(&mut p, s, a, 1.0, 0.0);
    p.apply(bind(s, b, ParamKind::Dx, "L/2+10")).unwrap();
    (p, s, a, b)
}

#[test]
fn distance_selon_un_axe_calculee_a_la_main_et_maintenue_en_une_etape() {
    let (p, s, _a, b) = base();
    // L = 120 : 120 / 2 + 10 = 70.
    at(&p, s, b, 70.0, 0.0);
    assert_eq!(value(&p, s, b, ParamKind::Dx), 70.0);
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    // L = 200 : 200 / 2 + 10 = 110, dans la même étape que le changement de variable.
    doc.execute(set_var("L", "200")).unwrap();
    at(doc.project(), s, b, 110.0, 0.0);
    assert_eq!(value(doc.project(), s, b, ParamKind::Dx), 110.0);
    let after = bytes(doc.project());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before, "annuler = octets d'avant");
    at(doc.project(), s, b, 70.0, 0.0);
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
    // Une seule étape : un seul annuler suffit pour revenir à (70 ; 0).
    assert!(doc.undo());
    at(doc.project(), s, b, 70.0, 0.0);
}

#[test]
fn angle_et_distance_en_direction() {
    let mut p = Project::new("Direction");
    let s = p.sheets[0].id;
    var(&mut p, "a", "30");
    var(&mut p, "L", "100");
    let a = free(&mut p, s, 10.0, 20.0);
    let c = polar(&mut p, s, a, 50.0, 0.0);
    at(&p, s, c, 60.0, 20.0);
    // Angle 90 - a = 60° ; distance min(L ; 80) / 2 = 40.
    p.apply(bind(s, c, ParamKind::Angle, "90-a")).unwrap();
    p.apply(bind(s, c, ParamKind::Distance, "min(L;80)/2"))
        .unwrap();
    assert_eq!(value(&p, s, c, ParamKind::Angle), 60.0);
    assert_eq!(value(&p, s, c, ParamKind::Distance), 40.0);
    // (10 + 40 cos 60° ; 20 + 40 sin 60°) = (30 ; 20 + 34,641016151377546).
    at(&p, s, c, 30.0, 20.0 + 20.0 * 3f64.sqrt());
    // a = 45 : angle 45°, distance inchangée.
    p.apply(set_var("a", "45")).unwrap();
    assert_eq!(value(&p, s, c, ParamKind::Angle), 45.0);
    let k = 40.0 * std::f64::consts::FRAC_1_SQRT_2;
    at(&p, s, c, 10.0 + k, 20.0 + k);
    // L = 60 : min(60 ; 80) / 2 = 30.
    p.apply(set_var("L", "60")).unwrap();
    assert_eq!(value(&p, s, c, ParamKind::Distance), 30.0);
}

#[test]
fn dy_negatif_et_virgule_decimale() {
    let mut p = Project::new("Dy");
    let s = p.sheets[0].id;
    var(&mut p, "L", "120");
    let a = free(&mut p, s, 5.0, 5.0);
    let b = offset(&mut p, s, a, 3.0, 4.0);
    // dy = -L / 10 = -12 ; dx = 2,5 * 4 = 10 (virgule décimale).
    p.apply(bind(s, b, ParamKind::Dy, "-L/10")).unwrap();
    p.apply(bind(s, b, ParamKind::Dx, "2,5*4")).unwrap();
    at(&p, s, b, 15.0, -7.0);
    assert_eq!(
        p.parameter_formula(target(s, b, ParamKind::Dx))
            .unwrap()
            .expr,
        "2,5*4"
    );
}

#[test]
fn les_enfants_suivent_apres_leurs_parents() {
    // A (0 ; 0) → B = A + (L ; 0) → C = B + (0 ; L / 2) : l'ordre de création ne compte pas,
    // les positions se lisent des parents aux enfants.
    let mut p = Project::new("Chaîne");
    let s = p.sheets[0].id;
    var(&mut p, "L", "100");
    let a = free(&mut p, s, 0.0, 0.0);
    let b = offset(&mut p, s, a, 1.0, 0.0);
    let c = offset(&mut p, s, b, 0.0, 1.0);
    p.apply(bind(s, c, ParamKind::Dy, "L/2")).unwrap();
    p.apply(bind(s, b, ParamKind::Dx, "L")).unwrap();
    at(&p, s, c, 100.0, 50.0);
    p.apply(set_var("L", "40")).unwrap();
    at(&p, s, b, 40.0, 0.0);
    at(&p, s, c, 40.0, 20.0);
}

#[test]
fn refus_sans_trace_a_la_liaison() {
    let (mut p, s, a, b) = base();
    let c = polar(&mut p, s, a, 20.0, 0.0);
    // Cible inconnue.
    refused(&mut p, bind(s, PointId(999_999), ParamKind::Dx, "1"));
    refused(
        &mut p,
        Command::SetParameterExpr {
            target: target(SheetId(999_999), b, ParamKind::Dx),
            expr: Some("1".into()),
        },
    );
    // Paramètre que ce point n'a pas.
    let e = refused(&mut p, bind(s, c, ParamKind::Dx, "1"));
    assert!(e.contains("n'a pas ce paramètre"), "{e}");
    let e = refused(&mut p, bind(s, a, ParamKind::Dx, "1"));
    assert!(e.contains("n'a pas ce paramètre"), "{e}");
    // Expression invalide, vide, variable inconnue, valeur non finie.
    assert!(refused(&mut p, bind(s, b, ParamKind::Dy, "L/")).contains("formule"));
    assert!(refused(&mut p, bind(s, b, ParamKind::Dy, "   ")).contains("vide"));
    assert!(refused(&mut p, bind(s, b, ParamKind::Dy, "M+1")).contains("inconnue"));
    assert!(refused(&mut p, bind(s, b, ParamKind::Dy, "1/0")).contains("zéro"));
    assert!(refused(&mut p, bind(s, b, ParamKind::Dy, "sqrt(0-4)")).contains("formule"));
    assert!(refused(&mut p, bind(s, b, ParamKind::Dy, "1e3")).contains("formule"));
    // Géométrie dégénérée : une distance de 0 (réutilise le refus de la direction).
    refused(&mut p, bind(s, c, ParamKind::Distance, "L-120"));
    // Hors de l'espace de travail (1e6 mm).
    refused(&mut p, bind(s, b, ParamKind::Dy, "L*L*L"));
}

#[test]
fn refus_d_un_changement_de_variable_qui_casse_une_formule() {
    let (mut p, s, a, b) = base();
    var(&mut p, "D", "4");
    p.apply(bind(s, b, ParamKind::Dy, "100/D")).unwrap();
    let c = polar(&mut p, s, a, 30.0, 45.0);
    p.apply(bind(s, c, ParamKind::Distance, "L-100")).unwrap();
    at(&p, s, b, 70.0, 25.0);
    // D = 0 : division par zéro dans la formule liée ; le message nomme la cible.
    let e = refused(&mut p, set_var("D", "0"));
    assert!(e.contains("point B (dy)") && e.contains("zéro"), "{e}");
    // L = 100 : la distance de C devient 0 (géométrie dégénérée) ; le message nomme la cible.
    let e = refused(&mut p, set_var("L", "100"));
    assert!(e.contains("point C (distance)"), "{e}");
    // L = 90 : distance -10, refusée aussi.
    refused(&mut p, set_var("L", "90"));
    // Un changement légitime passe.
    p.apply(set_var("L", "150")).unwrap();
    assert_eq!(value(&p, s, c, ParamKind::Distance), 50.0);
    assert_eq!(value(&p, s, b, ParamKind::Dx), 85.0);
}

#[test]
fn suppression_d_une_variable_utilisee_par_une_formule_refusee() {
    let (mut p, s, _a, b) = base();
    let e = refused(&mut p, Command::RemoveVariable { name: "L".into() });
    assert!(e.contains("point B (dx)"), "{e}");
    // Une variable qui n'est citée par rien se supprime ; celle de la formule après déliage.
    var(&mut p, "Z", "1");
    p.apply(Command::RemoveVariable { name: "Z".into() })
        .unwrap();
    p.apply(unbind(s, b, ParamKind::Dx)).unwrap();
    p.apply(Command::RemoveVariable { name: "L".into() })
        .unwrap();
    assert!(p.variables.is_empty());
    at(&p, s, b, 70.0, 0.0);
}

#[test]
fn cycle_et_variable_inconnue_refuses_meme_avec_des_formules() {
    let (mut p, s, _a, b) = base();
    var(&mut p, "M", "L+1");
    // L = M : cycle L → M → L ; rien ne bouge.
    let e = refused(&mut p, set_var("L", "M"));
    assert!(e.contains("circulaire"), "{e}");
    let e = refused(&mut p, set_var("L", "inconnue*2"));
    assert!(e.contains("inconnue"), "{e}");
    at(&p, s, b, 70.0, 0.0);
}

#[test]
fn delier_garde_la_valeur_relier_reprend_la_formule() {
    let (mut p, s, _a, b) = base();
    let mut doc = Document::new(p.clone());
    let bound = bytes(&p);
    // Un déliage sans formule n'est pas une étape.
    doc.execute(unbind(s, b, ParamKind::Dy)).unwrap();
    assert!(!doc.can_undo());
    doc.execute(unbind(s, b, ParamKind::Dx)).unwrap();
    assert!(doc.can_undo());
    let free_bytes = bytes(doc.project());
    assert!(!free_bytes.contains("formulas"));
    // La valeur courante reste, figée : changer la variable ne bouge plus rien.
    doc.execute(set_var("L", "500")).unwrap();
    at(doc.project(), s, b, 70.0, 0.0);
    assert!(doc.project().sheet(s).unwrap().formulas.is_empty());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), free_bytes);
    assert!(doc.undo());
    assert_eq!(
        bytes(doc.project()),
        bound,
        "annuler le déliage rend la formule"
    );
    // Relier : la formule revient et suit.
    doc.execute(unbind(s, b, ParamKind::Dx)).unwrap();
    doc.execute(bind(s, b, ParamKind::Dx, "L/4")).unwrap();
    at(doc.project(), s, b, 30.0, 0.0);
    // Changer le texte d'une formule déjà liée la remplace (pas de doublon).
    doc.execute(bind(s, b, ParamKind::Dx, "L/3")).unwrap();
    at(doc.project(), s, b, 40.0, 0.0);
    assert_eq!(doc.project().sheet(s).unwrap().formulas.len(), 1);
    p = doc.project().clone();
    assert!(bytes(&p).contains("\"formulas\""));
}

#[test]
fn renommer_une_variable_reecrit_les_formules_liees() {
    let (mut p, s, _a, b) = base();
    let mut doc = Document::new(p.clone());
    let before = bytes(&p);
    doc.execute(Command::RenameVariable {
        from: "L".into(),
        to: "Largeur".into(),
    })
    .unwrap();
    assert_eq!(
        doc.project()
            .parameter_formula(target(s, b, ParamKind::Dx))
            .unwrap()
            .expr,
        "Largeur/2+10"
    );
    at(doc.project(), s, b, 70.0, 0.0);
    doc.execute(set_var("Largeur", "300")).unwrap();
    at(doc.project(), s, b, 160.0, 0.0);
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    // Renommer vers un nom pris : refusé sans trace.
    var(&mut p, "M", "1");
    refused(
        &mut p,
        Command::RenameVariable {
            from: "L".into(),
            to: "m".into(),
        },
    );
}

#[test]
fn une_valeur_modifiee_directement_delie_avec_remarque() {
    let (mut p, s, _a, b) = base();
    let out = p
        .apply(Command::SetPointValues {
            sheet: s,
            point: b,
            values: vec![33.0, 0.0],
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("déliée")),
        "{:?}",
        out.notes
    );
    assert!(p.sheet(s).unwrap().formulas.is_empty());
    at(&p, s, b, 33.0, 0.0);
    // Même valeur qu'avant : la formule est conservée, rien n'est délié.
    let (mut q, s, _a, b) = base();
    let out = q
        .apply(Command::SetPointValues {
            sheet: s,
            point: b,
            values: vec![70.0, 5.0],
        })
        .unwrap();
    assert!(out.notes.is_empty());
    assert_eq!(q.sheet(s).unwrap().formulas.len(), 1);
    at(&q, s, b, 70.0, 5.0);
}

#[test]
fn supprimer_le_point_retire_la_formule_avec_remarque() {
    let (mut p, s, _a, b) = base();
    let out = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![b],
            entities: vec![],
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("retirée")),
        "{:?}",
        out.notes
    );
    assert!(p.sheet(s).unwrap().formulas.is_empty());
    p.validate().unwrap();
}

#[test]
fn inverser_le_trace_de_reference_delie_l_angle_avec_remarque() {
    let mut p = Project::new("Référence");
    let s = p.sheets[0].id;
    var(&mut p, "a", "90");
    let o = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap();
    let (seg, p0) = (o.entities[0], o.points[0]);
    let c = derived(
        &mut p,
        s,
        PointDefinition::Polar {
            from: p0,
            distance_mm: 10.0,
            angle_deg: 0.0,
            reference: Some(seg),
        },
    );
    p.apply(bind(s, c, ParamKind::Angle, "a")).unwrap();
    at(&p, s, c, 0.0, 10.0);
    let out = p
        .apply(Command::ReversePath {
            sheet: s,
            entity: seg,
        })
        .unwrap();
    // L'angle relatif change de 180° : la valeur écrite n'est plus celle de la formule.
    assert!(
        out.notes.iter().any(|n| n.contains("déliée")),
        "{:?}",
        out.notes
    );
    assert!(p.sheet(s).unwrap().formulas.is_empty());
    at(&p, s, c, 0.0, 10.0);
    p.validate().unwrap();
}

#[test]
fn copie_de_feuille_le_lien_suit() {
    let (mut p, s, _a, b) = base();
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = out.sheet.unwrap();
    let cs = p.sheet(copy).unwrap();
    assert_eq!(cs.formulas.len(), 1);
    let nb = cs.formulas[0].point;
    assert_ne!(nb, b, "le lien vise le point de la copie");
    assert!(out.notes.iter().all(|n| !n.contains("formule")));
    p.apply(set_var("L", "200")).unwrap();
    at(&p, s, b, 110.0, 0.0);
    at(&p, copy, nb, 110.0, 0.0);
}

#[test]
fn feuille_importee_dans_un_projet_sans_la_variable_valeurs_figees_avec_remarque() {
    let (p, s, _a, _b) = base();
    let cmd = p.import_command(s).unwrap();
    let mut other = Project::new("Autre");
    let out = other.apply(cmd).unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("formule")),
        "{:?}",
        out.notes
    );
    let imported = out.sheet.unwrap();
    assert!(other.sheet(imported).unwrap().formulas.is_empty());
    // Les valeurs restent en nombres figés : la feuille est identique.
    let ib = other.sheet(imported).unwrap().points[1].id;
    at(&other, imported, ib, 70.0, 0.0);
    // Même variable, même valeur : le lien est repris.
    let mut same = Project::new("Même table");
    var(&mut same, "L", "120");
    let out = same.apply(p.import_command(s).unwrap()).unwrap();
    let imported = out.sheet.unwrap();
    assert_eq!(same.sheet(imported).unwrap().formulas.len(), 1);
    assert!(out.notes.iter().all(|n| !n.contains("formule")));
}

#[test]
fn presse_papiers_la_formule_n_est_pas_reprise_avec_remarque() {
    let (mut p, s, a, b) = base();
    let fragment = p.copy_fragment(s, &[a, b], &[], &[], &[]).unwrap();
    assert!(
        fragment.notes.iter().any(|n| n.contains("formule")),
        "{:?}",
        fragment.notes
    );
    let out = p
        .apply(Command::PasteFragment {
            sheet: s,
            fragment: Box::new(fragment),
            layer: None,
        })
        .unwrap();
    assert!(out.notes.iter().any(|n| n.contains("formule")));
    // Seul l'original reste maintenu.
    assert_eq!(p.sheet(s).unwrap().formulas.len(), 1);
    p.validate().unwrap();
}

// ---------------------------------------------------------------------------------------------
// Fichiers
// ---------------------------------------------------------------------------------------------

fn value_of(p: &Project) -> Value {
    serde_json::from_str(&bytes(p)).unwrap()
}

fn open_err(v: &Value) -> String {
    persist::from_json(&serde_json::to_string(v).unwrap())
        .expect_err("devrait être refusé")
        .to_string()
}

fn point_index(v: &Value, label: &str) -> usize {
    v["sheets"][0]["points"]
        .as_array()
        .unwrap()
        .iter()
        .position(|p| p["label"] == label)
        .unwrap()
}

#[test]
fn projet_sans_formule_octets_inchanges() {
    let mut p = Project::new("Sans");
    var(&mut p, "L", "120");
    let s = p.sheets[0].id;
    let a = free(&mut p, s, 0.0, 0.0);
    offset(&mut p, s, a, 5.0, 0.0);
    let text = bytes(&p);
    assert!(!text.contains("formulas"));
    assert_eq!(bytes(&persist::from_json(&text).unwrap()), text);
    // Le fichier du dépôt qui ignore les formules est relu et réécrit à l'identique.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/jupe-droite.pforge.json");
    let raw = std::fs::read_to_string(path).unwrap();
    assert!(!raw.contains("formulas"));
    assert_eq!(bytes(&persist::from_json(&raw).unwrap()), raw);
}

#[test]
fn ouverture_valide_et_aller_retour_exact() {
    let (p, s, _a, b) = base();
    let text = bytes(&p);
    assert!(text.contains("\"formulas\""));
    let q = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&q), text);
    assert_eq!(q, p);
    // Un lecteur qui ignore `formulas` voit la bonne valeur : elle est dans la définition du point.
    let v = value_of(&p);
    let i = point_index(&v, p.sheet(s).unwrap().label(b));
    assert_eq!(v["sheets"][0]["points"][i]["definition"]["dx_mm"], 70.0);
    assert_eq!(
        v["sheets"][0]["formulas"],
        json!([{ "point": b.0, "param": "dx", "expr": "L/2+10" }])
    );
}

#[test]
fn ouverture_chaque_invalidite_refusee_avec_message_clair() {
    let (p, _s, a, b) = base();
    let ok = value_of(&p);
    // Variable inexistante.
    let mut v = ok.clone();
    v["sheets"][0]["formulas"][0]["expr"] = json!("Z/2+10");
    let e = open_err(&v);
    assert!(e.contains("inconnue") && e.contains("point B (dx)"), "{e}");
    // Valeur écrite différente de la formule.
    let mut v = ok.clone();
    let i = point_index(&v, "B");
    v["sheets"][0]["points"][i]["definition"]["dx_mm"] = json!(71.0);
    let e = open_err(&v);
    assert!(e.contains("ne correspond pas"), "{e}");
    // Cycle entre variables (a → b → a) cité par la formule.
    let mut v = ok.clone();
    v["variables"] = json!([
        { "name": "L", "unit": "mm", "expr": "M" },
        { "name": "M", "unit": "mm", "expr": "L" }
    ]);
    let e = open_err(&v);
    assert!(e.contains("circulaire"), "{e}");
    // Expression invalide, vide, hors de portée.
    for bad in ["L/", "", "1e3"] {
        let mut v = ok.clone();
        v["sheets"][0]["formulas"][0]["expr"] = json!(bad);
        assert!(open_err(&v).contains("formule"), "« {bad} »");
    }
    // Paramètre qui n'existe pas sur ce point (dy → ok ; distance sur un décalage → non).
    let mut v = ok.clone();
    v["sheets"][0]["formulas"][0]["param"] = json!("distance");
    assert!(open_err(&v).contains("inexistant"));
    // Point inexistant, paramètre inconnu, clé inconnue, doublon.
    let mut v = ok.clone();
    v["sheets"][0]["formulas"][0]["point"] = json!(a.0 + b.0 + 1000);
    assert!(open_err(&v).contains("inexistant"));
    let mut v = ok.clone();
    v["sheets"][0]["formulas"][0]["param"] = json!("rayon");
    assert!(!open_err(&v).is_empty());
    let mut v = ok.clone();
    v["sheets"][0]["formulas"][0]["value"] = json!(70.0);
    assert!(!open_err(&v).is_empty());
    let mut v = ok.clone();
    let f = v["sheets"][0]["formulas"][0].clone();
    v["sheets"][0]["formulas"].as_array_mut().unwrap().push(f);
    assert!(open_err(&v).contains("deux formules"));
    let mut v = ok.clone();
    v["sheets"][0]["formulas"][0]
        .as_object_mut()
        .unwrap()
        .remove("expr");
    assert!(!open_err(&v).is_empty());
    // Tolérance : 5e-10 d'écart passe, 2e-9 non.
    let mut v = ok.clone();
    let ib = point_index(&v, "B");
    v["sheets"][0]["points"][ib]["definition"]["dx_mm"] = json!(70.0 + 5e-10);
    persist::from_json(&serde_json::to_string(&v).unwrap()).unwrap();
    v["sheets"][0]["points"][ib]["definition"]["dx_mm"] = json!(70.0 + 2e-9);
    assert!(open_err(&v).contains("ne correspond pas"));
}

fn validate_cli(text: &str, name: &str) -> Option<i32> {
    let dir = std::env::temp_dir().join(format!("pf-formules-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    let code = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&path)
        .output()
        .unwrap()
        .status
        .code();
    std::fs::remove_dir_all(&dir).ok();
    code
}

#[test]
fn pf_validate_code_0_puis_1_pour_une_formule_incoherente() {
    let (p, ..) = base();
    assert_eq!(validate_cli(&bytes(&p), "ok.pforge.json"), Some(0));
    let mut v = value_of(&p);
    v["sheets"][0]["formulas"][0]["expr"] = json!("Z");
    assert_eq!(
        validate_cli(&serde_json::to_string(&v).unwrap(), "inconnue.pforge.json"),
        Some(1)
    );
    let mut v = value_of(&p);
    let i = point_index(&v, "B");
    v["sheets"][0]["points"][i]["definition"]["dx_mm"] = json!(1.0);
    assert_eq!(
        validate_cli(&serde_json::to_string(&v).unwrap(), "valeur.pforge.json"),
        Some(1)
    );
    let mut v = value_of(&p);
    v["variables"] = json!([
        { "name": "L", "unit": "mm", "expr": "L+1" }
    ]);
    assert_eq!(
        validate_cli(&serde_json::to_string(&v).unwrap(), "cycle.pforge.json"),
        Some(1)
    );
}

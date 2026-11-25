//! Étapes de capture (`--demo --etape NOM`, PH-1) : chaque étape prépare un état déterministe
//! (projet de démonstration ou projet dédié, construit par commandes) puis l'application capture
//! la vraie fenêtre. **`STAGES` est la liste unique**, affichée par
//! `--liste-etapes` ; le test `stages_tests` la parcourt. Les étapes historiques sont
//! posées dans `App::with_context` ; les autres sont dans `apply` ci-dessous.

use crate::app::{App, Pending};
use crate::text::core_error;
use pf_core::{
    ButtonElement, Command, Document, InternalKind, InternalTarget, MarkingAnchor, MarkingShape,
    NamingCounter, NamingRuleSpec, NamingTarget, PieceSource, PointRef, Project,
};
use pf_geometry::PointMm;

/// Toutes les étapes déclarées, une par état d'interface (fenêtre, section, outil en cours).
pub const STAGES: &[&str] = &[
    // Outils et gestes en cours (barre d'outils, bulle de saisie, aide au geste).
    "segment",
    "perpendiculaire",
    "point",
    "accrochage",
    "rectangle",
    "cran",
    // Inspecteur : sélection d'un objet.
    "feuille",
    "trait",
    "courbe",
    "cercle",
    "point-libre",
    "point-construit",
    "deux-points",
    "angle",
    "plusieurs-traits",
    "cran-coupe",
    "cran-coin",
    "taille-valeurs",
    "bareme",
    "fiche-client",
    "fiche-client-taille",
    "retouches",
    "cote",
    "repere",
    // Fiche de pièce et sections.
    "fiche-riche",
    "marge",
    "marge-progressive",
    "coupe-nette",
    "sur-le-pli",
    "sans-droit-fil",
    "matieres",
    "coins",
    "ourlet",
    "rempli",
    // Fenêtres modales et flottantes.
    "transformer",
    "alignement",
    "assembler",
    "deplier",
    "pli",
    "pli-creux",
    "ampleur-saisie",
    "ampleur",
    "decoupe",
    "decoupe-posee",
    "construction-rapide",
    "percages-au-pas",
    "piqure",
    "controle-piqure",
    "zones",
    "coutures",
    "variables",
    "formule",
    "maintenu",
    "rectangle-maintenu",
    "formule-compas",
    "formule-segment",
    "formule-creation",
    "longueur",
    "egalite",
    "egalite-somme",
    "longueur-lisse",
    "diagnostic",
    "dependances",
    "tailles",
    "gradation",
    "gradation-report",
    "gradation-derivee",
    "mesures-tailles",
    "mesures-export",
    "nomenclature",
    "variantes",
    "versions",
    "imprimer",
    "projection",
    "fond-image",
    "plan-coupe",
    "plan-coupe-metrage",
    "plan-coupe-auto",
    "plan-coupe-auto-apres",
    "plan-coupe-auto-completer",
    "cuir-peau",
    "cuir-peau-auto",
    "bibliotheque",
    "nommage",
    "nid-tailles",
    "controle-forme",
    "export-gradue",
    "marche",
    "crans-couture",
    "raccords",
    "pince",
    "pince-transfert",
    "pince-coupe-fermee",
    "impression",
    "export",
    "etalonnage",
    "dxf-export",
    "hpgl-export",
    "rapport",
    "dxf-import",
    "dxf-construction",
    "recherche",
    "aide",
    "plan",
    // Boîtes de dialogue.
    "erreur",
    "enregistrer-avant-quitter",
    "import-feuille",
    "recuperation",
    "fichier-modifie",
    "fichier-ouvert-ailleurs",
    "journal-erreurs",
    // PH-6 : écrans sans étape jusque-là.
    "import-aama",
    "historique",
    "preferences",
    "espace-gradation",
    "glossaire",
    // US-16.3 / US-16.8 : raccourcis, focus clavier visible, interface à 200 %.
    "raccourcis",
    "focus-visible",
    "echelle-200",
    // US-12.4 : import SVG / PDF vectoriel.
    "import-vectoriel",
    // US-16.5 : vues, unités d'affichage (pouces fractionnaires), grille et règles.
    "vues",
    // US-16.5 (tranche 132) : plusieurs fenêtres d'outil dont les champs de longueur suivent
    // l'unité d'affichage (pouces fractionnaires).
    "unites-fenetres",
];

/// SVG de démonstration (US-12.4) : un gabarit 100 x 60 mm avec un rectangle, un cercle, un arc,
/// une Bézier, une ellipse, un groupe transformé, et de quoi remplir le rapport des éléments
/// ignorés (texte, image, dégradé).
fn vector_demo_svg() -> String {
    concat!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100mm\" height=\"60mm\" ",
        "viewBox=\"0 0 100 60\">",
        "<defs><linearGradient id=\"g\"/></defs>",
        "<rect x=\"5\" y=\"5\" width=\"40\" height=\"25\" rx=\"3\" style=\"fill:none\"/>",
        "<circle cx=\"70\" cy=\"18\" r=\"10\"/>",
        "<path d=\"M 5 40 L 30 40 A 10 10 0 0 1 40 50 C 50 55 60 35 80 45\"/>",
        "<ellipse cx=\"80\" cy=\"45\" rx=\"12\" ry=\"6\"/>",
        "<g transform=\"translate(10 5) scale(2)\"><polyline points=\"0,0 5,10 10,0\"/></g>",
        "<text x=\"5\" y=\"58\">gabarit</text><image href=\"fond.png\"/>",
        "</svg>"
    )
    .to_string()
}

/// DXF de type AAMA minimal : un bloc de pièce (contour 100 x 50 mm, un cran, un perçage, un nom).
fn aama_demo_text() -> String {
    let mut codes: Vec<(i32, String)> = Vec::new();
    let mut put = |c: i32, v: &str| codes.push((c, v.to_string()));
    for (c, v) in [
        (0, "SECTION"),
        (2, "HEADER"),
        (9, "$INSUNITS"),
        (70, "4"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "BLOCKS"),
        (0, "BLOCK"),
        (8, "0"),
        (2, "PIECE_1"),
        (70, "0"),
        (10, "0"),
        (20, "0"),
        (0, "POLYLINE"),
        (8, "1"),
        (66, "1"),
        (70, "1"),
    ] {
        put(c, v);
    }
    for (x, y) in [("0", "0"), ("100", "0"), ("100", "50"), ("0", "50")] {
        for (c, v) in [(0, "VERTEX"), (8, "1"), (10, x), (20, y)] {
            put(c, v);
        }
    }
    for (c, v) in [
        (0, "SEQEND"),
        (8, "1"),
        (0, "LINE"),
        (8, "4"),
        (10, "30"),
        (20, "0"),
        (11, "30"),
        (21, "5"),
        (0, "CIRCLE"),
        (8, "13"),
        (10, "50"),
        (20, "25"),
        (40, "3"),
        (0, "TEXT"),
        (8, "84"),
        (10, "0"),
        (20, "0"),
        (40, "4"),
        (1, "Piece Name: Manche"),
        (0, "ENDBLK"),
        (8, "0"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "INSERT"),
        (8, "0"),
        (2, "PIECE_1"),
        (10, "0"),
        (20, "0"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ] {
        put(c, v);
    }
    codes
        .iter()
        .map(|(c, v)| format!("{c:>3}\n{v}\n"))
        .collect()
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Pièce d'exemple avec toutes les sections de fiche remplies : marge, pli interne, perçage,
/// groupe de boutons, surpiqûre de bord (droit-fil et crans viennent du projet de base).
fn rich_project() -> Result<Project, pf_core::CoreError> {
    let mut p = pf_core::demo_piece_project()?;
    let (piece, sheet) = (p.pieces[0].id, p.sheets[0].id);
    let fold = p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(pt(10.0, 60.0)),
        to: PointRef::New(pt(160.0, 60.0)),
    })?;
    p.apply(Command::AddPieceInternal {
        piece,
        target: InternalTarget::Path {
            entity: fold.entities[0],
        },
        kind: InternalKind::Fold,
    })?;
    let drill = p.apply(Command::AddPoint {
        sheet,
        at: pt(50.0, 30.0),
    })?;
    p.apply(Command::AddPieceMarking {
        piece,
        at: MarkingAnchor::Point {
            point: drill.points[0],
        },
        shape: MarkingShape::Drill { diameter_mm: 6.0 },
    })?;
    let a = p.apply(Command::AddPoint {
        sheet,
        at: pt(30.0, 90.0),
    })?;
    let b = p.apply(Command::AddPoint {
        sheet,
        at: pt(140.0, 90.0),
    })?;
    p.apply(Command::AddButtonGroup {
        piece,
        from: a.points[0],
        to: b.points[0],
        count: 3,
        element: ButtonElement::Button { diameter_mm: 12.0 },
        edge_margin_mm: 10.0,
    })?;
    p.apply(Command::AddTopstitch {
        sheet,
        source: pf_core::TopstitchSource::Piece { piece },
        distance_mm: 5.0,
        side: pf_core::TopstitchSide::Inside,
    })?;
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(pf_core::SeamAllowance {
            default_mm: 10.0,
            ..Default::default()
        }),
    })?;
    Ok(p)
}

/// Projet de capture `--etape sans-droit-fil` (PH-5d) : la pièce d'exemple en cuir, droit-fil
/// supprimé puis pièce déclarée « sans droit-fil » (plus d'avertissement dans la fiche).
fn no_grain_project() -> Result<Project, pf_core::CoreError> {
    let mut p = pf_core::demo_piece_project()?;
    let (piece, sheet) = (p.pieces[0].id, p.sheets[0].id);
    let grain = p.pieces[0]
        .grainline
        .ok_or_else(|| pf_core::CoreError::Invalid("droit-fil d'exemple introuvable".into()))?;
    p.apply(Command::Delete {
        sheet,
        points: Vec::new(),
        entities: vec![grain],
    })?;
    p.apply(Command::SetPieceNoGrain {
        piece,
        no_grain: true,
    })?;
    Ok(p)
}

/// Projet de capture `--etape variables` (US-03.1a) : la démonstration et une table de variables
/// (mesures de patron, une dérivée en mm, un angle en degrés, un rapport sans unité).
fn variables_project() -> Result<Project, pf_core::CoreError> {
    use pf_core::VariableUnit::{Deg, Mm, Unitless};
    let mut p = pf_core::demo_project()?;
    for (name, unit, expr) in [
        ("poitrine", Mm, "920"),
        ("aisance", Mm, "40"),
        ("largeur_dos", Mm, "(poitrine+aisance)/4"),
        ("angle_pince", Deg, "12,5"),
        ("rapport", Unitless, "largeur_dos/poitrine"),
    ] {
        p.apply(Command::AddVariable {
            name: name.into(),
            unit,
            expr: expr.into(),
        })?;
    }
    Ok(p)
}

/// Projet de capture `--etape formule` (US-03.1b) : deux variables (`largeur`, `a`), un point en
/// direction dont la **distance** est maintenue par `largeur/2+10` (largeur = 120 : 70 mm) et dont
/// l'**angle** (30°) reste libre ; le point est choisi dans l'inspecteur. Retourne le projet et
/// le point en direction.
pub(crate) fn formula_project() -> Result<(Project, pf_core::PointId), pf_core::CoreError> {
    use pf_core::{ParamKind, ParamTarget, PointDefinition, VariableUnit};
    let mut p = Project::new("Formule");
    let sheet = p.sheets[0].id;
    for (name, unit, expr) in [
        ("largeur", VariableUnit::Mm, "120"),
        ("a", VariableUnit::Deg, "30"),
    ] {
        p.apply(Command::AddVariable {
            name: name.into(),
            unit,
            expr: expr.into(),
        })?;
    }
    let origin = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(0.0, 0.0),
        })?
        .points[0];
    let c = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Polar {
                from: origin,
                distance_mm: 50.0,
                angle_deg: 30.0,
                reference: None,
            }),
        })?
        .points[0];
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::Existing(origin),
        to: PointRef::Existing(c),
    })?;
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet,
            point: c,
            param: ParamKind::Distance,
        },
        expr: Some("largeur/2+10".into()),
    })?;
    Ok((p, c))
}

/// Projet de capture `--etape maintenu` (US-03.2) : un segment de 100 mm à 30° depuis A, une
/// perpendiculaire de 40 mm à son extrémité B et une parallèle de 30 mm, les deux premiers
/// **maintenus** (la parallèle reste non maintenue) ; A a ensuite été déplacé : B, C suivent.
/// Retourne le projet et le segment de 100 mm.
pub(crate) fn kept_project() -> Result<(Project, pf_core::EntityId), pf_core::CoreError> {
    use pf_geometry::Side;
    let mut p = Project::new("Construction maintenue");
    let sheet = p.sheets[0].id;
    let a = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(0.0, 0.0),
        })?
        .points[0];
    let ab = p.apply(Command::AddSegmentPolar {
        sheet,
        from: PointRef::Existing(a),
        length_mm: 100.0,
        angle_deg: 30.0,
    })?;
    let (b, seg) = (ab.points[0], ab.entities[0]);
    let bc = p.apply(Command::AddPerpendicular {
        sheet,
        reference: seg,
        from: PointRef::Existing(b),
        length_mm: 40.0,
        side: Side::Left,
    })?;
    p.apply(Command::AddParallel {
        sheet,
        reference: seg,
        distance_mm: 30.0,
        side: Side::Right,
    })?;
    for segment in [seg, bc.entities[0]] {
        p.apply(Command::SetConstructionKept {
            sheet,
            segment,
            kept: true,
        })?;
    }
    p.apply(Command::MovePoint {
        sheet,
        point: a,
        to: PointMm::new(20.0, 15.0),
    })?;
    Ok((p, seg))
}

/// Projet de capture `--etape rectangle-maintenu` (US-03.2b) : un rectangle de 100 x 60 mm créé
/// **maintenu** (`AddConstructedRectangle`) dont le coin a ensuite été déplacé en (30 ; 20) : les
/// trois autres coins l'ont suivi dans la même étape. Retourne le projet et le côté de départ.
pub(crate) fn kept_rectangle_project() -> Result<(Project, pf_core::EntityId), pf_core::CoreError> {
    let mut p = Project::new("Rectangle maintenu");
    let sheet = p.sheets[0].id;
    let o = p.apply(Command::AddConstructedRectangle {
        sheet,
        corner: PointRef::New(PointMm::new(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 60.0,
        kept: true,
    })?;
    p.apply(Command::MovePoint {
        sheet,
        point: o.points[0],
        to: PointMm::new(30.0, 20.0),
    })?;
    Ok((p, o.entities[0]))
}

/// Projet de capture `--etape formule-compas` (US-03.1 CA 3) : un point d'arcs de compas (centres
/// à 100 mm, rayons 60 et 80 mm : le point est en (36 ; 48)) dont le rayon A est maintenu par
/// la variable `r` (`r` = 60). Retourne le projet et le point.
pub(crate) fn compass_formula_project() -> Result<(Project, pf_core::PointId), pf_core::CoreError> {
    use pf_core::{AimMm, ParamKind, ParamTarget, PointDefinition, VariableUnit};
    let mut p = Project::new("Formule compas");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddVariable {
        name: "r".into(),
        unit: VariableUnit::Mm,
        expr: "60".into(),
    })?;
    let a = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(0.0, 0.0),
        })?
        .points[0];
    let b = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(100.0, 0.0),
        })?
        .points[0];
    let c = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Compass {
                center_a: a,
                center_b: b,
                radius_a_mm: 50.0,
                radius_b_mm: 80.0,
                near: AimMm { x: 36.0, y: 48.0 },
            }),
        })?
        .points[0];
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet,
            point: c,
            param: ParamKind::RadiusA,
        },
        expr: Some("r".into()),
    })?;
    Ok((p, c))
}

/// Projet de capture `--etape formule-segment` et `formule-creation` (US-03.1 CA 3, restes) :
/// segment de 200 mm, variables `L` = 120 et `a` = 30, point à `L/2+10` du départ (maintenu) et
/// point à la proportion `1/4` (maintenu). Retourne le projet, ces deux points et le départ du
/// segment (point libre, parent du point en direction de l'étape de création).
pub(crate) fn rest_formula_project() -> Result<
    (
        Project,
        pf_core::PointId,
        pf_core::PointId,
        pf_core::PointId,
    ),
    pf_core::CoreError,
> {
    use pf_core::{ParamKind, ParamTarget, PointDefinition, SegmentEnd, VariableUnit};
    let mut p = Project::new("Formules restes");
    let sheet = p.sheets[0].id;
    for (name, unit, expr) in [
        ("L", VariableUnit::Mm, "120"),
        ("a", VariableUnit::Deg, "30"),
    ] {
        p.apply(Command::AddVariable {
            name: name.into(),
            unit,
            expr: expr.into(),
        })?;
    }
    let o = p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(PointMm::new(0.0, 0.0)),
        to: PointRef::New(PointMm::new(200.0, 0.0)),
    })?;
    let (seg, start) = (o.entities[0], o.points[0]);
    let mut add = |definition| -> Result<pf_core::PointId, pf_core::CoreError> {
        Ok(p.apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(definition),
        })?
        .points[0])
    };
    let ps = add(PointDefinition::OnSegment {
        segment: seg,
        distance_mm: 10.0,
        origin: SegmentEnd::Start,
    })?;
    let pr = add(PointDefinition::OnSegmentRatio {
        segment: seg,
        ratio: 0.9,
    })?;
    for (point, param, text) in [
        (ps, ParamKind::SegmentDistance, "L/2+10"),
        (pr, ParamKind::SegmentRatio, "1/4"),
    ] {
        p.apply(Command::SetParameterExpr {
            target: ParamTarget {
                sheet,
                point,
                param,
            },
            expr: Some(text.into()),
        })?;
    }
    Ok((p, ps, pr, start))
}

/// Projet de capture `--etape longueur` (US-03.3) : une courbe de Bézier (tête de manche) dont la
/// longueur développée est **imposée** à 260 mm en faisant glisser l'extrémité B le long de 0°, puis
/// le point de départ A déplacé : B s'est réajusté dans la même étape. Retourne le projet et la courbe.
pub(crate) fn imposed_project() -> Result<(Project, pf_core::EntityId), pf_core::CoreError> {
    let mut p = Project::new("Sleeve cap");
    let sheet = p.sheets[0].id;
    let o = p.apply(Command::AddCurve {
        sheet,
        start: PointRef::New(PointMm::new(0.0, 0.0)),
        c1: PointMm::new(40.0, 90.0),
        c2: PointMm::new(150.0, 90.0),
        end: PointRef::New(PointMm::new(190.0, 0.0)),
    })?;
    let (a, b, curve) = (o.points[0], o.points[1], o.entities[0]);
    p.apply(Command::SetCurveLengthConstraint {
        sheet,
        curve,
        spec: Some(pf_core::LengthSpec {
            length_mm: 260.0,
            adjust: pf_core::LengthAdjust::Slide {
                point: b,
                angle_deg: 0.0,
            },
            reference: None,
        }),
    })?;
    p.apply(Command::MovePoint {
        sheet,
        point: a,
        to: PointMm::new(15.0, 10.0),
    })?;
    Ok((p, curve))
}

/// Projet de capture `--etape egalite` (US-03.4) : une courbe de Bézier (tête de manche) dont la
/// longueur est **égale à celle de l'emmanchure + 12 mm** (embu), l'extrémité B glissant le long de
/// 0°, puis l'emmanchure déplacée : la tête de manche s'est réajustée dans la même étape. Retourne
/// le projet et la courbe contrainte.
pub(crate) fn equality_project() -> Result<(Project, pf_core::EntityId), pf_core::CoreError> {
    let mut p = Project::new("Armhole ease");
    let sheet = p.sheets[0].id;
    let armhole = p.apply(Command::AddCurve {
        sheet,
        start: PointRef::New(PointMm::new(0.0, 0.0)),
        c1: PointMm::new(40.0, 90.0),
        c2: PointMm::new(150.0, 90.0),
        end: PointRef::New(PointMm::new(190.0, 0.0)),
    })?;
    let (a, armhole) = (armhole.points[0], armhole.entities[0]);
    let cap = p.apply(Command::AddCurve {
        sheet,
        start: PointRef::New(PointMm::new(0.0, -70.0)),
        c1: PointMm::new(40.0, 20.0),
        c2: PointMm::new(150.0, 20.0),
        end: PointRef::New(PointMm::new(190.0, -70.0)),
    })?;
    let (b, cap) = (cap.points[1], cap.entities[0]);
    p.apply(Command::SetCurveLengthConstraint {
        sheet,
        curve: cap,
        spec: Some(pf_core::LengthSpec {
            length_mm: 1.0,
            adjust: pf_core::LengthAdjust::Slide {
                point: b,
                angle_deg: 0.0,
            },
            reference: Some(pf_core::LengthRef {
                entity: armhole,
                offset_mm: 12.0,
            }),
        }),
    })?;
    p.apply(Command::MovePoint {
        sheet,
        point: a,
        to: PointMm::new(15.0, 10.0),
    })?;
    Ok((p, cap))
}

/// Projet de capture `--etape egalite-somme` (US-03.4b) : la tête de manche (feuille « Manche »)
/// est égale à la **somme** de l'emmanchure d'une **autre feuille** (« Corsage »), d'une bande de
/// la même feuille (60 mm) et d'un décalage maintenu par la **formule** `embu` (8 mm). Retourne le
/// projet et la courbe contrainte.
pub(crate) fn equality_sum_project() -> Result<(Project, pf_core::EntityId), pf_core::CoreError> {
    let mut p = Project::new("Sleeve cap sum");
    let sleeve = p.sheets[0].id;
    p.apply(Command::RenameSheet {
        sheet: sleeve,
        name: "Manche".into(),
    })?;
    let bodice = p
        .apply(Command::CreateSheet {
            name: "Corsage".into(),
        })?
        .sheet
        .ok_or_else(|| pf_core::CoreError::Invalid("sheet not created".into()))?;
    p.apply(Command::AddVariable {
        name: "embu".into(),
        unit: pf_core::VariableUnit::Mm,
        expr: "8".into(),
    })?;
    let armhole = p
        .apply(Command::AddCurve {
            sheet: bodice,
            start: PointRef::New(PointMm::new(0.0, 0.0)),
            c1: PointMm::new(40.0, 90.0),
            c2: PointMm::new(150.0, 90.0),
            end: PointRef::New(PointMm::new(190.0, 0.0)),
        })?
        .entities[0];
    let band = p
        .apply(Command::AddSegment {
            sheet: sleeve,
            from: PointRef::New(PointMm::new(0.0, 40.0)),
            to: PointRef::New(PointMm::new(60.0, 40.0)),
        })?
        .entities[0];
    let cap = p.apply(Command::AddCurve {
        sheet: sleeve,
        start: PointRef::New(PointMm::new(0.0, -70.0)),
        c1: PointMm::new(40.0, 20.0),
        c2: PointMm::new(150.0, 20.0),
        end: PointRef::New(PointMm::new(190.0, -70.0)),
    })?;
    let (b, cap) = (cap.points[1], cap.entities[0]);
    p.apply(Command::SetCurveLengthConstraint {
        sheet: sleeve,
        curve: cap,
        spec: Some(pf_core::LengthSpec {
            length_mm: 300.0,
            adjust: pf_core::LengthAdjust::Slide {
                point: b,
                angle_deg: 0.0,
            },
            reference: None,
        }),
    })?;
    p.apply(Command::SetCurveLengthEquality {
        sheet: sleeve,
        curve: cap,
        equality: Some(pf_core::LengthEquality {
            terms: vec![
                pf_core::LengthTerm {
                    entity: armhole,
                    sheet: Some(bodice),
                },
                pf_core::LengthTerm {
                    entity: band,
                    sheet: None,
                },
            ],
            offset_mm: 0.0,
            offset_expr: Some("embu".into()),
        }),
    })?;
    Ok((p, cap))
}

/// Projet de capture `--etape longueur-lisse` (US-03.3b) : une courbe lisse à quatre points de
/// passage (tête de manche) dont la longueur est **imposée par la formule « emmanchure + 12 »**
/// (embu), l'extrémité D glissant le long de 0° ; la variable `emmanchure` passe de 248 à 252 mm :
/// la courbe s'est re-résolue dans la même étape (264 mm). Retourne le projet et la courbe.
pub(crate) fn smooth_imposed_project() -> Result<(Project, pf_core::EntityId), pf_core::CoreError> {
    let mut p = Project::new("Smooth sleeve cap");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddVariable {
        name: "emmanchure".into(),
        unit: pf_core::VariableUnit::Mm,
        expr: "248".into(),
    })?;
    let o = p.apply(Command::AddSpline {
        sheet,
        points: vec![
            PointRef::New(PointMm::new(0.0, 0.0)),
            PointRef::New(PointMm::new(55.0, 52.0)),
            PointRef::New(PointMm::new(125.0, 55.0)),
            PointRef::New(PointMm::new(190.0, 0.0)),
        ],
    })?;
    let (d, curve) = (o.points[3], o.entities[0]);
    p.apply(Command::SetCurveLengthConstraint {
        sheet,
        curve,
        spec: Some(pf_core::LengthSpec {
            length_mm: 260.0,
            adjust: pf_core::LengthAdjust::Slide {
                point: d,
                angle_deg: 0.0,
            },
            reference: None,
        }),
    })?;
    p.apply(Command::SetCurveLengthExpr {
        sheet,
        curve,
        expr: Some("emmanchure + 12".into()),
    })?;
    p.apply(Command::SetVariableExpr {
        name: "emmanchure".into(),
        expr: "252".into(),
    })?;
    Ok((p, curve))
}

/// Projet de capture `--etape diagnostic` (US-03.5) : les quatre mécanismes de liens (formule,
/// construction maintenue, longueur imposée, égalité de longueurs), deux variables que rien ne
/// cite et un calque verrouillé qui porte le point suivi de la construction maintenue. Aucune
/// erreur : une commande ne laisse jamais de lien non résolu.
pub(crate) fn diagnostic_project() -> Result<Project, pf_core::CoreError> {
    use pf_core::{
        LengthAdjust, LengthRef, LengthSpec, ParamKind, ParamTarget, PointDefinition, VariableUnit,
    };
    let mut p = Project::new("Diagnostic");
    let sheet = p.sheets[0].id;
    for (name, expr) in [("width", "120"), ("ease", "12"), ("spare", "30")] {
        p.apply(Command::AddVariable {
            name: name.into(),
            unit: VariableUnit::Mm,
            expr: expr.into(),
        })?;
    }
    let origin = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(0.0, 0.0),
        })?
        .points[0];
    let c = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Polar {
                from: origin,
                distance_mm: 50.0,
                angle_deg: 30.0,
                reference: None,
            }),
        })?
        .points[0];
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::Existing(origin),
        to: PointRef::Existing(c),
    })?;
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet,
            point: c,
            param: ParamKind::Distance,
        },
        expr: Some("width/2+10".into()),
    })?;
    let a = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(0.0, -60.0),
        })?
        .points[0];
    let ab = p.apply(Command::AddSegmentPolar {
        sheet,
        from: PointRef::Existing(a),
        length_mm: 90.0,
        angle_deg: 0.0,
    })?;
    let (b, seg) = (ab.points[0], ab.entities[0]);
    p.apply(Command::SetConstructionKept {
        sheet,
        segment: seg,
        kept: true,
    })?;
    let mut curves = Vec::new();
    for base in [-150.0, -220.0, -290.0] {
        let o = p.apply(Command::AddCurve {
            sheet,
            start: PointRef::New(PointMm::new(0.0, base)),
            c1: PointMm::new(40.0, base + 90.0),
            c2: PointMm::new(150.0, base + 90.0),
            end: PointRef::New(PointMm::new(190.0, base)),
        })?;
        curves.push((o.points[1], o.entities[0]));
    }
    let slide = |point| LengthAdjust::Slide {
        point,
        angle_deg: 0.0,
    };
    p.apply(Command::SetCurveLengthConstraint {
        sheet,
        curve: curves[1].1,
        spec: Some(LengthSpec {
            length_mm: 1.0,
            adjust: slide(curves[1].0),
            reference: Some(LengthRef {
                entity: curves[0].1,
                offset_mm: 12.0,
            }),
        }),
    })?;
    p.apply(Command::SetCurveLengthConstraint {
        sheet,
        curve: curves[2].1,
        spec: Some(LengthSpec {
            length_mm: 300.0,
            adjust: slide(curves[2].0),
            reference: None,
        }),
    })?;
    p.apply(Command::AddLayer {
        sheet,
        name: "Locked".into(),
    })?;
    let layer = p.sheets[0]
        .layers
        .last()
        .map(|l| l.id)
        .ok_or_else(|| pf_core::CoreError::Invalid("calque absent".into()))?;
    p.apply(Command::SetObjectsLayer {
        sheet,
        points: vec![b],
        entities: Vec::new(),
        layer: Some(layer),
    })?;
    p.apply(Command::SetLayerLocked {
        sheet,
        layer,
        locked: true,
    })?;
    Ok(p)
}

/// Projet de capture `--etape dependances` (US-03.6) : une variable (`width`), un point `C` en
/// direction dont la distance suit `width/2+10`, le segment `AC`, un point `F` au milieu de ce
/// segment, un point `G` décalé de `F`, un point `H` décalé de `G` et une cotation `F`–`G`.
/// Retourne le projet et le point `F` (parents : `AC`, `A`, `C`, `width` ; dépendants : `G`, la
/// cotation, puis `H`).
pub(crate) fn dependances_project() -> Result<(Project, pf_core::PointId), pf_core::CoreError> {
    use pf_core::{ParamKind, ParamTarget, PointDefinition, VariableUnit};
    let mut p = Project::new("Dependencies");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddVariable {
        name: "width".into(),
        unit: VariableUnit::Mm,
        expr: "120".into(),
    })?;
    let a = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(0.0, 0.0),
        })?
        .points[0];
    let c = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Polar {
                from: a,
                distance_mm: 50.0,
                angle_deg: 30.0,
                reference: None,
            }),
        })?
        .points[0];
    let ac = p.apply(Command::AddSegment {
        sheet,
        from: PointRef::Existing(a),
        to: PointRef::Existing(c),
    })?;
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet,
            point: c,
            param: ParamKind::Distance,
        },
        expr: Some("width/2+10".into()),
    })?;
    let f = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::OnSegmentRatio {
                segment: ac.entities[0],
                ratio: 0.5,
            }),
        })?
        .points[0];
    let g = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Offset {
                from: f,
                dx_mm: 0.0,
                dy_mm: 25.0,
            }),
        })?
        .points[0];
    let h = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Offset {
                from: g,
                dx_mm: 30.0,
                dy_mm: 0.0,
            }),
        })?
        .points[0];
    p.apply(Command::AddDimension {
        sheet,
        from: f,
        to: g,
        offset_mm: 10.0,
    })?;
    let _ = h;
    Ok((p, f))
}

/// Projet de l'étape `tailles` (US-09.1) : un rectangle et deux gammes indépendantes (une
/// alphanumérique avec deux tailles de rupture, une numérique), base changée sur la première.
pub(crate) fn tailles_project() -> Result<Project, pf_core::CoreError> {
    use pf_core::SizeKind;
    let mut p = Project::new("Sizes");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(PointMm::new(0.0, 0.0)),
        width_mm: 200.0,
        height_mm: 120.0,
    })?;
    let labels = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    p.apply(Command::AddSizeRange {
        name: "Women".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels(&["XS", "S", "M", "L", "XL"]),
        base: 0,
    })?;
    p.apply(Command::AddSizeRange {
        name: "Height".into(),
        kind: SizeKind::Numeric,
        labels: labels(&["160", "166", "172"]),
        base: 1,
    })?;
    let women: Vec<_> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    p.apply(Command::SetSizeBreak {
        size: women[1],
        break_size: true,
    })?;
    p.apply(Command::SetSizeBreak {
        size: women[4],
        break_size: true,
    })?;
    p.apply(Command::SetBaseSize { size: women[2] })?;
    Ok(p)
}

/// Projet de l'étape `gradation` (US-09.2a) : un panneau rectangulaire (4 points libres), une
/// gamme XS S M L XL de base M, écarts cumulés sur les deux coins de droite ; renvoie aussi le
/// coin haut-droit (le point sélectionné) et la taille XL (la taille active de la capture).
pub(crate) fn gradation_project(
) -> Result<(Project, pf_core::PointId, pf_core::SizeId), pf_core::CoreError> {
    use pf_core::SizeKind;
    let mut p = Project::new("Grading");
    let sheet = p.sheets[0].id;
    let polygon = p.apply(Command::AddPolygon {
        sheet,
        vertices: [(0.0, 0.0), (400.0, 0.0), (400.0, 600.0), (0.0, 600.0)]
            .iter()
            .map(|&(x, y)| PointRef::New(PointMm::new(x, y)))
            .collect(),
        closed: true,
        role: pf_core::PathRole::Construction,
        layer: None,
    })?;
    let corners = polygon.points.clone();
    p.apply(Command::AddSizeRange {
        name: "Women".into(),
        kind: SizeKind::Alphanumeric,
        labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
        base: 2,
    })?;
    let sizes: Vec<_> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    // Hanche (coin bas-droit) : élargissement ; hauteur (coin haut-droit) : élargissement et
    // allongement. Écarts cumulés depuis la base M.
    for (point, rows) in [
        (
            corners[1],
            [(0, -8.0, 0.0), (1, -4.0, 0.0), (3, 4.0, 0.0), (4, 8.0, 0.0)],
        ),
        (
            corners[2],
            [
                (0, -8.0, -12.0),
                (1, -4.0, -6.0),
                (3, 4.0, 6.0),
                (4, 8.0, 12.0),
            ],
        ),
    ] {
        for (i, dx, dy) in rows {
            p.apply(Command::SetPointGrading {
                sheet,
                point,
                size: sizes[i],
                dx_mm: dx,
                dy_mm: dy,
            })?;
        }
    }
    Ok((p, corners[2], sizes[4]))
}

/// Projet de l'étape `gradation-derivee` (US-09.5a) : le panneau de `gradation_project`, devenu
/// une pièce à marge de couture de 12 mm avec un cran à distance fixe (150 mm depuis le départ du
/// bord droit) et un cran ancré au milieu du bord haut ; le coin haut-gauche reçoit la hauteur du
/// coin haut-droit, donc chaque taille est un rectangle. Renvoie le coin haut-droit (le point
/// sélectionné), la taille XL (taille active de la capture) et la pièce.
pub(crate) fn gradation_derivee_project(
) -> Result<(Project, pf_core::PointId, pf_core::SizeId, pf_core::PieceId), pf_core::CoreError> {
    let (mut p, corner, xl) = gradation_project()?;
    let sheet = p.sheets[0].id;
    let corners: Vec<pf_core::PointId> = p.sheets[0].points.iter().map(|q| q.id).collect();
    let sizes: Vec<_> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    for (i, dy) in [(0, -12.0), (1, -6.0), (3, 6.0), (4, 12.0)] {
        p.apply(Command::SetPointGrading {
            sheet,
            point: corners[3],
            size: sizes[i],
            dx_mm: 0.0,
            dy_mm: dy,
        })?;
    }
    let edges: Vec<pf_core::EntityId> = p.sheets[0].entities.iter().map(|e| e.id).collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: pf_core::PieceSource::Paths {
                entities: edges.clone(),
            },
            name: "Devant".into(),
            detached: false,
        })?
        .pieces[0];
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(pf_core::SeamAllowance {
            default_mm: 12.0,
            ..Default::default()
        }),
    })?;
    p.apply(Command::AddNotch {
        sheet,
        entity: edges[1],
        distance_mm: 150.0,
        side: pf_geometry::Side::Left,
        length_mm: 10.0,
        style: pf_core::NotchStyle::Single,
    })?;
    p.apply(Command::AddAnchoredNotch {
        sheet,
        entity: edges[2],
        anchor: pf_core::NotchAnchor::Midpoint {},
        side: pf_geometry::Side::Left,
        length_mm: 10.0,
        style: pf_core::NotchStyle::Double,
    })?;
    Ok((p, corner, xl, piece))
}

/// Projet de l'étape `mesures-tailles` (US-09.6a) : `gradation_derivee_project` (pièce « Devant »
/// à marge de 12 mm, gamme XS S M L XL de base M), une seconde pièce « Dos » non graduée cousue au
/// bord droit du devant, et un repère dont l'extrémité reçoit des écarts irréguliers (90, 98,
/// 100, 108, 120 mm : le pas de 2 mm s'écarte de la médiane, l'alerte s'affiche). Renvoie aussi la
/// gamme.
/// Projet de l'étape « variantes » : les pièces du projet de `mesures_tailles_project`, deux
/// variantes (« Chemise » : Devant × 2 avec l'option « avec poche » active et Dos × 1 en « Lin » ;
/// « Gilet » : Dos × 1) ; rien n'est copié, les entrées citent les pièces.
pub(crate) fn variantes_project() -> Result<Project, pf_core::CoreError> {
    let (mut p, _) = mesures_tailles_project()?;
    let (a, b) = (p.pieces[0].id, p.pieces[1].id);
    let v = p
        .apply(Command::AddVariant {
            name: "Chemise".into(),
        })?
        .variants[0];
    p.apply(Command::AddVariantOption {
        variant: v,
        option: "avec poche".into(),
    })?;
    for (piece, quantity, material, option) in
        [(a, 2, None, Some("avec poche")), (b, 1, Some("Lin"), None)]
    {
        p.apply(Command::AddVariantEntry {
            variant: v,
            piece,
            quantity,
            material: material.map(str::to_string),
            option: option.map(str::to_string),
        })?;
    }
    p.apply(Command::SetVariantOptionActive {
        variant: v,
        option: "avec poche".into(),
        active: true,
    })?;
    let g = p
        .apply(Command::AddVariant {
            name: "Gilet".into(),
        })?
        .variants[0];
    p.apply(Command::AddVariantEntry {
        variant: g,
        piece: b,
        quantity: 1,
        material: None,
        option: None,
    })?;
    Ok(p)
}

/// Étape `nommage` (US-11.5) : trois pièces « P-001 », « P-002 » et « Manche », une règle
/// « Regle P » (préfixe P, séparateur -, compteur 1 pas 1 largeur 3) et une règle « Regle F »
/// (préfixe F, compteur 10 pas 10 largeur 2). Aucun nom n'est appliqué d'office.
pub(crate) fn nommage_project() -> Result<Project, pf_core::CoreError> {
    let mut p = Project::new("Nommage");
    let sheet = p.sheets[0].id;
    for (i, name) in ["P-001", "P-002", "Manche"].iter().enumerate() {
        let r = p.apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(PointMm::new(150.0 * i as f64, 0.0)),
            width_mm: 100.0,
            height_mm: 60.0,
        })?;
        p.apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: r.entities,
            },
            name: (*name).into(),
            detached: false,
        })?;
    }
    for (name, target, prefix, start, step, width) in [
        ("Regle P", NamingTarget::Piece, "P", 1, 1, 3),
        ("Regle F", NamingTarget::Sheet, "F", 10, 10, 2),
    ] {
        p.apply(Command::AddNamingRule {
            spec: NamingRuleSpec {
                name: name.into(),
                target,
                prefix: prefix.into(),
                separator: "-".into(),
                suffix: String::new(),
                counter: Some(NamingCounter { start, step, width }),
            },
        })?;
    }
    Ok(p)
}

pub(crate) fn mesures_tailles_project(
) -> Result<(Project, pf_core::SizeRangeId), pf_core::CoreError> {
    let (mut p, _, _, front) = gradation_derivee_project()?;
    let sheet = p.sheets[0].id;
    let range = p.sizes[0].id;
    let sizes: Vec<_> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    let right = p.pieces[0].outline[1].entity;
    let rect = p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(PointMm::new(600.0, 0.0)),
        width_mm: 400.0,
        height_mm: 600.0,
    })?;
    let back = p
        .apply(Command::ExtractPiece {
            sheet,
            source: pf_core::PieceSource::Paths {
                entities: rect.entities.clone(),
            },
            name: "Dos".into(),
            detached: false,
        })?
        .pieces[0];
    let vertical = {
        let s = p.sheet(sheet)?;
        rect.entities
            .iter()
            .copied()
            .find(|e| {
                s.segment_ends(*e)
                    .is_ok_and(|(a, b)| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() > 1.0)
            })
            .ok_or_else(|| pf_core::CoreError::Invalid("bord vertical introuvable".into()))?
    };
    let a = p.seam_side_range(front, right, 1)?;
    let b = p.seam_side_range(back, vertical, 1)?;
    p.apply(Command::AddSeamPair {
        a,
        b,
        embu_mm: None,
        tolerance_mm: None,
    })?;
    let mark = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(PointMm::new(0.0, -100.0)),
            to: PointRef::New(PointMm::new(100.0, -100.0)),
        })?
        .points;
    let end = *mark
        .last()
        .ok_or_else(|| pf_core::CoreError::Invalid("marker without end point".into()))?;
    for (i, dx) in [(0, -10.0), (1, -2.0), (3, 8.0), (4, 20.0)] {
        p.apply(Command::SetPointGrading {
            sheet,
            point: end,
            size: sizes[i],
            dx_mm: dx,
            dy_mm: 0.0,
        })?;
    }
    Ok((p, range))
}

/// Projet de l'étape `nid-tailles` (US-09.4a) : un corsage à six points, une gamme numérique de
/// `count` tailles (34, 36, 38…) de base la cinquième, écarts cumulés **proportionnels au rang**
/// depuis la base sur cinq des six points (le point A, en bas à gauche, ne bouge jamais). Renvoie
/// aussi le point C (hanche, point d'empilement de la capture) et la gamme.
pub(crate) fn nid_tailles_project(
    count: usize,
) -> Result<(Project, pf_core::PointId, pf_core::SizeRangeId), pf_core::CoreError> {
    use pf_core::SizeKind;
    let mut p = Project::new("Size nest");
    let sheet = p.sheets[0].id;
    let polygon = p.apply(Command::AddPolygon {
        sheet,
        vertices: [
            (0.0, 0.0),
            (400.0, 0.0),
            (430.0, 300.0),
            (380.0, 600.0),
            (150.0, 640.0),
            (0.0, 560.0),
        ]
        .iter()
        .map(|&(x, y)| PointRef::New(PointMm::new(x, y)))
        .collect(),
        closed: true,
        role: pf_core::PathRole::SewingContour,
        layer: None,
    })?;
    let corners = polygon.points.clone();
    let base = 4.min(count.saturating_sub(1));
    p.apply(Command::AddSizeRange {
        name: "Women".into(),
        kind: SizeKind::Numeric,
        labels: (0..count).map(|i| (34 + 2 * i).to_string()).collect(),
        base,
    })?;
    let range = p.sizes[0].id;
    let sizes: Vec<_> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    for (i, size) in sizes.iter().enumerate().filter(|(i, _)| *i != base) {
        let k = i as f64 - base as f64;
        for (point, dx, dy) in [
            (corners[1], 4.0 * k, 0.0),
            (corners[2], 5.0 * k, 3.0 * k),
            (corners[3], 4.0 * k, 8.0 * k),
            (corners[4], 2.0 * k, 8.0 * k),
            (corners[5], 0.0, 8.0 * k),
        ] {
            p.apply(Command::SetPointGrading {
                sheet,
                point,
                size: *size,
                dx_mm: dx,
                dy_mm: dy,
            })?;
        }
    }
    Ok((p, corners[2], range))
}

/// Projet de l'étape `controle-forme` (US-09.9) : gamme XS S M L XL de base M et trois courbes de
/// Bézier dont l'arrivée est graduée. A : `p3` rejoint `(120 ; 0)` en XL, où `c2 = (0 ; 80)` donne
/// un rebroussement exact (signalé) ; B : courbe en S, une inflexion partout (aucun signalement) ;
/// C : arc de parabole qui gagne une inflexion et une courbure plus forte en XL (signalé). Renvoie
/// aussi la gamme.
pub(crate) fn controle_forme_project() -> Result<(Project, pf_core::SizeRangeId), pf_core::CoreError>
{
    use pf_core::SizeKind;
    let mut p = Project::new("Shape check");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Women".into(),
        kind: SizeKind::Alphanumeric,
        labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
        base: 2,
    })?;
    let range = p.sizes[0].id;
    let sizes: Vec<_> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    type Curve = (f64, [(f64, f64); 4], [(f64, f64); 5]);
    let steps = [
        (-9.0, -20.0),
        (-5.0, -10.0),
        (0.0, 0.0),
        (5.0, 10.0),
        (10.0, 22.0),
    ];
    let curves: [Curve; 3] = [
        (
            0.0,
            [(0.0, 0.0), (120.0, 80.0), (-10.0, 58.0), (110.0, -22.0)],
            steps,
        ),
        (
            180.0,
            [(0.0, 0.0), (40.0, 40.0), (60.0, -40.0), (100.0, 0.0)],
            steps,
        ),
        (
            360.0,
            [(0.0, 0.0), (20.0, 0.0), (40.0, 6.0), (60.0, 18.0)],
            [(0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 1.0), (0.0, 10.0)],
        ),
    ];
    for (ox, q, deltas) in curves {
        let at = |i: usize| PointMm::new(ox + q[i].0, q[i].1);
        let end = p
            .apply(Command::AddCurve {
                sheet,
                start: PointRef::New(at(0)),
                c1: at(1),
                c2: at(2),
                end: PointRef::New(at(3)),
            })?
            .points
            .last()
            .copied()
            .ok_or_else(|| pf_core::CoreError::Invalid("end point missing".into()))?;
        for (size, (dx, dy)) in sizes.iter().zip(deltas).filter(|(_, d)| *d != (0.0, 0.0)) {
            p.apply(Command::SetPointGrading {
                sheet,
                point: end,
                size: *size,
                dx_mm: dx,
                dy_mm: dy,
            })?;
        }
    }
    Ok((p, range))
}

impl App {
    fn load_stage_project(&mut self, project: Result<Project, pf_core::CoreError>) -> bool {
        match project {
            Ok(p) => {
                self.editor = crate::editor::Editor::new(Document::new(p));
                self.fit_pending = true;
                true
            }
            Err(e) => {
                self.error_dialog = Some(core_error(&e));
                false
            }
        }
    }
}

/// Prépare l'étape `stage` (hors étapes historiques). `false` : nom inconnu.
pub(crate) fn apply(app: &mut App, stage: &str) -> bool {
    match stage {
        "feuille" => {
            app.load_stage_project(pf_core::demo_piece_project());
        }
        "trait" | "courbe" | "cercle" | "point-libre" | "point-construit" | "deux-points"
        | "plusieurs-traits" => {
            let project = pf_core::demo_project().and_then(|mut p| {
                if stage == "cercle" {
                    let sheet = p.sheets[0].id;
                    p.apply(Command::AddCircle {
                        sheet,
                        center: PointRef::New(pt(150.0, 60.0)),
                        radius_mm: 25.0,
                    })?;
                }
                Ok(p)
            });
            if !app.load_stage_project(project) {
                return true;
            }
            let sheet = app.editor.sheet();
            let entities: Vec<_> = sheet.entities.iter().map(|e| e.id).collect();
            let points: Vec<_> = sheet.points.iter().map(|p| p.id).collect();
            let derived = sheet
                .points
                .iter()
                .find(|p| !matches!(p.definition, pf_core::PointDefinition::Free { .. }))
                .map(|p| p.id);
            let sel = &mut app.editor.selection;
            match stage {
                "trait" => sel.entities.extend(entities.first()),
                "courbe" | "cercle" => sel.entities.extend(entities.last()),
                "point-libre" => sel.points.extend(points.first()),
                "point-construit" => sel.points.extend(derived),
                "deux-points" => sel.points.extend(points.iter().take(2)),
                _ => sel.entities.extend(entities.iter().take(3)),
            }
        }
        "cote" | "repere" => {
            let project = pf_core::demo_piece_project().and_then(|mut p| {
                if stage == "repere" {
                    let sheet = p.sheets[0].id;
                    p.apply(Command::AddGuide {
                        sheet,
                        x_mm: 85.0,
                        y_mm: 60.0,
                        angle_deg: 90.0,
                        axis: true,
                        layer: None,
                    })?;
                }
                Ok(p)
            });
            if app.load_stage_project(project) {
                let sheet = app.editor.sheet();
                let (dim, guide) = (
                    sheet.dimensions.first().map(|d| d.id),
                    sheet.guides.first().map(|g| g.id),
                );
                let sel = &mut app.editor.selection;
                if stage == "cote" {
                    sel.dimensions.extend(dim);
                } else {
                    sel.guides.extend(guide);
                }
            }
        }
        "fiche-riche" => {
            app.load_stage_project(rich_project());
        }
        "sans-droit-fil" => {
            app.load_stage_project(no_grain_project());
        }
        "alignement" => {
            app.load_stage_project(pf_core::demo_project());
            if let Some(first) = app.editor.sheet().entities.first().map(|e| e.id) {
                app.editor.selection.entities.push(first);
            }
            app.open_level(true);
        }
        // Construction rapide (PH-4a) : rectangle arrondi posé, fenêtre ouverte.
        "construction-rapide" => {
            app.load_stage_project(Ok(Project::new("Construction rapide")));
            crate::quick::demo_quick(app);
        }
        // Perçages au pas (PH-4b) : rangées créées, bloc ouvert avec l'aperçu d'une troisième.
        "percages-au-pas" => {
            if app.load_stage_project(crate::marking_run::demo_pitch_project()) {
                crate::marking_run::demo_pitch(app);
            }
        }
        // Contrôle des trous de piqûre (US-08.6) : deux pièces, couple déclaré, bloc ouvert.
        "controle-piqure" => {
            if app.load_stage_project(crate::stitch::demo_stitch_pair_project()) {
                crate::stitch::demo_stitch(app);
            }
        }
        // Zones de parage, de collage et de découpe interne (US-06.10) : trois zones, bloc ouvert.
        "zones" => {
            if app.load_stage_project(crate::zones::demo_zone_project()) {
                crate::zones::demo_zones(app);
            }
        }
        // Lignes de piqûre (US-06.9) : ligne autour du contour, bloc ouvert.
        "piqure" => {
            if app.load_stage_project(crate::stitch::demo_stitch_project()) {
                crate::stitch::demo_stitch(app);
            }
        }
        "coutures" => match crate::seams::demo_seam_project() {
            Ok(p) => {
                app.load_stage_project(Ok(p));
                app.open_seams();
                // Le premier couple est choisi : son embu et sa tolérance sont modifiables.
                app.editor.seam.selected = app.editor.project().seams.first().map(|s| s.id);
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        "variables" => {
            if app.load_stage_project(variables_project()) {
                crate::variables::demo_stage(app);
            }
        }
        // Valeur maintenue (US-03.1b) : le point en direction est choisi ; sa distance suit
        // `largeur/2+10`, une formule invalide vient d'être refusée sous le champ de l'angle.
        "formule" => match formula_project() {
            Ok((p, c)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.points.push(c);
                app.demo_formula_submit(c, 1, "90-");
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Construction maintenue (US-03.2) : le segment de 100 mm est choisi dans l'inspecteur.
        "maintenu" => match kept_project() {
            Ok((p, seg)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.entities.push(seg);
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Rectangle maintenu (US-03.2b) : le côté de départ est choisi dans l'inspecteur.
        "rectangle-maintenu" => match kept_rectangle_project() {
            Ok((p, seg)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.entities.push(seg);
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Formule sur un rayon du compas (US-03.1 CA 3) : le point est choisi dans l'inspecteur.
        "formule-compas" => match compass_formula_project() {
            Ok((p, c)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.points.push(c);
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Formule sur la distance d'un point d'un segment (US-03.1 CA 3, restes) : point choisi.
        "formule-segment" => match rest_formula_project() {
            Ok((p, ps, _, _)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.points.push(ps);
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Formule à la création : outil Point en mode Direction, parent choisi, expressions saisies
        // dans la palette (distance `L/2`, angle `a*2`) ; rien n'est créé avant Entrée.
        "formule-creation" => match rest_formula_project() {
            Ok((p, _, _, start)) => {
                app.load_stage_project(Ok(p));
                app.editor.set_tool(crate::editor::Tool::Point);
                app.editor
                    .set_point_mode(crate::editor::PointMode::Direction);
                app.editor
                    .rel_picks
                    .push(crate::editor::RelPick::Point(start));
                app.editor.state = crate::editor::ToolState::RelativePick;
                app.editor.length_text = "L/2".into();
                app.editor.angle_text = "a*2".into();
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Longueur imposée (US-03.3) : la courbe est choisie, la section de l'inspecteur est ouverte.
        "longueur" => match imposed_project() {
            Ok((p, curve)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.entities.push(curve);
                app.imposed_open_once = true;
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Égalité de longueurs (US-03.4) : la tête de manche est choisie, section ouverte.
        "egalite" => match equality_project() {
            Ok((p, curve)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.entities.push(curve);
                app.imposed_open_once = true;
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Somme de tracés d'autres feuilles et décalage par formule (US-03.4b) : section ouverte.
        "egalite-somme" => match equality_sum_project() {
            Ok((p, curve)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.entities.push(curve);
                app.imposed_open_once = true;
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Courbe lisse à longueur imposée par une formule (US-03.3b) : courbe choisie, section ouverte.
        "longueur-lisse" => match smooth_imposed_project() {
            Ok((p, curve)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.entities.push(curve);
                app.imposed_open_once = true;
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Diagnostic des liens (US-03.5) : fenêtre ouverte sur un projet qui porte les quatre
        // mécanismes, des variables inutilisées et un calque verrouillé.
        "diagnostic" => {
            if app.load_stage_project(diagnostic_project()) {
                app.open_diagnostic();
            }
        }
        // Historique de construction (US-03.6) : le point F choisi, fenêtre « Dépendances » ouverte,
        // tous les niveaux et les deux sections dépliées (elles sont repliées par défaut).
        "dependances" => match dependances_project() {
            Ok((p, point)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.points.push(point);
                app.open_dependencies();
                app.dependencies = Some(crate::dependencies::DependenciesDialog {
                    all: true,
                    parents_open: true,
                    children_open: true,
                });
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Tailles du modèle (US-09.1) : fenêtre « Tailles » ouverte, première gamme dépliée (les
        // gammes sont repliées par défaut).
        "tailles" => {
            if app.load_stage_project(tailles_project()) {
                app.open_sizes();
                let first = app.editor.project().sizes.first().map(|r| r.id);
                app.sizes = Some(crate::sizes::SizesDialog::with_open(first));
            }
        }
        // Gradation par point (US-09.2a) : fenêtre « Gradation » ouverte (gamme dépliée), coin
        // haut-droit sélectionné, taille XL affichée en lecture seule.
        "gradation" => match gradation_project() {
            Ok((p, point, size)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.points.push(point);
                app.editor.set_active_size(Some(size));
                app.open_grading();
                let first = app.editor.project().sizes.first().map(|r| r.id);
                app.grading = Some(crate::grading::GradingDialog::with_open(first));
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Report et interpolation de gradation (US-09.3a) : taille de base, les deux coins de
        // droite sont gradués ; le coin haut-droit (source) et le coin haut-gauche sont
        // sélectionnés, sections « Reporter la règle » (symétrie X) et « Interpoler entre deux
        // points » dépliées.
        "gradation-report" => match gradation_project() {
            Ok((p, point, _)) => {
                let corners = p.sheets[0].points.iter().map(|q| q.id).collect::<Vec<_>>();
                app.load_stage_project(Ok(p));
                app.editor.selection.points.push(point);
                app.editor.selection.points.push(corners[3]);
                app.open_grading();
                // Gamme repliée : la fenêtre montre les deux sections en entier.
                let mut d = crate::grading::GradingDialog::default();
                d.shown = Some(point);
                d.report_open = true;
                d.interp_open = true;
                d.mode = pf_core::GradingCopyMode::MirrorX;
                d.from = Some(corners[1]);
                d.to = Some(point);
                app.grading = Some(d);
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Géométrie dérivée gradée (US-09.5a) : pièce à marge et crans en taille XL (lecture
        // seule), rien de sélectionné : l'inspecteur montre les mesures de la pièce dans XL ; contour
        // de coupe, crans et mesures sont ceux de XL.
        "gradation-derivee" => match gradation_derivee_project() {
            Ok((p, _, size, _)) => {
                app.load_stage_project(Ok(p));
                app.editor.set_active_size(Some(size));
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Mesures par taille (US-09.6a) : fenêtre ouverte, coutures et tracés dépliés (pièces repliées),
        // taille de base ; le tracé en alerte remonte en tête de sa section.
        "mesures-tailles" => match mesures_tailles_project() {
            Ok((p, range)) => {
                app.load_stage_project(Ok(p));
                app.open_size_measures();
                let mut d = crate::size_measures::SizeMeasuresDialog::default();
                d.seams_open = true;
                d.paths_open = true;
                d.range = Some(range);
                app.size_measures = Some(d);
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Export des mesures par taille (US-09.8b) : même fenêtre, sections repliées pour que
        // « Copier en CSV », « Exporter en CSV… », « Exporter en PDF… » et la mention de lecture
        // seule soient visibles sans défiler ; aucun fichier n'est écrit par l'étape.
        "mesures-export" => match mesures_tailles_project() {
            Ok((p, range)) => {
                app.load_stage_project(Ok(p));
                app.open_size_measures();
                let mut d = crate::size_measures::SizeMeasuresDialog::default();
                d.range = Some(range);
                app.size_measures = Some(d);
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Nomenclature et fiche technique (US-11.2, US-11.3) : fenêtre ouverte sur un projet dont
        // les pièces ont une matière, une quantité et une paire en miroir ; aucun fichier écrit.
        "nomenclature" => match mesures_tailles_project() {
            Ok((mut p, range)) => {
                let cards = [(0, 2, true, Some("Cuir")), (1, 1, false, Some("Doublure"))];
                for (i, quantity, pair, material) in cards {
                    if let Some(piece) = p.pieces.get(i).map(|x| (x.id, x.name.clone())) {
                        let _ = p.apply(Command::SetPieceCard {
                            piece: piece.0,
                            name: piece.1,
                            code: None,
                            quantity,
                            mirrored_pair: pair,
                            material: material.map(str::to_string),
                            category: None,
                            comment: None,
                        });
                    }
                }
                app.load_stage_project(Ok(p));
                app.open_bom();
                if let Some(d) = app.bom.as_mut() {
                    d.range = Some(range);
                }
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Variantes (US-11.1) : fenêtre ouverte sur deux variantes qui citent les mêmes pièces ;
        // « Chemise » a une option « avec poche » active (le Devant compte) et une matière surchargée.
        "variantes" => match variantes_project() {
            Ok(p) => {
                app.load_stage_project(Ok(p));
                app.open_variants();
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Versions (US-17.2) : projet enregistré dans un dossier temporaire avec deux versions
        // nommées, puis modifié ; la fenêtre compare la première version à l'état courant.
        // Impression directe (US-13.6) : faux spouleur de démonstration (deux imprimantes), travail envoyé.
        "imprimer" => {
            app.spooler = Box::new(crate::direct_print::DemoSpooler);
            app.open_direct_print();
            app.direct_print_send();
        }
        // Projection (US-13.7) : plein écran noir, grille de 10 mm, carré étalon de 100 mm.
        "projection" => {
            app.open_projection();
            app.projection_arm();
        }
        // Image de fond calibree et decalque (US-14.1 / 14.2) : photo generee par le code,
        // calibration a deux points, contours calcules, points poses et apercu du lissage.
        "fond-image" => {
            if let Err(e) = crate::background::background_stage(app) {
                app.error_dialog = Some(e);
            }
        }
        "versions" => {
            if let Err(e) = versions_stage(app) {
                app.error_dialog = Some(core_error(&e));
            }
        }
        // Plan de coupe (US-15.1 / US-15.2) : fenêtre ouverte sur un plan où une poche chevauche le
        // devant et une autre déborde de la zone utile ; la poche qui chevauche est sélectionnée.
        "plan-coupe" => match crate::cutting::cutting_stage_project(false) {
            Ok((p, plan, placement)) => {
                app.load_stage_project(Ok(p));
                app.open_cutting();
                if let Some(d) = app.cutting.as_mut() {
                    d.selected = Some(plan);
                    d.placement = Some(placement);
                }
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Plan de coupe, métrage (US-15.3) : plan valide (aucun défaut), longueur, surface et
        // rendement affichés, boutons d'export.
        "plan-coupe-metrage" => match crate::cutting::cutting_stage_project(true) {
            Ok((p, plan, placement)) => {
                app.load_stage_project(Ok(p));
                app.open_cutting();
                if let Some(d) = app.cutting.as_mut() {
                    d.selected = Some(plan);
                    let _ = placement;
                    d.placement = None;
                    d.list_open = false;
                    d.canvas_open = false;
                    d.placed_open = false;
                    d.diag_open = false;
                }
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Plan de coupe, placement automatique (US-15.4) : plan valide, proposition calculée pour
        // toutes les pièces et affichée à côté du placement actuel (`-apres` : proposition appliquée).
        // `-completer` (tranche 133) : la Poche (4e pose) est retirée, mode « compléter » : seule la
        // Poche est reposée, les trois autres poses restent où elles sont.
        "plan-coupe-auto" | "plan-coupe-auto-apres" | "plan-coupe-auto-completer" => {
            match crate::cutting::cutting_stage_project(true) {
                Ok((mut p, plan, _)) => {
                    let complete = stage == "plan-coupe-auto-completer";
                    if complete {
                        let last = p
                            .cutting_plan(plan)
                            .ok()
                            .and_then(|c| c.placements.last().map(|q| q.id));
                        if let Some(placement) = last {
                            let _ = p.apply(Command::RemovePlacement { plan, placement });
                        }
                    }
                    app.load_stage_project(Ok(p));
                    app.open_cutting();
                    let project = app.editor.project().clone();
                    if let Some(mut d) = app.cutting.take() {
                        d.selected = Some(plan);
                        d.placement = None;
                        d.auto_open = true;
                        d.auto_all = true;
                        d.list_open = false;
                        d.placed_open = false;
                        d.diag_open = false;
                        if complete {
                            d.auto_mode = pf_core::cutting_auto::AutoMode::CompleteMissing;
                        }
                        let before = stage != "plan-coupe-auto-apres";
                        d.canvas_open = !before;
                        d.yield_open = !before;
                        d.auto_blocking(&project, plan);
                        if !before {
                            if let Some(Ok(proposal)) = d.auto_out.take() {
                                let _ = app.measure_exec(proposal.command);
                            }
                        }
                        app.cutting = Some(d);
                    }
                }
                Err(e) => app.error_dialog = Some(core_error(&e)),
            }
        }
        // Cuir (US-15.5) : plan « Peau » (contour irrégulier, un trou, une cicatrice, deux zones de
        // qualité) avec un devant posé sur le trou et une poche qui sort du coin ; `-auto` : plan
        // sans pose, proposition de placement sur peau calculée et affichée.
        "cuir-peau" | "cuir-peau-auto" => {
            match crate::cutting_hide::cuir_stage_project(stage == "cuir-peau-auto") {
                Ok((p, plan)) => {
                    app.load_stage_project(Ok(p));
                    app.open_cutting();
                    let project = app.editor.project().clone();
                    if let Some(mut d) = app.cutting.take() {
                        d.selected = Some(plan);
                        d.placement = None;
                        d.list_open = false;
                        d.placed_open = false;
                        d.yield_open = false;
                        if stage == "cuir-peau-auto" {
                            d.auto_open = true;
                            d.auto_all = true;
                            d.auto_blocking(&project, plan);
                        }
                        app.cutting = Some(d);
                    }
                }
                Err(e) => app.error_dialog = Some(core_error(&e)),
            }
        }
        // Bibliothèque (US-11.4) : dossier temporaire avec une pièce, un bloc et un modèle
        // enregistrés, un fichier abîmé listé à part ; la fenêtre est ouverte sur la première entrée.
        "bibliotheque" => {
            if let Err(e) = bibliotheque_stage(app) {
                app.error_dialog = Some(core_error(&e));
            }
        }
        // Conventions de nommage (US-11.5) : fenêtre ouverte sur la règle « Regle P » (prochain
        // nom P-003) avec l'aperçu du renommage en lot dépliés ; aucune règle appliquée d'office.
        "nommage" => match nommage_project() {
            Ok(p) => {
                let rule = p.naming_rules[0].id;
                app.load_stage_project(Ok(p));
                app.open_naming();
                if let Some(d) = app.naming.as_mut() {
                    d.selected = Some(rule);
                    d.preview = true;
                }
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Nid de tailles (US-09.4a) : douze tailles superposées, point d'empilement à la hanche,
        // fenêtre ouverte ; chaque taille a sa couleur, son motif de trait et son étiquette.
        "nid-tailles" => match nid_tailles_project(12) {
            Ok((p, anchor, range)) => {
                app.load_stage_project(Ok(p));
                app.nest.enabled = true;
                app.nest.range = Some(range);
                app.nest.anchor = Some(anchor);
                app.open_size_nest();
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Contrôle de forme (US-09.9) : fenêtre ouverte, signalements et tableau courbe × taille
        // dépliés ; un rebroussement introduit en XL, une inflexion et un saut de courbure.
        "controle-forme" => match controle_forme_project() {
            Ok((p, range)) => {
                app.load_stage_project(Ok(p));
                app.open_shape_control();
                let mut d = crate::shape_control::ShapeControlDialog::default();
                d.signals_open = true;
                d.table_open = true;
                d.range = Some(range);
                app.shape_control = Some(d);
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Export gradué (US-09.8) : fenêtre « Exporter des tailles » du dock, cinq des douze
        // tailles cochées, format PDF ; la taille exportée par défaut est la base.
        "export-gradue" => match nid_tailles_project(12) {
            Ok((p, _, _)) => {
                app.load_stage_project(Ok(p));
                let picked: std::collections::BTreeSet<pf_core::SizeId> =
                    app.editor.project().sizes[0]
                        .sizes
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| matches!(i, 2 | 4 | 6 | 8 | 10))
                        .map(|(_, s)| s.id)
                        .collect();
                app.open_export_sizes();
                if let Some(d) = app.export_sizes.as_mut() {
                    d.selected = picked;
                }
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        "marche" => match crate::seams::demo_march_project() {
            Ok(p) => {
                app.load_stage_project(Ok(p));
                app.open_seams();
                app.editor.seam.selected = app.editor.project().seams.first().map(|s| s.id);
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Cran proportionnel et valeurs de variables par taille (US-09.5c, US-09.7) : fenêtre
        // « Variables » avec la section « Valeurs par taille » dépliée sur L, cran choisi.
        "taille-valeurs" => match crate::variables::demo_sizes_project() {
            Ok((p, notch, size)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.notches.push(notch);
                crate::variables::demo_sizes_stage(app, size);
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Barème de mesures (US-10.1) : tableau mesures x tailles d'une gamme, section « Fichier
        // CSV » dépliée (réglages et boutons d'import / d'export) ; aucun fichier n'est écrit.
        "bareme" => match crate::measurements::demo_project() {
            Ok((p, _)) => {
                app.load_stage_project(Ok(p));
                app.open_bareme();
                if let Some(d) = app.bareme.as_mut() {
                    d.csv_open = true;
                }
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Fiche client (US-10.2) : fiche « Camille » (angle d'épaule manquant), section « Taille la
        // plus proche » dépliée avec l'écart par mesure ; rien n'est appliqué.
        "fiche-client" => match crate::measurements::demo_project() {
            Ok((p, client)) => {
                app.load_stage_project(Ok(p));
                app.open_clients();
                if let Some(d) = app.clients.as_mut() {
                    d.selected = Some(client);
                    d.proposal_open = true;
                }
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Taille sur-mesure générée depuis la fiche « Camille » (US-10.2) : taille « Camille »
        // ajoutée, angle d'épaule laissé vide et signalé dans le rapport.
        "fiche-client-taille" => match crate::measurements::demo_project().and_then(|(mut p, c)| {
            let range = p.sizes[0].id;
            p.apply(Command::GenerateClientSize {
                client: c,
                range,
                label: "Camille".into(),
                replace: false,
            })?;
            Ok((p, c))
        }) {
            Ok((p, client)) => {
                app.load_stage_project(Ok(p));
                app.open_clients();
                if let Some(d) = app.clients.as_mut() {
                    d.selected = Some(client);
                    d.gen_report = Some(("Camille".into(), 2, vec!["Shoulder angle".into()]));
                }
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Retouches et mesures finies (US-10.3 / 10.4) : rectangle gradué, retouche « Width -2 »,
        // deux mesures finies ; tableau de contrôle par taille (conforme, hors tolérance).
        "retouches" => match crate::finished::demo_project() {
            Ok(p) => {
                app.load_stage_project(Ok(p));
                app.open_finished();
                if let Some(d) = app.finished.as_mut() {
                    d.alt_open = false;
                }
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        // Cran en coin (US-06.1b) : pièce en L, crans aux sommets, celui du sommet rentrant choisi.
        "cran-coin" => match crate::notch_shape::demo_corner_project() {
            Ok((p, notch)) => {
                app.load_stage_project(Ok(p));
                app.editor.selection.notches.push(notch);
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        "crans-couture" => match crate::seams::demo_notch_check_project() {
            Ok(p) => {
                app.load_stage_project(Ok(p));
                app.open_seams();
                app.editor.seam.selected = app.editor.project().seams.first().map(|s| s.id);
                app.editor.seam.check_open = true;
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        "raccords" => match crate::seams::demo_junction_project() {
            Ok(p) => {
                app.load_stage_project(Ok(p));
                app.open_seams();
                let first = app.editor.project().seams.first().map(|s| s.id);
                app.editor.seam.selected = first;
                app.editor.seam.join_open = true;
                app.editor.seam.join_view = true;
                app.editor.seam.join_sel = first.map(|id| (id, pf_core::JunctionEnd::Start));
                app.fit_pending = true;
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        "assembler" => match crate::cutline::demo_cut_project() {
            Ok((p, piece)) => {
                app.load_stage_project(Ok(p));
                app.editor.cutline.piece = Some(piece);
                app.editor.cutline.line =
                    app.editor.project().cut_candidates(piece).first().copied();
                app.editor.cutline_apply();
                let first = app.editor.project().pieces.first().map(|p| p.id);
                app.open_assemble(first);
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        "deplier" => {
            if app.load_stage_project(rich_project()) {
                let piece = app.editor.project().pieces.first().map(|p| p.id);
                app.open_unfold(piece);
            }
        }
        "impression" => {
            if app.load_stage_project(crate::seam::demo_seam_project(false)) {
                app.open_print_setup();
            }
        }
        "etalonnage" => {
            app.load_stage_project(crate::seam::demo_seam_project(false));
            app.calib_dialog = Some(crate::print_setup::CalibDialog::new());
        }
        "dxf-export" => {
            if app.load_stage_project(rich_project()) {
                app.open_dxf_export();
            }
        }
        "hpgl-export" => {
            if app.load_stage_project(rich_project()) {
                app.open_hpgl_export();
            }
        }
        "rapport" => match crate::seams::demo_junction_project() {
            Ok(p) => {
                app.load_stage_project(Ok(p));
                app.open_report_export();
            }
            Err(e) => app.error_dialog = Some(core_error(&e)),
        },
        "dxf-construction" => {
            if app.load_stage_project(pf_core::demo_project()) {
                app.open_construction_export();
            }
        }
        "dxf-import" => {
            if app.load_stage_project(pf_core::demo_project()) {
                let sheet = app.editor.sheet;
                let source = pf_export::dxf_construction::export_construction_dxf(
                    app.editor.project(),
                    pf_export::dxf_construction::ConstructionScope::Sheet(sheet),
                );
                match source
                    .ok()
                    .and_then(|e| pf_export::dxf_import::DxfDocument::parse(e.bytes()).ok())
                {
                    Some(doc) => {
                        app.dxf_import = Some(crate::dxf_generic::ImportDialog::new(
                            "construction.dxf".into(),
                            doc,
                            sheet,
                        ));
                    }
                    None => app.error_dialog = Some("demo DXF unreadable".into()),
                }
            }
        }
        "recherche" => {
            app.load_stage_project(pf_core::demo_project());
            app.open_search();
        }
        "aide" => {
            app.load_stage_project(pf_core::demo_project());
            app.show_help = true;
        }
        "plan" => {
            if app.load_stage_project(rich_project()) {
                app.toggle_plan();
            }
        }
        "erreur" => {
            app.load_stage_project(pf_core::demo_project());
            app.error_dialog = Some(core_error(&pf_core::CoreError::UnknownEntity(
                pf_core::EntityId(999),
            )));
        }
        "enregistrer-avant-quitter" => {
            app.load_stage_project(pf_core::demo_project());
            app.pending = Some(Pending::Quit);
        }
        "import-feuille" => {
            if let Ok(source) = pf_core::demo_project() {
                app.load_stage_project(pf_core::demo_piece_project());
                app.import = Some(crate::app::ImportDialog {
                    path: "exemple.pforge.json".into(),
                    source,
                });
            }
        }
        "recuperation" => {
            if let Ok(project) = pf_core::demo_project() {
                app.recovery_prompt = Some(crate::app::RecoveryPrompt {
                    recovery: pf_core::recovery::Recovery {
                        autosave: "copie-de-secours.pforge.json".into(),
                        project,
                    },
                    path: None,
                });
            }
        }
        "fichier-modifie" => {
            app.load_stage_project(pf_core::demo_project());
            app.conflict_prompt = Some(crate::safeguard::ConflictPrompt {
                path: "exemple.pforge.json".into(),
                change: pf_core::ExternalChange::Modified,
            });
        }
        "fichier-ouvert-ailleurs" => {
            app.load_stage_project(pf_core::demo_project());
            app.lock_prompt = Some(crate::safeguard::LockPrompt {
                path: "exemple.pforge.json".into(),
                holder: pf_core::file_guard::LockHolder {
                    pid: 4242,
                    since: crate::safeguard::unix_now().saturating_sub(12),
                },
            });
        }
        "glossaire" => {
            app.load_stage_project(pf_core::demo_project());
            app.show_glossary = true;
        }
        "import-vectoriel" => {
            if app.load_stage_project(pf_core::demo_project()) {
                match pf_export::svg_import::SvgDocument::parse(vector_demo_svg().as_bytes()) {
                    Ok(doc) => {
                        app.vector_import = Some(crate::vector_import::VectorDialog::new(
                            "gabarit.svg".into(),
                            crate::vector_import::Source::Svg(doc),
                            app.editor.sheet,
                        ));
                    }
                    Err(_) => app.error_dialog = Some("demo SVG unreadable".into()),
                }
            }
        }
        "journal-erreurs" => {
            app.load_stage_project(pf_core::demo_project());
            for (t, kind, text) in [
                (
                    1_700_000_000,
                    "dialog",
                    "Enregistrement impossible. <chemin> : acc\u{e8}s refus\u{e9}",
                ),
                (
                    1_700_000_240,
                    "conflict",
                    "Enregistrement refus\u{e9} : fichier modifi\u{e9} ailleurs.",
                ),
                (
                    1_700_000_900,
                    "status",
                    "Refus : la feuille \u{ab}\u{2026}\u{bb} est verrouill\u{e9}e.",
                ),
            ] {
                app.error_log.record(t, kind, text, &[]);
            }
            app.show_error_log = true;
        }
        // PH-6 : import AAMA / ASTM (une pièce de 100 x 50 mm), historique, préférences et
        // espace de travail « Gradation ».
        "import-aama" => {
            app.load_stage_project(pf_core::demo_project());
            match pf_export::dxf_aama_import::AamaDocument::parse(aama_demo_text().as_bytes()) {
                Ok(doc) => {
                    app.aama_import =
                        Some(crate::dxf_aama::AamaDialog::new("pieces.dxf".into(), doc));
                }
                Err(_) => app.error_dialog = Some("demo AAMA unreadable".into()),
            }
        }
        "historique" => {
            if app.load_stage_project(pf_core::demo_piece_project()) {
                app.show_history = true;
            }
        }
        "preferences" => {
            app.load_stage_project(pf_core::demo_project());
            app.show_prefs = true;
        }
        "raccourcis" => {
            app.load_stage_project(pf_core::demo_project());
            app.show_prefs = true;
            app.shortcuts_open = true;
        }
        "focus-visible" => {
            app.load_stage_project(pf_core::demo_project());
            app.show_prefs = true;
            app.focus_demo = true;
        }
        "vues" => {
            app.load_stage_project(pf_core::demo_project());
            app.view_settings.units = pf_core::units::DisplayUnits {
                unit: pf_core::units::LengthUnit::In,
                inch_denominator: 16,
            };
            app.view_settings.grid =
                crate::views::GridSettings::default_for(pf_core::units::LengthUnit::In);
            app.view_settings.rulers = true;
            app.editor.show_rulers = true;
            for name in ["Ensemble", "Detail"] {
                let view = app.capture_view(name);
                let _ = app.view_settings.save_named(view);
            }
            app.show_views = true;
        }
        "unites-fenetres" => {
            app.load_stage_project(pf_core::demo_project());
            app.view_settings.units = pf_core::units::DisplayUnits {
                unit: pf_core::units::LengthUnit::In,
                inch_denominator: 16,
            };
            app.open_quick();
            app.open_pleats();
            app.open_seams();
        }
        "echelle-200" => {
            app.load_stage_project(pf_core::demo_project());
            app.prefs.ui_scale_pct = 200;
            app.show_prefs = true;
        }
        "espace-gradation" => {
            if app.load_stage_project(tailles_project()) {
                app.set_workspace(Some(crate::workspace::Workspace::Grading));
            }
        }
        _ => return false,
    }
    true
}

/// Étape « versions » : deux versions enregistrées (base, puis matière ajoutée), puis un point
/// déplacé de 50 mm (30 ; 40) et un tracé ajouté dans l'état courant.
/// Projet « Rabat » avec la variable `L` (50 mm) et un repère à `L` du départ du premier côté.
pub(crate) fn bibliotheque_project() -> Result<(Project, pf_core::PieceId), pf_core::CoreError> {
    let mut p = pf_core::demo_piece_project()?;
    p.apply(Command::AddVariable {
        name: "L".into(),
        unit: pf_core::VariableUnit::Mm,
        expr: "50".into(),
    })?;
    let sheet = p.sheets[0].id;
    let piece = p.pieces[0].id;
    let base = p.sheets[0].entities[0].id;
    let out = p.apply(Command::AddPointWithFormulas {
        sheet,
        definition: pf_core::PointDefinition::OnSegment {
            segment: base,
            distance_mm: 1.0,
            origin: pf_core::SegmentEnd::Start,
        },
        formulas: vec![pf_core::ParamExpr {
            param: pf_core::ParamKind::SegmentDistance,
            expr: "L".into(),
        }],
    })?;
    p.apply(Command::AddPieceInternal {
        piece,
        target: pf_core::InternalTarget::Point {
            point: out.points[0],
        },
        kind: pf_core::InternalKind::Mark,
    })?;
    Ok((p, piece))
}

fn bibliotheque_stage(app: &mut App) -> Result<(), pf_core::CoreError> {
    let io = |e: pf_core::LibraryError| pf_core::CoreError::Io(e.to_string());
    let (p, piece) = bibliotheque_project()?;
    let dir = std::env::temp_dir().join(format!(
        "patternforge-etape-bibliotheque-{}-{}-{}",
        std::process::id(),
        pf_i18n::lang().code(),
        format!("{:?}", std::thread::current().id())
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let lib = pf_core::Library::new(&dir);
    let sheet = p.sheets[0].id;
    let base = p.sheets[0].entities[0].id;
    let point = p.sheets[0].points.last().map(|q| q.id);
    lib.save("Rabat cuir", p.library_piece(piece)?)
        .map_err(io)?;
    let block = p.library_block(sheet, point.as_slice(), &[base], &[])?;
    lib.save("Bloc cote et repere", block).map_err(io)?;
    lib.save("Modele rabat", p.library_model()?).map_err(io)?;
    std::fs::write(
        dir.join("lib-000004.pforge-lib.json"),
        "{\"format\": \"patternforge-library\"",
    )
    .map_err(|e| pf_core::CoreError::Io(e.to_string()))?;
    app.load_stage_project(Ok(p));
    app.library_dir = Some(dir);
    app.open_library();
    if let Some(d) = app.library.as_mut() {
        d.selected = Some(1);
        d.rename_text = "Rabat cuir".into();
    }
    Ok(())
}

fn versions_stage(app: &mut App) -> Result<(), pf_core::CoreError> {
    use pf_core::versions;
    let io = |e: versions::VersionError| pf_core::CoreError::Io(e.to_string());
    let (mut p, _range) = mesures_tailles_project()?;
    let dir = std::env::temp_dir().join(format!(
        "patternforge-etape-versions-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| pf_core::CoreError::Io(e.to_string()))?;
    let path = dir.join("patron.pforge.json");
    let now = crate::safeguard::unix_now();
    let first = versions::create(&path, &p, "Base", now - 7200).map_err(io)?;
    if let Some(piece) = p.pieces.first().map(|x| (x.id, x.name.clone())) {
        p.apply(Command::SetPieceCard {
            piece: piece.0,
            name: piece.1,
            code: None,
            quantity: 1,
            mirrored_pair: false,
            material: Some("Cuir".into()),
            category: None,
            comment: None,
        })?;
    }
    versions::create(&path, &p, "Avant retouche", now - 3600).map_err(io)?;
    Document::new(p).save_as(&path)?;
    app.replace_document(Document::open(&path)?);
    let sheet = app.editor.project().sheets[0].id;
    let free = app.editor.project().sheets[0]
        .points
        .iter()
        .find_map(|pt| match pt.definition {
            pf_core::PointDefinition::Free { x, y } => Some((pt.id, x, y)),
            _ => None,
        });
    if let Some((id, x, y)) = free {
        app.editor.exec(Command::MovePoint {
            sheet,
            point: id,
            to: PointMm::new(x + 30.0, y + 40.0),
        });
        app.editor.exec(Command::AddSegment {
            sheet,
            from: PointRef::Existing(id),
            to: PointRef::New(PointMm::new(x + 200.0, y - 120.0)),
        });
    }
    app.open_versions();
    if let Some(d) = app.versions.as_mut() {
        d.selected = vec![first.file.clone()];
    }
    app.compare_versions(&[first.file]);
    Ok(())
}

#[cfg(test)]
mod stages_tests {
    use super::*;
    use crate::app::App;
    use crate::Launch;
    use eframe::egui;
    use egui_kittest::Harness;

    /// Étapes qui affichent volontairement une boîte d'erreur (le test n'y voit pas un échec).
    const STAGES_WITH_ERROR: &[&str] = &["erreur"];

    /// Chaque étape déclarée se prépare sans erreur, se dessine sans panic, produit une image non
    /// vide et ne laisse de boîte d'erreur que celles qui la demandent (un nom non géré en
    /// ouvrirait une). Pas de comparaison de pixels.
    #[test]
    fn chaque_etape_declaree_produit_une_image_non_vide() {
        let mut seen = std::collections::HashSet::new();
        for stage in STAGES {
            assert!(seen.insert(*stage), "duplicate stage: {stage}");
            let launch = Launch {
                demo: true,
                stage: Some((*stage).into()),
                ..Launch::default()
            };
            let app = App::with_context(&egui::Context::default(), launch);
            let mut h = Harness::builder()
                .with_size(egui::vec2(1600.0, 1000.0))
                .build_state(|ctx, app: &mut App| app.ui(ctx), app);
            h.run_steps(4);
            if !STAGES_WITH_ERROR.contains(stage) {
                assert!(
                    h.state().error_dialog.is_none(),
                    "stage {stage}: {:?}",
                    h.state().error_dialog
                );
            }
            let shapes = h.output().shapes.len();
            assert!(shapes > 10, "stage {stage}: empty image ({shapes} shapes)");
        }
    }

    #[test]
    fn une_etape_inconnue_est_refusee() {
        let launch = Launch {
            demo: true,
            stage: Some("n-existe-pas".into()),
            ..Launch::default()
        };
        let app = App::with_context(&egui::Context::default(), launch);
        assert!(app.error_dialog.is_some());
    }
}

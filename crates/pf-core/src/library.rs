//! Bibliothèque de bases (US-11.4) : contenu d'une entrée (pièce, bloc ou modèle), extraction
//! depuis un projet et insertion par **une** commande.
//!
//! La bibliothèque est un **dossier séparé du projet** (voir `library_store`) : aucun champ du
//! projet n'est ajouté (schéma 5 inchangé). Une entrée est un **instantané** : l'insérer crée une
//! copie indépendante (nouveaux identifiants, aucun lien ni suivi avec l'entrée ; la
//! préférence de la modéliste est que tout lien soit explicite et optionnel).
//!
//! Ce qui n'est pas repris est dit dans `LibraryContent::notes` (jamais de perte silencieuse) :
//! liens maintenus vers l'extérieur figés, longueurs imposées d'un bloc ou d'une pièce, écarts de
//! gradation, valeurs de variables par taille, et, pour un modèle, tout ce qui vit au niveau du
//! projet (tailles, variantes, coutures appariées, plans de coupe, barème, clients…).

use crate::clipboard::{Fragment, FRAGMENT_FORMAT, FRAGMENT_VERSION};
use crate::command::Outcome;
use crate::error::CoreError;
use crate::model::*;
use crate::piece_grain::GrainDirection;
use crate::piece_marking::MarkingAnchor;
use crate::variable::{check_variable_name, evaluate_variables, Variable};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Version du format d'un fichier de bibliothèque.
pub const LIBRARY_VERSION: u32 = 1;
/// Marqueur de format d'un fichier de bibliothèque.
pub const LIBRARY_FORMAT: &str = "patternforge-library";
/// Tolérance de comparaison des valeurs de variables (mm ou degrés).
const SAME_VALUE: f64 = 1e-9;

/// Nature d'une entrée de bibliothèque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    /// Une pièce avec sa géométrie, ses crans, son droit-fil et sa matière : insérée sur une
    /// **nouvelle feuille**.
    Piece,
    /// Une sélection de géométrie de construction : insérée dans une feuille **existante**.
    Block,
    /// Toutes les feuilles, pièces et variables du projet : insérées sur de **nouvelles feuilles**.
    Model,
}

/// Contenu autonome d'une entrée : feuilles, pièces et variables, avec leurs propres
/// identifiants (régénérés à l'insertion).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryContent {
    pub kind: LibraryKind,
    /// Compteur d'identifiants du contenu (tous ses identifiants lui sont inférieurs).
    pub next_id: u64,
    pub sheets: Vec<Sheet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pieces: Vec<Piece>,
    /// Variables dont dépend le contenu (nom, unité, expression ; jamais de valeur par taille).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    /// Ce qui n'a pas été repris à l'enregistrement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Variable de l'entrée qui existe déjà dans le projet avec une autre valeur ou une autre unité.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableConflict {
    /// Nom dans l'entrée.
    pub name: String,
    /// Nom de la variable du projet (diffère par la casse s'il est différent de `name`).
    pub existing_name: String,
    pub incoming_value: f64,
    pub existing_value: f64,
}

fn invalid(msg: impl Into<String>) -> CoreError {
    CoreError::Invalid(format!("bibliothèque : {}", msg.into()))
}

impl LibraryContent {
    /// Projet de travail qui ne sert qu'à valider : toutes les règles du format projet
    /// (identifiants, références, géométries, variables) s'appliquent au contenu.
    fn scratch(&self) -> Project {
        let mut p = Project::new("bibliothèque");
        p.sheets = self.sheets.clone();
        p.pieces = self.pieces.clone();
        p.variables = self.variables.clone();
        p.next_id = self.next_id;
        p
    }

    /// Vérifie la forme de l'entrée puis, par le validateur du projet, tous ses identifiants et
    /// toutes ses références. Une entrée qui échoue n'est jamais insérée.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.sheets.is_empty() {
            return Err(invalid("l'entrée ne contient aucune feuille"));
        }
        if self.sheets.iter().any(|s| !s.extra.is_empty())
            || self.pieces.iter().any(|p| !p.extra.is_empty())
        {
            return Err(invalid("champ inconnu dans une feuille ou une pièce"));
        }
        match self.kind {
            LibraryKind::Piece => {
                if self.sheets.len() != 1 || self.pieces.len() != 1 {
                    return Err(invalid(
                        "une entrée « pièce » contient une feuille et une pièce",
                    ));
                }
            }
            LibraryKind::Block => {
                let s = &self.sheets[0];
                if self.sheets.len() != 1 || !self.pieces.is_empty() {
                    return Err(invalid(
                        "une entrée « bloc » contient une feuille et aucune pièce",
                    ));
                }
                if s.points.is_empty() && s.entities.is_empty() && s.guides.is_empty() {
                    return Err(invalid("le bloc est vide"));
                }
            }
            LibraryKind::Model => {}
        }
        for p in &self.pieces {
            match p.link {
                PieceLink::Linked { sheet } if self.sheets.iter().any(|s| s.id == sheet) => {}
                _ => {
                    return Err(invalid(format!(
                        "la pièce « {} » n'est liée à aucune feuille de l'entrée",
                        p.name
                    )))
                }
            }
        }
        self.scratch().validate()
    }

    /// Texte JSON compact des variables (pour les comparaisons de test) : noms dans l'ordre.
    pub fn variable_names(&self) -> Vec<String> {
        self.variables.iter().map(|v| v.name.clone()).collect()
    }
}

/// Entités et points cités par une pièce (contour, droit-fil, marges, éléments internes,
/// marquages, pinces, boutons, piqûres, zones), dans la feuille `s`.
fn piece_refs(p: &Piece, s: &Sheet) -> (BTreeSet<PointId>, BTreeSet<EntityId>) {
    let mut points = BTreeSet::new();
    let mut ents = BTreeSet::new();
    ents.extend(p.outline.iter().map(|e| e.entity));
    ents.extend(p.grainline);
    let dir = |d: GrainDirection, ents: &mut BTreeSet<EntityId>| match d {
        GrainDirection::Parallel { reference } | GrainDirection::Perpendicular { reference } => {
            ents.insert(reference);
        }
        _ => {}
    };
    if let Some(g) = &p.grain {
        dir(g.direction, &mut ents);
        for e in &g.extra {
            ents.insert(e.entity);
            dir(e.direction, &mut ents);
        }
    }
    if let Some(sm) = &p.seam {
        ents.extend(sm.edges.iter().map(|e| e.entity));
        ents.extend(sm.hems.iter().map(|e| e.entity));
        ents.extend(sm.folds.iter().map(|e| e.entity));
        ents.extend(sm.corners.iter().map(|e| e.entity));
    }
    ents.extend(p.on_fold.iter().copied());
    for el in &p.internal {
        match el.target {
            InternalTarget::Path { entity } => {
                ents.insert(entity);
            }
            InternalTarget::Point { point } => {
                points.insert(point);
            }
            InternalTarget::Notch { notch } => {
                if let Some(n) = s.notches.iter().find(|n| n.id == notch) {
                    ents.insert(n.entity);
                }
            }
        }
    }
    for m in &p.markings {
        match m.at {
            MarkingAnchor::Point { point } => {
                points.insert(point);
            }
            MarkingAnchor::DartTip { tip, toward, .. } => {
                points.insert(tip);
                points.insert(toward);
            }
        }
    }
    for d in &p.darts {
        points.insert(d.apex);
        points.insert(d.toward);
    }
    for g in &p.buttons {
        points.insert(g.from);
        points.insert(g.to);
    }
    for l in &p.stitch_lines {
        ents.extend(l.edges.iter().copied());
    }
    for z in &p.zones {
        if let crate::piece_zone::ZoneShape::Band { edges, .. } = &z.shape {
            ents.extend(edges.iter().copied());
        }
    }
    (points, ents)
}

/// Ferme l'ensemble sous les dépendances : parents des points construits, points des tracés,
/// référence d'une perpendiculaire ou d'une parallèle, ancrages des crans, axes de symétrie.
fn close_selection(
    s: &Sheet,
    points: &mut BTreeSet<PointId>,
    ents: &mut BTreeSet<EntityId>,
    guides: &mut BTreeSet<GuideId>,
) {
    loop {
        let before = (points.len(), ents.len(), guides.len());
        let (pts, ens) = (points.clone(), ents.clone());
        for p in s.points.iter().filter(|p| pts.contains(&p.id)) {
            ents.extend(p.definition.dependencies());
            points.extend(p.definition.point_dependencies());
            guides.extend(p.definition.axis_guide());
        }
        for e in s.entities.iter().filter(|e| ens.contains(&e.id)) {
            points.extend(e.kind.all_points());
            if let EntityKind::Segment {
                construction:
                    Some(Construction::Perpendicular { reference, .. })
                    | Some(Construction::Parallel { reference, .. }),
                ..
            } = e.kind
            {
                ents.insert(reference);
            }
        }
        for n in s.notches.iter().filter(|n| ens.contains(&n.entity)) {
            points.extend(n.anchor.and_then(|a| a.point()));
            if let Some(NotchAnchor::Carried { entity, .. }) = n.anchor {
                ents.insert(entity);
            }
        }
        if before == (points.len(), ents.len(), guides.len()) {
            return;
        }
    }
}

/// Variables citées (directement ou par d'autres variables) par `exprs`, dans l'ordre de la
/// table du projet ; valeurs par taille retirées.
fn needed_variables<'a>(
    project: &Project,
    exprs: impl Iterator<Item = &'a str>,
) -> Result<Vec<Variable>, CoreError> {
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    let mut todo: Vec<String> = Vec::new();
    let cite = |text: &str, todo: &mut Vec<String>| -> Result<(), CoreError> {
        let e =
            crate::expr::parse(text).map_err(|e| invalid(format!("formule illisible : {e}")))?;
        todo.extend(e.references().into_iter().map(|r| r.name));
        Ok(())
    };
    for text in exprs {
        cite(text, &mut todo)?;
    }
    while let Some(name) = todo.pop() {
        if !wanted.insert(name.clone()) {
            continue;
        }
        let v = project
            .variables
            .iter()
            .find(|v| v.name == name)
            .ok_or_else(|| invalid(format!("la variable « {name} » est introuvable")))?;
        cite(&v.expr, &mut todo)?;
    }
    Ok(project
        .variables
        .iter()
        .filter(|v| wanted.contains(&v.name))
        .map(|v| Variable {
            per_size: Vec::new(),
            ..v.clone()
        })
        .collect())
}

impl Project {
    /// Construit le contenu « pièce » ou « bloc » à partir de l'ensemble choisi (déjà fermé pour
    /// une pièce) : géométrie par le presse-papiers (parent externe figé, avec remarque),
    /// formules des points restés construits, variables nécessaires.
    fn library_shell(
        &self,
        sheet: SheetId,
        points: &[PointId],
        ents: &[EntityId],
        notches: &[NotchId],
        guides: &[GuideId],
        name: &str,
    ) -> Result<(Sheet, Vec<Variable>, Vec<String>), CoreError> {
        let s = self.sheet(sheet)?;
        let fragment = self.copy_fragment_with_guides(sheet, points, ents, notches, &[], guides)?;
        let mut shell = fragment.as_sheet();
        shell.id = SheetId(self.next_id);
        shell.name = name.to_string();
        // Le presse-papiers fige toutes les formules ; ici elles sont reprises (voir plus bas).
        let mut notes: Vec<String> = fragment
            .notes
            .iter()
            .filter(|n| !n.contains("par une formule"))
            .cloned()
            .collect();
        // Formules maintenues : reprises pour les points qui sont restés construits.
        let kept: Vec<_> = s
            .formulas
            .iter()
            .filter(|f| {
                let original = s.point(f.point).map(|p| p.definition);
                let copy = shell.point(f.point).map(|p| p.definition);
                matches!((original, copy), (Ok(a), Ok(b)) if a == b)
            })
            .cloned()
            .collect();
        let in_copy = s
            .formulas
            .iter()
            .filter(|f| shell.point(f.point).is_ok())
            .count();
        if in_copy > kept.len() {
            notes.push(format!(
                "{} formule(s) non reprise(s) : le point dépend d'un objet qui n'est pas dans                  l'entrée, il est figé.",
                in_copy - kept.len()
            ));
        }
        shell.formulas = kept;
        let variables = needed_variables(self, shell.formulas.iter().map(|f| f.expr.as_str()))?;
        let imposed = s
            .length_constraints
            .iter()
            .filter(|c| ents.contains(&c.curve))
            .count();
        if imposed > 0 {
            notes.push(format!(
                "{imposed} longueur(s) imposée(s) ne sont pas reprises : la géométrie est \
                 conservée telle quelle."
            ));
        }
        Ok((shell, variables, notes))
    }

    /// Contenu « pièce » : la pièce, la géométrie dont elle dépend (fermée sous les
    /// dépendances), ses variables. Refus si une référence de la pièce ne peut pas être reprise.
    pub fn library_piece(&self, piece: PieceId) -> Result<LibraryContent, CoreError> {
        let p = self.piece(piece)?;
        let s = self.piece_sheet(piece)?;
        let (mut points, mut ents) = piece_refs(p, s);
        let mut guides = BTreeSet::new();
        close_selection(s, &mut points, &mut ents, &mut guides);
        let (points, ents, guides): (Vec<_>, Vec<_>, Vec<_>) = (
            points.into_iter().collect(),
            ents.into_iter().collect(),
            guides.into_iter().collect(),
        );
        let notches: Vec<NotchId> = s
            .notches
            .iter()
            .filter(|n| ents.contains(&n.entity))
            .map(|n| n.id)
            .collect();
        let name = if p.name.trim().is_empty() {
            "Pièce"
        } else {
            p.name.trim()
        };
        let (shell, variables, mut notes) =
            self.library_shell(s.id, &points, &ents, &notches, &guides, name)?;
        let mut np = p.clone();
        np.link = PieceLink::Linked { sheet: shell.id };
        np.placement = None;
        if np.derived.take().is_some() {
            notes.push("le lien avec la pièce mère n'est pas repris.".into());
        }
        let content = LibraryContent {
            kind: LibraryKind::Piece,
            next_id: self.next_id + 1,
            sheets: vec![shell],
            pieces: vec![np],
            variables,
            notes,
        };
        content.validate().map_err(|e| {
            invalid(format!(
                "la pièce « {} » ne peut pas être enregistrée : {e}",
                p.name
            ))
        })?;
        Ok(content)
    }

    /// Contenu « bloc » : la sélection (points, tracés, crans) d'une feuille, sans fermeture
    /// (un parent non choisi est figé), avec les variables de ses formules.
    pub fn library_block(
        &self,
        sheet: SheetId,
        points: &[PointId],
        entities: &[EntityId],
        notches: &[NotchId],
    ) -> Result<LibraryContent, CoreError> {
        let (shell, variables, notes) =
            self.library_shell(sheet, points, entities, notches, &[], "Bloc")?;
        let content = LibraryContent {
            kind: LibraryKind::Block,
            next_id: self.next_id + 1,
            sheets: vec![shell],
            pieces: Vec::new(),
            variables,
            notes,
        };
        content
            .validate()
            .map_err(|e| invalid(format!("le bloc ne peut pas être enregistré : {e}")))?;
        Ok(content)
    }

    /// Contenu « modèle » : feuilles, pièces et variables du projet ; ce qui vit au niveau du
    /// projet (tailles, variantes, coutures appariées, plans de coupe, barème, clients,
    /// retouches, mesures finies, règles) n'est pas repris et est compté dans les remarques.
    pub fn library_model(&self) -> Result<LibraryContent, CoreError> {
        let mut notes = Vec::new();
        let mut sheets = self.sheets.clone();
        let (mut grad, mut tops) = (0, 0);
        for s in &mut sheets {
            grad += s.gradings.len();
            tops += s.topstitches.len();
            s.gradings.clear();
            s.topstitches.clear();
        }
        let mut count = |n: usize, what: &str| {
            if n > 0 {
                notes.push(format!("{n} {what} non repris."));
            }
        };
        count(grad, "écart(s) de gradation");
        count(tops, "surpiqûre(s) liée(s)");
        count(self.sizes.len(), "gamme(s) de tailles");
        count(self.variants.len(), "variante(s)");
        count(self.seams.len(), "couture(s) appariée(s)");
        count(self.stitch_pairs.len(), "couple(s) de lignes de piqûre");
        count(self.cutting_plans.len(), "plan(s) de coupe");
        count(self.grading_rules.len(), "règle(s) de gradation");
        count(self.measurements.len(), "mesure(s) du barème");
        count(self.clients.len(), "fiche(s) client");
        count(self.alterations.len(), "retouche(s)");
        count(self.finished_measures.len(), "mesure(s) finie(s)");
        let per_size = self
            .variables
            .iter()
            .filter(|v| !v.per_size.is_empty())
            .count();
        count(
            per_size,
            "variable(s) avec des valeurs par taille (valeur de base seule)",
        );
        let mut detached = 0;
        let mut pieces = Vec::new();
        for p in &self.pieces {
            let mut np = p.clone();
            if np.link == PieceLink::Detached {
                let sheet = self.piece_sheet(p.id).map(|s| s.id).unwrap_or(sheets[0].id);
                np.link = PieceLink::Linked { sheet };
                detached += 1;
            }
            pieces.push(np);
        }
        count(detached, "pièce(s) détachée(s) reliée(s) à leur feuille");
        let variables = self
            .variables
            .iter()
            .map(|v| Variable {
                per_size: Vec::new(),
                ..v.clone()
            })
            .collect();
        let content = LibraryContent {
            kind: LibraryKind::Model,
            next_id: self.next_id,
            sheets,
            pieces,
            variables,
            notes,
        };
        content
            .validate()
            .map_err(|e| invalid(format!("le modèle ne peut pas être enregistré : {e}")))?;
        Ok(content)
    }

    /// Variables de l'entrée qui existent déjà ici avec une autre valeur, une autre unité ou une
    /// autre casse. Lecture seule : sert à prévenir avant l'insertion.
    pub fn library_conflicts(
        &self,
        content: &LibraryContent,
    ) -> Result<Vec<VariableConflict>, CoreError> {
        if content.variables.is_empty() {
            return Ok(Vec::new());
        }
        let incoming: BTreeMap<String, f64> = evaluate_variables(&content.variables)?
            .into_iter()
            .collect();
        let existing: BTreeMap<String, f64> = self.variable_values()?.into_iter().collect();
        let mut out = Vec::new();
        for v in &content.variables {
            let Some(other) = self
                .variables
                .iter()
                .find(|o| o.name.eq_ignore_ascii_case(&v.name))
            else {
                continue;
            };
            let (a, b) = (incoming[&v.name], existing[&other.name]);
            let same = other.name == v.name
                && other.unit == v.unit
                && (a - b).abs() <= SAME_VALUE * a.abs().max(b.abs()).max(1.0);
            if !same {
                out.push(VariableConflict {
                    name: v.name.clone(),
                    existing_name: other.name.clone(),
                    incoming_value: a,
                    existing_value: b,
                });
            }
        }
        Ok(out)
    }

    /// `InsertLibrary` : une seule étape. Variables d'abord (réutilisées si identiques, refus
    /// nommé en cas de conflit, ou renommées sur demande), puis géométrie et pièces avec de
    /// nouveaux identifiants.
    pub(crate) fn insert_library(
        &mut self,
        content: &LibraryContent,
        sheet: Option<SheetId>,
        layer: Option<LayerId>,
        rename: bool,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        content.validate()?;
        match (content.kind, sheet) {
            (LibraryKind::Block, None) => {
                return Err(invalid("un bloc s'insère dans une feuille : choisissez-la"))
            }
            (LibraryKind::Piece | LibraryKind::Model, Some(_)) => {
                return Err(invalid(
                    "une pièce ou un modèle s'insère sur de nouvelles feuilles : pas de feuille \
                     cible",
                ))
            }
            _ => {}
        }
        if layer.is_some() && content.kind != LibraryKind::Block {
            return Err(invalid("le calque ne concerne que l'insertion d'un bloc"));
        }
        let mut content = content.clone();
        let conflicts = self.library_conflicts(&content)?;
        if !conflicts.is_empty() {
            if !rename {
                let list = conflicts
                    .iter()
                    .map(|c| {
                        format!(
                            "« {} » (entrée : {}, projet : {})",
                            c.existing_name, c.incoming_value, c.existing_value
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(invalid(format!(
                    "variable(s) déjà présente(s) avec une autre valeur ou une autre unité : \
                     {list}. Renommez-les à l'insertion ou modifiez le projet."
                )));
            }
            for c in &conflicts {
                let new = self.free_variable_name(&content, &c.name)?;
                rename_in_content(&mut content, &c.name, &new)?;
                out.notes.push(format!(
                    "la variable « {} » de l'entrée est insérée sous le nom « {new} ».",
                    c.name
                ));
            }
        }
        let (mut added, mut reused) = (0, 0);
        for v in &content.variables {
            if self.variables.iter().any(|o| o.name == v.name) {
                reused += 1;
            } else {
                check_variable_name(&v.name)?;
                self.variables.push(Variable {
                    per_size: Vec::new(),
                    ..v.clone()
                });
                added += 1;
            }
        }
        if added + reused > 0 {
            out.notes.push(format!(
                "variables : {added} ajoutée(s), {reused} déjà présente(s) avec la même valeur."
            ));
        }
        match content.kind {
            LibraryKind::Block => {
                self.insert_block(&content, sheet.expect("vérifié"), layer, out)?
            }
            LibraryKind::Piece | LibraryKind::Model => {
                for shell in &content.sheets {
                    let pieces: Vec<Piece> = content
                        .pieces
                        .iter()
                        .filter(|p| p.link == PieceLink::Linked { sheet: shell.id })
                        .cloned()
                        .collect();
                    let mut tmp = Outcome::default();
                    let end = self.sheets.len();
                    let id = self.insert_sheet_copy(shell, &pieces, end, false, &mut tmp)?;
                    out.sheet.get_or_insert(id);
                    out.points.extend(tmp.points);
                    out.entities.extend(tmp.entities);
                    out.notches.extend(tmp.notches);
                    out.layers.extend(tmp.layers);
                    out.pieces.extend(tmp.pieces);
                    out.notes.extend(tmp.notes);
                }
            }
        }
        out.notes.extend(content.notes.iter().cloned());
        Ok(())
    }

    /// Premier nom libre `nom_2`, `nom_3`… (casse ignorée), dans le projet et dans l'entrée.
    fn free_variable_name(
        &self,
        content: &LibraryContent,
        base: &str,
    ) -> Result<String, CoreError> {
        let taken = |n: &str| {
            self.variables
                .iter()
                .map(|v| v.name.as_str())
                .chain(content.variables.iter().map(|v| v.name.as_str()))
                .any(|o| o.eq_ignore_ascii_case(n))
        };
        (2..1000)
            .map(|i| format!("{base}_{i}"))
            .find(|n| !taken(n) && check_variable_name(n).is_ok())
            .ok_or_else(|| invalid(format!("aucun nom libre pour la variable « {base} »")))
    }

    /// Insère le bloc dans la feuille `sheet` (collage du presse-papiers) puis reprend ses
    /// formules sur les nouveaux points.
    fn insert_block(
        &mut self,
        content: &LibraryContent,
        sheet: SheetId,
        layer: Option<LayerId>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let shell = &content.sheets[0];
        let fragment = Fragment {
            format: FRAGMENT_FORMAT.to_string(),
            version: FRAGMENT_VERSION,
            points: shell.points.clone(),
            entities: shell.entities.clone(),
            notches: shell.notches.clone(),
            dimensions: shell.dimensions.clone(),
            full_dimensions: shell.full_dimensions.clone(),
            groups: shell.groups.clone(),
            guides: shell.guides.clone(),
            notes: Vec::new(),
        };
        let mut tmp = Outcome::default();
        self.paste_fragment(sheet, &fragment, layer, &mut tmp)?;
        if tmp.points.len() != shell.points.len() {
            return Err(invalid("insertion incohérente : points manquants"));
        }
        let map: BTreeMap<PointId, PointId> = shell
            .points
            .iter()
            .map(|p| p.id)
            .zip(tmp.points.iter().copied())
            .collect();
        let mut target = self.sheet(sheet)?.clone();
        for f in &shell.formulas {
            let point = *map
                .get(&f.point)
                .ok_or_else(|| invalid("formule sur un point absent du bloc"))?;
            target.formulas.push(crate::formula::ParamFormula {
                point,
                param: f.param,
                expr: f.expr.clone(),
            });
        }
        let mut notes = Outcome::default();
        self.retain_adoptable_formulas(&mut target, &mut notes);
        self.sheet_mut(sheet)?.formulas = target.formulas;
        out.sheet = Some(sheet);
        out.points.extend(tmp.points);
        out.entities.extend(tmp.entities);
        out.notches.extend(tmp.notches);
        out.dimensions.extend(tmp.dimensions);
        out.full_dimensions.extend(tmp.full_dimensions);
        out.groups.extend(tmp.groups);
        out.guides.extend(tmp.guides);
        out.notes.extend(tmp.notes);
        out.notes.extend(notes.notes);
        Ok(())
    }
}

/// Renomme une variable de l'entrée dans toutes ses expressions (variables, formules de points,
/// cibles de longueur imposée).
fn rename_in_content(c: &mut LibraryContent, from: &str, to: &str) -> Result<(), CoreError> {
    let re = |text: &str| {
        crate::expr::rename_reference(text, from, to)
            .map_err(|e| invalid(format!("expression illisible : {e}")))
    };
    for v in &mut c.variables {
        v.expr = re(&v.expr)?;
        if v.name == from {
            v.name = to.to_string();
        }
    }
    for s in &mut c.sheets {
        for f in &mut s.formulas {
            f.expr = re(&f.expr)?;
        }
        for l in &mut s.length_constraints {
            if let Some(t) = &l.expr {
                l.expr = Some(re(t)?);
            }
            if let Some(link) = &mut l.link {
                if let Some(t) = &link.offset_expr {
                    link.offset_expr = Some(re(t)?);
                }
            }
        }
    }
    Ok(())
}

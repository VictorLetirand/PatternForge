//! US-10.1 et US-10.2 — commandes du barème de mesures et des fiches client, lectures dérivées.
//!
//! Le stockage est celui de `measurement.rs` (schéma 5, inchangé). Rien de dérivé n'est stocké :
//! les mesures **manquantes** d'un client et la **taille la plus proche** sont calculées à la
//! lecture ([`Project::client_missing_measurements`], [`Project::client_size_proposal`]) ; la
//! proposition est une information, jamais appliquée par une commande.
//!
//! Retirer une taille (ou une gamme) retire ses valeurs de barème dans la même étape ; retirer une
//! mesure retire ses valeurs dans les fiches client, ses retouches et ses mesures finies (avec
//! remarque), pour que la validation n'ait rien à refuser.

use crate::command::{Command, Outcome};
use crate::error::CoreError;
use crate::grading_rule::check_stored_name;
use crate::measurement::*;
use crate::model::*;
use crate::size::SizeError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Cellule d'une ligne importée : `None` = cellule vide.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasurementImportCell {
    pub size: SizeId,
    pub value: Option<f64>,
}

/// Ligne d'import du barème : une mesure (par son nom) et ses cellules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasurementImportRow {
    pub name: String,
    pub unit: MeasurementUnit,
    pub cells: Vec<MeasurementImportCell>,
}

/// Écart d'une mesure du client à la valeur d'une taille : `gap = client − taille` (> 0 : le client
/// est plus grand que la taille).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeGap {
    pub measurement: MeasurementId,
    pub client_value: f64,
    pub size_value: f64,
    pub gap: f64,
}

/// Comparaison du client à une taille.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeCandidate {
    pub size: SizeId,
    /// Un écart par mesure renseignée à la fois dans la fiche et dans la taille.
    pub gaps: Vec<SizeGap>,
    /// Mesures de la fiche sans valeur dans cette taille : la taille n'est alors pas comparable.
    pub missing_in_size: Vec<MeasurementId>,
    /// Somme des écarts absolus des mesures en mm ; `None` si la taille n'est pas comparable.
    pub score_mm: Option<f64>,
}

/// Proposition de taille (information, jamais appliquée).
#[derive(Debug, Clone, PartialEq)]
pub struct ClientSizeProposal {
    pub client: ClientId,
    pub range: SizeRangeId,
    /// Mesures du barème sans valeur dans la fiche (jamais devinées).
    pub missing: Vec<MeasurementId>,
    /// Une entrée par taille de la gamme, dans l'ordre de la gamme.
    pub candidates: Vec<SizeCandidate>,
    /// Taille comparable de plus petit score (à égalité, la première de la gamme).
    pub nearest: Option<SizeId>,
    /// Autres tailles au même score (à 1e-9 mm près que `nearest`).
    pub tied: Vec<SizeId>,
}

/// Tolérance d'égalité de deux scores (mm).
pub const PROPOSAL_TIE_MM: f64 = 1e-9;

fn bad(m: String) -> CoreError {
    CoreError::Invalid(m)
}

pub(crate) fn clean_name(name: &str, what: &str) -> Result<String, CoreError> {
    let n = name.trim().to_string();
    check_stored_name(&n, what)?;
    Ok(n)
}

fn check_value(v: f64, what: &str) -> Result<(), CoreError> {
    if !v.is_finite() {
        return Err(bad(format!("{what} : valeur non finie")));
    }
    if v < 0.0 {
        return Err(bad(format!("{what} : valeur négative refusée")));
    }
    if v > MAX_MEASUREMENT_VALUE {
        return Err(bad(format!(
            "{what} : valeur hors bornes ({MAX_MEASUREMENT_VALUE} au plus)"
        )));
    }
    Ok(())
}

pub(crate) fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl Project {
    pub fn measurement(&self, id: MeasurementId) -> Result<&Measurement, CoreError> {
        self.measurements
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| bad(format!("mesure introuvable (id {})", id.0)))
    }

    pub fn client(&self, id: ClientId) -> Result<&Client, CoreError> {
        self.clients
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| bad(format!("fiche client introuvable (id {})", id.0)))
    }

    fn measurement_mut(&mut self, id: MeasurementId) -> Result<&mut Measurement, CoreError> {
        self.measurements
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or_else(|| bad(format!("mesure introuvable (id {})", id.0)))
    }

    fn client_mut(&mut self, id: ClientId) -> Result<&mut Client, CoreError> {
        self.clients
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| bad(format!("fiche client introuvable (id {})", id.0)))
    }

    fn check_new_measurement_name(
        &self,
        name: &str,
        except: Option<MeasurementId>,
    ) -> Result<(), CoreError> {
        if self
            .measurements
            .iter()
            .any(|m| Some(m.id) != except && same_name(&m.name, name))
        {
            return Err(bad(format!(
                "une mesure « {name} » existe déjà (casse ignorée)"
            )));
        }
        Ok(())
    }

    fn check_new_client_name(&self, name: &str, except: Option<ClientId>) -> Result<(), CoreError> {
        if self
            .clients
            .iter()
            .any(|c| Some(c.id) != except && same_name(&c.name, name))
        {
            return Err(bad(format!(
                "une fiche client « {name} » existe déjà (casse ignorée)"
            )));
        }
        Ok(())
    }

    /// `AddMeasurement`.
    pub(crate) fn add_measurement(
        &mut self,
        name: &str,
        unit: MeasurementUnit,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let name = clean_name(name, "le nom de la mesure")?;
        self.check_new_measurement_name(&name, None)?;
        let id = MeasurementId(self.alloc_id());
        self.measurements.push(Measurement {
            id,
            name,
            unit,
            values: Vec::new(),
        });
        out.measurements.push(id);
        Ok(())
    }

    /// `RenameMeasurement` : même nom = aucun effet.
    pub(crate) fn rename_measurement(
        &mut self,
        id: MeasurementId,
        name: &str,
    ) -> Result<(), CoreError> {
        self.measurement(id)?;
        let name = clean_name(name, "le nom de la mesure")?;
        self.check_new_measurement_name(&name, Some(id))?;
        self.measurement_mut(id)?.name = name;
        Ok(())
    }

    /// `RemoveMeasurement` : retire aussi les références (valeurs de fiches client, retouches,
    /// mesures finies), avec une remarque qui les compte.
    pub(crate) fn remove_measurement(
        &mut self,
        id: MeasurementId,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let name = self.measurement(id)?.name.clone();
        self.measurements.retain(|m| m.id != id);
        let mut values = 0usize;
        for c in &mut self.clients {
            let before = c.values.len();
            c.values.retain(|v| v.measurement != id);
            values += before - c.values.len();
        }
        let alterations = self.alterations.len();
        self.alterations.retain(|a| a.target != id);
        let alterations = alterations - self.alterations.len();
        let finished = self.finished_measures.len();
        self.finished_measures.retain(|f| f.measurement != id);
        let finished = finished - self.finished_measures.len();
        if values + alterations + finished > 0 {
            out.notes.push(format!(
                "Mesure « {name} » retirée avec ses références : {values} valeur(s) de fiche \
                 client, {alterations} retouche(s), {finished} mesure(s) finie(s)."
            ));
        }
        Ok(())
    }

    /// `SetMeasurementValue` : valeur de la mesure pour une taille (même valeur = aucun effet).
    pub(crate) fn set_measurement_value(
        &mut self,
        id: MeasurementId,
        size: SizeId,
        value: f64,
    ) -> Result<(), CoreError> {
        let name = self.measurement(id)?.name.clone();
        if self.find_size(size).is_none() {
            return Err(SizeError::UnknownSize(size).into());
        }
        check_value(value, &format!("mesure « {name} »"))?;
        let m = self.measurement_mut(id)?;
        match m.values.iter_mut().find(|v| v.size == size) {
            Some(v) => v.value = value,
            None => {
                m.values.push(MeasurementValue { size, value });
                m.values.sort_by_key(|v| v.size);
            }
        }
        Ok(())
    }

    /// `ClearMeasurementValue` : la taille redevient non renseignée (déjà vide = aucun effet).
    pub(crate) fn clear_measurement_value(
        &mut self,
        id: MeasurementId,
        size: SizeId,
    ) -> Result<(), CoreError> {
        self.measurement(id)?;
        if self.find_size(size).is_none() {
            return Err(SizeError::UnknownSize(size).into());
        }
        self.measurement_mut(id)?.values.retain(|v| v.size != size);
        Ok(())
    }

    /// `ImportMeasurements` : fusion par nom (casse ignorée). Une ligne dont le nom existe met à
    /// jour cette mesure (unité identique exigée) ; sinon elle crée une mesure. Une cellule vide
    /// laisse la valeur existante, sauf `clear_empty` (elle l'efface). Tout ou rien.
    pub(crate) fn import_measurements(
        &mut self,
        rows: &[MeasurementImportRow],
        clear_empty: bool,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if rows.is_empty() {
            return Err(bad("import du barème : aucune ligne".into()));
        }
        let (mut created, mut updated) = (0usize, 0usize);
        let mut seen: Vec<String> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let what = format!("import du barème, ligne {}", i + 1);
            let name = clean_name(&row.name, &format!("{what} : le nom de la mesure"))?;
            if seen.iter().any(|s| same_name(s, &name)) {
                return Err(bad(format!(
                    "{what} : la mesure « {name} » est répétée (casse ignorée)"
                )));
            }
            seen.push(name.clone());
            let mut sizes = BTreeSet::new();
            for c in &row.cells {
                if self.find_size(c.size).is_none() {
                    return Err(SizeError::UnknownSize(c.size).into());
                }
                if !sizes.insert(c.size) {
                    return Err(bad(format!(
                        "{what} : deux cellules pour la taille {}",
                        c.size.0
                    )));
                }
                if let Some(v) = c.value {
                    check_value(v, &format!("{what}, mesure « {name} »"))?;
                }
            }
            let existing = self
                .measurements
                .iter()
                .find(|m| same_name(&m.name, &name))
                .map(|m| (m.id, m.unit));
            let id = match existing {
                Some((id, unit)) => {
                    if unit != row.unit {
                        return Err(bad(format!(
                            "{what} : la mesure « {name} » existe en {} et la ligne est en {}",
                            unit_text(unit),
                            unit_text(row.unit)
                        )));
                    }
                    updated += 1;
                    id
                }
                None => {
                    let id = MeasurementId(self.alloc_id());
                    self.measurements.push(Measurement {
                        id,
                        name,
                        unit: row.unit,
                        values: Vec::new(),
                    });
                    out.measurements.push(id);
                    created += 1;
                    id
                }
            };
            for c in &row.cells {
                match c.value {
                    Some(v) => self.set_measurement_value(id, c.size, v)?,
                    None if clear_empty => self.clear_measurement_value(id, c.size)?,
                    None => {}
                }
            }
        }
        out.notes.push(format!(
            "Import du barème : {created} mesure(s) créée(s), {updated} mise(s) à jour."
        ));
        Ok(())
    }

    /// `AddClient`.
    pub(crate) fn add_client(&mut self, name: &str, out: &mut Outcome) -> Result<(), CoreError> {
        let name = clean_name(name, "le nom de la fiche client")?;
        self.check_new_client_name(&name, None)?;
        let id = ClientId(self.alloc_id());
        self.clients.push(Client {
            id,
            name,
            values: Vec::new(),
        });
        out.clients.push(id);
        Ok(())
    }

    /// `RenameClient`.
    pub(crate) fn rename_client(&mut self, id: ClientId, name: &str) -> Result<(), CoreError> {
        self.client(id)?;
        let name = clean_name(name, "le nom de la fiche client")?;
        self.check_new_client_name(&name, Some(id))?;
        self.client_mut(id)?.name = name;
        Ok(())
    }

    /// `RemoveClient` : refusé tant qu'une taille générée depuis la fiche la cite (schéma v6,
    /// décision 456) ; la taille est nommée, rien n'est retiré ni réécrit en silence.
    pub(crate) fn remove_client(&mut self, id: ClientId) -> Result<(), CoreError> {
        let name = self.client(id)?.name.clone();
        if let Some((r, s)) = self
            .sizes
            .iter()
            .flat_map(|r| r.sizes.iter().map(move |s| (r, s)))
            .find(|(_, s)| s.client == Some(id))
        {
            return Err(bad(format!(
                "la fiche « {name} » est citée par la taille « {} » de la gamme « {} » : supprimez d'abord cette taille",
                s.label, r.name
            )));
        }
        self.clients.retain(|c| c.id != id);
        Ok(())
    }

    /// `SetClientValue`.
    pub(crate) fn set_client_value(
        &mut self,
        id: ClientId,
        measurement: MeasurementId,
        value: f64,
    ) -> Result<(), CoreError> {
        let name = self.client(id)?.name.clone();
        self.measurement(measurement)?;
        check_value(value, &format!("fiche « {name} »"))?;
        let c = self.client_mut(id)?;
        match c.values.iter_mut().find(|v| v.measurement == measurement) {
            Some(v) => v.value = value,
            None => {
                c.values.push(ClientValue { measurement, value });
                c.values.sort_by_key(|v| v.measurement);
            }
        }
        Ok(())
    }

    /// `ClearClientValue` : la mesure redevient manquante (déjà manquante = aucun effet).
    pub(crate) fn clear_client_value(
        &mut self,
        id: ClientId,
        measurement: MeasurementId,
    ) -> Result<(), CoreError> {
        self.client(id)?;
        self.measurement(measurement)?;
        self.client_mut(id)?
            .values
            .retain(|v| v.measurement != measurement);
        Ok(())
    }

    /// `GenerateClientSize` : taille « sur-mesure » dont la colonne du barème est celle de la fiche.
    /// Une mesure absente de la fiche laisse la cellule vide (jamais devinée ; une taille existante
    /// remplacée voit aussi ses anciennes cellules effacées). Seule la **référence** à la fiche est
    /// mémorisée (`Size.client`, schéma v6) : les valeurs ne le sont pas, la taille reste
    /// indépendante de la fiche une fois générée.
    pub(crate) fn generate_client_size(
        &mut self,
        client: ClientId,
        range: SizeRangeId,
        label: &str,
        replace: bool,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let c = self.client(client)?.clone();
        let r = self.size_range(range)?;
        let label = label.trim().to_string();
        let existing = r
            .sizes
            .iter()
            .find(|s| same_name(&s.label, &label))
            .map(|s| s.id);
        let size = match existing {
            Some(id) if replace => id,
            Some(_) => return Err(SizeError::SizeLabelTaken { label }.into()),
            None => {
                let cmd = Command::AddSize {
                    range,
                    label: label.clone(),
                    at: None,
                };
                self.apply_size_command(cmd, out)?;
                out.sizes
                    .last()
                    .copied()
                    .ok_or_else(|| bad("taille non créée".into()))?
            }
        };
        if let Some(s) = self
            .sizes
            .iter_mut()
            .flat_map(|r| r.sizes.iter_mut())
            .find(|s| s.id == size)
        {
            s.client = Some(client);
        }
        let ids: Vec<MeasurementId> = self.measurements.iter().map(|m| m.id).collect();
        let mut missing: Vec<String> = Vec::new();
        for id in ids {
            match c.values.iter().find(|v| v.measurement == id) {
                Some(v) => self.set_measurement_value(id, size, v.value)?,
                None => {
                    self.clear_measurement_value(id, size)?;
                    missing.push(self.measurement(id)?.name.clone());
                }
            }
        }
        let filled = self.measurements.len() - missing.len();
        let mut note = format!(
            "Taille « {label} » générée depuis la fiche « {} » : {filled} mesure(s) renseignée(s)",
            c.name
        );
        if missing.is_empty() {
            note.push('.');
        } else {
            note.push_str(&format!(
                ", {} manquante(s) laissée(s) vide(s) : {}.",
                missing.len(),
                missing.join(", ")
            ));
        }
        out.notes.push(note);
        Ok(())
    }

    /// Retire les valeurs de barème de ces tailles (suppression d'une taille ou d'une gamme),
    /// dans la même étape, avec une remarque.
    pub(crate) fn drop_size_measurement_values(
        &mut self,
        sizes: &BTreeSet<SizeId>,
        out: &mut Outcome,
    ) {
        let mut removed = 0usize;
        for m in &mut self.measurements {
            let before = m.values.len();
            m.values.retain(|v| !sizes.contains(&v.size));
            removed += before - m.values.len();
        }
        if removed > 0 {
            out.notes.push(format!(
                "{removed} valeur(s) du barème de mesures retirée(s) avec la taille supprimée ; \
                 les valeurs des autres tailles sont conservées."
            ));
        }
    }

    /// Mesures du barème sans valeur dans la fiche, dans l'ordre du barème. Lecture pure : rien
    /// n'est deviné, interpolé ni rempli.
    pub fn client_missing_measurements(
        &self,
        client: ClientId,
    ) -> Result<Vec<MeasurementId>, CoreError> {
        let c = self.client(client)?;
        Ok(self
            .measurements
            .iter()
            .filter(|m| !c.values.iter().any(|v| v.measurement == m.id))
            .map(|m| m.id)
            .collect())
    }

    /// Compare la fiche aux tailles de `range` (lecture pure, jamais appliquée). Une taille est
    /// **comparable** quand elle a une valeur pour **chaque** mesure renseignée dans la fiche ;
    /// son score est la somme des écarts absolus des mesures en mm (les degrés sont listés dans
    /// les écarts mais hors score). La plus proche est la comparable de plus petit score ; à égalité
    /// la première de la gamme, les autres sont dans `tied`. Rien si aucune n'est comparable.
    pub fn client_size_proposal(
        &self,
        client: ClientId,
        range: SizeRangeId,
    ) -> Result<ClientSizeProposal, CoreError> {
        let c = self.client(client)?;
        let r = self.size_range(range)?;
        let missing = self.client_missing_measurements(client)?;
        let mut candidates = Vec::new();
        for s in &r.sizes {
            let mut gaps = Vec::new();
            let mut missing_in_size = Vec::new();
            let mut score = 0.0;
            let mut mm_count = 0usize;
            for m in &self.measurements {
                let Some(cv) = c.values.iter().find(|v| v.measurement == m.id) else {
                    continue;
                };
                match m.values.iter().find(|v| v.size == s.id) {
                    Some(sv) => {
                        let gap = cv.value - sv.value;
                        if m.unit == MeasurementUnit::Mm {
                            score += gap.abs();
                            mm_count += 1;
                        }
                        gaps.push(SizeGap {
                            measurement: m.id,
                            client_value: cv.value,
                            size_value: sv.value,
                            gap,
                        });
                    }
                    None => missing_in_size.push(m.id),
                }
            }
            let comparable = missing_in_size.is_empty() && mm_count > 0;
            candidates.push(SizeCandidate {
                size: s.id,
                gaps,
                missing_in_size,
                score_mm: comparable.then_some(score),
            });
        }
        let best = candidates
            .iter()
            .filter_map(|c| c.score_mm.map(|s| (c.size, s)))
            .fold(None::<(SizeId, f64)>, |acc, (id, s)| match acc {
                Some((_, b)) if s >= b => acc,
                _ => Some((id, s)),
            });
        let (nearest, tied) = match best {
            Some((id, b)) => (
                Some(id),
                candidates
                    .iter()
                    .filter(|c| c.size != id)
                    .filter(|c| c.score_mm.is_some_and(|s| (s - b).abs() <= PROPOSAL_TIE_MM))
                    .map(|c| c.size)
                    .collect(),
            ),
            None => (None, Vec::new()),
        };
        Ok(ClientSizeProposal {
            client,
            range,
            missing,
            candidates,
            nearest,
            tied,
        })
    }
}

fn unit_text(u: MeasurementUnit) -> &'static str {
    match u {
        MeasurementUnit::Mm => "mm",
        MeasurementUnit::Deg => "deg",
    }
}

//! US-08.5 — rapport de contrôle : modèle de données **en lecture seule** (aucune `Command`,
//! aucun champ sérialisé) qui rassemble les lectures existantes (fiche de pièce, coutures
//! appariées US-08.1, crans US-08.4, raccords US-08.3). Le formatage (CSV, PDF) est dans
//! `pf-export::report` ; ce modèle ne contient aucun texte localisé.
//!
//! Toutes les valeurs sont des **mesures observées**, recalculées à chaque appel, jamais
//! stockées ; rien ici ne dit qu'un patron est « bon » : un statut dit seulement qu'une mesure
//! est dans ou hors de la tolérance déclarée.
//!
//! Conventions d'atelier (non validées) :
//! - aire et périmètre d'une pièce dont le contour n'est pas fermé : **absents** (pas de valeur) ;
//! - anomalies : une par élément fautif, triées par type (ordre de [`AnomalyKind`]), puis couple,
//!   pièce, côté, extrémité, cran ; un décalage de cran n'est signalé qu'une fois (côté A) ;
//! - un couple non calculable n'est signalé qu'une fois (pas de doublon crans / raccords).

use crate::model::*;
use crate::notch_check::{NotchCheckReading, NotchMatch};
use crate::piece::PieceIssue;
use crate::seam_junction::{JunctionEnd, JunctionIssue, JunctionReading, JunctionStatus};
use crate::seam_pair::{SeamIssue, SeamPairReading, SeamStatus, SeamWhich};

/// Une pièce du rapport.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportPiece {
    pub id: PieceId,
    /// Nom de la feuille qui porte la pièce (`None` : introuvable).
    pub sheet: Option<String>,
    pub name: String,
    pub code: Option<String>,
    pub category: Option<PieceCategory>,
    pub quantity: u32,
    pub material: Option<String>,
    /// Aire (mm²) ; `None` si le contour n'est pas fermé et mesurable.
    pub area_mm2: Option<f64>,
    /// Périmètre (mm) ; `None` si le contour n'est pas fermé et mesurable.
    pub perimeter_mm: Option<f64>,
    pub closed: bool,
    pub issues: Vec<PieceIssue>,
    /// Bords déclarés sur le pli (PH-5b) encore dans le contour, et leur longueur cumulée (mm).
    pub fold_edges: usize,
    pub fold_length_mm: f64,
}

/// Un couple cousu du rapport : noms de pièces, tracés des deux bords et lecture.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportSeam {
    pub piece_a: Option<String>,
    pub piece_b: Option<String>,
    pub edges_a: Vec<EntityId>,
    pub edges_b: Vec<EntityId>,
    pub reading: SeamPairReading,
}

/// Type d'anomalie ; l'ordre des variantes est l'ordre de tri du rapport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnomalyKind {
    SeamOutOfTolerance,
    SeamNotComputable,
    NotchMissing,
    NotchShifted,
    NotchNotComputable,
    JunctionBroken,
    JunctionNotComputable,
    PieceIssue,
}

impl AnomalyKind {
    pub const ALL: [AnomalyKind; 8] = [
        AnomalyKind::SeamOutOfTolerance,
        AnomalyKind::SeamNotComputable,
        AnomalyKind::NotchMissing,
        AnomalyKind::NotchShifted,
        AnomalyKind::NotchNotComputable,
        AnomalyKind::JunctionBroken,
        AnomalyKind::JunctionNotComputable,
        AnomalyKind::PieceIssue,
    ];

    /// Identifiant stable (colonne `type` du CSV, clés de langue).
    pub fn key(self) -> &'static str {
        match self {
            AnomalyKind::SeamOutOfTolerance => "seam_out",
            AnomalyKind::SeamNotComputable => "seam_na",
            AnomalyKind::NotchMissing => "notch_missing",
            AnomalyKind::NotchShifted => "notch_shifted",
            AnomalyKind::NotchNotComputable => "notch_na",
            AnomalyKind::JunctionBroken => "junction_broken",
            AnomalyKind::JunctionNotComputable => "junction_na",
            AnomalyKind::PieceIssue => "piece_issue",
        }
    }
}

/// Une anomalie du récapitulatif (valeurs brutes ; le texte est produit par le formateur).
#[derive(Debug, Clone, PartialEq)]
pub struct Anomaly {
    pub kind: AnomalyKind,
    /// Rang du couple (à partir de 1), pour les anomalies de couture, de cran et de raccord.
    pub pair: Option<usize>,
    pub side: Option<SeamWhich>,
    /// Extrémité de A (raccords).
    pub end: Option<JunctionEnd>,
    pub notch: Option<NotchId>,
    pub piece: Option<PieceId>,
    pub piece_name: Option<String>,
    pub issue: Option<PieceIssue>,
    /// Écart observé (mm pour une couture ou un cran, degrés pour un raccord).
    pub value: Option<f64>,
    pub tolerance: Option<f64>,
    /// Raison d'un « non calculable » (identifiant stable).
    pub reason: Option<&'static str>,
}

impl Anomaly {
    fn new(kind: AnomalyKind) -> Anomaly {
        Anomaly {
            kind,
            pair: None,
            side: None,
            end: None,
            notch: None,
            piece: None,
            piece_name: None,
            issue: None,
            value: None,
            tolerance: None,
            reason: None,
        }
    }
}

/// Le rapport de contrôle d'un projet.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlReport {
    pub project_name: String,
    pub schema_version: u32,
    pub sheet_count: usize,
    pub pieces: Vec<ReportPiece>,
    pub seams: Vec<ReportSeam>,
    pub notch_checks: Vec<NotchCheckReading>,
    pub junctions: Vec<JunctionReading>,
    /// Mesures finies (US-10.4) dans la taille de base de la première gamme, retouches comprises ;
    /// vide sans mesure finie. Pas d'anomalie associée (le récapitulatif reste inchangé).
    pub finished: Vec<crate::finished_measure::FinishedReading>,
    /// Contrôle des trous de piqûre des couples déclarés (US-08.6, tolérance par défaut) ; vide
    /// sans couple. Pas d'anomalie associée : le récapitulatif reste inchangé, les trous en trop
    /// ou manquants sont listés avec leur position dans la section.
    pub stitch_checks: Vec<crate::stitch_pair::StitchCheckReading>,
    /// Récapitulatif trié (voir l'en-tête du module) ; vide = aucune anomalie.
    pub anomalies: Vec<Anomaly>,
}

impl ControlReport {
    /// Nombre d'anomalies d'un type.
    pub fn count(&self, kind: AnomalyKind) -> usize {
        self.anomalies.iter().filter(|a| a.kind == kind).count()
    }
}

/// Clé de tri d'une anomalie : type, couple, rang de la pièce, côté, extrémité, cran.
type SortKey = (AnomalyKind, usize, usize, u8, u8, u64);

fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

fn side_rank(s: Option<SeamWhich>) -> u8 {
    match s {
        None => 0,
        Some(SeamWhich::A) => 1,
        Some(SeamWhich::B) => 2,
    }
}

fn end_rank(e: Option<JunctionEnd>) -> u8 {
    match e {
        None => 0,
        Some(JunctionEnd::Start) => 1,
        Some(JunctionEnd::End) => 2,
    }
}

impl Project {
    /// Rapport de contrôle : pièces, coutures, crans, raccords et anomalies (lecture seule,
    /// recalculé à chaque appel, le projet n'est pas modifié).
    pub fn control_report(&self) -> ControlReport {
        let name_of = |id: PieceId| self.piece(id).ok().map(|p| p.name.clone());
        let mut pieces = Vec::with_capacity(self.pieces.len());
        for p in &self.pieces {
            let closed = self.piece_contour(p.id).is_ok_and(|c| c.closed);
            let card = self.piece_card(p.id).ok();
            let (area, perimeter, issues) = match card {
                Some(c) => (c.area_mm2, c.perimeter_mm, c.issues),
                None => (None, None, Vec::new()),
            };
            let fold = self.piece_fold_summary(p.id);
            pieces.push(ReportPiece {
                fold_edges: fold.count,
                fold_length_mm: fold.length_mm,
                id: p.id,
                sheet: self.piece_sheet(p.id).ok().map(|s| s.name.clone()),
                name: p.name.clone(),
                code: p.code.clone(),
                category: p.category,
                quantity: p.quantity,
                material: p.material.clone(),
                area_mm2: area.and_then(finite).filter(|_| closed),
                perimeter_mm: perimeter.and_then(finite).filter(|_| closed),
                closed,
                issues,
            });
        }
        let readings = self.seam_pair_report();
        let seams: Vec<ReportSeam> = self
            .seams
            .iter()
            .zip(readings)
            .map(|(s, reading)| ReportSeam {
                piece_a: name_of(s.a.piece),
                piece_b: name_of(s.b.piece),
                edges_a: s.a.edges.clone(),
                edges_b: s.b.edges.clone(),
                reading,
            })
            .collect();
        let notch_checks = self.notch_check_report();
        let junctions = self.seam_junction_report();

        // Anomalies : générées par famille, puis triées (ordre stable, voir `AnomalyKind`).
        let mut found: Vec<(SortKey, Anomaly)> = Vec::new();
        let mut push = |pi: usize, a: Anomaly| {
            let key = (
                a.kind,
                a.pair.unwrap_or(0),
                pi,
                side_rank(a.side),
                end_rank(a.end),
                a.notch.map_or(0, |n| n.0),
            );
            found.push((key, a));
        };
        for (pi, p) in pieces.iter().enumerate() {
            for issue in &p.issues {
                let mut a = Anomaly::new(AnomalyKind::PieceIssue);
                a.piece = Some(p.id);
                a.piece_name = Some(p.name.clone());
                a.issue = Some(issue.clone());
                push(pi, a);
            }
        }
        let seam_computable: Vec<bool> = seams
            .iter()
            .map(|s| !matches!(s.reading.status, SeamStatus::NotComputable { .. }))
            .collect();
        for s in &seams {
            let r = &s.reading;
            let mut a = match r.status {
                SeamStatus::Within => continue,
                SeamStatus::OutOfTolerance => {
                    let mut a = Anomaly::new(AnomalyKind::SeamOutOfTolerance);
                    a.value = r.deviation_mm.and_then(finite);
                    a.tolerance = finite(r.tolerance_mm);
                    a
                }
                SeamStatus::NotComputable { side, reason } => {
                    let mut a = Anomaly::new(AnomalyKind::SeamNotComputable);
                    a.side = Some(side);
                    a.reason = Some(reason.key());
                    a
                }
            };
            a.pair = Some(r.number);
            push(0, a);
        }
        for (i, c) in notch_checks.iter().enumerate() {
            if let Some((side, _)) = c.not_computable {
                if seam_computable.get(i).copied().unwrap_or(true) {
                    let mut a = Anomaly::new(AnomalyKind::NotchNotComputable);
                    a.pair = Some(c.number);
                    a.side = Some(side);
                    a.reason = Some(SeamIssue::Degenerate.key());
                    push(0, a);
                }
                continue;
            }
            for it in &c.items {
                let kind = match it.status {
                    NotchMatch::Matched => continue,
                    NotchMatch::Missing => AnomalyKind::NotchMissing,
                    // Un décalage lie un cran de A et un cran de B : signalé une fois (côté A).
                    NotchMatch::Shifted if it.side == SeamWhich::A => AnomalyKind::NotchShifted,
                    NotchMatch::Shifted => continue,
                };
                let mut a = Anomaly::new(kind);
                a.pair = Some(c.number);
                a.side = Some(it.side);
                a.notch = Some(it.notch);
                a.value = it.gap_mm.and_then(finite);
                a.tolerance = finite(c.tolerance_mm);
                push(0, a);
            }
        }
        for j in &junctions {
            let mut a = match j.status {
                JunctionStatus::Smooth => continue,
                JunctionStatus::Broken => {
                    let mut a = Anomaly::new(AnomalyKind::JunctionBroken);
                    a.value = j.deviation_deg.and_then(finite);
                    a.tolerance = finite(j.tolerance_deg);
                    a
                }
                // Le couple lui-même non calculable est déjà signalé (couture).
                JunctionStatus::NotComputable {
                    reason: JunctionIssue::Seam(_),
                    ..
                } => continue,
                JunctionStatus::NotComputable { side, reason } => {
                    let mut a = Anomaly::new(AnomalyKind::JunctionNotComputable);
                    a.side = Some(side);
                    a.reason = Some(reason.key());
                    a
                }
            };
            a.pair = Some(j.number);
            a.end = Some(j.end);
            push(0, a);
        }
        found.sort_by_key(|(k, _)| *k);
        ControlReport {
            project_name: self.name.clone(),
            schema_version: self.schema_version,
            sheet_count: self.sheets.len(),
            pieces,
            seams,
            notch_checks,
            junctions,
            finished: self.finished_measure_base_readings(),
            stitch_checks: self.stitch_check_report(),
            anomalies: found.into_iter().map(|(_, a)| a).collect(),
        }
    }
}

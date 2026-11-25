//! Logique d'édition, indépendante de toute fenêtre : outils en machine à
//! états, sélection, saisie numérique, glisser. L'interface egui ne fait que
//! traduire souris/clavier en appels à [`Editor`] et dessiner son état.
//!
//! Tout est en millimètres ; la tolérance de visée (`tol_mm`) est fournie par
//! l'appelant, qui la déduit du zoom. Elle sert à *choisir* un objet, jamais à
//! calculer une cote.

use crate::dart::DartTransferUi;
use crate::text::{core_error, entity_name};
use pf_core::command::MERGE_TOLERANCE_MM;
use pf_core::dimension::DimensionGeometry;
use pf_core::input::{
    format_deg, format_mm, format_number, is_coordinate_text, parse_angle_deg, parse_coordinate,
    parse_division, CoordInput,
};
use pf_core::{
    measure, Command, CoreError, CurveHandle, DartId, DimensionId, Document, EntityId, EntityKind,
    Fragment, GroupId, Guide, GuideId, LayerId, MirrorAxis, NotchId, NotchStyle, Outcome, PathRole,
    PieceId, PieceSource, PointDefinition, PointId, PointRef, Project, SegmentEnd, Sheet, SheetId,
    SizeId, SplitPreview,
};
use pf_geometry::{
    Bbox, CornerStyle, PointMm, RegionError, Side, Trace, ANGLE_LOCK_STEP_DEG, EPSILON_MM,
};
use pf_i18n::{tr, tr_args};
use std::cell::RefCell;

/// Commandes encore permises quand le plan est en lecture seule (taille active ≠ base, US-09.2a) :
/// celles des tailles et de la gradation. Toute autre commande est refusée par `Editor::exec`.
pub(crate) fn size_view_allows(command: &Command) -> bool {
    matches!(
        command,
        Command::AddSizeRange { .. }
            | Command::RenameSizeRange { .. }
            | Command::RemoveSizeRange { .. }
            | Command::AddSize { .. }
            | Command::RemoveSize { .. }
            | Command::RenameSize { .. }
            | Command::MoveSize { .. }
            | Command::SetSizeBreak { .. }
            | Command::SetBaseSize { .. }
            | Command::SetPointGrading { .. }
            | Command::ClearPointGrading { .. }
            | Command::SetVariableSizeExpr { .. }
            | Command::ClearVariableSizeExpr { .. }
            | Command::AddMeasurement { .. }
            | Command::RenameMeasurement { .. }
            | Command::RemoveMeasurement { .. }
            | Command::SetMeasurementValue { .. }
            | Command::ClearMeasurementValue { .. }
            | Command::ImportMeasurements { .. }
            | Command::AddClient { .. }
            | Command::RenameClient { .. }
            | Command::RemoveClient { .. }
            | Command::SetClientValue { .. }
            | Command::ClearClientValue { .. }
            | Command::GenerateClientSize { .. }
            | Command::AddAlteration { .. }
            | Command::RenameAlteration { .. }
            | Command::RemoveAlteration { .. }
            | Command::SetAlterationTarget { .. }
            | Command::SetAlterationDelta { .. }
            | Command::AddFinishedMeasure { .. }
            | Command::SetFinishedMeasure { .. }
            | Command::RemoveFinishedMeasure { .. }
            | Command::AddVariant { .. }
            | Command::RenameVariant { .. }
            | Command::RemoveVariant { .. }
            | Command::DuplicateVariant { .. }
            | Command::AddVariantEntry { .. }
            | Command::UpdateVariantEntry { .. }
            | Command::RemoveVariantEntry { .. }
            | Command::AddVariantOption { .. }
            | Command::RemoveVariantOption { .. }
            | Command::RenameVariantOption { .. }
            | Command::MoveVariantOption { .. }
            | Command::SetVariantOptionActive { .. }
    )
}

/// Polylignes d'un dessin (aperçu du décalage).
pub type Lines = Vec<Vec<PointMm>>;
/// Clé de l'aperçu du décalage : distance, coins, nombre de tracés choisis et de la feuille.
type OffsetKey = (u64, CornerStyle, usize, usize);

/// Longueur d'une tangente posée sans saisie (mm).
const DEFAULT_TANGENT_LENGTH_MM: f64 = 50.0;
/// Longueur maximale (caractères) d'un champ de la bulle de saisie.
const MAX_ENTRY_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Point,
    Segment,
    Rectangle,
    Perpendicular,
    Parallel,
    PointOnSegment,
    Notch,
    Grainline,
    Curve,
    /// Courbe lisse par points de passage (US-02.2).
    Spline,
    Circle,
    Arc,
    /// Congé entre deux tracés (US-02.3).
    Fillet,
    /// Chanfrein entre deux segments (US-02.3).
    Chamfer,
    /// Tangente : depuis un point à un cercle/arc, ou en un point d'une courbe (US-02.3).
    Tangent,
    /// Prolonger ou raccourcir un segment ou un arc (US-02.7).
    Extend,
    /// Couper un tracé en un point (US-02.7).
    Split,
    /// Fusionner deux tracés bout à bout (US-02.7).
    Merge,
    /// Décalage d'une chaîne de tracés (US-02.10).
    Offset,
    Measure,
    Dimension,
    /// Ligne d'aide infinie (US-02.8).
    Guide,
    /// Extraire une pièce d'une zone fermée (US-04.1).
    ExtractPiece,
    /// Pince (US-07.1) : sommet, bord de la base, ouverture.
    Dart,
    /// Décalque assisté sur l'image de fond (US-14.2).
    Trace,
}

/// Ouverture trouvée par l'outil Extraire une pièce : où le contour ne se ferme pas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PieceGap {
    pub from: PointMm,
    pub to: PointMm,
    pub gap_mm: f64,
}

/// Séparation d'une pièce en cours (US-04.6) : la pièce visée et le premier point de la
/// ligne de coupe (une droite infinie), posé au premier clic. État d'interface, jamais enregistré.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitJob {
    pub piece: PieceId,
    pub first: Option<PointMm>,
}

/// Aperçu de la ligne de coupe sous le curseur : le résultat de `Project::split_preview`
/// (mesures observées des deux moitiés) ou la raison du refus.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitDraft {
    pub from: PointMm,
    pub to: PointMm,
    pub preview: Result<SplitPreview, String>,
}

/// Sous-mode de l'outil Ligne d'aide (US-02.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GuideMode {
    /// Horizontale : un clic (ou une valeur Y saisie).
    #[default]
    Horizontal,
    /// Verticale : un clic (ou une valeur X saisie).
    Vertical,
    /// Oblique : un point de passage, puis un angle saisi ou un second clic.
    Oblique,
}

impl GuideMode {
    pub const ALL: [GuideMode; 3] = [
        GuideMode::Horizontal,
        GuideMode::Vertical,
        GuideMode::Oblique,
    ];

    pub fn label(self) -> String {
        match self {
            GuideMode::Horizontal => tr("guide.mode.horizontal"),
            GuideMode::Vertical => tr("guide.mode.vertical"),
            GuideMode::Oblique => tr("guide.mode.oblique"),
        }
    }
}

/// Sous-mode de création des outils Cercle et Arc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoundMode {
    /// Cercle : centre + rayon. Arc : centre + angles de début et de fin.
    #[default]
    Center,
    /// Cercle ou arc passant par trois points.
    ThreePoints,
    /// Cercle ou arc de rayon donné passant par deux points.
    TwoPointsRadius,
}

/// Sous-mode de l'outil Point (US-02.5) : `Free` pose un point comme avant, les
/// autres créent un point relatif **maintenu** (il suit ses parents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointMode {
    #[default]
    Free,
    /// Distance le long d'une courbe (longueur d'arc).
    AlongCurve,
    /// dx / dy depuis un point.
    Offset,
    /// Milieu de deux points.
    Midpoint,
    /// Projection orthogonale d'un point sur un tracé.
    Projection,
    /// Bissectrice d'un angle (sommet, deux points), à distance donnée.
    Bisector,
    /// Distance et angle absolu depuis un point.
    Direction,
    /// Distance et angle relatif à un segment depuis un point.
    DirectionRelative,
    /// Intersection de deux arcs de compas (centres, rayons).
    Compass,
    /// Division d'un tracé en parts égales ou proportionnelles (US-02.6) ; cercle :
    /// un clic sur un point après le cercle fixe le point de départ.
    Division,
}

/// Ce que le sous-mode attend comme prochain clic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    Point,
    Trace,
    Segment,
}

/// Parent désigné par un clic, en attendant la création du point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RelPick {
    Point(PointId),
    Trace(EntityId),
}

impl PointMode {
    pub const ALL: [PointMode; 10] = [
        PointMode::Free,
        PointMode::AlongCurve,
        PointMode::Offset,
        PointMode::Midpoint,
        PointMode::Projection,
        PointMode::Bisector,
        PointMode::Direction,
        PointMode::DirectionRelative,
        PointMode::Compass,
        PointMode::Division,
    ];

    pub fn label(self) -> String {
        match self {
            PointMode::Free => tr("point.mode.free"),
            PointMode::AlongCurve => tr("point.mode.along_curve"),
            PointMode::Offset => tr("point.mode.offset"),
            PointMode::Midpoint => tr("point.mode.midpoint"),
            PointMode::Projection => tr("point.mode.projection"),
            PointMode::Bisector => tr("point.mode.bisector"),
            PointMode::Direction => tr("point.mode.direction"),
            PointMode::DirectionRelative => tr("point.mode.direction_relative"),
            PointMode::Compass => tr("point.mode.compass"),
            PointMode::Division => tr("point.mode.division"),
        }
    }

    /// Parents à désigner, dans l'ordre.
    pub fn picks(self) -> &'static [PickKind] {
        use PickKind::{Point, Segment, Trace};
        match self {
            PointMode::Free => &[],
            PointMode::AlongCurve | PointMode::Division => &[Trace],
            PointMode::Offset | PointMode::Direction => &[Point],
            PointMode::Midpoint | PointMode::Compass => &[Point, Point],
            PointMode::Projection => &[Point, Trace],
            PointMode::Bisector => &[Point, Point, Point],
            PointMode::DirectionRelative => &[Point, Segment],
        }
    }

    /// Vrai si des valeurs sont à saisir (palette) après les parents.
    pub fn needs_values(self) -> bool {
        !matches!(
            self,
            PointMode::Free | PointMode::Midpoint | PointMode::Projection
        )
    }
}

impl RoundMode {
    pub const ALL: [RoundMode; 3] = [
        RoundMode::Center,
        RoundMode::ThreePoints,
        RoundMode::TwoPointsRadius,
    ];

    pub fn label(self, tool: Tool) -> String {
        match (self, tool) {
            (RoundMode::Center, Tool::Arc) => tr("round.mode.arc_center"),
            (RoundMode::Center, _) => tr("round.mode.circle_center"),
            (RoundMode::ThreePoints, _) => tr("round.mode.three_points"),
            (RoundMode::TwoPointsRadius, _) => tr("round.mode.two_points_radius"),
        }
    }
}

impl Tool {
    pub const ALL: [Tool; 26] = [
        Tool::Select,
        Tool::Point,
        Tool::Segment,
        Tool::Rectangle,
        Tool::Perpendicular,
        Tool::Parallel,
        Tool::PointOnSegment,
        Tool::Notch,
        Tool::Grainline,
        Tool::Curve,
        Tool::Spline,
        Tool::Circle,
        Tool::Arc,
        Tool::Fillet,
        Tool::Chamfer,
        Tool::Tangent,
        Tool::Extend,
        Tool::Split,
        Tool::Merge,
        Tool::Offset,
        Tool::Measure,
        Tool::Dimension,
        Tool::Guide,
        Tool::ExtractPiece,
        Tool::Dart,
        Tool::Trace,
    ];

    pub fn label(self) -> String {
        match self {
            Tool::Select => tr("tool.select.label"),
            Tool::Point => tr("tool.point.label"),
            Tool::Segment => tr("tool.segment.label"),
            Tool::Rectangle => tr("tool.rectangle.label"),
            Tool::Perpendicular => tr("tool.perpendicular.label"),
            Tool::Parallel => tr("tool.parallel.label"),
            Tool::PointOnSegment => tr("tool.point_on_segment.label"),
            Tool::Notch => tr("tool.notch.label"),
            Tool::Grainline => tr("tool.grainline.label"),
            Tool::Curve => tr("tool.curve.label"),
            Tool::Spline => tr("tool.spline.label"),
            Tool::Circle => tr("tool.circle.label"),
            Tool::Arc => tr("tool.arc.label"),
            Tool::Fillet => tr("tool.fillet.label"),
            Tool::Chamfer => tr("tool.chamfer.label"),
            Tool::Tangent => tr("tool.tangent.label"),
            Tool::Extend => tr("tool.extend.label"),
            Tool::Split => tr("tool.split.label"),
            Tool::Merge => tr("tool.merge.label"),
            Tool::Offset => tr("tool.offset.label"),
            Tool::Measure => tr("tool.measure.label"),
            Tool::Dimension => tr("tool.dimension.label"),
            Tool::Guide => tr("tool.guide.label"),
            Tool::ExtractPiece => tr("tool.extract_piece.label"),
            Tool::Dart => tr("tool.dart.label"),
            Tool::Trace => tr("tool.trace.label"),
        }
    }

    /// Lettre courante du raccourci (table personnalisable).
    pub fn shortcut(self) -> String {
        crate::shortcuts::current().label(self)
    }
}

/// Étape en cours de l'outil actif.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolState {
    Idle,
    /// Segment : départ posé, extrémité en aperçu.
    SegmentFrom {
        start: PointRef,
    },
    /// Rectangle : premier coin posé, coin opposé en aperçu.
    RectFrom {
        start: PointRef,
    },
    /// Parallèle : segment de référence choisi, distance en aperçu.
    ParallelOf {
        reference: EntityId,
    },
    /// Perpendiculaire : référence choisie, départ à poser.
    PerpPickStart {
        reference: EntityId,
    },
    /// Perpendiculaire : départ posé, longueur en aperçu.
    PerpFrom {
        reference: EntityId,
        start: PointRef,
    },
    /// Cran : tracé porteur choisi, position en aperçu.
    NotchOn {
        reference: EntityId,
    },
    /// Point à distance : segment choisi, distance en aperçu.
    PointOn {
        segment: EntityId,
        origin: SegmentEnd,
    },
    /// Courbe : départ posé.
    CurveFrom {
        start: PointRef,
    },
    /// Courbe lisse : au moins un point posé (voir `Editor::spline_points`).
    SplineNext,
    /// Cercle (centre + rayon) : centre posé, rayon en aperçu.
    CircleCenter {
        center: PointRef,
    },
    /// Arc (centre + angles) : centre posé, rayon et angle de début en aperçu.
    ArcCenter {
        center: PointRef,
    },
    /// Arc (centre + angles) : début fixé, angle de fin en aperçu.
    ArcSweep {
        center: PointRef,
        radius_mm: f64,
        start_deg: f64,
    },
    /// Cercle ou arc par 3 points / 2 points + rayon : premier point posé.
    RoundFirst {
        first: PointRef,
    },
    /// Idem : deuxième point posé, troisième point (ou rayon) en aperçu.
    RoundSecond {
        first: PointRef,
        second: PointRef,
    },
    /// Congé / chanfrein : premier tracé choisi, au point visé `pick`.
    JoinFirst {
        first: EntityId,
        pick: PointMm,
    },
    /// Congé / chanfrein : les deux tracés choisis ; rayon ou distance à saisir.
    JoinReady {
        first: EntityId,
        pick_first: PointMm,
        second: EntityId,
        pick_second: PointMm,
    },
    /// Tangente : point d'origine posé, cercle ou arc à désigner.
    TangentFrom {
        from: PointRef,
    },
    /// Tangente : courbe choisie, position et sens suivent la souris.
    TangentAt {
        entity: EntityId,
    },
    /// Prolonger : tracé et extrémité choisis ; valeur à saisir, ou autre tracé à cliquer.
    ExtendFrom {
        entity: EntityId,
        end: SegmentEnd,
    },
    /// Fusionner : premier tracé choisi.
    MergeFirst {
        first: EntityId,
    },
    /// Pince (US-07.1) : sommet posé dans `piece`, bord de la base à cliquer.
    DartApex {
        piece: PieceId,
        apex: PointRef,
    },
    /// Pince : sommet et pied de la perpendiculaire au bord choisis ; ouverture (et longueur) à
    /// saisir, ou suivie à la souris.
    DartBase {
        piece: PieceId,
        apex: PointRef,
        base: PointRef,
    },
    /// Décalage : chaîne choisie (`Editor::offset_paths`), distance saisie ou
    /// suivie à la souris, côté suivant la souris.
    OffsetReady,
    /// Point relatif en cours (US-02.5) : parents déjà désignés dans
    /// `Editor::rel_picks`, valeurs éventuelles dans la palette.
    RelativePick,
    /// Mesure : premier point posé.
    MeasureFrom {
        start: PointMm,
    },
    /// Cotation : premier point choisi, second point et décalage en aperçu.
    DimensionFrom {
        from: PointId,
    },
    /// Ligne d'aide oblique : point de passage posé, angle à saisir ou second clic.
    GuideFrom {
        through: PointMm,
    },
}

/// Ce qui se trouve sous le curseur.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    Point(PointId),
    Entity(EntityId),
    Notch(NotchId),
    Dimension(DimensionId),
    Handle(EntityId, CurveHandle),
    Guide(GuideId),
}

/// Nature d'un accrochage, par priorité décroissante.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    /// Point existant (extrémité, point construit).
    Point,
    /// Intersection de deux segments.
    Intersection,
    /// Milieu d'un segment.
    Midpoint,
    /// Point quelconque d'un segment ou d'une courbe.
    OnPath,
    /// Point quelconque d'une ligne d'aide (US-02.8).
    OnGuide,
    /// Aucun objet : position libre (aimantée à la grille si elle est active).
    Free,
}

impl SnapKind {
    pub fn label(self) -> String {
        match self {
            SnapKind::Point => tr("snap.point"),
            SnapKind::Intersection => tr("snap.intersection"),
            SnapKind::Midpoint => tr("snap.midpoint"),
            SnapKind::OnPath => tr("snap.on_path"),
            SnapKind::OnGuide => tr("snap.on_guide"),
            SnapKind::Free => String::new(),
        }
    }
}

/// Résultat d'une visée : où l'on accroche et quel point cela désigne.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub kind: SnapKind,
    pub pos: PointMm,
    pub target: PointRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection {
    pub points: Vec<PointId>,
    pub entities: Vec<EntityId>,
    pub notches: Vec<NotchId>,
    pub dimensions: Vec<DimensionId>,
    pub guides: Vec<GuideId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
            && self.entities.is_empty()
            && self.notches.is_empty()
            && self.dimensions.is_empty()
            && self.guides.is_empty()
    }
    pub fn clear(&mut self) {
        self.points.clear();
        self.entities.clear();
        self.notches.clear();
        self.dimensions.clear();
        self.guides.clear();
    }
    pub fn len(&self) -> usize {
        self.points.len()
            + self.entities.len()
            + self.notches.len()
            + self.dimensions.len()
            + self.guides.len()
    }
    fn toggle(&mut self, target: Target) {
        match target {
            Target::Point(id) => toggle(&mut self.points, id),
            Target::Entity(id) => toggle(&mut self.entities, id),
            Target::Notch(id) => toggle(&mut self.notches, id),
            Target::Dimension(id) => toggle(&mut self.dimensions, id),
            Target::Guide(id) => toggle(&mut self.guides, id),
            Target::Handle(..) => {}
        }
    }
    fn set(&mut self, target: Target) {
        self.clear();
        self.toggle(target);
    }
    pub(crate) fn retain_valid(&mut self, sheet: &Sheet, show_guides: bool) {
        self.guides
            .retain(|g| show_guides && sheet.guide(*g).is_ok_and(|g| sheet.guide_visible(g)));
        // Un objet d'un calque masqué ne peut pas rester sélectionné.
        self.points
            .retain(|p| sheet.point(*p).is_ok() && sheet.point_visible(*p));
        self.entities
            .retain(|e| sheet.entity(*e).is_ok() && sheet.entity_visible(*e));
        self.notches
            .retain(|n| sheet.notch(*n).is_ok_and(|n| sheet.notch_visible(n)));
        self.dimensions.retain(|d| {
            sheet
                .dimension(*d)
                .is_ok_and(|d| sheet.dimension_visible(d))
        });
    }
}

/// Valeur saisie d'un paramètre : `expr` = formule à maintenir (US-03.1, formule à la création).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TypedParam {
    pub value: f64,
    pub expr: Option<String>,
}

/// Vrai si la commande peut créer des points ou des tracés (rangés alors
/// dans le calque actif).
fn creates_objects(command: &Command) -> bool {
    match command {
        Command::AddPoint { .. }
        | Command::AddPointAt { .. }
        | Command::AddPointWithFormulas { .. }
        | Command::AddSegment { .. }
        | Command::AddSegmentPolar { .. }
        | Command::AddPerpendicular { .. }
        | Command::AddRectangle { .. }
        | Command::AddConstructedRectangle { .. }
        | Command::AddParallel { .. }
        | Command::AddCurve { .. }
        | Command::AddSpline { .. }
        | Command::AddSplinePoint { .. }
        | Command::AddCircle { .. }
        | Command::AddCircleThreePoints { .. }
        | Command::AddCircleTwoPointsRadius { .. }
        | Command::AddArc { .. }
        | Command::AddArcThreePoints { .. }
        | Command::AddArcTwoPointsRadius { .. }
        | Command::AddPointOnSegment { .. }
        | Command::AddGuide { .. }
        | Command::OffsetPaths { .. }
        | Command::Duplicate { .. } => true,
        Command::Batch { commands } => commands.iter().any(creates_objects),
        _ => false,
    }
}

fn toggle<T: PartialEq>(v: &mut Vec<T>, id: T) {
    match v.iter().position(|x| *x == id) {
        Some(i) => {
            v.remove(i);
        }
        None => v.push(id),
    }
}

/// Aperçu de l'opération en cours : sert à la fois au dessin et à la
/// validation, pour que ce qui est montré soit exactement ce qui sera créé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Preview {
    Segment {
        from: PointMm,
        to: PointMm,
        length_mm: f64,
        angle_deg: f64,
    },
    Perpendicular {
        from: PointMm,
        to: PointMm,
        length_mm: f64,
        side: Side,
    },
    Rectangle {
        from: PointMm,
        to: PointMm,
        width_mm: f64,
        height_mm: f64,
    },
    Parallel {
        from: PointMm,
        to: PointMm,
        distance_mm: f64,
        side: Side,
    },
    PointOn {
        origin: PointMm,
        at: PointMm,
        distance_mm: f64,
    },
    Notch {
        base: PointMm,
        distance_mm: f64,
        side: Side,
        length_mm: f64,
        style: NotchStyle,
    },
    Curve {
        from: PointMm,
        to: PointMm,
    },
    /// Cercle en cours de création (centre et rayon résolus).
    Circle {
        center: PointMm,
        radius_mm: f64,
    },
    /// Arc en cours de création (centre, rayon, angles en degrés, sens).
    Arc {
        center: PointMm,
        radius_mm: f64,
        start_deg: f64,
        end_deg: f64,
        ccw: bool,
    },
    Measure {
        from: PointMm,
        to: PointMm,
        distance_mm: f64,
    },
    /// Ligne d'aide oblique en cours : point de passage et angle.
    Guide {
        through: PointMm,
        angle_deg: f64,
    },
    /// Cotation (mesure affichée) : points cotés et décalage signé.
    Dimension {
        from: PointId,
        to: PointId,
        geometry: DimensionGeometry,
        offset_mm: f64,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub text: String,
    pub error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    Point(PointId),
    /// Point construit sur un tracé : glisse le long de ce tracé.
    Slide(PointId),
    Handle(EntityId, CurveHandle),
    Guide(GuideId),
}

/// Outil de création à réactiver par « Répéter la dernière commande » (US-16.2) :
/// l'outil, ses sous-modes et les dernières valeurs saisies. Aucune commande ni
/// référence à un objet n'est mémorisée : seule la configuration de l'outil.
#[derive(Debug, Clone, PartialEq)]
pub struct RepeatMemo {
    pub tool: Tool,
    pub point_mode: PointMode,
    pub round_mode: RoundMode,
    pub guide_mode: GuideMode,
    pub length_text: String,
    pub angle_text: String,
}

/// Un champ de la bulle de saisie près du curseur (US-16.2).
#[derive(Debug, Clone, PartialEq)]
pub struct EntryRow {
    pub label: String,
    pub unit: &'static str,
    /// Texte tapé (vide = rien saisi : la valeur affichée est la mesure observée).
    pub typed: String,
    /// Mesure observée en temps réel (aperçu), mise en forme.
    pub observed: Option<String>,
    /// Champ qui reçoit les touches tapées.
    pub active: bool,
}

pub struct Editor {
    pub doc: Document,
    pub sheet: SheetId,
    pub tool: Tool,
    pub state: ToolState,
    pub selection: Selection,
    /// Filtre de sélection persistant (US-16.1), non enregistré dans le projet.
    pub select_filter: crate::select_ops::SelectionFilter,
    /// Le glisser de l'outil Sélection trace un lasso au lieu d'un rectangle (US-16.1).
    pub lasso_mode: bool,
    /// Champs de la palette de saisie. Vide = la valeur suit la souris ;
    /// rempli = la valeur est imposée par la saisie.
    pub length_text: String,
    pub angle_text: String,
    /// Saisie directe d'une position (`x;y`, `@dx;dy`, `@L<A`) avant le premier
    /// point d'un outil (US-16.2). Non enregistrée ; vide hors de cette saisie.
    pub position_text: String,
    /// Champ de la bulle qui reçoit les touches (0 = premier, 1 = second).
    pub entry_field: usize,
    /// Maj enfoncée : la direction des tracés obliques est verrouillée au pas de 15°.
    pub angle_lock: bool,
    /// Dernier point posé (créé ou saisi), origine des coordonnées relatives
    /// hors tracé en cours ; réinitialisé à l'ouverture, à l'annulation, au changement de feuille.
    pub last_placed: Option<PointMm>,
    /// Dernier outil de création abouti, pour « Répéter la dernière commande ».
    pub repeat_memo: Option<RepeatMemo>,
    /// Inverse le côté proposé pour la perpendiculaire (et la jambe fixe d'une pince : droite).
    pub perp_flip: bool,
    /// Case « Conserver la construction » de l'outil Rectangle (US-03.2b) : état d'interface,
    /// jamais écrit dans le projet ; le rectangle créé est alors maintenu.
    pub rect_kept: bool,
    /// Pince sélectionnée par l'outil Pince : aperçu fermé et mesures (état d'interface).
    pub dart_selected: Option<(PieceId, DartId)>,
    /// Transfert de la pince sélectionnée (US-07.2) : en attente du clic sur le bord cible,
    /// mode partiel et part saisie en pourcentage (état d'interface).
    pub dart_transfer: DartTransferUi,
    /// Décalque assisté (US-14.2) : points posés, accrochage aux contours, lissage.
    pub trace: crate::trace::TraceState,
    /// Fenêtre « Plis » (US-07.3), état d'interface.
    pub pleat: crate::pleat::PleatUi,
    /// Fenêtre « Ampleur » (US-07.4), état d'interface.
    pub flare: crate::flare::FlareUi,
    /// Fenêtre « Découper… » (US-07.5), état d'interface.
    pub cutline: crate::cutline::CutUi,
    /// Fenêtre « Construction rapide » (PH-4a), état d'interface.
    pub quick: crate::quick::QuickUi,
    /// Fenêtre « Coutures… » (US-08.1), état d'interface.
    pub seam: crate::seams::SeamUi,
    /// Le prochain cran posé est un cran double.
    pub notch_double: bool,
    /// Sous-mode des outils Cercle et Arc.
    pub round_mode: RoundMode,
    /// Sous-mode de l'outil Point (US-02.5).
    pub point_mode: PointMode,
    /// Sous-mode de l'outil Ligne d'aide (US-02.8).
    pub guide_mode: GuideMode,
    /// Affichage et accrochage des lignes d'aide (réglage d'affichage, non enregistré).
    pub show_guides: bool,
    /// Règles graduées sur les bords du canevas (affichage seul).
    pub show_rulers: bool,
    /// Parents déjà désignés pour le point relatif en cours (non enregistrés).
    pub rel_picks: Vec<RelPick>,
    /// Point à distance d'une courbe : mesurer depuis la fin plutôt que le départ.
    pub rel_from_end: bool,
    /// Points de passage posés par l'outil Courbe lisse (non enregistrés).
    pub spline_points: Vec<PointRef>,
    /// Décalage (US-02.10) : chaîne choisie, côté inversé, traitement des coins
    /// et rôle du résultat (réglages d'outil, non enregistrés).
    pub offset_paths: Vec<EntityId>,
    pub offset_flip: bool,
    pub offset_corner: CornerStyle,
    pub offset_role: PathRole,
    /// Dernier aperçu de décalage calculé : `(distance, coins, nombre de tracés)` → lignes.
    offset_cache: RefCell<Option<(OffsetKey, Lines)>>,
    /// Dernière position du curseur dans le monde (mm).
    pub cursor: PointMm,
    /// Pas d'aimantation des clics libres (mm) ; `None` = désactivé.
    pub grid_snap_mm: Option<f64>,
    /// Accrochage aux milieux, intersections et tracés (les points existants
    /// accrochent toujours).
    pub snap_to_paths: bool,
    /// Rayon de visée courant (mm), fourni par le canevas d'après le zoom.
    pub pick_tol_mm: f64,
    /// Dernière distance mesurée (lecture seule, non enregistrée).
    pub last_measure: Option<(PointMm, PointMm)>,
    pub status: Status,
    /// Calque actif : reçoit les nouveaux objets (état d'éditeur, non enregistré).
    pub active_layer: Option<LayerId>,
    /// Outil Extraire une pièce : créer une copie détachée (feuille dédiée) au lieu
    /// d'une pièce liée à la construction (réglage d'outil, non enregistré).
    pub piece_detached: bool,
    /// Nommer les nouvelles pièces avec la règle de nommage (US-11.5) ; décoché par défaut, jamais
    /// enregistré.
    pub piece_use_rule: bool,
    /// Idem pour les nouvelles feuilles.
    pub sheet_use_rule: bool,
    /// Dernière ouverture trouvée par l'outil Extraire une pièce, à surligner.
    pub piece_gap: Option<PieceGap>,
    /// Séparation d'une pièce en cours (US-04.6) : ligne de coupe à poser en deux clics.
    pub split_job: Option<SplitJob>,
    /// Choix par clic du tracé de référence d'une égalité de longueurs (US-03.4a).
    pub length_pick: crate::imposed::LengthPick,
    /// Presse-papiers interne (US-01.6), non enregistré.
    pub clipboard: Option<Fragment>,
    /// JSON du dernier fragment copié, à confier au presse-papiers système
    /// (l'application le prend et le vide).
    pub clipboard_out: Option<String>,
    /// Taille affichée (US-09.2a) : état d'interface, jamais enregistré. Quand ce n'est pas la
    /// taille de base, le plan est une **lecture seule** de la géométrie évaluée.
    pub active_size: Option<SizeId>,
    /// Projet évalué dans la taille active (affichage et sélection seulement) ; `None` = géométrie
    /// de base. Recalculé par `refresh_display` ; `project()` reste toujours celui de base.
    display: Option<Project>,
    drag: Option<Drag>,
    /// Nombre de commandes exécutées avec succès par cet éditeur (sert à repérer
    /// qu'un clic ou une validation a abouti).
    exec_count: u64,
    /// Nom technique de la dernière commande exécutée (étiquette de l'historique visible).
    pub(crate) last_command: Option<String>,
    /// Longueur de cran proposée à la pose (préférence, US-16.7).
    pub default_notch_mm: f64,
}

impl Editor {
    pub fn new(doc: Document) -> Editor {
        let sheet = doc.project().sheets[0].id;
        Editor {
            doc,
            sheet,
            tool: Tool::Select,
            state: ToolState::Idle,
            selection: Selection::default(),
            select_filter: crate::select_ops::SelectionFilter::default(),
            lasso_mode: false,
            length_text: String::new(),
            angle_text: String::new(),
            position_text: String::new(),
            entry_field: 0,
            angle_lock: false,
            last_placed: None,
            repeat_memo: None,
            perp_flip: false,
            rect_kept: false,
            dart_selected: None,
            dart_transfer: DartTransferUi::default(),
            trace: crate::trace::TraceState::default(),
            pleat: crate::pleat::PleatUi::default(),
            flare: crate::flare::FlareUi::default(),
            cutline: crate::cutline::CutUi::default(),
            quick: crate::quick::QuickUi::default(),
            seam: crate::seams::SeamUi::default(),
            notch_double: false,
            round_mode: RoundMode::Center,
            point_mode: PointMode::Free,
            guide_mode: GuideMode::Horizontal,
            show_guides: true,
            show_rulers: false,
            rel_picks: Vec::new(),
            rel_from_end: false,
            spline_points: Vec::new(),
            offset_paths: Vec::new(),
            offset_flip: false,
            offset_corner: CornerStyle::Miter,
            offset_role: PathRole::Construction,
            offset_cache: RefCell::new(None),
            cursor: PointMm::ORIGIN,
            grid_snap_mm: Some(1.0),
            snap_to_paths: true,
            pick_tol_mm: 0.0,
            last_measure: None,
            status: Status::default(),
            active_layer: None,
            piece_detached: false,
            piece_use_rule: false,
            sheet_use_rule: false,
            piece_gap: None,
            split_job: None,
            length_pick: Default::default(),
            clipboard: None,
            clipboard_out: None,
            active_size: None,
            display: None,
            drag: None,
            exec_count: 0,
            last_command: None,
            default_notch_mm: pf_core::notch::DEFAULT_NOTCH_LENGTH_MM,
        }
    }

    /// Libellé de la taille active quand elle n'est **pas** la taille de base de sa gamme (le plan
    /// est alors en lecture seule : US-09.2a) ; `None` en taille de base.
    pub fn size_view(&self) -> Option<String> {
        let id = self.active_size?;
        let (range, i) = self.doc.project().find_size(id)?;
        (range.base != id).then(|| range.sizes[i].label.clone())
    }

    /// Choisit la taille affichée (`None` = taille de base) et recalcule l'affichage.
    pub fn set_active_size(&mut self, size: Option<SizeId>) {
        self.active_size = size;
        self.refresh_display();
        // Plan en lecture seule : seul l'outil Sélection reste utilisable.
        if self.size_view().is_some() && self.tool != Tool::Select {
            self.split_job = None;
            self.tool = Tool::Select;
            self.state = ToolState::Idle;
            self.reset_inputs();
        }
    }

    /// Recalcule le projet évalué de la taille active (rien en taille de base) ; une taille qui
    /// n'existe plus ramène à la taille de base.
    pub fn refresh_display(&mut self) {
        if let Some(id) = self.active_size {
            if self.doc.project().find_size(id).is_none() {
                self.active_size = None;
            }
        }
        self.display = match (self.size_view(), self.active_size) {
            (Some(_), Some(id))
                if self
                    .doc
                    .project()
                    .sheets
                    .iter()
                    .any(|s| s.gradings.iter().any(|g| g.size == id)) =>
            {
                self.doc.project().graded_project(Some(id)).ok()
            }
            _ => None,
        };
    }

    /// Projet à **dessiner** : celui de la taille active (évalué) ou celui de base. Jamais pour
    /// modifier, exporter ou mesurer : ces chemins lisent `project()`.
    pub fn view_project(&self) -> &Project {
        self.display.as_ref().unwrap_or_else(|| self.doc.project())
    }

    /// Refus d'une édition de géométrie quand le plan est en lecture seule (taille active ≠ base).
    /// Seules les commandes de tailles et de gradation passent.
    fn size_view_refuses(&mut self, command: &Command) -> bool {
        if self.size_view().is_none() || size_view_allows(command) {
            return false;
        }
        self.error(tr("size.readonly"));
        true
    }

    /// Calque actif s'il existe encore sur la feuille courante.
    pub fn current_layer(&self) -> Option<LayerId> {
        self.active_layer.filter(|&l| self.sheet().layer(l).is_ok())
    }

    /// Retire de la sélection les objets disparus ou masqués.
    pub fn prune_selection(&mut self) {
        let sheet = self.sheet().clone();
        self.selection.retain_valid(&sheet, self.show_guides);
    }

    /// Range la sélection (points et tracés) dans le calque actif.
    pub fn move_selection_to_active_layer(&mut self) {
        if self.selection.points.is_empty() && self.selection.entities.is_empty() {
            return;
        }
        let command = Command::SetObjectsLayer {
            sheet: self.sheet,
            points: self.selection.points.clone(),
            entities: self.selection.entities.clone(),
            layer: self.current_layer(),
        };
        if self.exec(command).is_some() {
            self.prune_selection();
        }
    }

    pub fn project(&self) -> &Project {
        self.doc.project()
    }

    /// Feuille active (toujours valide : rétablie si elle a disparu).
    pub fn sheet(&self) -> &Sheet {
        let p = self.view_project();
        p.sheet(self.sheet).unwrap_or(&p.sheets[0])
    }

    pub(crate) fn info(&mut self, text: impl Into<String>) {
        self.status = Status {
            text: text.into(),
            error: false,
        };
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.status = Status {
            text: text.into(),
            error: true,
        };
    }

    /// Exécute une commande ; en cas de refus, affiche la raison et ne
    /// modifie rien.
    pub fn exec(&mut self, command: Command) -> Option<Outcome> {
        if self.size_view_refuses(&command) {
            return None;
        }
        self.doc.end_group();
        self.piece_gap = None;
        let name = crate::history::command_name(&command);
        let command = match self.current_layer() {
            Some(layer) if creates_objects(&command) => Command::InLayer {
                sheet: self.sheet,
                layer,
                command: Box::new(command),
            },
            _ => command,
        };
        match self.doc.execute(command) {
            Ok(outcome) => {
                self.exec_count += 1;
                self.last_command = Some(name);
                self.refresh_display();
                if let Some(&id) = outcome.points.last() {
                    self.last_placed = self.sheet().position(id).ok().or(self.last_placed);
                }
                if !outcome.notes.is_empty() {
                    self.info(tr_args("note.core", &[("text", &outcome.notes.join(" "))]));
                }
                Some(outcome)
            }
            Err(e) => {
                self.error(core_error(&e));
                None
            }
        }
    }

    pub(crate) fn reset_inputs(&mut self) {
        self.length_text.clear();
        self.angle_text.clear();
        self.position_text.clear();
        self.entry_field = 0;
        self.perp_flip = false;
        self.spline_points.clear();
        self.offset_paths.clear();
        self.offset_flip = false;
        self.rel_picks.clear();
        self.rel_from_end = false;
        self.piece_gap = None;
    }

    /// Recale l'éditeur après un changement global (annuler, ouvrir…).
    pub fn sync(&mut self) {
        if self.doc.project().sheet(self.sheet).is_err() {
            self.sheet = self.doc.project().sheets[0].id;
        }
        let sheet = self.sheet().clone();
        self.selection.retain_valid(&sheet, self.show_guides);
        self.state = ToolState::Idle;
        self.drag = None;
        self.last_placed = None;
        if let Some(job) = self.split_job {
            if self.doc.project().piece(job.piece).is_err() {
                self.split_job = None;
            }
        }
        self.reset_inputs();
    }

    pub fn set_tool(&mut self, tool: Tool) {
        // Plan en lecture seule (taille active ≠ base) : la sélection seule reste disponible.
        if tool != Tool::Select && self.size_view().is_some() {
            self.error(tr("size.readonly"));
            return;
        }
        self.split_job = None;
        self.dart_selected = None;
        self.dart_transfer.armed = false;
        self.pleat.picking = 0;
        self.seam.picking = 0;
        self.tool = tool;
        self.state = ToolState::Idle;
        self.reset_inputs();
        self.info(match tool {
            Tool::Select => tr("status.hint.select"),
            Tool::Point => tr("status.hint.point"),
            Tool::Segment => tr("status.hint.segment"),
            Tool::Rectangle => tr("status.hint.rectangle"),
            Tool::Parallel => tr("status.hint.parallel"),
            Tool::Perpendicular => tr("status.hint.perpendicular"),
            Tool::PointOnSegment => tr("status.hint.point_on_segment"),
            Tool::Notch => tr("status.hint.notch"),
            Tool::Grainline => tr("status.hint.grainline"),
            Tool::Curve => tr("status.hint.curve"),
            Tool::Spline => tr("status.hint.spline"),
            Tool::Circle => tr("status.hint.circle"),
            Tool::Arc => tr("status.hint.arc"),
            Tool::Fillet => tr("status.hint.fillet"),
            Tool::Chamfer => tr("status.hint.chamfer"),
            Tool::Tangent => tr("status.hint.tangent"),
            Tool::Extend => tr("status.hint.extend"),
            Tool::Split => tr("status.hint.split"),
            Tool::Merge => tr("status.hint.merge"),
            Tool::Offset => tr("status.hint.offset"),
            Tool::Measure => tr("status.hint.measure"),
            Tool::Dimension => tr("status.hint.dimension"),
            Tool::Guide => self.guide_hint(),
            Tool::ExtractPiece => tr("status.hint.extract_piece"),
            Tool::Dart => tr("status.hint.dart"),
            Tool::Trace => tr("status.hint.trace"),
        });
    }

    fn guide_hint(&self) -> String {
        match self.guide_mode {
            GuideMode::Horizontal => tr("status.guide.horizontal"),
            GuideMode::Vertical => tr("status.guide.vertical"),
            GuideMode::Oblique => tr("status.guide.oblique"),
        }
    }

    /// Change le sous-mode de l'outil Ligne d'aide (annule l'étape en cours).
    pub fn set_guide_mode(&mut self, mode: GuideMode) {
        self.guide_mode = mode;
        self.state = ToolState::Idle;
        self.reset_inputs();
        self.info(self.guide_hint());
    }

    /// Change le sous-mode de création des cercles et arcs (annule l'étape en cours).
    pub fn set_round_mode(&mut self, mode: RoundMode) {
        self.round_mode = mode;
        self.state = ToolState::Idle;
        self.reset_inputs();
        let hint = match (self.tool, mode) {
            (Tool::Arc, RoundMode::Center) => tr("status.hint.arc"),
            (_, RoundMode::Center) => tr("status.hint.circle"),
            _ => tr("status.round.first"),
        };
        self.info(hint);
    }

    /// Change le sous-mode de l'outil Point (annule l'étape en cours).
    pub fn set_point_mode(&mut self, mode: PointMode) {
        self.point_mode = mode;
        self.state = ToolState::Idle;
        self.reset_inputs();
        self.info(match mode {
            PointMode::Free => tr("status.hint.point"),
            PointMode::AlongCurve => tr("status.rel.hint.along_curve"),
            PointMode::Offset => tr("status.rel.hint.offset"),
            PointMode::Midpoint => tr("status.rel.hint.midpoint"),
            PointMode::Projection => tr("status.rel.hint.projection"),
            PointMode::Bisector => tr("status.rel.hint.bisector"),
            PointMode::Direction => tr("status.rel.hint.direction"),
            PointMode::DirectionRelative => tr("status.rel.hint.direction_relative"),
            PointMode::Compass => tr("status.rel.hint.compass"),
            PointMode::Division => tr("status.rel.hint.division"),
        });
    }

    pub fn set_sheet(&mut self, sheet: SheetId) {
        if self.doc.project().sheet(sheet).is_ok() && sheet != self.sheet {
            self.sheet = sheet;
            self.selection.clear();
            self.state = ToolState::Idle;
            self.last_measure = None;
            self.last_placed = None;
            self.reset_inputs();
        }
    }

    /// Échap : annule l'étape en cours, sinon revient à la sélection, sinon
    /// vide la sélection. Rien d'incomplet n'est jamais enregistré.
    pub fn escape(&mut self) {
        if self.tool == Tool::Trace && !self.trace.points.is_empty() {
            self.trace.points.clear();
            self.info(tr("status.cancelled"));
        } else if self.dart_transfer.armed {
            self.dart_transfer.armed = false;
            self.info(tr("status.cancelled"));
        } else if self.cancel_pleat_pick() || self.cancel_seam_pick() || self.cancel_length_pick() {
        } else if self.split_job.is_some() {
            self.cancel_split();
        } else if self.drag.take().is_some() {
            self.doc.cancel_group();
        } else if self.state != ToolState::Idle || !self.position_text.is_empty() {
            self.state = ToolState::Idle;
            self.reset_inputs();
            self.info(tr("status.cancelled"));
        } else if self.tool != Tool::Select {
            self.set_tool(Tool::Select);
        } else {
            self.selection.clear();
            self.last_measure = None;
        }
    }

    pub fn undo(&mut self) {
        if self.doc.undo() {
            self.refresh_display();
            self.sync();
            self.info(tr("status.undone"));
        }
    }

    pub fn redo(&mut self) {
        if self.doc.redo() {
            self.refresh_display();
            self.sync();
            self.info(tr("status.redone"));
        }
    }

    /// Remplace le projet par une version restaurée (US-17.2) : une seule étape annulable.
    pub fn restore_version(&mut self, project: pf_core::Project) -> Result<(), pf_core::CoreError> {
        self.doc.restore_project(project)?;
        self.refresh_display();
        self.sync();
        Ok(())
    }

    // ----- visée ---------------------------------------------------------

    fn grid(&self, p: PointMm) -> PointMm {
        match self.grid_snap_mm {
            Some(step) if step > 0.0 => {
                PointMm::new((p.x / step).round() * step, (p.y / step).round() * step)
            }
            _ => p,
        }
    }

    fn nearest_point(&self, world: PointMm, tol_mm: f64) -> Option<(PointId, PointMm)> {
        let sheet = self.sheet();
        sheet
            .points
            .iter()
            .filter(|p| sheet.layer_visible(p.layer))
            .filter_map(|p| sheet.position(p.id).ok().map(|pos| (p.id, pos)))
            .map(|(id, pos)| (id, pos, pf_geometry::distance_mm(pos, world)))
            .filter(|(_, _, d)| *d <= tol_mm)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(id, pos, _)| (id, pos))
    }

    /// Accrochage sous le curseur. Priorité déterministe : point existant,
    /// intersection, milieu, point sur tracé, puis position libre. `tol_mm`
    /// (rayon de visée) vient du zoom ; il choisit la cible sans jamais
    /// entrer dans le calcul de sa position.
    pub fn snap_at(&self, world: PointMm, tol_mm: f64) -> Snap {
        if let Some((id, pos)) = self.nearest_point(world, tol_mm) {
            return Snap {
                kind: SnapKind::Point,
                pos,
                target: PointRef::Existing(id),
            };
        }
        let free = Snap {
            kind: SnapKind::Free,
            pos: self.grid(world),
            target: PointRef::New(self.grid(world)),
        };
        if !self.snap_to_paths || tol_mm <= 0.0 {
            return free;
        }
        let sheet = self.sheet();
        let dist = |p: PointMm| pf_geometry::distance_mm(p, world);
        // Lignes d'aide visibles à portée (accrochables, US-02.8).
        let guides: Vec<&Guide> = if self.show_guides {
            sheet
                .guides
                .iter()
                .filter(|g| sheet.guide_visible(g) && g.distance_to(world) <= tol_mm)
                .collect()
        } else {
            Vec::new()
        };
        // Segments à portée : (id, départ, arrivée, abscisse du pied, distance).
        let mut near: Vec<(EntityId, PointMm, PointMm, f64, f64)> = Vec::new();
        for e in sheet
            .entities
            .iter()
            .filter(|e| sheet.layer_visible(e.layer))
        {
            if let EntityKind::Segment { .. } = e.kind {
                if let Ok((a, b)) = sheet.segment_ends(e.id) {
                    if let Ok((along, d)) = pf_geometry::project_on_segment(world, a, b) {
                        if d <= tol_mm {
                            near.push((e.id, a, b, along, d));
                        }
                    }
                }
            }
        }
        near.sort_by(|x, y| x.4.total_cmp(&y.4).then(x.0.cmp(&y.0)));

        let mut best: Option<(f64, Snap)> = None;
        for (i, s1) in near.iter().enumerate() {
            for s2 in &near[i + 1..] {
                if let Ok(Some(x)) = pf_geometry::segment_intersection(s1.1, s1.2, s2.1, s2.2, 1e-9)
                {
                    let (first, second) = (s1.0.min(s2.0), s1.0.max(s2.0));
                    if dist(x) <= tol_mm && best.as_ref().is_none_or(|(d, _)| dist(x) < *d) {
                        let target =
                            PointRef::Derived(PointDefinition::Intersection { first, second });
                        best = Some((
                            dist(x),
                            Snap {
                                kind: SnapKind::Intersection,
                                pos: x,
                                target,
                            },
                        ));
                    }
                }
            }
        }
        // Cercles et arcs : intersections avec les segments et les autres cercles à portée.
        let rounds: Vec<EntityId> = sheet
            .entities
            .iter()
            .filter(|e| sheet.layer_visible(e.layer))
            .filter(|e| matches!(e.kind, EntityKind::Arc { .. } | EntityKind::Circle { .. }))
            .filter(|e| {
                sheet
                    .shape(e.id)
                    .is_ok_and(|s| s.distance_to_point(world) <= tol_mm)
            })
            .map(|e| e.id)
            .collect();
        for &c in &rounds {
            let others = near
                .iter()
                .map(|n| n.0)
                .chain(rounds.iter().copied().filter(|&o| o > c));
            for o in others {
                for x in sheet.circle_intersections(c, o).unwrap_or_default() {
                    if dist(x) <= tol_mm && best.as_ref().is_none_or(|(d, _)| dist(x) < *d) {
                        best = Some((
                            dist(x),
                            Snap {
                                kind: SnapKind::Intersection,
                                pos: x,
                                target: PointRef::Derived(PointDefinition::CircleIntersection {
                                    first: c,
                                    second: o,
                                    near: x.into(),
                                }),
                            },
                        ));
                    }
                }
            }
        }
        // Courbes (Bézier, lisses) : intersections géométriques avec tout tracé à portée.
        let in_reach: Vec<(EntityId, bool)> = sheet
            .entities
            .iter()
            .filter(|e| sheet.layer_visible(e.layer))
            .filter(|e| {
                sheet
                    .shape(e.id)
                    .is_ok_and(|s| s.distance_to_point(world) <= tol_mm)
            })
            .map(|e| {
                (
                    e.id,
                    matches!(e.kind, EntityKind::Curve { .. } | EntityKind::Spline { .. }),
                )
            })
            .collect();
        for &(c, _) in in_reach.iter().filter(|(_, is_curve)| *is_curve) {
            for &(o, other_is_curve) in &in_reach {
                if o == c || (other_is_curve && o < c) {
                    continue;
                }
                for h in sheet.trace_intersections(c, o).unwrap_or_default() {
                    let x = h.point;
                    if dist(x) <= tol_mm && best.as_ref().is_none_or(|(d, _)| dist(x) < *d) {
                        best = Some((
                            dist(x),
                            Snap {
                                kind: SnapKind::Intersection,
                                pos: x,
                                target: PointRef::Derived(PointDefinition::TraceIntersection {
                                    first: c.min(o),
                                    second: c.max(o),
                                    near: x.into(),
                                }),
                            },
                        ));
                    }
                }
            }
        }
        // Intersections guide/guide et guide/tracé : point libre à la position exacte.
        {
            let mut consider = |x: PointMm| {
                if dist(x) <= tol_mm && best.as_ref().is_none_or(|(d, _)| dist(x) < *d) {
                    best = Some((
                        dist(x),
                        Snap {
                            kind: SnapKind::Intersection,
                            pos: x,
                            target: PointRef::New(x),
                        },
                    ));
                }
            };
            for (i, g) in guides.iter().enumerate() {
                for h in &guides[i + 1..] {
                    if let Some(x) = g.intersection(h) {
                        consider(x);
                    }
                }
                for e in sheet
                    .entities
                    .iter()
                    .filter(|e| sheet.layer_visible(e.layer))
                    .filter(|e| {
                        sheet
                            .shape(e.id)
                            .is_ok_and(|s| s.distance_to_point(world) <= tol_mm)
                    })
                {
                    for x in sheet.guide_trace_hits(g, e.id) {
                        consider(x);
                    }
                }
            }
        }
        if let Some((_, snap)) = best {
            return snap;
        }
        for &(segment, a, b, _, _) in &near {
            let mid = pf_geometry::midpoint(a, b);
            if dist(mid) <= tol_mm {
                return Snap {
                    kind: SnapKind::Midpoint,
                    pos: mid,
                    target: PointRef::Derived(PointDefinition::OnSegmentRatio {
                        segment,
                        ratio: 0.5,
                    }),
                };
            }
        }
        if let Some(&(segment, a, b, along, _)) = near.first() {
            let len = pf_geometry::distance_mm(a, b);
            let distance_mm = match self.grid_snap_mm {
                Some(step) if step > 0.0 => ((along / step).round() * step).clamp(0.0, len),
                _ => along,
            };
            if let Ok(pos) = pf_geometry::point_on_segment(a, b, distance_mm) {
                return Snap {
                    kind: SnapKind::OnPath,
                    pos,
                    target: PointRef::Derived(PointDefinition::OnSegment {
                        segment,
                        distance_mm,
                        origin: SegmentEnd::Start,
                    }),
                };
            }
        }
        let mut on_curve: Option<(f64, Snap)> = None;
        for e in sheet
            .entities
            .iter()
            .filter(|e| sheet.layer_visible(e.layer))
        {
            if let Ok(c) = sheet.curve_geometry(e.id) {
                let (t, d) = c.nearest_t(world);
                if d <= tol_mm && on_curve.as_ref().is_none_or(|(best, _)| d < *best) {
                    on_curve = Some((
                        d,
                        Snap {
                            kind: SnapKind::OnPath,
                            pos: c.point_at(t),
                            target: PointRef::Derived(PointDefinition::OnCurve { curve: e.id, t }),
                        },
                    ));
                }
            }
        }
        if let Some((_, snap)) = on_curve {
            return snap;
        }
        // Une ligne d'aide : projection du point visé (aimanté à la grille).
        let nearest_guide = guides
            .iter()
            .min_by(|a, b| a.distance_to(world).total_cmp(&b.distance_to(world)));
        match nearest_guide {
            Some(g) => {
                let pos = g.project(self.grid(world));
                Snap {
                    kind: SnapKind::OnGuide,
                    pos,
                    target: PointRef::New(pos),
                }
            }
            None => free,
        }
    }

    /// Point désigné par un clic : existant, dérivé ou libre.
    fn snap(&self, world: PointMm, tol_mm: f64) -> PointRef {
        self.snap_at(world, tol_mm).target
    }

    /// Position visée par le curseur, accrochage compris (pour les aperçus).
    fn aim(&self) -> PointMm {
        self.snap_at(self.cursor, self.pick_tol_mm).pos
    }

    /// Accrochage à signaler à l'écran pour l'outil et l'étape en cours.
    pub fn hover_snap(&self) -> Option<Snap> {
        let typed = !self.length_text.trim().is_empty() || !self.angle_text.trim().is_empty();
        let placing = match (self.tool, self.state) {
            (
                Tool::Point
                | Tool::Segment
                | Tool::Grainline
                | Tool::Rectangle
                | Tool::Curve
                | Tool::Measure
                | Tool::Trace,
                ToolState::Idle,
            ) => true,
            (Tool::Dimension, ToolState::Idle | ToolState::DimensionFrom { .. }) => true,
            (Tool::Dart, ToolState::Idle | ToolState::DartApex { .. }) => true,
            (Tool::Circle | Tool::Arc | Tool::Spline, ToolState::Idle) => true,
            (_, ToolState::SplineNext) => true,
            (_, ToolState::RoundFirst { .. } | ToolState::RoundSecond { .. }) => !typed,
            (_, ToolState::ArcCenter { .. }) => !typed,
            (_, ToolState::SegmentFrom { .. }) => !typed && !self.angle_lock,
            (_, ToolState::RectFrom { .. }) => !typed,
            (_, ToolState::CurveFrom { .. } | ToolState::MeasureFrom { .. }) => true,
            (_, ToolState::PerpPickStart { .. }) => true,
            (Tool::Guide, ToolState::Idle) => true,
            (_, ToolState::GuideFrom { .. }) => !typed && !self.angle_lock,
            _ => false,
        };
        let snap = self.snap_at(self.cursor, self.pick_tol_mm);
        (placing && snap.kind != SnapKind::Free).then_some(snap)
    }

    fn ref_pos(&self, r: PointRef) -> Option<PointMm> {
        match r {
            PointRef::Existing(id) => self.sheet().position(id).ok(),
            PointRef::New(p) => Some(p),
            PointRef::Derived(definition) => self.sheet().evaluate(&definition).ok(),
        }
    }

    fn nearest_entity(&self, world: PointMm, tol_mm: f64, segments_only: bool) -> Option<EntityId> {
        let sheet = self.sheet();
        sheet
            .entities
            .iter()
            .filter(|e| sheet.layer_visible(e.layer))
            .filter_map(|e| {
                let d = match e.kind {
                    EntityKind::Segment { .. } => {
                        let (a, b) = sheet.segment_ends(e.id).ok()?;
                        pf_geometry::project_on_segment(world, a, b).ok()?.1
                    }
                    EntityKind::Curve { .. } if !segments_only => {
                        sheet.curve_geometry(e.id).ok()?.distance_to_point(world)
                    }
                    EntityKind::Curve { .. } => return None,
                    EntityKind::Arc { .. } | EntityKind::Circle { .. } if !segments_only => {
                        sheet.shape(e.id).ok()?.distance_to_point(world)
                    }
                    EntityKind::Arc { .. } | EntityKind::Circle { .. } => return None,
                    EntityKind::Spline { .. } if !segments_only => {
                        sheet.spline_geometry(e.id).ok()?.distance_to_point(world)
                    }
                    EntityKind::Spline { .. } => return None,
                };
                (d <= tol_mm).then_some((e.id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Objet sous le curseur : poignées des courbes sélectionnées, puis
    /// points, puis tracés.
    pub fn hit_test(&self, world: PointMm, tol_mm: f64) -> Option<Target> {
        let sheet = self.sheet();
        for &id in &self.selection.entities {
            if let Ok(c) = sheet.curve_geometry(id) {
                if pf_geometry::distance_mm(c.c1, world) <= tol_mm {
                    return Some(Target::Handle(id, CurveHandle::Start));
                }
                if pf_geometry::distance_mm(c.c2, world) <= tol_mm {
                    return Some(Target::Handle(id, CurveHandle::End));
                }
            }
        }
        // Filtre de sélection (US-16.1) : une catégorie décochée n'est pas atteignable.
        let filter = self.select_filter;
        if filter.points {
            if let Some((id, _)) = self.nearest_point(world, tol_mm) {
                return Some(Target::Point(id));
            }
        }
        let notch = sheet
            .notches
            .iter()
            .filter(|n| filter.notches && sheet.notch_visible(n))
            .filter_map(|n| {
                // Un cran « sur la coupe » se touche là où il est dessiné (US-06.1).
                let marks = self.project().notch_placed_marks(self.sheet, n).ok()?.1;
                let d = marks
                    .iter()
                    .filter_map(|(a, b)| pf_geometry::project_on_segment(world, *a, *b).ok())
                    .map(|(_, d)| d)
                    .fold(f64::INFINITY, f64::min);
                (d <= tol_mm).then_some((n.id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = notch {
            return Some(Target::Notch(id));
        }
        let dimension = sheet
            .dimensions
            .iter()
            .filter(|d| filter.dimensions && sheet.dimension_visible(d))
            .filter_map(|d| {
                let g = sheet.dimension_geometry(d.id).ok()?;
                let dist = [
                    (g.from_offset, g.to_offset),
                    (g.from, g.from_offset),
                    (g.to, g.to_offset),
                ]
                .iter()
                .filter_map(|(a, b)| pf_geometry::project_on_segment(world, *a, *b).ok())
                .map(|(_, d)| d)
                .fold(f64::INFINITY, f64::min);
                (dist <= tol_mm).then_some((d.id, dist))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = dimension {
            return Some(Target::Dimension(id));
        }
        if filter.paths {
            if let Some(entity) = self.nearest_entity(world, tol_mm, false) {
                return Some(Target::Entity(entity));
            }
        }
        if filter.guides {
            self.nearest_guide(world, tol_mm).map(Target::Guide)
        } else {
            None
        }
    }

    /// Ligne d'aide visible la plus proche, à portée de visée.
    fn nearest_guide(&self, world: PointMm, tol_mm: f64) -> Option<GuideId> {
        if !self.show_guides {
            return None;
        }
        let sheet = self.sheet();
        sheet
            .guides
            .iter()
            .filter(|g| sheet.guide_visible(g))
            .map(|g| (g.id, g.distance_to(world)))
            .filter(|(_, d)| *d <= tol_mm)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    // ----- aperçu et saisie ----------------------------------------------

    pub fn pointer_moved(&mut self, world: PointMm) {
        self.cursor = world;
    }

    pub(crate) fn typed_length(&self) -> Result<Option<f64>, String> {
        if self.length_text.trim().is_empty() {
            return Ok(None);
        }
        pf_core::units::parse_length_display(&self.length_text)
            .map(Some)
            .map_err(|e| tr_args("error.length", &[("detail", &e)]))
    }

    /// Seconde valeur de la palette lue comme une longueur (hauteur d'un rectangle).
    pub(crate) fn typed_height(&self) -> Result<Option<f64>, String> {
        if self.angle_text.trim().is_empty() {
            return Ok(None);
        }
        pf_core::units::parse_length_display(&self.angle_text)
            .map(Some)
            .map_err(|e| tr_args("error.height", &[("detail", &e)]))
    }

    /// Saisie d'un paramètre d'un point construit qui peut être **maintenu** : un nombre reste
    /// figé, une expression (variables de la table) est évaluée et sera liée à la création
    /// (`AddPointWithFormulas`). Vide : `None`. `key` : clé du message d'erreur de nombre.
    pub(crate) fn typed_param(
        &self,
        text: &str,
        angle: bool,
        key: &str,
    ) -> Result<Option<TypedParam>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let plain = if angle {
            parse_angle_deg(text)
        } else {
            pf_core::units::parse_length_display(text)
        };
        if let Ok(value) = plain {
            return Ok(Some(TypedParam { value, expr: None }));
        }
        let values: std::collections::HashMap<String, f64> = self
            .project()
            .variable_values()
            .map(|v| v.into_iter().collect())
            .unwrap_or_default();
        match pf_core::expr::evaluate(text, &|n| values.get(n).copied()) {
            Ok(value) => Ok(Some(TypedParam {
                value,
                expr: Some(text.to_string()),
            })),
            Err(e) => Err(tr_args(
                key,
                &[("detail", &crate::variables::expr_error_text(&e))],
            )),
        }
    }

    /// Expressions saisies qui seront **maintenues** à la création du point (aperçu de la palette) :
    /// `(expression, valeur calculée, est un angle)`. Vide hors point à distance / relatif, ou si
    /// les champs ne contiennent que des nombres.
    pub(crate) fn typed_creation_formulas(&self) -> Vec<(String, f64, bool)> {
        let second = match (self.state, self.point_mode) {
            (ToolState::PointOn { .. }, _) => None,
            (ToolState::RelativePick, PointMode::Offset | PointMode::Compass) => Some(false),
            (ToolState::RelativePick, PointMode::Direction | PointMode::DirectionRelative) => {
                Some(true)
            }
            (ToolState::RelativePick, m) if m != PointMode::Division => None,
            _ => return Vec::new(),
        };
        let mut out = Vec::new();
        let fields = [
            (&self.length_text, false),
            (&self.angle_text, second == Some(true)),
        ];
        for (i, (text, angle)) in fields.into_iter().enumerate() {
            if i == 1 && second.is_none() {
                break;
            }
            if let Ok(Some(TypedParam {
                value,
                expr: Some(expr),
            })) = self.typed_param(text, angle, "error.length")
            {
                out.push((expr, value, angle));
            }
        }
        out
    }

    fn typed_angle(&self) -> Result<Option<f64>, String> {
        if self.angle_text.trim().is_empty() {
            return Ok(None);
        }
        parse_angle_deg(&self.angle_text)
            .map(Some)
            .map_err(|e| tr_args("error.angle", &[("detail", &e)]))
    }

    /// Le champ Longueur contient-il une coordonnée (`@…`, `x;y`, `L<A`) plutôt
    /// qu'une longueur ? Si oui, `origin` sert aux coordonnées relatives. Une
    /// coordonnée se saisit seule : le champ Angle doit alors être vide.
    fn typed_coordinate(&self, origin: PointMm) -> Result<Option<CoordInput>, String> {
        if !is_coordinate_text(&self.length_text) {
            return Ok(None);
        }
        if !self.angle_text.trim().is_empty() {
            return Err(tr("error.coord.with_angle"));
        }
        parse_coordinate(&self.length_text, origin)
            .map(Some)
            .map_err(|e| tr_args("error.coord", &[("detail", &e)]))
    }

    /// Angle ramené au pas de 15° si Maj est enfoncée (US-16.2), sinon inchangé.
    fn locked_angle(&self, angle_deg: f64) -> f64 {
        if self.angle_lock {
            pf_geometry::snap_angle_deg(angle_deg, ANGLE_LOCK_STEP_DEG).unwrap_or(angle_deg)
        } else {
            angle_deg
        }
    }

    /// Aperçu de l'étape en cours, d'après la souris et les champs saisis.
    pub fn preview(&self) -> Result<Option<Preview>, String> {
        let sheet = self.sheet();
        let err = |e: CoreError| core_error(&e);
        let gerr = |e: pf_geometry::GeometryError| core_error(&e);
        Ok(match self.state {
            ToolState::Idle
            | ToolState::PerpPickStart { .. }
            | ToolState::SplineNext
            | ToolState::RelativePick
            | ToolState::ExtendFrom { .. }
            | ToolState::MergeFirst { .. }
            | ToolState::OffsetReady
            | ToolState::DartApex { .. }
            | ToolState::DartBase { .. }
            | ToolState::JoinFirst { .. } => None,
            ToolState::JoinReady {
                first,
                pick_first,
                second,
                pick_second,
            } => {
                let Some(r) = self.typed_length()? else {
                    return Ok(None);
                };
                if self.tool == Tool::Fillet {
                    let (a, b) = (
                        sheet.fillet_path(first).map_err(err)?,
                        sheet.fillet_path(second).map_err(err)?,
                    );
                    let f =
                        pf_geometry::fillet(&a, &b, r, pick_first, pick_second).map_err(gerr)?;
                    Some(Preview::Arc {
                        center: f.center,
                        radius_mm: r,
                        start_deg: pf_geometry::direction_angle_deg(f.center, f.tangent_first)
                            .map_err(gerr)?,
                        end_deg: pf_geometry::direction_angle_deg(f.center, f.tangent_second)
                            .map_err(gerr)?,
                        ccw: f.ccw,
                    })
                } else {
                    let d2 = self.typed_height()?.unwrap_or(r);
                    let ends = |id: EntityId| sheet.segment_ends(id).map_err(err);
                    let c = pf_geometry::chamfer(
                        ends(first)?,
                        ends(second)?,
                        r,
                        d2,
                        pick_first,
                        pick_second,
                    )
                    .map_err(gerr)?;
                    Some(Preview::Segment {
                        from: c.first,
                        to: c.second,
                        length_mm: pf_geometry::distance_mm(c.first, c.second),
                        angle_deg: pf_geometry::direction_angle_deg(c.first, c.second)
                            .map_err(gerr)?,
                    })
                }
            }
            ToolState::TangentFrom { from } => {
                let p = self
                    .ref_pos(from)
                    .ok_or_else(|| tr("error.start_missing"))?;
                let Some(target) = self.nearest_entity(self.cursor, self.pick_tol_mm, false) else {
                    return Ok(None);
                };
                let (circle, arc) = match sheet.shape(target) {
                    Ok(pf_core::Shape::Circle(c)) => (c.circle, None),
                    Ok(pf_core::Shape::Arc(a)) => (
                        pf_geometry::Circle::new(a.center, a.radius_mm).map_err(gerr)?,
                        Some(a),
                    ),
                    _ => return Ok(None),
                };
                let Ok(mut solutions) = pf_geometry::tangent_points_from_point(&circle, p) else {
                    return Ok(None);
                };
                if let Some(a) = arc {
                    solutions.retain(|t| a.contains_angle_of(*t));
                }
                // Point sur le cercle : la longueur est à saisir, pas d'aperçu.
                if (pf_geometry::distance_mm(p, circle.center) - circle.radius_mm).abs()
                    <= EPSILON_MM
                {
                    return Ok(None);
                }
                let Some(to) = pf_geometry::nearest_solution(&solutions, self.cursor) else {
                    return Ok(None);
                };
                Some(Preview::Segment {
                    from: p,
                    to,
                    length_mm: pf_geometry::distance_mm(p, to),
                    angle_deg: pf_geometry::direction_angle_deg(p, to).map_err(gerr)?,
                })
            }
            ToolState::TangentAt { entity } => {
                let d = sheet.path_distance_of(entity, self.cursor).map_err(err)?;
                let (contact, tangent) = sheet.path_point_at(entity, d).map_err(err)?;
                let len = self.typed_length()?.unwrap_or(DEFAULT_TANGENT_LENGTH_MM);
                let k = if (self.cursor - contact).dot(tangent) < 0.0 {
                    -len
                } else {
                    len
                };
                let to = contact + tangent * k;
                Some(Preview::Segment {
                    from: contact,
                    to,
                    length_mm: len,
                    angle_deg: pf_geometry::direction_angle_deg(contact, to).map_err(gerr)?,
                })
            }
            ToolState::NotchOn { reference } => {
                let len = sheet.path_length_mm(reference).map_err(err)?;
                let raw = sheet
                    .path_distance_of(reference, self.cursor)
                    .map_err(err)?;
                let distance_mm = match self.typed_length()? {
                    Some(d) => d,
                    None => match self.grid_snap_mm {
                        Some(step) if step > 0.0 => ((raw / step).round() * step).clamp(0.0, len),
                        _ => raw,
                    },
                };
                if !(0.0..=len).contains(&distance_mm) {
                    return Err(tr_args(
                        "error.notch.position",
                        &[
                            ("position", &pf_core::input::format_mm(distance_mm)),
                            ("length", &pf_core::input::format_mm(len)),
                        ],
                    ));
                }
                let length_mm = self.typed_height()?.unwrap_or(self.default_notch_mm);
                if length_mm <= EPSILON_MM || length_mm > pf_core::notch::MAX_NOTCH_LENGTH_MM {
                    return Err(tr_args(
                        "error.notch.length",
                        &[("max", &pf_core::notch::MAX_NOTCH_LENGTH_MM.to_string())],
                    ));
                }
                let (base, tangent) = sheet.path_point_at(reference, distance_mm).map_err(err)?;
                let mut side = if tangent.cross(self.cursor - base) >= 0.0 {
                    Side::Left
                } else {
                    Side::Right
                };
                if self.perp_flip {
                    side = side.flipped();
                }
                Some(Preview::Notch {
                    base,
                    distance_mm,
                    side,
                    length_mm,
                    style: if self.notch_double {
                        NotchStyle::Double
                    } else {
                        NotchStyle::Single
                    },
                })
            }
            ToolState::RectFrom { start } => {
                let from = self
                    .ref_pos(start)
                    .ok_or_else(|| tr("error.start_missing"))?;
                let cursor = self.aim();
                let width_mm = signed_size(self.typed_length()?, cursor.x - from.x);
                let height_mm = signed_size(self.typed_height()?, cursor.y - from.y);
                if width_mm.abs() <= EPSILON_MM || height_mm.abs() <= EPSILON_MM {
                    return Ok(None);
                }
                Some(Preview::Rectangle {
                    from,
                    to: PointMm::new(from.x + width_mm, from.y + height_mm),
                    width_mm,
                    height_mm,
                })
            }
            ToolState::ParallelOf { reference } => {
                let (ra, rb) = sheet.segment_ends(reference).map_err(err)?;
                let normal = (rb - ra).unit().map_err(gerr)?.rotated_left();
                let cursor = self.aim();
                let offset = (cursor - ra).dot(normal);
                let mut side = if offset < 0.0 {
                    Side::Right
                } else {
                    Side::Left
                };
                if self.perp_flip {
                    side = side.flipped();
                }
                let distance_mm = self.typed_length()?.unwrap_or(offset.abs());
                if distance_mm <= EPSILON_MM {
                    return Ok(None);
                }
                let k = if side == Side::Left {
                    distance_mm
                } else {
                    -distance_mm
                };
                Some(Preview::Parallel {
                    from: ra + normal * k,
                    to: rb + normal * k,
                    distance_mm,
                    side,
                })
            }
            ToolState::SegmentFrom { start } => {
                let from = self
                    .ref_pos(start)
                    .ok_or_else(|| tr("error.start_missing"))?;
                let cursor = self.aim();
                let d = cursor - from;
                // Coordonnée saisie (`x;y`, `@dx;dy`, `@L<A`) : même aperçu, même commande.
                let (length_mm, angle_deg) = if let Some(c) = self.typed_coordinate(from)? {
                    match c {
                        CoordInput::Polar {
                            length_mm,
                            angle_deg,
                            ..
                        } => (length_mm, angle_deg.rem_euclid(360.0)),
                        other => {
                            let v = other.point() - from;
                            if v.norm() <= EPSILON_MM {
                                return Err(tr("error.coord.degenerate"));
                            }
                            (
                                v.norm(),
                                pf_geometry::direction_angle_deg(from, other.point())
                                    .map_err(gerr)?,
                            )
                        }
                    }
                } else {
                    let length_mm = self.typed_length()?.unwrap_or(d.norm());
                    let angle_deg = match self.typed_angle()? {
                        Some(a) => a.rem_euclid(360.0),
                        None if d.norm() > EPSILON_MM => self.locked_angle(
                            pf_geometry::direction_angle_deg(from, cursor).map_err(gerr)?,
                        ),
                        None => 0.0,
                    };
                    (length_mm, angle_deg)
                };
                if length_mm <= EPSILON_MM {
                    return Ok(None);
                }
                let to = pf_geometry::point_from_polar(from, length_mm, angle_deg).map_err(gerr)?;
                Some(Preview::Segment {
                    from,
                    to,
                    length_mm,
                    angle_deg,
                })
            }
            ToolState::PerpFrom { reference, start } => {
                let (ra, rb) = sheet.segment_ends(reference).map_err(err)?;
                let from = self
                    .ref_pos(start)
                    .ok_or_else(|| tr("error.start_missing"))?;
                let cursor = self.aim();
                let mut side = pf_geometry::side_of(ra, rb, cursor).unwrap_or(Side::Left);
                if self.perp_flip {
                    side = side.flipped();
                }
                let normal = (rb - ra).unit().map_err(gerr)?.rotated_left();
                let length_mm = self
                    .typed_length()?
                    .unwrap_or((cursor - from).dot(normal).abs());
                if length_mm <= EPSILON_MM {
                    return Ok(None);
                }
                let to =
                    pf_geometry::perpendicular_end(ra, rb, from, length_mm, side).map_err(gerr)?;
                Some(Preview::Perpendicular {
                    from,
                    to,
                    length_mm,
                    side,
                })
            }
            ToolState::PointOn { segment, origin } => {
                let (a, b) = sheet.segment_ends(segment).map_err(err)?;
                let (a, b) = match origin {
                    SegmentEnd::Start => (a, b),
                    SegmentEnd::End => (b, a),
                };
                let distance_mm = match self
                    .typed_param(&self.length_text, false, "error.length")?
                    .map(|t| t.value)
                {
                    Some(d) => d,
                    None => {
                        let along = pf_geometry::project_on_segment(self.cursor, a, b)
                            .map_err(gerr)?
                            .0;
                        match self.grid_snap_mm {
                            Some(step) if step > 0.0 => {
                                ((along / step).round() * step).min(pf_geometry::distance_mm(a, b))
                            }
                            _ => along,
                        }
                    }
                };
                let at = pf_geometry::point_on_segment(a, b, distance_mm).map_err(gerr)?;
                Some(Preview::PointOn {
                    origin: a,
                    at,
                    distance_mm,
                })
            }
            ToolState::CurveFrom { start } => {
                let from = self
                    .ref_pos(start)
                    .ok_or_else(|| tr("error.start_missing"))?;
                Some(Preview::Curve {
                    from,
                    to: self.aim(),
                })
            }
            ToolState::DimensionFrom { from } => {
                let Some((to, to_pos)) = self.nearest_point(self.cursor, self.pick_tol_mm) else {
                    return Ok(None);
                };
                if to == from {
                    return Ok(None);
                }
                let from_pos = sheet.position(from).map_err(err)?;
                let magnitude = self
                    .typed_length()?
                    .map(f64::abs)
                    .unwrap_or(pf_core::dimension::DEFAULT_DIMENSION_OFFSET_MM);
                // Côté de la souris par rapport à from -> to ; à gauche par défaut.
                let mut left = (to_pos - from_pos).cross(self.cursor - from_pos) >= 0.0;
                if self.perp_flip {
                    left = !left;
                }
                let offset_mm = if left { magnitude } else { -magnitude };
                if magnitude > pf_core::dimension::MAX_DIMENSION_OFFSET_MM {
                    return Err(tr_args(
                        "error.dimension.offset",
                        &[(
                            "max",
                            &pf_core::dimension::MAX_DIMENSION_OFFSET_MM.to_string(),
                        )],
                    ));
                }
                let n = (to_pos - from_pos).unit().map_err(gerr)?.rotated_left() * offset_mm;
                let value_mm = pf_geometry::distance_mm(from_pos, to_pos);
                Some(Preview::Dimension {
                    from,
                    to,
                    geometry: DimensionGeometry {
                        from: from_pos,
                        to: to_pos,
                        from_offset: from_pos + n,
                        to_offset: to_pos + n,
                        value_mm,
                    },
                    offset_mm,
                })
            }
            ToolState::CircleCenter { center } => {
                let c = self
                    .ref_pos(center)
                    .ok_or_else(|| tr("error.start_missing"))?;
                let radius_mm = self
                    .typed_length()?
                    .unwrap_or_else(|| pf_geometry::distance_mm(c, self.aim()));
                if radius_mm <= EPSILON_MM {
                    return Ok(None);
                }
                Some(Preview::Circle {
                    center: c,
                    radius_mm,
                })
            }
            ToolState::ArcCenter { center } => {
                let c = self
                    .ref_pos(center)
                    .ok_or_else(|| tr("error.start_missing"))?;
                let cursor = self.aim();
                let d = cursor - c;
                let radius_mm = self.typed_length()?.unwrap_or(d.norm());
                let angle_deg = match self.typed_angle()? {
                    Some(a) => a.rem_euclid(360.0),
                    None if d.norm() > EPSILON_MM => {
                        pf_geometry::direction_angle_deg(c, cursor).map_err(gerr)?
                    }
                    None => 0.0,
                };
                if radius_mm <= EPSILON_MM {
                    return Ok(None);
                }
                let to = pf_geometry::point_from_polar(c, radius_mm, angle_deg).map_err(gerr)?;
                Some(Preview::Segment {
                    from: c,
                    to,
                    length_mm: radius_mm,
                    angle_deg,
                })
            }
            ToolState::ArcSweep {
                center,
                radius_mm,
                start_deg,
            } => {
                let c = self
                    .ref_pos(center)
                    .ok_or_else(|| tr("error.start_missing"))?;
                let cursor = self.aim();
                let end_deg = match self.typed_angle()? {
                    Some(a) => a.rem_euclid(360.0),
                    None if (cursor - c).norm() > EPSILON_MM => {
                        pf_geometry::direction_angle_deg(c, cursor).map_err(gerr)?
                    }
                    None => return Ok(None),
                };
                Some(Preview::Arc {
                    center: c,
                    radius_mm,
                    start_deg,
                    end_deg,
                    ccw: !self.perp_flip,
                })
            }
            ToolState::RoundFirst { first } => {
                let from = self
                    .ref_pos(first)
                    .ok_or_else(|| tr("error.start_missing"))?;
                Some(Preview::Curve {
                    from,
                    to: self.aim(),
                })
            }
            ToolState::RoundSecond { first, second } => {
                let a = self
                    .ref_pos(first)
                    .ok_or_else(|| tr("error.start_missing"))?;
                let b = self
                    .ref_pos(second)
                    .ok_or_else(|| tr("error.start_missing"))?;
                let cursor = self.aim();
                match self.round_mode {
                    RoundMode::TwoPointsRadius => {
                        let chord = b - a;
                        let mid = a.lerp(b, 0.5);
                        let normal = chord.unit().map_err(gerr)?.rotated_left();
                        let center = match self.typed_length()? {
                            Some(r) => {
                                let (left, right) =
                                    pf_geometry::Circle::centers_through_two_points(a, b, r)
                                        .map_err(gerr)?;
                                if chord.cross(cursor - a) >= 0.0 {
                                    left
                                } else {
                                    right
                                }
                            }
                            None => mid + normal * (cursor - mid).dot(normal),
                        };
                        let radius_mm = pf_geometry::distance_mm(center, a);
                        if radius_mm <= EPSILON_MM {
                            return Ok(None);
                        }
                        if self.tool == Tool::Arc {
                            Some(Preview::Arc {
                                center,
                                radius_mm,
                                start_deg: pf_geometry::direction_angle_deg(center, a)
                                    .map_err(gerr)?,
                                end_deg: pf_geometry::direction_angle_deg(center, b)
                                    .map_err(gerr)?,
                                ccw: !self.perp_flip,
                            })
                        } else {
                            Some(Preview::Circle { center, radius_mm })
                        }
                    }
                    _ => {
                        let Ok(circle) = pf_geometry::Circle::through_three_points(a, b, cursor)
                        else {
                            return Ok(None);
                        };
                        if self.tool == Tool::Arc {
                            let arc = pf_geometry::Arc::through_three_points(a, b, cursor)
                                .map_err(gerr)?;
                            Some(Preview::Arc {
                                center: arc.center,
                                radius_mm: arc.radius_mm,
                                start_deg: arc.start_deg(),
                                end_deg: arc.end_deg(),
                                ccw: arc.is_ccw(),
                            })
                        } else {
                            Some(Preview::Circle {
                                center: circle.center,
                                radius_mm: circle.radius_mm,
                            })
                        }
                    }
                }
            }
            ToolState::GuideFrom { through } => {
                let aim = self.aim();
                let angle_deg = match self.typed_angle()? {
                    Some(a) => a,
                    None if (aim - through).norm() > EPSILON_MM => self.locked_angle(
                        pf_geometry::direction_angle_deg(through, aim).map_err(gerr)?,
                    ),
                    None => 0.0,
                };
                Some(Preview::Guide {
                    through,
                    angle_deg: angle_deg.rem_euclid(180.0),
                })
            }
            ToolState::MeasureFrom { start } => Some(Preview::Measure {
                from: start,
                to: self.cursor,
                distance_mm: pf_geometry::distance_mm(start, self.cursor),
            }),
        })
    }

    /// Vrai si la palette de saisie doit être affichée.
    pub fn wants_numeric_input(&self) -> bool {
        self.tool == Tool::Guide
            || matches!(
                self.state,
                ToolState::SegmentFrom { .. }
                    | ToolState::RectFrom { .. }
                    | ToolState::NotchOn { .. }
                    | ToolState::ParallelOf { .. }
                    | ToolState::PerpFrom { .. }
                    | ToolState::PointOn { .. }
                    | ToolState::DimensionFrom { .. }
                    | ToolState::CircleCenter { .. }
                    | ToolState::ArcCenter { .. }
                    | ToolState::ArcSweep { .. }
                    | ToolState::JoinReady { .. }
                    | ToolState::ExtendFrom { .. }
                    | ToolState::OffsetReady
                    | ToolState::DartBase { .. }
                    | ToolState::TangentFrom { .. }
                    | ToolState::TangentAt { .. }
            )
            || (self.state == ToolState::RelativePick
                && self.point_mode.needs_values()
                && self.rel_picks.len() >= self.point_mode.picks().len())
            || (matches!(self.state, ToolState::RoundSecond { .. })
                && self.round_mode == RoundMode::TwoPointsRadius)
    }

    /// Bascule l'extrémité d'origine du point à distance.
    pub fn flip_origin(&mut self) {
        if let ToolState::PointOn { segment, origin } = self.state {
            let origin = match origin {
                SegmentEnd::Start => SegmentEnd::End,
                SegmentEnd::End => SegmentEnd::Start,
            };
            self.state = ToolState::PointOn { segment, origin };
        }
    }

    // ----- clics ---------------------------------------------------------

    /// Clic gauche dans le canevas.
    pub fn click(&mut self, world: PointMm, tol_mm: f64, shift: bool) {
        self.click_with(world, tol_mm, shift, false);
    }

    /// Clic gauche avec Alt éventuel : en sélection, Alt désigne le membre
    /// d'un groupe seul, sans le reste du groupe.
    pub fn click_with(&mut self, world: PointMm, tol_mm: f64, shift: bool, alt: bool) {
        let (before, done) = (self.repeat_snapshot(), self.exec_count);
        self.click_tool(world, tol_mm, shift, alt);
        self.remember_if_done(before, done);
    }

    fn click_tool(&mut self, world: PointMm, tol_mm: f64, shift: bool, alt: bool) {
        self.cursor = world;
        if self.pleat.picking > 0 {
            self.pleat_click(world, tol_mm);
            return;
        }
        if self.seam.picking > 0 {
            self.seam_click(world, tol_mm);
            return;
        }
        if self.length_pick.waiting.is_some() {
            self.length_pick_click(world, tol_mm);
            return;
        }
        if self.split_job.is_some() {
            self.split_piece_click(world, tol_mm);
            return;
        }
        match (self.tool, self.state) {
            (Tool::Select, _) => match self.hit_test(world, tol_mm) {
                Some(Target::Handle(..)) => {}
                Some(Target::Entity(id)) if !alt && self.sheet().group_of(id).is_some() => {
                    self.select_group_of(id, shift)
                }
                Some(target) if shift => self.selection.toggle(target),
                Some(target) => self.selection.set(target),
                None if shift => {}
                None => self.selection.clear(),
            },
            (Tool::Point, _) if self.point_mode != PointMode::Free => {
                self.relative_click(world, tol_mm)
            }
            (Tool::Point, _) => {
                let snap = self.snap_at(world, tol_mm);
                self.place_point(snap);
            }
            (Tool::Notch, ToolState::Idle) => match self.nearest_entity(world, tol_mm, false) {
                Some(reference) => {
                    self.state = ToolState::NotchOn { reference };
                    let name = entity_name(self.sheet(), reference);
                    self.info(tr_args("status.notch.next", &[("name", &name)]));
                }
                None => self.error(tr("status.notch.pick")),
            },
            (Tool::Notch, ToolState::NotchOn { .. }) => self.confirm(),
            (Tool::Segment | Tool::Grainline, ToolState::Idle) => {
                self.state = ToolState::SegmentFrom {
                    start: self.snap(world, tol_mm),
                };
                self.info(tr("status.segment.next"));
            }
            (Tool::Segment | Tool::Grainline, ToolState::SegmentFrom { start }) => {
                let typed =
                    !self.length_text.trim().is_empty() || !self.angle_text.trim().is_empty();
                // Maj : la direction verrouillée passe par l'aperçu (longueur de la souris).
                if typed || self.angle_lock {
                    self.confirm();
                } else {
                    let to = self.snap(world, tol_mm);
                    self.commit_segment(Command::AddSegment {
                        sheet: self.sheet,
                        from: start,
                        to,
                    });
                }
            }
            (Tool::Rectangle, ToolState::Idle) => {
                self.state = ToolState::RectFrom {
                    start: self.snap(world, tol_mm),
                };
                self.info(tr("status.rect.next"));
            }
            (Tool::Rectangle, ToolState::RectFrom { .. }) => self.confirm(),
            (Tool::Parallel, ToolState::Idle) => match self.nearest_entity(world, tol_mm, true) {
                Some(reference) => {
                    self.state = ToolState::ParallelOf { reference };
                    let name = entity_name(self.sheet(), reference);
                    self.info(tr_args("status.parallel.next", &[("name", &name)]));
                }
                None => self.error(tr("status.perp.pick_reference")),
            },
            (Tool::Parallel, ToolState::ParallelOf { .. }) => self.confirm(),
            (Tool::Perpendicular, ToolState::Idle) => {
                match self.nearest_entity(world, tol_mm, true) {
                    Some(reference) => {
                        self.state = ToolState::PerpPickStart { reference };
                        let name = entity_name(self.sheet(), reference);
                        self.info(tr_args("status.perp.reference", &[("name", &name)]));
                    }
                    None => self.error(tr("status.perp.pick_reference")),
                }
            }
            (Tool::Perpendicular, ToolState::PerpPickStart { reference }) => {
                self.state = ToolState::PerpFrom {
                    reference,
                    start: self.snap(world, tol_mm),
                };
                self.info(tr("status.perp.next"));
            }
            (Tool::Perpendicular, ToolState::PerpFrom { .. }) => self.confirm(),
            (Tool::PointOnSegment, ToolState::Idle) => {
                match self.nearest_entity(world, tol_mm, true) {
                    Some(segment) => {
                        self.state = ToolState::PointOn {
                            segment,
                            origin: SegmentEnd::Start,
                        };
                        self.info(tr("status.point_on.next"));
                    }
                    None => self.error(tr("status.point_on.pick")),
                }
            }
            (Tool::PointOnSegment, ToolState::PointOn { .. }) => self.confirm(),
            (Tool::Curve, ToolState::Idle) => {
                self.state = ToolState::CurveFrom {
                    start: self.snap(world, tol_mm),
                };
                self.info(tr("status.curve.end"));
            }
            (Tool::Curve, ToolState::CurveFrom { start }) => {
                let end = self.snap(world, tol_mm);
                self.commit_curve(start, end);
            }
            (Tool::Spline, ToolState::Idle | ToolState::SplineNext) => {
                let target = self.snap(world, tol_mm);
                let at = self.ref_pos(target);
                let repeated = at.is_some_and(|p| {
                    self.spline_points
                        .iter()
                        .filter_map(|r| self.ref_pos(*r))
                        .any(|q| pf_geometry::distance_mm(p, q) <= MERGE_TOLERANCE_MM)
                });
                if repeated {
                    self.error(tr("status.spline.duplicate"));
                } else {
                    self.spline_points.push(target);
                    self.state = ToolState::SplineNext;
                    self.info(tr_args(
                        "status.spline.next",
                        &[("n", &self.spline_points.len().to_string())],
                    ));
                }
            }
            (Tool::Circle | Tool::Arc, ToolState::Idle) => {
                let start = self.snap(world, tol_mm);
                match (self.tool, self.round_mode) {
                    (_, RoundMode::ThreePoints | RoundMode::TwoPointsRadius) => {
                        self.state = ToolState::RoundFirst { first: start };
                        self.info(tr("status.round.second"));
                    }
                    (Tool::Circle, RoundMode::Center) => {
                        self.state = ToolState::CircleCenter { center: start };
                        self.info(tr("status.circle.radius"));
                    }
                    (_, RoundMode::Center) => {
                        self.state = ToolState::ArcCenter { center: start };
                        self.info(tr("status.arc.start"));
                    }
                }
            }
            (Tool::Circle | Tool::Arc, ToolState::RoundFirst { first }) => {
                self.state = ToolState::RoundSecond {
                    first,
                    second: self.snap(world, tol_mm),
                };
                self.info(if self.round_mode == RoundMode::ThreePoints {
                    tr("status.round.third")
                } else {
                    tr("status.round.radius")
                });
            }
            (
                Tool::Circle | Tool::Arc,
                ToolState::CircleCenter { .. }
                | ToolState::ArcCenter { .. }
                | ToolState::ArcSweep { .. }
                | ToolState::RoundSecond { .. },
            ) => self.confirm(),
            (Tool::Dimension, ToolState::Idle) => match self.nearest_point(world, tol_mm) {
                Some((from, _)) => {
                    self.state = ToolState::DimensionFrom { from };
                    let label = self.sheet().label(from).to_string();
                    self.info(tr_args("status.dimension.next", &[("label", &label)]));
                }
                None => self.error(tr("status.dimension.pick")),
            },
            (Tool::Dimension, ToolState::DimensionFrom { .. }) => {
                if self.nearest_point(world, tol_mm).is_none() {
                    self.error(tr("status.dimension.pick_second"));
                } else {
                    self.confirm();
                }
            }
            (Tool::Fillet | Tool::Chamfer, ToolState::Idle) => {
                match self.join_target(world, tol_mm) {
                    Ok(first) => {
                        self.state = ToolState::JoinFirst { first, pick: world };
                        self.info(tr("status.join.second"));
                    }
                    Err(msg) => self.error(msg),
                }
            }
            (Tool::Fillet | Tool::Chamfer, ToolState::JoinFirst { first, pick }) => {
                match self.join_target(world, tol_mm) {
                    Ok(second) if second == first => self.error(tr("status.join.same")),
                    Ok(second) => {
                        self.state = ToolState::JoinReady {
                            first,
                            pick_first: pick,
                            second,
                            pick_second: world,
                        };
                        self.info(if self.tool == Tool::Fillet {
                            tr("status.join.ready_fillet")
                        } else {
                            tr("status.join.ready_chamfer")
                        });
                    }
                    Err(msg) => self.error(msg),
                }
            }
            (Tool::Fillet | Tool::Chamfer, ToolState::JoinReady { .. }) => self.confirm(),
            (Tool::Tangent, ToolState::Idle) => {
                if let Some((id, _)) = self.nearest_point(world, tol_mm) {
                    self.state = ToolState::TangentFrom {
                        from: PointRef::Existing(id),
                    };
                    self.info(tr("status.tangent.from_next"));
                } else if let Some(entity) = self
                    .nearest_entity(world, tol_mm, false)
                    .filter(|e| !self.is_segment(*e))
                {
                    self.state = ToolState::TangentAt { entity };
                    self.info(tr("status.tangent.at_next"));
                } else {
                    self.state = ToolState::TangentFrom {
                        from: self.snap(world, tol_mm),
                    };
                    self.info(tr("status.tangent.from_next"));
                }
            }
            (Tool::Tangent, ToolState::TangentFrom { .. } | ToolState::TangentAt { .. }) => {
                self.commit_tangent(world, tol_mm)
            }
            (Tool::Measure, ToolState::MeasureFrom { start }) => {
                let to = self.snap_at(world, tol_mm).pos;
                self.last_measure = Some((start, to));
                self.state = ToolState::Idle;
                let d = pf_geometry::distance_mm(start, to);
                self.info(tr_args(
                    "status.measure.result",
                    &[("value", &format_mm(d))],
                ));
            }
            (Tool::Measure, _) => {
                let start = self.snap_at(world, tol_mm).pos;
                self.last_measure = None;
                self.state = ToolState::MeasureFrom { start };
                self.info(tr("status.measure.second"));
            }
            (Tool::Extend, ToolState::Idle | ToolState::ExtendFrom { .. }) => {
                self.extend_click(world, tol_mm)
            }
            (Tool::Split, ToolState::Idle) => self.split_click(world, tol_mm),
            (Tool::Offset, ToolState::Idle) => self.offset_pick(world, tol_mm),
            (Tool::Offset, ToolState::OffsetReady) => self.confirm(),
            (Tool::Merge, ToolState::Idle) => match self.nearest_entity(world, tol_mm, false) {
                Some(first) => {
                    self.state = ToolState::MergeFirst { first };
                    self.info(tr("status.merge.next"));
                }
                None => self.error(tr("status.merge.pick")),
            },
            (Tool::Merge, ToolState::MergeFirst { first }) => {
                match self.nearest_entity(world, tol_mm, false) {
                    Some(second) if second == first => self.error(tr("status.merge.same")),
                    Some(second) => {
                        let cmd = Command::MergePaths {
                            sheet: self.sheet,
                            first,
                            second,
                        };
                        if let Some(out) = self.exec(cmd) {
                            self.selection.clear();
                            self.selection.entities.extend(out.entities.iter().copied());
                            self.state = ToolState::Idle;
                            self.reset_inputs();
                            if out.notes.is_empty() {
                                let len = self.entity_len(first);
                                self.info(tr_args("status.merge.done", &[("length", &len)]));
                            }
                        }
                    }
                    None => self.error(tr("status.merge.pick")),
                }
            }
            (Tool::ExtractPiece, _) => self.extract_piece_click(world),
            (Tool::Dart, _) => self.dart_click(world, tol_mm),
            (Tool::Trace, _) => self.trace_click(world, tol_mm),
            (Tool::Guide, ToolState::Idle) => self.guide_click(world, tol_mm),
            (Tool::Guide, ToolState::GuideFrom { through }) => {
                if self.angle_text.trim().is_empty() {
                    let to = self.snap_at(world, tol_mm).pos;
                    self.commit_guide_through(through, to);
                } else {
                    self.confirm();
                }
            }
            // États incohérents avec l'outil : on repart proprement.
            _ => self.state = ToolState::Idle,
        }
    }

    /// Entrée : valide l'étape en cours avec les valeurs de l'aperçu.
    pub fn confirm(&mut self) {
        let (before, done) = (self.repeat_snapshot(), self.exec_count);
        self.confirm_step();
        self.remember_if_done(before, done);
    }

    fn confirm_step(&mut self) {
        if self.state == ToolState::Idle && !self.position_text.trim().is_empty() {
            self.confirm_position();
            return;
        }
        if self.tool == Tool::Guide {
            self.confirm_guide();
            return;
        }
        if self.tool == Tool::Point && self.state == ToolState::RelativePick {
            self.commit_relative(self.cursor);
            return;
        }
        if self.state == ToolState::SplineNext {
            self.commit_spline();
            return;
        }
        if matches!(
            self.state,
            ToolState::TangentFrom { .. } | ToolState::TangentAt { .. }
        ) {
            self.commit_tangent(self.cursor, self.pick_tol_mm);
            return;
        }
        if self.state == ToolState::OffsetReady {
            self.commit_offset();
            return;
        }
        if matches!(self.state, ToolState::DartBase { .. }) {
            self.commit_dart();
            return;
        }
        if let ToolState::ExtendFrom { entity, end } = self.state {
            match self.typed_length() {
                Ok(Some(delta_mm)) => {
                    let cmd = Command::ExtendPath {
                        sheet: self.sheet,
                        entity,
                        end,
                        delta_mm,
                    };
                    self.commit_extend(cmd, entity);
                }
                Ok(None) => self.error(tr("status.extend.value")),
                Err(e) => self.error(e),
            }
            return;
        }
        if let ToolState::JoinReady {
            first,
            pick_first,
            second,
            pick_second,
        } = self.state
        {
            let radius = match self.typed_length() {
                Ok(Some(r)) => r,
                Ok(None) => {
                    self.error(if self.tool == Tool::Fillet {
                        tr("status.join.radius")
                    } else {
                        tr("status.join.distance")
                    });
                    return;
                }
                Err(e) => {
                    self.error(e);
                    return;
                }
            };
            let sheet = self.sheet;
            let cmd = if self.tool == Tool::Fillet {
                Command::FilletPaths {
                    sheet,
                    first,
                    second,
                    radius_mm: radius,
                    pick_first,
                    pick_second,
                }
            } else {
                let second_distance = match self.typed_height() {
                    Ok(v) => v.unwrap_or(radius),
                    Err(e) => {
                        self.error(e);
                        return;
                    }
                };
                Command::ChamferPaths {
                    sheet,
                    first,
                    second,
                    distance_first_mm: radius,
                    distance_second_mm: second_distance,
                    pick_first,
                    pick_second,
                }
            };
            self.commit_join(cmd);
            return;
        }
        let preview = match self.preview() {
            Ok(Some(p)) => p,
            Ok(None) => {
                if self.state != ToolState::Idle {
                    self.error(tr("status.confirm.nothing"));
                }
                return;
            }
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let sheet = self.sheet;
        match (self.state, preview) {
            (
                ToolState::SegmentFrom { start },
                Preview::Segment {
                    length_mm,
                    angle_deg,
                    ..
                },
            ) => {
                self.commit_segment(Command::AddSegmentPolar {
                    sheet,
                    from: start,
                    length_mm,
                    angle_deg,
                });
            }
            (
                ToolState::PerpFrom { reference, start },
                Preview::Perpendicular {
                    length_mm, side, ..
                },
            ) => {
                self.commit_segment(Command::AddPerpendicular {
                    sheet,
                    reference,
                    from: start,
                    length_mm,
                    side,
                });
            }
            (
                ToolState::NotchOn { reference },
                Preview::Notch {
                    distance_mm,
                    side,
                    length_mm,
                    style,
                    ..
                },
            ) => {
                let cmd = Command::AddNotch {
                    sheet,
                    entity: reference,
                    distance_mm,
                    side,
                    length_mm,
                    style,
                };
                if let Some(out) = self.exec(cmd) {
                    self.selection.clear();
                    self.selection.notches = out.notches;
                    self.state = ToolState::Idle;
                    self.reset_inputs();
                    self.info(tr_args(
                        "status.notch.placed",
                        &[("value", &format_mm(distance_mm))],
                    ));
                }
            }
            (
                ToolState::RectFrom { start },
                Preview::Rectangle {
                    width_mm,
                    height_mm,
                    ..
                },
            ) => {
                let cmd = if self.rect_kept {
                    Command::AddConstructedRectangle {
                        sheet,
                        corner: start,
                        width_mm,
                        height_mm,
                        kept: true,
                    }
                } else {
                    Command::AddRectangle {
                        sheet,
                        corner: start,
                        width_mm,
                        height_mm,
                    }
                };
                if let Some(out) = self.exec(cmd) {
                    self.selection.clear();
                    self.selection.entities = out.entities.clone();
                    self.state = ToolState::Idle;
                    self.reset_inputs();
                    self.info(tr_args(
                        "status.rect.created",
                        &[
                            ("width", &format_mm(width_mm.abs())),
                            ("height", &format_mm(height_mm.abs())),
                        ],
                    ));
                }
            }
            (
                ToolState::ParallelOf { reference },
                Preview::Parallel {
                    distance_mm, side, ..
                },
            ) => {
                self.commit_segment(Command::AddParallel {
                    sheet,
                    reference,
                    distance_mm,
                    side,
                });
            }
            (ToolState::PointOn { segment, origin }, Preview::PointOn { distance_mm, .. }) => {
                let expr = self
                    .typed_param(&self.length_text, false, "error.length")
                    .ok()
                    .flatten()
                    .and_then(|t| t.expr);
                let cmd = match expr {
                    Some(expr) => Command::AddPointWithFormulas {
                        sheet,
                        definition: PointDefinition::OnSegment {
                            segment,
                            distance_mm,
                            origin,
                        },
                        formulas: vec![pf_core::ParamExpr {
                            param: pf_core::ParamKind::SegmentDistance,
                            expr,
                        }],
                    },
                    None => Command::AddPointOnSegment {
                        sheet,
                        segment,
                        distance_mm,
                        origin,
                    },
                };
                if let Some(out) = self.exec(cmd) {
                    self.selection.clear();
                    self.selection.points = out.points.clone();
                    let label = self.sheet().label(out.points[0]).to_string();
                    self.state = ToolState::Idle;
                    self.reset_inputs();
                    self.info(tr_args(
                        "status.point.placed_at",
                        &[("label", &label), ("value", &format_mm(distance_mm))],
                    ));
                }
            }
            (
                ToolState::DimensionFrom { .. },
                Preview::Dimension {
                    from,
                    to,
                    geometry,
                    offset_mm,
                },
            ) => {
                let cmd = Command::AddDimension {
                    sheet,
                    from,
                    to,
                    offset_mm,
                };
                if let Some(out) = self.exec(cmd) {
                    self.selection.clear();
                    self.selection.dimensions = out.dimensions;
                    self.state = ToolState::Idle;
                    self.reset_inputs();
                    self.info(tr_args(
                        "status.dimension.created",
                        &[("value", &format_mm(geometry.value_mm))],
                    ));
                }
            }
            (ToolState::CurveFrom { start }, Preview::Curve { to, .. }) => {
                self.commit_curve(start, PointRef::New(to));
            }
            (ToolState::CircleCenter { center }, Preview::Circle { radius_mm, .. }) => {
                self.commit_round(Command::AddCircle {
                    sheet,
                    center,
                    radius_mm,
                });
            }
            (
                ToolState::ArcCenter { center },
                Preview::Segment {
                    length_mm,
                    angle_deg,
                    ..
                },
            ) => {
                self.state = ToolState::ArcSweep {
                    center,
                    radius_mm: length_mm,
                    start_deg: angle_deg,
                };
                self.length_text.clear();
                self.angle_text.clear();
                self.info(tr("status.arc.end"));
            }
            (
                ToolState::ArcSweep {
                    center,
                    radius_mm,
                    start_deg,
                },
                Preview::Arc { end_deg, ccw, .. },
            ) => {
                self.commit_round(Command::AddArc {
                    sheet,
                    center,
                    radius_mm,
                    start_deg,
                    end_deg,
                    ccw,
                });
            }
            (ToolState::RoundSecond { first, second }, preview) => {
                let third = self.snap(self.cursor, self.pick_tol_mm);
                let side_of_center = |center: PointMm| {
                    let (a, b) = (self.ref_pos(first), self.ref_pos(second));
                    match (a, b) {
                        (Some(a), Some(b)) => {
                            pf_geometry::side_of(a, b, center).unwrap_or(Side::Left)
                        }
                        _ => Side::Left,
                    }
                };
                let cmd = match (self.tool, self.round_mode, preview) {
                    (Tool::Circle, RoundMode::ThreePoints, _) => Command::AddCircleThreePoints {
                        sheet,
                        a: first,
                        b: second,
                        c: third,
                    },
                    (Tool::Arc, RoundMode::ThreePoints, _) => Command::AddArcThreePoints {
                        sheet,
                        start: first,
                        through: second,
                        end: third,
                    },
                    (
                        Tool::Circle,
                        RoundMode::TwoPointsRadius,
                        Preview::Circle {
                            center, radius_mm, ..
                        },
                    ) => Command::AddCircleTwoPointsRadius {
                        sheet,
                        a: first,
                        b: second,
                        radius_mm,
                        side: side_of_center(center),
                    },
                    (
                        Tool::Arc,
                        RoundMode::TwoPointsRadius,
                        Preview::Arc {
                            center,
                            radius_mm,
                            ccw,
                            ..
                        },
                    ) => Command::AddArcTwoPointsRadius {
                        sheet,
                        start: first,
                        end: second,
                        radius_mm,
                        side: side_of_center(center),
                        ccw,
                    },
                    _ => return,
                };
                self.commit_round(cmd);
            }
            _ => {}
        }
    }

    // ----- saisie directe (US-16.2) ----------------------------------------

    /// Pose un point libre ou accroché : clic de l'outil Point et saisie d'une position.
    fn place_point(&mut self, snap: Snap) {
        let cmd = Command::AddPointAt {
            sheet: self.sheet,
            point: snap.target,
        };
        if let Some(out) = self.exec(cmd) {
            let id = match (out.points.first(), snap.target) {
                (Some(id), _) => Some(*id),
                (None, PointRef::Existing(id)) => Some(id),
                _ => self.nearest_point(snap.pos, 1e-6).map(|(id, _)| id),
            };
            self.selection.clear();
            self.selection.points.extend(id);
            let label = id
                .map(|i| self.sheet().label(i).to_string())
                .unwrap_or_default();
            if out.points.is_empty() {
                self.info(tr_args("status.point.exists", &[("label", &label)]));
            } else if snap.kind == SnapKind::Free {
                self.info(tr_args("status.point.placed", &[("label", &label)]));
            } else {
                self.info(tr_args(
                    "status.point.placed_snap",
                    &[
                        ("label", &label),
                        ("kind", &snap.kind.label().to_lowercase()),
                    ],
                ));
            }
        }
    }

    /// Vrai si l'outil attend un premier point : une position peut être saisie
    /// au clavier (`x;y`, `@dx;dy`, `@L<A`) au lieu d'un clic.
    pub fn accepts_position(&self) -> bool {
        self.state == ToolState::Idle
            && match self.tool {
                Tool::Point => self.point_mode == PointMode::Free,
                Tool::Circle | Tool::Arc => self.round_mode == RoundMode::Center,
                Tool::Segment | Tool::Grainline | Tool::Rectangle | Tool::Curve => true,
                _ => false,
            }
    }

    /// Origine des coordonnées relatives quand aucun tracé n'est en cours : le
    /// dernier point posé, à défaut le dernier point de la feuille, à défaut
    /// l'origine du repère.
    pub fn relative_origin(&self) -> PointMm {
        self.last_placed
            .or_else(|| {
                let sheet = self.sheet();
                sheet.points.last().and_then(|p| sheet.position(p.id).ok())
            })
            .unwrap_or(PointMm::ORIGIN)
    }

    /// Entrée avec une position saisie : pose le premier point de l'outil à la
    /// position exacte (aucune aimantation). Texte invalide : message, rien ne change.
    fn confirm_position(&mut self) {
        if !self.accepts_position() {
            self.position_text.clear();
            return;
        }
        let at = match parse_coordinate(&self.position_text, self.relative_origin()) {
            Ok(c) => c.point(),
            Err(e) => {
                self.error(tr_args("error.coord", &[("detail", &e)]));
                return;
            }
        };
        let start = PointRef::New(at);
        match self.tool {
            Tool::Point => {
                let done = self.exec_count;
                self.place_point(Snap {
                    kind: SnapKind::Free,
                    pos: at,
                    target: start,
                });
                if self.exec_count != done {
                    self.position_text.clear();
                }
                return;
            }
            Tool::Segment | Tool::Grainline => {
                self.state = ToolState::SegmentFrom { start };
                self.info(tr("status.segment.next"));
            }
            Tool::Rectangle => {
                self.state = ToolState::RectFrom { start };
                self.info(tr("status.rect.next"));
            }
            Tool::Curve => {
                self.state = ToolState::CurveFrom { start };
                self.info(tr("status.curve.end"));
            }
            Tool::Circle => {
                self.state = ToolState::CircleCenter { center: start };
                self.info(tr("status.circle.radius"));
            }
            _ => {
                self.state = ToolState::ArcCenter { center: start };
                self.info(tr("status.arc.start"));
            }
        }
        self.last_placed = Some(at);
        self.position_text.clear();
        self.entry_field = 0;
    }

    /// Nombre de champs de la bulle de saisie pour l'étape en cours (0 = pas de bulle).
    fn entry_row_count(&self) -> usize {
        match self.state {
            ToolState::Idle => usize::from(self.accepts_position()),
            ToolState::SegmentFrom { .. } | ToolState::RectFrom { .. } => 2,
            ToolState::PerpFrom { .. }
            | ToolState::ParallelOf { .. }
            | ToolState::PointOn { .. }
            | ToolState::CircleCenter { .. } => 1,
            _ => 0,
        }
    }

    /// Vrai si la bulle de saisie près du curseur est affichée.
    pub fn entry_active(&self) -> bool {
        self.entry_row_count() > 0
    }

    /// Champs de la bulle de saisie : texte tapé (les mêmes `length_text`,
    /// `angle_text` que la palette) et mesure observée en temps réel.
    pub fn entry_rows(&self) -> Vec<EntryRow> {
        let count = self.entry_row_count();
        if count == 0 {
            return Vec::new();
        }
        let live = self.preview().ok().flatten();
        let mm = |v: f64| Some(pf_core::units::len_number(v, 2));
        let deg = |v: f64| Some(format_number(v, 2));
        let unit = pf_core::units::display_units().unit.suffix();
        let active = self.entry_field.min(count - 1);
        let row = |i: usize, label: String, unit: &'static str, typed: &str, observed| EntryRow {
            label,
            unit,
            typed: typed.to_string(),
            observed,
            active: i == active,
        };
        match (self.state, live) {
            (ToolState::Idle, _) => {
                let at = self.aim();
                let observed = pf_core::units::len_pair(at.x, at.y, 2);
                vec![row(
                    0,
                    tr("bubble.position"),
                    "",
                    &self.position_text,
                    Some(observed),
                )]
            }
            (ToolState::SegmentFrom { .. }, live) => {
                let (length, angle) = match live {
                    Some(Preview::Segment {
                        length_mm,
                        angle_deg,
                        ..
                    }) => (mm(length_mm), deg(angle_deg)),
                    _ => (None, None),
                };
                vec![
                    row(0, tr("palette.length"), unit, &self.length_text, length),
                    row(1, tr("palette.angle"), "°", &self.angle_text, angle),
                ]
            }
            (ToolState::RectFrom { .. }, live) => {
                let (width, height) = match live {
                    Some(Preview::Rectangle {
                        width_mm,
                        height_mm,
                        ..
                    }) => (mm(width_mm.abs()), mm(height_mm.abs())),
                    _ => (None, None),
                };
                vec![
                    row(0, tr("palette.width"), unit, &self.length_text, width),
                    row(1, tr("palette.height"), unit, &self.angle_text, height),
                ]
            }
            (state, live) => {
                let (label, value) = match (state, live) {
                    (
                        ToolState::PerpFrom { .. },
                        Some(Preview::Perpendicular { length_mm, .. }),
                    ) => (tr("palette.length"), mm(length_mm)),
                    (ToolState::PerpFrom { .. }, _) => (tr("palette.length"), None),
                    (ToolState::ParallelOf { .. }, Some(Preview::Parallel { distance_mm, .. }))
                    | (ToolState::PointOn { .. }, Some(Preview::PointOn { distance_mm, .. })) => {
                        (tr("palette.distance"), mm(distance_mm))
                    }
                    (ToolState::CircleCenter { .. }, Some(Preview::Circle { radius_mm, .. })) => {
                        (tr("palette.radius"), mm(radius_mm))
                    }
                    (ToolState::CircleCenter { .. }, _) => (tr("palette.radius"), None),
                    _ => (tr("palette.distance"), None),
                };
                vec![row(0, label, unit, &self.length_text, value)]
            }
        }
    }

    /// Caractères acceptés par la bulle : chiffres, séparateurs, signes et
    /// opérateurs de coordonnée (`@`, `;`, `<`, `°`).
    pub fn is_entry_char(c: char) -> bool {
        c.is_ascii_digit() || matches!(c, '.' | ',' | ';' | '<' | '@' | '-' | '+' | '°')
    }

    /// Champ de saisie actif de la bulle : la position avant le premier point,
    /// sinon `length_text` ou `angle_text` (les champs de la palette).
    fn entry_buffer_mut(&mut self) -> Option<&mut String> {
        let count = self.entry_row_count();
        if count == 0 {
            None
        } else if self.accepts_position() {
            Some(&mut self.position_text)
        } else if self.entry_field.min(count - 1) == 0 {
            Some(&mut self.length_text)
        } else {
            Some(&mut self.angle_text)
        }
    }

    /// Ajoute un caractère au champ actif de la bulle (les autres sont ignorés).
    pub fn type_entry(&mut self, c: char) {
        if !Self::is_entry_char(c) {
            return;
        }
        if let Some(buffer) = self.entry_buffer_mut() {
            if buffer.chars().count() < MAX_ENTRY_CHARS {
                buffer.push(c);
            }
        }
    }

    /// Efface le dernier caractère du champ actif de la bulle.
    pub fn backspace_entry(&mut self) {
        if let Some(buffer) = self.entry_buffer_mut() {
            buffer.pop();
        }
    }

    /// Entrée doit-elle valider quelque chose ? (étape en cours, ligne d'aide
    /// ou position saisie).
    pub fn wants_confirm(&self) -> bool {
        self.state != ToolState::Idle
            || self.tool == Tool::Guide
            || !self.position_text.trim().is_empty()
    }

    // ----- pièces (US-04.1) ------------------------------------------------

    /// Prochain nom de la règle de nommage de la cible, si la case est cochée et qu'une règle
    /// applicable existe (US-11.5) ; sinon `None` : le nom par défaut est utilisé.
    pub fn rule_name(&self, target: pf_core::NamingTarget, enabled: bool) -> Option<String> {
        if !enabled {
            return None;
        }
        let rule = self.project().naming_rule_for(target)?;
        self.project().next_name(rule.id).ok()
    }

    /// Nom proposé pour une nouvelle pièce : « Pièce n », premier numéro libre.
    fn next_piece_name(&self) -> String {
        if let Some(name) = self.rule_name(pf_core::NamingTarget::Piece, self.piece_use_rule) {
            return name;
        }
        let taken: Vec<&str> = self
            .project()
            .pieces
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        (self.project().pieces.len() + 1..)
            .map(|n| tr_args("piece.default_name", &[("n", &n.to_string())]))
            .find(|name| !taken.contains(&name.as_str()))
            .unwrap_or_default()
    }

    /// Message traduit d'une détection de région impossible.
    fn region_error_text(&self, e: &RegionError) -> String {
        let pos = |p: PointMm| pf_core::units::len_pair(p.x, p.y, 2);
        let traced = |index: usize| {
            let (ids, _) = self.sheet().region_traces();
            ids.get(index)
                .map(|id| entity_name(self.sheet(), *id))
                .unwrap_or_default()
        };
        match e {
            RegionError::Open(g) => tr_args(
                "status.piece.open",
                &[
                    ("gap", &format_mm(g.gap_mm)),
                    ("from", &pos(g.from)),
                    ("to", &pos(g.to)),
                ],
            ),
            RegionError::NoRegion => tr("status.piece.none"),
            RegionError::InvalidClick => tr("status.piece.invalid_click"),
            RegionError::SelfTouching => tr("status.piece.self_touch"),
            RegionError::Overlap { first, second } => tr_args(
                "status.piece.overlap",
                &[("first", &traced(*first)), ("second", &traced(*second))],
            ),
            RegionError::InvalidTrace { index, .. } => {
                tr_args("status.piece.invalid_trace", &[("name", &traced(*index))])
            }
        }
    }

    /// Clic de l'outil Extraire une pièce : la plus petite région fermée sous le
    /// curseur devient une pièce ; sinon, message (avec l'ouverture surlignée).
    fn extract_piece_click(&mut self, world: PointMm) {
        self.piece_gap = None;
        // Diagnostic structuré avant la commande : texte traduit, extrémités à surligner.
        if let Err(e) = self.sheet().find_region_at(world) {
            if let RegionError::Open(g) = &e {
                self.piece_gap = Some(PieceGap {
                    from: g.from,
                    to: g.to,
                    gap_mm: g.gap_mm,
                });
            }
            let text = self.region_error_text(&e);
            self.error(text);
            return;
        }
        let name = self.next_piece_name();
        let cmd = Command::ExtractPiece {
            sheet: self.sheet,
            source: PieceSource::Click { at: world },
            name: name.clone(),
            detached: self.piece_detached,
        };
        self.run_extract(cmd, &name);
    }

    // ----- séparer une pièce (US-04.6) ---------------------------------------

    /// Démarre la séparation de `piece` : deux clics posent la ligne de coupe (une droite
    /// infinie) ; le résultat est une seule commande annulable.
    pub fn begin_split(&mut self, piece: PieceId) {
        let Ok(p) = self.project().piece(piece) else {
            return;
        };
        let name = p.name.clone();
        self.set_tool(Tool::Select);
        self.split_job = Some(SplitJob { piece, first: None });
        self.info(tr_args("status.split.first", &[("name", &name)]));
    }

    /// Abandonne la séparation en cours.
    pub fn cancel_split(&mut self) {
        if self.split_job.take().is_some() {
            self.info(tr("status.cancelled"));
        }
    }

    fn split_piece_click(&mut self, world: PointMm, tol_mm: f64) {
        let Some(job) = self.split_job else {
            return;
        };
        let at = self.snap_at(world, tol_mm).pos;
        let Some(first) = job.first else {
            self.split_job = Some(SplitJob {
                first: Some(at),
                ..job
            });
            self.info(tr("status.split.second"));
            return;
        };
        if pf_geometry::distance_mm(first, at) <= EPSILON_MM {
            self.error(tr("status.split.same_point"));
            return;
        }
        let name = self
            .project()
            .piece(job.piece)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let cmd = Command::SplitPiece {
            piece: job.piece,
            a: first,
            b: at,
        };
        let Some(out) = self.exec(cmd) else {
            // Refus : la raison est affichée, on repart du premier point.
            self.split_job = Some(SplitJob { first: None, ..job });
            return;
        };
        self.split_job = None;
        let describe = |ed: &Self, i: usize| -> (String, String, String) {
            let Some(&id) = out.pieces.get(i) else {
                return Default::default();
            };
            let name = ed
                .project()
                .piece(id)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let area = ed
                .project()
                .piece_contour(id)
                .map(|c| format_number(c.area_mm2 / 100.0, 2))
                .unwrap_or_default();
            let sheet = ed
                .project()
                .piece_sheet(id)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            (name, area, sheet)
        };
        let (n1, a1, s1) = describe(self, 0);
        let (n2, a2, s2) = describe(self, 1);
        let mut text = tr_args(
            "status.split_piece.done",
            &[
                ("name", &name),
                ("first", &n1),
                ("first_area", &a1),
                ("first_sheet", &s1),
                ("second", &n2),
                ("second_area", &a2),
                ("second_sheet", &s2),
            ],
        );
        if !out.notes.is_empty() {
            text.push(' ');
            text.push_str(&tr_args("note.core", &[("text", &out.notes.join(" "))]));
        }
        self.info(text);
    }

    /// Aperçu de la coupe : ligne du premier point au curseur, points de coupe et mesures
    /// observées des deux moitiés (ou la raison du refus). Lecture seule.
    pub fn split_draft(&self) -> Option<SplitDraft> {
        let job = self.split_job?;
        let from = job.first?;
        let to = self.aim();
        if pf_geometry::distance_mm(from, to) <= EPSILON_MM {
            return None;
        }
        let preview = self
            .project()
            .split_preview(job.piece, from, to)
            .map_err(|e| core_error(&e));
        Some(SplitDraft { from, to, preview })
    }

    /// Crée une pièce depuis les tracés sélectionnés (s'ils forment un contour fermé).
    pub fn piece_from_selection(&mut self) {
        let name = self.next_piece_name();
        let cmd = Command::ExtractPiece {
            sheet: self.sheet,
            source: PieceSource::Paths {
                entities: self.selection.entities.clone(),
            },
            name: name.clone(),
            detached: self.piece_detached,
        };
        self.run_extract(cmd, &name);
    }

    /// Les tracés sélectionnés forment-ils une chaîne fermée simple ?
    pub fn selection_is_closed_contour(&self) -> bool {
        !self.selection.entities.is_empty()
            && pf_core::contour::contour_from_entities(self.sheet(), &self.selection.entities)
                .is_ok()
    }

    fn run_extract(&mut self, cmd: Command, name: &str) {
        let Some(out) = self.exec(cmd) else { return };
        let Some(&piece) = out.pieces.first() else {
            return;
        };
        let (area, perimeter) = match self.project().piece_contour(piece) {
            Ok(c) => (c.area_mm2, c.perimeter_mm),
            Err(_) => (0.0, 0.0),
        };
        let area = format_number(area / 100.0, 2);
        let perimeter = format_mm(perimeter);
        let mut text = match out.sheet.and_then(|s| self.project().sheet(s).ok()) {
            Some(copy) => tr_args(
                "status.piece.created_detached",
                &[
                    ("name", name),
                    ("sheet", &copy.name),
                    ("area", &area),
                    ("perimeter", &perimeter),
                ],
            ),
            None => tr_args(
                "status.piece.created",
                &[("name", name), ("area", &area), ("perimeter", &perimeter)],
            ),
        };
        if !out.notes.is_empty() {
            text.push(' ');
            text.push_str(&tr_args("note.core", &[("text", &out.notes.join(" "))]));
        }
        // Liée : le contour de la pièce est mis en évidence par la sélection.
        let outline: Vec<EntityId> = if out.sheet.is_none() {
            self.project()
                .piece(piece)
                .map(|p| p.outline.iter().map(|e| e.entity).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        self.selection.clear();
        self.selection.entities = outline;
        self.info(text);
    }

    // ----- répétition de la dernière commande (US-16.2) --------------------

    fn repeat_snapshot(&self) -> RepeatMemo {
        RepeatMemo {
            tool: self.tool,
            point_mode: self.point_mode,
            round_mode: self.round_mode,
            guide_mode: self.guide_mode,
            length_text: self.length_text.clone(),
            angle_text: self.angle_text.clone(),
        }
    }

    /// Mémorise `before` si un clic ou une validation vient d'aboutir à une commande
    /// avec un outil (jamais avec l'outil Sélection).
    fn remember_if_done(&mut self, before: RepeatMemo, exec_count_before: u64) {
        if self.exec_count != exec_count_before && before.tool != Tool::Select {
            self.repeat_memo = Some(before);
        }
    }

    /// Une valeur mémorisée qui ne se lit plus (nombre, coordonnée, division).
    fn unreadable_memo_value(&self, memo: &RepeatMemo) -> Option<String> {
        let division = memo.tool == Tool::Point && memo.point_mode == PointMode::Division;
        let length_ok = |t: &str| {
            t.trim().is_empty()
                || pf_core::units::parse_length_display(t).is_ok()
                || (is_coordinate_text(t) && parse_coordinate(t, PointMm::ORIGIN).is_ok())
                || (division && parse_division(t).is_ok())
        };
        let angle_ok = |t: &str| {
            t.trim().is_empty()
                || parse_angle_deg(t).is_ok()
                || pf_core::units::parse_length_display(t).is_ok()
        };
        if !length_ok(&memo.length_text) {
            Some(memo.length_text.clone())
        } else if !angle_ok(&memo.angle_text) {
            Some(memo.angle_text.clone())
        } else {
            None
        }
    }

    /// Réactive le dernier outil de création abouti, avec ses sous-modes et ses
    /// dernières valeurs saisies. Ce n'est pas un rejeu de commande : aucune
    /// commande n'est exécutée, le projet ne change pas. Valeurs illisibles :
    /// message, rien ne change.
    pub fn repeat_last(&mut self) {
        let Some(memo) = self.repeat_memo.clone() else {
            self.info(tr("status.repeat.none"));
            return;
        };
        if let Some(bad) = self.unreadable_memo_value(&memo) {
            self.error(tr_args("status.repeat.invalid", &[("value", &bad)]));
            return;
        }
        self.set_tool(memo.tool);
        match memo.tool {
            Tool::Point => self.set_point_mode(memo.point_mode),
            Tool::Circle | Tool::Arc => self.set_round_mode(memo.round_mode),
            Tool::Guide => self.set_guide_mode(memo.guide_mode),
            _ => {}
        }
        self.length_text.clone_from(&memo.length_text);
        self.angle_text.clone_from(&memo.angle_text);
        let values: Vec<&str> = [memo.length_text.trim(), memo.angle_text.trim()]
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect();
        let tool = memo.tool.label();
        self.info(if values.is_empty() {
            tr_args("status.repeat.armed_plain", &[("tool", &tool)])
        } else {
            tr_args(
                "status.repeat.armed",
                &[("tool", &tool), ("values", &values.join(" ; "))],
            )
        });
    }

    /// Crée un segment et lui donne le rôle « droit-fil » en une seule étape d'historique.
    fn exec_grainline(&mut self, cmd: Command) -> Option<Outcome> {
        self.doc.end_group();
        self.doc.begin_group();
        let sheet = self.sheet;
        let result = self.doc.execute(cmd).and_then(|out| {
            let entity = out
                .entities
                .first()
                .copied()
                .ok_or_else(|| CoreError::Invalid(tr("error.no_path_created")))?;
            self.doc
                .execute(Command::SetPathRole {
                    sheet,
                    entity,
                    role: PathRole::Grainline,
                })
                .map(|_| out)
        });
        match result {
            Ok(out) => {
                self.doc.end_group();
                Some(out)
            }
            Err(e) => {
                self.doc.cancel_group();
                self.error(core_error(&e));
                None
            }
        }
    }

    /// Clic de l'outil Ligne d'aide, sans étape en cours.
    fn guide_click(&mut self, world: PointMm, tol_mm: f64) {
        let at = self.snap_at(world, tol_mm).pos;
        let typed = match self.typed_length() {
            Ok(v) => v,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        match self.guide_mode {
            GuideMode::Horizontal => self.commit_guide(at.x, typed.unwrap_or(at.y), 0.0),
            GuideMode::Vertical => self.commit_guide(typed.unwrap_or(at.x), at.y, 90.0),
            GuideMode::Oblique => {
                self.state = ToolState::GuideFrom { through: at };
                self.length_text.clear();
                self.angle_text.clear();
                self.info(tr("status.guide.next"));
            }
        }
    }

    /// Entrée avec l'outil Ligne d'aide : position saisie (horizontale,
    /// verticale, point de passage) ou angle saisi (oblique).
    fn confirm_guide(&mut self) {
        match (self.state, self.guide_mode) {
            (ToolState::GuideFrom { through }, _) => match self.typed_angle() {
                Ok(Some(angle)) => self.commit_guide(through.x, through.y, angle),
                Ok(None) => {
                    let to = self.aim();
                    self.commit_guide_through(through, to);
                }
                Err(e) => self.error(e),
            },
            (_, GuideMode::Oblique) => match (self.typed_length(), self.typed_height()) {
                (Ok(Some(x)), Ok(Some(y))) => {
                    self.state = ToolState::GuideFrom {
                        through: PointMm::new(x, y),
                    };
                    self.length_text.clear();
                    self.angle_text.clear();
                    self.info(tr("status.guide.next"));
                }
                (Err(e), _) | (_, Err(e)) => self.error(e),
                _ => self.error(tr("status.guide.through")),
            },
            (_, mode) => match self.typed_length() {
                Ok(Some(v)) if mode == GuideMode::Horizontal => {
                    let x = self.cursor.x;
                    self.commit_guide(x, v, 0.0);
                }
                Ok(Some(v)) => {
                    let y = self.cursor.y;
                    self.commit_guide(v, y, 90.0);
                }
                Ok(None) => self.error(tr("status.guide.value")),
                Err(e) => self.error(e),
            },
        }
    }

    /// Guide passant par `through` et `to` (direction de l'un vers l'autre).
    fn commit_guide_through(&mut self, through: PointMm, to: PointMm) {
        match pf_geometry::direction_angle_deg(through, to) {
            Ok(angle) => self.commit_guide(through.x, through.y, self.locked_angle(angle)),
            Err(e) => self.error(core_error(&e)),
        }
    }

    /// Crée une ligne d'aide (une étape d'historique), la sélectionne et l'annonce.
    fn commit_guide(&mut self, x_mm: f64, y_mm: f64, angle_deg: f64) {
        let cmd = Command::AddGuide {
            sheet: self.sheet,
            x_mm,
            y_mm,
            angle_deg,
            axis: false,
            layer: None,
        };
        if let Some(out) = self.exec(cmd) {
            self.show_guides = true;
            self.selection.clear();
            self.selection.guides = out.guides;
            self.state = ToolState::Idle;
            self.reset_inputs();
            self.info(tr_args(
                "status.guide.created",
                &[
                    ("x", &format_mm(x_mm)),
                    ("y", &format_mm(y_mm)),
                    ("angle", &format_deg(angle_deg.rem_euclid(180.0))),
                ],
            ));
        }
    }

    fn commit_segment(&mut self, cmd: Command) {
        let created = if self.tool == Tool::Grainline {
            self.exec_grainline(cmd)
        } else {
            self.exec(cmd)
        };
        if let Some(out) = created {
            let id = out.entities[0];
            self.selection.clear();
            self.selection.entities.push(id);
            self.state = ToolState::Idle;
            self.reset_inputs();
            let sheet = self.sheet();
            let name = entity_name(sheet, id);
            let len = measure::entity_length(sheet, id)
                .map(|m| m.value)
                .unwrap_or(f64::NAN);
            let dir = measure::segment_direction_deg(sheet, id).unwrap_or(f64::NAN);
            self.info(tr_args(
                "status.segment.created",
                &[
                    ("name", &name),
                    ("length", &format_mm(len)),
                    ("direction", &format_deg(dir)),
                ],
            ));
        }
    }

    /// Crée un cercle ou un arc (une étape d'historique) et l'annonce.
    fn commit_round(&mut self, cmd: Command) {
        if let Some(out) = self.exec(cmd) {
            let Some(&id) = out.entities.first() else {
                return;
            };
            self.selection.clear();
            self.selection.entities.push(id);
            self.state = ToolState::Idle;
            self.reset_inputs();
            let sheet = self.sheet();
            let name = entity_name(sheet, id);
            let len = measure::entity_length(sheet, id)
                .map(|m| m.value)
                .unwrap_or(f64::NAN);
            let radius = sheet
                .shape(id)
                .map(|s| match s {
                    pf_core::Shape::Arc(a) => a.radius_mm,
                    pf_core::Shape::Circle(c) => c.circle.radius_mm,
                    _ => f64::NAN,
                })
                .unwrap_or(f64::NAN);
            self.info(tr_args(
                "status.round.created",
                &[
                    ("name", &name),
                    ("radius", &format_mm(radius)),
                    ("length", &format_mm(len)),
                ],
            ));
        }
    }

    /// Clic de l'outil Décalage : choisit la chaîne de tracés bout à bout qui
    /// contient le tracé visé (détection par les points partagés), ou toute la
    /// sélection si le tracé visé en fait partie (plusieurs tracés sélectionnés).
    fn offset_pick(&mut self, world: PointMm, tol_mm: f64) {
        let Some(hit) = self.nearest_entity(world, tol_mm, false) else {
            self.error(tr("status.offset.pick"));
            return;
        };
        let paths = if self.selection.entities.len() > 1 && self.selection.entities.contains(&hit) {
            self.selection.entities.clone()
        } else {
            match pf_core::offset::detect_chain(self.sheet(), hit) {
                Ok(p) => p,
                Err(e) => {
                    self.error(core_error(&e));
                    return;
                }
            }
        };
        if let Err(e) = pf_core::offset::ordered_chain(self.sheet(), &paths) {
            self.error(core_error(&e));
            return;
        }
        let count = paths.len().to_string();
        self.offset_paths = paths;
        self.offset_flip = false;
        self.length_text.clear();
        self.state = ToolState::OffsetReady;
        self.info(tr_args("status.offset.ready", &[("count", &count)]));
    }

    /// Distance signée du décalage en cours (`> 0` = à gauche du sens de la
    /// chaîne) : la valeur saisie (positive) du côté de la souris, ou la
    /// distance de la souris à la chaîne ; `offset_flip` prend l'autre côté.
    pub fn offset_distance(&self) -> Result<f64, String> {
        let aim = self.grid(self.cursor);
        let signed = pf_core::offset::offset_signed_distance(self.sheet(), &self.offset_paths, aim);
        let flip = if self.offset_flip { -1.0 } else { 1.0 };
        match self.typed_length()? {
            Some(t) if t <= 0.0 => Err(tr("status.offset.positive")),
            Some(t) => Ok(signed.map_or(1.0, f64::signum) * t * flip),
            None => signed.map(|s| s * flip).map_err(|e| core_error(&e)),
        }
    }

    /// Côté proposé pour le décalage en cours, de la souris : `true` = à gauche
    /// du sens de la chaîne.
    pub fn offset_left(&self) -> Option<bool> {
        self.offset_distance().ok().map(|d| d > 0.0)
    }

    /// Inverse le côté du décalage.
    pub fn flip_offset(&mut self) {
        self.offset_flip = !self.offset_flip;
    }

    /// Change le traitement des coins extérieurs.
    pub fn set_offset_corner(&mut self, corner: CornerStyle) {
        self.offset_corner = corner;
    }

    /// Exécute le décalage (une étape d'historique) et sélectionne le résultat.
    fn commit_offset(&mut self) {
        let d = match self.offset_distance() {
            Ok(d) => d,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let cmd = Command::OffsetPaths {
            sheet: self.sheet,
            paths: self.offset_paths.clone(),
            distance_mm: d,
            corner: self.offset_corner,
            role: self.offset_role,
        };
        let Some(out) = self.exec(cmd) else {
            return;
        };
        self.selection.clear();
        self.selection.entities.extend(out.entities.iter().copied());
        self.state = ToolState::Idle;
        self.reset_inputs();
        if out.notes.is_empty() {
            self.info(tr_args(
                "status.offset.done",
                &[
                    ("count", &out.entities.len().to_string()),
                    ("distance", &format_mm(d.abs())),
                ],
            ));
        }
    }

    /// Aperçu du décalage : lignes de la chaîne choisie et lignes du résultat
    /// (échantillonnées pour le dessin). `None` hors de l'étape Décalage.
    pub fn offset_draft(&self) -> Option<(Lines, Lines)> {
        if self.state != ToolState::OffsetReady {
            return None;
        }
        let sheet = self.sheet();
        let chain: Vec<Vec<PointMm>> = self
            .offset_paths
            .iter()
            .filter_map(|&id| sheet.shape(id).ok())
            .map(|s| s.sample(48))
            .collect();
        let d = self.offset_distance().ok()?;
        let key = (
            d.to_bits(),
            self.offset_corner,
            self.offset_paths.len(),
            sheet.entities.len(),
        );
        if let Some((k, lines)) = &*self.offset_cache.borrow() {
            if *k == key {
                return Some((chain, lines.clone()));
            }
        }
        let traces =
            pf_core::offset::offset_preview(sheet, &self.offset_paths, d, self.offset_corner)
                .unwrap_or_default();
        let lines: Vec<Vec<PointMm>> = traces
            .iter()
            .map(|t| match t {
                Trace::Line(a, b) => vec![*a, *b],
                Trace::Arc(a) => a.sample(48),
                Trace::Cubic(c) => c.sample(48),
                Trace::Spline(s) => s.sample(24),
                Trace::Circle(c) => (0..=96)
                    .map(|i| c.point_at_deg(360.0 * i as f64 / 96.0))
                    .collect(),
            })
            .collect();
        *self.offset_cache.borrow_mut() = Some((key, lines.clone()));
        Some((chain, lines))
    }

    /// Longueur d'un tracé, mise en forme (vide si elle ne se calcule pas).
    fn entity_len(&self, id: EntityId) -> String {
        measure::entity_length(self.sheet(), id)
            .map(|m| format_mm(m.value))
            .unwrap_or_default()
    }

    /// Clic de l'outil Prolonger : choisit le tracé et son extrémité la plus
    /// proche, puis, à l'étape suivante, le tracé jusqu'auquel aller.
    fn extend_click(&mut self, world: PointMm, tol_mm: f64) {
        let hit = self.nearest_entity(world, tol_mm, false);
        let editable = |ed: &Editor, id: EntityId| {
            matches!(
                ed.sheet().entity(id).map(|e| &e.kind),
                Ok(EntityKind::Segment { .. } | EntityKind::Arc { .. })
            )
        };
        match (self.state, hit) {
            (ToolState::ExtendFrom { entity, end }, Some(target)) if target != entity => {
                let cmd = Command::TrimPathTo {
                    sheet: self.sheet,
                    entity,
                    end,
                    target,
                    pick: world,
                };
                self.commit_extend(cmd, entity);
            }
            (ToolState::ExtendFrom { .. }, None) => self.error(tr("status.extend.target")),
            (_, Some(id)) if editable(self, id) => {
                let sheet = self.sheet();
                let end = match (sheet.path_length_mm(id), sheet.path_distance_of(id, world)) {
                    (Ok(len), Ok(d)) if d > len / 2.0 => SegmentEnd::End,
                    _ => SegmentEnd::Start,
                };
                self.state = ToolState::ExtendFrom { entity: id, end };
                self.length_text.clear();
                self.info(tr("status.extend.next"));
            }
            _ => self.error(tr("status.extend.pick")),
        }
    }

    /// Exécute un prolongement (une étape d'historique), sélectionne le tracé
    /// et annonce sa nouvelle longueur ; les remarques du cœur restent affichées.
    fn commit_extend(&mut self, cmd: Command, entity: EntityId) {
        let Some(out) = self.exec(cmd) else {
            return;
        };
        self.selection.clear();
        self.selection.entities.push(entity);
        self.state = ToolState::Idle;
        self.reset_inputs();
        if out.notes.is_empty() {
            let name = self.sheet().entity_name(entity);
            let length = self.entity_len(entity);
            self.info(tr_args(
                "status.extend.done",
                &[("name", &name), ("length", &length)],
            ));
        }
    }

    /// Point de coupe visé sur un tracé : un point existant du tracé, un point
    /// de passage d'une courbe lisse, sinon la projection du clic sur le tracé.
    fn split_target(&self, entity: EntityId, world: PointMm, tol_mm: f64) -> PointMm {
        let sheet = self.sheet();
        if let Ok(nodes) = sheet.spline_nodes(entity) {
            let near = nodes
                .iter()
                .map(|n| (n.position, pf_geometry::distance_mm(n.position, world)))
                .filter(|(_, d)| *d <= tol_mm)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((pos, _)) = near {
                return pos;
            }
        }
        if let Some((_, pos)) = self.nearest_point(world, tol_mm) {
            let on = sheet
                .shape(entity)
                .map(|s| s.distance_to_point(pos))
                .is_ok_and(|d| d <= pf_core::command::SPLIT_TOLERANCE_MM);
            if on {
                return pos;
            }
        }
        sheet
            .path_distance_of(entity, world)
            .and_then(|d| sheet.path_point_at(entity, d))
            .map(|(p, _)| p)
            .unwrap_or(world)
    }

    /// Clic de l'outil Couper : coupe le tracé visé à l'endroit du clic.
    fn split_click(&mut self, world: PointMm, tol_mm: f64) {
        let Some(entity) = self.nearest_entity(world, tol_mm, false) else {
            self.error(tr("status.split.pick"));
            return;
        };
        let at = self.split_target(entity, world, tol_mm);
        let cmd = Command::SplitPath {
            sheet: self.sheet,
            entity,
            at,
        };
        let Some(out) = self.exec(cmd) else {
            return;
        };
        self.selection.clear();
        self.selection.entities.push(entity);
        self.selection.entities.extend(out.entities.iter().copied());
        self.reset_inputs();
        if let Some(&second) = out.entities.first() {
            let (a, b) = (self.entity_len(entity), self.entity_len(second));
            self.info(tr_args(
                "status.split.done",
                &[("first", &a), ("second", &b)],
            ));
        }
    }

    /// Inverse le sens des tracés sélectionnés (une seule étape d'historique).
    pub fn reverse_selection(&mut self) {
        let ids = self.selection.entities.clone();
        if ids.is_empty() {
            self.error(tr("status.reverse.none"));
            return;
        }
        let sheet = self.sheet;
        let mut commands: Vec<Command> = ids
            .iter()
            .map(|&entity| Command::ReversePath { sheet, entity })
            .collect();
        let cmd = if commands.len() == 1 {
            commands.remove(0)
        } else {
            Command::Batch { commands }
        };
        if self.exec(cmd).is_some() {
            self.info(tr_args(
                "status.reverse.done",
                &[("count", &ids.len().to_string())],
            ));
        }
    }

    /// Symétrie liée (US-02.11) : la sélection (points, tracés) est reflétée
    /// par la ligne d'aide « axe » sélectionnée ; les copies sont maintenues.
    pub fn link_mirror_selection(&mut self) {
        let [guide] = self.selection.guides[..] else {
            self.error(tr("status.mirror.need_axis"));
            return;
        };
        if self.selection.points.is_empty() && self.selection.entities.is_empty() {
            self.error(tr("status.mirror.need_objects"));
            return;
        }
        let cmd = Command::LinkMirror {
            sheet: self.sheet,
            points: self.selection.points.clone(),
            entities: self.selection.entities.clone(),
            axis: MirrorAxis::Guide { guide },
        };
        if let Some(out) = self.exec(cmd) {
            self.info(tr_args(
                "status.mirror.done",
                &[("count", &out.points.len().to_string())],
            ));
        }
    }

    /// Déplie la symétrie de l'axe sélectionné, ou de l'axe d'un point miroir sélectionné.
    pub fn unfold_selection(&mut self) {
        let axis = match self.selection.guides[..] {
            [guide] => Some(MirrorAxis::Guide { guide }),
            _ => self.selection.points.iter().find_map(|&p| {
                match self.sheet().point(p).map(|p| p.definition) {
                    Ok(PointDefinition::Mirror { axis, .. }) => Some(axis),
                    _ => None,
                }
            }),
        };
        let Some(axis) = axis else {
            self.error(tr("status.unfold.none"));
            return;
        };
        let cmd = Command::UnfoldMirror {
            sheet: self.sheet,
            axis,
        };
        if self.exec(cmd).is_some() {
            self.info(tr("status.unfold.done"));
        }
    }

    fn is_segment(&self, id: EntityId) -> bool {
        matches!(
            self.sheet().entity(id).map(|e| &e.kind),
            Ok(EntityKind::Segment { .. })
        )
    }

    /// Tracé visé par un clic de congé ou de chanfrein : segment, arc ou
    /// cercle (segment seulement pour un chanfrein).
    fn join_target(&self, world: PointMm, tol_mm: f64) -> Result<EntityId, String> {
        let chamfer = self.tool == Tool::Chamfer;
        let bad = || {
            if chamfer {
                tr("status.join.pick_segment")
            } else {
                tr("status.join.pick")
            }
        };
        let id = self.nearest_entity(world, tol_mm, false).ok_or_else(bad)?;
        let kind = &self.sheet().entity(id).map_err(|_| bad())?.kind;
        match kind {
            EntityKind::Segment { .. } => Ok(id),
            EntityKind::Arc { .. } | EntityKind::Circle { .. } if !chamfer => Ok(id),
            _ => Err(bad()),
        }
    }

    /// Exécute un congé ou un chanfrein (une étape d'historique), sélectionne
    /// le raccord créé et l'annonce ; les remarques du cœur restent affichées.
    fn commit_join(&mut self, cmd: Command) {
        let fillet = matches!(cmd, Command::FilletPaths { .. });
        let Some(out) = self.exec(cmd) else {
            return;
        };
        let Some(&id) = out.entities.last() else {
            return;
        };
        self.selection.clear();
        self.selection.entities.push(id);
        self.state = ToolState::Idle;
        self.reset_inputs();
        if out.notes.is_empty() {
            let sheet = self.sheet();
            let len = measure::entity_length(sheet, id)
                .map(|m| m.value)
                .unwrap_or(f64::NAN);
            if fillet {
                let radius = sheet
                    .arc_geometry(id)
                    .map(|a| a.radius_mm)
                    .unwrap_or(f64::NAN);
                self.info(tr_args(
                    "status.fillet.created",
                    &[("radius", &format_mm(radius)), ("length", &format_mm(len))],
                ));
            } else {
                self.info(tr_args(
                    "status.chamfer.created",
                    &[("length", &format_mm(len))],
                ));
            }
        }
    }

    /// Pose la tangente de l'état courant (clic ou Entrée à la position `world`).
    fn commit_tangent(&mut self, world: PointMm, tol_mm: f64) {
        let sheet = self.sheet;
        let length = match self.typed_length() {
            Ok(v) => v,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        let cmd = match self.state {
            ToolState::TangentFrom { from } => {
                let target = self.nearest_entity(world, tol_mm, false).filter(|e| {
                    matches!(
                        self.sheet().entity(*e).map(|x| &x.kind),
                        Ok(EntityKind::Arc { .. } | EntityKind::Circle { .. })
                    )
                });
                let Some(target) = target else {
                    self.error(tr("status.tangent.pick_circle"));
                    return;
                };
                Command::AddTangentLine {
                    sheet,
                    from,
                    target,
                    pick: world,
                    length_mm: length,
                }
            }
            ToolState::TangentAt { entity } => {
                let backwards = self
                    .sheet()
                    .path_distance_of(entity, world)
                    .and_then(|d| self.sheet().path_point_at(entity, d))
                    .map(|(contact, tangent)| (world - contact).dot(tangent) < 0.0)
                    .unwrap_or(false);
                Command::AddTangentAt {
                    sheet,
                    entity,
                    at: pf_core::TangentAt::Near(world),
                    length_mm: length.unwrap_or(DEFAULT_TANGENT_LENGTH_MM),
                    backwards,
                }
            }
            _ => return,
        };
        let Some(out) = self.exec(cmd) else {
            return;
        };
        let Some(&id) = out.entities.last() else {
            return;
        };
        self.selection.clear();
        self.selection.entities.push(id);
        self.state = ToolState::Idle;
        self.reset_inputs();
        let len = measure::entity_length(self.sheet(), id)
            .map(|m| m.value)
            .unwrap_or(f64::NAN);
        self.info(tr_args(
            "status.tangent.created",
            &[("length", &format_mm(len))],
        ));
    }

    /// Courbe lisse en cours de pose : points posés, et courbe d'aperçu passant
    /// aussi par la position visée (tangentes par défaut).
    pub fn spline_draft(&self) -> Option<(Vec<PointMm>, Vec<PointMm>)> {
        if self.state != ToolState::SplineNext {
            return None;
        }
        let placed: Vec<PointMm> = self
            .spline_points
            .iter()
            .filter_map(|r| self.ref_pos(*r))
            .collect();
        let aim = self.aim();
        let mut pts = placed.clone();
        if pts
            .last()
            .is_none_or(|l| pf_geometry::distance_mm(*l, aim) > MERGE_TOLERANCE_MM)
        {
            pts.push(aim);
        }
        let curve = pf_geometry::Spline::through_points(&pts)
            .map(|s| s.sample(24))
            .unwrap_or_default();
        Some((curve, placed))
    }

    /// Entrée : crée la courbe lisse avec les points posés (au moins deux).
    fn commit_spline(&mut self) {
        if self.spline_points.len() < 2 {
            self.error(tr("status.spline.min"));
            return;
        }
        let cmd = Command::AddSpline {
            sheet: self.sheet,
            points: self.spline_points.clone(),
        };
        if let Some(out) = self.exec(cmd) {
            let id = out.entities[0];
            self.selection.clear();
            self.selection.entities.push(id);
            self.tool = Tool::Select;
            self.state = ToolState::Idle;
            self.reset_inputs();
            let sheet = self.sheet();
            let len = measure::entity_length(sheet, id)
                .map(|m| m.value)
                .unwrap_or(f64::NAN);
            let n = sheet.spline_nodes(id).map(|n| n.len()).unwrap_or(0);
            self.info(tr_args(
                "status.spline.created",
                &[("n", &n.to_string()), ("length", &format_mm(len))],
            ));
        }
    }

    fn commit_curve(&mut self, start: PointRef, end: PointRef) {
        let (Some(a), Some(b)) = (self.ref_pos(start), self.ref_pos(end)) else {
            self.error(tr("status.curve.end_missing"));
            return;
        };
        let chord = b - a;
        if chord.norm() <= EPSILON_MM {
            self.error(tr("status.curve.degenerate"));
            return;
        }
        // Forme de départ : légèrement bombée, poignées aux tiers.
        let bow = chord.rotated_left() * 0.25;
        let c1 = a + chord * (1.0 / 3.0) + bow;
        let c2 = a + chord * (2.0 / 3.0) + bow;
        let cmd = Command::AddCurve {
            sheet: self.sheet,
            start,
            c1,
            c2,
            end,
        };
        if let Some(out) = self.exec(cmd) {
            self.selection.clear();
            self.selection.entities.push(out.entities[0]);
            self.tool = Tool::Select;
            self.state = ToolState::Idle;
            self.info(tr("status.curve.created"));
        }
    }

    // ----- points relatifs (US-02.5) --------------------------------------

    /// Clic de l'outil Point en sous-mode relatif : désigne le parent suivant,
    /// puis crée le point (ou attend les valeurs de la palette).
    fn relative_click(&mut self, world: PointMm, tol_mm: f64) {
        let mode = self.point_mode;
        let kinds = mode.picks();
        let Some(&kind) = kinds.get(self.rel_picks.len()) else {
            // Division d'un cercle : un clic sur un point fixe le point de départ.
            if mode == PointMode::Division && self.rel_picks.len() == 1 {
                if let (Some(RelPick::Trace(e)), Some((p, _))) =
                    (self.rel_picks.first(), self.nearest_point(world, tol_mm))
                {
                    if matches!(
                        self.sheet().entity(*e).map(|e| &e.kind),
                        Ok(EntityKind::Circle { .. })
                    ) {
                        self.rel_picks.push(RelPick::Point(p));
                        self.info(tr("status.rel.division_start"));
                        return;
                    }
                }
            }
            // Tous les parents sont désignés : le clic valide (compas : choisit la solution).
            self.commit_relative(world);
            return;
        };
        let pick = match kind {
            PickKind::Point => self
                .nearest_point(world, tol_mm)
                .map(|(id, _)| RelPick::Point(id)),
            PickKind::Trace => self
                .nearest_entity(world, tol_mm, false)
                .map(RelPick::Trace),
            PickKind::Segment => self.nearest_entity(world, tol_mm, true).map(RelPick::Trace),
        };
        let Some(pick) = pick else {
            self.error(match kind {
                PickKind::Point => tr("status.rel.pick_point"),
                PickKind::Trace => tr("status.rel.pick_trace"),
                PickKind::Segment => tr("status.rel.pick_segment"),
            });
            return;
        };
        self.rel_picks.push(pick);
        self.state = ToolState::RelativePick;
        if self.rel_picks.len() < kinds.len() {
            self.info(tr("status.rel.next_parent"));
        } else if mode.needs_values() {
            self.info(if mode == PointMode::Compass {
                tr("status.rel.values_compass")
            } else if mode == PointMode::Division {
                tr("status.rel.values_division")
            } else {
                tr("status.rel.values")
            });
        } else {
            self.commit_relative(world);
        }
    }

    /// Définition du point relatif d'après les parents désignés et les valeurs
    /// saisies ; `near` ne sert qu'au compas (choix de la solution).
    ///
    /// Renvoie aussi les formules saisies (une expression à la place d'un nombre) : elles sont
    /// maintenues dès la création (`AddPointWithFormulas`).
    fn relative_definition(
        &self,
        near: PointMm,
    ) -> Result<(PointDefinition, Vec<pf_core::ParamExpr>), String> {
        use pf_core::ParamKind as K;
        let point = |i: usize| match self.rel_picks.get(i) {
            Some(RelPick::Point(p)) => Ok(*p),
            _ => Err(tr("status.rel.pick_point")),
        };
        let trace = |i: usize| match self.rel_picks.get(i) {
            Some(RelPick::Trace(e)) => Ok(*e),
            _ => Err(tr("status.rel.pick_trace")),
        };
        let first_param = self.typed_param(&self.length_text, false, "error.length")?;
        let second_param = if matches!(self.point_mode, PointMode::Offset | PointMode::Compass) {
            self.typed_param(&self.angle_text, false, "error.height")?
        } else {
            None
        };
        let angle_param = if matches!(
            self.point_mode,
            PointMode::Direction | PointMode::DirectionRelative
        ) {
            self.typed_param(&self.angle_text, true, "error.angle")?
        } else {
            None
        };
        let first = first_param.as_ref().map(|t| t.value);
        let second = second_param.as_ref().map(|t| t.value);
        let need = |v: Option<f64>| v.ok_or_else(|| tr("status.rel.need_value"));
        let bound = |pairs: &[(K, &Option<TypedParam>)]| -> Vec<pf_core::ParamExpr> {
            pairs
                .iter()
                .filter_map(|(param, t)| {
                    let expr = t.as_ref()?.expr.clone()?;
                    Some(pf_core::ParamExpr {
                        param: *param,
                        expr,
                    })
                })
                .collect()
        };
        let formulas = match self.point_mode {
            PointMode::AlongCurve => bound(&[(K::AlongDistance, &first_param)]),
            PointMode::Offset => bound(&[(K::Dx, &first_param), (K::Dy, &second_param)]),
            PointMode::Bisector => bound(&[(K::BisectorDistance, &first_param)]),
            PointMode::Direction | PointMode::DirectionRelative => {
                bound(&[(K::Distance, &first_param), (K::Angle, &angle_param)])
            }
            PointMode::Compass => bound(&[(K::RadiusA, &first_param), (K::RadiusB, &second_param)]),
            _ => Vec::new(),
        };
        let definition = match self.point_mode {
            PointMode::Free | PointMode::Division => return Err(tr("status.rel.pick_point")),
            PointMode::AlongCurve => PointDefinition::AlongCurve {
                curve: trace(0)?,
                distance_mm: need(first)?,
                origin: if self.rel_from_end {
                    SegmentEnd::End
                } else {
                    SegmentEnd::Start
                },
            },
            PointMode::Offset => {
                if first.is_none() && second.is_none() {
                    return Err(tr("status.rel.need_value"));
                }
                PointDefinition::Offset {
                    from: point(0)?,
                    dx_mm: first.unwrap_or(0.0),
                    dy_mm: second.unwrap_or(0.0),
                }
            }
            PointMode::Midpoint => PointDefinition::Midpoint {
                first: point(0)?,
                second: point(1)?,
            },
            PointMode::Projection => PointDefinition::Projection {
                point: point(0)?,
                trace: trace(1)?,
            },
            PointMode::Bisector => PointDefinition::Bisector {
                vertex: point(0)?,
                first: point(1)?,
                second: point(2)?,
                distance_mm: need(first)?,
            },
            PointMode::Direction | PointMode::DirectionRelative => PointDefinition::Polar {
                from: point(0)?,
                distance_mm: need(first)?,
                angle_deg: angle_param.as_ref().map_or(0.0, |t| t.value),
                reference: if self.point_mode == PointMode::DirectionRelative {
                    Some(trace(1)?)
                } else {
                    None
                },
            },
            PointMode::Compass => PointDefinition::Compass {
                center_a: point(0)?,
                center_b: point(1)?,
                radius_a_mm: need(first)?,
                radius_b_mm: need(second)?,
                near: near.into(),
            },
        };
        Ok((definition, formulas))
    }

    /// Crée le point relatif. En cas de refus (rayons qui ne se coupent pas,
    /// distance trop grande…), le message s'affiche, rien n'est modifié et la
    /// saisie reste ouverte pour être corrigée.
    fn commit_relative(&mut self, near: PointMm) {
        if self.point_mode == PointMode::Division {
            self.commit_division();
            return;
        }
        let (definition, formulas) = match self.relative_definition(near) {
            Ok(d) => d,
            Err(e) => {
                self.error(e);
                return;
            }
        };
        // Une expression saisie est maintenue dès la création (une seule étape) ; un nombre
        // reste figé.
        let cmd = if formulas.is_empty() {
            Command::AddPointAt {
                sheet: self.sheet,
                point: PointRef::Derived(definition),
            }
        } else {
            Command::AddPointWithFormulas {
                sheet: self.sheet,
                definition,
                formulas,
            }
        };
        if let Some(out) = self.exec(cmd) {
            self.selection.clear();
            self.selection.points.extend(out.points.iter().copied());
            self.state = ToolState::Idle;
            self.reset_inputs();
            match out.points.first() {
                Some(id) => {
                    let label = self.sheet().label(*id).to_string();
                    self.info(tr_args("status.rel.created", &[("label", &label)]));
                }
                None => self.info(tr("status.rel.exists")),
            }
        }
    }

    /// Division du tracé désigné d'après la saisie (`5`, `1:2:1`, `1/4;1/2;1/4`) :
    /// une seule commande, donc une seule étape d'historique.
    fn commit_division(&mut self) {
        let Some(RelPick::Trace(trace)) = self.rel_picks.first().copied() else {
            self.error(tr("status.rel.pick_trace"));
            return;
        };
        let start = match self.rel_picks.get(1) {
            Some(RelPick::Point(p)) => Some(*p),
            _ => None,
        };
        let spec = match parse_division(&self.length_text) {
            Ok(s) => s,
            Err(e) => {
                self.error(tr_args("error.division", &[("detail", &e)]));
                return;
            }
        };
        let cmd = Command::DivideTrace {
            sheet: self.sheet,
            trace,
            spec,
            start,
        };
        if let Some(out) = self.exec(cmd) {
            self.selection.clear();
            self.selection.points.extend(out.points.iter().copied());
            self.state = ToolState::Idle;
            self.reset_inputs();
            self.info(tr_args(
                "status.division.created",
                &[("n", &out.points.len().to_string())],
            ));
        }
    }

    // ----- glisser -------------------------------------------------------

    /// Début d'un glisser sur un point ou une poignée. Renvoie `false` si
    /// l'objet ne se déplace pas ainsi.
    pub fn begin_drag(&mut self, target: Target) -> bool {
        if self.size_view().is_some() {
            self.error(tr("size.readonly"));
            return false;
        }
        let drag = match target {
            Target::Point(id) => match self.sheet().point(id).map(|p| p.definition) {
                Ok(PointDefinition::Free { .. }) => Drag::Point(id),
                Ok(
                    PointDefinition::Intersection { .. }
                    | PointDefinition::CircleIntersection { .. }
                    | PointDefinition::TraceIntersection { .. },
                ) => {
                    self.error(tr("status.drag.intersection"));
                    return false;
                }
                Ok(
                    PointDefinition::AlongCurve { .. }
                    | PointDefinition::Offset { .. }
                    | PointDefinition::Midpoint { .. }
                    | PointDefinition::Projection { .. }
                    | PointDefinition::Bisector { .. }
                    | PointDefinition::Polar { .. }
                    | PointDefinition::Compass { .. }
                    | PointDefinition::Division { .. },
                ) => {
                    self.error(tr("status.drag.relative"));
                    return false;
                }
                Ok(_) => Drag::Slide(id),
                Err(_) => return false,
            },
            Target::Handle(curve, handle) => Drag::Handle(curve, handle),
            Target::Guide(id) => Drag::Guide(id),
            Target::Entity(_) | Target::Notch(_) | Target::Dimension(_) => return false,
        };
        self.doc.begin_group();
        self.drag = Some(drag);
        true
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn drag_to(&mut self, world: PointMm) {
        self.cursor = world;
        let to = self.grid(world);
        let sheet = self.sheet;
        let cmd = match self.drag {
            Some(Drag::Point(point)) => Command::MovePoint { sheet, point, to },
            Some(Drag::Slide(point)) => match self.slide_command(point, world) {
                Some(cmd) => cmd,
                None => return,
            },
            Some(Drag::Handle(curve, handle)) => Command::MoveCurveHandle {
                sheet,
                curve,
                handle,
                to,
            },
            Some(Drag::Guide(guide)) => match self.guide_drag_command(guide, to) {
                Some(cmd) => cmd,
                None => return,
            },
            None => return,
        };
        match self.doc.execute(cmd) {
            Ok(_) => self.status = Status::default(),
            Err(e) => self.error(core_error(&e)),
        }
    }

    /// Commande qui déplace une ligne d'aide parallèlement à elle-même, de
    /// façon à passer par la projection de `to` sur sa normale.
    fn guide_drag_command(&self, guide: GuideId, to: PointMm) -> Option<Command> {
        let g = self.sheet().guide(guide).ok()?;
        let n = g.direction().rotated_left();
        let at = g.origin() + n * n.dot(to - g.origin());
        Some(Command::MoveGuide {
            sheet: self.sheet,
            guide,
            x_mm: at.x,
            y_mm: at.y,
            angle_deg: g.angle_deg,
        })
    }

    /// Commande qui fait glisser un point construit le long de son tracé.
    fn slide_command(&self, point: PointId, world: PointMm) -> Option<Command> {
        let s = self.sheet();
        let sheet = self.sheet;
        let step = self.grid_snap_mm.filter(|v| *v > 0.0);
        match s.point(point).ok()?.definition {
            PointDefinition::OnSegment {
                segment, origin, ..
            } => {
                let (a, b) = s.segment_ends(segment).ok()?;
                let (a, b) = match origin {
                    SegmentEnd::Start => (a, b),
                    SegmentEnd::End => (b, a),
                };
                let along = pf_geometry::project_on_segment(world, a, b).ok()?.0;
                let len = pf_geometry::distance_mm(a, b);
                let distance_mm = match step {
                    Some(step) => ((along / step).round() * step).clamp(0.0, len),
                    None => along,
                };
                Some(Command::SetPointDistance {
                    sheet,
                    point,
                    distance_mm,
                })
            }
            PointDefinition::OnSegmentRatio { segment, .. } => {
                let (a, b) = s.segment_ends(segment).ok()?;
                let along = pf_geometry::project_on_segment(world, a, b).ok()?.0;
                let value = (along / pf_geometry::distance_mm(a, b)).clamp(0.0, 1.0);
                Some(Command::SetPointParameter {
                    sheet,
                    point,
                    value,
                })
            }
            PointDefinition::OnCurve { curve, .. } => {
                let value = s.curve_geometry(curve).ok()?.nearest_t(world).0;
                Some(Command::SetPointParameter {
                    sheet,
                    point,
                    value,
                })
            }
            _ => None,
        }
    }

    pub fn end_drag(&mut self) {
        if self.drag.take().is_some() {
            self.doc.end_group();
        }
    }

    // ----- sélection et suppression ---------------------------------------

    /// Sélectionne tout ce qui est entièrement dans le rectangle.
    pub fn select_window(&mut self, a: PointMm, b: PointMm, additive: bool) {
        let mut window = Bbox::from_point(a);
        window.include(b);
        let sheet = self.sheet();
        let mut points = Vec::new();
        let mut entities = Vec::new();
        // Filtre de sélection (US-16.1) : points et tracés seulement s'ils sont cochés.
        let filter = self.select_filter;
        for p in sheet
            .points
            .iter()
            .filter(|p| filter.points && sheet.layer_visible(p.layer))
        {
            if sheet.position(p.id).is_ok_and(|pos| window.contains(pos)) {
                points.push(p.id);
            }
        }
        for e in sheet
            .entities
            .iter()
            .filter(|e| filter.paths && sheet.layer_visible(e.layer))
        {
            let inside = match e.kind {
                EntityKind::Segment { .. } => sheet
                    .segment_ends(e.id)
                    .is_ok_and(|(p, q)| window.contains(p) && window.contains(q)),
                EntityKind::Curve { .. } => sheet
                    .curve_geometry(e.id)
                    .is_ok_and(|c| window.contains_bbox(&c.bbox())),
                EntityKind::Arc { .. } | EntityKind::Circle { .. } | EntityKind::Spline { .. } => {
                    sheet
                        .shape(e.id)
                        .is_ok_and(|s| window.contains_bbox(&s.bbox()))
                }
            };
            if inside {
                entities.push(e.id);
            }
        }
        if !additive {
            self.selection.clear();
        }
        for p in points {
            if !self.selection.points.contains(&p) {
                self.selection.points.push(p);
            }
        }
        for e in entities {
            if !self.selection.entities.contains(&e) {
                self.selection.entities.push(e);
            }
        }
        // Un membre sélectionné entraîne son groupe.
        let sheet = self.sheet();
        let all: Vec<EntityId> = sheet
            .expand_groups(&self.selection.entities)
            .into_iter()
            .filter(|m| sheet.entity_visible(*m))
            .collect();
        self.selection.entities = all;
    }

    /// Sélectionne (ou, avec Maj, bascule) tous les membres visibles du groupe de `id`.
    fn select_group_of(&mut self, id: EntityId, toggle: bool) {
        let sheet = self.sheet();
        let members: Vec<EntityId> = sheet
            .expand_groups(&[id])
            .into_iter()
            .filter(|m| sheet.entity_visible(*m))
            .collect();
        let was_selected = self.selection.entities.contains(&id);
        if !toggle {
            self.selection.clear();
        }
        for m in members {
            let present = self.selection.entities.contains(&m);
            if toggle && was_selected {
                self.selection.entities.retain(|e| *e != m);
            } else if !present {
                self.selection.entities.push(m);
            }
        }
    }

    pub fn can_group(&self) -> bool {
        self.selection.entities.len() >= 2
    }

    pub fn can_ungroup(&self) -> bool {
        let sheet = self.sheet();
        self.selection
            .entities
            .iter()
            .any(|e| sheet.group_of(*e).is_some())
    }

    /// Groupe les tracés sélectionnés (une seule étape d'historique).
    pub fn group_selection(&mut self) {
        if !self.can_group() {
            self.error(tr("status.group.need_two"));
            return;
        }
        let command = Command::CreateGroup {
            sheet: self.sheet,
            entities: self.selection.entities.clone(),
        };
        let count = self.selection.entities.len();
        if self.exec(command).is_some() {
            self.info(tr_args(
                "status.group.created",
                &[("count", &count.to_string())],
            ));
        }
    }

    /// Dissout les groupes des tracés sélectionnés ; les tracés restent sélectionnés.
    pub fn ungroup_selection(&mut self) {
        let sheet = self.sheet();
        let mut ids: Vec<GroupId> = self
            .selection
            .entities
            .iter()
            .filter_map(|e| sheet.group_of(*e).map(|g| g.id))
            .collect();
        ids.sort();
        ids.dedup();
        if ids.is_empty() {
            self.error(tr("status.group.none"));
            return;
        }
        let sheet = self.sheet;
        let mut commands: Vec<Command> = ids
            .iter()
            .map(|&group| Command::Ungroup { sheet, group })
            .collect();
        let count = commands.len();
        let command = if count == 1 {
            commands.remove(0)
        } else {
            Command::Batch { commands }
        };
        if self.exec(command).is_some() {
            self.info(tr_args(
                "status.group.dissolved",
                &[("count", &count.to_string())],
            ));
        }
    }

    /// Fragment de la sélection courante (erreur affichée si rien à copier).
    fn build_fragment(&mut self) -> Option<Fragment> {
        let sel = &self.selection;
        match self.project().copy_fragment_with_guides(
            self.sheet,
            &sel.points,
            &sel.entities,
            &sel.notches,
            &sel.dimensions,
            &sel.guides,
        ) {
            Ok(f) => Some(f),
            Err(e) => {
                self.error(core_error(&e));
                None
            }
        }
    }

    fn store_fragment(&mut self, fragment: Fragment) -> usize {
        let count = fragment.points.len() + fragment.entities.len() + fragment.guides.len();
        self.clipboard_out = Some(fragment.to_json());
        self.clipboard = Some(fragment);
        count
    }

    pub fn copy_selection(&mut self) {
        if let Some(f) = self.build_fragment() {
            let count = self.store_fragment(f);
            self.info(tr_args(
                "status.clipboard.copied",
                &[("count", &count.to_string())],
            ));
        }
    }

    /// Couper = copier puis supprimer, en une étape ; si la suppression est
    /// refusée (calque verrouillé…), le presse-papiers reste inchangé.
    pub fn cut_selection(&mut self) {
        let Some(fragment) = self.build_fragment() else {
            return;
        };
        if let Some(out) = self.delete_selection() {
            let count = self.store_fragment(fragment);
            if out.notes.is_empty() {
                self.info(tr_args(
                    "status.clipboard.cut",
                    &[("count", &count.to_string())],
                ));
            }
        }
    }

    /// Colle dans la feuille active et le calque actif, puis sélectionne les
    /// nouveaux objets. `text` : contenu du presse-papiers système (collage
    /// d'une autre instance) ; un texte qui n'est pas un fragment est ignoré
    /// sans bruit. Sans texte, c'est le presse-papiers interne.
    pub fn paste(&mut self, text: Option<&str>) {
        let fragment = match text {
            Some(t) if Fragment::looks_like(t) => match Fragment::from_json(t) {
                Ok(f) => f,
                Err(e) => {
                    self.error(core_error(&e));
                    return;
                }
            },
            Some(_) => return,
            None => match &self.clipboard {
                Some(f) => f.clone(),
                None => {
                    self.info(tr("status.clipboard.empty"));
                    return;
                }
            },
        };
        let command = Command::PasteFragment {
            sheet: self.sheet,
            fragment: Box::new(fragment),
            layer: self.current_layer(),
        };
        if let Some(out) = self.exec(command) {
            self.selection = Selection {
                points: out.points.clone(),
                entities: out.entities.clone(),
                notches: out.notches.clone(),
                dimensions: out.dimensions.clone(),
                guides: out.guides.clone(),
            };
            if out.notes.is_empty() {
                let count = out.points.len() + out.entities.len() + out.guides.len();
                self.info(tr_args(
                    "status.clipboard.pasted",
                    &[("count", &count.to_string())],
                ));
            }
        }
    }

    pub fn delete_selection(&mut self) -> Option<Outcome> {
        if self.delete_dart() {
            return None;
        }
        if self.selection.is_empty() {
            return None;
        }
        // Les crans d'abord : supprimer leur tracé les supprimerait déjà. La commande est celle
        // dont `Project::deletion_impact` annonce les conséquences (US-03.6).
        let objects = crate::dependencies::selection_objects(&self.selection);
        let cmd = Project::deletion_command(self.sheet, &objects)?;
        let count = self.selection.len();
        if let Some(out) = self.exec(cmd) {
            self.selection.clear();
            if out.notes.is_empty() {
                self.info(tr_args("status.deleted", &[("count", &count.to_string())]));
            }
            return Some(out);
        }
        None
    }

    // ----- feuilles ------------------------------------------------------

    pub fn add_sheet(&mut self) {
        let ruled = self.rule_name(pf_core::NamingTarget::Sheet, self.sheet_use_rule);
        let names: Vec<&str> = self
            .project()
            .sheets
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        let name = (self.project().sheets.len() + 1..)
            .map(|n| tr_args("sheet.default_name", &[("n", &n.to_string())]))
            .find(|n| !names.contains(&n.as_str()))
            .unwrap_or_default();
        let name = ruled.unwrap_or(name);
        if let Some(out) = self.exec(Command::CreateSheet { name }) {
            if let Some(id) = out.sheet {
                self.set_sheet(id);
            }
        }
    }

    /// Duplique la feuille (une seule étape d'historique) et l'affiche.
    pub fn duplicate_sheet(&mut self, sheet: SheetId) {
        if let Some(out) = self.exec(Command::DuplicateSheet { sheet }) {
            if let Some(id) = out.sheet {
                self.set_sheet(id);
                let name = self.sheet().name.clone();
                self.info(tr_args("status.sheet_duplicated", &[("name", &name)]));
            }
        }
    }

    /// Importe une feuille d'un autre projet (commande déjà construite).
    pub fn import_sheet(&mut self, command: Command) {
        if let Some(out) = self.exec(command) {
            if let Some(id) = out.sheet {
                self.set_sheet(id);
                let name = self.sheet().name.clone();
                self.info(tr_args("status.sheet_imported", &[("name", &name)]));
            }
        }
    }

    pub fn delete_sheet(&mut self, sheet: SheetId) {
        let index = self
            .project()
            .sheets
            .iter()
            .position(|s| s.id == sheet)
            .unwrap_or(0);
        if self.exec(Command::DeleteSheet { sheet }).is_some() {
            let sheets = &self.project().sheets;
            self.sheet = sheets[index.min(sheets.len() - 1)].id;
            self.sync();
        }
    }
}

/// Dimension signée d'un rectangle : une valeur saisie négative est respectée,
/// une valeur saisie positive prend le sens de la souris (droite/haut par défaut).
fn signed_size(typed: Option<f64>, mouse: f64) -> f64 {
    match typed {
        Some(v) if v < 0.0 => v,
        Some(v) if mouse < 0.0 => -v,
        Some(v) => v,
        None => mouse,
    }
}

pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pf_core::measure::{angle_between_segments, entity_length, point_distance};

    const TOL: f64 = 2.0;

    fn pt(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    fn editor() -> Editor {
        Editor::new(Document::new(Project::new("Test")))
    }

    #[test]
    fn cercle_centre_rayon_saisi_et_annulable() {
        let mut ed = editor();
        ed.set_tool(Tool::Circle);
        ed.click(pt(10.0, 10.0), TOL, false);
        assert!(matches!(ed.state, ToolState::CircleCenter { .. }));
        ed.length_text = "25".into();
        ed.confirm();
        assert_eq!(ed.sheet().entities.len(), 1, "{}", ed.status.text);
        let id = ed.sheet().entities[0].id;
        let c = ed.sheet().circle_geometry(id).unwrap();
        assert_eq!(
            (c.circle.center, c.circle.radius_mm),
            (pt(10.0, 10.0), 25.0)
        );
        assert_eq!(ed.state, ToolState::Idle);
        ed.undo();
        assert!(ed.sheet().entities.is_empty());
    }

    #[test]
    fn cercle_rayon_invalide_refuse_sans_trace() {
        let mut ed = editor();
        ed.set_tool(Tool::Circle);
        ed.click(pt(0.0, 0.0), TOL, false);
        let before = ed.project().clone();
        for bad in ["0", "-5", "abc"] {
            ed.length_text = bad.into();
            ed.confirm();
            assert!(ed.status.error, "{bad}");
            assert_eq!(*ed.project(), before, "{bad}");
        }
    }

    #[test]
    fn arc_centre_et_angles_saisis_sens_anti_horaire_par_defaut() {
        let mut ed = editor();
        ed.set_tool(Tool::Arc);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.length_text = "10".into();
        ed.angle_text = "0".into();
        ed.confirm();
        assert!(matches!(ed.state, ToolState::ArcSweep { .. }));
        ed.angle_text = "90".into();
        ed.confirm();
        assert_eq!(ed.sheet().entities.len(), 1, "{}", ed.status.text);
        let id = ed.sheet().entities[0].id;
        let arc = ed.sheet().arc_geometry(id).unwrap();
        assert!(arc.is_ccw());
        assert!((arc.length_mm() - 5.0 * std::f64::consts::PI).abs() < 1e-9);
        // Sens inversé : le grand arc de 270°.
        ed.set_tool(Tool::Arc);
        ed.click(pt(100.0, 0.0), TOL, false);
        ed.length_text = "10".into();
        ed.angle_text = "0".into();
        ed.confirm();
        ed.perp_flip = true;
        ed.angle_text = "90".into();
        ed.confirm();
        let id = ed.sheet().entities[1].id;
        assert!(!ed.sheet().arc_geometry(id).unwrap().is_ccw());
        assert!(
            (entity_length(ed.sheet(), id).unwrap().value - 15.0 * std::f64::consts::PI).abs()
                < 1e-9
        );
    }

    #[test]
    fn cercle_et_arc_par_trois_points_et_deux_points_rayon() {
        let mut ed = editor();
        ed.set_tool(Tool::Circle);
        ed.set_round_mode(RoundMode::ThreePoints);
        for p in [pt(10.0, 0.0), pt(0.0, 10.0), pt(-10.0, 0.0)] {
            ed.click(p, TOL, false);
        }
        assert_eq!(ed.sheet().entities.len(), 1, "{}", ed.status.text);
        // Trois points alignés : refusé, rien n'est créé.
        let before = ed.project().clone();
        for p in [pt(100.0, 0.0), pt(110.0, 0.0), pt(120.0, 0.0)] {
            ed.click(p, TOL, false);
        }
        assert!(ed.status.error);
        assert_eq!(*ed.project(), before);
        ed.escape();
        // Cercle 2 points + rayon, centre du côté de la souris.
        ed.set_tool(Tool::Circle);
        ed.set_round_mode(RoundMode::TwoPointsRadius);
        ed.click(pt(-30.0, 60.0), TOL, false);
        ed.click(pt(-24.0, 60.0), TOL, false);
        ed.pointer_moved(pt(-27.0, 70.0));
        ed.length_text = "5".into();
        ed.confirm();
        let id = ed.sheet().entities.last().unwrap().id;
        let g = ed.sheet().circle_geometry(id).unwrap();
        assert!(pf_geometry::distance_mm(g.circle.center, pt(-27.0, 64.0)) < 1e-9);
        // Arc par trois points.
        ed.set_tool(Tool::Arc);
        ed.set_round_mode(RoundMode::ThreePoints);
        for p in [pt(60.0, 0.0), pt(50.0, 10.0), pt(40.0, 0.0)] {
            ed.click(p, TOL, false);
        }
        let id = ed.sheet().entities.last().unwrap().id;
        assert!(
            (ed.sheet().arc_geometry(id).unwrap().length_mm() - 10.0 * std::f64::consts::PI).abs()
                < 1e-9
        );
    }

    #[test]
    fn accrochage_intersection_cercle_droite_et_cercle_cercle() {
        let mut ed = editor();
        ed.snap_to_paths = true;
        ed.grid_snap_mm = None;
        let id = |ed: &Editor, i: usize| ed.sheet().entities[i].id;
        ed.exec(Command::AddCircle {
            sheet: ed.sheet,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 5.0,
        })
        .unwrap();
        ed.exec(Command::AddSegment {
            sheet: ed.sheet,
            from: PointRef::New(pt(-10.0, 3.0)),
            to: PointRef::New(pt(10.0, 3.0)),
        })
        .unwrap();
        // Deux solutions (±4, 3) : la plus proche du clic est retenue.
        let s = ed.snap_at(pt(3.9, 3.1), TOL);
        assert_eq!(s.kind, SnapKind::Intersection);
        assert!(pf_geometry::distance_mm(s.pos, pt(4.0, 3.0)) < 1e-9);
        let s2 = ed.snap_at(pt(-4.1, 2.9), TOL);
        assert!(pf_geometry::distance_mm(s2.pos, pt(-4.0, 3.0)) < 1e-9);
        // Le point créé par cet accrochage suit les tracés.
        ed.set_tool(Tool::Point);
        ed.click(pt(3.9, 3.1), TOL, false);
        let p = *ed.selection.points.first().unwrap();
        assert!(matches!(
            ed.sheet().point(p).unwrap().definition,
            PointDefinition::CircleIntersection { .. }
        ));
        // Cercle / cercle.
        ed.exec(Command::AddCircle {
            sheet: ed.sheet,
            center: PointRef::New(pt(6.0, 0.0)),
            radius_mm: 5.0,
        })
        .unwrap();
        let (c1, c2) = (id(&ed, 0), id(&ed, 2));
        assert_eq!(ed.sheet().circle_intersections(c1, c2).unwrap().len(), 2);
        let s = ed.snap_at(pt(3.0, -4.1), TOL);
        assert_eq!(s.kind, SnapKind::Intersection);
        assert!(pf_geometry::distance_mm(s.pos, pt(3.0, -4.0)) < 1e-9);
    }

    #[test]
    fn accrochage_intersection_courbes_splines_arcs_et_cercles() {
        let mut ed = editor();
        ed.snap_to_paths = true;
        ed.grid_snap_mm = None;
        let sheet = ed.sheet;
        // Arche (sommet 50 ; 75), courbe lisse en travers, cercle et arc.
        ed.exec(Command::AddCurve {
            sheet,
            start: PointRef::New(pt(0.0, 0.0)),
            c1: pt(0.0, 100.0),
            c2: pt(100.0, 100.0),
            end: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap();
        ed.exec(Command::AddSpline {
            sheet,
            points: vec![
                PointRef::New(pt(-20.0, 40.0)),
                PointRef::New(pt(50.0, 50.0)),
                PointRef::New(pt(120.0, 40.0)),
            ],
        })
        .unwrap();
        ed.exec(Command::AddCircle {
            sheet,
            center: PointRef::New(pt(50.0, 0.0)),
            radius_mm: 60.0,
        })
        .unwrap();
        ed.exec(Command::AddArc {
            sheet,
            center: PointRef::New(pt(300.0, 300.0)),
            radius_mm: 100.0,
            start_deg: 180.0,
            end_deg: 270.0,
            ccw: true,
        })
        .unwrap();
        let ids: Vec<EntityId> = ed.sheet().entities.iter().map(|e| e.id).collect();
        let (curve, spline, ring) = (ids[0], ids[1], ids[2]);
        let mut seen = 0;
        for (a, b) in [(curve, spline), (curve, ring), (spline, ring)] {
            for h in ed.sheet().trace_intersections(a, b).unwrap() {
                let s = ed.snap_at(h.point, TOL);
                assert_eq!(s.kind, SnapKind::Intersection, "{a:?} / {b:?}");
                assert!(pf_geometry::distance_mm(s.pos, h.point) < 1e-9);
                assert!(matches!(
                    s.target,
                    PointRef::Derived(PointDefinition::TraceIntersection { .. })
                ));
                seen += 1;
            }
        }
        assert!(seen >= 4, "{seen}");
        // Spline / segment : l'intersection est proposée, le segment n'est pas prolongé.
        ed.exec(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(25.0, -50.0)),
            to: PointRef::New(pt(25.0, 100.0)),
        })
        .unwrap();
        let line = ed.sheet().entities.last().unwrap().id;
        let hits = ed.sheet().trace_intersections(spline, line).unwrap();
        assert_eq!(hits.len(), 1);
        let s = ed.snap_at(hits[0].point, TOL);
        assert!(matches!(
            s.target,
            PointRef::Derived(PointDefinition::TraceIntersection { .. })
        ));
    }

    #[test]
    fn selection_a_la_souris_et_glisser_du_centre() {
        let mut ed = editor();
        ed.exec(Command::AddArc {
            sheet: ed.sheet,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 10.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap();
        let id = ed.sheet().entities[0].id;
        // Clic sur l'arc (à 45°), hors de tout point.
        let on_arc = pt(10.0 * 0.5_f64.sqrt(), 10.0 * 0.5_f64.sqrt());
        ed.click(on_arc, TOL, false);
        assert_eq!(ed.selection.entities, vec![id]);
        // Fenêtre englobante.
        ed.selection.clear();
        ed.select_window(pt(-1.0, -1.0), pt(11.0, 11.0), false);
        assert_eq!(ed.selection.entities, vec![id]);
        // Glisser le centre : l'arc suit (translation).
        let center = match ed.sheet().entity(id).unwrap().kind {
            EntityKind::Arc { center, .. } => center,
            _ => unreachable!(),
        };
        assert!(ed.begin_drag(Target::Point(center)));
        ed.grid_snap_mm = None;
        ed.drag_to(pt(5.0, 5.0));
        ed.end_drag();
        let g = ed.sheet().arc_geometry(id).unwrap();
        assert!(pf_geometry::distance_mm(g.center, pt(5.0, 5.0)) < 1e-9);
        assert!((g.radius_mm - 10.0).abs() < 1e-9);
        ed.undo();
        let g = ed.sheet().arc_geometry(id).unwrap();
        assert!(pf_geometry::distance_mm(g.center, pt(0.0, 0.0)) < 1e-9);
    }

    #[test]
    fn courbe_lisse_pose_refus_et_apercu() {
        let mut ed = editor();
        ed.grid_snap_mm = None;
        ed.set_tool(Tool::Spline);
        ed.click(pt(0.0, 0.0), TOL, false);
        // Un seul point : Entrée est refusée, rien n'est créé.
        ed.confirm();
        assert!(ed.status.error && ed.sheet().entities.is_empty());
        // Le même point deux fois est refusé sans être ajouté.
        ed.click(pt(0.0, 0.0), TOL, false);
        assert!(ed.status.error);
        assert_eq!(ed.spline_points.len(), 1);
        ed.click(pt(40.0, 30.0), TOL, false);
        ed.pointer_moved(pt(80.0, 0.0));
        let (curve, placed) = ed.spline_draft().unwrap();
        assert_eq!(placed.len(), 2);
        assert!(curve.len() > 3, "l'aperçu passe aussi par la souris");
        ed.confirm();
        let s = ed.sheet();
        assert_eq!(s.entities.len(), 1);
        assert_eq!(s.spline_nodes(s.entities[0].id).unwrap().len(), 2);
        assert!(ed.spline_points.is_empty());
        ed.undo();
        assert!(ed.sheet().entities.is_empty());
    }

    #[test]
    fn courbe_lisse_selection_a_la_souris_fenetre_et_glisser_d_un_point() {
        let mut ed = editor();
        ed.grid_snap_mm = None;
        let o = ed
            .exec(Command::AddSpline {
                sheet: ed.sheet,
                points: [(0.0, 0.0), (40.0, 30.0), (90.0, -10.0)]
                    .iter()
                    .map(|&(x, y)| PointRef::New(pt(x, y)))
                    .collect(),
            })
            .unwrap();
        let id = o.entities[0];
        // Clic sur la courbe, entre deux points de passage.
        let on = ed.sheet().spline_geometry(id).unwrap().segments()[0].point_at(0.5);
        ed.click(on, TOL, false);
        assert_eq!(ed.selection.entities, vec![id]);
        ed.selection.clear();
        ed.select_window(pt(-50.0, -50.0), pt(150.0, 80.0), false);
        assert_eq!(ed.selection.entities, vec![id]);
        // Glisser le point de passage du milieu : la courbe suit.
        let mid = o.points[1];
        assert!(ed.begin_drag(Target::Point(mid)));
        ed.drag_to(pt(40.0, 55.0));
        ed.end_drag();
        let n = ed.sheet().spline_nodes(id).unwrap();
        assert!(pf_geometry::distance_mm(n[1].position, pt(40.0, 55.0)) < 1e-9);
        ed.undo();
        let n = ed.sheet().spline_nodes(id).unwrap();
        assert!(pf_geometry::distance_mm(n[1].position, pt(40.0, 30.0)) < 1e-9);
    }

    #[test]
    fn cran_sur_un_arc_a_distance_saisie() {
        let mut ed = editor();
        ed.exec(Command::AddArc {
            sheet: ed.sheet,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 10.0,
            start_deg: 0.0,
            end_deg: 180.0,
            ccw: true,
        })
        .unwrap();
        ed.set_tool(Tool::Notch);
        ed.click(pt(0.0, 10.0), TOL, false);
        assert!(matches!(ed.state, ToolState::NotchOn { .. }));
        ed.length_text = "10".into();
        ed.confirm();
        assert_eq!(ed.sheet().notches.len(), 1, "{}", ed.status.text);
        assert_eq!(ed.sheet().notches[0].distance_mm, 10.0);
    }

    #[test]
    fn couper_coller_une_etape_et_texte_systeme_ignore() {
        let mut ed = editor();
        let (ab, bc) = draw_ab_bc(&mut ed);
        ed.selection.entities = vec![ab, bc];
        ed.cut_selection();
        assert!(ed.sheet().entities.is_empty(), "couper supprime");
        let json = ed
            .clipboard_out
            .take()
            .expect("JSON pour le presse-papiers système");
        ed.paste(None);
        assert_eq!(ed.sheet().entities.len(), 2);
        assert_eq!(ed.selection.entities.len(), 2);
        // Coller depuis le texte système (autre instance) : fragment reconnu ;
        // texte quelconque : ignoré sans erreur.
        ed.paste(Some("du texte"));
        assert!(!ed.status.error);
        assert_eq!(ed.sheet().entities.len(), 2);
        ed.paste(Some(&json));
        assert_eq!(ed.sheet().entities.len(), 4);
        // Un fragment corrompu est refusé.
        ed.paste(Some(&json.replace("\"version\":1", "\"version\":9")));
        assert!(ed.status.error);
        assert_eq!(ed.sheet().entities.len(), 4);
        // Annuler : coller, coller, couper = trois étapes distinctes.
        ed.undo();
        assert_eq!(ed.sheet().entities.len(), 2);
    }

    #[test]
    fn couper_refuse_sur_calque_verrouille_laisse_le_presse_papiers() {
        let mut ed = editor();
        let (ab, _) = draw_ab_bc(&mut ed);
        let sheet = ed.sheet;
        let layer = ed
            .exec(Command::AddLayer {
                sheet,
                name: "L".into(),
            })
            .unwrap()
            .layers[0];
        ed.selection.entities = vec![ab];
        ed.selection.points = ed
            .sheet()
            .entity_points(ab)
            .map(|(a, b)| vec![a, b])
            .unwrap();
        ed.active_layer = Some(layer);
        ed.move_selection_to_active_layer();
        ed.exec(Command::SetLayerLocked {
            sheet,
            layer,
            locked: true,
        })
        .unwrap();
        ed.selection.entities = vec![ab];
        ed.cut_selection();
        assert!(ed.status.error, "couper un objet verrouillé est refusé");
        assert!(ed.clipboard.is_none());
        assert!(ed.sheet().entity(ab).is_ok());
        // Coller dans un calque actif verrouillé est refusé aussi.
        ed.copy_selection();
        assert!(ed.clipboard.is_some());
        let n = ed.sheet().entities.len();
        ed.paste(None);
        assert!(ed.status.error);
        assert_eq!(ed.sheet().entities.len(), n);
    }

    /// Trace AB = 120 mm à 0° par saisie, puis BC ⟂ 80 mm : le scénario de la
    /// démonstration, rejoué exactement comme le fait l'interface.
    fn draw_ab_bc(ed: &mut Editor) -> (EntityId, EntityId) {
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.2, -0.3), TOL, false); // aimanté sur (0, 0)
        ed.pointer_moved(pt(57.3, 21.9)); // la souris est ailleurs : la saisie prime
        ed.length_text = "120".into();
        ed.angle_text = "0".into();
        ed.confirm();
        let ab = ed.selection.entities[0];

        ed.set_tool(Tool::Perpendicular);
        ed.click(pt(60.0, 0.4), TOL, false); // désigne AB
        ed.click(pt(119.6, 0.5), TOL, false); // accroche B
        ed.pointer_moved(pt(131.0, 33.0)); // côté gauche de A→B
        ed.length_text = "80".into();
        ed.confirm();
        let bc = ed.selection.entities[0];
        (ab, bc)
    }

    #[test]
    fn at01_at06_scenario_de_demonstration() {
        let mut ed = editor();
        let (ab, bc) = draw_ab_bc(&mut ed);
        assert!(!ed.status.error, "{}", ed.status.text);
        let s = ed.sheet();
        assert_eq!(entity_length(s, ab).unwrap().value, 120.0);
        assert_eq!(entity_length(s, bc).unwrap().value, 80.0);
        assert_eq!(angle_between_segments(s, ab, bc).unwrap().value, 90.0);
        assert_eq!(s.entity_name(bc), "Segment BC");
        assert_eq!(s.points.len(), 3, "B est partagé, pas dupliqué");
        assert_eq!(ed.state, ToolState::Idle);
        assert!(ed.length_text.is_empty());
    }

    #[test]
    fn at08_resultat_identique_quel_que_soit_le_zoom() {
        // La tolérance de visée varie avec le zoom ; la géométrie saisie non.
        let mut results = Vec::new();
        for tol in [8.0, 2.0, 0.5] {
            let mut ed = editor();
            ed.set_tool(Tool::Segment);
            ed.click(pt(0.0, 0.0), tol, false);
            ed.length_text = "120".into();
            ed.angle_text = "0".into();
            ed.confirm();
            let ab = ed.selection.entities[0];
            ed.set_tool(Tool::Perpendicular);
            ed.click(pt(60.0, 0.1), tol, false);
            ed.click(pt(120.0, 0.1), tol, false);
            ed.pointer_moved(pt(125.0, 30.0));
            ed.length_text = "80".into();
            ed.confirm();
            let bc = ed.selection.entities[0];
            let s = ed.sheet();
            results.push((
                entity_length(s, ab).unwrap().value,
                entity_length(s, bc).unwrap().value,
                angle_between_segments(s, ab, bc).unwrap().value,
            ));
        }
        assert!(
            results.iter().all(|r| *r == (120.0, 80.0, 90.0)),
            "{results:?}"
        );
    }

    #[test]
    fn at02_at03_point_a_distance_par_saisie() {
        let mut ed = editor();
        let (ab, _) = draw_ab_bc(&mut ed);
        ed.set_tool(Tool::PointOnSegment);
        ed.click(pt(80.0, 0.3), TOL, false);
        ed.length_text = "42".into();
        ed.confirm();
        let p = ed.selection.points[0];
        let s = ed.sheet();
        let a = s.segment_point_ids(ab).unwrap().0;
        assert_eq!(point_distance(s, a, p).unwrap().value, 42.0);
        assert_eq!(s.position(p).unwrap(), pt(42.0, 0.0));

        // 130 mm sur 120 : refus visible, rien n'est créé, la saisie reste ouverte.
        let before = ed.project().clone();
        ed.set_tool(Tool::PointOnSegment);
        ed.click(pt(80.0, 0.3), TOL, false);
        ed.length_text = "130".into();
        ed.confirm();
        assert!(
            ed.status.error && ed.status.text.contains("130"),
            "{}",
            ed.status.text
        );
        assert_eq!(ed.project(), &before);
        assert!(matches!(ed.state, ToolState::PointOn { .. }));

        // Depuis l'autre extrémité.
        ed.flip_origin();
        ed.length_text = "20".into();
        ed.confirm();
        assert_eq!(
            ed.sheet().position(ed.selection.points[0]).unwrap(),
            pt(100.0, 0.0)
        );
    }

    #[test]
    fn us031_formule_a_la_creation_point_a_distance() {
        let mut ed = editor();
        let (ab, _) = draw_ab_bc(&mut ed);
        ed.exec(Command::AddVariable {
            name: "L".into(),
            unit: pf_core::VariableUnit::Mm,
            expr: "50".into(),
        })
        .unwrap();
        ed.set_tool(Tool::PointOnSegment);
        ed.click(pt(80.0, 0.3), TOL, false);
        // Un nombre reste figé : aucun aperçu de formule, aucune formule.
        ed.length_text = "42".into();
        assert!(ed.typed_creation_formulas().is_empty());
        // Une expression est annoncée (valeur calculée à la main : 50 / 2 + 10 = 35).
        ed.length_text = "L/2+10".into();
        assert_eq!(
            ed.typed_creation_formulas(),
            vec![("L/2+10".to_string(), 35.0, false)]
        );
        // Refus : 50 x 10 = 500 > 120 mm ; syntaxe ; variable inconnue. Rien n'est créé.
        let before = ed.project().clone();
        for bad in ["L*10", "L/", "zz+1"] {
            ed.length_text = bad.into();
            ed.confirm();
            assert!(ed.status.error, "{bad}");
            assert_eq!(ed.project(), &before, "{bad}");
            assert!(matches!(ed.state, ToolState::PointOn { .. }), "{bad}");
        }
        ed.length_text = "L/2+10".into();
        ed.confirm();
        assert!(!ed.status.error, "{}", ed.status.text);
        let p = ed.selection.points[0];
        assert_eq!(ed.sheet().position(p).unwrap(), pt(35.0, 0.0));
        let f = ed
            .sheet()
            .formula(p, pf_core::ParamKind::SegmentDistance)
            .expect("formule maintenue dès la création");
        assert_eq!(f.expr, "L/2+10");
        let a = ed.sheet().segment_point_ids(ab).unwrap().0;
        assert_eq!(point_distance(ed.sheet(), a, p).unwrap().value, 35.0);
        // Maintenu : L = 80 donne 80 / 2 + 10 = 50.
        ed.exec(Command::SetVariableExpr {
            name: "L".into(),
            expr: "80".into(),
        })
        .unwrap();
        assert_eq!(ed.sheet().position(p).unwrap(), pt(50.0, 0.0));
        // Une seule étape pour la création : annuler deux fois (variable, point) retire le point.
        ed.undo();
        ed.undo();
        assert!(ed.sheet().point(p).is_err());
        assert!(ed.sheet().formulas.is_empty());
    }

    #[test]
    fn at13_echap_ne_laisse_aucune_entite_incomplete() {
        let mut ed = editor();
        let before = ed.project().clone();
        ed.set_tool(Tool::Segment);
        ed.click(pt(10.0, 10.0), TOL, false);
        ed.length_text = "75".into();
        assert!(ed.preview().unwrap().is_some());
        ed.escape();
        assert_eq!(ed.state, ToolState::Idle);
        assert_eq!(
            ed.project(),
            &before,
            "aucun point ni segment ne doit subsister"
        );
        assert!(!ed.doc.is_dirty() && !ed.doc.can_undo());
        assert!(ed.length_text.is_empty());
        // Deuxième Échap : retour à la sélection.
        ed.escape();
        assert_eq!(ed.tool, Tool::Select);

        // Même chose au milieu d'une perpendiculaire.
        let mut ed = editor();
        draw_ab_bc(&mut ed);
        let before = ed.project().clone();
        ed.set_tool(Tool::Perpendicular);
        ed.click(pt(60.0, 0.0), TOL, false);
        ed.click(pt(30.0, 40.0), TOL, false);
        ed.escape();
        assert_eq!(ed.project(), &before);
    }

    #[test]
    fn saisie_invalide_signalee_sans_creation() {
        let mut ed = editor();
        let before = ed.project().clone();
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        for bad in ["abc", "-5", "0", "12,5,3"] {
            ed.length_text = bad.into();
            ed.angle_text = "0".into();
            ed.confirm();
            assert_eq!(ed.project(), &before, "« {bad} » ne doit rien créer");
        }
        assert!(ed.status.error);
        // Unités et virgule française acceptées.
        ed.length_text = "12,5 cm".into();
        ed.confirm();
        let id = ed.selection.entities[0];
        assert_eq!(entity_length(ed.sheet(), id).unwrap().value, 125.0);
    }

    #[test]
    fn trace_a_la_souris_et_longueur_seule_imposee() {
        let mut ed = editor();
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.click(pt(30.2, 39.8), TOL, false); // aimant 1 mm → (30, 40)
        let id = ed.selection.entities[0];
        assert_eq!(entity_length(ed.sheet(), id).unwrap().value, 50.0);

        // Longueur imposée, direction donnée par la souris.
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.length_text = "100".into();
        ed.click(pt(0.0, 37.0), TOL, false);
        let id = ed.selection.entities[0];
        let (_, b) = ed.sheet().segment_ends(id).unwrap();
        assert_eq!(b, pt(0.0, 100.0));
    }

    #[test]
    fn at14_courbe_poignee_et_longueur_recalculee() {
        let mut ed = editor();
        ed.set_tool(Tool::Curve);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.click(pt(100.0, 0.0), TOL, false);
        assert_eq!(ed.tool, Tool::Select);
        let curve = ed.selection.entities[0];
        let before = entity_length(ed.sheet(), curve).unwrap().value;
        assert!(before > 100.0);

        let c1 = ed.sheet().curve_geometry(curve).unwrap().c1;
        let Some(Target::Handle(id, handle)) = ed.hit_test(c1, TOL) else {
            panic!("la poignée de la courbe sélectionnée doit être saisissable");
        };
        assert!(ed.begin_drag(Target::Handle(id, handle)));
        for step in 1..=10 {
            ed.drag_to(pt(c1.x, c1.y + 6.0 * f64::from(step)));
        }
        ed.end_drag();
        let after = entity_length(ed.sheet(), curve).unwrap().value;
        assert!(after > before + 5.0, "{before} → {after}");
        assert_eq!(
            measure::curve_chord(ed.sheet(), curve).unwrap().value,
            100.0
        );
        // Tout le glisser = une seule étape d'annulation.
        ed.undo();
        assert_eq!(entity_length(ed.sheet(), curve).unwrap().value, before);
    }

    #[test]
    fn at12_selection_simple_multiple_fenetre_et_suppression() {
        let mut ed = editor();
        let (ab, bc) = draw_ab_bc(&mut ed);
        ed.set_tool(Tool::Select);
        ed.click(pt(60.0, 0.5), TOL, false);
        assert_eq!(ed.selection.entities, vec![ab]);
        ed.click(pt(120.4, 40.0), TOL, true);
        assert_eq!(ed.selection.entities, vec![ab, bc]);
        ed.click(pt(60.0, 0.5), TOL, true); // Maj+clic retire
        assert_eq!(ed.selection.entities, vec![bc]);
        ed.click(pt(300.0, 300.0), TOL, true); // Maj+clic dans le vide : inchangé
        assert_eq!(ed.selection.entities, vec![bc]);
        ed.click(pt(300.0, 300.0), TOL, false);
        assert!(ed.selection.is_empty());

        // Fenêtre : seuls les éléments entièrement inclus.
        ed.select_window(pt(110.0, -10.0), pt(130.0, 90.0), false);
        assert_eq!(ed.selection.entities, vec![bc]);
        assert_eq!(ed.selection.points.len(), 2);

        // Suppression de BC seul : AB intact.
        ed.selection.clear();
        ed.selection.entities.push(bc);
        ed.delete_selection();
        assert!(!ed.status.error, "{}", ed.status.text);
        assert!(ed.sheet().entity(ab).is_ok() && ed.sheet().entity(bc).is_err());
        assert_eq!(entity_length(ed.sheet(), ab).unwrap().value, 120.0);
        ed.undo();
        assert!(ed.sheet().entity(bc).is_ok());
    }

    #[test]
    fn point_construit_glisse_le_long_de_son_segment() {
        let mut ed = editor();
        let (ab, _) = draw_ab_bc(&mut ed);
        ed.set_tool(Tool::PointOnSegment);
        ed.click(pt(80.0, 0.0), TOL, false);
        ed.length_text = "42".into();
        ed.confirm();
        let p = ed.selection.points[0];
        let Some(Target::Point(hit)) = ed.hit_test(pt(42.0, 0.0), TOL) else {
            panic!("le point doit être saisissable");
        };
        assert!(ed.begin_drag(Target::Point(hit)));
        ed.drag_to(pt(70.3, 25.0)); // loin du segment : seule l'abscisse compte
        ed.drag_to(pt(500.0, -3.0)); // au-delà de B : bloqué à l'extrémité
        ed.end_drag();
        assert_eq!(ed.sheet().position(p).unwrap(), pt(120.0, 0.0));
        ed.undo();
        assert_eq!(ed.sheet().position(p).unwrap(), pt(42.0, 0.0));
        assert!(ed.sheet().entity(ab).is_ok());
    }

    /// AB (0,0)→(120,0) et un segment vertical qui le croise en (40,0).
    fn crossed(ed: &mut Editor) -> (EntityId, EntityId) {
        ed.snap_to_paths = false; // tracé brut, sans accrochage, pour préparer la scène
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.click(pt(120.0, 0.0), TOL, false);
        let ab = ed.selection.entities[0];
        ed.set_tool(Tool::Segment);
        ed.click(pt(40.0, -30.0), TOL, false);
        ed.click(pt(40.0, 50.0), TOL, false);
        let cd = ed.selection.entities[0];
        ed.snap_to_paths = true;
        (ab, cd)
    }

    #[test]
    fn accrochage_priorites_deterministes() {
        let mut ed = editor();
        let (ab, cd) = crossed(&mut ed);
        // Extrémité : prioritaire sur tout le reste.
        let s = ed.snap_at(pt(0.6, 0.5), TOL);
        assert_eq!((s.kind, s.pos), (SnapKind::Point, pt(0.0, 0.0)));
        // Intersection : avant « sur le tracé ».
        let s = ed.snap_at(pt(40.8, 0.7), TOL);
        assert_eq!((s.kind, s.pos), (SnapKind::Intersection, pt(40.0, 0.0)));
        assert_eq!(
            s.target,
            PointRef::Derived(PointDefinition::Intersection {
                first: ab.min(cd),
                second: ab.max(cd)
            })
        );
        // Milieu de AB.
        let s = ed.snap_at(pt(60.5, -0.9), TOL);
        assert_eq!((s.kind, s.pos), (SnapKind::Midpoint, pt(60.0, 0.0)));
        // Point quelconque du segment : projeté, abscisse arrondie au mm.
        let s = ed.snap_at(pt(87.4, 1.2), TOL);
        assert_eq!((s.kind, s.pos), (SnapKind::OnPath, pt(87.0, 0.0)));
        // Rien à portée : position libre aimantée.
        let s = ed.snap_at(pt(87.4, 20.3), TOL);
        assert_eq!((s.kind, s.pos), (SnapKind::Free, pt(87.0, 20.0)));
        // Accrochage aux tracés désactivé : les points existants accrochent encore.
        ed.snap_to_paths = false;
        assert_eq!(ed.snap_at(pt(60.5, -0.9), TOL).kind, SnapKind::Free);
        assert_eq!(ed.snap_at(pt(0.6, 0.5), TOL).kind, SnapKind::Point);
    }

    #[test]
    fn accrochage_identique_a_tout_zoom() {
        // Le rayon de visée change avec le zoom ; la position accrochée, jamais.
        let mut ed = editor();
        crossed(&mut ed);
        for tol in [0.4, 2.0, 9.0] {
            let d = tol * 0.6;
            assert_eq!(
                ed.snap_at(pt(40.0 + d * 0.5, d * 0.5), tol).pos,
                pt(40.0, 0.0)
            );
            assert_eq!(ed.snap_at(pt(60.0, d), tol).pos, pt(60.0, 0.0));
        }
    }

    #[test]
    fn outil_point_cree_des_points_lies() {
        let mut ed = editor();
        let (ab, _) = crossed(&mut ed);
        ed.set_tool(Tool::Point);
        ed.click(pt(40.5, 0.4), TOL, false); // intersection
        let x = ed.selection.points[0];
        ed.click(pt(60.2, 0.3), TOL, false); // milieu
        let m = ed.selection.points[0];
        ed.click(pt(200.0, 90.0), TOL, false); // libre
        let free = ed.selection.points[0];
        let count = ed.sheet().points.len();
        ed.click(pt(40.2, -0.3), TOL, false); // de nouveau l'intersection : pas de doublon
        assert_eq!(ed.sheet().points.len(), count);
        assert_eq!(ed.selection.points, vec![x]);
        assert!(ed.sheet().point(free).unwrap().definition.is_free());

        // Déplacer B : le milieu et l'intersection suivent.
        let b = ed.sheet().segment_point_ids(ab).unwrap().1;
        ed.exec(Command::MovePoint {
            sheet: ed.sheet,
            point: b,
            to: pt(120.0, 60.0),
        })
        .unwrap();
        assert_eq!(ed.sheet().position(m).unwrap(), pt(60.0, 30.0));
        assert_eq!(ed.sheet().position(x).unwrap(), pt(40.0, 20.0));
        // L'intersection ne se glisse pas ; le milieu glisse (et devient une autre proportion).
        assert!(!ed.begin_drag(Target::Point(x)));
        assert!(ed.begin_drag(Target::Point(m)));
        ed.drag_to(pt(30.0, 15.0));
        ed.end_drag();
        let slid = ed.sheet().position(m).unwrap();
        assert!(
            pf_geometry::distance_mm(slid, pt(30.0, 15.0)) < 1e-9,
            "{slid:?}"
        );
    }

    #[test]
    fn segment_trace_depuis_un_milieu_reste_attache() {
        let mut ed = editor();
        let (ab, _) = crossed(&mut ed);
        ed.set_tool(Tool::Segment);
        ed.click(pt(60.3, 0.2), TOL, false); // départ accroché au milieu de AB
        ed.length_text = "50".into();
        ed.angle_text = "90".into();
        ed.confirm();
        let seg = ed.selection.entities[0];
        let (from, _) = ed.sheet().segment_point_ids(seg).unwrap();
        assert_eq!(
            ed.sheet().point(from).unwrap().definition,
            PointDefinition::OnSegmentRatio {
                segment: ab,
                ratio: 0.5
            }
        );
        assert_eq!(entity_length(ed.sheet(), seg).unwrap().value, 50.0);
        // Courbe accrochée sur une courbe existante.
        ed.set_tool(Tool::Curve);
        ed.click(pt(0.0, 100.0), TOL, false);
        ed.click(pt(100.0, 100.0), TOL, false);
        let curve = ed.selection.entities[0];
        let on = ed.sheet().curve_geometry(curve).unwrap().point_at(0.3);
        let s = ed.snap_at(pt(on.x + 0.3, on.y - 0.4), TOL);
        assert_eq!(s.kind, SnapKind::OnPath);
        assert!(matches!(
            s.target,
            PointRef::Derived(PointDefinition::OnCurve { .. })
        ));
        assert!(pf_geometry::distance_mm(s.pos, on) < 0.6);
    }

    #[test]
    fn mesure_ne_modifie_pas_le_projet() {
        let mut ed = editor();
        draw_ab_bc(&mut ed);
        let before = ed.project().clone();
        ed.set_tool(Tool::Measure);
        ed.click(pt(0.3, 0.2), TOL, false);
        ed.click(pt(120.2, 79.7), TOL, false);
        let (a, b) = ed.last_measure.unwrap();
        assert!((pf_geometry::distance_mm(a, b) - 144.222_051_018_559_57).abs() < 1e-9);
        assert_eq!(ed.project(), &before);
        assert!(!ed.doc.is_dirty() || ed.doc.can_undo());
    }

    #[test]
    fn feuilles_ajout_changement_suppression() {
        let mut ed = editor();
        draw_ab_bc(&mut ed);
        let first = ed.sheet;
        ed.add_sheet();
        ed.add_sheet();
        assert_eq!(ed.project().sheets.len(), 3);
        assert_ne!(ed.sheet, first);
        assert!(
            ed.sheet().entities.is_empty(),
            "chaque feuille est indépendante"
        );
        ed.set_sheet(first);
        assert_eq!(ed.sheet().entities.len(), 2);
        let last = ed.project().sheets[2].id;
        ed.delete_sheet(last);
        assert_eq!(ed.project().sheets.len(), 2);
        assert_eq!(ed.sheet, ed.project().sheets[1].id);
    }

    #[test]
    fn rectangle_par_saisie_et_a_la_souris() {
        let mut ed = editor();
        ed.set_tool(Tool::Rectangle);
        ed.click(pt(10.0, 20.0), TOL, false);
        ed.length_text = "120".into();
        ed.angle_text = "80".into();
        ed.confirm();
        assert!(!ed.status.error, "{}", ed.status.text);
        assert_eq!(ed.selection.entities.len(), 4);
        let s = ed.sheet();
        let m =
            pf_core::contour::measure_contour(s, &pf_core::contour::closed_contours(s)[0]).unwrap();
        assert_eq!((m.perimeter_mm, m.area_mm2), (400.0, 9600.0));
        assert_eq!(ed.state, ToolState::Idle);

        // À la souris, vers la gauche et le bas : les valeurs saisies suivent le sens du curseur.
        ed.set_tool(Tool::Rectangle);
        ed.click(pt(300.0, 300.0), TOL, false);
        ed.pointer_moved(pt(250.0, 270.0));
        ed.length_text = "40".into();
        ed.confirm();
        let s = ed.sheet();
        assert!(s
            .points
            .iter()
            .any(|p| s.position(p.id).unwrap() == pt(260.0, 300.0)));
        assert!(s
            .points
            .iter()
            .any(|p| s.position(p.id).unwrap() == pt(260.0, 270.0)));

        // Largeur nulle ou saisie invalide : rien n'est créé.
        let before = ed.project().clone();
        ed.set_tool(Tool::Rectangle);
        ed.click(pt(500.0, 500.0), TOL, false);
        ed.length_text = "0".into();
        ed.angle_text = "10".into();
        ed.confirm();
        ed.length_text = "abc".into();
        ed.confirm();
        assert_eq!(ed.project(), &before);
        ed.escape();
        assert_eq!(ed.project(), &before);
    }

    #[test]
    fn parallele_par_saisie_et_cote_de_la_souris() {
        let mut ed = editor();
        let (ab, _) = draw_ab_bc(&mut ed);
        ed.set_tool(Tool::Parallel);
        ed.click(pt(60.0, 0.4), TOL, false);
        assert!(matches!(ed.state, ToolState::ParallelOf { .. }));
        ed.pointer_moved(pt(60.0, -30.0)); // sous AB : côté droit
        ed.length_text = "25".into();
        ed.confirm();
        assert!(!ed.status.error, "{}", ed.status.text);
        let id = ed.selection.entities[0];
        let (a, b) = ed.sheet().segment_ends(id).unwrap();
        assert_eq!((a, b), (pt(0.0, -25.0), pt(120.0, -25.0)));
        assert!(matches!(
            ed.sheet().entity(id).unwrap().kind,
            EntityKind::Segment {
                construction: Some(pf_core::Construction::Parallel { .. }),
                ..
            }
        ));
        // Côté inversé.
        ed.set_tool(Tool::Parallel);
        ed.click(pt(60.0, 0.0), TOL, false);
        ed.pointer_moved(pt(60.0, -30.0));
        ed.perp_flip = true;
        ed.length_text = "10".into();
        ed.confirm();
        let (a, _) = ed.sheet().segment_ends(ed.selection.entities[0]).unwrap();
        assert_eq!(a, pt(0.0, 10.0));
        assert!(ed.sheet().entity(ab).is_ok());
    }

    #[test]
    fn cran_par_saisie_cote_et_double() {
        let mut ed = editor();
        let (ab, _) = draw_ab_bc(&mut ed);
        ed.set_tool(Tool::Notch);
        ed.click(pt(80.0, 0.3), TOL, false);
        assert!(matches!(ed.state, ToolState::NotchOn { .. }));
        ed.pointer_moved(pt(80.0, -20.0)); // sous AB (dirigé vers +x) : côté droit
        ed.length_text = "42".into();
        ed.confirm();
        assert!(!ed.status.error, "{}", ed.status.text);
        let id = ed.selection.notches[0];
        let n = ed.sheet().notch(id).unwrap().clone();
        assert_eq!((n.entity, n.distance_mm, n.length_mm), (ab, 42.0, 5.0));
        assert_eq!(n.side, Side::Right);
        let marks = ed.sheet().notch_marks(&n).unwrap();
        assert!(pf_geometry::distance_mm(marks[0].1, pt(42.0, -5.0)) < 1e-9);

        // Côté inversé, longueur saisie, cran double.
        ed.set_tool(Tool::Notch);
        ed.click(pt(80.0, 0.3), TOL, false);
        ed.pointer_moved(pt(80.0, -20.0));
        ed.perp_flip = true;
        ed.notch_double = true;
        ed.length_text = "100".into();
        ed.angle_text = "8".into();
        ed.confirm();
        let n = ed.sheet().notch(ed.selection.notches[0]).unwrap().clone();
        assert_eq!(
            (n.side, n.length_mm, n.style),
            (Side::Left, 8.0, NotchStyle::Double)
        );

        // Position hors du tracé ou longueur invalide : rien n'est créé.
        let before = ed.project().clone();
        ed.set_tool(Tool::Notch);
        ed.click(pt(80.0, 0.3), TOL, false);
        ed.length_text = "130".into();
        ed.confirm();
        assert!(ed.status.error, "{}", ed.status.text);
        ed.length_text = "50".into();
        ed.angle_text = "0".into();
        ed.confirm();
        assert!(ed.status.error);
        assert_eq!(ed.project(), &before);
        ed.escape();
        assert_eq!(ed.project(), &before);
    }

    #[test]
    fn cran_sur_courbe_a_la_longueur_developpee() {
        let mut ed = editor();
        ed.set_tool(Tool::Curve);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.click(pt(100.0, 0.0), TOL, false);
        let curve = ed.selection.entities[0];
        let total = ed.sheet().path_length_mm(curve).unwrap();
        ed.set_tool(Tool::Notch);
        let (mid, _) = ed.sheet().path_point_at(curve, total / 2.0).unwrap();
        ed.click(mid, TOL, false);
        assert!(
            matches!(ed.state, ToolState::NotchOn { .. }),
            "{}",
            ed.status.text
        );
        ed.length_text = format!("{}", total / 4.0);
        ed.confirm();
        assert!(!ed.status.error, "{}", ed.status.text);
        let n = ed.sheet().notch(ed.selection.notches[0]).unwrap().clone();
        assert!((n.distance_mm - total / 4.0).abs() < 1e-6);
    }

    #[test]
    fn selection_et_suppression_d_un_cran() {
        let mut ed = editor();
        let (ab, _) = draw_ab_bc(&mut ed);
        ed.exec(Command::AddNotch {
            sheet: ed.sheet,
            entity: ab,
            distance_mm: 30.0,
            side: Side::Left,
            length_mm: 6.0,
            style: NotchStyle::Single,
        })
        .unwrap();
        let notch = ed.sheet().notches[0].id;
        ed.set_tool(Tool::Select);
        // Clic sur la pointe du cran (30 ; 6) : le cran est choisi, pas le segment.
        ed.click(pt(30.0, 5.0), TOL, false);
        assert_eq!(ed.selection.notches, vec![notch]);
        assert!(ed.selection.entities.is_empty());
        ed.delete_selection();
        assert!(ed.sheet().notches.is_empty(), "{}", ed.status.text);
        assert!(
            ed.sheet().entity(ab).is_ok(),
            "le tracé porteur est conservé"
        );
        ed.undo();
        assert_eq!(ed.sheet().notches.len(), 1);

        // Sélection mêlant le cran et son tracé : un seul lot, sans erreur.
        ed.selection.entities.push(ab);
        ed.selection.notches.push(notch);
        ed.delete_selection();
        assert!(!ed.status.error, "{}", ed.status.text);
        assert!(ed.sheet().notches.is_empty() && ed.sheet().entity(ab).is_err());
        ed.undo();
        assert!(ed.sheet().entity(ab).is_ok() && ed.sheet().notches.len() == 1);
    }

    #[test]
    fn droit_fil_en_une_etape_d_historique() {
        let mut ed = editor();
        ed.set_tool(Tool::Grainline);
        ed.click(pt(40.0, 20.0), TOL, false);
        ed.length_text = "80".into();
        ed.angle_text = "90".into();
        ed.confirm();
        assert!(!ed.status.error, "{}", ed.status.text);
        let id = ed.selection.entities[0];
        assert_eq!(ed.sheet().entity(id).unwrap().role, PathRole::Grainline);
        assert_eq!(ed.sheet().grainline_count(), 1);
        let (a, b) = ed.sheet().segment_ends(id).unwrap();
        assert_eq!((a, b), (pt(40.0, 20.0), pt(40.0, 100.0)));
        // Une seule annulation retire le segment et son rôle.
        ed.undo();
        assert!(ed.sheet().entities.is_empty());
        assert!(!ed.doc.can_undo());
        // À la souris aussi.
        ed.set_tool(Tool::Grainline);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.click(pt(0.0, 60.0), TOL, false);
        assert_eq!(ed.sheet().grainline_count(), 1);
    }
    fn add_layer(ed: &mut Editor, name: &str) -> LayerId {
        let sheet = ed.sheet;
        ed.exec(Command::AddLayer {
            sheet,
            name: name.into(),
        })
        .expect("calque créé")
        .layers[0]
    }

    fn segment_mouse(ed: &mut Editor, a: PointMm, b: PointMm) -> EntityId {
        ed.set_tool(Tool::Segment);
        ed.click(a, TOL, false);
        ed.click(b, TOL, false);
        ed.set_tool(Tool::Select);
        *ed.sheet().entities.last().map(|e| &e.id).expect("tracé")
    }

    #[test]
    fn calques_les_nouveaux_objets_vont_dans_le_calque_actif_et_pas_ailleurs() {
        let mut ed = editor();
        let first = segment_mouse(&mut ed, pt(0.0, 0.0), pt(50.0, 0.0));
        assert_eq!(ed.sheet().entity(first).unwrap().layer, None);
        let a = add_layer(&mut ed, "A");
        ed.active_layer = Some(a);
        // Milieu accroché (point dérivé) + nouveau tracé : tout dans A.
        let second = segment_mouse(&mut ed, pt(0.0, 30.0), pt(50.0, 30.0));
        let s = ed.sheet();
        assert_eq!(s.entity(second).unwrap().layer, Some(a));
        let (p, q) = s.entity_points(second).unwrap();
        assert_eq!(s.point(p).unwrap().layer, Some(a));
        assert_eq!(s.point(q).unwrap().layer, Some(a));
        assert_eq!(
            s.entity(first).unwrap().layer,
            None,
            "l'ancien ne bouge pas"
        );
        // Calque actif disparu : retour au calque par défaut, sans erreur.
        ed.active_layer = Some(LayerId(123_456));
        let third = segment_mouse(&mut ed, pt(0.0, 60.0), pt(50.0, 60.0));
        assert_eq!(ed.sheet().entity(third).unwrap().layer, None);
        assert!(!ed.status.error, "{}", ed.status.text);
    }

    #[test]
    fn calque_masque_ni_accrochable_ni_selectionnable_ni_dans_la_fenetre() {
        let mut ed = editor();
        let a = add_layer(&mut ed, "A");
        ed.active_layer = Some(a);
        let e = segment_mouse(&mut ed, pt(0.0, 0.0), pt(50.0, 0.0));
        let (p, _) = ed.sheet().entity_points(e).unwrap();
        // Visible : sélectionnable et accrochable.
        assert_eq!(ed.hit_test(pt(25.0, 0.0), TOL), Some(Target::Entity(e)));
        assert_eq!(ed.snap_at(pt(0.1, 0.1), TOL).kind, SnapKind::Point);
        ed.selection.set(Target::Entity(e));
        let sheet = ed.sheet;
        ed.exec(Command::SetLayerVisible {
            sheet,
            layer: a,
            visible: false,
        })
        .unwrap();
        ed.prune_selection();
        assert!(ed.selection.is_empty(), "la sélection oublie le masqué");
        assert_eq!(ed.hit_test(pt(25.0, 0.0), TOL), None);
        assert_eq!(ed.hit_test(pt(0.0, 0.0), TOL), None);
        assert_ne!(ed.snap_at(pt(0.1, 0.1), TOL).kind, SnapKind::Point);
        assert_eq!(ed.snap_at(pt(25.0, 0.1), TOL).kind, SnapKind::Free);
        ed.select_window(pt(-10.0, -10.0), pt(60.0, 10.0), false);
        assert!(ed.selection.is_empty());
        let _ = p;
    }

    #[test]
    fn calque_verrouille_accrochable_mais_suppression_et_deplacement_refuses() {
        let mut ed = editor();
        let a = add_layer(&mut ed, "A");
        ed.active_layer = Some(a);
        let e = segment_mouse(&mut ed, pt(0.0, 0.0), pt(50.0, 0.0));
        let sheet = ed.sheet;
        ed.exec(Command::SetLayerLocked {
            sheet,
            layer: a,
            locked: true,
        })
        .unwrap();
        assert_eq!(ed.snap_at(pt(0.1, 0.1), TOL).kind, SnapKind::Point);
        assert_eq!(ed.hit_test(pt(25.0, 0.0), TOL), Some(Target::Entity(e)));
        // Suppression refusée, avec message, sans effet.
        ed.selection.set(Target::Entity(e));
        let before = ed.project().clone();
        ed.delete_selection();
        assert!(ed.status.error, "{}", ed.status.text);
        assert!(ed.status.text.contains("verrouillé"), "{}", ed.status.text);
        assert_eq!(*ed.project(), before);
        // Dessiner dans le calque actif verrouillé est refusé aussi.
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 40.0), TOL, false);
        ed.click(pt(50.0, 40.0), TOL, false);
        assert!(ed.status.error, "{}", ed.status.text);
        assert_eq!(ed.sheet().entities.len(), 1);
    }

    #[test]
    fn calques_deplacer_la_selection_vers_le_calque_actif() {
        let mut ed = editor();
        let e = segment_mouse(&mut ed, pt(0.0, 0.0), pt(50.0, 0.0));
        let b = add_layer(&mut ed, "B");
        ed.selection.set(Target::Entity(e));
        ed.active_layer = Some(b);
        ed.move_selection_to_active_layer();
        assert_eq!(ed.sheet().entity(e).unwrap().layer, Some(b));
        ed.undo();
        assert_eq!(ed.sheet().entity(e).unwrap().layer, None);
    }

    // ----- US-01.3 : noms et groupes ---------------------------------------

    fn two_segments(ed: &mut Editor) -> (EntityId, EntityId) {
        let a = segment_mouse(ed, pt(0.0, 0.0), pt(50.0, 0.0));
        let b = segment_mouse(ed, pt(0.0, 30.0), pt(50.0, 30.0));
        (a, b)
    }

    #[test]
    fn groupes_ca1_nom_unique_par_feuille_refuse_en_doublon() {
        let mut ed = editor();
        let (a, b) = two_segments(&mut ed);
        let sheet = ed.sheet;
        let set = |ed: &mut Editor, entity, name: &str| {
            ed.exec(Command::SetEntityName {
                sheet,
                entity,
                name: Some(name.into()),
            })
        };
        assert!(set(&mut ed, a, "Ourlet").is_some());
        assert_eq!(
            ed.sheet().entity(a).unwrap().name.as_deref(),
            Some("Ourlet")
        );
        assert!(set(&mut ed, b, "Ourlet").is_none(), "doublon refusé");
        assert!(ed.status.error, "{}", ed.status.text);
        assert_eq!(ed.sheet().entity(b).unwrap().name, None);
    }

    #[test]
    fn groupes_ca2_clic_membre_selectionne_le_groupe_alt_clic_le_membre_seul() {
        let mut ed = editor();
        let (a, b) = two_segments(&mut ed);
        let c = segment_mouse(&mut ed, pt(0.0, 60.0), pt(50.0, 60.0));
        ed.selection.entities = vec![a, b];
        ed.group_selection();
        assert!(!ed.status.error, "{}", ed.status.text);
        assert_eq!(ed.sheet().groups.len(), 1);

        ed.selection.clear();
        ed.click(pt(25.0, 0.0), TOL, false);
        assert_eq!(
            ed.selection.entities,
            vec![a, b],
            "un membre = tout le groupe"
        );
        ed.click_with(pt(25.0, 30.0), TOL, false, true);
        assert_eq!(ed.selection.entities, vec![b], "Alt+clic : le membre seul");
        // Maj+clic bascule le groupe entier.
        ed.selection.clear();
        ed.click(pt(25.0, 60.0), TOL, false);
        ed.click(pt(25.0, 0.0), TOL, true);
        assert_eq!(ed.selection.entities, vec![c, a, b]);
        ed.click(pt(25.0, 30.0), TOL, true);
        assert_eq!(ed.selection.entities, vec![c]);
        // Fenêtre : un membre entraîne son groupe.
        ed.select_window(pt(-5.0, -5.0), pt(55.0, 5.0), false);
        assert_eq!(ed.selection.entities, vec![a, b]);

        // Dégrouper : plus de sélection groupée.
        ed.ungroup_selection();
        assert!(ed.sheet().groups.is_empty());
        ed.selection.clear();
        ed.click(pt(25.0, 0.0), TOL, false);
        assert_eq!(ed.selection.entities, vec![a]);
        // Rien à dégrouper, ou trop peu de tracés : erreur affichée.
        ed.ungroup_selection();
        assert!(ed.status.error);
        ed.group_selection();
        assert!(ed.status.error);
    }

    #[test]
    fn groupes_ca3_transformation_du_groupe_en_une_etape_annulable() {
        let mut ed = editor();
        let (a, b) = two_segments(&mut ed);
        let c = segment_mouse(&mut ed, pt(0.0, 60.0), pt(50.0, 60.0));
        ed.selection.entities = vec![a, b];
        ed.group_selection();
        ed.selection.clear();
        ed.click(pt(25.0, 0.0), TOL, false);
        let before = ed.project().clone();

        let sheet = ed.sheet;
        let mut points = Vec::new();
        for &e in &ed.selection.entities {
            let (p, q) = ed.sheet().entity_points(e).unwrap();
            points.extend([p, q]);
        }
        let cmd = Command::Transform {
            sheet,
            points,
            transform: pf_geometry::Transform::Translate {
                dx_mm: 10.0,
                dy_mm: 5.0,
            },
        };
        assert!(ed.exec(cmd).is_some());
        let s = ed.sheet();
        assert_eq!(s.segment_ends(a).unwrap().0, pt(10.0, 5.0));
        assert_eq!(s.segment_ends(b).unwrap().0, pt(10.0, 35.0));
        assert_eq!(s.segment_ends(c).unwrap().0, pt(0.0, 60.0));
        ed.undo();
        assert_eq!(
            *ed.project(),
            before,
            "une seule annulation pour tout le groupe"
        );
    }

    // ----- US-02.8 : lignes d'aide ---------------------------------------

    fn ed_with_segment() -> Editor {
        let mut ed = editor();
        ed.snap_to_paths = false;
        ed.grid_snap_mm = None;
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.click(pt(100.0, 0.0), TOL, false);
        ed.snap_to_paths = true;
        ed.set_tool(Tool::Guide);
        ed
    }

    #[test]
    fn guide_horizontal_vertical_clic_ou_valeur_saisie() {
        let mut ed = ed_with_segment();
        ed.grid_snap_mm = Some(1.0);
        // Clic : horizontale à la hauteur du clic.
        ed.click(pt(30.2, 20.3), TOL, false);
        let g = ed.sheet().guides[0].clone();
        assert_eq!((g.y_mm, g.angle_deg), (20.0, 0.0));
        assert_eq!(ed.selection.guides, vec![g.id]);
        // Valeur saisie (Entrée, sans clic) : verticale à X = 42,5.
        ed.set_guide_mode(GuideMode::Vertical);
        ed.length_text = "42,5".into();
        ed.confirm();
        let v = ed.sheet().guides[1].clone();
        assert_eq!((v.x_mm, v.angle_deg), (42.5, 90.0), "{}", ed.status.text);
        // Sans valeur, Entrée est refusé et ne crée rien.
        ed.confirm();
        assert!(ed.status.error);
        assert_eq!(ed.sheet().guides.len(), 2);
        // Une étape d'historique par guide.
        ed.undo();
        ed.undo();
        assert!(ed.sheet().guides.is_empty());
    }

    #[test]
    fn guide_oblique_par_angle_saisi_ou_par_deux_points() {
        let mut ed = ed_with_segment();
        ed.set_guide_mode(GuideMode::Oblique);
        ed.grid_snap_mm = Some(1.0);
        // Point de passage cliqué, angle saisi.
        ed.click(pt(10.0, 10.0), TOL, false);
        assert!(matches!(ed.state, ToolState::GuideFrom { .. }));
        ed.angle_text = "45".into();
        ed.confirm();
        let a = ed.sheet().guides[0].clone();
        assert_eq!((a.x_mm, a.y_mm, a.angle_deg), (10.0, 10.0, 45.0));
        // Deux clics : direction du premier vers le second (ramenée dans [0 ; 180[).
        ed.click(pt(0.0, 50.0), TOL, false);
        ed.click(pt(30.0, 20.0), TOL, false);
        let b = ed.sheet().guides[1].clone();
        assert!((b.angle_deg - 135.0).abs() < 1e-9, "{}", b.angle_deg);
        // Position saisie du point de passage (X puis Y), puis angle.
        ed.length_text = "5".into();
        ed.angle_text = "7".into();
        ed.confirm();
        assert!(matches!(
            ed.state,
            ToolState::GuideFrom { through } if through == pt(5.0, 7.0)
        ));
        ed.angle_text = "30".into();
        ed.confirm();
        assert_eq!(ed.sheet().guides.len(), 3);
        // Deux clics sur le même point : refus, rien de créé.
        ed.click(pt(60.0, 60.0), TOL, false);
        ed.click(pt(60.0, 60.0), TOL, false);
        assert!(ed.status.error);
        assert_eq!(ed.sheet().guides.len(), 3);
        // Angle illisible : refus sans trace.
        ed.angle_text = "abc".into();
        let before = ed.project().clone();
        ed.confirm();
        assert!(ed.status.error);
        assert_eq!(*ed.project(), before);
    }

    #[test]
    fn guide_accrochage_sur_le_guide_et_aux_intersections() {
        let mut ed = ed_with_segment();
        ed.grid_snap_mm = None;
        ed.click(pt(10.0, 25.0), TOL, false); // horizontale y = 25
        ed.set_guide_mode(GuideMode::Vertical);
        ed.click(pt(40.0, 60.0), TOL, false); // verticale x = 40
        ed.set_tool(Tool::Select);
        // Sur le guide : projection exacte du point visé.
        let s = ed.snap_at(pt(70.0, 25.4), TOL);
        assert_eq!(s.kind, SnapKind::OnGuide);
        assert_eq!(s.pos, pt(70.0, 25.0));
        // Guide / guide.
        let s = ed.snap_at(pt(40.3, 24.8), TOL);
        assert_eq!(s.kind, SnapKind::Intersection);
        assert_eq!(s.pos, pt(40.0, 25.0));
        // Guide / tracé : la verticale coupe le segment en (40 ; 0).
        let s = ed.snap_at(pt(40.4, 0.3), TOL);
        assert_eq!(s.kind, SnapKind::Intersection);
        assert!(pf_geometry::distance_mm(s.pos, pt(40.0, 0.0)) < 1e-6);
        // Un point posé là existe réellement (libre, à la position exacte).
        ed.set_tool(Tool::Point);
        ed.click(pt(40.4, 0.3), TOL, false);
        let id = ed.selection.points[0];
        assert!(pf_geometry::distance_mm(ed.sheet().position(id).unwrap(), pt(40.0, 0.0)) < 1e-6);
        // Hors de portée : position libre.
        assert_eq!(ed.snap_at(pt(70.0, 40.0), TOL).kind, SnapKind::Free);
        // Accrochage aux tracés désactivé : les guides n'accrochent pas non plus.
        ed.snap_to_paths = false;
        assert_eq!(ed.snap_at(pt(70.0, 25.4), TOL).kind, SnapKind::Free);
    }

    #[test]
    fn guide_masque_globalement_ou_par_calque_n_accroche_ni_ne_se_selectionne() {
        let mut ed = ed_with_segment();
        ed.grid_snap_mm = None;
        ed.click(pt(10.0, 25.0), TOL, false);
        ed.set_tool(Tool::Select);
        assert_eq!(ed.snap_at(pt(70.0, 25.2), TOL).kind, SnapKind::OnGuide);
        assert!(matches!(
            ed.hit_test(pt(70.0, 25.2), TOL),
            Some(Target::Guide(_))
        ));
        // Masquage global (Affichage > Lignes d'aide).
        ed.show_guides = false;
        assert_eq!(ed.snap_at(pt(70.0, 25.2), TOL).kind, SnapKind::Free);
        assert_eq!(ed.hit_test(pt(70.0, 25.2), TOL), None);
        ed.prune_selection();
        assert!(ed.selection.guides.is_empty());
        ed.show_guides = true;
        // Masquage du calque du guide.
        let sheet = ed.sheet;
        let layer = ed
            .exec(Command::AddLayer {
                sheet,
                name: "Aides".into(),
            })
            .unwrap()
            .layers[0];
        ed.active_layer = Some(layer);
        ed.set_tool(Tool::Guide);
        ed.set_guide_mode(GuideMode::Vertical);
        ed.click(pt(55.0, 60.0), TOL, false);
        let g = ed.sheet().guides[1].clone();
        assert_eq!(g.layer, Some(layer), "le guide suit le calque actif");
        ed.set_tool(Tool::Select);
        assert_eq!(ed.snap_at(pt(55.2, 70.0), TOL).kind, SnapKind::OnGuide);
        ed.exec(Command::SetLayerVisible {
            sheet,
            layer,
            visible: false,
        })
        .unwrap();
        assert_eq!(ed.snap_at(pt(55.2, 70.0), TOL).kind, SnapKind::Free);
        assert_eq!(ed.hit_test(pt(55.2, 70.0), TOL), None);
        ed.prune_selection();
        assert!(ed.selection.guides.is_empty());
    }

    #[test]
    fn guide_non_pris_par_la_fenetre_de_selection_mais_selectionnable_au_clic() {
        let mut ed = ed_with_segment();
        ed.grid_snap_mm = None;
        ed.click(pt(10.0, 25.0), TOL, false);
        ed.set_tool(Tool::Select);
        ed.selection.clear();
        ed.select_window(pt(-50.0, -50.0), pt(200.0, 200.0), false);
        assert!(ed.selection.guides.is_empty(), "jamais par la fenêtre");
        assert_eq!(ed.selection.entities.len(), 1);
        ed.selection.clear();
        ed.click(pt(70.0, 25.3), TOL, false);
        assert_eq!(ed.selection.guides.len(), 1);
        // Un tracé prime sur un guide au même endroit.
        ed.selection.clear();
        ed.set_tool(Tool::Guide);
        ed.set_guide_mode(GuideMode::Horizontal);
        ed.length_text = "0".into();
        ed.confirm();
        ed.set_tool(Tool::Select);
        ed.selection.clear();
        ed.click(pt(50.0, 0.1), TOL, false);
        assert_eq!(ed.selection.entities.len(), 1);
        assert!(ed.selection.guides.is_empty());
    }

    #[test]
    fn guide_deplacer_au_glisser_supprimer_annuler_et_verrou() {
        let mut ed = ed_with_segment();
        ed.grid_snap_mm = Some(1.0);
        ed.click(pt(10.0, 25.0), TOL, false);
        let id = ed.sheet().guides[0].id;
        ed.set_tool(Tool::Select);
        // Glisser : une seule étape d'historique, parallèlement à lui-même.
        assert!(ed.begin_drag(Target::Guide(id)));
        ed.drag_to(pt(10.0, 31.4));
        ed.drag_to(pt(10.0, 40.2));
        ed.end_drag();
        let g = ed.sheet().guide(id).unwrap().clone();
        assert_eq!((g.y_mm, g.angle_deg), (40.0, 0.0));
        ed.undo();
        assert_eq!(ed.sheet().guide(id).unwrap().y_mm, 25.0);
        // Suppression de la sélection, annulable.
        ed.selection.guides = vec![id];
        ed.delete_selection().unwrap();
        assert!(ed.sheet().guides.is_empty());
        ed.undo();
        assert_eq!(ed.sheet().guides.len(), 1);
        // Calque verrouillé : déplacer et supprimer sont refusés sans trace.
        let sheet = ed.sheet;
        let layer = ed
            .exec(Command::AddLayer {
                sheet,
                name: "Gelé".into(),
            })
            .unwrap()
            .layers[0];
        ed.active_layer = Some(layer);
        ed.set_tool(Tool::Guide);
        ed.set_guide_mode(GuideMode::Vertical);
        ed.click(pt(80.0, 10.0), TOL, false);
        let locked = ed.sheet().guides[1].id;
        ed.exec(Command::SetLayerLocked {
            sheet,
            layer,
            locked: true,
        })
        .unwrap();
        ed.set_tool(Tool::Select);
        let before = ed.project().clone();
        assert!(ed.begin_drag(Target::Guide(locked)));
        ed.drag_to(pt(90.0, 10.0));
        ed.end_drag();
        assert!(ed.status.error, "{}", ed.status.text);
        assert_eq!(*ed.project(), before);
        ed.selection.guides = vec![locked];
        assert!(ed.delete_selection().is_none());
        assert_eq!(*ed.project(), before);
    }

    #[test]
    fn guide_copier_coller_et_symetrie_par_axe() {
        let mut ed = ed_with_segment();
        ed.grid_snap_mm = None;
        ed.click(pt(10.0, 25.0), TOL, false);
        let id = ed.sheet().guides[0].id;
        ed.set_tool(Tool::Select);
        ed.selection.guides = vec![id];
        ed.copy_selection();
        assert!(!ed.status.error, "{}", ed.status.text);
        ed.paste(None);
        assert_eq!(ed.sheet().guides.len(), 2);
        assert_ne!(ed.sheet().guides[1].id, id, "identifiants réécrits");
        assert_eq!(ed.selection.guides, vec![ed.sheet().guides[1].id]);
        // Couper = copier puis supprimer.
        ed.cut_selection();
        assert_eq!(ed.sheet().guides.len(), 1);
        // Axe : guide vertical x = 50 marqué « axe » ; symétrie du segment (0→100).
        ed.set_tool(Tool::Guide);
        ed.set_guide_mode(GuideMode::Vertical);
        ed.length_text = "150".into();
        ed.confirm();
        let axis = ed.sheet().guides[1].id;
        let sheet = ed.sheet;
        ed.exec(Command::SetGuideAxis {
            sheet,
            guide: axis,
            axis: true,
        })
        .unwrap();
        let g = ed.sheet().guide(axis).unwrap().clone();
        let seg = ed.sheet().entities[0].id;
        let out = ed
            .exec(Command::Duplicate {
                sheet,
                entities: vec![seg],
                transform: pf_geometry::Transform::Mirror {
                    axis_point: g.origin(),
                    axis_angle_deg: g.angle_deg,
                },
            })
            .unwrap();
        let (a, b) = ed.sheet().segment_ends(out.entities[0]).unwrap();
        let (lo, hi) = (a.x.min(b.x), a.x.max(b.x));
        assert_eq!((lo, hi, a.y, b.y), (200.0, 300.0, 0.0, 0.0));
    }

    // ----- US-16.2 : saisie directe -----------------------------------------

    /// Projet sérialisé tel quel (comparaison octet pour octet).
    fn bytes(ed: &Editor) -> String {
        serde_json::to_string(ed.project()).unwrap()
    }

    /// Nombre d'étapes d'annulation disponibles (annule tout puis rétablit tout).
    fn undo_depth(ed: &mut Editor) -> usize {
        let mut n = 0;
        while ed.doc.undo() {
            n += 1;
        }
        for _ in 0..n {
            assert!(ed.doc.redo());
        }
        n
    }

    fn free_editor() -> Editor {
        let mut ed = editor();
        ed.grid_snap_mm = None;
        ed.snap_to_paths = false;
        ed
    }

    fn type_all(ed: &mut Editor, text: &str) {
        text.chars().for_each(|c| ed.type_entry(c));
    }

    fn last_point(ed: &Editor) -> PointMm {
        let id = ed.sheet().points.last().unwrap().id;
        ed.sheet().position(id).unwrap()
    }

    fn assert_close(p: PointMm, x: f64, y: f64) {
        assert!(
            (p.x - x).abs() <= 1e-9 && (p.y - y).abs() <= 1e-9,
            "attendu ({x}, {y}), obtenu ({}, {})",
            p.x,
            p.y
        );
    }

    #[test]
    fn us162_ca1_champs_de_la_bulle_partages_avec_le_panneau() {
        // Chemin « panneau » : les champs sont renseignés directement.
        let mut panel = free_editor();
        panel.set_tool(Tool::Segment);
        panel.click(pt(0.0, 0.0), TOL, false);
        panel.length_text = "50".into();
        panel.angle_text = "30".into();
        panel.confirm();
        // Chemin « bulle » : même résultat avec les touches tapées.
        let mut bubble = free_editor();
        bubble.set_tool(Tool::Segment);
        bubble.click(pt(0.0, 0.0), TOL, false);
        assert!(bubble.entry_active());
        type_all(&mut bubble, "50");
        bubble.entry_field = 1;
        type_all(&mut bubble, "30");
        assert_eq!(
            (bubble.length_text.as_str(), bubble.angle_text.as_str()),
            ("50", "30"),
            "un seul état, deux vues"
        );
        let rows = bubble.entry_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (rows[0].typed.as_str(), rows[1].typed.as_str()),
            ("50", "30")
        );
        assert!(rows[1].active && !rows[0].active);
        bubble.confirm();
        assert_eq!(bytes(&panel), bytes(&bubble), "Entrée = même résultat");
        let id = bubble.sheet().entities[0].id;
        assert!((entity_length(bubble.sheet(), id).unwrap().value - 50.0).abs() < 1e-9);
        assert!(
            (pf_core::measure::segment_direction_deg(bubble.sheet(), id).unwrap() - 30.0).abs()
                < 1e-9
        );
        // Caractères refusés, effacement.
        let mut ed = free_editor();
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        type_all(&mut ed, "1a2b é\n");
        assert_eq!(ed.length_text, "12");
        ed.backspace_entry();
        assert_eq!(ed.length_text, "1");
        for _ in 0..5 {
            ed.backspace_entry();
        }
        assert!(ed.length_text.is_empty());
    }

    #[test]
    fn us162_ca1_mesure_observee_tant_que_rien_n_est_saisi() {
        let mut ed = free_editor();
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.pointer_moved(pt(30.0, 40.0));
        let rows = ed.entry_rows();
        assert!(rows.iter().all(|r| r.typed.is_empty()));
        assert_eq!(rows[0].observed, Some(format_number(50.0, 2)));
        assert_eq!(rows[0].unit, "mm");
        assert_eq!(rows[1].unit, "°");
        let angle = 40f64.atan2(30.0).to_degrees();
        assert_eq!(rows[1].observed, Some(format_number(angle, 2)));
        // Une valeur tapée est distinguée : texte non vide, la mesure reste celle de l'aperçu.
        type_all(&mut ed, "20");
        let rows = ed.entry_rows();
        assert_eq!(rows[0].typed, "20");
        assert_eq!(rows[0].observed, Some(format_number(20.0, 2)));
        // Autres outils : un champ par valeur attendue.
        let mut c = free_editor();
        c.set_tool(Tool::Circle);
        c.click(pt(0.0, 0.0), TOL, false);
        c.pointer_moved(pt(6.0, 8.0));
        let rows = c.entry_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].observed, Some(format_number(10.0, 2)));
        let mut r = free_editor();
        r.set_tool(Tool::Rectangle);
        r.click(pt(0.0, 0.0), TOL, false);
        r.pointer_moved(pt(-30.0, 12.0));
        let rows = r.entry_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].observed, Some(format_number(30.0, 2)));
        assert_eq!(rows[1].observed, Some(format_number(12.0, 2)));
        // Pas de bulle hors saisie.
        let mut s = free_editor();
        assert!(!s.entry_active() && s.entry_rows().is_empty());
        s.set_tool(Tool::Notch);
        assert!(!s.entry_active());
    }

    #[test]
    fn us162_ca2_point_pose_en_absolu_relatif_et_polaire() {
        let mut ed = free_editor();
        ed.set_tool(Tool::Point);
        assert!(ed.accepts_position());
        // Absolu, virgule décimale (FR).
        ed.position_text = "12,5;-3,25".into();
        ed.confirm();
        assert_close(last_point(&ed), 12.5, -3.25);
        assert!(ed.position_text.is_empty());
        // Relatif au dernier point posé.
        ed.position_text = "@10;5".into();
        ed.confirm();
        assert_close(last_point(&ed), 22.5, 1.75);
        // Polaire relative : 20 mm à 90° (Y vers le haut).
        ed.position_text = "@20<90".into();
        ed.confirm();
        assert_close(last_point(&ed), 22.5, 21.75);
        // 100 mm à 30° : (86,602540378443865 ; 50).
        ed.position_text = "@100<30".into();
        ed.confirm();
        assert_close(last_point(&ed), 22.5 + 86.602_540_378_443_86, 21.75 + 50.0);
        assert_eq!(ed.sheet().points.len(), 4);
        // Anglais : point décimal, virgule séparatrice.
        pf_core::input::set_decimal_separator('.');
        ed.position_text = "7.5,2.5".into();
        ed.confirm();
        pf_core::input::set_decimal_separator(',');
        assert_close(last_point(&ed), 7.5, 2.5);
        // Une étape d'annulation par point.
        assert_eq!(undo_depth(&mut ed), 5);
    }

    #[test]
    fn us162_ca2_premier_point_de_segment_puis_coordonnees_dans_la_bulle() {
        let mut ed = free_editor();
        ed.set_tool(Tool::Segment);
        let empty = bytes(&ed);
        ed.position_text = "3;4".into();
        ed.confirm();
        // Le premier point n'est qu'un départ : rien n'est encore créé.
        assert!(matches!(ed.state, ToolState::SegmentFrom { .. }));
        assert_eq!(bytes(&ed), empty);
        // Polaire relative au départ : 30 mm à 45°.
        type_all(&mut ed, "@30<45");
        assert_eq!(ed.length_text, "@30<45");
        let before = bytes(&ed);
        ed.confirm();
        assert_ne!(bytes(&ed), before);
        let id = ed.sheet().entities[0].id;
        let (a, b) = ed.sheet().segment_ends(id).unwrap();
        let k = 30.0 / 2f64.sqrt();
        assert_close(a, 3.0, 4.0);
        assert_close(b, 3.0 + k, 4.0 + k);
        assert!((entity_length(ed.sheet(), id).unwrap().value - 30.0).abs() < 1e-9);
        // Relatif (dx;dy) depuis le dernier point posé.
        ed.set_tool(Tool::Segment);
        ed.position_text = "@0;0".into();
        ed.confirm(); // départ = extrémité du segment précédent
        assert_close(
            match ed.state {
                ToolState::SegmentFrom { start } => ed.ref_pos(start).unwrap(),
                other => panic!("{other:?}"),
            },
            3.0 + k,
            4.0 + k,
        );
        ed.length_text = "@10;0".into();
        ed.confirm();
        let id = ed.sheet().entities[1].id;
        let (a, b) = ed.sheet().segment_ends(id).unwrap();
        assert_close(b, a.x + 10.0, a.y);
        // Absolu depuis un départ quelconque.
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.length_text = "0;100".into();
        ed.confirm();
        let id = ed.sheet().entities[2].id;
        assert_close(ed.sheet().segment_ends(id).unwrap().1, 0.0, 100.0);
    }

    #[test]
    fn us162_ca2_coordonnees_refusees_sans_trace() {
        let mut ed = free_editor();
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        let before = bytes(&ed);
        for bad in [
            "@", "1;2;3", "@0;0", "@5<", "5<45", "1e300;1", "@abc", "NaN;1",
        ] {
            ed.length_text = bad.into();
            ed.confirm();
            assert!(ed.status.error, "{bad:?} : {}", ed.status.text);
            assert_eq!(bytes(&ed), before, "{bad:?}");
            assert!(matches!(ed.state, ToolState::SegmentFrom { .. }), "{bad:?}");
        }
        // Une coordonnée se saisit seule : l'angle doit rester vide.
        ed.length_text = "@10;10".into();
        ed.angle_text = "30".into();
        ed.confirm();
        assert!(ed.status.error);
        assert_eq!(bytes(&ed), before);
        // Position invalide avant le premier point.
        let mut p = free_editor();
        p.set_tool(Tool::Point);
        let before = bytes(&p);
        for bad in ["", "12", "abc;1", "1;2;3", "99999999;0", "@"] {
            p.position_text = bad.into();
            p.confirm();
            assert_eq!(bytes(&p), before, "{bad:?}");
        }
        assert!(!p.doc.can_undo());
    }

    #[test]
    fn us162_ca3_maj_verrouille_la_direction_au_pas_de_15_degres() {
        let mut ed = free_editor();
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.pointer_moved(pt(100.0, 30.0)); // 16,70° à la souris
        let free = ed.preview().unwrap().unwrap();
        let Preview::Segment { angle_deg, .. } = free else {
            panic!("{free:?}");
        };
        assert!((angle_deg - 30f64.atan2(100.0).to_degrees()).abs() < 1e-9);
        ed.angle_lock = true;
        let Some(Preview::Segment {
            length_mm,
            angle_deg,
            ..
        }) = ed.preview().unwrap()
        else {
            panic!("aperçu attendu");
        };
        assert_eq!(angle_deg, 15.0);
        assert!(
            (length_mm - 100.0f64.hypot(30.0)).abs() < 1e-9,
            "longueur de la souris"
        );
        // Par le clic : l'extrémité est sur la direction verrouillée, longueur conservée.
        ed.click(pt(100.0, 30.0), TOL, false);
        let id = ed.sheet().entities[0].id;
        let dir = pf_core::measure::segment_direction_deg(ed.sheet(), id).unwrap();
        assert!((dir - 15.0).abs() < 1e-9, "{dir}");
        assert!((dir / 15.0 - (dir / 15.0).round()).abs() < 1e-9);
        let len = entity_length(ed.sheet(), id).unwrap().value;
        assert!((len - 100.0f64.hypot(30.0)).abs() < 1e-9);
        // Verrou relâché : la direction libre est conservée.
        ed.angle_lock = false;
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 50.0), TOL, false);
        ed.click(pt(100.0, 80.0), TOL, false);
        let id = ed.sheet().entities[1].id;
        let dir = pf_core::measure::segment_direction_deg(ed.sheet(), id).unwrap();
        assert!((dir - 30f64.atan2(100.0).to_degrees()).abs() < 1e-9);
        // Un angle saisi l'emporte sur le verrou.
        ed.angle_lock = true;
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, -50.0), TOL, false);
        ed.pointer_moved(pt(100.0, -20.0));
        ed.angle_text = "20".into();
        ed.length_text = "40".into();
        ed.confirm();
        let id = ed.sheet().entities[2].id;
        let dir = pf_core::measure::segment_direction_deg(ed.sheet(), id).unwrap();
        assert!((dir - 20.0).abs() < 1e-9);
    }

    #[test]
    fn us162_ca3_le_verrou_l_emporte_sur_l_accrochage_a_un_point() {
        let mut ed = free_editor();
        ed.snap_to_paths = true;
        // Un point existant hors de la direction à 15°.
        ed.exec(Command::AddPointAt {
            sheet: ed.sheet,
            point: PointRef::New(pt(100.0, 30.0)),
        })
        .unwrap();
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.pointer_moved(pt(100.0, 30.0));
        assert_eq!(
            ed.hover_snap().map(|s| s.kind),
            Some(SnapKind::Point),
            "sans Maj, le point s'accroche"
        );
        ed.angle_lock = true;
        assert!(
            ed.hover_snap().is_none(),
            "avec Maj, pas de repère trompeur"
        );
        ed.click(pt(100.0, 30.0), TOL, false);
        let id = ed.sheet().entities[0].id;
        let (_, end) = ed.sheet().segment_ends(id).unwrap();
        assert!(pf_geometry::distance_mm(end, pt(100.0, 30.0)) > 1.0);
        let dir = pf_core::measure::segment_direction_deg(ed.sheet(), id).unwrap();
        assert!((dir - 15.0).abs() < 1e-9);
        // Sans Maj, le même clic rejoint le point existant.
        ed.angle_lock = false;
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.click(pt(100.0, 30.0), TOL, false);
        let id = ed.sheet().entities[1].id;
        assert_close(ed.sheet().segment_ends(id).unwrap().1, 100.0, 30.0);
    }

    #[test]
    fn us162_ca3_ligne_d_aide_oblique_verrouillee() {
        let mut ed = free_editor();
        ed.set_tool(Tool::Guide);
        ed.set_guide_mode(GuideMode::Oblique);
        ed.click(pt(0.0, 0.0), TOL, false);
        assert!(matches!(ed.state, ToolState::GuideFrom { .. }));
        ed.angle_lock = true;
        ed.pointer_moved(pt(100.0, 60.0)); // 30,96°
        ed.click(pt(100.0, 60.0), TOL, false);
        assert_eq!(ed.sheet().guides.len(), 1, "{}", ed.status.text);
        assert!((ed.sheet().guides[0].angle_deg - 30.0).abs() < 1e-9);
    }

    #[test]
    fn us162_ca4_repeter_la_derniere_commande_outil_sous_mode_et_valeurs() {
        let mut ed = free_editor();
        // Aucun outil mémorisé : rien ne se passe, message neutre.
        let before = bytes(&ed);
        ed.repeat_last();
        assert_eq!(
            (ed.tool, ed.state, ed.repeat_memo.is_none()),
            (Tool::Select, ToolState::Idle, true)
        );
        assert_eq!(bytes(&ed), before);
        // Segment 50 mm à 30° : mémorisé avec ses valeurs.
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.length_text = "50".into();
        ed.angle_text = "30".into();
        ed.confirm();
        let memo = ed.repeat_memo.clone().expect("outil mémorisé");
        assert_eq!(
            (
                memo.tool,
                memo.length_text.as_str(),
                memo.angle_text.as_str()
            ),
            (Tool::Segment, "50", "30")
        );
        // Un clic de sélection ne remplace pas la mémoire.
        ed.set_tool(Tool::Select);
        ed.click(pt(5.0, 5.0), TOL, false);
        assert_eq!(ed.repeat_memo.as_ref().unwrap().tool, Tool::Segment);
        // Répéter = réactivation : aucune commande, aucune étape d'annulation.
        let (before, depth) = (bytes(&ed), undo_depth(&mut ed));
        ed.repeat_last();
        assert_eq!(ed.tool, Tool::Segment);
        assert_eq!(ed.state, ToolState::Idle);
        assert_eq!(
            (ed.length_text.as_str(), ed.angle_text.as_str()),
            ("50", "30")
        );
        assert_eq!(bytes(&ed), before);
        assert_eq!(undo_depth(&mut ed), depth);
        // Un clic de départ puis Entrée recrée un segment 50 mm à 30°.
        ed.click(pt(0.0, 100.0), TOL, false);
        ed.confirm();
        let id = ed.sheet().entities[1].id;
        assert!((entity_length(ed.sheet(), id).unwrap().value - 50.0).abs() < 1e-9);
        assert!(
            (pf_core::measure::segment_direction_deg(ed.sheet(), id).unwrap() - 30.0).abs() < 1e-9
        );
        // Sous-mode conservé : cercle par deux points et rayon.
        ed.set_tool(Tool::Circle);
        ed.set_round_mode(RoundMode::TwoPointsRadius);
        ed.click(pt(-30.0, 60.0), TOL, false);
        ed.click(pt(-24.0, 60.0), TOL, false);
        ed.pointer_moved(pt(-27.0, 70.0));
        ed.length_text = "5".into();
        ed.confirm();
        assert_eq!(ed.sheet().entities.len(), 3, "{}", ed.status.text);
        ed.set_tool(Tool::Select);
        ed.set_round_mode(RoundMode::Center);
        ed.repeat_last();
        assert_eq!(
            (ed.tool, ed.round_mode, ed.length_text.as_str()),
            (Tool::Circle, RoundMode::TwoPointsRadius, "5")
        );
        // Une commande refusée ne remplace pas la mémoire.
        ed.set_round_mode(RoundMode::Center);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.length_text = "0".into();
        ed.confirm();
        assert!(ed.status.error);
        assert_eq!(
            ed.repeat_memo.as_ref().unwrap().round_mode,
            RoundMode::TwoPointsRadius
        );
    }

    #[test]
    fn us162_ca4_parametres_invalides_aucun_effet() {
        let mut ed = free_editor();
        ed.repeat_memo = Some(RepeatMemo {
            tool: Tool::Segment,
            point_mode: PointMode::Free,
            round_mode: RoundMode::Center,
            guide_mode: GuideMode::Horizontal,
            length_text: "abc".into(),
            angle_text: String::new(),
        });
        let before = bytes(&ed);
        ed.repeat_last();
        assert!(ed.status.error, "{}", ed.status.text);
        assert_eq!((ed.tool, ed.state), (Tool::Select, ToolState::Idle));
        assert!(ed.length_text.is_empty());
        assert_eq!(bytes(&ed), before);
        assert!(!ed.doc.can_undo());
        // Angle illisible : même refus.
        ed.repeat_memo.as_mut().unwrap().length_text = "10".into();
        ed.repeat_memo.as_mut().unwrap().angle_text = "xx".into();
        ed.repeat_last();
        assert!(ed.status.error);
        assert_eq!(ed.tool, Tool::Select);
        // Valeurs lisibles : coordonnée, division (mode Division du point) acceptées.
        ed.repeat_memo = Some(RepeatMemo {
            tool: Tool::Point,
            point_mode: PointMode::Division,
            round_mode: RoundMode::Center,
            guide_mode: GuideMode::Horizontal,
            length_text: "1:3".into(),
            angle_text: String::new(),
        });
        ed.repeat_last();
        assert!(!ed.status.error, "{}", ed.status.text);
        assert_eq!(
            (ed.tool, ed.point_mode, ed.length_text.as_str()),
            (Tool::Point, PointMode::Division, "1:3")
        );
    }

    #[test]
    fn us162_ca5_echap_sans_trace_et_une_etape_d_annulation() {
        let mut ed = free_editor();
        ed.set_tool(Tool::Segment);
        ed.click(pt(0.0, 0.0), TOL, false);
        ed.length_text = "40".into();
        ed.confirm();
        let (before, depth) = (bytes(&ed), undo_depth(&mut ed));
        assert_eq!(depth, 1);
        // Position tapée puis Échap : le texte est effacé, l'outil reste, rien ne bouge.
        ed.set_tool(Tool::Segment);
        type_all(&mut ed, "5;5");
        assert_eq!(ed.position_text, "5;5");
        ed.escape();
        assert!(ed.position_text.is_empty());
        assert_eq!(ed.tool, Tool::Segment);
        assert_eq!((bytes(&ed), undo_depth(&mut ed)), (before.clone(), depth));
        // Étape en cours avec valeurs et coordonnée : Échap annule tout.
        ed.click(pt(0.0, 50.0), TOL, false);
        type_all(&mut ed, "@10<45");
        ed.entry_field = 1;
        type_all(&mut ed, "7");
        ed.escape();
        assert_eq!(ed.state, ToolState::Idle);
        assert!(ed.length_text.is_empty() && ed.angle_text.is_empty());
        assert_eq!((bytes(&ed), undo_depth(&mut ed)), (before.clone(), depth));
        // Une commande validée par coordonnée = une seule étape d'annulation.
        ed.click(pt(0.0, 50.0), TOL, false);
        type_all(&mut ed, "@10<45");
        ed.confirm();
        assert_eq!(undo_depth(&mut ed), depth + 1, "{}", ed.status.text);
        ed.undo();
        assert_eq!(bytes(&ed), before);
        // Point par coordonnée : idem.
        ed.set_tool(Tool::Point);
        type_all(&mut ed, "1;1");
        ed.confirm();
        assert_eq!(undo_depth(&mut ed), depth + 1);
        ed.undo();
        assert_eq!(bytes(&ed), before);
    }

    /// PRNG déterministe (graine fixe).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    fn random_entry(rng: &mut Lcg) -> String {
        const PIECES: &[&str] = &[
            "0",
            "1",
            "7",
            "12",
            "45",
            "90",
            "180",
            "360",
            "-5",
            "+3",
            "1,5",
            "2.75",
            ";",
            ",",
            ".",
            "@",
            "<",
            " ",
            "mm",
            "cm",
            "m",
            "°",
            "e",
            "E",
            "NaN",
            "inf",
            "-inf",
            "1e300",
            "é",
            "１２",
            "💥",
            "\u{a0}",
            "\0",
            "99999999999",
            "0.0000000001",
            "@@",
            "<<",
            ";;",
            "x",
            "10;20",
            "@5;5",
            "@10<30",
            "@0<0",
            "-0",
            "1/0",
        ];
        const HUGE: &[&str] = &[
            "100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
        ];
        let mut text = String::new();
        for _ in 0..rng.below(7) {
            if rng.below(40) == 0 {
                text.push_str(HUGE[0]);
            } else {
                text.push_str(PIECES[rng.below(PIECES.len())]);
            }
        }
        text
    }

    #[test]
    fn us162_campagne_aleatoire_de_saisies_graine_fixe() {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let mut rng = Lcg(0x162_0000_0000_0002);
        let tools = [
            Tool::Point,
            Tool::Segment,
            Tool::Grainline,
            Tool::Rectangle,
            Tool::Circle,
            Tool::Arc,
            Tool::Curve,
            Tool::Perpendicular,
            Tool::Parallel,
            Tool::PointOnSegment,
        ];
        let mut accepted = 0;
        let mut refused = 0;
        for draw in 0..400 {
            let mut ed = free_editor();
            // Un segment de référence pour les outils qui en demandent un.
            ed.exec(Command::AddSegment {
                sheet: ed.sheet,
                from: PointRef::New(pt(0.0, 0.0)),
                to: PointRef::New(pt(100.0, 0.0)),
            })
            .unwrap();
            let tool = tools[rng.below(tools.len())];
            ed.set_tool(tool);
            if rng.below(2) == 0 {
                ed.angle_lock = true;
            }
            // Amorce de l'étape : un ou deux clics selon l'outil.
            let clicks = match tool {
                Tool::Perpendicular => vec![pt(50.0, 0.0), pt(60.0, 0.0)],
                Tool::Parallel | Tool::PointOnSegment => vec![pt(50.0, 0.0)],
                Tool::Point => Vec::new(),
                _ => vec![pt(10.0, 10.0)],
            };
            for c in clicks {
                ed.click(c, TOL, false);
            }
            ed.pointer_moved(pt(
                (rng.below(2000) as f64) - 1000.0,
                (rng.below(2000) as f64) - 1000.0,
            ));
            let before = bytes(&ed);
            let depth = undo_depth(&mut ed);
            let (text_a, text_b) = (random_entry(&mut rng), random_entry(&mut rng));
            let via_keys = rng.below(2) == 0;
            let result = catch_unwind(AssertUnwindSafe(|| {
                if via_keys {
                    type_all(&mut ed, &text_a);
                    ed.entry_field = 1;
                    type_all(&mut ed, &text_b);
                } else {
                    ed.length_text = text_a.clone();
                    ed.angle_text = text_b.clone();
                    ed.position_text = text_a.clone();
                }
                let _ = ed.entry_rows();
                let _ = ed.preview();
                let _ = ed.hover_snap();
                ed.confirm();
                ed
            }));
            let Ok(mut ed) = result else {
                panic!("panique au tirage {draw} : {tool:?} {text_a:?} {text_b:?}");
            };
            ed.project()
                .validate()
                .unwrap_or_else(|e| panic!("projet corrompu au tirage {draw} : {e:?}"));
            let after = undo_depth(&mut ed);
            if after == depth {
                // Refus (ou étape intermédiaire) : projet identique octet pour octet.
                assert_eq!(
                    bytes(&ed),
                    before,
                    "tirage {draw} : {tool:?} {text_a:?} {text_b:?}"
                );
                refused += 1;
            } else {
                assert_eq!(after, depth + 1, "tirage {draw} : plus d'une étape");
                ed.undo();
                assert_eq!(bytes(&ed), before, "tirage {draw} : annulation inexacte");
                accepted += 1;
            }
            // Échap ne laisse aucune trace de saisie.
            ed.escape();
            ed.escape();
            assert!(ed.position_text.is_empty() && ed.length_text.is_empty());
        }
        assert!(accepted > 10 && refused > 100, "{accepted} / {refused}");
    }
}

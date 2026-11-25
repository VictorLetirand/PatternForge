//! Agencement des étiquettes du plan (PH-2c, défaut 11) : un seul module, purement d'affichage.
//!
//! Les étiquettes (points, cotes, libellés de tracés, de raccords, de coutures…) sont d'abord
//! **collectées** pendant le dessin du canevas, puis posées d'un coup en fin de dessin :
//! priorité fixe sélection > cote > point > libellé de tracé (à priorité égale, ordre de
//! dessin) ; pour chacune, on retient le premier candidat sans chevauchement parmi la position
//! d'origine puis 24 positions décalées (3 couronnes de 8 directions), sinon la position
//! d'origine. Un trait de rappel très fin relie une étiquette déplacée à son point d'attache.
//!
//! Rien n'est écrit dans le projet ni dans un export ; aucune étiquette n'est supprimée.
//! Borne de coût : au plus [`MAX_LABELS`] étiquettes visibles sont agencées (les moins
//! prioritaires, au-delà, restent à leur position d'origine ; leur nombre est annoncé au pied
//! du canevas, jamais passé sous silence).

use eframe::egui::{self, Align2, Color32, Id, LayerId, Pos2, Rect, Stroke, Vec2};
use std::collections::HashMap;
use std::sync::Arc;

/// Nombre maximal d'étiquettes visibles agencées par image.
pub(crate) const MAX_LABELS: usize = 400;
/// Écart laissé entre deux étiquettes voisines (pixels).
const GAP: f32 = 2.0;
/// Nombre de couronnes de candidats autour de la position d'origine.
const RINGS: usize = 3;
/// Distance (pixels) au-delà de laquelle une étiquette déplacée reçoit un trait de rappel.
const LEADER_MIN: f32 = 4.0;
/// Côté d'une case de la grille d'accélération (pixels).
const CELL: f32 = 64.0;

/// Nature d'une étiquette ; l'ordre des variantes est l'ordre de priorité (la première est la
/// plus prioritaire : une étiquette de sélection ne bouge que si rien d'autre ne la gêne).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    Selected,
    Dimension,
    Point,
    Path,
}

/// Une étiquette à poser : son encombrement d'origine (pixels écran) et sa nature.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Request {
    pub rect: Rect,
    pub kind: Kind,
}

/// Résultat pour une étiquette (même indice que la demande).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placed {
    /// Encombrement retenu (identique à la demande si l'étiquette n'a pas bougé).
    pub rect: Rect,
    /// Déplacée vers un candidat libre.
    pub moved: bool,
    /// Gênée sans candidat libre : laissée à sa position d'origine.
    pub unresolved: bool,
    /// Visible et dans la borne : a été examinée.
    pub treated: bool,
}

/// Résultat de l'agencement.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Layout {
    pub placed: Vec<Placed>,
    /// Étiquettes visibles au-delà de la borne, laissées à leur position d'origine.
    pub skipped: usize,
}

/// Chevauchement strict (deux rectangles qui se touchent seulement par un bord ne se
/// recouvrent pas).
fn overlaps(a: Rect, b: Rect) -> bool {
    a.min.x < b.max.x && b.min.x < a.max.x && a.min.y < b.max.y && b.min.y < a.max.y
}

/// Décalages essayés après la position d'origine, dans cet ordre : par couronne `k` (1 à 3),
/// haut, bas, droite, gauche, puis les quatre diagonales (haut-droite, haut-gauche, bas-droite,
/// bas-gauche) ; `dx = largeur + 2`, `dy = hauteur + 2`.
fn shifts(size: Vec2) -> Vec<Vec2> {
    let (dx, dy) = (size.x + GAP, size.y + GAP);
    let mut out = Vec::with_capacity(RINGS * 8);
    for k in 1..=RINGS {
        let k = k as f32;
        let (x, y) = (k * dx, k * dy);
        for (sx, sy) in [
            (0.0, -y),
            (0.0, y),
            (x, 0.0),
            (-x, 0.0),
            (x, -y),
            (-x, -y),
            (x, y),
            (-x, y),
        ] {
            out.push(Vec2::new(sx, sy));
        }
    }
    out
}

/// Rectangles déjà posés, indexés par une grille pour ne tester que les voisins.
struct Occupied {
    rects: Vec<Rect>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

impl Occupied {
    fn new() -> Self {
        Occupied {
            rects: Vec::new(),
            cells: HashMap::new(),
        }
    }

    fn span(r: Rect) -> (i32, i32, i32, i32) {
        (
            (r.min.x / CELL).floor() as i32,
            (r.min.y / CELL).floor() as i32,
            (r.max.x / CELL).floor() as i32,
            (r.max.y / CELL).floor() as i32,
        )
    }

    fn add(&mut self, r: Rect) {
        let id = self.rects.len();
        self.rects.push(r);
        let (x0, y0, x1, y1) = Self::span(r);
        for cx in x0..=x1 {
            for cy in y0..=y1 {
                self.cells.entry((cx, cy)).or_default().push(id);
            }
        }
    }

    /// Vrai si `r` en recouvre un. Seule l'existence d'un recouvrement compte : l'ordre
    /// d'itération de la table n'influence pas le résultat (déterministe).
    fn blocked(&self, r: Rect) -> bool {
        let (x0, y0, x1, y1) = Self::span(r);
        for cx in x0..=x1 {
            for cy in y0..=y1 {
                if let Some(ids) = self.cells.get(&(cx, cy)) {
                    if ids.iter().any(|&i| overlaps(self.rects[i], r)) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Agence les étiquettes avec la borne par défaut ([`MAX_LABELS`]).
pub(crate) fn layout(requests: &[Request], obstacles: &[Rect], clip: Rect) -> Layout {
    layout_limited(requests, obstacles, clip, MAX_LABELS)
}

/// Agence les étiquettes : `obstacles` = rectangles fixes à éviter (marqueurs de points),
/// `clip` = zone visible (une étiquette hors de cette zone n'est pas examinée ; un candidat ne
/// sort jamais de la zone). Au plus `limit` étiquettes visibles sont examinées, des plus
/// prioritaires aux moins prioritaires.
pub(crate) fn layout_limited(
    requests: &[Request],
    obstacles: &[Rect],
    clip: Rect,
    limit: usize,
) -> Layout {
    let mut placed: Vec<Placed> = requests
        .iter()
        .map(|q| Placed {
            rect: q.rect,
            moved: false,
            unresolved: false,
            treated: false,
        })
        .collect();
    let mut order: Vec<usize> = (0..requests.len())
        .filter(|&i| overlaps(requests[i].rect, clip))
        .collect();
    // Tri stable : priorité, puis ordre de dessin.
    order.sort_by_key(|&i| (requests[i].kind, i));
    let skipped = order.len().saturating_sub(limit);
    order.truncate(limit);

    let mut occupied = Occupied::new();
    let reach = clip.expand(CELL);
    for o in obstacles.iter().filter(|o| overlaps(**o, reach)) {
        occupied.add(*o);
    }
    for i in order {
        let original = requests[i].rect;
        placed[i].treated = true;
        if !occupied.blocked(original) {
            occupied.add(original);
            continue;
        }
        let free = shifts(original.size())
            .into_iter()
            .map(|d| original.translate(d))
            .find(|c| clip.contains_rect(*c) && !occupied.blocked(*c));
        match free {
            Some(c) => {
                placed[i].rect = c;
                placed[i].moved = true;
                occupied.add(c);
            }
            None => {
                placed[i].unresolved = true;
                occupied.add(original);
            }
        }
    }
    Layout { placed, skipped }
}

/// Trait de rappel d'une étiquette déplacée : de son point d'attache au point le plus proche de
/// son rectangle, seulement si elle en est éloignée de plus de 4 pixels.
pub(crate) fn leader(rect: Rect, anchor: Pos2) -> Option<[Pos2; 2]> {
    (rect.distance_to_pos(anchor) > LEADER_MIN).then(|| [anchor, rect.clamp(anchor)])
}

// ----- collecte pendant le dessin du canevas --------------------------------------------------

/// Comment une étiquette se dessine une fois sa place connue.
#[derive(Clone)]
enum Draw {
    /// Étiquette à fond (le rectangle est celui du fond ; texte à 3 pixels du bord).
    Boxed { galley: Arc<egui::Galley> },
    /// Texte seul (le rectangle englobe le texte).
    Plain { galley: Arc<egui::Galley> },
    /// Texte orienté (le rectangle est l'emprise du texte tourné) ; `pos` = coin du texte.
    Rotated {
        galley: Arc<egui::Galley>,
        pos: Pos2,
        angle: f32,
    },
}

#[derive(Clone)]
struct Pending {
    request: Request,
    anchor: Pos2,
    color: Color32,
    text: String,
    draw: Draw,
}

#[derive(Clone, Default)]
struct Sink {
    layer: Option<LayerId>,
    items: Vec<Pending>,
    obstacles: Vec<Rect>,
}

/// Bilan de la dernière image (lecture par l'interface et par les tests).
#[derive(Clone, Debug, Default)]
pub(crate) struct Report {
    pub skipped: usize,
    #[cfg(test)]
    pub count: usize,
    #[cfg(test)]
    pub moved: usize,
    /// Texte de l'étiquette sous le pointeur (info-bulle).
    #[cfg(test)]
    pub tip: Option<String>,
}

fn sink_id() -> Id {
    Id::new("pf_label_sink")
}

fn report_id() -> Id {
    Id::new("pf_label_report")
}

/// Ouvre la collecte : les étiquettes dessinées sur le calque de `painter` sont retenues
/// jusqu'à [`flush`].
pub(crate) fn begin(painter: &egui::Painter) {
    painter.ctx().data_mut(|d| {
        *d.get_temp_mut_or_default::<Sink>(sink_id()) = Sink {
            layer: Some(painter.layer_id()),
            ..Sink::default()
        };
    });
}

/// Retient une étiquette si une collecte est ouverte pour le calque de `painter` ; sinon
/// renvoie `false` et l'appelant la dessine tout de suite.
fn collect(painter: &egui::Painter, pending: Pending) -> bool {
    painter.ctx().data_mut(|d| {
        let sink = d.get_temp_mut_or_default::<Sink>(sink_id());
        if sink.layer == Some(painter.layer_id()) {
            sink.items.push(pending);
            true
        } else {
            false
        }
    })
}

/// Signale un rectangle fixe (marqueur de point) que les étiquettes évitent.
pub(crate) fn reserve(painter: &egui::Painter, rect: Rect) {
    painter.ctx().data_mut(|d| {
        let sink = d.get_temp_mut_or_default::<Sink>(sink_id());
        if sink.layer == Some(painter.layer_id()) {
            sink.obstacles.push(rect);
        }
    });
}

/// Étiquette à fond, comme `canvas::label` (`pos` = point d'attache, décalé par l'appelant).
pub(crate) fn boxed(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    color: Color32,
    kind: Kind,
) -> bool {
    let galley = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(13.0), color);
    let rect = anchor.anchor_size(pos, galley.size()).expand(3.0);
    collect(
        painter,
        Pending {
            request: Request { rect, kind },
            anchor: pos,
            color,
            text: text.to_string(),
            draw: Draw::Boxed { galley },
        },
    )
}

/// Texte seul (nom d'un point) ; la demande englobe le texte à 1 pixel près.
pub(crate) fn plain(
    painter: &egui::Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    font_size: f32,
    color: Color32,
    kind: Kind,
) -> bool {
    let galley = painter.layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(font_size),
        color,
    );
    let rect = anchor.anchor_size(pos, galley.size());
    collect(
        painter,
        Pending {
            request: Request {
                rect: rect.expand(1.0),
                kind,
            },
            anchor: pos,
            color,
            text: text.to_string(),
            draw: Draw::Plain { galley },
        },
    )
}

/// Texte orienté (cote complète) : `pos` = coin du texte avant rotation (rotation autour de
/// ce coin, `angle` en radians), `center` = centre du texte tourné, `anchor` = point attaché.
pub(crate) fn rotated(
    painter: &egui::Painter,
    galley: Arc<egui::Galley>,
    pos: Pos2,
    angle: f32,
    anchor: Pos2,
    color: Color32,
    kind: Kind,
) -> bool {
    let size = galley.size();
    let (s, c) = angle.sin_cos();
    let corners = [
        Vec2::ZERO,
        Vec2::new(size.x, 0.0),
        Vec2::new(0.0, size.y),
        size,
    ]
    .map(|v| pos + Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c));
    let rect = Rect::from_points(&corners);
    collect(
        painter,
        Pending {
            request: Request { rect, kind },
            anchor,
            color,
            text: galley.text().to_string(),
            draw: Draw::Rotated { galley, pos, angle },
        },
    )
}

/// Pose et dessine les étiquettes collectées. `clip` = zone où un candidat peut tomber ;
/// `hover` = position du pointeur : renvoie le texte complet de l'étiquette qu'il survole
/// (info-bulle ; vide pour aucune).
pub(crate) fn flush(painter: &egui::Painter, clip: Rect, hover: Option<Pos2>) -> Option<String> {
    let ctx = painter.ctx();
    let sink = ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<Sink>(sink_id())));
    let requests: Vec<Request> = sink.items.iter().map(|p| p.request).collect();
    let result = layout(&requests, &sink.obstacles, clip);
    let mut tip = None;
    for (item, placed) in sink.items.iter().zip(&result.placed) {
        let delta = placed.rect.min - item.request.rect.min;
        if placed.moved {
            if let Some(line) = leader(placed.rect, item.anchor) {
                painter.line_segment(line, Stroke::new(0.75_f32, item.color.gamma_multiply(0.7)));
            }
        }
        match &item.draw {
            Draw::Boxed { galley } => {
                painter.rect_filled(placed.rect, 3.0, crate::palette::pal().label_bg);
                painter.galley(
                    placed.rect.min + Vec2::splat(3.0),
                    galley.clone(),
                    item.color,
                );
            }
            Draw::Plain { galley } => {
                painter.galley(
                    placed.rect.min + Vec2::splat(1.0),
                    galley.clone(),
                    item.color,
                );
            }
            Draw::Rotated { galley, pos, angle } => {
                painter.add(
                    egui::epaint::TextShape::new(*pos + delta, galley.clone(), item.color)
                        .with_angle(*angle),
                );
            }
        }
        if tip.is_none() && hover.is_some_and(|h| placed.rect.contains(h)) {
            tip = Some(item.text.clone());
        }
    }
    let report = Report {
        skipped: result.skipped,
        #[cfg(test)]
        count: sink.items.len(),
        #[cfg(test)]
        moved: result.placed.iter().filter(|p| p.moved).count(),
        #[cfg(test)]
        tip: tip.clone(),
    };
    ctx.data_mut(|d| d.insert_temp(report_id(), report));
    tip
}

/// Bilan de la dernière image dessinée (étiquettes, déplacées, au-delà de la borne).
#[cfg(test)]
pub(crate) fn last_report(ctx: &egui::Context) -> Report {
    ctx.data(|d| d.get_temp::<Report>(report_id()))
        .unwrap_or_default()
}

/// Nombre d'étiquettes laissées à leur place d'origine faute de borne, à la dernière image.
pub(crate) fn skipped(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp::<Report>(report_id()))
        .map_or(0, |r| r.skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
    }

    fn big() -> Rect {
        r(-1000.0, -1000.0, 1000.0, 1000.0)
    }

    fn req(rect: Rect, kind: Kind) -> Request {
        Request { rect, kind }
    }

    fn same(n: usize, kind: Kind) -> Vec<Request> {
        vec![req(r(0.0, 0.0, 20.0, 10.0), kind); n]
    }

    #[test]
    fn deux_etiquettes_superposees_la_seconde_monte() {
        // Étiquettes de 20 x 10, pas dx = 22, dy = 12. La première (priorité égale, ordre de
        // dessin) garde (0,0)-(20,10). La seconde : candidat « haut » = décalage (0, -12) soit
        // (0,-12)-(20,-2), qui ne recouvre pas (0,0)-(20,10) (y : -2 < 0) : retenu.
        let out = layout(&same(2, Kind::Path), &[], big());
        assert_eq!(out.placed[0].rect, r(0.0, 0.0, 20.0, 10.0));
        assert!(!out.placed[0].moved);
        assert_eq!(out.placed[1].rect, r(0.0, -12.0, 20.0, -2.0));
        assert!(out.placed[1].moved && !out.placed[1].unresolved);
        assert_eq!(out.skipped, 0);
    }

    #[test]
    fn deux_etiquettes_qui_se_touchent_ne_se_recouvrent_pas() {
        let a = req(r(0.0, 0.0, 20.0, 10.0), Kind::Path);
        let b = req(r(20.0, 0.0, 40.0, 10.0), Kind::Path);
        let out = layout(&[a, b], &[], big());
        assert!(!out.placed[0].moved && !out.placed[1].moved);
    }

    #[test]
    fn etiquette_entouree_de_quatre_voisins_prend_une_diagonale() {
        // Cible (0,0)-(20,10), cachée par un obstacle (5,2)-(15,8). Voisins fixes : haut
        // (0,-12)-(20,-2), bas (0,12)-(20,22), droite (22,0)-(42,10), gauche (-22,0)-(-2,10).
        // Couronne 1 : haut, bas, droite, gauche bloqués (rectangles identiques) ; haut-droite
        // = décalage (22,-12) soit (22,-12)-(42,-2) : x = 22 >= 20 (hors du voisin du haut),
        // y = -2 < 0 (hors du voisin de droite) : libre, retenu.
        let obstacles = [
            r(5.0, 2.0, 15.0, 8.0),
            r(0.0, -12.0, 20.0, -2.0),
            r(0.0, 12.0, 20.0, 22.0),
            r(22.0, 0.0, 42.0, 10.0),
            r(-22.0, 0.0, -2.0, 10.0),
        ];
        let out = layout(&same(1, Kind::Point), &obstacles, big());
        assert_eq!(out.placed[0].rect, r(22.0, -12.0, 42.0, -2.0));
        assert!(out.placed[0].moved);
    }

    #[test]
    fn sans_candidat_libre_l_etiquette_reste_a_sa_place() {
        // Un obstacle de (-200,-200) à (200,200) couvre l'origine et les 24 candidats (décalage
        // maximal 66 en x et 36 en y) : position d'origine, marquée non résolue.
        let out = layout(
            &same(1, Kind::Path),
            &[r(-200.0, -200.0, 200.0, 200.0)],
            big(),
        );
        assert_eq!(out.placed[0].rect, r(0.0, 0.0, 20.0, 10.0));
        assert!(!out.placed[0].moved && out.placed[0].unresolved);
    }

    #[test]
    fn la_selection_est_prioritaire_quel_que_soit_l_ordre_de_dessin() {
        // [libellé de tracé, sélection] superposés : la sélection (posée en premier) garde sa
        // place, le libellé de tracé monte en (0,-12)-(20,-2).
        let o = r(0.0, 0.0, 20.0, 10.0);
        let out = layout(&[req(o, Kind::Path), req(o, Kind::Selected)], &[], big());
        assert_eq!(out.placed[1].rect, o);
        assert!(!out.placed[1].moved);
        assert_eq!(out.placed[0].rect, r(0.0, -12.0, 20.0, -2.0));
    }

    #[test]
    fn ordre_de_priorite_cote_point_trace() {
        // Dessinés dans l'ordre tracé, point, cote ; posés cote (reste), point (haut :
        // (0,-12)-(20,-2)), tracé (haut bloqué par le point, bas libre : (0,12)-(20,22)).
        let o = r(0.0, 0.0, 20.0, 10.0);
        let out = layout(
            &[
                req(o, Kind::Path),
                req(o, Kind::Point),
                req(o, Kind::Dimension),
            ],
            &[],
            big(),
        );
        assert_eq!(out.placed[2].rect, o);
        assert_eq!(out.placed[1].rect, r(0.0, -12.0, 20.0, -2.0));
        assert_eq!(out.placed[0].rect, r(0.0, 12.0, 20.0, 22.0));
    }

    #[test]
    fn six_etiquettes_identiques_se_rangent_autour_de_la_premiere() {
        // 0 : origine. 1 : haut (0,-12). 2 : haut bloqué, bas (0,12). 3 : haut et bas bloqués,
        // droite (22,0). 4 : droite bloquée, gauche (-22,0). 5 : les quatre bloqués, haut-droite
        // (22,-12)-(42,-2), libre (x >= 20 face à 1, y <= 0 face à 3).
        let out = layout(&same(6, Kind::Path), &[], big());
        let want = [
            (0.0, 0.0),
            (0.0, -12.0),
            (0.0, 12.0),
            (22.0, 0.0),
            (-22.0, 0.0),
            (22.0, -12.0),
        ];
        for (p, (x, y)) in out.placed.iter().zip(want) {
            assert_eq!(p.rect, r(x, y, x + 20.0, y + 10.0));
        }
        assert_eq!(out.placed.iter().filter(|p| p.moved).count(), 5);
    }

    #[test]
    fn la_borne_laisse_les_dernieres_a_leur_place_et_les_compte() {
        // limite 3, 5 identiques : 0 reste, 1 monte, 2 descend (haut déjà pris), 3 et 4 ne sont
        // pas examinées : position d'origine, `skipped` = 2.
        let out = layout_limited(&same(5, Kind::Path), &[], big(), 3);
        assert_eq!(out.skipped, 2);
        assert_eq!(out.placed[1].rect, r(0.0, -12.0, 20.0, -2.0));
        assert_eq!(out.placed[2].rect, r(0.0, 12.0, 20.0, 22.0));
        for i in [3, 4] {
            assert_eq!(out.placed[i].rect, r(0.0, 0.0, 20.0, 10.0));
            assert!(!out.placed[i].treated && !out.placed[i].moved);
        }
    }

    #[test]
    fn la_borne_par_defaut_est_400() {
        assert_eq!(MAX_LABELS, 400);
        let out = layout(&same(405, Kind::Path), &[], big());
        assert_eq!(out.skipped, 5);
        assert_eq!(out.placed.len(), 405);
        assert_eq!(out.placed.iter().filter(|p| p.treated).count(), 400);
    }

    #[test]
    fn un_candidat_hors_de_la_zone_visible_est_refuse() {
        // Zone (0,-5)-(100,100) : le candidat « haut » (0,-12)-(20,-2) sort par le haut ; la
        // seconde étiquette prend « bas » (0,12)-(20,22).
        let out = layout(&same(2, Kind::Path), &[], r(0.0, -5.0, 100.0, 100.0));
        assert_eq!(out.placed[1].rect, r(0.0, 12.0, 20.0, 22.0));
    }

    #[test]
    fn une_etiquette_hors_de_la_zone_n_est_pas_examinee_et_ne_gene_personne() {
        // La première (0,0)-(20,10) est hors de la zone (50,50)-(100,100) : intacte, non
        // examinée ; la seconde, identique mais d'origine visible (60,60)-(80,70), reste.
        let out = layout(
            &[
                req(r(0.0, 0.0, 20.0, 10.0), Kind::Path),
                req(r(60.0, 60.0, 80.0, 70.0), Kind::Path),
            ],
            &[],
            r(50.0, 50.0, 100.0, 100.0),
        );
        assert!(!out.placed[0].treated);
        assert!(out.placed[1].treated && !out.placed[1].moved);
    }

    #[test]
    fn determinisme_et_aucune_etiquette_perdue() {
        // Séquence pseudo-aléatoire fixe : deux exécutions identiques, même nombre et mêmes
        // tailles en sortie.
        let mut seed = 12345_u32;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 16) as f32 / 65536.0
        };
        let reqs: Vec<Request> = (0..60)
            .map(|i| {
                let (x, y) = (next() * 200.0, next() * 120.0);
                let kind = [Kind::Selected, Kind::Dimension, Kind::Point, Kind::Path][i % 4];
                req(r(x, y, x + 24.0, y + 12.0), kind)
            })
            .collect();
        let a = layout(&reqs, &[], r(0.0, 0.0, 300.0, 200.0));
        let b = layout(&reqs, &[], r(0.0, 0.0, 300.0, 200.0));
        assert_eq!(a, b);
        assert_eq!(a.placed.len(), reqs.len());
        for (q, p) in reqs.iter().zip(&a.placed) {
            assert_eq!(q.rect.size(), p.rect.size());
        }
    }

    #[test]
    fn trait_de_rappel_seulement_si_l_etiquette_est_eloignee() {
        // Rectangle (0,-12)-(20,-2), attache (10,5) : plus proche point (10,-2), distance 7 > 4.
        let line = leader(r(0.0, -12.0, 20.0, -2.0), Pos2::new(10.0, 5.0)).expect("trait");
        assert_eq!(line, [Pos2::new(10.0, 5.0), Pos2::new(10.0, -2.0)]);
        // Attache à 3 pixels du bord : pas de trait.
        assert!(leader(r(0.0, -12.0, 20.0, -2.0), Pos2::new(10.0, 1.0)).is_none());
    }
}

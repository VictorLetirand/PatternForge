//! Glossaire métier FR/EN du patronage (US-19.4) : entrée du menu Aide, lecture seule. Chaque
//! terme existe dans les deux langues (clés `glossary.t.<id>.term` et `.def` des deux
//! catalogues) ; la liste montre toujours « français / anglais », la définition suit la langue de
//! l'interface. Aucune traduction n'est validée par une modéliste : les termes les plus
//! incertains portent la marque « à faire valider ».

use crate::app::App;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_i18n::{lookup, tr, Lang};

/// Un terme du patronage.
pub struct Term {
    /// Identifiant stable (minuscules et `_`), racine des clés de catalogue.
    pub id: &'static str,
    /// Traduction ou emploi à faire valider par une modéliste.
    pub to_validate: bool,
}

const fn term(id: &'static str, to_validate: bool) -> Term {
    Term { id, to_validate }
}

pub const TERMS: &[Term] = &[
    term("pattern", false),
    term("pattern_piece", false),
    term("sheet", false),
    term("block", true),
    term("grainline", false),
    term("selvedge", false),
    term("nap", true),
    term("bias", false),
    term("notch", false),
    term("dart", false),
    term("pleat", true),
    term("gathers", false),
    term("fold", false),
    term("seam_allowance", false),
    term("sewing_line", false),
    term("cutting_line", false),
    term("hem", false),
    term("topstitch", false),
    term("stitch_holes", true),
    term("center_front", false),
    term("center_back", false),
    term("waistline", false),
    term("hipline", false),
    term("bustline", false),
    term("armhole", false),
    term("neckline", false),
    term("skiving", true),
    term("buttonhole", false),
    term("lining", false),
    term("interfacing", false),
    term("ease", true),
    term("grading", false),
    term("size_range", true),
    term("nesting", true),
    term("calibration_square", true),
];

impl Term {
    /// Libellé du terme dans une langue.
    pub fn label(&self, lang: Lang) -> String {
        lookup(lang, &format!("glossary.t.{}.term", self.id))
    }

    /// Définition dans une langue.
    pub fn definition(&self, lang: Lang) -> String {
        lookup(lang, &format!("glossary.t.{}.def", self.id))
    }

    /// Le filtre (sans casse) trouve le terme dans l'une ou l'autre langue.
    pub fn matches(&self, filter: &str) -> bool {
        let f = filter.trim().to_lowercase();
        f.is_empty()
            || Lang::ALL
                .iter()
                .any(|l| self.label(*l).to_lowercase().contains(&f))
    }
}

/// Termes visibles pour un filtre, dans l'ordre alphabétique du français.
pub fn visible(filter: &str) -> Vec<&'static Term> {
    let mut v: Vec<&Term> = TERMS.iter().filter(|t| t.matches(filter)).collect();
    v.sort_by_key(|t| t.label(Lang::Fr).to_lowercase());
    v
}

impl App {
    /// Fenêtre « Glossaire » : lecture seule, filtre sur les deux langues.
    pub fn glossary_window(&mut self, ctx: &egui::Context) {
        if !self.show_glossary {
            return;
        }
        let mut open = true;
        let muted = pal().muted;
        let ui_lang = pf_i18n::lang();
        egui::Window::new(tr("glossary.title"))
            .open(&mut open)
            .default_width(520.0)
            .show(ctx, |ui| {
                crate::a11y::label(ui, RichText::new(tr("glossary.note")).small().color(muted));
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("glossary.filter"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut self.glossary_filter),
                    );
                });
                ui.separator();
                let terms = visible(&self.glossary_filter);
                if terms.is_empty() {
                    crate::a11y::label(ui, tr("glossary.empty"));
                }
                egui::ScrollArea::vertical()
                    .max_height(
                        420.0_f32
                            .min(crate::panels::window_room(ctx) - 130.0)
                            .max(80.0),
                    )
                    .show(ui, |ui| {
                        for t in terms {
                            let mut head = format!("{} / {}", t.label(Lang::Fr), t.label(Lang::En));
                            if t.to_validate {
                                head.push_str(&format!(" ({})", tr("glossary.to_validate")));
                            }
                            ui.push_id(t.id, |ui| {
                                crate::a11y::label(ui, RichText::new(head).strong());
                                crate::a11y::label(ui, t.definition(ui_lang));
                                ui.add_space(4.0);
                            });
                        }
                    });
            });
        self.show_glossary = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Launch;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use std::collections::HashSet;

    #[test]
    fn chaque_terme_existe_en_francais_et_en_anglais() {
        assert!(TERMS.len() >= 30);
        let mut ids = HashSet::new();
        let mut fr = HashSet::new();
        for t in TERMS {
            assert!(ids.insert(t.id), "id en double : {}", t.id);
            assert!(t.id.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            for lang in Lang::ALL {
                for suffix in ["term", "def"] {
                    let key = format!("glossary.t.{}.{suffix}", t.id);
                    assert!(
                        lang.catalog().iter().any(|(k, _)| *k == key),
                        "{key} absente du catalogue {lang:?}"
                    );
                }
                assert!(!t.label(lang).trim().is_empty(), "{} ({lang:?})", t.id);
                assert!(
                    t.definition(lang).chars().count() > 20,
                    "définition {} ({lang:?})",
                    t.id
                );
            }
            assert!(
                fr.insert(t.label(Lang::Fr)),
                "terme français en double : {}",
                t.id
            );
            assert_ne!(
                t.label(Lang::Fr),
                t.label(Lang::En),
                "{} : FR et EN identiques (copie ?)",
                t.id
            );
        }
    }

    #[test]
    fn aucune_entree_de_glossaire_orpheline_dans_les_catalogues() {
        for lang in Lang::ALL {
            for (key, _) in lang.catalog() {
                if let Some(rest) = key.strip_prefix("glossary.t.") {
                    let id = rest.rsplit_once('.').unwrap().0;
                    assert!(TERMS.iter().any(|t| t.id == id), "{key} sans terme");
                }
            }
        }
    }

    #[test]
    fn le_glossaire_couvre_les_roles_de_trait_de_l_application() {
        for id in [
            "grainline",
            "notch",
            "dart",
            "seam_allowance",
            "sewing_line",
            "cutting_line",
            "topstitch",
            "center_front",
            "center_back",
            "waistline",
            "hipline",
            "bustline",
            "skiving",
            "buttonhole",
            "fold",
            "grading",
        ] {
            assert!(TERMS.iter().any(|t| t.id == id), "terme absent : {id}");
        }
    }

    #[test]
    fn le_filtre_cherche_dans_les_deux_langues() {
        let ids = |f: &str| visible(f).iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids("pince"), ["dart"]);
        assert_eq!(ids("DART"), ["dart"]);
        assert!(ids("zzz").is_empty());
        assert_eq!(ids("  ").len(), TERMS.len());
        assert!(ids("couture").contains(&"seam_allowance"));
        assert!(ids("seam").contains(&"seam_allowance"));
        let all = visible("");
        assert!(all
            .windows(2)
            .all(|w| w[0].label(Lang::Fr).to_lowercase() <= w[1].label(Lang::Fr).to_lowercase()));
    }

    fn harness(lang: Lang) -> Harness<'static, App> {
        pf_i18n::set_lang(lang);
        let app = App::with_context(&egui::Context::default(), Launch::default());
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_state(|ctx, app: &mut App| app.ui(ctx), app);
        h.run();
        h
    }

    #[test]
    fn la_fenetre_s_affiche_et_filtre_en_francais_et_en_anglais() {
        for (lang, title, def) in [
            (Lang::Fr, "Glossaire", "Pli cousu en pointe"),
            (Lang::En, "Glossary", "Pointed sewn fold"),
        ] {
            let mut h = harness(lang);
            h.state_mut().show_glossary = true;
            h.run_steps(3);
            h.get_by_label(title);
            h.state_mut().glossary_filter = "dart".into();
            h.run_steps(3);
            h.get_by_label_contains(def);
            h.get_by_label_contains("pince / dart");
            assert!(h.query_by_label_contains("ourlet / hem").is_none());
            h.state_mut().glossary_filter = "zzz".into();
            h.run_steps(3);
            h.get_by_label(&tr("glossary.empty"));
        }
    }
}

//! US-16.5 (tranche 132) : unités d'affichage dans les fenêtres secondaires. Les champs de
//! longueur de l'interface suivent l'unité choisie (saisie, préremplissage, libellés) ; le modèle
//! reste en mm exacts. Fils d'exécution : chaque test règle l'unité de son propre fil.
//!
//! Module enfant de `ui_tests` : réutilise ses aides (harnais, saisie dans un champ).

use super::*;
use pf_core::units::{
    convert_length_text, display_units, field_text, len_number, parse_length_with,
    set_display_units, DisplayUnits, LengthUnit, MM_PER_INCH,
};

/// Remet l'unité (et le repère « (mm) » des libellés) à mm en sortie de test.
struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        crate::views::publish_units(DisplayUnits::default());
        pf_i18n::set_lang(pf_i18n::Lang::Fr);
    }
}

fn inches(denominator: u32) -> DisplayUnits {
    DisplayUnits {
        unit: LengthUnit::In,
        inch_denominator: denominator,
    }
}

fn units_of(unit: LengthUnit) -> DisplayUnits {
    DisplayUnits {
        unit,
        inch_denominator: 16,
    }
}

fn set_units(h: &mut Harness<'_, App>, units: DisplayUnits) {
    h.state_mut().view_settings.units = units;
    h.run_steps(3);
}

fn near(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-9, "{a} != {b}");
}

// ===== fenêtres : un test egui_kittest par fenêtre, valeurs à la main =========================

#[test]
fn marges_de_couture_en_pouces_saisie_affichage_et_mm_exacts() {
    let _reset = Reset;
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    set_units(&mut h, inches(16));
    h.get_by_label("Marge de couture").click();
    h.run_steps(3);
    // Le libellé suit l'unité ; le champ vide affiche 0 (pas « 0,0 »).
    h.get_by_label("Par défaut (in)");
    assert!(h.query_by_label("Par défaut (mm)").is_none());
    // 3/8 po = 9,525 mm (valeur à la main), une seule commande, mm exacts.
    type_in_field_valued(&mut h, "0", 0, "3/8");
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    near(piece_of(&h).seam.expect("marge").default_mm, 9.525);
    // Le champ réaffiche la fraction exacte ; le périmètre du contour de coupe (396,2 mm =
    // 15,598 po) est arrondi à 1/16 et signalé par ≈.
    assert!(
        h.get_all_by_role(egui::accesskit::Role::TextInput)
            .any(|f| f.value().as_deref() == Some("3/8")),
        "préremplissage exact"
    );
    h.get_by_label_contains("≈ 15 5/8\"");
    // 1 po = 25,4 mm.
    type_in_field_valued(&mut h, "3/8", 0, "1");
    near(piece_of(&h).seam.expect("marge").default_mm, 25.4);
    // Une saisie illisible est refusée sans trace.
    let before = h.state().editor.project().clone();
    type_in_field_valued(&mut h, "1", 0, "3/0");
    assert!(h.state().editor.status.error);
    assert_eq!(*h.state().editor.project(), before);
    // Retour en mm : la valeur exacte 25,4 s'affiche telle quelle, rien n'a bougé.
    set_units(&mut h, DisplayUnits::default());
    h.get_by_label("Par défaut (mm)");
    near(piece_of(&h).seam.expect("marge").default_mm, 25.4);
}

#[test]
fn mise_en_page_en_pouces_apercu_saisie_et_mm_exacts() {
    let _reset = Reset;
    let mut h = piece_harness();
    set_units(&mut h, inches(16));
    open_print_window(&mut h);
    // A4 = 210 × 297 mm : les pouces ne sont pas exacts, d'où ≈ ; marges de 10 mm.
    h.get_by_label_contains("Page : ≈ 8 1/4 × ≈ 11 11/16\"");
    h.get_by_label("Marge haute (in)");
    // Appliquer sans rien toucher ne change rien (aucune dérive des champs préremplis).
    print_apply(&mut h);
    assert!(
        h.state().page_setup.is_default(),
        "{:?}",
        h.state().page_setup
    );
    open_print_window(&mut h);
    type_in_field_after_label(&mut h, "Marge haute (in)", "3/8");
    type_in_field_after_label(&mut h, "Marge gauche (in)", "1/2");
    print_apply(&mut h);
    let m = h.state().page_setup.margins;
    near(m.top_mm, 9.525);
    near(m.left_mm, 12.7);
    near(m.bottom_mm, 10.0);
    near(m.right_mm, 10.0);
}

#[test]
fn transformer_en_pouces_le_champ_et_le_deplacement_sont_coherents() {
    let _reset = Reset;
    let mut h = piece_harness();
    set_units(&mut h, inches(8));
    h.state_mut().editor.selection.entities = h
        .state()
        .editor
        .sheet()
        .entities
        .iter()
        .map(|e| e.id)
        .collect();
    h.state_mut().open_transform(false);
    h.run_steps(3);
    type_in_field_after_label(&mut h, "Déplacement X", "1 1/2");
    near(h.state().transform.as_ref().expect("fenêtre").dx, 38.1);
    // Le champ réaffiche la valeur en pouces, pas 38,1.
    assert!(
        h.get_all_by_role(egui::accesskit::Role::TextInput)
            .any(|f| f.value().as_deref() == Some("1 1/2")),
        "valeur affichée = valeur saisie"
    );
    type_in_field_after_label(&mut h, "Déplacement Y", "2,54 cm");
    near(h.state().transform.as_ref().expect("fenêtre").dy, 25.4);
}

#[test]
fn construction_rapide_en_pouces_defauts_convertis_sans_derive() {
    let _reset = Reset;
    let mut h = quick_harness();
    let before = h.state().editor.project().clone();
    // Défauts en mm (100 × 60, rayon 5) : changer d'unité les réécrit exactement.
    set_units(&mut h, inches(16));
    {
        let ui = &h.state().editor.quick;
        assert_eq!(ui.width, "100,0 mm");
        assert_eq!(ui.height, "60,0 mm");
        assert_eq!(ui.radius, "5,0 mm");
        assert_eq!(ui.x, "0");
    }
    // Rectangle arrondi 4 × 2 1/2 po, rayon 1/4 po, coin à (1 ; 1) po.
    {
        let ui = &mut h.state_mut().editor.quick;
        ui.kind = crate::quick::QuickKind::Rounded;
        (ui.x, ui.y, ui.width, ui.height, ui.radius) = (
            "1".into(),
            "1".into(),
            "4".into(),
            "2 1/2".into(),
            "1/4".into(),
        );
    }
    h.run_steps(3);
    h.get_by_label_contains("Dimensions : 4\" × 2 1/2\"");
    assert_eq!(
        *h.state().editor.project(),
        before,
        "l'aperçu ne change rien"
    );
    h.get_by_label("Créer").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let sheet = h.state().editor.sheet().clone();
    let xs: Vec<f64> = sheet
        .points
        .iter()
        .map(|p| sheet.position(p.id).unwrap().x)
        .collect();
    let (min_x, max_x) = xs
        .iter()
        .fold((f64::MAX, f64::MIN), |a, x| (a.0.min(*x), a.1.max(*x)));
    near(min_x, 25.4);
    near(max_x, 25.4 + 101.6);
    // Une seule annulation défait tout.
    undo(&mut h);
    assert_eq!(*h.state().editor.project(), before);
}

#[test]
fn plis_en_pouces_profondeur_saisie_et_largeur_ajoutee_exactes() {
    let _reset = Reset;
    let mut h = piece_harness();
    set_units(&mut h, inches(16));
    open_pleats_with_line(&mut h, pt(50.0, -10.0), pt(50.0, 70.0));
    // Profondeur 1/4 po = 6,35 mm : pli plat = 2 × 6,35 = 12,7 mm de largeur ajoutée.
    h.state_mut().editor.pleat.depth = "1/4".into();
    h.run_steps(2);
    h.get_by_label("Poser les plis").click();
    h.run_steps(3);
    assert!(
        !h.state().editor.status.error,
        "{}",
        h.state().editor.status.text
    );
    let pleats = pleats_of(&h);
    assert_eq!(pleats.len(), 1);
    near(pleats[0].width_mm(), 12.7);
    // Un texte en mm explicite reste lisible dans une unité quelconque.
    undo(&mut h);
    open_pleats_with_line(&mut h, pt(50.0, -10.0), pt(50.0, 70.0));
    h.state_mut().editor.pleat.depth = "7 mm".into();
    h.run_steps(2);
    h.get_by_label("Poser les plis").click();
    h.run_steps(3);
    near(pleats_of(&h)[0].width_mm(), 14.0);
}

#[test]
fn libelles_et_gabarits_francais_et_anglais_suivent_l_unite() {
    let _reset = Reset;
    let mut h = piece_harness();
    h.set_size(egui::vec2(1280.0, 1600.0));
    set_units(&mut h, units_of(LengthUnit::Cm));
    h.get_by_label("Marge de couture").click();
    h.run_steps(3);
    // FR : libellé en cm, valeur saisie en cm = mm × 10.
    h.get_by_label("Par défaut (cm)");
    type_in_field_valued(&mut h, "0,0", 0, "1,25");
    near(piece_of(&h).seam.expect("marge").default_mm, 12.5);
    h.get_by_label_contains("périmètre 42,0 cm");
    // EN : même libellé traduit, même valeur, gabarit « cm ».
    pf_i18n::set_lang(pf_i18n::Lang::En);
    h.run_steps(3);
    h.get_by_label("Default (cm)");
    h.get_by_label_contains("perimeter 42");
    set_units(&mut h, inches(16));
    h.get_by_label("Default (in)");
    h.get_by_label_contains("≈ 16 9/16\"");
    pf_i18n::set_lang(pf_i18n::Lang::Fr);
    h.run_steps(2);
}

// ===== modèle et exports inchangés =============================================================

#[test]
fn le_choix_de_l_unite_ne_modifie_ni_le_projet_ni_les_exports() {
    let _reset = Reset;
    let mut h = piece_harness();
    let project = h.state().editor.project().clone();
    let sheet = h.state().editor.sheet;
    let scene = |h: &Harness<'_, App>| {
        pf_export::Scene::from_sheet(h.state().editor.project(), sheet).expect("scène")
    };
    let svg_mm = pf_export::svg::export_svg(&scene(&h)).expect("svg");
    let pdf_mm = pf_export::pdf::export_pdf(&[scene(&h)]).expect("pdf").bytes;
    let json_mm = serde_json::to_string(&project).expect("json");
    for units in [
        units_of(LengthUnit::Cm),
        inches(64),
        DisplayUnits::default(),
    ] {
        set_units(&mut h, units);
        assert_eq!(*h.state().editor.project(), project);
        let svg = pf_export::svg::export_svg(&scene(&h)).expect("svg");
        assert_eq!(svg, svg_mm, "SVG identique en {units:?}");
        let pdf = pf_export::pdf::export_pdf(&[scene(&h)]).expect("pdf").bytes;
        assert_eq!(pdf, pdf_mm, "PDF identique en {units:?}");
        assert_eq!(
            serde_json::to_string(h.state().editor.project()).unwrap(),
            json_mm
        );
    }
}

// ===== campagne adverse à graine fixe ==========================================================

/// Générateur congruentiel (graine fixe) : aucune dépendance, séquence reproductible.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Oracle indépendant de `units` : texte et valeur attendue en mm d'une saisie fabriquée par
/// construction (entiers, jamais relue par le code testé).
fn fabricate(rng: &mut Lcg) -> (String, f64) {
    let whole = rng.below(40);
    let den = [2u64, 4, 8, 16, 32, 64][rng.below(6) as usize];
    let num = rng.below(den);
    let frac = whole as f64 + num as f64 / den as f64;
    match rng.below(7) {
        0 => (format!("{whole} {num}/{den} in"), frac * 25.4),
        1 => (format!("{whole}-{num}/{den}\""), frac * 25.4),
        2 => (format!("{num}/{den}\""), num as f64 / den as f64 * 25.4),
        3 => {
            let tenths = rng.below(2000);
            (format!("{},{} cm", tenths / 10, tenths % 10), tenths as f64)
        }
        4 => {
            let hundredths = rng.below(30000);
            (
                format!("{},{:02} mm", hundredths / 100, hundredths % 100),
                hundredths as f64 / 100.0,
            )
        }
        5 => (format!("{whole} in"), whole as f64 * 25.4),
        _ => (format!("{whole} po"), whole as f64 * 25.4),
    }
}

#[test]
fn campagne_adverse_unites_150_sequences_a_graine_fixe() {
    let _reset = Reset;
    let mut rng = Lcg(0x5EED_1650_0132);
    let (mut accepted, mut refused, mut roundtrips, mut fractions, mut approximated) =
        (0u32, 0u32, 0u32, 0u32, 0u32);
    let invalid = [
        "",
        " ",
        "abc",
        "1/0",
        "3/8 cm",
        "1/2 mm",
        "nan",
        "inf",
        "1e3",
        "--2",
        "1//2",
        "1 in 1/2",
        "12 pouces",
    ];
    for seq in 0..180u32 {
        let unit = [LengthUnit::Mm, LengthUnit::Cm, LengthUnit::In][rng.below(3) as usize];
        let den = [2u32, 4, 8, 16, 32, 64][rng.below(6) as usize];
        let units = DisplayUnits {
            unit,
            inch_denominator: den,
        };
        set_display_units(units);
        match rng.below(3) {
            // 1. Saisie fabriquée : la valeur relue = l'attente de l'oracle, à 1e-9 près.
            0 => {
                let (text, want) = fabricate(&mut rng);
                let got = parse_length_with(&text, unit).unwrap_or_else(|e| {
                    panic!("séquence {seq} : « {text} » refusé : {e}");
                });
                assert!(
                    (got - want).abs() <= 1e-9,
                    "séquence {seq} : {text} -> {got} != {want}"
                );
                accepted += 1;
                if text.contains('/') {
                    fractions += 1;
                }
            }
            // 2. Saisie invalide : refusée dans toute unité, jamais prise pour un nombre.
            1 => {
                let text = invalid[rng.below(invalid.len() as u64) as usize];
                assert!(
                    parse_length_with(text, unit).is_err(),
                    "séquence {seq} : « {text} » aurait dû être refusé en {unit:?}"
                );
                refused += 1;
            }
            // 3. Aller-retour mm -> texte du champ -> mm, sans dérive, puis changement d'unité
            //    en chaîne (mm -> unité -> autre unité -> mm) : mêmes mm.
            _ => {
                let mm = (rng.below(3_000_000) as f64) / 1000.0 - 500.0;
                let text = field_text(mm);
                let back = parse_length_with(&text, unit).expect("champ relu");
                assert!(
                    (back - mm).abs() <= 1e-9 * mm.abs().max(1.0),
                    "séquence {seq} : {mm} -> « {text} » -> {back} ({unit:?})"
                );
                let other = units_of(
                    [LengthUnit::Mm, LengthUnit::Cm, LengthUnit::In][rng.below(3) as usize],
                );
                let moved = convert_length_text(&text, units, other);
                let again = convert_length_text(&moved, other, DisplayUnits::default());
                let end = parse_length_with(&again, LengthUnit::Mm).expect("relu en mm");
                assert!(
                    (end - mm).abs() <= 1e-9 * mm.abs().max(1.0),
                    "séquence {seq} : {mm} -> {moved} -> {again} -> {end}"
                );
                roundtrips += 1;
                if unit == LengthUnit::In && len_number(mm, 2).starts_with('≈') {
                    approximated += 1;
                }
                // L'affichage arrondi n'est jamais réinjecté : 25,4 mm = 1 po exactement.
                if (mm - 25.4).abs() < 1e-12 {
                    assert_eq!(
                        len_number(mm, 2),
                        if unit == LengthUnit::In { "1" } else { "2.54" }
                    );
                }
            }
        }
    }
    set_display_units(DisplayUnits::default());
    assert!(accepted >= 40, "saisies valides : {accepted}");
    assert!(refused >= 40, "saisies refusées : {refused}");
    assert!(roundtrips >= 40, "allers-retours : {roundtrips}");
    assert!(fractions >= 10, "fractions : {fractions}");
    assert!(
        approximated >= 5,
        "affichages arrondis signalés par ≈ : {approximated}"
    );
    assert_eq!(accepted + refused + roundtrips, 180);
}

#[test]
fn campagne_adverse_fenetre_construction_rapide_40_sequences_ui() {
    let _reset = Reset;
    let mut rng = Lcg(0xFEED_0132);
    let mut h = quick_harness();
    let before = h.state().editor.project().clone();
    let (mut created, mut refused) = (0u32, 0u32);
    for seq in 0..40u32 {
        let unit = [LengthUnit::Mm, LengthUnit::Cm, LengthUnit::In][rng.below(3) as usize];
        set_units(&mut h, units_of(unit));
        // Largeur et hauteur fabriquées (≥ 1 mm), ou une saisie invalide.
        let (wt, ww) = loop {
            let (t, v) = fabricate(&mut rng);
            if v >= 5.0 {
                break (t, v);
            }
        };
        let (ht, hh) = loop {
            let (t, v) = fabricate(&mut rng);
            if v >= 5.0 {
                break (t, v);
            }
        };
        let broken = rng.below(5) == 0;
        {
            let ui = &mut h.state_mut().editor.quick;
            ui.kind = crate::quick::QuickKind::Rectangle;
            (ui.y, ui.angle, ui.centered) = ("0".into(), "0".into(), false);
            // Une saisie sans unité explicite se lit dans l'unité du champ : on l'écrit telle quelle
            // (les textes fabriqués portent leur unité, sauf les fractions seules en pouces).
            ui.x = format!("{} mm", seq * 600);
            ui.width = if broken { "3/0".into() } else { wt.clone() };
            ui.height = ht.clone();
        }
        h.run_steps(2);
        let n_before = h.state().editor.sheet().entities.len();
        h.get_by_label("Créer").click();
        h.run_steps(3);
        if broken {
            // Saisie illisible : le bouton reste sans effet, rien n'est créé.
            assert_eq!(h.state().editor.sheet().entities.len(), n_before);
            refused += 1;
            continue;
        }
        // Une fraction seule (« 3/8" » n'a pas d'unité manquante, « n/d » sans guillemet non) :
        // les textes fabriqués ont toujours leur unité, donc indépendants de l'unité du champ.
        assert!(
            !h.state().editor.status.error,
            "séquence {seq} : {wt} × {ht} en {unit:?} : {}",
            h.state().editor.status.text
        );
        let sheet = h.state().editor.sheet().clone();
        let xs: Vec<f64> = sheet
            .points
            .iter()
            .map(|p| sheet.position(p.id).unwrap().x)
            .collect();
        let ys: Vec<f64> = sheet
            .points
            .iter()
            .map(|p| sheet.position(p.id).unwrap().y)
            .collect();
        let span = |v: &[f64]| {
            v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
        };
        // Le plan contient tous les rectangles déjà créés à l'origine : on prend le dernier.
        let _ = (span(&xs), span(&ys));
        let last = sheet.entities.len();
        assert_eq!(last, n_before + 4, "séquence {seq}");
        let ends: Vec<(f64, f64)> = sheet
            .entities
            .iter()
            .rev()
            .take(4)
            .map(|e| {
                let (a, b) = sheet.segment_ends(e.id).unwrap();
                ((b.x - a.x).abs(), (b.y - a.y).abs())
            })
            .collect();
        let widths: Vec<f64> = ends.iter().map(|e| e.0).filter(|w| *w > 1e-9).collect();
        let heights: Vec<f64> = ends.iter().map(|e| e.1).filter(|w| *w > 1e-9).collect();
        assert!(
            widths.iter().all(|w| (w - ww).abs() <= 1e-9),
            "séquence {seq} : largeur {wt} en {unit:?} attendue {ww}, lue {widths:?}"
        );
        assert!(
            heights.iter().all(|v| (v - hh).abs() <= 1e-9),
            "séquence {seq} : hauteur {ht} en {unit:?} attendue {hh}, lue {heights:?}"
        );
        created += 1;
    }
    assert!(created >= 25, "créations : {created}");
    assert!(refused >= 3, "refus : {refused}");
    assert_eq!(created + refused, 40);
    let _ = before;
}

#[test]
fn les_unites_par_defaut_et_la_largeur_en_pouces_des_cellules() {
    // 25,4 mm = 1 po, 3/8 po = 9,525 mm : les valeurs à la main de la tranche.
    near(parse_length_with("1", LengthUnit::In).unwrap(), 25.4);
    near(parse_length_with("3/8", LengthUnit::In).unwrap(), 9.525);
    assert_eq!(MM_PER_INCH, 25.4);
    set_display_units(inches(16));
    assert_eq!(len_number(25.4, 2), "1");
    assert_eq!(len_number(9.525, 2), "3/8");
    assert_eq!(len_number(10.0, 2), "≈ 3/8");
    assert_eq!(field_text(25.4), "1");
    assert_eq!(
        field_text(10.0),
        "10,0 mm",
        "non exact en 1/16 : mm explicite"
    );
    assert_eq!(display_units().unit, LengthUnit::In);
    set_display_units(DisplayUnits::default());
    assert_eq!(field_text(10.0), "10,0");
}

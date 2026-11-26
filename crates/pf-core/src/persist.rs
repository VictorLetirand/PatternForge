//! Sauvegarde et ouverture des projets `.pforge.json`.
//!
//! - Ouverture : format et version contrôlés avant toute interprétation, puis
//!   validation complète (références, identifiants, géométries). Un fichier
//!   refusé n'est jamais modifié.
//! - Écriture : le projet est validé, écrit dans un fichier temporaire voisin,
//!   synchronisé sur disque, puis renommé à la place de la cible. Un échec en
//!   cours de route laisse le fichier d'origine intact.

use crate::contour;
use crate::error::CoreError;
use crate::model::{
    PathRole, Piece, PieceEdge, PieceId, PieceLink, Project, SheetId, FORMAT_TAG,
    MIN_SCHEMA_VERSION, SCHEMA_VERSION,
};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Taille maximale acceptée à l'ouverture (64 Mo).
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

pub fn to_json(project: &Project) -> Result<String, CoreError> {
    project.validate()?;
    serde_json::to_string_pretty(project)
        .map(|s| s + "\n")
        .map_err(|e| CoreError::Invalid(format!("sérialisation impossible : {e}")))
}

/// Refuse un JSON syntaxiquement faux ou qui répète une clé dans un même objet (D-02 : `serde_json`
/// garde sinon la dernière valeur en silence). L'erreur nomme la clé et donne ligne et colonne.
/// Les clés qui ne diffèrent que par la casse sont distinctes.
pub fn reject_duplicate_keys(text: &str) -> Result<(), serde_json::Error> {
    use serde::de::{DeserializeSeed, Deserializer, Error, MapAccess, SeqAccess, Visitor};
    use std::collections::HashSet;
    use std::fmt;

    struct Unique;
    impl<'de> DeserializeSeed<'de> for Unique {
        type Value = ();
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
            d.deserialize_any(self)
        }
    }
    impl<'de> Visitor<'de> for Unique {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("une valeur JSON")
        }
        fn visit_bool<E: Error>(self, _: bool) -> Result<(), E> {
            Ok(())
        }
        fn visit_i64<E: Error>(self, _: i64) -> Result<(), E> {
            Ok(())
        }
        fn visit_u64<E: Error>(self, _: u64) -> Result<(), E> {
            Ok(())
        }
        fn visit_f64<E: Error>(self, _: f64) -> Result<(), E> {
            Ok(())
        }
        fn visit_str<E: Error>(self, _: &str) -> Result<(), E> {
            Ok(())
        }
        fn visit_unit<E: Error>(self) -> Result<(), E> {
            Ok(())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
            while seq.next_element_seed(Unique)?.is_some() {}
            Ok(())
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
            let mut seen = HashSet::new();
            while let Some(key) = map.next_key::<String>()? {
                if !seen.insert(key.clone()) {
                    return Err(A::Error::custom(format!("clé « {key} » dupliquée")));
                }
                map.next_value_seed(Unique)?;
            }
            Ok(())
        }
    }

    let mut de = serde_json::Deserializer::from_str(text);
    Unique.deserialize(&mut de)?;
    de.end()
}

pub fn from_json(text: &str) -> Result<Project, CoreError> {
    reject_duplicate_keys(text)
        .map_err(|e| CoreError::Malformed(format!("JSON invalide ({e})")))?;
    let value: Value = serde_json::from_str(text)
        .map_err(|e| CoreError::Malformed(format!("JSON invalide ({e})")))?;
    let obj = value.as_object().ok_or(CoreError::NotAPatternForgeFile)?;
    match obj.get("format").and_then(Value::as_str) {
        Some(FORMAT_TAG) => {}
        _ => return Err(CoreError::NotAPatternForgeFile),
    }
    let version = obj
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            CoreError::Malformed("champ « schema_version » absent ou invalide".into())
        })?;
    if version < u64::from(MIN_SCHEMA_VERSION) || version > u64::from(SCHEMA_VERSION) {
        return Err(CoreError::UnsupportedVersion {
            found: u32::try_from(version).unwrap_or(u32::MAX),
            supported: SCHEMA_VERSION,
        });
    }
    let mut value = value;
    let legacy = migrate(&mut value, version as u32)?;
    let mut project: Project =
        serde_json::from_value(value).map_err(|e| CoreError::Malformed(e.to_string()))?;
    upgrade_pieces(&mut project, legacy);
    // Rotation de placement ramenée dans [0 ; 360[ (valeurs hors bornes : refusées
    // par la validation).
    for piece in &mut project.pieces {
        if let Some(n) = piece.placement.and_then(|p| p.normalized().ok()) {
            piece.placement = Some(n);
        }
    }
    project.validate()?;
    Ok(project)
}

/// Pièce-étiquette du schéma v1/v2 (`Sheet.piece`), lue dans le JSON brut
/// uniquement : le modèle courant n'a plus ce champ.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyPiece {
    name: String,
    quantity: u32,
    #[serde(default)]
    material: String,
}

/// Met à niveau le JSON brut d'une ancienne version vers la version courante
/// et renvoie les pièces-étiquettes à convertir (feuille, fiche).
///
/// - v1 → v2 : la v2 ajoute des définitions de points (proportion,
///   intersection, point sur courbe) et des champs optionnels ; un document v1
///   est déjà un document v2 valide, seul le numéro change.
/// - v2 → v3 : `Sheet.piece` est retiré du JSON ; les pièces sont recréées
///   comme entités de projet par [`upgrade_pieces`] (il faut le modèle typé
///   pour détecter les contours).
/// - v3 → v4 : le projet reçoit `sizes` (gammes de tailles, US-09.1), absent = aucune ; un
///   document v3 est déjà un document v4 valide, seul le numéro change. Une clé `sizes` dans un
///   fichier antérieur à la v4 est refusée (clé réservée : la lire changerait son sens en
///   silence) ; de même la clé `gradings` d'une feuille (écarts de gradation, US-09.2a, ajout
///   optionnel de la v4 : omise quand vide, donc un projet sans gradation est inchangé).
/// - v4 → v5 (décision 334) : seul le numéro change ; les objets ajoutés (règles de gradation
///   nommées, barème, fiches client, retouches, mesures finies, valeurs de variable par taille,
///   cran proportionnel) sont tous optionnels et omis quand vides. Un fichier antérieur à la v5
///   qui en porte un est refusé (clé réservée, rien n'est jeté en silence).
/// - v5 → v6 (décision 455) : les entrées de variante reçoivent un identifiant stable, pris dans
///   le compteur du projet (`next_id`) dans l'ordre du fichier (variantes puis entrées) ; le
///   compteur avance d'autant. Les autres ajouts (image de fond d'une feuille, peau d'un plan de
///   coupe, fiche client d'origine d'une taille) sont optionnels et omis quand vides : un fichier
///   antérieur à la v6 qui en porte un est refusé (clé réservée, voir [`reserved_v6_key`]).
fn migrate(value: &mut Value, from: u32) -> Result<Vec<(SheetId, LegacyPiece)>, CoreError> {
    let mut legacy = Vec::new();
    let Some(obj) = value.as_object_mut() else {
        return Ok(legacy);
    };
    if from < 3 {
        if let Some(sheets) = obj.get_mut("sheets").and_then(Value::as_array_mut) {
            for sheet in sheets {
                let Some(sheet) = sheet.as_object_mut() else {
                    continue;
                };
                let Some(old) = sheet.remove("piece") else {
                    continue;
                };
                let id = sheet
                    .get("id")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| CoreError::Malformed("feuille sans identifiant".into()))?;
                let piece: LegacyPiece = serde_json::from_value(old)
                    .map_err(|e| CoreError::Malformed(format!("pièce de la feuille {id} : {e}")))?;
                legacy.push((SheetId(id), piece));
            }
        }
    }
    if from < 4 && obj.contains_key("sizes") {
        return Err(CoreError::Malformed(format!(
            "la clé « sizes » est réservée au format version 4 (ce fichier est en version {from})"
        )));
    }
    if from < 4 {
        let reserved = obj
            .get("sheets")
            .and_then(Value::as_array)
            .is_some_and(|sheets| {
                sheets
                    .iter()
                    .any(|s| s.as_object().is_some_and(|o| o.contains_key("gradings")))
            });
        if reserved {
            return Err(CoreError::Malformed(format!(
                "la clé « gradings » d'une feuille est réservée au format version 4 (ce fichier est en version {from})"
            )));
        }
    }
    if from < 5 {
        if let Some(key) = reserved_v5_key(obj) {
            return Err(CoreError::Malformed(format!(
                "la clé « {key} » est réservée au format version 5 (ce fichier est en version {from})"
            )));
        }
    }
    if from < 6 {
        if let Some(key) = reserved_v6_key(obj) {
            return Err(CoreError::Malformed(format!(
                "la clé « {key} » est réservée au format version 6 (ce fichier est en version {from})"
            )));
        }
        give_variant_entry_ids(obj)?;
    }
    if from < SCHEMA_VERSION {
        obj.insert("schema_version".into(), Value::from(SCHEMA_VERSION));
    }
    Ok(legacy)
}

/// Première clé ajoutée par la v5 trouvée dans un fichier plus ancien (clés réservées : les
/// lire changerait leur sens en silence). Chemins : projet, `variables[].per_size`,
/// `sheets[].notches[].proportional`, `pieces[].stitch_lines`, `pieces[].zones` ; `cutting_plans` (projet).
fn reserved_v5_key(obj: &serde_json::Map<String, Value>) -> Option<String> {
    const PROJECT: [&str; 9] = [
        "stitch_pairs",
        "cutting_plans",
        "variants",
        "naming_rules",
        "grading_rules",
        "measurements",
        "clients",
        "alterations",
        "finished_measures",
    ];
    if let Some(k) = PROJECT.iter().find(|k| obj.contains_key(**k)) {
        return Some((*k).to_string());
    }
    let items = |key: &str| {
        obj.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
    };
    if items("pieces").any(|v| v.contains_key("stitch_lines")) {
        return Some("stitch_lines".into());
    }
    if items("pieces").any(|v| v.contains_key("zones")) {
        return Some("zones".into());
    }
    if items("variables").any(|v| v.contains_key("per_size")) {
        return Some("per_size".into());
    }
    let proportional = items("sheets").any(|s| {
        s.get("notches")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .any(|n| n.contains_key("proportional"))
    });
    proportional.then(|| "proportional".into())
}

/// Première clé ajoutée par la v6 trouvée dans un fichier plus ancien (clés réservées : les lire
/// changerait leur sens en silence). Chemins : `sheets[].background`, `cutting_plans[].hide`,
/// `sizes[].sizes[].client`, `variants[].entries[].id`.
pub fn reserved_v6_key(obj: &serde_json::Map<String, Value>) -> Option<String> {
    fn objects(v: Option<&Value>) -> Vec<&serde_json::Map<String, Value>> {
        v.and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .collect()
    }
    if objects(obj.get("sheets"))
        .iter()
        .any(|s| s.contains_key("background"))
    {
        return Some("background".into());
    }
    if objects(obj.get("cutting_plans"))
        .iter()
        .any(|p| p.contains_key("hide"))
    {
        return Some("hide".into());
    }
    if objects(obj.get("sizes"))
        .iter()
        .flat_map(|r| objects(r.get("sizes")))
        .any(|s| s.contains_key("client"))
    {
        return Some("client".into());
    }
    objects(obj.get("variants"))
        .iter()
        .flat_map(|v| objects(v.get("entries")))
        .any(|e| e.contains_key("id"))
        .then(|| "id (entrée de variante)".into())
}

/// v5 → v6 : donne un identifiant à chaque entrée de variante, dans l'ordre du fichier, à partir
/// de `next_id` (le compteur avance d'autant). Sans variante, rien ne change.
fn give_variant_entry_ids(obj: &mut serde_json::Map<String, Value>) -> Result<(), CoreError> {
    let Some(mut next) = obj.get("next_id").and_then(Value::as_u64) else {
        return Ok(()); // compteur illisible : la lecture typée le refusera
    };
    let mut given = false;
    if let Some(variants) = obj.get_mut("variants").and_then(Value::as_array_mut) {
        for v in variants {
            let Some(entries) = v
                .as_object_mut()
                .and_then(|v| v.get_mut("entries"))
                .and_then(Value::as_array_mut)
            else {
                continue;
            };
            for e in entries.iter_mut().filter_map(Value::as_object_mut) {
                e.insert("id".into(), Value::from(next));
                next = next.checked_add(1).ok_or_else(|| {
                    CoreError::Malformed("compteur d'identifiants au maximum".into())
                })?;
                given = true;
            }
        }
    }
    if given {
        obj.insert("next_id".into(), Value::from(next));
    }
    Ok(())
}

/// Crée une pièce liée par pièce-étiquette migrée. Le contour est rempli si la
/// feuille a exactement un contour fermé de coupe ; sinon il reste vide et une
/// remarque est conservée dans `migration_notes`.
fn upgrade_pieces(project: &mut Project, legacy: Vec<(SheetId, LegacyPiece)>) {
    for (sheet_id, old) in legacy {
        let (outline, grainline, note) = match project.sheet(sheet_id) {
            Ok(sheet) => {
                let cutting: Vec<_> = contour::closed_contours(sheet)
                    .into_iter()
                    .filter(|c| c.role == Some(PathRole::CuttingContour))
                    .collect();
                if let [one] = cutting.as_slice() {
                    let grains: Vec<_> = sheet
                        .entities
                        .iter()
                        .filter(|e| e.role == PathRole::Grainline)
                        .map(|e| e.id)
                        .collect();
                    let outline = one
                        .edges
                        .iter()
                        .map(|e| PieceEdge {
                            entity: e.entity,
                            reversed: e.reversed,
                        })
                        .collect();
                    (outline, (grains.len() == 1).then(|| grains[0]), None)
                } else {
                    let why = if cutting.is_empty() {
                        "aucun contour de coupe fermé".to_string()
                    } else {
                        format!(
                            "{} contours de coupe fermés (un seul attendu)",
                            cutting.len()
                        )
                    };
                    (
                        Vec::new(),
                        None,
                        Some(format!(
                            "Feuille « {} » : la pièce « {} » est migrée sans contour ({why}).",
                            sheet.name, old.name
                        )),
                    )
                }
            }
            Err(_) => continue, // feuille absente : la validation signalera le reste
        };
        let id = PieceId(project.alloc_id());
        project.pieces.push(Piece {
            id,
            name: old.name,
            quantity: old.quantity,
            material: Some(old.material).filter(|m| !m.trim().is_empty()),
            outline,
            grainline,
            link: PieceLink::Linked { sheet: sheet_id },
            ..Piece::default()
        });
        project.migration_notes.extend(note);
    }
}

pub fn load(path: &Path) -> Result<Project, CoreError> {
    let meta =
        fs::metadata(path).map_err(|e| CoreError::Io(format!("{} : {e}", path.display())))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(CoreError::Malformed(
            "fichier trop volumineux pour un projet".into(),
        ));
    }
    let text =
        fs::read_to_string(path).map_err(|e| CoreError::Io(format!("{} : {e}", path.display())))?;
    from_json(text.trim_start_matches('\u{feff}'))
}

/// Comme `load`, et renvoie la révision (empreinte) des octets effectivement lus,
/// lus une seule fois : pas de course entre la lecture et l'empreinte.
pub fn load_with_revision(path: &Path) -> Result<(Project, String), CoreError> {
    let meta =
        fs::metadata(path).map_err(|e| CoreError::Io(format!("{} : {e}", path.display())))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(CoreError::Malformed(
            "fichier trop volumineux pour un projet".into(),
        ));
    }
    let bytes = fs::read(path).map_err(|e| CoreError::Io(format!("{} : {e}", path.display())))?;
    from_bytes(bytes, &path.display().to_string())
}

/// Analyse des octets d'un projet déjà lus (lecture par morceaux d'US-17.4, versions d'US-17.2) :
/// même validation que `load`, et la révision des octets. `origin` ne sert qu'aux messages.
pub fn from_bytes(bytes: Vec<u8>, origin: &str) -> Result<(Project, String), CoreError> {
    let revision = crate::file_guard::revision_of(&bytes);
    let text = String::from_utf8(bytes)
        .map_err(|_| CoreError::Io(format!("{origin} : stream did not contain valid UTF-8")))?;
    Ok((from_json(text.trim_start_matches('\u{feff}'))?, revision))
}

pub fn save(path: &Path, project: &Project) -> Result<(), CoreError> {
    write_atomic(path, &to_json(project)?)
}

/// Refuse d'écraser un fichier en lecture seule. Sans ce contrôle, le renommage d'un temporaire
/// l'écraserait sous Linux et macOS (seul le droit d'écrire dans le dossier y compte).
pub fn refuse_read_only(path: &Path) -> std::io::Result<()> {
    match fs::metadata(path) {
        Ok(m) if m.permissions().readonly() => Err(std::io::ErrorKind::PermissionDenied.into()),
        _ => Ok(()),
    }
}

/// Écrit `json` dans un temporaire voisin, le synchronise, puis le renomme sur
/// `path` : un échec ou un arrêt en cours de route laisse `path` intact.
pub(crate) fn write_atomic(path: &Path, json: &str) -> Result<(), CoreError> {
    let io = |e: std::io::Error| CoreError::Io(format!("{} : {e}", path.display()));
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| CoreError::Io("chemin invalide".into()))?
        .to_os_string();
    tmp_name.push(format!(".tmp{}", std::process::id()));
    let tmp = path.with_file_name(tmp_name);
    let write = || -> std::io::Result<()> {
        refuse_read_only(path)?;
        let mut f = fs::File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io(e)
    })
}

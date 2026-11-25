# ADR — stockage v5 : une seule montée de schéma (décision 334)

Statut : acceptée le 22/11/2026 (rapport de passation du 22/11, conseil n° 1 : une seule migration au lieu de v5, v6, v7).
Périmètre : **types, validation et format seulement**. Aucune commande, aucune interface : les objets sont posés mais pas
encore utilisables (stories 09.3b, 09.5b, 09.7, E10). Format : `FORMAT_PFORGE_V5.md` § 4.10, schéma `pforge-v5.schema.json`.

## Contexte
Le schéma 4 stocke les gammes (`Project.sizes`), les écarts (`Sheet.gradings`) et les variables (`name, unit, expr`).
Quatre stories à venir ont besoin de stockage : règles nommées (09.3b), cran proportionnel (09.5b), variables par taille
(09.7), barème / fiches / retouches / mesures finies (E10). Chaque montée coûte fixtures, migration, schéma et documents.

## Décision (a) — où et sous quelle forme
Tout est **optionnel, omis quand vide**, en objets `deny_unknown_fields`, ids du compteur du projet (stables, jamais réutilisés).
- `Project.grading_rules[]` : `{ id, name, mode?, deltas: [{ size, dx_mm, dy_mm }] }`. Règle **relative** (sans point) ;
  `mode` = copie (`mirror_x`, `mirror_y`, `invert` ; omis = identique, enum `GradingCopyMode` existante).
- `Variable.per_size[]` : `{ size, expr }` (formule ou nombre, texte comme `expr`) ; la variable garde `expr` = base.
  Une taille sans entrée garde `expr`. Les valeurs ne sont pas stockées (calculées à la lecture, comme en v4).
- `Notch.proportional` : booléen omis quand faux ; refusé avec `anchor` (un cran ancré suit déjà son bord).
- `Project.measurements[]` : `{ id, name, unit (mm|deg), values?: [{ size, value }] }` ; taille sans valeur = non renseignée.
- `Project.clients[]` : `{ id, name, values?: [{ measurement, value }] }` ; mesure absente = manquante, **jamais devinée**.
- `Project.alterations[]` : `{ id, name, target (mesure), delta }`.
- `Project.finished_measures[]` : `{ id, name, measurement, source, tolerance_minus, tolerance_plus }`, `source` = couture
  appariée (`seam`) ou cote complète (`full_dimension`), enum à étiquette interne sans variante unitaire (D-04 évité).
Écartés : conteneur générique `Project.v5`, valeurs par taille stockées en table (`map` : ordre de clés non naturel),
cran « mode d'ancrage » (un booléen suffit : l'ancrage existe déjà), retouche à plusieurs cibles (une retouche = une cible).
**Extensible sans nouvelle version** : on peut ajouter un champ optionnel à ces objets (omis quand absent).

## Décision (b) — unité et référence de taille
Une taille se désigne **toujours par son `SizeId`** (unique dans le projet, vérifié par `check_id`), jamais par son rang ni
son libellé : réordonner, renommer ou insérer une taille ne casse aucune référence. Plusieurs gammes : une entrée peut viser
une taille de n'importe quelle gamme (comme `gradings`). Valeurs en mm (degrés pour `deg`), étiquette sans conversion.

## Décision (c) — validation des références (refus au chargement, message nommant l'objet)
Taille, mesure, couture, cote inexistantes : `DeadReference`. Doublons refusés : id (projet), taille par règle / mesure /
variable, mesure par fiche, nom (casse ignorée). Nombres finis et bornés (±1 000 000 ; valeurs et tolérances ≥ 0).
`per_size` : pour chaque taille citée, **toute la table** substituée doit s'évaluer (syntaxe, références, cycles).
Un fichier déclaré **< 5** qui porte une de ces clés est refusé (« clé réservée au format version 5 »), comme `sizes` en v4.

## Décision (d) — sérialisation
Ordre = ordre des champs des structures (déterministe) : `sizes`, puis `grading_rules`, `measurements`, `clients`,
`alterations`, `finished_measures`, avant `migration_notes`. Sans objet v5, les octets sont ceux de la v4 **sauf** la ligne
`schema_version` (vérifié sur toutes les fixtures v4 et les `examples/`).

## Conséquences
- `SCHEMA_VERSION = 5`, lecture 1 à 5, écriture 5 ; version future (6) refusée. `examples/*.pforge.json` changent d'**une ligne**.
- Limites déclarées (testées, `schema_v5::ca8_*`) : renommer une variable citée par un `per_size`, ou retirer une taille citée
  par un objet v5, est **refusé sans trace** par la validation (jamais retiré ni réécrit en silence). Les commandes de
  09.3b / 09.7 / E10 devront retirer ou réécrire ces références dans la même étape (comme `gradings` pour `RemoveSize`).
  **Mis à jour (tranche 104, décision 335)** : pour `per_size` et `proportional`, c'est fait (renommage de variable et retrait de taille ou de gamme réécrivent / retirent les valeurs dans la même étape).
  **Mis à jour (tranche 105, décision 336)** : pour `measurements` / `clients`, c'est fait aussi (retrait d'une taille ou d'une gamme retire les valeurs du barème ; retrait d'une mesure retire ses valeurs de fiches, ses retouches et ses mesures finies, avec remarque). Reste refusé sans trace : rien d'autre pour ces deux objets ; les sources des mesures finies (couture, cote) restent à traiter avec US-10.4.
  **Mis à jour (tranche 106, décision 337)** : les sources des mesures finies sont traitées aussi (couture, cote ou feuille supprimée retire la mesure finie dans la même étape, avec remarque) ; les retouches et mesures finies ne citent aucune taille.

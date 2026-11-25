# ADR — stockage v6 : une seule montée de schéma groupée (décisions 455 à 459)

Statut : acceptée le 24/11/2026 (le propriétaire autorise les montées tant que le produit n'est pas utilisé ; on en fait **une**).
Précédent : `ADR_V5_STOCKAGE.md` (tranche 103, décision 334). Format : `FORMAT_PFORGE_V6.md` § 4.8, 4.12, 4.14, 4.15, schéma `pforge-v6.schema.json`.
Périmètre : **types, validation et format** pour les quatre manques ci-dessous ; commandes seulement pour (b) et (c) ; **aucune commande ni interface**
pour (d) et (e).

## Contexte
Quatre manques de modèle sont connus : la taille générée depuis une fiche client n'en garde pas la trace (décision 376), une entrée de variante n'a
d'identité que par (pièce, option) donc renommer ou réordonner une option casse son adresse, et deux stories à venir (E14 image de fond, E15 cuir) n'ont
nulle part où stocker leurs données. Chaque montée coûte fixtures, migration, schéma et documents : on les regroupe.

## Décision (a) — `SCHEMA_VERSION` 6
Lecture 1 à 6, écriture 6, version future (7) refusée. Tout est **optionnel et omis quand vide** (`deny_unknown_fields`, ids du compteur du projet),
sauf l'`id` d'entrée de variante, obligatoire (seule donnée dont l'absence changerait le sens). Fixtures v5 : `tests/fixtures_v5/`. Un fichier déclaré
< 6 qui porte une clé v6 est refusé (« clé réservée au format version 6 », `persist::reserved_v6_key`) : rien n'est jeté ni réinterprété en silence.

## Décision (b) — client d'origine d'une taille (456)
`Size.client: Option<ClientId>`, écrit par `GenerateClientSize` (remplacement compris), seule la **référence** est mémorisée (les valeurs restent
indépendantes). Intégrité référentielle, jamais d'état obsolète : fiche inexistante = fichier refusé ; supprimer une fiche citée = **refus qui nomme la
taille** et sa gamme (on ne retire pas la référence en silence). Retirer la taille ou la gamme retire la référence avec elle ; renommer une taille ou une
fiche ne touche rien (références par id). Les tailles existantes n'ont pas de client.

## Décision (c) — identifiant d'entrée de variante (457)
`VariantEntry.id: VariantEntryId`, du compteur du projet, jamais réutilisé. Migration : dans l'ordre du fichier (variantes puis entrées), `next_id` avance
d'autant (déterministe). Création par `AddVariantEntry` ; `DuplicateVariant` donne des ids neufs aux copies ; `UpdateVariantEntry` garde l'id. Les commandes
existantes restent adressées par (pièce, option), **sans changement observable**. Ajouts : `RenameVariantOption` (réécrit option, état actif et entrées ;
refus : vide, doublon casse ignorée, option inconnue) et `MoveVariantOption` (ordre de `options`), un pas d'annulation chacune.
Écarté : adresser les commandes par id d'entrée (changement d'API sans besoin), id d'option (une option reste un nom).

## Décision (d) — image de fond (458)
`Sheet.background` : chemin relatif sûr, SHA-256, dimensions en pixels, calibration (`two_points` / `four_points`), opacité, visible, verrouillé.
L'image n'est **jamais intégrée** au JSON. La validation contrôle la forme (elle ne lit pas l'image : une empreinte fausse mais bien formée passe).
Écarté : base64 dans le fichier (poids, 64 Mo), chemin absolu (non portable), `\` (un seul séparateur : `/`).

## Décision (e) — peau, défauts, zones de qualité (459)
`CuttingPlan.hide` : contour (polygone fermé implicitement, simple), défauts (id, genre, polygone, note), zones de qualité (id, `grade` 1 à 3, polygone).
Bornes : 500 défauts, 100 zones, 1000 sommets de contour, 200 par défaut ou zone, 20 000 en tout (la validation tourne après chaque commande : le coût est
borné). Un défaut hors du contour est refusé en le nommant. Aucun placement ne tient compte de la peau (hors périmètre).

## Conséquences
- `examples/*.pforge.json` (dont `aide/sac-cabas`) : **une ligne** change (`schema_version`) ; sorties PDF / SVG / DXF et rapports identiques à l'octet.
- Un fichier v5 sans variante relu puis réécrit ne change que la ligne `schema_version` ; avec variantes, `id` d'entrée et `next_id` en plus.
- Limites : une version précédente de l'application refuse un fichier v6 (non vérifié en pratique) ; l'ordre d'attribution des ids suit l'ordre du
  fichier, pas un tri.

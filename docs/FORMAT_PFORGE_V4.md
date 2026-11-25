# Format de fichier `.pforge.json`, version de schéma 4

> **Remplacé par la version 5 pour l'écriture** : voir [`FORMAT_PFORGE_V5.md`](FORMAT_PFORGE_V5.md) et
> [`pforge-v5.schema.json`](pforge-v5.schema.json). Ce document et son schéma restent la référence de la version précédente (fichiers
> v4, lus et migrés en mémoire par l'application) ; ils ne sont plus mis à jour.

Spécification du format de projet de PatternForge, **tel qu'implémenté à ce commit**. Cette version **remplace**
[`FORMAT_PFORGE_V3.md`](FORMAT_PFORGE_V3.md) pour l'écriture ; la v3 reste lisible (migrée en mémoire). Seule différence avec la v3 :
la clé `sizes` du projet (§ 4.8, gammes de tailles, US-09.1) et le numéro de version ; le reste est repris tel quel.

- **Elle n'est pas figée.** Le propriétaire a décidé de passer en version 4 avec la première story de gradation (décision 285) ;
  les évolutions suivantes de la gradation (écarts par point, US-09.2) peuvent encore modifier ce format. Rien dans ce document
  n'est une promesse de compatibilité future, ni une norme ou un standard.
- **Elle ne remplace pas le code.** En cas de doute, le chargeur (`pf_core::persist`) a raison et ce document est à corriger.
- Le **schéma JSON** [`pforge-v4.schema.json`](pforge-v4.schema.json) décrit la forme du fichier ; il **ne remplace pas le validateur**
  `pf-validate` (§ 8), seul à vérifier les références, les identifiants et les règles métier.
- Licence de la spécification : à décider par le propriétaire (point en attente, aucune licence n'est choisie ici).

## 1. Vue d'ensemble

Un projet est **un fichier JSON UTF-8** (un BOM initial est toléré en lecture), d'extension `.pforge.json`, de 64 Mo au plus.
L'application l'écrit indenté sur 2 espaces, avec un saut de ligne final, de façon atomique (fichier temporaire voisin puis
renommage). L'ordre des clés n'a pas de sens pour la lecture.

```json
{
  "format": "patternforge",
  "schema_version": 4,
  "name": "Pièce avec crans et droit-fil",
  "next_id": 30,
  "sheets": [ { "id": 1, "name": "Rabat", "points": [], "entities": [] } ],
  "pieces": []
}
```

| Champ | Type | Obligatoire | Sens |
|---|---|---|---|
| `format` | texte, toujours `"patternforge"` | oui | premier garde-fou : sinon « ce fichier n'est pas un projet PatternForge » |
| `schema_version` | entier, `4` | oui | voir § 6 |
| `name` | texte | oui | nom du projet (règles de nom, § 7) |
| `next_id` | entier ≥ 1 | oui | prochain identifiant libre ; les identifiants ne sont jamais réutilisés |
| `sheets` | liste de feuilles, au moins une | oui | § 4 |
| `pieces` | liste de pièces | non (omis si vide) | § 5 |
| `seam_defaults` | `{ "fabric_mm", "lining_mm", "interfacing_mm", "leather_mm", "reinforcement_mm" }` (nombres de 0 à 1000, tous obligatoires) | non (omis s'il vaut les valeurs d'atelier 10 / 10 / 5 / 6 / 5) | marge de couture proposée par catégorie de pièce (US-05.5) ; ne réécrit jamais une marge posée ; le chargeur refuse une clé inconnue, une valeur hors bornes et un jeu identique aux valeurs d'atelier |
| `seams` | liste de couples `{ "id", "a", "b", "embu_mm"?, "tolerance_mm"? }`, un côté = `{ "piece", "edges" }` (identifiant de pièce, tracés **consécutifs** de son contour, dans l'ordre, le départ pouvant faire le tour de la fermeture) ; `embu_mm` et `tolerance_mm` de 0 à 1000 | non (omis si vide) | coutures appariées (US-08.1) : **déclarations** ; longueurs, écart et embu sont calculés à la lecture, jamais écrits ; `a` = côté à embuer, tolérance absente = 1 mm ; le chargeur refuse une clé inconnue, une pièce ou un tracé inconnu, des tracés qui ne se suivent pas, un tracé cousu dans deux couples, une valeur hors bornes |
| `variables` | liste ordonnée de `{ "name", "unit", "expr" }` (§ 4.5), 500 au plus | non (omis si vide) | table de variables (US-03.1a) : nom, unité d'affichage (`mm`, `deg`, `none`) et **texte** de l'expression ; les valeurs sont calculées à la lecture, jamais écrites ; le chargeur refuse une clé inconnue, une unité inconnue, un nom invalide ou en double, une expression qui ne s'analyse ou ne s'évalue pas, un cycle, une valeur non finie |
| `sizes` | liste ordonnée de gammes `{ "id", "name", "kind", "sizes", "base" }` (§ 4.8), 20 au plus | non (omis si vide) | gammes de tailles (US-09.1, **nouveau en v4**) : tailles ordonnées, taille de base et tailles de rupture ; le chargeur refuse une clé inconnue, un type inconnu, un identifiant en double, une base absente de sa gamme, un nom de gamme ou un libellé vide, trop long ou en double, une gamme sans taille |
| `migration_notes` | liste de textes | non (omis si vide) | remarques écrites par la migration d'un ancien fichier |

Les **clés inconnues** du projet, d'une feuille ou d'une pièce sont conservées telles quelles et réécrites (emplacement
réservé aux évolutions). Partout ailleurs, une clé inconnue est refusée.

## 2. Conventions

- **Unités** : toutes les longueurs sont en **millimètres** (`f64`), tous les angles en **degrés**, sens **anti-horaire**
  (trigonométrique) ; repère de la feuille : X vers la droite, **Y vers le haut**. Les pixels n'existent pas dans le fichier.
  Les noms des champs portent l'unité (`distance_mm`, `angle_deg`, `x_mm`…) ; `x` / `y` des points libres sont en mm.
- **Identifiants stables** : entiers ≥ 1, attribués par le compteur `next_id`, **uniques dans tout le projet** (feuilles, points,
  tracés, crans, cotations, calques, groupes, lignes d'aide, surpiqûres, cotes complètes, pièces, marquages, groupes de boutons).
  Tout identifiant est strictement inférieur à `next_id`. Une référence est l'identifiant de l'objet visé.
- **Champs optionnels omis quand vides ou par défaut** : une liste vide, un `null` logique ou un réglage par défaut n'est pas écrit
  (un fichier sans l'objet reste identique octet pour octet). À la lecture, un champ absent prend sa valeur par défaut
  (indiquée dans les tableaux).
- **Variantes** : un objet « à variantes » porte un champ de discrimination, `type` (partout) ou `mode` (direction du droit-fil),
  dont la valeur est en `snake_case`. Une variante sans donnée s'écrit `{"type": "punch"}`.
- **Mesure observée ≠ paramètre de construction ≠ contrainte** : `construction` (§ 4.3) et les cotations mémorisent une provenance
  ou une mesure affichée ; `kept` (§ 4.3) maintient un résultat par recalcul direct, jamais une contrainte résolue par un solveur.

## 3. Types communs

| Nom | Forme |
|---|---|
| id | entier ≥ 1 |
| mm, degrés | nombre JSON (entier ou décimal) |
| `side` | `"left"` ou `"right"` (par rapport au sens départ → arrivée) |
| `origin` | `"start"` ou `"end"` |
| offset | `{"dx": mm, "dy": mm}` (poignée ou tangente relative) |
| xy | `{"x": mm, "y": mm}` (position visée) |

## 4. Feuille (`sheets[]`)

Obligatoires : `id`, `name`, `points`, `entities`. Optionnels (omis si vides) : `notches`, `dimensions`, `layers`, `groups`,
`guides`, `topstitches`, `full_dimensions`. Clés inconnues conservées.

### 4.1 Points (`points[]`)

`{ "id", "label", "definition", "role"?, "layer"? }`. `role` : `"construction"` (défaut) ou `"reference"`. `layer` : id d'un calque de la
feuille (absent = calque par défaut). `definition` dit **comment la position est obtenue** (source de vérité) :

| `type` | Champs |
|---|---|
| `free` | `x`, `y` |
| `on_segment` | `segment`, `distance_mm`, `origin` |
| `on_segment_ratio` | `segment`, `ratio` |
| `intersection` | `first`, `second` (tracés) |
| `on_curve` | `curve`, `t` |
| `circle_intersection`, `trace_intersection` | `first`, `second`, `near` (xy : solution la plus proche retenue) |
| `along_curve` | `curve`, `distance_mm`, `origin` |
| `offset` | `from` (point), `dx_mm`, `dy_mm` |
| `midpoint` | `first`, `second` (points) |
| `projection` | `point`, `trace` |
| `bisector` | `vertex`, `first`, `second` (points), `distance_mm` |
| `polar` | `from` (point), `distance_mm`, `angle_deg`, `reference`? (tracé : angle relatif à sa direction) |
| `compass` | `center_a`, `center_b` (points), `radius_a_mm`, `radius_b_mm`, `near` |
| `division` | `trace`, `ratio` (fraction de longueur d'arc), `start`? (point) |
| `mirror` | `source` (point), `axis` : `{"type":"guide","guide":id}` ou `{"type":"segment","segment":id}` |

Les dépendances entre points ne doivent pas être circulaires.

### 4.2 Tracés (`entities[]`)

`{ "id", "kind", "role"?, "layer"?, "name"? }`. `role` (défaut `construction`) est le **rôle métier du tracé**, jamais une couleur :
`construction`, `sewing_contour`, `cutting_contour`, `grainline`, `fold`, `annotation_axis`, `topstitch`, `center_front`,
`center_back`, `waist_line`, `hip_line`, `bust_line`, `skiving_line`, `glue_line`. `name` : unique dans la feuille.
`kind` :

| `type` | Champs |
|---|---|
| `segment` | `from`, `to` (points), `construction`? (§ 4.3) |
| `curve` (Bézier cubique) | `start`, `end` (points), `handle_start`, `handle_end` (offsets relatifs à leur extrémité) |
| `circle` | `center` (point), `radius_mm`, `start_deg`? (défaut 0), `ccw`? (défaut vrai) |
| `arc` | `center`, `start`, `end` (points distincts, `end` à la même distance du centre que `start`), `ccw`? (défaut vrai) |
| `spline` (courbe par points de passage) | `nodes` : liste de `{ "point", "corner"? (défaut faux), "tangent"?, "tangent_in"? }` |

### 4.3 Paramètres de construction (`segment.construction`)

Provenance saisie à la création : `polar` (`length_mm`, `angle_deg`), `perpendicular` (`reference`, `length_mm`, `side`),
`parallel` (`reference`, `distance_mm`, `side`). Par défaut elle est **non maintenue** (non contraignante). Champ optionnel
`kept` (US-03.2, booléen, **omis quand faux** : un projet sans lien se réécrit à l'identique à l'octet, format inchangé) :
la construction est **maintenue**, les points d'arrivée suivent leurs parents (point de départ, extrémités du segment de
référence) ; `to` pour `polar` et `perpendicular`, `from` et `to` pour `parallel`. Les coordonnées de ces points restent écrites
dans leur définition `free`. **Invariant vérifié à l'ouverture** : position écrite = position recalculée **à 1e-9 près**, la
référence (`perpendicular`, `parallel`) existe et est un segment, chaque point suivi est libre, suivi par un seul segment, ni centre
ni extrémité d'arc, aucun cycle ; sinon fichier refusé avec un message qui nomme le segment (`pf-validate` : code 1). Le
rectangle n'est pas repris (US-03.2b).

### 4.4 Crans, cotations, calques, groupes, lignes d'aide

- `notches[]` : `{ "id", "entity", "distance_mm", "side", "length_mm", "style"?, "anchor"? }`. `style` : `"single"` (défaut) ou
  `"double"`. `distance_mm` est la position le long du tracé depuis son départ ; **sans effet quand `anchor` est présent**
  (écrit à 0, la position se calcule à la lecture). `anchor` : `{"type":"point","point"}`, `{"type":"midpoint"}`,
  `{"type":"ratio","ratio"}` ou `{"type":"carried","entity","point","from_end"?}`. Crans avancés (US-06.1), tous optionnels et omis
  à leur valeur par défaut : `form` (`"line"` par défaut, `"t"`, `"v"`, `"u"`, `"slot"`) ; `width_mm` (0,1 à 100, **interdite pour
  `"line"`** ; absente = 3 mm) ; `on_cut` (booléen, faux par défaut) : le cran est **projeté** sur le contour de coupe calculé de la pièce
  perpendiculairement à la couture, vers l'extérieur ; la position sur la coupe n'est jamais écrite. Un `"u"` exige `width_mm` ≤ 2 ×
  `length_mm`. `on_cut` n'est pas validé contre les pièces à la lecture (un cran hors pièce ou non projetable est signalé, pas refusé).
  `corner` (US-06.1b, booléen, faux par défaut) : cran **en coin**, posé à un sommet de la couture (début ou fin de son tracé, écrit
  avec `anchor` `ratio` 0 ou 1) et orienté selon la bissectrice de l'angle du contour de la pièce ; sommet et direction ne sont jamais
  écrits. `corner` avec `on_cut` est **refusé** à la lecture (combinaison reportée) ; un cran en coin hors sommet identifiable est
  signalé, pas refusé (repli sur la normale du tracé).
- `dimensions[]` (cotations d'origine) : `{ "id", "from", "to", "offset_mm" }` — mesure affichée entre deux points distincts.
- `full_dimensions[]` (cotes complètes, mesures observées) : `{ "id", "kind", "label"?, "text_shift_mm"? }`, `kind` :
  `path_length` (`entity`, `offset_mm`), `angle` (`vertex`, `first`, `second`, `radius_mm`), `radius` / `diameter` (`entity`,
  `angle_deg`), `horizontal` / `vertical` (`from`, `to`, `offset_mm`).
- `formulas[]` (valeurs maintenues par une formule, US-03.1b) : `{ "point", "param", "expr" }`, omis si vide ; voir § 4.6.
- `length_constraints[]` (longueur développée imposée à une courbe de Bézier, US-03.3) : `{ "curve", "length_mm", "adjust" }` (+ `"reference"` optionnel, US-03.4), omis si vide ; voir § 4.7.
- `gradings[]` (écarts de gradation d'un point libre par taille, US-09.2a) : `{ "point", "size", "dx_mm", "dy_mm" }`, omis si vide ; voir § 4.9.
- `layers[]` : `{ "id", "name", "visible"? (défaut vrai), "locked"? (défaut faux) }`, du plus bas au plus haut ; noms uniques.
- `groups[]` : `{ "id", "members": [id de tracé] }`, jamais vide ; un tracé dans un seul groupe.
- `guides[]` : `{ "id", "x_mm", "y_mm", "angle_deg", "axis"?, "layer"? }` — droite infinie d'aide.
- `topstitches[]` (surpiqûres liées à un bord) : `{ "id", "source", "distance_mm", "side", "paths", "stale"? }` ; `source` =
  `{"type":"piece","piece"}` ou `{"type":"paths","paths":[id],"from"?}` ; `side` : `inside`, `outside`, `left`, `right` ; `paths` =
  tracés générés (de vrais tracés de rôle `topstitch`) ; `stale` = raison du dernier échec de mise à jour.

### 4.5 Variables (`variables[]`)

`{ "name", "unit", "expr" }`, les trois champs obligatoires ; la **position dans la liste** est l'ordre d'insertion (sans autre sens :
l'évaluation suit les dépendances). La table est stockée et évaluée (US-03.1a) ; une variable peut être citée par la formule d'un
paramètre de point (§ 4.6, US-03.1b).

- `name` : `[A-Za-z_][A-Za-z0-9_]*`, 40 caractères au plus, **sans accent** (portable dans le JSON et les outils tiers) ; unique **sans tenir
  compte de la casse** (`Tour` et `tour` se confondent : refusé) ; jamais `min`, `max`, `abs`, `sqrt`, `pi` (casse comprise). Les références
  d'une expression, elles, respectent la casse.
- `unit` : `mm`, `deg` ou `none`. **Étiquette d'affichage** : aucune conversion, le calcul ne manipule que des nombres purs.
- `expr` : 1 à 500 caractères. Opérateurs `+ - * /`, parenthèses, moins unaire, nombres décimaux (point **ou** virgule : la virgule est
  toujours une décimale), `;` sépare les arguments (`min(1,5;2)`), fonctions `min` `max` (2 à 8 arguments) `abs` `sqrt` (1 argument), constante
  `pi`, noms d'autres variables. Pas de puissance, de notation scientifique, de `+` unaire ni de séparateur de milliers ; imbrication de 32
  niveaux au plus. Le résultat est un nombre **fini** : division par zéro, racine d'un négatif, débordement sont refusés.
- 500 variables au plus ; aucun cycle (`A → B → A`, auto-référence) ; toute référence vise une variable existante.

### 4.6 Formules maintenues (`sheets[].formulas[]`, US-03.1b)

`{ "point", "param", "expr" }`, les trois champs obligatoires, clé inconnue refusée ; la clé `formulas` d'une feuille est omise
quand elle est vide (un projet sans formule se réécrit à l'identique à l'octet, format inchangé). La **valeur évaluée** n'est
pas dupliquée : elle reste dans la définition du point, à sa place habituelle (`distance_mm`, `angle_deg`, `dx_mm`, `dy_mm`), de
sorte qu'un lecteur qui ignore `formulas` voit la bonne valeur et que « délier » garde la valeur courante en nombre figé.

- `point` : identifiant d'un point de **la même feuille** ; `param` : `distance` ou `angle` (point `polar`), `dx` ou `dy` (point
  `offset`) ; un seul lien par couple `(point, param)`.
- `expr` : même grammaire que § 4.5 (1 à 500 caractères), qui cite des **variables** (jamais la géométrie : aucun cycle possible
  entre formules et points).
- **Invariant vérifié à l'ouverture** : l'expression s'évalue (variables existantes, table valide) et son résultat est égal à la
  valeur écrite dans le point **à 1e-9 près** ; sinon le fichier est refusé avec un message qui nomme le point et le paramètre
  (`pf-validate` : code 1). Rien n'est corrigé en silence.
- L'écart de 1e-9 mm absorbe l'arrondi d'une relecture JSON ; le chargeur ne réécrit pas la valeur.

### 4.7 Longueurs imposées (`sheets[].length_constraints[]`, US-03.3)

`{ "curve", "length_mm", "adjust" }`, les trois champs obligatoires, clé inconnue refusée ; la clé est omise quand elle est vide
(un projet sans contrainte se réécrit à l'identique à l'octet, format inchangé). Une contrainte par courbe. L'**inconnue** reste
écrite dans le modèle (poignée de la courbe ou coordonnées de l'extrémité libre) : un lecteur qui ignore la clé voit la bonne
géométrie, et retirer la contrainte garde la géométrie courante.

- `curve` : identifiant d'une courbe de Bézier (`type: curve`) de **la même feuille** (pas un segment, un arc, un cercle ni une courbe
  lisse). `length_mm` : longueur développée imposée, > 0 et ≤ 1 000 000 (le schéma borne `[0 ; 1 000 000]`, le chargeur refuse 0).
- `adjust` : `{ "type": "handle", "point" }` = la **longueur de la poignée** attachée à cette extrémité (direction inchangée, au moins
  0,001 mm) ; `{ "type": "slide", "point", "angle_deg" }` = cette extrémité, un point **libre**, glisse le long d'une droite de direction
  `angle_deg` (degrés, anti-horaire depuis +X) qui passe par sa position courante. `point` est l'une des deux extrémités de la courbe
  (qui doivent être distinctes) ; un point glissant n'est ni construit, ni centre ou extrémité d'arc, ni écrit par une construction
  maintenue (§ 4.3), et n'est glissant que pour une seule contrainte.
- **Invariant vérifié à l'ouverture** : structure valide, aucun cycle entre contraintes (l'une écrit ce que l'autre lit) et longueur
  développée réelle égale à `length_mm` **à 1e-6 mm près** ; sinon le fichier est refusé avec un message qui nomme la feuille et la
  courbe (`pf-validate` : code 1). La position exacte de l'inconnue n'est pas contrôlée au-delà de cette longueur.

- **Égalité de longueurs (US-03.4a)** : `reference` optionnel, `{ "entity", "offset_mm" }` (clé omise quand la cible est une valeur : les
  projets existants restent identiques à l'octet, format inchangé). `entity` : tracé **de la même feuille** (segment, Bézier, courbe
  lisse, arc ou cercle), jamais la courbe contrainte elle-même ; `offset_mm` : décalage signé, fini, de valeur absolue au plus
  1 000 000. `length_mm` reste **écrit** et vaut la longueur du tracé de référence + `offset_mm` (**à 1e-6 mm près**, vérifié à
  l'ouverture) ; la longueur développée réelle de la courbe vaut aussi `length_mm` à 1e-6 mm. Une référence peut elle-même être
  imposée (chaîne A = B + 3, B = C + 2) ; un cycle est refusé, y compris quand la référence dépend de ce que la contrainte modifie
  (extrémité glissante partagée). Référence inconnue, à soi-même, cible écrite fausse ou cycle : fichier refusé avec un message qui
  nomme la feuille et la courbe (`pf-validate` : code 1).

### 4.8 Tailles (`sizes[]`, US-09.1, nouveau en version 4)

Clé du **projet** (pas d'une feuille), omise quand il n'y a aucune gamme : un projet sans gamme se réécrit à l'identique à l'octet,
sauf la ligne `schema_version`. Liste **ordonnée** de gammes :

```json
"sizes": [
  { "id": 31, "name": "Femme", "kind": "alphanumeric", "base": 34,
    "sizes": [ { "id": 32, "label": "XS" }, { "id": 33, "label": "S", "break_size": true }, { "id": 34, "label": "M" } ] }
]
```

- Gamme : `{ "id", "name", "kind", "sizes", "base" }`, tous obligatoires, clé inconnue refusée. `kind` : `alphanumeric` (XS S M…) ou `numeric`
  (34 36 38…), une **étiquette** : le chargeur ne vérifie que la cohérence des libellés (ci-dessous), jamais un ordre alphabétique ni numérique.
- Taille : `{ "id", "label", "break_size"? }` ; `break_size` (taille de rupture, marqueur) est **omis quand faux**.
- **L'ordre de `sizes` est significatif** et conservé tel quel (c'est l'ordre de gradation). `base` désigne **une** taille de **cette** gamme
  (donc au plus une base par gamme, et toute gamme a au moins une taille ; 20 gammes au plus, 60 tailles par gamme).
- Les identifiants de gammes et de tailles sont dans l'espace d'identifiants du projet : ≥ 1, < `next_id`, **uniques dans tout le projet**.
- Nom de gamme : règles de nom du § 7 (sans espace en début ni en fin), unique dans le projet **sans tenir compte de la casse**. Libellé :
  non vide, **sans espace en début ni en fin**, 20 caractères au plus, sans caractère de contrôle, unique dans la gamme **sans tenir compte
  de la casse** ; le même libellé peut exister dans deux gammes.
- **Plusieurs gammes** (stature × tour) = plusieurs gammes **indépendantes** ; aucun produit cartésien n'est écrit.
- Ce qui n'est **pas** dans le fichier : la « taille active » d'affichage (état d'interface) et toute géométrie graduée (elle se calcule à la
  lecture). Une gamme seule ne modifie aucune géométrie : les écarts par point sont dans `sheets[].gradings` (§ 4.9).
- Un fichier de version **inférieure à 4** qui contient la clé `sizes` est refusé (« clé réservée au format version 4 »), même vide : rien
  n'est jeté ni réinterprété en silence.

### 4.9 Écarts de gradation (`sheets[].gradings[]`, US-09.2a, ajout optionnel de la version 4)

`{ "point", "size", "dx_mm", "dy_mm" }`, les quatre champs obligatoires, clé inconnue refusée ; la clé `gradings` d'une feuille est omise quand elle
est vide (un projet sans gradation se réécrit à l'identique à l'octet, schéma toujours en version **4**, pas de version 5). Aucune géométrie graduée
n'est stockée : **position du point dans la taille `size` = position écrite dans sa définition + `(dx_mm, dy_mm)`**, repère de la feuille (X vers la droite,
Y vers le haut). L'écart est **cumulé** : il part de la taille de base, jamais de la taille voisine ; une taille sans entrée a l'écart nul (géométrie de base).

```json
"gradings": [
  { "point": 2, "size": 32, "dx_mm": -9, "dy_mm": -20 },
  { "point": 2, "size": 33, "dx_mm": -5, "dy_mm": -10 }
]
```

- `point` : identifiant d'un point **libre** (`type: free`) de **la même feuille** (un point construit se recalcule depuis ses parents gradués :
  l'écart sur un point construit est refusé). `size` : identifiant d'une taille de **n'importe quelle gamme** du projet (les identifiants de taille sont uniques) ;
  l'évaluation prend **une** taille (une taille = une gamme, aucune combinaison stature × tour).
- `dx_mm`, `dy_mm` : nombres finis, de valeur absolue au plus 1 000 000 mm (le schéma les borne de même). Une entrée `(0 ; 0)` n'est **jamais** écrite
  (elle s'omet) ; **la taille de base d'une gamme n'a jamais d'entrée** (écart nul implicite) ; au plus une entrée par couple `(point, taille)`.
- Les entrées sont écrites triées par `(point, taille)` ; le chargeur ne l'exige pas (il garde l'ordre lu).
- **Invariants vérifiés à l'ouverture** (`pf-validate` : code 1 sinon, message qui nomme la feuille) : point existant et libre, taille existante, pas de
  base, pas de doublon, valeurs finies et bornées, entrée non nulle, **position graduée dans l'espace de travail** (±1 000 000 mm).
- **Changer la base** d'une gamme re-exprime les écarts (`écart'(s) = écart(s) − écart(nouvelle base)`) et déplace la position écrite des points
  concernés : toutes les positions évaluées restent identiques. Retirer une taille retire ses écarts ; supprimer un point retire les siens.
- Un fichier de version **inférieure à 4** qui contient la clé `gradings` dans une feuille est refusé (« clé réservée au format version 4 »).
- Non réévalués par taille (US-09.5 à 09.7) : constructions maintenues, formules et longueurs imposées (un point suivi par un lien n'est pas gradué à la
  pose ; un conflit né ensuite est signalé par le diagnostic). Le repère de gradation orientable et les écarts par intervalle sont reportés.

## 5. Pièces (`pieces[]`)

Une pièce est une **entité du projet** qui référence les tracés de son contour ; elle ne les possède pas.

| Champ | Type | Défaut / omission |
|---|---|---|
| `id`, `name`, `quantity` (entier ≥ 1), `link` | obligatoires | |
| `link` | `{"type":"linked","sheet":id}` (pièce liée aux tracés d'une feuille) ou `{"type":"detached"}` | |
| `code`, `material`, `comment` | textes | omis si absents |
| `category` | `fabric`, `lining`, `interfacing`, `leather`, `reinforcement` | omis si absente |
| `mirrored_pair` | booléen | faux |
| `outline` | liste de `{ "entity", "reversed"? }` | liste vide (pièce-étiquette) |
| `grainline` | id d'un tracé (droit-fil principal) | omis |
| `grain` | droit-fil avancé : `direction`?, `sense`?, `reversed`?, `rotation_tolerance_deg`?, `extra`? | omis quand tout est par défaut |
| `seam` | marge de couture : `{ "default_mm", "edges"?, "hems"?, "folds"?, "corners"?, "max_corner_mm"? }` | omis quand vide (défaut nul, sans surcharge, ourlet, rempli ni coin) |
| `net_cut` | `true` : pièce « coupe nette » (US-05.6), contour de coupe = contour de couture, déclaré | omis quand faux ; `false` refusé ; exclusif avec `seam` (le chargeur refuse les deux) |
| `no_grain` | `true` : pièce « sans droit-fil » (PH-5d, cuir) : plus d'avertissement « droit-fil manquant », mention « Sans droit-fil » à l'export | omis quand faux ; `false` refusé ; exclusif avec `grainline` et `grain` (le chargeur refuse les deux) |
| `on_fold` | ids des tracés **rectilignes** du contour déclarés « sur le pli » (PH-5b) : marge nulle, pas de coupe à cet endroit, libellé « SUR LE PLI » à l'export | omis si vide ; sans doublon ; chaque tracé doit exister sur la feuille de la pièce et être un segment (le chargeur refuse sinon) ; un tracé sorti du contour reste déclaré mais sans effet (contrôle) |
| `internal` | éléments internes `{ "target", "kind" }` | omis si vide |
| `markings` | perçages / pointages / croix | omis si vide |
| `darts` | pinces (US-07.1) | omis si vide |
| `pleats` | plis (US-07.3) | omis si vide |
| `flares` | lignes d'ampleur par ouverture en éventail (US-07.4) | omis si vide |
| `buttons` | groupes de boutons et boutonnières | omis si vide |
| `derived` | lien avec la pièce mère | omis |
| `placement` | `{ "x_mm", "y_mm", "rotation_deg" }` (plan de pièces) | omis = placement automatique, jamais écrit |

- `grain.direction` (`mode`) : `drawn` (défaut), `parallel` / `perpendicular` (`reference` = tracé), `bias` (`angle_deg`) ;
  `sense` : `none` (défaut), `nap`, `pattern` ; `extra` : lignes supplémentaires `{ "entity", "direction"?, "sense"?, "reversed"? }`.
- `seam` : `default_mm` (nombre de 0 à 1000, obligatoire) ; `edges` = surcharges `{ "entity", "start_mm", "end_mm" }` (une par tracé du
  contour, valeurs de 0 à 1000). `start_mm` est la marge au premier point du tracé et `end_mm` au dernier, le contour étant lu dans le sens
  anti-horaire ; différentes = marge progressive (segments seulement, refusée sur une courbe). La marge est mesurée vers l'extérieur. Le contour
  de coupe **n'est pas écrit** : il est calculé à la lecture. Un `seam` à défaut nul sans surcharge ni coin n'est pas écrit (le chargeur le refuse).
  `corners` (US-05.2) = types de coins `{ "entity", "corner" }` : le sommet est celui où **commence** le tracé `entity` (contour lu en
  anti-horaire), un seul type par sommet ; `corner` = `{"type": …}` avec `perpendicular_previous`, `perpendicular_next`, `symmetric`,
  `truncated` (`depth_mm`, 0 à 10 000), `stepped`, `hem_fold_next` ou `hem_fold_previous` (l'onglet, par défaut, n'est jamais écrit).
  `max_corner_mm` (0 à 10 000) = longueur maximale de la pointe d'un onglet ; absent : 4 × la plus grande marge adjacente.
  `hems` (US-05.3) = ourlets `{ "entity", "height_mm" }` (0 à 1000) sur un **segment** du contour : marge d'ourlet = `height_mm`, pli sur le bord,
  marges voisines réfléchies aux coins saillants. `folds` = rempliés (maroquinerie) `{ "entity", "value_mm", "notch_step_mm"?, "notch_count"? }` :
  marge de coupe = `value_mm` (0 à 1000), droit ou courbe ; `notch_step_mm` (1 à 1000) **ou** `notch_count` (1 à 1000), jamais les deux, seulement
  sur un bord courbe ; les crans eux-mêmes sont calculés à la lecture (défaut : un pas de 25 mm), jamais écrits. Un tracé n'a qu'un réglage parmi
  `edges`, `hems` et `folds` (doublon refusé) ; tracé hors contour refusé.
- `internal[].target` : `{"type":"path","entity"}`, `{"type":"point","point"}` ou `{"type":"notch","notch"}` ; `kind` :
  `fold`, `topstitch`, `pocket_position` (lignes), `mark`, `buttonhole` (repères), `other`.
- `markings[]` : `{ "id", "at", "shape" }` ; `at` = `{"type":"point","point"}` ou `{"type":"dart_tip","tip","toward","setback_mm"}` ;
  `shape` = `punch`, `drill` (`diameter_mm`), `cross` (`size_mm`) ou `slot` (fente de poinçon, PH-4b : `length_mm` hors tout, `width_mm`
  de 0,1 à 100 et au plus `length_mm` (≤ 100), `angle_deg` dans [0 ; 360[ depuis +X, anti-horaire ; les trois champs sont obligatoires, une
  clé en plus est refusée ; absent de tout fichier sans fente). Positions calculées à la lecture. Les perçages « au pas » d'un bord
  (`AddPieceMarkingRun`) sont des marquages ordinaires : la rangée n'est pas stockée ; leurs points d'ancrage sont des points libres
  du calque masqué « Points de perçages » (nom ordinaire de calque, aucun champ nouveau).
- `darts[]` : `{ "id", "apex", "toward", "length_mm", "opening_mm", "fixed", "cut_closed"? }` ; `cut_closed` (PH-5a, booléen, **omis quand faux**, un
  fichier sans le champ est relu et réécrit à l'octet) = le contour de coupe est tracé pince fermée puis rouvert à la bouche (jamais stocké : calculé
  à la lecture, repli sur la coupe ordinaire si impossible) ; `apex` = sommet et `toward` = point d'axe (deux points de la
  feuille de la pièce, différents), `length_mm` (1 à 2000) = distance du sommet au milieu de la base sur l'axe, `opening_mm` (0,1 à 1000, jamais
  plus que la longueur) = largeur de la base, `fixed` = `left` ou `right` (jambe qui reste en place à la fermeture, vue du sommet vers la base).
  Pieds, jambes, fermeture et capuchon sont **calculés à la lecture**, jamais écrits ; la géométrie n'est pas jugée à la lecture (un point déplacé
  rend la pince « non calculable » sans la perdre).
- `pleats[]` (US-07.3) : `{ "id", "kind", "from_x_mm", "from_y_mm", "to_x_mm", "to_y_mm", "depth_mm", "sense" }` ; `kind` = `flat`
  (pli plat, bande de 2 × profondeur) ou `hollow` (pli creux, 4 × profondeur) ; `from → to` = droite qui porte le **bord de départ** de la
  bande, en mm dans le repère de la feuille de la pièce (coordonnées, pas des références) ; `sense` = `left` ou `right` de `from → to` (côté où
  s'étend la bande) ; `depth_mm` de 0,1 à 500, 100 plis au plus par pièce. Le contour de la pièce est le contour **déplié** (avec la bande de chaque
  pli) ; les plis sont listés dans l'ordre de pose et leurs lignes sont dans le repère du contour actuel. La pièce repliée et les repères (pli,
  contre-pli, flèche) sont **calculés à la lecture**, jamais écrits ; un contour modifié ensuite peut les rendre « non calculables » sans les perdre.
- `flares[]` (US-07.4) : `{ "id", "hinge_x_mm", "hinge_y_mm", "stay_x_mm", "stay_y_mm", "moved_x_mm", "moved_y_mm" }` ; une ligne de coupe d'une
  ampleur par ouverture en éventail déjà faite dans le contour : `hinge` = pivot (sur le bord fixe), `stay` = extrémité haute de la lèvre restée en
  place, `moved` = extrémité haute de la lèvre qui a tourné ; coordonnées en mm dans le repère de la feuille de la pièce, 50 lignes au plus. Les deux
  lèvres partent du pivot avec la même longueur et sont écartées (sinon le fichier est refusé). Ouverture (distance `stay`–`moved`), longueur de la ligne
  et angle sont **calculés à la lecture**, jamais écrits. Le contour de la pièce est le contour **ouvert** (le vide entre les lèvres est rempli).
- `buttons[]` : `{ "id", "from", "to", "count", "element", "edge_margin_mm" }` ; `element` = `button` (`diameter_mm`) ou
  `buttonhole` (`length_mm`, `width_mm`, `orientation` : `horizontal`, `vertical`, `perpendicular` ou `angle` avec `degrees`).
  Les positions des `count` éléments, réparties entre les deux points, sont calculées à la lecture.
- `derived` : `{ "mother", "kind", "operation", "stale"? }` ; `kind` : `facing`, `lining`, `interfacing`, `reinforcement`, `unfolded` ;
  `operation` = `offset` (`distance_mm`) ou `unfold` (`fold` = tracé de pliure).

## 6. Versions et compatibilité

- La version écrite aujourd'hui est **4** (`schema_version`). Le chargeur lit les versions **1 à 4** : un fichier 1, 2 ou 3 est migré
  **en mémoire** (v1 → v2 : seul le numéro change ; v2 → v3 : l'ancien champ `sheets[].piece` est retiré et remplacé par une pièce du
  projet, avec une remarque dans `migration_notes` si le contour n'a pas pu être déterminé ; v3 → v4 : seul le numéro change, `sizes`
  absent = aucune gamme). Le fichier d'origine n'est pas modifié tant que l'utilisateur n'enregistre pas ; `pf-validate` ne l'enregistre
  jamais. L'écriture est toujours en v4.
- Un numéro supérieur à 4, nul ou non entier est **refusé** ; le message invite à mettre l'application à jour et le fichier reste intact.
- Politique : jusqu'à la v3, les nouvelles variantes (cercles, arcs, courbes lisses, guides, symétries, cotes complètes, surpiqûres
  liées…) ont été ajoutées **sans changer le numéro**, avec des champs optionnels omis quand vides. La v4 (décision 285) marque l'arrivée
  de la gradation : la v3 écrite par l'application précédente est relue sans perte, mais une **version précédente de l'application ne sait
  pas ouvrir un fichier v4** (elle le refuse comme « plus récent » ; non vérifié en pratique, voir la décision 324).
- Aucune garantie de compatibilité n'est donnée au-delà de ce que le chargeur de l'application accepte aujourd'hui.

## 7. Règles de validation et intégrité référentielle

Appliquées par le chargeur à l'ouverture (et après chaque commande de l'application) ; un fichier refusé n'est jamais modifié :

1. `format` = `"patternforge"`, `schema_version` entier accepté, JSON bien formé, taille ≤ 64 Mo.
2. Aucune clé inconnue sauf au niveau du projet, d'une feuille et d'une pièce (conservées).
3. Noms (projet, feuille, calque, tracé, pièce, matière, code) : non vides après suppression des blancs, 120 caractères au plus, sans
   caractère de contrôle ; commentaire de pièce : non vide, 2000 caractères au plus, sauts de ligne permis. Noms de calques uniques par
   feuille, noms de tracés uniques par feuille.
4. Au moins une feuille. Tout identifiant est ≥ 1, < `next_id` et **unique dans le projet**.
4 bis. Tailles (§ 4.8) : identifiants uniques dans le projet, base dans sa gamme, libellés et noms cohérents, gamme non vide.
5. Toute référence vise un objet existant (point, tracé, calque, feuille, pièce, cran…), et un point ne dépend pas de lui-même.
6. Géométries non dégénérées : segment de longueur non nulle, courbe valide, arc à trois points distincts et équidistants, cercle de
   rayon valide, nombres finis ; cran dans la longueur de son tracé (sauf ancré) et de longueur bornée.
7. Pièces : quantité ≥ 1 ; contour et droit-fil référencent des tracés existants (de la feuille liée), sans tracé en double ; éléments
   internes cohérents avec leur cible, sans doublon, jamais un tracé du contour ou du droit-fil ; marquages, boutons, droit-fil avancé,
   placement, pièces dérivées et surpiqûres liées sont validés par le chargeur (bornes et références propres à chaque objet).

## 8. Schéma JSON et validateur

### Schéma

[`pforge-v4.schema.json`](pforge-v4.schema.json) (JSON Schema draft 2020-12) est **écrit à la main** d'après les types du code : aucune
dépendance de génération n'a été ajoutée au projet (pas de réseau garanti) ; un test compare ses listes d'énumérations au code
(rôles de tracé, catégories, types d'éléments internes) et un autre le confronte à tout le corpus du dépôt. Il utilise
`additionalProperties: false` partout où le code refuse les clés inconnues, et l'autorise au niveau du projet, des feuilles et des pièces.
Il est vérifié par un **mini-validateur écrit pour les tests** (sous-ensemble de la draft 2020-12, indépendant du code de production) ;
**aucune bibliothèque tierce de validation (par exemple Python `jsonschema`) n'a été exécutée** : à faire si elle est disponible.

### Validateur en ligne de commande `pf-validate`

```bash
cargo run -q -p pf-core --bin pf-validate -- projet.pforge.json [--json]
```

Sans interface graphique, il charge le fichier avec **le même chargeur que l'application** (donc les mêmes règles que le § 7) et **ne
modifie jamais** le fichier. Code de sortie : **0** valide, **1** invalide, **2** fichier illisible (absent, droits, UTF-8 invalide)
ou usage incorrect. Une erreur est donnée avec une catégorie (`json`, `format`, `version`, `structure`, `reference`, `identifiant`,
`validation`, `lecture`), le message du chargeur et, **quand l'analyseur JSON la connaît**, la ligne et la colonne. Il n'existe pas de
chemin JSON pour les erreurs de structure ni pour les erreurs de validation (le message cite l'identifiant ou le nom de l'objet) ; une
seule erreur, la première trouvée, est rapportée. `--json` écrit un seul objet à clés triées (`file`, `valid`, `file_schema_version`,
`app_schema_version`, `errors` : `[{kind, message, line, column}]`, et pour un fichier valide `project`, `migrated_in_memory`,
`migration_notes`), identique d'un appel à l'autre. Les textes du binaire sont en français (ils ne passent pas par l'interface egui).

## 9. Limites connues

Écarts entre le schéma, le chargeur et ce que l'on pourrait attendre. Liste **fermée** vérifiée par une campagne de 240 mutations
(`crates/pf-core/tests/format_ouvert.rs`) :

| Écart | Schéma | Chargeur |
|---|---|---|
| **Règles sémantiques** : références mortes, identifiants dupliqués ou ≥ `next_id`, noms vides, bornes métier, géométries dégénérées, contour de pièce incohérent | accepte | refuse |
| **`null` sur un champ optionnel** (`"code": null`…) : le chargeur le lit comme « absent », l'application ne l'écrit jamais | refuse | accepte |
| **Versions 1 à 3** : le schéma ne décrit que la version 4 (la v3 garde son schéma, [`pforge-v3.schema.json`](pforge-v3.schema.json)) | refuse | accepte (migration) |
| **Entier écrit en flottant** (`3.0`, `1.0`) : égal à l'entier pour JSON Schema | accepte | refuse |
| **Clé dupliquée** dans un objet (`"name"` deux fois) : le schéma reçoit un document déjà analysé et ne peut pas la voir | accepte | refuse (message qui nomme la clé, ligne et colonne) |
| Identifiants plus grands que 2^64 − 1, valeurs `NaN` (non représentables en JSON) | non borné | refuse |

Défauts corrigés (plus des écarts) : **D-02** (clé dupliquée, désormais refusée par le chargeur, `pf-validate` et le presse-papiers) et
**D-04** (une clé en trop sur `{"type":"punch"}` ou `{"type":"detached"}` est désormais refusée ; les formes sans clé en trop restent lues
et écrites à l'identique).

Aussi non couvert : le schéma ne vérifie ni longueurs de textes, ni bornes numériques métier (sauf identifiant ≥ 1 et entiers 32 bits),
ni l'unicité, ni les références. La spécification ne décrit **que ce qui existe dans le code** : les gammes de tailles existent (§ 4.8), mais les écarts de
gradation par point, la géométrie gradée et les règles de coupe calculées n'existent pas dans ce format.

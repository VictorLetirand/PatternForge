# Guide utilisateur — PatternForge

*English version: [USER_GUIDE.md](USER_GUIDE.md).*

Atelier de CAO 2D pour construire des patrons précis, en millimètres, hors ligne. Ce guide décrit
ce que fait la version actuelle, et ce qu'elle ne fait pas.

> **Avant d'imprimer pour de vrai** : l'export PDF reste *expérimental* tant que le carré de
> 100 mm n'a pas été mesuré sur papier (voir « Vérifier l'impression »).

## 1. Démarrer

```bash
cargo run --release -p pf-desktop
```

Options : un chemin de projet à ouvrir ; `--demo` (exercice de référence) ; `--lang fr|en` ;
`--theme dark`. Fichiers de projet : `.pforge` (texte JSON lisible, versionné). Les anciens `.pforge.json` s'ouvrent toujours. Avec
une version installée, un double-clic sur un `.pforge` l'ouvre (sous macOS, l'application démarre mais il faut
ouvrir le fichier par *Fichier › Ouvrir*).

## 2. L'écran

| Zone | Rôle |
|---|---|
| Barre de menus | Fichier, Édition, Affichage (accrochage, thème, langue), Aide |
| Barre d'outils | Sélection, puis un menu par famille (Dessin, Construction, Modification, Pièce, Mesure) qui liste les outils avec leur lettre ; Annuler, Rétablir et Cadrer restent à droite ; une seule rangée |
| Panneau gauche | Projet, feuilles, pièce de patron |
| Canevas | Dessin à l'échelle ; molette = zoom autour du curseur, bouton du milieu ou Espace + glisser = déplacer, `Début` = tout cadrer |
| Inspecteur (droite) | Propriétés de la sélection, mesures, contours fermés |
| Barre d'état | Outil, position de la souris en mm, zoom, aide contextuelle |

## 3. Les feuilles

Un projet contient plusieurs **feuilles** indépendantes (« + Nouvelle feuille », `F2` ou
double-clic pour renommer, flèches pour réordonner). Une feuille n'est pas une page A4. Elle peut
porter une ou plusieurs pièces de patron (nom, quantité, matière) — la case « Cette feuille porte une pièce » crée une fiche sans contour, l'outil
« Extraire une pièce » crée une pièce avec son contour (voir ci-dessous). Une pièce est enregistrée dans le projet ; les anciens fichiers s'ouvrent et sont migrés ; pour un fichier v1 ou v2 : le contour de coupe est rattaché à la pièce
s'il est unique, sinon la pièce est conservée sans contour (remarque enregistrée dans le fichier).

**Dupliquer une feuille** : bouton « Dupliquer » du panneau (ou `Fichier › Dupliquer la feuille`). La copie
s'appelle « X (copie) », se place juste après l'originale et devient la feuille active ; elle reprend
points, tracés, crans, cotations, groupes, calques (verrous compris) et la pièce liée, tous indépendants
de l'original. Annulable en une fois avec `Ctrl+Z`.

**Importer une feuille d'un autre projet** : `Fichier › Importer une feuille depuis un projet…`, choisissez
le fichier `.pforge` (les anciens fichiers v1/v2 sont migrés), puis la feuille à copier. Elle est
ajoutée en dernier dans le projet courant (nom rendu unique : « X (2) » s'il existe déjà) ; le projet
source n'est pas modifié. Annulable en une fois.

### Les calques

Le panneau « Calques » (colonne de gauche, sous la pièce) range les points et tracés d'une feuille.
« + Calque » en crée un et le rend **actif** : tout ce que vous dessinez ensuite (y compris les points
accrochés ou construits) y est rangé ; « Calque par défaut » est le calque de départ, qui n'existe pas
dans la liste d'un fichier sans calque. Cliquer un nom le rend actif ; le champ « Nom » renomme le
calque actif ; ⏶ ⏷ changent son rang ; « Supprimer » refuse un calque qui contient des objets
(déplacez-les d'abord). « Déplacer la sélection ici » range la sélection dans le calque actif.
- **Voir** : un calque masqué n'est ni dessiné, ni accrochable, ni sélectionnable, ni exporté (SVG/PDF).
- **Verr.** : un calque verrouillé reste visible et accrochable, mais ses objets ne peuvent plus être
  modifiés, déplacés ni supprimés (message d'erreur, rien n'est changé). Un point appartient au calque
  où il a été créé ; un tracé verrouillé protège aussi ses deux extrémités.
Tout est annulable. Les crans et les cotations suivent le calque de leur tracé ou de leurs points.
Pas encore : couleur de calque, ordre de dessin selon les calques.

### Noms et groupes

- **Nom d'un tracé** : sélectionnez un tracé, champ « Nom » de l'inspecteur (validé à Entrée ou en
  quittant le champ). Facultatif ; un même nom ne peut servir qu'une fois par feuille (message
  d'erreur sinon) ; un champ vidé retire le nom.
- **Grouper** : sélectionnez au moins deux tracés, `Édition › Grouper` ou `Ctrl+G`. Cliquer un membre
  sélectionne tout le groupe (idem au rectangle de sélection) ; `Alt+clic` ne sélectionne que le membre.
  Déplacer, pivoter, symétrie et duplication (`Édition › Transformer / Dupliquer`) s'appliquent à toute
  la sélection en une seule étape annulable (`Ctrl+Z`).
- **Dégrouper** : `Édition › Dégrouper` ou `Ctrl+Maj+G`. Un tracé n'est que dans un groupe ; supprimer un
  membre le retire du groupe, un groupe vide disparaît. Un tracé d'un calque verrouillé ne se groupe pas.
- Une copie (duplication) n'est pas regroupée et ne reprend pas le nom de l'original.

### Rechercher un objet

`Édition › Rechercher…` ou `Ctrl+F` ouvre une fenêtre : tapez une partie du nom (sans souci de
majuscules ni d'accents), puis affinez par type (tracé, segment, courbe, cran, cotation, point), par
rôle (tracés seulement) et par calque. Un clic sur un résultat sélectionne l'objet et cadre la vue
dessus. La recherche porte sur la feuille active ; les objets d'un calque masqué ne sont pas listés.

### Couper, copier, coller

`Ctrl+X`, `Ctrl+C`, `Ctrl+V` (ou `Édition › Couper / Copier / Coller`) agissent sur la sélection, entre
feuilles et entre projets (un fragment est aussi placé dans le presse-papiers du système : on peut
coller dans une autre fenêtre de PatternForge). Les objets collés reçoivent de nouveaux identifiants,
vont dans le **calque actif** et sont sélectionnés ; collés à la même place dans la même feuille, ils sont
décalés de 5 mm (autant de fois que nécessaire). Sont conservés : les points construits dont le tracé
parent est aussi copié, les crans, les cotations dont les deux points sont copiés, les groupes entièrement
sélectionnés. Un point dont le parent n'est pas copié devient **libre** (remarque affichée) ; une
perpendiculaire ou parallèle dont la référence n'est pas copiée perd seulement la mémoire de sa
construction. Les pièces ne sont pas copiées. Coller dans un calque verrouillé est refusé. Couper est
refusé si un objet est dans un calque verrouillé. Le collage est une seule étape annulable. Dans un champ
de texte, ces touches gardent leur rôle habituel. Le menu « Coller » n'utilise que le presse-papiers
interne ; le collage depuis une autre fenêtre se fait au clavier.

## 4. Les outils

Chaque outil se choisit à la barre ou à la lettre indiquée ; `Échap` annule le geste en cours sans
rien laisser. Les nombres se tapent avec virgule ou point, avec unité facultative
(`120`, `12,5 cm`, `1,2 m`) ; `Tab` passe au champ suivant, `Entrée` valide.

| Outil | Lettre | Usage |
|---|---|---|
| Sélection | `V` | Clic, Maj+clic (ajouter/retirer), fenêtre de sélection ; glisser un point pour le déplacer |
| Point | `O` | Pose un point libre (ou accroché) |
| Segment | `S` | Départ, puis longueur et angle saisis, ou clic d'arrivée |
| Rectangle | `R` | Largeur et hauteur saisies, ou deux coins |
| Perpendiculaire | `P` | Segment de référence, point de départ, longueur ; « Inverser » change de côté |
| Parallèle | `L` | Segment de référence, distance saisie ; le côté suit la souris |
| Point à distance | `D` | Un point sur un segment à une distance donnée de son départ |
| Cran | `N` | Cran sur un tracé à une distance de son départ ; longueur, côté, cran double |
| Droit-fil | `G` | Comme un segment ; dessiné avec une flèche à deux pointes |
| Courbe | `C` | Départ, arrivée, puis deux poignées à tirer ; l'inspecteur donne la longueur développée |
| Courbe lisse | `B` | Clics successifs sur les points de passage (accrochage habituel) ; `Entrée` termine (2 points minimum), `Échap` annule. L'aperçu montre la courbe qui passera par le point visé |
| Cercle | `E` | Quatre sous-modes à la barre : **centre + rayon** (clic du centre, rayon cliqué ou tapé), **3 points**, **2 points + rayon** (le côté du centre suit la souris ; sans rayon tapé, le centre est la projection de la souris sur la médiatrice) |
| Arc | `A` | **Centre + angles** : centre, puis rayon et angle de début (cliqués ou tapés, `Tab` passe de l'un à l'autre), puis angle de fin ; sens **anti-horaire** par défaut, « Inverser » prend l'autre arc. Aussi : 3 points (départ, passage, arrivée) et 2 points + rayon |
| Congé | `F` | Clic sur un premier tracé (segment, arc ou cercle) **du côté à conserver**, puis sur le second ; tapez le **rayon**, `Entrée`. L'aperçu montre l'arc ; les deux tracés sont raccourcis aux points de tangence (un cercle n'est jamais coupé). Rayon trop grand, tracés parallèles ou qui ne se coupent pas : refusé avec la raison, rien n'est modifié. Les crans tombant dans la partie retirée sont supprimés avec une remarque |
| Chanfrein | `H` | Comme le congé, entre deux segments ; une distance (ou deux, `Tab`), `Entrée` |
| Tangente | `T` | Clic sur un point puis sur un cercle ou un arc : droite tangente, le côté du clic choisit parmi les deux solutions (point sur le cercle : tapez la longueur). Clic sur une courbe (Bézier, courbe lisse, arc, cercle) : segment tangent au point visé, longueur tapée (50 mm par défaut). Construction fixe : elle ne suit pas la courbe si celle-ci change |
| Mesurer | `M` | Distance entre deux points (lecture seule) |
| Cotation | `Q` | Clic sur deux points existants ; le décalage (10 mm par défaut) se tape ou s'édite dans l'inspecteur ; « Inverser » change de côté. C'est une **mesure affichée** qui suit les points, pas une contrainte ; elle apparaît aussi en SVG et en PDF |

**Cercles et arcs** : le rayon d'un cercle est un *paramètre de création* (modifiable dans l'inspecteur, non
imposé) ; la circonférence, l'aire, la longueur d'arc exacte (rayon × angle) et le rayon d'un arc sont des
*mesures*. Le centre et les extrémités d'un arc sont des points : glisser le centre déplace l'arc, glisser une
extrémité change son rayon (l'autre extrémité suit sur son rayon). Un cercle ou des arcs fermés forment un
contour (périmètre, aire). Les crans se placent à une distance le long de l'arc. Une symétrie inverse le sens.
L'accrochage « intersection » fonctionne entre cercle/arc et segment ou autre cercle ; avec deux solutions,
la plus proche de la souris est retenue et mémorisée. Un arc s'accroche par ses points (centre, extrémités),
pas encore « sur le tracé » ni à son milieu. Les arcs sont exportés en vrais arcs (SVG) ou en courbes de Bézier
d'erreur inférieure à 0,005 mm (PDF).

**Intersection de deux tracés** : avec l'outil Point (`O`) et l'accrochage activé, cliquez à un croisement entre une
courbe (Bézier ou courbe lisse) et un segment, une autre courbe, un arc ou un cercle : un point construit est créé à
l'intersection exacte (précision 1e-9 mm, tangence comprise). Les tracés ne sont pas prolongés. Quand il y a plusieurs
croisements, celui le plus proche de votre clic est retenu et mémorisé. Le point suit ses tracés : si vous les déplacez
de sorte qu'ils ne se coupent plus (ou se superposent), le déplacement est **refusé** avec un message et rien ne change.
Il ne se glisse pas à la souris. Entre deux segments, ou avec un cercle ou un arc, l'intersection garde son comportement
décrit plus haut (droites et cercles prolongés).

**Points relatifs** : l'outil Point (`O`) propose, dans la barre d'outils, des sous-modes qui créent un point **maintenu** (il suit ses
parents) : *Sur courbe* (cliquez le tracé, saisissez la longueur d'arc ; case « Mesurer depuis la fin »), *dx / dy* (cliquez un point,
saisissez dx et dy), *Milieu* (deux points), *Projeté* (un point, puis le tracé ; un segment est prolongé en droite), *Bissectrice*
(sommet, puis un point de chaque côté, distance saisie), *Direction* (point, distance et angle absolu) et *Direction / tracé* (angle relatif
à un segment), *Compas* (centres A puis B, rayons saisis, puis un clic près de la solution voulue). Les parents se désignent en cliquant sur
des points existants. Les valeurs se modifient ensuite dans l'inspecteur. Si le changement rend la construction impossible (distance plus
longue que la courbe, rayons qui ne se coupent plus, points confondus), il est **refusé** avec un message et rien ne bouge. Ces points ne
se glissent pas à la souris ; supprimer un point parent est refusé tant que le point relatif existe.

**Divisions** : sous-mode *Division* de l'outil Point (`O`). Cliquez un tracé (segment, Bézier, courbe lisse, arc ou cercle), puis saisissez
dans la palette soit un nombre de parts (`5`), soit des proportions (`1:2:1`, `1/4;1/2;1/4`, `0,5 1,5`), et validez par Entrée : N − 1 points
sont créés d'un coup (un seul Annuler les retire). Les parts sont égales en **longueur d'arc** (depuis le départ du tracé) ; sur un cercle,
elles sont égales en angle, et un clic sur un point après le cercle fixe le point de départ (sinon le départ propre du cercle). Ces points sont
**maintenus** : si le tracé change, ils gardent leur fraction. N va de 2 à 1000 ; proportions finies et strictement positives. Le tracé lui-même
n'est pas découpé. Refusé (message, rien ne change) pour un calque verrouillé ou une saisie invalide.

**Lignes d'aide** (outil *Ligne d'aide* `U`). Une ligne d'aide est une droite **infinie** (horizontale, verticale ou oblique) qui sert de
repère : elle n'est pas un tracé, n'est jamais exportée en PDF ou SVG par défaut et ne fait partie d'aucune pièce. Choisissez le mode dans la
barre d'outils : *Horizontale* ou *Verticale* (un clic, ou tapez la position Y ou X dans la palette puis Entrée) ; *Oblique* (cliquez un point
de passage, puis tapez l'angle et Entrée, ou cliquez un second point ; vous pouvez aussi taper X et Y du point de passage). Elle est dessinée en
trait fin pointillé bleu (point et tiret pour un axe de symétrie) sur toute la zone visible, et s'**accroche** : le curseur propose « Sur ligne
d'aide » et les intersections guide/guide et guide/tracé ; le point posé est un point libre exact (il ne suit pas la ligne ensuite). Cliquez
une ligne (outil Sélection) pour l'inspecter : point de passage, angle, case *Axe de symétrie*, calque ; glissez-la une fois sélectionnée pour
la déplacer ; Suppr la supprime ; copier/couper/coller marchent. Elle suit le calque actif à sa création : masquer ou verrouiller ce calque la
masque ou la protège. Menu *Affichage* : *Lignes d'aide* (masque toutes les lignes, désactive leur accrochage) et *Règles* (graduations en mm sur
les bords du canevas, affichage seul). *Transformer › Symétrie* propose l'axe des lignes marquées « axe » ou sélectionnées avec les objets.
Pour inclure les lignes dans un export, l'option existe dans la bibliothèque `pf-export` (`include_guides`), pas encore dans l'interface.

**Modifier un tracé** (outils *Prolonger* `I`, *Couper* `K`, *Fusionner* `J` ; menu Édition ; *Inverser le sens* au menu, sur la sélection).
Chaque opération est un seul Annuler. *Prolonger* : cliquez un segment ou un arc près de l'extrémité à modifier, puis tapez la valeur dans la
palette (positive : prolonge ; négative : raccourcit) et Entrée, ou cliquez le tracé jusqu'auquel aller. Courbes et cercles : refusés.
*Couper* : cliquez le tracé à l'endroit voulu (une courbe lisse se coupe à l'un de ses points de passage, un cercle ne se coupe pas) : on obtient
deux tracés de même calque, rôle et groupe ; le second se nomme « nom (2) » ; les crans restent exactement à la même place. *Fusionner* :
cliquez deux tracés qui se touchent par une extrémité commune et sont de même type (segments alignés, arcs d'un même cercle, courbes
issues d'une coupe) ; même calque et même rôle requis. *Inverser* : départ et arrivée échangés, la forme et les crans ne bougent pas.
Les points construits sur un tracé coupé, prolongé ou fusionné deviennent libres (message) ; un cran perdu est signalé. Tout cas impossible
(point hors du tracé, tracés non adjacents, types différents, valeur trop grande, calque verrouillé) est refusé avec un message, sans rien changer.

**Courbe lisse** : une courbe passe exactement par ses points de passage. Chaque point est **lisse** (la tangente
continue : pas de pli) ou **anguleux** (case « Anguleux » de l'inspecteur : deux tangentes, un pli ; pas aux
extrémités). Les tangentes ont un défaut calculé ; pour les modifier, saisissez un **module** (mm) et une **direction**
dans l'inspecteur (« Recalculer » revient au défaut). Le module est celui de la tangente d'Hermite : un tiers de cette
valeur est la longueur de la poignée de Bézier équivalente. Les tangentes ne se tirent pas à la souris : seuls les
points de passage se glissent. « Insérer après » ajoute un point au milieu du tronçon suivant, « Retirer » en enlève
un (le point reste sur la feuille ; deux points minimum). L'inspecteur donne la longueur développée totale et celle de
chaque tronçon : ce sont des **mesures**, rien n'est imposé. La continuité garantie est celle de la tangente (C1), pas
celle de la courbure. Les crans se posent à une distance le long de toute la courbe. Une courbe à deux points se convertit
en Bézier, et une Bézier en courbe lisse à deux points, sans changer de forme (boutons de l'inspecteur).

**Accrochage** (Affichage) : extrémités, intersections, milieux, points d'un tracé. Le marqueur
indique la nature de la cible ; l'accrochage choisit une cible, il ne déforme jamais la valeur
calculée.

**Édition › Transformer / Dupliquer** : déplacer, pivoter, symétrie, avec ou sans copie. Les
crans suivent leur tracé ; une symétrie inverse leur côté.

**Édition › Aligner / Répartir / Mettre à l'horizontale ou à la verticale** : agissent sur les points
sélectionnés (et les extrémités des tracés sélectionnés). *Aligner* : à gauche, à droite, en haut, en bas,
centre horizontal (tous sur l'horizontale médiane de la sélection), centre vertical, ou sur la ligne d'aide
(ou le segment) sélectionné(e) avec les points. *Répartir* : espacement égal horizontal ou vertical entre
les deux extrêmes, ou le long de la droite entre les deux points les plus éloignés. *Mettre à
l'horizontale / à la verticale…* : le ou les segments sélectionnés pivotent avec tout ce qui leur est
attaché (tracés et points partagés), autour de leur point de départ ou d'un point saisi ; si la sélection
mêle d'autres objets, c'est toute la sélection qui pivote, le premier segment servant de référence. Un
point construit qui devrait bouger, un calque verrouillé ou des segments liés entre eux rendent
l'opération impossible : un message le dit et rien n'est déplacé. Moins de 2 points (3 pour répartir) ou
déjà alignés : « rien à faire », sans étape d'historique. Une seule étape annulable.

**Symétrie liée** (Édition › *Symétrie liée…*, puis *Déplier la symétrie*) : construisez une moitié, marquez une ligne d'aide comme
*axe*, sélectionnez les points et tracés à refléter **et** la ligne d'aide, puis choisissez *Symétrie liée…*. L'autre moitié est créée :
ses points sont **maintenus** (ils suivent l'original et l'axe quand ils bougent ; le message « point maintenu par symétrie » s'affiche
si vous essayez de les déplacer), ses tracés sont parcourus en sens inverse, avec même rôle, même calque et crans reflétés. Un point posé
sur l'axe est partagé par les deux moitiés. Poignées de courbes et crans du côté miroir sont **copiés à la création** : ils ne suivent pas
les réglages faits ensuite sur l'original. *Déplier la symétrie* (axe ou point miroir sélectionné) rend les deux moitiés indépendantes ;
si elles se rejoignent sur l'axe, le contour fermé obtenu est signalé. Une seule étape annulable. Refusé sans rien modifier : axe non
marqué, copie d'un point déjà miroir, calque verrouillé.

**Décalage de contour** (outil *Décalage* `X`, menu Édition › Décaler un contour) : crée un tracé **parallèle** à une
chaîne de tracés bout à bout (segments, arcs, courbes de Bézier, courbes lisses ; chaîne ouverte ou contour fermé ; un
cercle se décale seul). Cliquez un tracé : la chaîne qui le contient est détectée par les points partagés (elle
s'arrête aux extrémités libres et aux embranchements) ; ou sélectionnez plusieurs tracés puis cliquez l'un d'eux. Tapez
la distance dans la palette (valeur positive), ou laissez-la suivre la souris ; **le côté suit la souris**, *Inverser*
prend l'autre côté. *Coins extérieurs* : *Pointe* (prolongement jusqu'à l'intersection ; si la pointe dépasse 4 fois la
distance, ou à un demi-tour, le coin est coupé en biseau) ou *Arrondi* (arc de raccord). Les coins intérieurs sont
toujours coupés à l'intersection, et si la courbure est plus forte que la distance les boucles sont supprimées : le
contour ne se retourne jamais. Le rôle du résultat se choisit dans la palette (*Construction* par défaut) ; il va dans
le calque actif (refusé si ce calque est verrouillé). Une seule étape annulable. Un décalage trop grand qui annulerait
le contour (petit contour décalé vers l'intérieur), une distance nulle, une chaîne non connexe ou ramifiée sont refusés
avec un message, sans rien modifier. **Le résultat est un nouveau tracé indépendant : il n'est pas recalculé si
l'original change** (ce n'est pas une contrainte). Les courbes sont approchées par des Béziers à moins de 0,00002 mm.
La distance au tracé d'origine est exactement la distance saisie (au centième de mm près en pratique : 0,00002 mm sur
les courbes), sauf sur les pointes et biseaux des coins extérieurs, qui sont par nature plus éloignés.

## 5. Trois sortes de valeurs

| Sorte | Exemple | Comportement |
|---|---|---|
| Mesure observée | longueur, angle, longueur développée | Lecture seule, ne verrouille rien |
| Paramètre de création | « 120 mm à 0° » d'un segment | Mémorisé pour information, **non maintenu** si un point bouge |
| Paramètre de construction | distance d'un point placé sur un segment | Maintenu : le point suit son segment |

Il n'y a **pas** de solveur de contraintes général. Seuls les liens explicites sont maintenus : constructions que vous
choisissez de garder, formules, longueur imposée d'une courbe. Rien d'autre ne suit automatiquement. Supprimer le parent d'un point construit le « fige » (une remarque s'affiche).

## 6. Rôles des tracés

Construction, contour de coupe, contour de couture, droit-fil, pli, axe, puis les lignes spéciales
(surpiqûre, milieu devant, milieu dos, ligne de taille, de hanche, de poitrine, de parage, de collage) :
un rôle se distingue par la couleur **et** par le motif et l'épaisseur du trait (pas la couleur seule),
à l'écran comme à l'export ; deux rôles n'ont jamais le même trait. Le rôle se choisit dans la liste
« RÔLE DU TRACÉ » de l'inspecteur (une étape annulable ; refusé si le calque du tracé est verrouillé). Les
contours fermés simples donnent périmètre et aire dans l'inspecteur (lecture seule). Voir aussi
« Lignes spéciales et surpiqûre liée » plus bas.

## 7. Enregistrer, annuler

`Ctrl+S` / `Ctrl+Maj+S` enregistrent (écriture atomique : un plantage ne détruit pas le fichier),
`Ctrl+O` ouvre, `Ctrl+N` crée. `Ctrl+Z` / `Ctrl+Y` annulent et rétablissent ; un geste complet est
une seule étape. Un fichier refusé (version inconnue, référence manquante) s'ouvre en erreur
claire, sans perte silencieuse.

**Sauvegarde automatique.** Toutes les 60 secondes (si le projet a changé), une copie de secours est
écrite à côté du fichier : `monprojet.pforge.autosave`. Un projet jamais enregistré est copié
dans `%APPDATA%\PatternForge\recovery\`. Votre fichier n'est jamais écrasé par cette copie ;
`Ctrl+S` la supprime. Après un arrêt brutal, à l'ouverture du projet (ou au démarrage pour un projet
jamais enregistré), une boîte propose **Récupérer** (le projet revient comme modifié : enregistrez
pour le conserver) ou **Ignorer** (la copie est supprimée). Une copie illisible n'est pas
supprimée : elle est renommée en `.corrupt` et un message l'indique. L'intervalle se règle avec la
clé `autosave_seconds` de `settings.json` (`0` = désactivée).

## 8. Exporter

Fichier › Exporter en **SVG** (millimètres, dimensions exactes) ou en **PDF** (1:1, pièces plus
grandes que la zone imprimable découpées en pages avec recouvrement de 10 mm et repères `L#C#`).
Par défaut le PDF est en **A4 portrait, marges de 10 mm**. La page 1 est une page d'étalonnage :
carré de 100 mm, règle, légende.

### Format de page, orientation et marges (PDF)

Fichier › **Mise en page du PDF…** règle le papier ; le réglage est mémorisé **sur ce poste**
(préférences de l'application, pas dans le projet : un autre poste ou une autre imprimante peut en
avoir un autre) et sert à l'export de la feuille, de toutes les feuilles, et à l'étalon.

- **Format** : A4 (défaut), A3, A0, Letter, **Format libre** (largeur × hauteur, de 190 à 5000 mm
  chacune) ou **Rouleau** (largeur de 190 à 5000 mm).
- **Orientation** : portrait ou paysage (largeur et hauteur échangées ; pour un format libre, les
  valeurs saisies sont échangées). **Sans effet pour un rouleau**, le contrôle est grisé.
- **Marges** : haut, bas, gauche, droite, de 0 à 100 mm : la zone que votre imprimante ne peut pas
  imprimer. La zone imprimable restante doit faire au moins 160 mm de large et, hors rouleau,
  170 mm de haut (le carré étalon de 100 mm doit toujours tenir). **Saisissez les marges réelles de
  votre imprimante** : PatternForge ne les lit pas dans le pilote.
- **Aperçu** : taille de la page, zone imprimable, nombre de pages de la feuille courante (étalon
  compris) ; pour un rouleau, la longueur de la page. Une saisie invalide est expliquée en rouge et
  le bouton « Appliquer la mise en page » reste inactif ; rien n'est appliqué avant ce bouton.
- **L'échelle reste toujours 1:1** : un patron plus grand que la zone imprimable est **découpé en
  pages** (recouvrement de 10 mm), jamais réduit. Un **rouleau** reçoit chaque feuille en **une seule
  page continue** (longueur = emprise du patron arrondie à 10 mm, plus le bandeau et les marges) ; si
  le patron est plus large que le rouleau ou plus long que 20 000 mm, l'export est **refusé** (jamais
  réduit ni tronqué).
- Avec un réglage autre que A4 portrait / 10 mm, la légende de la page d'étalonnage peut continuer
  sur une **page de suite** (en A4 paysage, par exemple).

### Assemblage des pages

Dans la même fenêtre, le bloc **Assemblage des pages** règle le découpage d'un grand patron. Tout est
mémorisé sur ce poste, comme le format ; rien n'est ajouté au projet. Par défaut (recouvrement de
10 mm, cases décochées) le PDF est **identique à celui d'avant**.

- **Recouvrement (mm)**, de 0 à 50 : largeur de la bande que deux pages voisines ont en commun. Un
  recouvrement plus grand demande au moins autant de pages. Hors de 0 à 50, ou pas un nombre : la
  saisie est expliquée en rouge, « Appliquer » reste inactif et rien n'est enregistré.
- **Repères d'assemblage** : dans chaque bande de recouvrement, quatre petites **croix** ; en posant
  une page sur sa voisine, les croix doivent se superposer exactement. Au milieu de chaque bord, une
  flèche et l'identifiant de la page voisine (« L1C3 p.5 » : ligne 1, colonne 3, page 5 du fichier).
  Chaque page porte aussi son identifiant (« L1C2 sur 2×4 ») et son rang (« page 4/10 »).
- **Plan d'assemblage** : une page ajoutée **après la page d'étalonnage et avant les pages de patron**.
  C'est une **miniature de la grille de pages** (cases numérotées, haut du patron, emprise) ; elle
  n'est **pas à l'échelle 1:1** et le dit. Les pages de patron restent à 100 %. Sans effet pour un
  rouleau.
- **Économiser le papier** : les pages **sans aucun trait** sont omises (les autres gardent leur
  identifiant d'origine : L1C2 omise, L2C2 reste L2C2 ; le plan les signale). De plus, si cela réduit
  le nombre de pages, la feuille peut être **tournée de 90°, 180° ou 270° dans le PDF seulement** :
  votre patron n'est jamais modifié. La rotation est refusée dès qu'un droit-fil dépasserait la
  tolérance de rotation de sa pièce (une pièce sans droit-fil peut tourner librement ; en cas de
  doute, PatternForge ne tourne pas). Une feuille tournée est indiquée sur chaque page.
- L'aperçu de la fenêtre donne le **nombre de pages avant l'export**, plan compris, avec le nombre de
  pages vides omises et la rotation retenue.

Ce que cette version ne fait pas : les formats A5, A2, A1 ; rotation pièce par pièce d'une planche. **L'échelle
réellement obtenue sur papier n'est pas validée**, pas plus que la superposition des repères :
imprimez l'étalon dans le format choisi et mesurez-le (ci-dessous).

### Vérifier l'impression

1. Exporter « l'étalon 100 mm » (ou ouvrir `examples/etalon-100mm.pdf`) avec le format de page que
   vous comptez utiliser.
2. Imprimer à **100 %** (jamais « ajuster à la page »).
3. Mesurer le carré au réglet : 100 mm attendus.

Tant que cette mesure n'est pas faite sur votre imprimante, ne coupez pas de matière sur la foi de
l'export.

### Étalonner l'imprimante (correction d'échelle X / Y)

Si votre imprimante rétrécit ou étire un peu le dessin (le carré de 100 mm mesure par exemple 99,5 mm),
vous pouvez le **corriger** : le PDF est alors dessiné à l'échelle inverse pour que le papier sorte à la
bonne taille. Cela se règle **par imprimante** et se mémorise **sur ce poste** (préférences, pas dans le
projet).

1. Exportez et imprimez la page d'étalonnage (**sans correction** : aucun profil actif), à 100 %.
2. Mesurez au réglet les côtés du carré : **X** = horizontal, **Y** = vertical de la page imprimée.
3. Fichier › **Mise en page du PDF…** › **Étalonner l'imprimante…** : donnez un **nom** au profil
   (40 caractères au plus, sans caractère de contrôle, un nom par profil, 16 profils au plus) et saisissez
   les deux mesures en mm (à 0,01 près). L'assistant affiche les **facteurs avant tout enregistrement** :
   facteur = 100 / mesure (99,5 mm → 1,005025). Un facteur doit rester entre 0,9 et 1,1 (mesure entre
   90,91 et 111,11 mm) ; sinon un message l'explique, le bouton reste inactif et rien n'est enregistré.
4. **Enregistrer le profil**, puis **Activer**. La liste permet aussi de **Modifier** (renommer ou nouvelle
   mesure), **Désactiver** et **Supprimer**. Ces actions sont enregistrées tout de suite.
5. Avec un profil actif, tout le patron est mis à l'échelle des facteurs ; une page de papier couvre donc un
   peu plus ou un peu moins de patron, et le **nombre de pages** affiché dans la fenêtre en tient compte.
   Chaque page de patron indique la correction (« correction d'échelle X ×… Y ×… »).
6. **Vérifiez sur papier** : réimprimez l'étalon et mesurez à nouveau. Le carré **corrigé** doit mesurer
   100 mm ; si ce n'est pas le cas, refaites la mesure sur les **règles non corrigées** (étalon renforcé,
   ci-dessous) pour calculer un nouveau facteur.

La case **Étalon renforcé** (activée d'office quand un profil est actif) ajoute sur l'étalon deux règles
graduées (jusqu'à 200 mm : 160 mm en horizontal sur un A4) et, sur chaque page de patron, deux règles de
100 mm le long des bords bas et gauche de la fenêtre. Ces règles sont **toujours en mm papier réels, non
corrigés** : elles servent à mesurer l'imprimante même quand une correction est active. Le carré de 100 mm
de l'étalon, lui, est corrigé. Les facteurs sont exprimés dans les axes de la page imprimée : un profil
mesuré en portrait vaut pour le portrait.

**Ce que le logiciel ne sait pas** : il ne dit jamais que l'imprimante est « calibrée ». Le facteur n'est
qu'un réglage calculé d'après **votre** mesure ; **vérifiez-le par mesure au réglet sur le papier
imprimé**, comme au point 6. Cette correction n'existe que pour le PDF (pas pour le SVG).

### Contenu de la sortie (ce qui est écrit dans le PDF et le SVG)

Fichier › **Mise en page du PDF…** › section **Contenu de la sortie** (repliée tant que tout est inclus ;
elle s'ouvre d'elle-même quand vous avez exclu quelque chose). Chaque case porte le **nombre d'éléments de
la feuille courante** : « Contours de coupe (4) ». Décochez ce que vous ne voulez pas dans la sortie,
puis **Appliquer la mise en page** : le choix vaut pour **toutes les feuilles**, pour le PDF **et** le SVG,
et se mémorise **sur ce poste** (préférences, pas dans le projet : rien à annuler, le fichier du projet
ne change pas).

- **Contours** : coupe, couture, droits-fils. « **Couture seule** » et « **Coupe seule** » sont des
  raccourcis. Attention : le « contour de coupe » est le **tracé que vous avez dessiné avec le rôle
  coupe**. Le contour de coupe d'une pièce à marge de couture, lui, est calculé depuis sa fiche. Si la feuille n'a aucun tracé de coupe, « coupe seule » n'en invente pas, et une remarque vous le dit.
- **Lignes** : tracés de construction, pliures et lignes spéciales (surpiqûre, milieux, taille, hanche,
  poitrine, parage, collage), axes de repère et lignes d'aide.
- **Repères et marquages** : crans, perçages / pointages / croix, boutons et boutonnières, points, noms
  des points (ils ne s'écrivent que si les points le sont).
- **Cotes** : cotations (mesures simples) et cotes complètes, séparément.
- **Taille** : la taille exportée se choisit en haut de « Mise en page du PDF » (et des fenêtres DXF
  et HPGL) ; la taille de base par défaut. Fichier › « Exporter des tailles… » écrit un fichier par
  taille cochée ; le nid de tailles s'exporte depuis « Nid de tailles… » (SVG ou PDF).

Ce qui ne change pas : l'**échelle 1:1**, la **page étalon** (carré de 100 mm, règles, consignes) et les
repères d'assemblage ne sont jamais filtrés ; la **légende de l'étalon** n'annonce pas un type de trait
exclu. Le découpage en pages se fait sur ce qui est **écrit** : si l'élément exclu dépassait du patron
(une construction éloignée, par exemple), la grille de pages se resserre sur le reste et le patron se
retrouve décalé sur ses pages ; sinon pages et coordonnées sont celles de l'export complet. Le
droit-fil exclu **continue** de limiter la rotation de « Économiser le papier ».

L'aperçu de la fenêtre (nombre de pages, « contenu retenu : n sur m ») utilise **exactement la même
construction** que l'export, mais ce n'est **pas un rendu papier** : c'est la liste de ce qui sera écrit.
Si votre choix ne laisse **rien** à écrire sur une feuille, l'export est refusé avec un message, aucun
fichier n'est créé (« Exporter toutes les feuilles » saute les feuilles dans ce cas). Aucune impression
n'a été essayée : mesurez l'étalon comme ci-dessus avant de vous fier à une sortie.

### Exporter les pièces en DXF (type AAMA / ASTM)

Fichier › **Exporter les pièces en DXF de type AAMA / ASTM…** (en bas du menu, avant « Quitter »). C'est
un export **de type AAMA / ASTM D6673** : **l'ouverture dans un logiciel métier n'a pas été vérifiée**,
et la table des couches reste à confirmer avec la spécification officielle. Ne l'utilisez pas comme un
fichier « validé » : essayez-le d'abord dans votre logiciel et dites-nous ce qui ne va pas.

- **Ce qui est écrit** : toutes les pièces du projet (liées, détachées, dérivées, dépliées), de **la taille
  unique** du projet, en **millimètres, échelle 1:1**. Une pièce = un bloc, placée comme dans le plan de
  pièces (placement manuel et rotation conservés, sinon rangées sans chevauchement). Contour fermé (courbes
  découpées en segments qui s'écartent de la courbe de 0,001 mm au plus), crans, droit-fil avec ses
  pointes, perçages, pointages, croix, boutons, boutonnières, éléments internes, et un texte (nom, code,
  quantité, matière, paire).
- **Couches** : 1 contour de pièce · 4 crans · 6 droit-fil · 7 lignes internes · 8 découpes internes ·
  13 perçages, marques et boutons · 15 ligne de couture · 84 texte. Les couches 2 et 3 (points de
  rotation et de courbe) et 10 (ligne de miroir) sont déclarées mais **restent vides**.
- **Contour de coupe** : pour une pièce à marge de couture, le contour dessiné est la couture (couche 15) et la coupe
  calculée va sur la couche 1. Sans marge de couture, le contour est écrit **tel que vous l'avez dessiné**. S'il a le rôle « contour de coupe » il va sur la couche 1 ;
  s'il a le rôle « ligne de couture » il va **seulement** sur la couche 15 (aucun contour de coupe n'est
  écrit, la fenêtre le dit) ; sinon il va sur la couche 1 avec une remarque.
- **Aucun fichier de règles de gradation (`.rul`)**.
- **Rapport avant l'écriture** : la fenêtre montre le nombre de pièces, de couches et d'entités, puis
  **ce qui n'est pas exporté** (pièce sans contour, contour non fermé ou plat, éléments illisibles, texte
  tronqué à 80 caractères) et des remarques. Rien n'est perdu sans que la fenêtre le dise.
  « Enregistrer le DXF… » ouvre la boîte d'enregistrement. Sans pièce, ou si aucune pièce ne peut être
  écrite, l'export est refusé et **aucun fichier n'est créé**. Le projet n'est jamais modifié.

### Importer un DXF et exporter la construction en DXF (DXF générique)

Deux entrées en bas du menu Fichier (sans raccourci) : **Importer un DXF…** et **Exporter la construction en
DXF…**. Ce sont des échanges **génériques** de géométrie de construction, distincts de l'export de pièces
AAMA / ASTM ci-dessus. **L'import n'a été essayé que sur des fichiers écrits à la main pour les tests et sur
l'export de PatternForge : pas sur des fichiers de logiciels réels** ; l'ouverture de l'export dans un
logiciel tiers n'a pas été vérifiée. Ne le tenez pas pour « compatible » avant d'avoir essayé.

**Importer un DXF…** (DXF **texte** seulement ; un DXF binaire est refusé, enregistrez-le en DXF texte) :

1. Choisissez le fichier. La fenêtre « Importer un DXF » s'ouvre : rien n'est encore modifié.
2. **Unité** : si le fichier déclare une unité reconnue (pouce, pied, mm, cm, dm, m, yard), elle est proposée.
   Sinon (aucune unité, « sans unité », unité exotique) **vous devez choisir** : millimètre, centimètre,
   mètre, pouce (25,4 mm) ou pied (304,8 mm). Tant qu'aucune unité n'est choisie, il n'y a pas d'aperçu et
   le bouton d'import est grisé.
3. **Aperçu avant import** : dimensions en mm de ce qui sera créé (boîte englobante), facteur appliqué,
   nombre d'objets, entités importées par type, **entités ignorées avec leur nombre et leur motif** (texte,
   hachures, cotes, blocs `INSERT`, polylignes 3D, valeurs illisibles, etc.), remarques et correspondance
   couches → calques. Changer d'unité recalcule l'aperçu ; ce que vous voyez est ce qui sera créé.
4. Choisissez la **feuille cible** (la feuille courante par défaut) puis **Importer dans la feuille**.

Ce qui devient quoi : lignes, polylignes (les arcs de polyligne sont conservés), arcs, cercles et points
deviennent des tracés et points **ordinaires** (rôle « construction », points libres) : aucune pièce ni
contrainte n'est créée. Une courbe `SPLINE` à points de contrôle est **convertie en segments** (écart de
0,005 mm au plus) : sa forme est exacte à cette tolérance mais ce n'est pas une courbe modifiable ; une
`SPLINE` à points d'ajustement seulement devient une courbe lisse passant par ces points (la forme exacte du
logiciel d'origine n'est pas reproduite). Les coordonnées Z sont ignorées (signalé). Chaque **couche** du
fichier devient un **calque** de même nom (la couche « 0 » = calque par défaut). **Les blocs ne sont pas
dépliés** : un DXF de pièces par blocs (comme celui de l'export AAMA / ASTM) s'importe vide, le rapport le dit.

Tout l'import est **une seule étape** : un `Ctrl+Z` le défait. Un fichier tronqué, illisible, trop gros
(plus de 50 Mo ou 200 000 entités), ou qui créerait plus de **200 000 objets** est refusé avec un message ; si
la commande est refusée (par exemple calque verrouillé du même nom) le projet reste strictement inchangé et
la fenêtre reste ouverte.

**Exporter la construction en DXF…** : écrit les tracés de la **feuille courante** (ou de **toutes les
feuilles**, côte à côte à 100 mm d'écart) en DXF R12 texte, en **millimètres, échelle 1:1** : segments
(`LINE`), arcs (`ARC`), cercles (`CIRCLE`), points isolés (`POINT`), courbes écrites en polylignes (écart
0,005 mm au plus). Une couche par calque (« 0 » pour le calque par défaut) ; un tracé dont le rôle n'est pas
« construction » est sur la couche `<calque>.<rôle>` (par exemple `0.cutting`). La fenêtre montre le rapport
**avant** l'écriture, dont ce qui **n'est pas écrit** (crans, cotations, cotes, lignes d'aide, groupes) ; une
feuille vide est refusée, aucun fichier n'est créé. Le projet n'est jamais modifié. Au réimport, le rôle
n'est pas restitué (la couche `0.cutting` devient un calque de ce nom).

### Exporter pour un traceur (HPGL)

Entrée du menu Fichier (sans raccourci) : **Exporter pour traceur (HPGL)…**. Elle écrit la feuille
courante en fichier texte **de type HPGL** (extension `.plt`), à l'échelle **1:1** (1 unité = 0,025 mm). **Ce
fichier n'a pas été essayé sur un vrai traceur ni dans un logiciel de découpe** : vérifiez l'échelle sur une
première feuille de contrôle (par exemple en mesurant un carré de 100 mm) avant tout tracé sur du papier de
patron.

- **Contenu et format** : ce que vous avez choisi dans « Contenu de la sortie » et « Mise en page du PDF » (une
  catégorie exclue n'est pas tracée). Sur une **feuille fixe** (A4, A3…), le tracé doit tenir dans la zone
  imprimable (page moins marges) ; sinon l'export est refusé et le message propose un **rouleau** ou un format
  plus grand. Sur un **rouleau**, la largeur imprimable est la limite, la longueur est celle du dessin. Le tracé
  n'est **jamais tourné ni réduit** : choisissez l'orientation paysage dans la mise en page si besoin. Les
  facteurs d'étalonnage X / Y du profil actif sont appliqués.
- **Plumes** (la fenêtre affiche la table et ce qui est utilisé) : 1 contour de coupe · 2 contour de couture ·
  3 construction · 4 droit-fil · 5 crans · 6 marquages, boutons et boutonnières · 7 traits des cotes et texte des étiquettes · 8 pliure,
  lignes spéciales et axes · 9 jambes des pinces. Seules les plumes utilisées sont sélectionnées, une fois chacune. Le fichier ne
  contient aucune pause de changement de plume : le traceur ou son logiciel décide.
- **Ordre de tracé** : par plume, puis de proche en proche depuis le coin bas-gauche pour réduire les
  déplacements plume levée ; la fenêtre donne la longueur à vide et celle de l'ordre naturel. L'origine est le
  coin bas-gauche du dessin.
- **Non tracé** : les textes des cotes, les noms de points et les repères de points (le nombre est affiché).
  Les courbes sont écrites en petits segments (écart 0,005 mm au plus) ; les coordonnées sont arrondies à
  0,025 mm (erreur au plus 0,0125 mm).

La fenêtre montre le rapport **avant** l'écriture ; un refus (rien à tracer, tracé trop grand, nombre non
fini) n'écrit aucun fichier, et le projet n'est jamais modifié.

### Sélection avancée

Menu `Édition › Sélection` :

- **Tout sélectionner** (`Ctrl+A`) et **Inverser la sélection** (`Ctrl+I`) : seuls les objets visibles et non verrouillés de la feuille
  active sont concernés (calque masqué ou verrouillé : jamais). Un tracé groupé entraîne son groupe.
- **Lasso** (`Ctrl+L` ou case « Sélection au lasso ») : avec l'outil Sélection, glissez sur le vide pour tracer une zone libre ; un objet est
  retenu s'il est **entièrement** dedans. `Maj` ajoute à la sélection ; `Alt` enfoncé au relâchement retient aussi les objets que la zone croise.
  Un contour qui se recoupe laisse des zones vides (règle pair-impair) ; une zone trop petite ou alignée ne sélectionne rien.
- **Par rôle, par calque, par type** : sélectionne tout ce qui correspond (`Maj` enfoncée en cliquant : ajoute à la sélection).
- **Sélectionner la chaîne** (`Ctrl+Maj+A`, ou double-clic sur un tracé) : tous les tracés bout à bout, jusqu'aux extrémités libres ou à un
  contour refermé. Si la chaîne se divise en un point, rien n'est sélectionné et un message l'explique.
- **Filtres de sélection** : décochez points, tracés, crans, cotations ou lignes d'aide pour les rendre inatteignables (clic, fenêtre, lasso,
  tout, inverser) ; ce qui était déjà sélectionné dans une catégorie décochée est désélectionné. Tout décocher ne laisse rien de sélectionnable.

Ces opérations ne modifient pas le projet : rien à annuler, et le titre n'affiche pas d'astérisque à cause d'elles.

### Saisie directe

Pendant qu'un outil attend des valeurs (segment, droit-fil, perpendiculaire, parallèle, point à distance, cercle, rectangle), une **bulle suit
le curseur** et montre les mêmes champs que la palette « Tracé précis » (c'est le même texte : ce que vous tapez apparaît aux deux endroits).
Un champ vide affiche la **mesure observée** (en italique, marquée « mesuré ») ; une valeur tapée est **surlignée et soulignée**, marquée « saisi ».
`Tab` passe d'un champ au suivant, `Entrée` valide, `Échap` (ou clic droit) annule sans rien laisser. Avant le premier point, la bulle montre un
champ « Position » : tapez les chiffres directement (sans cliquer dans un champ) puis `Entrée`.

Coordonnées (pour la position d'un premier point, ou dans le champ Longueur d'un segment) :

| Saisie | Sens |
|---|---|
| `20;10` | position **absolue** x = 20 mm, y = 10 mm (Y vers le haut) |
| `@30;-5` | **relative** au dernier point posé (ou au départ du segment en cours) : dx = 30, dy = −5 |
| `@30<45` | **polaire relative** : 30 mm à 45° (degrés, sens trigonométrique, 0° = vers la droite) |

Avec la virgule décimale (français) séparez x et y par `;` : `12,5;3`. En anglais (point décimal), `,` marche aussi pour les positions
(`12.5,3`) ; dans le champ Longueur d'un segment, `12,5` reste une longueur, une coordonnée y demande `;` ou `@`. Une coordonnée se saisit
seule : laissez le champ Angle vide. Les unités (`2 cm`) sont acceptées. Une saisie illisible, hors de l'espace de travail ou qui se confond
avec le départ est refusée avec un message, sans toucher au projet. Une coordonnée est un **paramètre de création** : elle n'est pas conservée
ensuite comme contrainte.

**Verrou d'angle** : `Maj` enfoncée pendant le tracé d'un segment (ou d'une ligne d'aide oblique) ramène la direction au multiple de 15° le plus
proche, depuis l'horizontale ; la longueur reste celle de la souris (ou celle que vous avez tapée). Un angle tapé l'emporte sur le verrou. Le verrou
l'emporte sur l'accrochage à un point existant : l'extrémité n'est alors pas rattachée à ce point (le repère d'accrochage disparaît).

**Répéter la dernière commande** : `Entrée`, quand l'outil Sélection est actif et qu'aucun champ n'a le clavier, ou Édition › Répéter la dernière
commande (grisé tant qu'aucun outil n'a servi). Cela **réactive le dernier outil** avec son sous-mode et les dernières valeurs tapées
(longueur, angle, rayon…) ; il reste à cliquer le départ puis `Entrée`. Rien n'est rejoué automatiquement. Si ces valeurs ne se lisent plus,
un message l'indique et rien ne change.

### Extraire une pièce

Outil **Extraire une pièce** (`Z`, ou Édition › Extraire une pièce d'une zone fermée). Cliquez **à l'intérieur d'une zone fermée** : la plus
petite région qui contient le clic devient une pièce. Les tracés qui la bordent n'ont pas besoin de se rejoindre en un point commun : deux
rectangles qui se croisent donnent une pièce pour la zone commune et une pièce en L pour chaque partie non commune. Un clic exactement sur un
tracé choisit la plus petite des régions voisines. Une forme entièrement à l'intérieur de la région sans toucher son bord (un bouton, un cercle de
repère) n'est pas un trou : la pièce garde son contour extérieur. Les extrémités qui sont à moins de 0,001 mm l'une de l'autre sont considérées
comme jointes.

- **Pièce liée (par défaut)** : le contour est formé de **vrais tracés** de la feuille. Là où un tracé n'est utilisé qu'en partie, il est **coupé aux
  croisements** (un cercle coupé devient des arcs ; les points construits sur un tracé coupé deviennent libres, avec une remarque ; une courbe lisse
  ne se coupe qu'en un de ses points de passage). La forme et la longueur totale des tracés ne changent pas. La pièce **suit la construction** : si
  vous déplacez un point du contour, son aire et son périmètre sont recalculés. Une seule étape d'annulation défait les coupes et la pièce.
- **Détachée** (case « Détachée » de la barre d'outils) : le contour est **copié dans une nouvelle feuille** portant le nom de la pièce ; la
  construction n'est pas touchée et la pièce ne la suit plus. Pour supprimer cette feuille, supprimez d'abord la pièce.
- **Aire, périmètre et sens du contour** sont des **mesures observées**, recalculées à chaque affichage et jamais enregistrées ; on les lit dans
  l'inspecteur de la feuille (une fiche par pièce : nom, quantité, matière, lien, mesures, « Supprimer la pièce »). Le contour d'une pièce extraite
  est anti-horaire.
- **Zone non fermée** : si un petit écart empêche la fermeture, le message donne l'écart en mm et les deux extrémités sont entourées et reliées par un
  trait pointillé avec la valeur de l'écart ; rien n'est créé. Hors de toute zone, le message est « Aucune région fermée à cet endroit ».
- **Choisir les tracés plutôt que cliquer** : sélectionnez des tracés qui forment exactement un contour fermé, puis Édition › « Créer une pièce depuis
  la sélection » (ou le bouton de l'inspecteur). Aucun tracé n'est coupé. Le bouton est grisé si les tracés ne forment pas un contour fermé.

Limites : aucun aperçu au survol (la région est calculée au clic) ; deux cercles, arcs ou courbes superposés sont refusés (supprimez le doublon) ;
une région dont le contour touche son propre bord en un point est refusée ; un cercle qui porte des crans ou qui est déjà le contour d'une pièce ne se
découpe pas. Ces pièces ne sont ni cotées, ni marquées d'un droit-fil, ni dotées de marges de couture : ce sont les stories suivantes.

### Fiche de pièce

L'inspecteur de la feuille (colonne de droite, sans sélection) montre une **fiche par pièce** :

- **Nom**, **Code** (référence de la pièce), **Quantité** (entier ≥ 1), case **Paire (×2 en miroir)**, **Matière**, **Catégorie** (tissu, doublure,
  entoilage, cuir, renfort ; « non renseignée » par défaut) et **Commentaire** (plusieurs lignes ; Entrée ajoute une ligne, la validation se fait en
  quittant le champ).
- Un champ se valide en quittant le champ ou par Entrée ; chaque champ validé est **une seule étape d'annulation** (Ctrl+Z). Une saisie refusée
  (quantité qui n'est pas un entier ≥ 1, texte de plus de 120 caractères — 2000 pour le commentaire —, caractère de contrôle, nom vide) affiche un
  message dans la barre d'état et le champ reprend sa valeur précédente. Un champ laissé vide est effacé.
- **Avertissements** : « ⚠ Avertissement : droit-fil manquant » tant que la pièce n'a pas de droit-fil ; aussi contour absent, ouvert ou qui se recoupe,
  droit-fil supprimé, code déjà porté par une autre pièce. Ce sont des **signalements** : la pièce reste enregistrable. Un résumé « n pièce(s) sur cette
  feuille, m avec avertissement(s) » compte les pièces concernées.
- **Assigner un droit-fil** : tracez-le avec l'outil Droit-fil (`G`), sélectionnez ce tracé, puis cliquez « Assigner le droit-fil sélectionné » dans la
  fiche de la pièce. Le bouton est grisé tant qu'un seul tracé de rôle droit-fil n'est pas sélectionné.

Limites : les catégories et le vocabulaire n'ont pas été relus par une modéliste ; la recherche (Ctrl+F) ne trouve pas encore les pièces ; copier-coller
des tracés ne copie pas la pièce (« Dupliquer la feuille » copie la fiche, et le code apparaît alors en double : signalé, pas refusé) ; ces champs ne sont
écrits dans l'**étiquette de pièce** des exports PDF, SVG, HPGL et DXF (case « Étiquettes de pièce » du contenu de la sortie ; rien n'est écrit tant que la fiche n'a que son nom) ; les **jambes des pinces** sont exportées aussi (case « Pinces (jambes) »), pas la ligne pliée ni le capuchon.

### Éléments internes

Une pièce peut porter, en plus de son contour, des **éléments internes** : des **lignes** (pli, surpiqûre, position de poche, autre) et des **repères**
(marque, boutonnière, autre). Ce sont des **liens** vers des tracés, des points ou des crans de la construction, pas des copies : si vous déplacez un
point de la construction, la pièce le voit. Dans l'inspecteur de la feuille, chaque pièce a une section **Éléments internes** :

- **Importer depuis la construction** : ajoute d'un coup les tracés, points libres (qui ne servent d'extrémité à aucun tracé) et crans de la feuille qui
  sont **entièrement dans le contour** de la pièce. Un tracé de rôle pliure devient un « pli », les autres « autre » ; les repères sont des « marques ».
  Le **rapport** s'affiche dessous : ajoutés, **ignorés avec leur raison** (hors du contour, tracé du contour, droit-fil, ligne de couture ou de coupe,
  contour d'une autre pièce, cran du contour) et déjà présents. Relancer l'import n'ajoute jamais de doublon. Une seule étape d'annulation. Il faut un
  contour fermé. Un tracé dont l'extrémité est sur le contour à une fraction de micromètre près peut être jugé « hors du contour » : ajoutez-le alors
  avec le bouton suivant.
- **Ajouter la sélection comme élément interne** : sélectionnez des tracés, points ou crans de la feuille ; le bouton est grisé si la sélection est
  vide ou contient le contour de la pièce, son droit-fil ou un élément déjà interne.
- Chaque ligne donne le nom, la longueur (tracé) ou la position (point, cran), une **liste déroulante du type** et **Retirer** (la construction n'est
  pas touchée). Chaque action est une étape d'annulation.
- Sur le canevas, les éléments internes des pièces de la feuille sont dessinés **par-dessus** la construction, avec un motif de trait propre au type
  (pli : tiret-point, surpiqûre : pointillé serré, position de poche : tirets, autre : points espacés) **et** son nom écrit ; les repères sont un cercle
  à croix (marque), un rectangle (boutonnière) ou un losange (autre).
- Supprimer un tracé, un point ou un cran qui est élément interne le **retire** de la pièce dans la même étape (annulable), avec une remarque. Si la
  construction change et qu'un élément sort du contour, la fiche affiche « ⚠ Avertissement : n élément(s) interne(s) ne sont plus entièrement dans le
  contour » : la pièce reste enregistrable. « Dupliquer la feuille » copie les éléments internes.

Limites : types (pli, surpiqûre, position de poche, marque, boutonnière) **non relus par une modéliste** ; « position de poche » n'existe que pour une
ligne ; aucun export (PDF, SVG, DXF) ne les écrit encore ; le dessin vaut pour toutes les pièces de la feuille (pas de « pièce sélectionnée ») et n'a pas
été vu dans la vraie fenêtre.

### Plan de pièces

Le **plan de pièces** (menu **Affichage › Plan de pièces**) montre **toutes les pièces du projet à plat**, avec leur nom, à la place de la feuille. C'est une **mise
en page** : déplacer ou tourner une pièce dans le plan **ne modifie jamais la construction** (tracés, points, crans, cotations, aire et périmètre restent identiques).

- **Déplacer** : glissez une pièce à la souris. Un glisser = **une seule étape d'annulation** ; **Échap** ou **clic droit** pendant le glisser l'annule sans laisser de
  trace. Un clic sélectionne une pièce, un clic dans le vide désélectionne (Échap aussi).
- **Tourner** : panneau de droite, champ **Rotation (°)** (sens trigonométrique) ou boutons **−90° −15° +15° +90°** ; les champs **Centre X / Centre Y** (mm) placent
  le **centre de la boîte englobante** de la pièce. Chaque saisie ou bouton = une étape d'annulation. La rotation est enregistrée entre 0 et 360°.
- **Placement automatique** : tant que vous n'avez pas déplacé une pièce, le plan la range seul (rangées, 20 mm d'écart, 2000 mm de large) ; **rien n'est enregistré dans le
  fichier**. Dès que vous la déplacez ou la tournez, son placement est enregistré dans le projet. **Replacer automatiquement** supprime ce placement (annulable). Quand
  vous ajoutez, supprimez ou changez la taille d'une pièce, les pièces restées en automatique peuvent se décaler. Une feuille dupliquée donne des pièces en automatique.
- **Liste des pièces** (en bas) : nom, code, catégorie, quantité, aire, périmètre, « placée à la main », avertissements. Un clic sur un **en-tête** trie (croissant ▲,
  puis décroissant ▼) ; un clic sur une **ligne** sélectionne la pièce et la cadre. **Tout voir** cadre tout le plan. Les avertissements de la fiche s'affichent avec
  « ⚠ » dans la liste, sur le dessin et dans le panneau.
- **Chevauchement** : si les **boîtes englobantes** de deux pièces se recouvrent, un cadre pointillé et une phrase d'information le signalent. Ce n'est **pas une
  détection exacte** des recouvrements (deux contours peuvent avoir des boîtes qui se recouvrent sans se toucher) et rien n'est refusé.
- Dans le plan, les outils et raccourcis de la feuille (Suppr, copier-coller, sélection…) sont **inactifs** ; **Retour à la feuille** (ou un clic sur une feuille) rend la feuille.

Limites : pas d'aimantation aux bords ni de rotation à la souris ; le droit-fil et les crans ne sont **pas dessinés** dans le plan ; ce plan ne s'exporte pas ; c'est une aide de mise en page : pour placer sur tissu ou sur peau, utilisez **Fichier › Plan de coupe…** ; le rendu n'a pas été vu dans la vraie fenêtre.

### Pièces dérivées

Une **pièce dérivée** (parementure, doublure, entoilage ou renfort) est une pièce créée **à partir d'une autre**, la **pièce mère**, par **décalage de son
contour**. Elle y reste **liée** : quand vous modifiez la mère, la dérivée est **recalculée** (ce n'est pas un solveur ni une contrainte : un recalcul,
dans la même étape d'annulation que votre modification).

- **Créer** : dans l'inspecteur de la feuille, sous la pièce mère, **Créer une pièce dérivée…**. Choisissez le **type**, le **décalage** en mm (**positif =
  vers l'extérieur, négatif = vers l'intérieur** ; entre 0,01 et 100 000 mm) et, si vous voulez, un nom (par défaut « Mère – doublure »). La dérivée a sa
  **propre feuille** (créée avec elle) ; la fiche (code, quantité, catégorie…) se remplit comme pour toute pièce. Le contour de la mère doit être fermé,
  lisible et ne pas se recouper ; le décalage doit donner un contour fermé d'un seul tenant (une forme qui disparaît ou se coupe en deux est refusée avec
  un message). Une seule étape d'annulation défait tout.
- **Suivre la mère** : déplacez un point de la mère, transformez-la, changez son contour : la dérivée est recalculée, et la liste « Pièces dérivées » sous
  la mère affiche son aire à jour. Annuler la modification de la mère annule aussi la mise à jour. Les tracés de la dérivée gardent leurs identifiants tant
  que la forme garde la même structure (même nombre et même type de tracés) ; si elle change (un coin devient un biseau…), les tracés sont recréés et une
  remarque l'écrit.
- **Obsolète** : si le recalcul est impossible (la mère se recoupe, n'a plus de contour, la forme disparaît…), votre modification de la mère **n'est pas
  refusée** : la dérivée garde sa **dernière géométrie valide** et l'inspecteur écrit « État : ⚠ obsolète — raison ». Elle est recalculée dès que la mère
  redevient utilisable.
- **Inspecteur d'une dérivée** : « Dérivée de « Mère » (doublure, -10,0 mm) », son état (« à jour » ou « ⚠ obsolète ») et **Casser le lien**.
  La géométrie d'une dérivée liée **ne se modifie pas directement** (la commande est refusée avec « cassez le lien avant de la modifier ») ; la fiche, le
  droit-fil, les éléments internes et les crans restent modifiables.
- **Casser le lien** : la pièce devient une pièce ordinaire (sa géométrie reste, plus aucun recalcul), annulable. Supprimer la mère (ou sa feuille) casse
  aussi le lien de ses dérivées, avec une remarque. Dupliquer la feuille d'une dérivée, ou importer une feuille, ne copie **jamais** le lien.
- Une dérivée peut être la mère d'une autre (8 niveaux au plus).

Limites : le **vocabulaire** (parementure, doublure, entoilage, renfort) **n'a pas été relu par une modéliste** ; seul le **décalage** du contour est
livré (coins en pointe, pas de coins arrondis au choix) : la **bande de parementure** limitée par une ligne de découpe n'existe pas encore ; pas de marge de couture sur la dérivée ; les raisons d'obsolescence viennent du cœur et sont écrites en français ; aucun export ne les écrit encore ; le rendu n'a pas été vu dans
la vraie fenêtre.

### Séparer et assembler des pièces

**Séparer une pièce** par une ligne donne **deux pièces**. Menu **Édition › Séparer une pièce par une ligne** (choisissez la pièce), ou le bouton **Séparer…** de la fiche de la pièce.
Cliquez le **premier point** puis le **second** de la ligne de coupe : c'est une **droite infinie** (elle ne s'arrête pas au second point). Avant le second clic, le dessin montre la
droite, les deux points où elle traverse le contour et l'**aire de chaque moitié** (mesures observées) ; **Échap** abandonne.

- Le résultat : la pièce d'origine est **remplacée** par « Nom (1/2) » (à gauche de la ligne, dans le sens de vos deux clics) et « Nom (2/2) », deux **pièces détachées** qui ont chacune **leur
  propre feuille**. La construction n'est **jamais modifiée**. Matière, catégorie, quantité, commentaire et paire en miroir sont repris ; le code devient « CODE-1 » et « CODE-2 ». Une seule étape
  d'annulation défait tout. La somme des aires des deux moitiés est celle de la pièce d'origine.
- **Droit-fil et éléments internes** (lignes, repères) : copiés dans la moitié qui les **contient entièrement** ; l'autre moitié affiche alors « droit-fil manquant ». Ce qui traverse la
  ligne n'est repris par aucune moitié (une remarque le dit). Les **crans** ne sont **pas repris**. Le placement manuel dans le plan n'est pas repris. Les pièces dérivées de la pièce
  coupée deviennent des pièces ordinaires (lien cassé, avec remarque).
- **Refusé avec un message, sans rien modifier** : la ligne ne traverse pas le contour, ne fait que le toucher ou le longer, ou le traverse en **plus de deux points** (pièce très concave :
  seuls deux morceaux sont pris en charge) ; deux points confondus ; pièce dérivée encore liée (« cassez le lien ») ; contour ouvert ou qui se recoupe.

**Assembler deux pièces** : menu **Édition › Assembler deux pièces…** (ou le bouton **Assembler…** de la fiche). Choisissez les deux pièces, **un bord de chacune** dans la liste et la
**tolérance** (0,5 mm par défaut, de 0 à 10 mm). La **seconde pièce est déplacée sans déformation** (rotation et translation, pas de symétrie) pour que son bord coïncide avec celui de la
première, bord retourné ; le bord commun disparaît du contour. La fenêtre montre un petit dessin (pièce 1 en trait plein, pièce 2 déplacée en pointillé, bord assemblé en trait épais), les
**longueurs des deux bords**, l'**écart de longueur** et l'**écart de forme** avec la tolérance — l'écart est **toujours affiché**, jamais masqué — puis l'aire de la pièce obtenue.

- **Accepté** si l'écart de longueur et l'écart de forme (pour un bord courbe : les deux courbes doivent coïncider) sont dans la tolérance ; **refusé** sinon (raison écrite en rouge, bouton
  grisé), ainsi que si les deux pièces se recouvrent une fois assemblées, si un bord est le seul bord de sa pièce (un cercle), ou si une pièce est une dérivée encore liée.
- Le résultat remplace les **deux** pièces par une seule, « A + B » (fiche de la première conservée ; les différences de la seconde sont notées) sur sa propre feuille ; droit-fil et éléments
  internes sont repris (ceux de la seconde déplacés avec elle). Une seule étape d'annulation. Les pièces sont lues **telles que construites** (le placement du plan n'intervient pas).
- **Séparer puis assembler** les deux moitiés par leur bord de coupe retrouve l'aire d'origine.

Limites : **pas de découpe en plus de deux morceaux** ; la ligne est une droite infinie ; **aucune marge de couture n'est recalculée** pour les pièces créées ; les bords se choisissent dans
une liste (pas au clic) ; le recouvrement n'est détecté que si les contours se croisent ; les messages de refus viennent du cœur et sont écrits en français ; **le rendu et les gestes à la souris n'ont
été exercés que par des tests automatiques, jamais vus dans la vraie fenêtre**.

### Déplier une pièce construite sur une pliure

Une **moitié** dessinée contre sa pliure (le bord de la pliure est un **segment droit du contour** de la pièce) devient une **pièce entière symétrique** : menu **Édition › Déplier une pièce sur
sa pliure…** (tout en bas du menu) ou le bouton **Déplier…** de la fiche de la pièce. Choisissez la **pièce** et la **pliure** (liste des segments du contour avec leur longueur), puis un nom si
vous ne voulez pas « Nom – dépliée ». La fenêtre montre un petit dessin (la moitié en trait plein, la pièce dépliée en pointillés, la pliure en trait épais), l'**aire et le périmètre observés** de
la moitié et de la pièce entière, et la raison d'un refus éventuel (bouton grisé).

- Le résultat : une **nouvelle pièce** sur **sa propre feuille**, dont le contour est la moitié suivie de son **image en miroir** par rapport à la pliure, en un seul contour fermé ; la pliure disparaît,
  les deux moitiés se rejoignent exactement à ses extrémités. L'aire est le **double** de celle de la moitié, le périmètre est 2 × (périmètre de la moitié − longueur de la pliure). Quantité,
  matière et catégorie sont reprises, le code devient « CODE-D » ; le commentaire n'est pas repris et la pièce n'est pas une « paire ». La construction n'est **jamais modifiée**. Une seule étape
  d'annulation.
- La pièce dépliée reste **liée à la moitié** : si vous modifiez la moitié (déplacer un point, un contour…), la pièce dépliée est **recalculée dans la même étape**. Si le recalcul est
  impossible (par exemple un sommet passe de l'autre côté de la pliure), votre modification **n'est pas refusée** : la pièce dépliée garde sa dernière forme valide et affiche « ⚠ obsolète » avec la
  raison. Sa géométrie n'est pas modifiable directement : **Casser le lien** la rend ordinaire. Une copie de feuille ne copie jamais le lien.
- **Refusé avec un message, sans rien modifier** : le contour traverse la droite de la pliure (toute la moitié doit être d'un seul côté, à 1e-6 mm près) ou la touche ailleurs qu'à ses extrémités ;
  la pliure n'est pas un segment droit (courbe, arc, cercle) ou n'est pas dans le contour ; contour ouvert ou qui se recoupe ; cette pliure est déjà dépliée ; la pièce est elle-même une dérivée
  encore liée (« cassez le lien »).

Limites : le **droit-fil**, les **éléments internes** et les **crans** de la moitié ne sont **pas repris** sur la pièce dépliée (une remarque le dit, la pièce affiche « droit-fil manquant ») ; pliure
rectiligne seulement et une seule pliure à la fois ; **aucune marge de couture n'est recalculée** pour les pièces créées ; la quantité reprise telle quelle, le mot « dépliée » et
l'absence de « paire » sont à faire relire par une modéliste ; le rendu et la fenêtre n'ont été exercés que par des tests automatiques, **jamais vus dans la vraie fenêtre**.

### Perçages et pointages

Une pièce peut porter des **marquages intérieurs**, exportés en PDF et en SVG comme un **groupe à part** (reconnaissables par leur **forme**, pas par la couleur) : un **pointage** (poinçon : un petit
disque plein de 1 mm), un **perçage** (un cercle dont vous donnez le **diamètre**, de 0,1 à 100 mm) ou une **croix** « + » (côté de 0,5 à 100 mm). Ce ne sont pas des éléments internes : la « marque »
des éléments internes n'a ni forme ni dimension et n'est pas exportée. Dans l'inspecteur de la feuille, la section **Perçages et pointages** de chaque pièce :

- **Ajouter un marquage…** ouvre un petit formulaire : choisissez la forme, saisissez le diamètre ou le côté en mm, puis **sélectionnez un point** de la feuille (clic sur le point) et cliquez **Ajouter le
  marquage**. Le marquage **suit le point** : si vous le déplacez, le marquage le suit. Le bouton est grisé tant que le nombre de points sélectionnés ne convient pas.
- **Bout de pince reculé** : cochez la case, saisissez le **recul** en mm et sélectionnez **deux points** — d'abord la **pointe** de la pince, puis un **point qui donne la direction** de la pince. Le marquage est
  posé sur la droite qui va de la pointe vers ce point, à la distance indiquée de la pointe (elle peut dépasser le point de direction). **Ce n'est pas une pince** : aucune pince n'est créée et aucune marge n'est
  calculée ; seule la position du marquage est calculée. Si vous déplacez ensuite la pointe ou la direction, le marquage suit ; un déplacement qui les **confondrait** est refusé.
- Chaque ligne donne la forme, le point (ou la pince), la position, un champ de **dimension** (perçage, croix) ou de **recul** (bout de pince) à modifier — une étape d'annulation par valeur validée — et **Retirer
  le marquage** (la construction n'est pas touchée). Une dimension hors des bornes, nulle ou non numérique est refusée avec un message, sans rien modifier.
- Sur le canevas, les marquages de la feuille sont dessinés **par-dessus** la construction : disque plein, cercle **à l'échelle** (au moins 3 pixels de rayon), croix, avec leur nom écrit. Ils ne se
  sélectionnent pas à la souris et ne sont pas dessinés dans le plan de pièces.
- Supprimer un point qui porte un marquage **retire** le marquage dans la même étape (annulable), avec une remarque. Si le **centre** d'un marquage sort du contour de la pièce, la fiche affiche « ⚠
  Avertissement : n marquage(s) ont leur centre hors du contour » : la pièce reste enregistrable. « Dupliquer la feuille » copie les marquages ; **Séparer** les reprend dans la moitié qui les contient ;
  **Assembler** les reprend ; **Déplier** ne les reprend **pas** (une remarque le dit) : à replacer sur la pièce entière.
- **Export** : en SVG, un groupe `marquage` avec une forme par marquage (`pointage`, `percage`, `croix`) ; en PDF, les mêmes formes en vectoriel, à l'échelle 1:1, et une ligne de légende sur la page étalon.
  Un perçage de 6 mm de diamètre est exporté avec un diamètre de 6 mm (mesuré dans le fichier par les tests ; **non mesuré sur papier**).

Limites : la direction d'une pince se donne par deux points ; la croix est un « + » ; les bornes de dimension et le vocabulaire (pointage, perçage, bout de pince) n'ont **pas été relus par une modéliste** ;
le rendu, le formulaire et l'export n'ont été exercés que par des tests automatiques, **jamais vus dans la vraie fenêtre ni imprimés**.

### Boutons et boutonnières

Une pièce peut porter des **groupes de boutons** (cercles) ou de **boutonnières** (fentes à bouts arrondis), répartis **régulièrement entre deux points** de la feuille : le premier élément est sur le
**premier point**, le dernier sur le **second**, l'espacement est la distance divisée par (nombre − 1) ; avec un seul élément, il est sur le premier point. Les positions sont **calculées** : si vous
déplacez l'un des deux points, le groupe suit (un déplacement qui les **confondrait** est refusé). Ce n'est ni un perçage ni une marque interne. Dans l'inspecteur, section **Boutons et boutonnières** :

- **Ajouter des boutons…** ouvre un formulaire : boutons ou boutonnières, **nombre** (de 1 à 100), **diamètre** (1 à 100 mm) ou **longueur** (hors tout, 2 à 100 mm) et **largeur** (0,5 à 50 mm, au plus
  la longueur), **orientation** d'une boutonnière, **recul du bord** (0 à 1000 mm). Puis **sélectionnez deux points** de la feuille — d'abord celui du premier élément, puis celui du dernier — et cliquez
  **Ajouter le groupe**. Le bouton est grisé tant que deux points ne sont pas sélectionnés.
- **Orientation** d'une boutonnière (sens de sa longueur) : **horizontale**, **verticale**, **perpendiculaire à l'axe** des deux points (l'usage d'une patte de boutonnage) ou **angle libre** en degrés
  (depuis l'horizontale, sens trigonométrique ; −90° est enregistré 270°).
- **Recul du bord** : c'est la distance **minimale souhaitée entre le bord de l'élément** (le cercle du bouton, la fente de la boutonnière) **et le contour de la pièce**. C'est un **contrôle** : un élément
  plus près que ce recul — ou qui touche, traverse ou sort du contour — est signalé par « ⚠ n élément(s) plus près du bord que le recul demandé » sous le groupe et dans les avertissements de la fiche ; **rien
  n'est refusé ni déplacé**. Pour une boutonnière, l'orientation compte (une fente verticale près du bord du bas n'a pas le même recul qu'une fente horizontale). Certains ateliers mesurent le recul depuis
  le **centre** du bouton : ce n'est pas la règle ici.
- Chaque ligne donne le nombre, la taille, l'orientation, les deux points et l'**espacement calculé**, avec des champs **nombre**, **dimensions**, **angle** et **recul** à modifier (une étape d'annulation
  par valeur validée), **Points = sélection** (remplace les deux points par la sélection) et **Retirer le groupe** (la construction n'est pas touchée). Une valeur non numérique ou hors des bornes est refusée
  avec un message, sans rien modifier.
- Sur le canevas, les éléments sont dessinés **par-dessus** la construction : cercle à l'échelle (au moins 3 pixels de rayon) avec son centre, fente à l'échelle, avec le nom écrit sur le premier élément et
  un « ⚠ » (en plus de la couleur) pour un élément trop près du bord. Ils ne se sélectionnent pas à la souris et ne sont pas dessinés dans le plan de pièces.
- Supprimer un point d'un groupe **retire** le groupe dans la même étape (annulable), avec une remarque. « Dupliquer la feuille » copie les groupes ; **Séparer** reprend ceux dont les deux points sont dans la
  même moitié (les autres sont signalés) ; **Assembler** les reprend (l'orientation suit la rotation de la pièce) ; **Déplier** ne les reprend **pas** (une remarque le dit) : à replacer sur la pièce entière.
- **Export** : en SVG, un groupe `boutons` avec un cercle (`bouton`) ou un chemin fermé (`boutonniere`) par élément ; en PDF, les mêmes formes en vectoriel, à l'échelle 1:1, et une ligne de légende sur la
  page étalon. Un bouton de 12 mm de diamètre est exporté avec un diamètre de 12 mm, une fente de 24 × 4 mm avec 24 × 4 mm (mesuré dans le fichier par les tests ; **non mesuré sur papier**).

Limites : un seul type d'élément par groupe et une répartition régulière ; aucun contrôle de chevauchement entre éléments ; le bouton est un cercle simple (pas de trous) et la boutonnière une fente à bouts
arrondis (pas d'œillet) ; le recul est mesuré du bord de l'élément ; les bornes et le vocabulaire (groupe de boutons, recul du bord, perpendiculaire à l'axe) n'ont **pas été relus par une modéliste** ; le
dessin, le formulaire et l'export n'ont été exercés que par des tests automatiques, **jamais vus dans la vraie fenêtre ni imprimés**.

### Cran lié à un point

Par défaut un cran est posé à une **distance fixe** depuis le départ de son tracé. Vous pouvez le **lier** à la construction : sa position est alors **calculée** (jamais saisie) et le cran la suit quand
la construction change. Posez le cran avec l'outil Cran (`N`), laissez-le sélectionné, puis, dans l'inspecteur, section **ANCRAGE**, choisissez le mode (un clic = une étape annulable) :

- **Point** : le cran se place au **pied de la perpendiculaire** abaissée d'un point de la feuille sur le tracé (il n'est donc sur le point lui-même que si le point est sur le tracé). Choisissez le
  point dans la liste (le premier de la feuille est proposé). Déplacez le point : le cran suit. Un point au-delà d'une extrémité donne l'extrémité.
- **Milieu** : au milieu du tracé, en **longueur développée** pour une courbe, un arc ou un cercle (pas le milieu de la corde). Le cran suit le tracé quand sa longueur change.
- **Proportion** : à un pourcentage de la longueur du tracé depuis son départ (0 % = départ, 100 % = arrivée), toujours en longueur développée (ce n'est pas le paramètre interne d'une courbe de
  Bézier). En passant à ce mode la position actuelle est conservée ; le champ se règle au clavier ou en glissant (un glisser = une étape).
- **Report** : un point (par exemple celui d'**une autre pièce** de la feuille) est projeté sur un **tracé source** ; la distance obtenue **depuis le départ de ce tracé** est reportée sur le tracé du cran,
  depuis son départ, ou depuis son arrivée si vous cochez la case (deux bords parcourus en sens inverse). Source et point doivent être **de la même feuille** : le point d'une pièce détachée sur une autre
  feuille ne se reporte pas. Si la longueur reportée dépasse celle du tracé du cran, c'est refusé (rien n'est modifié) ; un déplacement qui l'y amènerait l'est aussi.
- **Distance fixe** : détache le cran, qui **reste exactement où il est**, désormais à distance fixe.

La **position calculée** s'affiche (le champ de distance disparaît tant que le cran est lié). Le côté, la longueur et le style se changent comme d'habitude.

Ce qui se passe quand la construction change : **supprimer le point** (ou le tracé source) retire le cran, dans la même étape, avec une remarque (annulable) ; **couper, fusionner, raccourcir, prolonger**
le tracé **détache** le cran, qui reste où il était (remarque) ; **inverser** le tracé garde le lien et la place ; la **symétrie liée**, la **duplication** et le **copier-coller** gardent le lien si tout ce
qu'il cite est copié, sinon le cran copié est à distance fixe (remarque) ; **Dupliquer la feuille** copie les crans liés vers les objets de la copie. **Séparer**, **assembler**, **déplier** et les **pièces
dérivées** ne reprennent pas les crans (une remarque le dit) : la construction garde les siens, liés. **Export** PDF et SVG : un cran lié est exporté à sa position calculée, exactement comme un cran fixe
posé au même endroit.

Limites : l'outil Cran pose toujours un cran fixe (le lien se choisit ensuite dans l'inspecteur) ; points et tracé source se choisissent dans des listes, pas au clic ; pas de report d'une feuille à
l'autre ; la précision du pied d'un point sur une courbe de Bézier est de l'ordre du dix-millième de millimètre (mesurée) mais n'a pas été étudiée sur de très longues courbes ; les **crans projetés sur
la coupe** se règlent avec les marges de couture, non décrites ici ; le vocabulaire (ancrage, report, proportion) n'a **pas été relu par une modéliste** ; l'inspecteur n'a été exercé que par
des tests automatiques, **jamais vu dans la vraie fenêtre**.

### Droit-fil avancé

Le droit-fil d'une pièce reste un **tracé de rôle « droit-fil »** (outil Droit-fil `G`, puis « Assigner le droit-fil sélectionné » dans la fiche de la pièce). Dès qu'une pièce a un droit-fil, la section
**DROIT-FIL AVANCÉ** de son inspecteur permet de régler comment il se lit ; rien n'est réglé par défaut et un fichier sans réglage reste identique. Chaque clic ou valeur validée = **une étape annulable**.

- **Direction** : **Tracé dessiné** (la direction du tracé, comme avant) ; **Parallèle à…** ou **Perpendiculaire à…** un autre tracé de la feuille, choisi dans la **liste des tracés de référence** (la
  direction d'une courbe est celle de sa **corde**, du premier au dernier point ; un cercle ne peut pas servir) ; **Biais +45°** ou **Biais −45°**, mesuré depuis l'**axe X de la feuille** (Y vers le haut).
  La direction est **calculée à la lecture** : si vous déplacez le tracé de référence, la flèche suit. La flèche garde la **longueur et le milieu** du tracé dessiné ; « Inverser la flèche » la retourne.
  La ligne « Flèche : … depuis l'axe X » est la mesure **observée**, pas un réglage.
- **Sens du tissu** : **Aucun** (flèche à deux pointes ouvertes, comme avant), **Poil** (une **seule pointe fermée**, pour le sens du poil d'un velours ou d'une fourrure) ou **Motif** (les deux pointes,
  **doublées**). La forme se lit sans la couleur, sur la feuille comme à l'export.
- **Tolérance de rotation** : degrés de 0 à 180 (0 = aucune rotation libre, valeur par défaut). Dans le **plan de pièces**, si la rotation du placement fait dévier le droit-fil de plus que la tolérance,
  la fiche affiche « la rotation du placement dévie le droit-fil au-delà de la tolérance de rotation » et l'inspecteur du plan donne rotation, déviation et tolérance. **C'est un avertissement : rien n'est
  refusé.** Une déviation **égale** à la tolérance est acceptée. Un droit-fil **sans sens** est un axe : tourner la pièce de 180° le laisse parallèle (déviation 0) ; avec un sens (poil, motif), 180° est une
  déviation de 180°. Avec plusieurs lignes, la pire déviation compte. **Attention** : une pièce déjà tournée à la main dans le plan, sans tolérance, porte désormais cet avertissement.
- **Lignes supplémentaires** (rayures, raccords), jusqu'à **8** en plus de la principale : sélectionnez un **segment de rôle droit-fil** de la feuille qui n'est le droit-fil d'aucune pièce, puis
  **Ajouter la ligne sélectionnée** ; chaque ligne a son propre éditeur (direction, sens, inversion) et **Retirer cette ligne**. Une ligne réglée n'appartient qu'à une pièce.
- **Sur la feuille** : la flèche calculée remplace le tracé dessiné ; **le tracé dessiné reste visible en filigrane** (pointillé gris) et c'est lui qu'on sélectionne : la flèche calculée ne se clique pas.
- **Si le tracé de référence est supprimé** : la direction **repasse à celle du tracé dessiné**, avec une remarque (annulable) ; une ligne supplémentaire supprimée est retirée avec une remarque ; si le
  droit-fil principal est supprimé, tous les réglages sont retirés avec une remarque. Couper un tracé de référence droit ne change pas la direction. Un déplacement qui rendrait nulle la longueur d'un tracé de
  référence, ou changer le rôle d'une ligne réglée, est refusé.
- **Copie, séparer, assembler, déplier** : « Dupliquer la feuille » copie les réglages vers les objets de la copie ; **Séparer** reporte la **flèche calculée** (sa direction est alors **figée**, une
  remarque le dit) avec son sens et sa tolérance dans la moitié qui contient le droit-fil, et signale les lignes supplémentaires de l'autre moitié, non reprises ; **Assembler** reprend les réglages de la
  première pièce (sinon de la seconde) ; **Déplier** ne reprend pas le droit-fil (remarque) ; une pièce dérivée n'a pas de droit-fil.
- **Export** : la flèche exportée est la flèche calculée. **Aucun sens** : inchangé. **Poil** : une pointe fermée (triangle) à l'extrémité de la flèche. **Motif** : deux pointes à chaque bout, doublées
  (la seconde à 2 mm en retrait). En SVG, les traits d'une ligne qui a un sens portent `data-grain-sense` ; en PDF, une ligne de légende de la page étalon apparaît seulement si un sens est utilisé.

Limites : biais limité à ±45° ; direction d'une courbe = corde ; 8 lignes supplémentaires au plus, segments seulement ; séparer et assembler **figent** les directions ; pas de marge de couture ni de
texte orienté le long du droit-fil. Le **vocabulaire** (poil, motif, biais, tolérance de rotation), la convention du biais (axe X de la feuille), l'idée qu'un droit-fil sans sens est un axe et la règle de
la « pire ligne » **n'ont pas été relus par une modéliste**. La section, le dessin et l'export n'ont été exercés que par des tests automatiques, **jamais vus dans la vraie fenêtre ni imprimés**.

### Lignes spéciales et surpiqûre liée

**Lignes spéciales** : *milieu devant*, *milieu dos*, *ligne de taille*, *ligne de hanche*, *ligne de poitrine*, *ligne de parage*, *ligne de collage* et *surpiqûre* sont des **rôles de tracé** : dessinez
un tracé, choisissez son rôle dans l'inspecteur. La **pliure** est le rôle « Pliure » qui existait déjà. Chaque rôle a son motif de trait (tirets, tirets-points, petits tirets serrés, gros points…) et son
épaisseur, **visibles en noir et blanc**, sur la feuille comme dans le SVG (attribut `data-role` : `surpiqure`, `milieu-devant`, `milieu-dos`, `ligne-taille`, `ligne-hanche`, `ligne-poitrine`, `ligne-parage`,
`ligne-collage`) et le PDF (la page étalon ajoute une ligne de légende pour chaque rôle spécial utilisé). Ce sont de simples lignes : le logiciel n'en tire aucune mesure ni contrôle.

**Surpiqûre liée à un bord** : une ligne de surpiqûre **générée à une distance donnée d'un bord** et **recalculée quand ce bord change**.

- **Créer** : sélectionnez le bord (un ou plusieurs tracés bout à bout, un cercle), la section **SURPIQÛRE LIÉE** de l'inspecteur apparaît ; saisissez la **distance** (en mm, de 0,1 à 1000 ; 5 par défaut), puis
  **Créer — Intérieur** ou **Extérieur** (bord fermé) ou **Gauche** / **Droite** (bord ouvert : le côté se compte à partir du point nommé sous le champ). Pour suivre le **contour d'une pièce**, utilisez
  « Surpiqûre le long du contour » dans la fiche de la pièce. Une étape annulable ; une distance non numérique, nulle, négative ou hors bornes, un bord illisible ou un décalage impossible (par exemple 40 mm
  à l'intérieur d'un bord de 60 mm de haut) sont refusés avec un message, et rien ne change.
- **Suivre le bord** : si vous déplacez un point du bord, la ligne est recalculée **dans la même étape** (un seul Ctrl+Z annule le déplacement et la ligne). Si le recalcul est impossible (bord devenu trop étroit,
  contour qui se recoupe…), **votre déplacement n'est pas refusé** : la surpiqûre garde sa dernière forme valide et est marquée **« ⚠ Obsolète : raison »** (dans l'inspecteur quand vous sélectionnez une de ses
  lignes, dans la ligne « Surpiqûres liées » de la feuille, et par « ⚠ surpiqûre obsolète » sur le dessin) ; dès que le bord redevient valide, elle est mise à jour.
- **Régler** : sélectionnez une ligne de la surpiqûre ; changez la **distance** (champ validé à la perte du focus) ou le **côté** : une étape, refusée sans trace si le résultat est impossible.
- **Ne pas modifier directement** : une ligne liée ne se déplace pas, ne se coupe pas, ne change pas de rôle (« Cassez le lien avant de la modifier »). **Casser le lien** la rend libre : ses lignes restent
  (rôle surpiqûre) mais ne suivent plus le bord. Déplacer **tout** d'un bloc (bord et ligne) est permis.
- **Copie, séparer, déplier** : « Dupliquer la feuille » ne copie pas le lien (les lignes copiées sont libres, remarque) ; séparer ou assembler une pièce remplace la pièce et **casse** le lien qui la suivait
  (remarque) ; déplier et créer une pièce dérivée ne reprennent pas les surpiqûres de la mère (remarque). Supprimer un tracé du bord ou une ligne casse le lien, avec une remarque.
- **Pièce et élément interne** : déclarer une ligne comme élément interne « Surpiqûre » d'une pièce (section « Éléments internes ») dit seulement qu'elle appartient à cette pièce ; ce n'est pas le lien de
  décalage, les deux se combinent.

Limites : **aucune marge de couture** (la distance est un décalage de mise en page, pas une marge calculée) ; une surpiqûre par rang (créez plusieurs liens à des distances différentes pour plusieurs rangs, rien
ne les groupe) ; pas de longueur de point ; parage et collage sont des **lignes**, pas des zones ; les liens ne sont pas visibles pour l'assistant MCP (les rôles le sont). Le **vocabulaire** (milieu devant / dos,
taille, hanche, poitrine, parage, collage), les **motifs de trait** et les règles de côté **n'ont pas été relus par une modéliste**. Le dessin, la section et l'export n'ont été exercés que par des tests
automatiques, **jamais vus dans la vraie fenêtre ni imprimés**.

### Cotes complètes (longueur, angle, rayon, projections)

Une **cote** est une **mesure** : sa valeur est **recalculée depuis la construction** à chaque affichage, elle suit les points et les tracés qu'on déplace, et elle **n'impose rien** (aucune longueur ni aucun
angle n'est « verrouillé » par une cote). La cotation entre deux points (outil `Q`) reste telle quelle ; les cotes ci-dessous la complètent.

- **Créer** : sélectionnez les objets à mesurer, puis cliquez le bouton dans la section **COTES (MESURES)** de l'inspecteur (les boutons sont grisés tant que la sélection ne convient pas ; le survol dit ce
  qu'il faut sélectionner ; **aucun raccourci clavier**) :
  - **Longueur du tracé** : un tracé (segment, courbe, arc, courbe lisse) — **longueur développée**, courbes comprises ; la ligne de cote est parallèle à la **corde** du tracé, décalée de 10 mm ; le texte
    annonce « dév. » pour un tracé qui n'est pas un segment (valeur développée, ligne = corde) ;
  - **Angle** : trois points (**le sommet d'abord**, puis un point sur chaque côté) ou deux segments qui se touchent. L'angle se mesure **dans le sens anti-horaire, du premier côté au second** : de l'est au
    nord, 90° ; du nord à l'est, 270° (échangez les côtés pour obtenir l'angle rentrant). Un angle plat (180°) est accepté ; deux côtés superposés (0°) sont refusés ;
  - **Rayon** et **Diamètre** : un arc ou un cercle (« R 12,50 mm », « Ø 25,00 mm ») ; un segment ou une courbe n'a pas de rayon (refus) ;
  - **Horizontale** et **Verticale** : deux points — l'écart sur l'axe X (ou Y) **du premier au second** ; la valeur **signée** est écrite dans la liste, le dessin affiche la longueur.
- **Lire** : chaque cote apparaît dans la liste (type, valeur, valeur signée pour les projections) et sur le dessin, avec ses traits d'attache, ses flèches et son **texte orienté le long de la ligne** (jamais à
  l'envers : une ligne verticale se lit de bas en haut). Valeur au **centième de millimètre** (« 123,45 mm »). Dans le SVG et le PDF, la cote est exportée (lignes et **texte réel**).
- **Légende et position du texte** : champ **Légende** (40 caractères au plus, accents autorisés : « Épaule — côté œil » donne « Épaule — côté œil : 80,00 mm »), **Texte, décalage (mm)** (le long de la ligne,
  négatif vers l'arrière), et la mise en page : **décalage de la ligne** (longueur, horizontale, verticale), **rayon de l'arc** (angle) ou **direction du trait** (rayon, diamètre). Chaque réglage valide à la perte
  du focus, **une étape annulable**, refusé sans trace si la valeur est invalide (légende trop longue ou avec saut de ligne, nombre non fini).
- **Quand la cible change** : une cote dont l'objet est **supprimé** est **retirée avec une remarque** (jamais de cote orpheline). Si un déplacement la rend impossible (deux côtés d'angle superposés, deux
  points de même abscisse pour une horizontale), elle devient **« non calculable pour le moment »** (avec la raison) et n'est ni dessinée ni exportée, **sans refuser votre déplacement** ; elle revient dès que
  la construction la redéfinit.
- **Copie, calques, pièces** : « Dupliquer la feuille » et le copier-coller reprennent les cotes (au copier-coller, seulement celles dont **tous** les objets sont copiés, avec une remarque pour les autres) ;
  masquer un calque masque ses cotes ; un objet sur un **calque verrouillé** ne peut pas recevoir de cote et sa cote ne se modifie plus ; séparer, assembler et déplier une pièce **laissent les cotes de la
  feuille d'origine** (les feuilles créées n'en reçoivent pas).

Limites : le PDF **embarque** un sous-ensemble de la police Noto Sans (licence SIL OFL) : les accents du français (é è à ç ê ô œ « » — °, Ø…), le latin étendu (« Ł », « ź »…) et le grec sont fidèles et le texte reste copiable ; un caractère absent de cette police (cyrillique, `≤ ≥ → ∞` ou
le japonais) devient **« ? »** dans le PDF (il reste intact dans le SVG, qui n'embarque pas de police). Les cotes **ne se sélectionnent pas à la souris** (on les gère depuis la section) et ne
sont pas dans la recherche ; pas de cote chaînée, de cote de pièce (aire, périmètre) ni de tolérance. Une **cotation entre deux points** (outil `Q`) n'a ni légende ni texte orienté. Le **vocabulaire** (cote,
longueur développée « dév. », légende, sens de l'angle, texte au-dessus de la ligne) **n'a pas été relu par une modéliste**. La section, le dessin et l'export n'ont été exercés que par des tests automatiques,
**jamais vus dans la vraie fenêtre ni imprimés**.

### Contrôle d'assemblage (coutures, marche, crans, raccords, rapport)

Tout ce qui suit **mesure** : rien n'est imposé aux pièces, rien n'est enregistré, et aucune de ces mesures
n'a été validée par une modéliste. Menu **Édition › Coutures…** (fenêtre à gauche, sans raccourci).

- **Coutures appariées** : déclarez que le bord A d'une pièce se coud avec le bord B d'une autre (ou de la
  même, s'ils n'ont aucune arête commune) : par les listes ou en cliquant le bord A puis le bord B sur le plan.
  A est le côté **à embuer**. Le tableau donne, en direct, les longueurs de **couture** (jamais la coupe),
  l'écart, l'embu mesuré (A − B) et l'embu attendu que vous avez saisi (0 sans embu), et un statut **dans / hors
  tolérance** (1 mm par défaut) ou **non calculable** avec la raison (contour ouvert, tracé sorti du contour…).
- **Marche de pièces** : « Reporter les crans » copie les crans d'un côté sur l'autre à la même abscisse de
  couture, l'embu étant réparti proportionnellement ; ce sont de vrais crans, que vous pouvez ensuite déplacer
  ou supprimer (une étape d'annulation).
- **Contrôle des crans** : chaque cran d'un côté doit avoir son vis-à-vis de l'autre. La fenêtre liste les
  crans **manquants** et **décalés** (tolérance 1 mm) ; « Reporter les manquants » les ajoute par la marche.
- **Raccords** : à chaque bout de couture, l'angle de chaque pièce, leur somme (lisse si elle vaut 180°) et
  l'écart (> 0 : bosse, < 0 : creux) ; tolérance 2°. « Vue assemblée » montre les deux pièces rapprochées.
- **Rapport de contrôle** : **Fichier › Exporter le rapport de contrôle…** écrit un **PDF** (A4, plusieurs
  pages si besoin) ou un **CSV** (tableur : UTF-8, séparateur « ; », point décimal, mm à 0,01 près). Il
  rassemble les pièces (aire, périmètre, avertissements), les coutures, le contrôle des crans, les raccords et un
  récapitulatif des **anomalies** (« Aucune anomalie » quand il n'y en a pas). Il n'y a ni date ni heure : le
  même projet donne le même fichier. Le pied de page rappelle « mesures observées, non validées par une
  modéliste ». Dans un tableur, un texte qui commence par `=`, `+`, `-` ou `@` est précédé d'une apostrophe
  (protection contre les formules). Un refus d'écriture (dossier absent, fichier verrouillé) n'écrit rien et
  ne change pas le projet. Le rendu dans un tableur et à l'impression n'a pas encore été vérifié par un humain.

### Vues, unités et grille

Menu **Affichage > Vues, unités et grille…**. L'**unité d'affichage** (millimètres, centimètres ou
pouces fractionnaires, précision de 1/2 à 1/64, 1/16 par défaut) ne change que l'affichage et la
saisie : le projet reste en millimètres et les exports PDF, SVG et DXF aussi. En pouces, une valeur
arrondie à la fraction est précédée de « ≈ ». On saisit `3/8`, `1 1/16` ou `1-1/16` ; sans unité, la
valeur est dans l'unité choisie, et une unité écrite (`12 mm`) l'emporte. **Tous** les champs de
longueur suivent l'unité (marges, crans, pinces, plis, boutons, piqûre, cotes, coutures, gradation,
barème, plans de coupe, mise en page du PDF, préférences…) : le libellé « (mm) » devient « (in) » ou
« (cm) ». Un champ dont la valeur n'est pas une fraction exacte s'affiche en millimètres explicites
(`10,0 mm`) plutôt qu'arrondi, pour que rien ne dérive. Restent en millimètres : la valeur d'une
variable déclarée en mm, la mesure du carré d'étalonnage, l'échelle et l'unité des imports.

La **grille** se règle (pas des repères, subdivisions, affichée ou non) et les **règles** suivent
l'unité. **Affichage > Zoom sur la sélection** (touche **Fin**) cadre les points et tracés
sélectionnés. Une **vue nommée** retient le cadrage, le zoom, la feuille et les calques visibles :
« Enregistrer la vue courante », puis « Restaurer » (le changement des calques s'annule en une
étape). Ces réglages sont mémorisés hors projet, dans `settings.json`.

### Photo ou scan d'un patron papier : fond calibré et décalque

Menu **Affichage > Image de fond (photo, scan)…**. **Enregistrez d'abord le projet** : l'image est
**copiée** dans un dossier voisin du fichier du projet (`jupe.assets/`) ; elle n'est jamais écrite dans le
fichier du projet ni exportée (PDF, SVG, DXF et HPGL ne la contiennent pas). Formats : PNG et JPEG.

1. **Importer** : tapez ou parcourez le chemin d'une photo ou d'un scan, puis « Importer ».
2. **Calibrer** sur l'aperçu : *deux points* (cliquez deux points de l'image dont vous connaissez la
   distance réelle, saisissez-la) ou *quatre points* (cliquez les coins d'un rectangle connu dans
   l'ordre bas-gauche, bas-droite, haut-droite, haut-gauche, saisissez sa largeur et sa hauteur : la
   perspective de la photo est corrigée). Clic droit retire le dernier point ; les coordonnées se
   règlent au pixel près dans les champs. Un point cliqué sur l'aperçu n'est précis qu'à quelques
   pixels d'image près : réglez-le dans les champs.
3. **Poser le fond** : l'image apparaît sous la grille et les tracés, à l'échelle exacte du modèle (le
   premier point cliqué est posé à l'origine de la feuille). La fenêtre affiche le **résidu** de la
   calibration, en moyenne quadratique et par point : avec 2 ou 4 points il vaut environ 0 et **ne mesure
   pas** l'erreur de pointage ; vérifiez l'échelle avec l'outil Mesure sur une dimension connue.
4. Réglez l'**opacité**, masquez ou **verrouillez** le fond ; « Recalibrer », « Relier » (autre fichier de
   mêmes dimensions) et « Retirer le fond » sont des étapes annulables. Si l'image est **introuvable ou
   modifiée**, le projet s'ouvre quand même, une bannière le dit et rien n'est perdu : reliez-la.

**Décalque.** Dans la même fenêtre : « Choisir l'outil Décalque » (famille Dessin, sans raccourci par
défaut), puis cliquez des points le long du contour de l'image. *Accrocher aux contours de l'image* aide
à viser : « Calculer les contours » lance un calcul (annulable) ; un clic proche d'un contour (au-dessus
du *seuil de contour*) se pose dessus, sinon à l'endroit cliqué. Le point posé est un point ordinaire :
rien ne le lie à l'image. L'aperçu du lissage (orange) suit la *tolérance* et l'*angle de coin* ;
« Appliquer lissé » crée des courbes de Bézier ordinaires, « Appliquer en segments » des segments, en
**une seule étape annulable**. Échap efface les points non appliqués.

Limites : la photo n'est pas redressée (utilisez les quatre points) ; l'orientation EXIF des JPEG n'est
pas appliquée (faites pivoter l'image avant de l'importer) ; jamais essayé avec une vraie photo de patron
papier.

**Enregistrer sous.** Dans un autre dossier, les images sont **copiées** à côté du nouveau fichier, sous le
même nom de dossier (`jupe.assets/`, même si le projet s'appelle désormais autrement : gardez-le avec lui).
Dans le même dossier, rien n'est copié : le projet continue d'utiliser ce dossier. L'original n'est jamais
modifié ; si un fichier différent porte déjà le même nom ou si la copie échoue, un message le nomme et le
projet reste enregistré. Les versions nommées (`.pforge.versions/`) ne sont pas copiées.
### Plan de coupe sur peau (cuir)

Dans la fenêtre « Plan de coupe », la section **Peau (cuir)** remplace la laize rectangulaire par le contour réel d'une peau. Trois sortes d'objets, tous des polygones en millimètres : le **contour** de la peau, les **défauts** à éviter (trou, cicatrice, tache, zone mince, autre, avec une note) et les **zones de qualité** (grade 1 = meilleure, à 3).

- **Saisir un polygone** : choisissez l'objet, puis cliquez les sommets sur le canevas (ou tapez leurs coordonnées X et Y et « Ajouter ce point »), puis « Terminer le polygone » (3 points au moins). Vous pouvez aussi partir d'une **polyligne fermée** de la feuille (sélectionnez ses tracés puis « Depuis la polyligne fermée sélectionnée ») ou du **contour d'une pièce**. Chaque geste est une étape annulable ; un polygone qui se croise, un défaut qui sort du contour ou qui en recoupe un autre est refusé avec un message.
- **Contrôles** : une pièce posée qui **touche un défaut** est signalée avec l'aire exacte de recoupement (mm²) ; une pièce qui **sort de la peau** avec l'aire dehors ; la **qualité** d'une pièce est le **pire grade** des zones qu'elle recoupe, avec l'aire par grade. Le défaut est dit par un motif de hachures propre à son genre, pas par la couleur seule. Avec une peau, la laize, les lisières et le pli ne comptent plus.
- **Exigence de qualité** : choisissez le pire grade accepté ; les pièces qui ne la respectent pas sont listées. Ce réglage n'est **pas enregistré** dans le fichier.
- **Métrage cuir** : surface posée sur surface utile (contour moins défauts), en pourcentage ; un plan invalide n'affiche aucun chiffre.
- **Placement automatique** : sur une peau, la proposition reste dans le contour, évite les défauts et respecte l'exigence de qualité (une seule passe, première position valide) ; si une pièce ne tient pas, elle est nommée. Ce n'est ni optimal ni garanti minimal.
- **Exports** (SVG, PDF 1:1, CSV) : contour, défauts (hachures, libellé) et zones (tirets, libellé) sont dessinés ; le CSV ajoute les sections `peau` et `peau_poses`.
- **Limites** : jamais essayé avec une vraie peau ; le contour numérisé vient de la saisie (le calque d'une photo n'est pas encore branché) ; deux zones de même grade qui se recouvrent comptent deux fois.

### Marges de couture

Dans la fiche de pièce, section **Marge de couture**. Le contour dessiné est la **ligne de couture** ; le
**contour de coupe est calculé** et le dessin n'est jamais modifié.

- **Défaut (mm)** s'applique à tous les bords (0 à 1000 mm, vers l'extérieur).
- **Surcharges par bord** : sélectionnez un tracé du contour puis surchargez-le ; la valeur peut différer au
  début et à la fin.
- **Coins** : onglet (par défaut), perpendiculaire au bord précédent ou suivant, symétrique, tronqué, en
  escalier, ourlet (pli sur le bord suivant ou précédent). Une pointe maximale limite les onglets longs.
- **Ourlets** : sur un bord droit, avec leur hauteur.
- **Sur le pli** : un bord droit déclaré sur le pli n'a ni marge ni coupe ; « SUR LE PLI » est écrit à l'export.
- **Marges par matière** : appliquer la valeur de la catégorie de la pièce.

L'inspecteur donne le périmètre et l'aire de la coupe calculée, ou la raison d'un échec. Valeurs et
conventions de coins **non relues par une modéliste**.

### Pinces, plis, ampleur et découpe

- Outil **Pince** (`Y`) : cliquez le bord droit qui porte la base, puis le sommet dans la pièce ; tapez
  l'**ouverture** ou visez à la souris, Entrée. Le panneau donne les jambes, l'angle et la ligne avant et
  après pliage ; il permet de changer de côté, de **couper pince fermée**, de supprimer ou de **transférer la
  pince** (tout ou un pourcentage) vers un autre bord, ce qui reconstruit la pièce sur une nouvelle feuille.
- **Édition › Plis…** : ligne de pli en 2 clics, pli plat ou creux, côté, profondeur, nombre et répartition ;
  largeur ajoutée, aire avant et après, vue de la pièce pliée.
- **Édition › Ampleur…** : ouverture en éventail. Bord fixe droit, nombre ou positions des lignes, ouverture
  en haut (une valeur ou une par ligne). Une seule ampleur par pièce.
- **Édition › Découper…** : coupe une pièce en deux selon une ligne et pose des **crans de repérage**
  identiques sur les deux nouveaux bords.

Fronces et godets ne sont pas proposés.

### Lignes de piqûre et zones du cuir

Dans la fiche de pièce.

- **Lignes de piqûre…** : tracés consécutifs du contour, forme du trou (emporte-pièce rond ou plat, losange
  français, oblique japonais), **nombre automatique** d'après un pas demandé ou **nombre fixe**, et
  **retrait** du bord. Les positions suivent le bord.
- **Apparier** deux lignes de deux pièces compare nombres de trous et pas moyens et liste les trous manquants
  ou décalés.
- **Zones (parage, collage, découpe)…** : **bande** le long d'un bord (recalculée avec lui) ou
  **polygone libre** depuis des points sélectionnés ; l'aire est affichée.

### Variables, formules et constructions maintenues

- **Édition › Variables…** : valeurs nommées (mm, degrés ou sans unité) définies par des expressions :
  `+ - * / ( )`, `min(a;b)`, `max(a;b)`, `abs`, `sqrt`, `pi` ; arguments séparés par `;`. Rien ne change tant
  qu'une saisie est invalide.
- **Formule** : dans l'inspecteur d'un point placé par distance, proportion, direction ou dx / dy, tapez par
  exemple `L/2+10` ; la valeur reste **maintenue**. **Délier** garde le nombre courant.
- **Maintenir la construction** : segment par longueur et angle, perpendiculaire, parallèle, rectangle ;
  coché, la forme suit ses références ; décoché, les points restent où ils sont.
- **Longueur imposée** : une courbe de Bézier ou lisse peut avoir sa longueur développée imposée, une poignée
  ou une extrémité s'ajustant ; elle peut aussi suivre la longueur d'un autre tracé.
- **Affichage › Diagnostic des liens** liste les liens et leurs problèmes ; **Affichage › Dépendances** montre
  parents et dépendants de la sélection.

Il n'y a toujours pas de solveur de contraintes général.

### Tailles et gradation

- **Affichage › Tailles…** : gammes (alphanumériques ou numériques) dans l'ordre de gradation, avec une
  **taille de base**.
- **Affichage › Gradation…** : sur un point libre, écarts **cumulés** (dx, dy) depuis la base, par taille. Les
  points construits suivent leurs parents. Une règle se **copie** (à l'identique, en miroir, inversée) ou
  s'**interpole** entre deux points gradués.
- Une variable peut avoir une expression différente par taille.
- On bascule la taille affichée pour contrôler ; hors base, elle est en lecture seule.
- **Mesures par taille**, **nid de tailles** et **contrôle de forme** (courbure d'une taille à l'autre) dans le
  menu Affichage ; **Fichier › Exporter des tailles…** écrit un fichier par taille.

Constructions maintenues et longueurs imposées ne sont pas réévaluées par taille ; aucun fichier `.rul`.

### Barème, fiches client et retouches

- **Affichage › Barème de mesures…** : mesures (mm ou degrés) par taille. Case vide = non renseignée.
- **Affichage › Fiches client…** : les mesures d'une personne ; une valeur absente n'est jamais devinée. La
  taille la plus proche est indiquée pour information, et une **taille sur mesure** peut être générée.
- **Affichage › Retouches et mesures finies…** : une retouche ajoute un écart à la lecture ; une mesure finie
  est lue sur une couture ou une cote et comparée à sa tolérance.

Ces fenêtres ne modifient jamais le dessin et ne fournissent aucune valeur par défaut.

### Variantes, nomenclature, bibliothèque, versions

- **Fichier › Variantes…** : pièces du projet avec quantité, matière et option, sans les copier.
- **Fichier › Nomenclature et fiche technique…** : pièces, matières, coupes et surface nette par taille.
- **Fichier › Bibliothèque…** : enregistrer une pièce, un bloc (la sélection) ou un modèle, et l'insérer
  ensuite comme copie indépendante.
- **Fichier › Versions…** : instantanés nommés à côté du projet ; comparer, ou restaurer en une étape
  annulable.
- **Fichier › Conventions de nommage…** : noms par préfixe, compteur et suffixe, sur demande seulement.

### Plan de coupe et placement automatique

**Fichier › Plan de coupe…** : plusieurs plans par projet. Laize, lisière, tissu plié et écart minimal, puis
glissez les pièces sur le tissu.

- **Contrôles** : recouvrements (hachures rouges) et débordements (orange) ; le plan dit s'il est valide.
- **Métrage** : longueur utilisée, laize utile, surface, rendement et part par taille ; aucun chiffre pour un
  plan invalide.
- **Placement automatique** : tout remplacer ou **compléter** les pièces non placées. Le résultat est une
  **proposition**, ni optimale ni garantie minimale.
- **Export** SVG ou PDF à 1:1, métrage en CSV.

Pour le cuir, voir « Plan de coupe sur peau » ci-dessus.

### Impression et projection

- **Fichier › Imprimer…** envoie la même mise en page que le PDF au service d'impression du système, à
  100 %, carré étalon compris. **Windows seulement.**
- **Fichier › Projection…** : plein écran sur fond noir pour un vidéoprojecteur. Saisissez la largeur réelle
  de l'image projetée ; miroir, grille et épaisseur réglables ; `Échap` quitte.

La précision est calculée, **non mesurée** sur papier ou à l'écran : contrôlez le carré de 100 mm au réglet.

### Imports et construction rapide

- **Fichier › Importer des pièces d'un DXF de type AAMA / ASTM…** : une pièce détachée par bloc, en une
  étape annulable.
- **Fichier › Importer un SVG / PDF…** : les tracés seulement, comme géométrie ordinaire.
- **Édition › Construction rapide…** : polygone par coordonnées, rectangle ou rectangle arrondi ; les cotes
  saisies ne sont pas maintenues ensuite.

Aucun de ces imports n'a été essayé avec des fichiers de logiciels métier.

### Espace de travail et ligne de commande

**Affichage › Espace de travail**, **Personnaliser les barres d'outils…**, **Préférences…** et
**Historique…** règlent l'interface et permettent de revenir en arrière.

La ligne de commande (`patternforge-cli`) valide, convertit et exporte sans interface : voir
[CLI.md](CLI.md).

## 9. Raccourcis et thème

`F1` ouvre la table des raccourcis. Les lettres des **outils** peuvent être changées (bouton
« Changer », puis une lettre A–Z ; une lettre déjà prise est refusée avec un message ; « Rétablir »
remet les valeurs par défaut). Les raccourcis `Ctrl+…` et les touches spéciales sont fixes. Le
choix est mémorisé hors projet, comme la langue (menu Affichage › Langue) et le thème
(Affichage › Thème, clair ou sombre).

Réglages : `%APPDATA%\PatternForge\settings.json` (Windows). Supprimer le fichier rétablit les
valeurs par défaut.

## 10. Serveur MCP (assistants IA)

`crates/pf-mcp` expose un projet à un assistant via le protocole MCP, en local. Par défaut il est
**en lecture seule** (listes, mesures, validation). Avec `--allow-write`, il offre aussi
`preview_command` (simulation sans écriture), `apply_command` (avec la révision attendue du
fichier ; refus si le fichier a changé) et `undo_last`, avec un journal à côté du projet. Détails :
[../crates/pf-mcp/README.md](../crates/pf-mcp/README.md).

### Valider un fichier de projet

`pf-validate` vérifie un fichier `.pforge` sans l'ouvrir dans l'application et sans jamais le modifier :
`cargo run -q -p pf-core --bin pf-validate -- mon-projet.pforge` (ajouter `--json` pour une sortie lisible par un programme).
Il applique les mêmes règles que l'ouverture dans l'application. Code de sortie 0 = valide, 1 = invalide (le message dit
pourquoi, avec la ligne et la colonne quand le fichier n'est pas du JSON correct), 2 = fichier illisible ou commande mal écrite.
La description du format est dans [FORMAT_PFORGE_V6.md](FORMAT_PFORGE_V6.md), le schéma JSON dans
[pforge-v6.schema.json](pforge-v6.schema.json) ; le schéma seul ne remplace pas ce contrôle.

## 11. Ce que la version actuelle ne fait pas

- Pas de solveur de contraintes général : seules existent les constructions maintenues, les formules et la
  longueur imposée d'une courbe, décrites plus haut.
- Pas de fichier de règles de gradation `.rul` à l'export DXF ; la table des couches DXF de type AAMA / ASTM
  n'est pas confirmée et aucune compatibilité avec un logiciel métier n'est annoncée.
- Le placement automatique des pièces est une aide, pas une optimisation industrielle.
- Pas de macros, de vectorisation automatique d'une image, ni de prise en charge d'une table à digitaliser.
- Aucun test sur papier ni auprès d'une modéliste n'a encore été fait : les conventions métier restent à valider.

## 12. Dépannage

| Symptôme | Cause probable |
|---|---|
| « Aucune ligne de coupe » à l'export | Aucun tracé de rôle « contour de coupe » : l'export est marqué « tracé de construction » |
| Raccourci d'outil sans effet | Focus dans un champ de saisie ; cliquer le canevas ou `Échap` |
| Cran refusé | Distance hors du tracé, ou tracé raccourci sous la position d'un cran existant |
| Fichier non ouvert | Version de format plus récente que l'application, ou référence manquante : le message le dit |

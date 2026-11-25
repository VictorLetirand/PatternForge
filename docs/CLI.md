# Ligne de commande `patternforge-cli` (US-18.2, US-18.4, US-12.6)

Binaire du crate `pf-cli` (aucune interface graphique). Il appelle les mêmes fonctions de `pf-export` que
l'application : les fichiers produits sont identiques à l'octet. Fait, non validé métier.

```
cargo run -p pf-cli -- <commande> [options]      (ou target/release/patternforge-cli)
```

## Codes de retour

| Code | Sens |
|---|---|
| 0 | succès |
| 1 | projet invalide ou non exportable (JSON, version, références, feuille vide, DXF sans pièce, image trop grande) ; pour `lot` : au moins un échec |
| 2 | fichier illisible ou erreur d'entrée-sortie, dont **refus d'écraser** un fichier existant |
| 3 | argument invalide (option inconnue, valeur manquante, format, `--dpi`, `--langue`) |
| 4 | taille inconnue (`--taille` absente de la gamme, ou projet sans taille) |

Ordre de contrôle : arguments (3), puis fichier (2 ou 1), puis taille (4), puis export (1). Tout est calculé
avant la première écriture ; un refus n'écrit rien.

## Options communes

`--langue fr|en` (défaut fr : messages et légendes des exports), `--ecraser` (sans elle, un fichier existant
est refusé, code 2), `--aide`.

## Sous-commandes

**`valider <fichier>`** : lecture seule, mêmes verdicts que `pf-validate` (`pf_core::diagnose`).
`patternforge-cli valider jupe.pforge.json` (0 valide, 1 invalide, 2 illisible).

**`convertir <fichier> --vers {svg,pdf,dxf,hpgl}`** : raccourci de `exporter` (png et jpeg refusés, code 3).
`patternforge-cli convertir jupe.pforge.json --vers pdf --sortie plans`

**`exporter <fichier> --format {pdf,svg,dxf,hpgl,png,jpeg} [--taille <nom>] [--tailles-toutes] [--sortie <dossier ou fichier>] [--dpi <36 à 1200>]`**
`patternforge-cli exporter jupe.pforge.json --format png --dpi 300 --tailles-toutes --sortie images`

- `--sortie` : dossier (créé au besoin) ; ou un fichier portant l'extension du format si un seul fichier est
  produit. Défaut : dossier du projet.
- `--taille M` : une taille de la gamme (casse ignorée) ; `--tailles-toutes` : un fichier par taille ; les deux
  ensemble sont refusés (3). Sans option : la base. Projet sans gamme : `--tailles-toutes` exporte la base.
- `--dpi` (png et jpeg seulement, défaut 300) : entier de 36 à 1200 ; pixels = `round(mm / 25,4 x dpi)`
  (100 mm à 300 dpi = 1181 px), PNG avec pHYs, JPEG avec densité JFIF, niveaux de gris, fond blanc.

**`lot <dossier> --format <f> --sortie <dossier> [--tailles-toutes] [--recursif] [--dpi n] [--rapport-csv <nom>]`**
`patternforge-cli lot projets --format svg --sortie sorties --recursif --rapport-csv rapport.csv`

- Traite tous les `*.pforge.json`, triés par chemin relatif ; un échec n'arrête pas le lot et est listé dans le
  rapport de fin (texte sur la sortie standard, CSV `fichier;statut;code;fichiers;message` dans le dossier de
  sortie, `--rapport-csv` est un **nom** de fichier sans séparateur). Code 1 si au moins un échec, sinon 0.
- Rien n'est écrit hors du dossier de sortie. Noms uniques sans tenir compte de la casse (`a`, `a-2`).

## Noms de fichiers produits (conventions non validées)

`<projet>.<ext>` pour la base ; `<projet>-<taille>` pour une autre taille ou `--tailles-toutes` ; `-<feuille>`
ajouté si plusieurs feuilles non vides (le PDF reste un seul fichier) ; HPGL : `-p<n>` par page (même découpage
que le PDF) ; extensions `pdf svg dxf plt png jpg`. Une feuille vide est ignorée et signalée.

## Limites

PNG / JPEG : tracés, droit-fil, coupe calculée, crans et pinces seulement (pas de textes, cotes, points,
marquages, zones ni boutons). Réglage de page = défaut (A4). Messages du chargeur de projet en français.
Hors périmètre de cet outil : macros (18.3) et impression directe. Le serveur MCP (US-18.1) existe : c'est un
binaire distinct, `pf-mcp-server` (`crates/pf-mcp/README.md`), dont l'outil `export` appelle les mêmes fonctions
que `exporter`.

# PatternForge

*English version: [README.md](README.md).*

Atelier de CAO 2D de patronage (couture et maroquinerie), écrit en Rust, hors ligne. Les pièces se
construisent en millimètres, avec des longueurs et des angles saisis, puis s'impriment à l'échelle 1:1.

> **État : prototype (0.1.0).** Rien n'a encore été validé par une modéliste, et aucune impression n'a
> été mesurée sur papier. Avant de couper dans la matière, imprimez l'étalon de 100 mm et mesurez-le.
> Les conventions métier (marges, crans, gradation, placement) restent à confirmer.

## Ce qu'on peut faire

- **Dessiner avec précision** : segment (longueur et angle), perpendiculaire, parallèle, point à
  distance, rectangle, arc, cercle, courbes avec longueur développée, accrochage, cotes.
- **Garder des liens, quand on le demande** : constructions maintenues, variables et formules, longueur
  imposée d'une courbe. Rien ne suit automatiquement sans ce choix.
- **Travailler les pièces** : feuilles, calques, extraction de pièces, marges de couture, crans,
  droit-fil, pinces, plis, perçages, boutonnières, contrôle d'assemblage.
- **Décliner** : tailles et gradation, variantes, barème de mesures et fiches client.
- **Maroquinerie** : lignes de piqûre, zones de parage et de collage, placement sur peau.
- **Placer et sortir** : plan de coupe, PDF A4 tuilé à l'échelle 1:1, SVG, DXF, HPGL, PNG et JPEG ;
  impression directe et projection.
- **Reprendre un existant** : import DXF, SVG et PDF vectoriel, photo ou scan calibré à décalquer.
- **Automatiser** : ligne de commande ([docs/CLI.md](docs/CLI.md)) et serveur MCP local, en lecture
  seule par défaut ([crates/pf-mcp](crates/pf-mcp/README.md)).

L'interface est en français et en anglais ([docs/I18N.md](docs/I18N.md)). Le détail de chaque outil,
avec ses limites, est dans le [guide utilisateur](docs/GUIDE_UTILISATEUR.md).

## Limites à connaître

- Pas de solveur de contraintes général.
- Pas de fichier de règles de gradation `.rul` ; la table des couches DXF de type AAMA / ASTM n'est pas
  confirmée, et aucune compatibilité avec un logiciel métier n'est annoncée.
- Le placement automatique est une aide, pas une optimisation industrielle.
- Développé et essayé sous Windows. Linux et macOS sont visés par la CI, mais l'application n'y a pas été
  essayée à la main. L'impression directe n'existe que sous Windows. Pas d'installateur ni de binaire signé.

## Compiler et lancer

Il faut une chaîne Rust stable récente (essayé avec la 1.90).

```bash
cargo run --release -p pf-desktop
```

L'exécutable est `target/release/patternforge` (`.exe` sous Windows). Il accepte le chemin d'un projet
à ouvrir, `--demo` pour un exercice de référence et `--lang fr|en`. Des projets d'exemple sont dans
`examples/`.

Sous Linux, installer d'abord les bibliothèques de fenêtrage et de dialogues (liste dans
[.github/workflows/ci.yml](.github/workflows/ci.yml)).

## macOS

L'application n'est pas signée auprès d'Apple. Un binaire téléchargé est donc bloqué à la première
ouverture : l'autoriser dans Réglages Système › Confidentialité et sécurité, ou retirer la quarantaine du
dossier décompressé :

```bash
xattr -dr com.apple.quarantine PatternForge-macos-arm64
```

L'archive produite par la CI vise les Mac à puce Apple ; sur un Mac Intel, compiler depuis les sources.
Les menus affichent « Ctrl » là où le raccourci utilise la touche Cmd.

## Format de fichier

Un projet est un fichier texte `.pforge.json`, versionné et documenté :
[format version 6](docs/FORMAT_PFORGE_V6.md) et son [schéma JSON](docs/pforge-v6.schema.json). Les
versions antérieures restent lisibles. Pour contrôler un fichier :

```bash
cargo run -q -p pf-core --bin pf-validate -- mon-projet.pforge.json
```

## Développement

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Les tests d'intégration de `pf-core` et `pf-export` forment un seul programme par paquet
(`tests/suite.rs`) : un nouveau fichier de test s'ajoute à sa liste.

| Dossier | Rôle |
|---|---|
| `crates/pf-geometry` | Calculs géométriques purs, en mm |
| `crates/pf-core` | Modèle, commandes d'édition, annuler / rétablir, fichiers |
| `crates/pf-export` | Exports et imports (PDF, SVG, DXF, HPGL, images) |
| `crates/pf-i18n` | Textes français et anglais |
| `crates/pf-ui` | Éléments d'interface partagés |
| `crates/pf-cli` | Ligne de commande |
| `crates/pf-mcp` | Serveur MCP local |
| `apps/pf-desktop` | Application (egui) |
| `examples/` | Projets d'exemple et exports de référence (comparés octet par octet par les tests) |

Les commentaires du code citent parfois des numéros de story, de tranche ou de décision : ils viennent
des documents de travail internes, qui ne sont pas publiés.

## Licence

[MIT](LICENSE). Composants tiers et leurs licences : [THIRD-PARTY.md](THIRD-PARTY.md).

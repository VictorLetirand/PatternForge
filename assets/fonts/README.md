# Police embarquée dans le PDF : Noto Sans Regular (sous-ensemble)

- **Famille** : Noto Sans Regular, version 2.015 (`ttfautohint v1.8.4.7`), 2022, The Noto Project Authors.
- **Licence** : SIL Open Font License 1.1, texte intégral dans `OFL.txt` (copié du dépôt amont, avec
  l'avis de copyright « Copyright 2022 The Noto Project Authors »). L'avis reste aussi dans la table
  `name` du fichier de police.
- **Source officielle** (téléchargée le 17/11/2026, rien d'autre n'a été téléchargé) :
  - police : `https://raw.githubusercontent.com/notofonts/notofonts.github.io/main/fonts/NotoSans/full/ttf/NotoSans-Regular.ttf`
  - licence : `https://raw.githubusercontent.com/notofonts/latin-greek-cyrillic/main/OFL.txt`
- **SHA-256**
  - `NotoSans-Regular.ttf` complet (825 628 octets, non conservé dans le dépôt) :
    `f5f552c8c5edb61fe6efb824baf4d4de47b1a8689ab4925ff43f7bd6a4ebece5`
  - `NotoSans-Regular-subset.ttf` (73 724 octets, **fichier livré**) :
    `c261055070251fd691be7f2f3066289239e9d92f0e50a8f93f6c664b0d1b8f65`
  - `OFL.txt` (4 396 octets) : `cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a`

## Sous-ensemble

`pyftsubset` (fonttools) n'était pas disponible : le sous-ensemble est produit par le sous-ensembleur
du crate `pf-export` (aucun outil tiers), de façon reproductible :

```
cargo run -p pf-export --example subset_police -- NotoSans-Regular.ttf NotoSans-Regular-subset.ttf
```

397 caractères : ASCII, Latin-1, Latin étendu A, ponctuation courante (tirets, guillemets, `…`, `‰`,
`′ ″`, `‹ ›`), `€`, `™`, `−`, grec majuscule et minuscule. Tables conservées : `head`, `hhea`,
`maxp`, `hmtx`, `loca`, `glyf`, `cmap` (format 12), `name`, `OS/2`, `post` (3.0), `cvt `, `fpgm`,
`prep` (programmes de hinting). Contours des glyphes **inchangés** ; seule la liste des glyphes est réduite.

Absents de Noto Sans (écrits « ? » dans le PDF) : `≤ ≥ ≠ ≈ ∞ ∈`, flèches, cyrillique, CJK.

Le PDF réduit encore cette police aux seuls glyphes écrits (`FontFile2`, préfixe de sous-ensemble).
Le binaire de l'application embarque ce fichier (`include_bytes!`) et n'accède à aucun réseau.

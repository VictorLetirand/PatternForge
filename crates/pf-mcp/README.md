# pf-mcp — serveur MCP local de PatternForge (lecture seule par défaut)

Expose un projet `.pforge.json` à un assistant externe via le Model Context
Protocol (JSON-RPC 2.0 sur stdio, un message JSON par ligne, journaux sur stderr).

## Lancement

```bash
cargo run -p pf-mcp --bin pf-mcp-server -- --project chemin/vers/projet.pforge.json
```

Pour activer les outils d'écriture (désactivés par défaut) : ajouter `--allow-write`
(voir « Outils d'écriture »).

Le fichier est relu à chaque appel : les enregistrements faits par l'application
sont visibles sans redémarrer le serveur.

## Ressource et outils

Unités : mm ; repère : X droite, Y haut, angles en degrés anti-horaires.

| Nom | Arguments | Résultat |
|---|---|---|
| `patternforge://project/summary` (resource) | | nom, `schema_version`, unité, repère, feuilles |
| `list_sheets` | | `revision` du fichier ; feuilles : id, nom, nombre de points et de tracés, pièce |
| `list_entities` | `sheet_id` | points (`x_mm`, `y_mm`, type de définition, rôle) et tracés |
| `measure_entity` | `sheet_id`, `entity_id` | segment : `length_mm`, `direction_deg` ; courbe : `length_mm` (développée), `chord_mm` |
| `measure_angle` | `sheet_id`, `first_entity_id`, `second_entity_id` | `angle_deg` (intérieur, 0–180) |
| `validate_project` | | `{"valid": true, "revision": "sha256:..."}` ou `{"valid": false, "error": "..."}` |

Les résultats d'outil portent `content` (JSON compact), `structuredContent` et
`isError`. Une erreur métier (feuille inconnue, fichier illisible) donne
`isError: true` ; un argument manquant ou mal typé donne l'erreur JSON-RPC -32602.
Les lignes vides sont ignorées ; une ligne de plus de 1 Mio est refusée (-32600).

## Outils d'écriture (`--allow-write`)

Sans `--allow-write`, ces outils sont absents de `tools/list` et tout appel est refusé
(`isError: true`, message citant `--allow-write`).

| Nom | Arguments | Effet |
|---|---|---|
| `preview_command` | `command` | applique la Command JSON de `pf-core` sur une copie en mémoire ; renvoie `outcome`, `notes`, `validation`, `revision` et `revision_after`. N'écrit rien (ni projet, ni journal). |
| `apply_command` | `command`, `expected_revision` (obligatoire) | refuse si la révision du fichier a changé ; sinon `Project::apply`, validation, sauvegarde atomique (`pf-core::persist`), renvoie la nouvelle `revision`. |
| `undo_last` | | restaure le fichier d'avant la dernière mutation non annulée faite par ce serveur ; refuse s'il n'y a rien à annuler ou si le fichier a changé depuis. |

- `command` : objet JSON `{"type": "rename_project", "name": "..."}` (forme sérialisée de
  `pf_core::Command`) ; une chaîne contenant ce JSON est aussi acceptée. JSON invalide ou type inconnu : erreur d'outil, rien d'écrit.
- Révision : `sha256:<hex>` des octets du fichier (renvoyée par `list_sheets` et `validate_project`).
  Flux : lire la révision, `preview_command`, puis `apply_command` avec cette révision.
- Aucune écriture si la commande échoue ou si la validation échoue.
- Journal : `<projet>.mcp-journal.jsonl` à côté du projet, une ligne par mutation ou annulation :
  `seq`, `kind` (`mutation`/`undo`), `revision_before`, `revision_after`, `command`, `notes`, et pour une
  mutation `snapshot_before` (contenu intégral du fichier d'avant, utilisé par `undo_last`).
  Le journal grossit à chaque mutation (un instantané par ligne) ; le supprimer efface l'historique d'annulation.
- Annulation : octet par octet depuis l'instantané ; l'annulation est elle-même journalisée, `undo_last`
  répété remonte donc l'historique.

## Modèle de sécurité

- Lecture seule par défaut ; l'écriture exige `--allow-write` au lancement (jamais activable par le client).
- Même avec `--allow-write` : uniquement le fichier de `--project` et son journal voisin.
- Un seul fichier : celui de `--project`, jamais de chemin fourni par le client.
- Aucun réseau, aucune exécution de commande, aucune dépendance hors `pf-core`/`serde`/`serde_json` (SHA-256 interne).
- Transport stdio local uniquement.

## Configuration d'un client MCP

(Ajouter `"--allow-write"` aux `args` pour autoriser l'écriture.)

```json
{
  "mcpServers": {
    "patternforge": {
      "command": "pf-mcp-server",
      "args": ["--project", "C:/chemin/vers/projet.pforge.json"]
    }
  }
}
```

## Autres outils

Lecture seule : `list_commands` (noms et schémas des 214 commandes), `grade_size` (`sheet_id`, `size_id` facultatif), `validate` (diagnostic localisé, `application_lock`). Avec `--allow-write` : `extract_piece`, `apply_seam_allowance` (même contrôle de révision et même journal que `apply_command`) et `export` (pdf, svg, dxf, hpgl, png, jpeg vers un chemin relatif au dossier du projet ; refus de `..`, de `.json`, `.lock`, des noms réservés Windows, de `:` (flux NTFS), d'un nom finissant par un point ou une espace, des dossiers `.assets` / `.versions`, d'une sortie du dossier par un lien ou une jonction, et de tout écrasement sans `overwrite`).

Verrou d'écriture `<projet>.mcp-write.lock` (repris après 45 s) ; empreinte relue juste avant l'écriture (« modification concurrente ») ; journal tourné au-delà de 8 Mio (variable `PF_MCP_JOURNAL_MAX_BYTES`) : courant + `.1` à `.4`, `undo_last` lit toute la fenêtre. L'application ouverte voit la modification au prochain contrôle d'enregistrement (elle refuse d'écraser) ; pas d'affichage en direct.

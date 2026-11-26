# -*- coding: utf-8 -*-
"""Rassemble les textes de licence des dependances, a joindre aux binaires distribues.

Usage : python scripts/licences_tierces.py   (apres `cargo fetch`) -> THIRD-PARTY-LICENSES.txt
"""
import json
import os
import re
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NOMS = re.compile(r"^(licen[cs]e|copying|notice|unlicense|copyright|ofl|ufl)", re.I)

meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT))
membres = set(meta["workspace_members"])
textes = {}  # texte -> liste de composants
sans = []

for paquet in sorted((p for p in meta["packages"] if p["id"] not in membres), key=lambda p: (p["name"], p["version"])):
    base = os.path.dirname(paquet["manifest_path"])
    nom = f"{paquet['name']} {paquet['version']} ({paquet.get('license') or 'voir le texte'})"
    fichiers = []
    for dossier, sous, noms in os.walk(base):
        profondeur = os.path.relpath(dossier, base).count(os.sep)
        if profondeur >= 1:
            sous[:] = []
        sous[:] = [s for s in sous if s not in ("target", "tests", "benches", "examples")]
        fichiers += [os.path.join(dossier, n) for n in noms if NOMS.match(n) and not n.endswith((".rs", ".toml"))]
    if not fichiers:
        sans.append(nom)
    for f in sorted(fichiers):
        with open(f, encoding="utf-8", errors="replace") as h:
            contenu = "\n".join(l.rstrip() for l in h.read().replace("\r\n", "\n").split("\n")).strip()
        if contenu:
            textes.setdefault(contenu, []).append(nom)

with open(os.path.join(ROOT, "assets", "fonts", "OFL.txt"), encoding="utf-8") as h:
    textes.setdefault(h.read().replace("\r\n", "\n").strip(), []).append("Noto Sans Regular (assets/fonts)")

sortie = [
    "Textes de licence des composants tiers de PatternForge / third-party license texts.",
    "Liste des composants : THIRD-PARTY.md.",
    "",
]
for contenu, composants in sorted(textes.items(), key=lambda kv: kv[1][0].lower()):
    sortie += ["=" * 78, "Applies to: " + ", ".join(dict.fromkeys(composants)), "=" * 78, contenu, ""]
if sans:
    sortie += ["=" * 78, "No license file in the package (see the SPDX expression):", "=" * 78, *sans, ""]
with open(os.path.join(ROOT, "THIRD-PARTY-LICENSES.txt"), "w", encoding="utf-8", newline="\n") as h:
    h.write("\n".join(sortie))
print(len(textes), "textes distincts,", len(sans), "paquets sans fichier de licence")

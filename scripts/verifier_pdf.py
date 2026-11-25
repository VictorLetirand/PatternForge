"""Contrôle indépendant des PDF de PatternForge avec MuPDF (pip install pymupdf).

Usage : python scripts/verifier_pdf.py fichier.pdf [dossier_images]

Vérifie avec un lecteur PDF tiers, sans réutiliser le code de PatternForge :
- que le fichier s'ouvre sans réparation et que chaque page est un A4 ;
- que la page 1 contient un carré de 100 x 100 mm ;
- qu'aucune image matricielle n'est incorporée (document vectoriel).
Affiche aussi la longueur des traits noirs de chaque page, en mm.
"""
import sys

import pymupdf

PT_TO_MM = 25.4 / 72.0


def main() -> int:
    path = sys.argv[1]
    out_dir = sys.argv[2] if len(sys.argv) > 2 else None
    doc = pymupdf.open(path)
    ok = True
    print(f"{path} : {doc.page_count} page(s), réparé à l'ouverture : {doc.is_repaired}")
    ok &= not doc.is_repaired
    for number, page in enumerate(doc, start=1):
        w, h = page.rect.width * PT_TO_MM, page.rect.height * PT_TO_MM
        a4 = abs(w - 210.0) < 0.01 and abs(h - 297.0) < 0.01
        ok &= a4 and not page.get_images()
        lengths, squares = [], []
        for drawing in page.get_drawings():
            black = drawing.get("color") == (0.0, 0.0, 0.0)
            for item in drawing["items"]:
                if item[0] == "l" and black:
                    length = (item[2] - item[1]).norm() * PT_TO_MM
                    if length > 5.0:
                        lengths.append(round(length, 3))
                elif item[0] == "re" and black:
                    r = item[1]
                    squares.append((round(r.width * PT_TO_MM, 3), round(r.height * PT_TO_MM, 3)))
                elif item[0] == "qu" and black:
                    q = item[1]
                    squares.append(
                        (round((q.ur - q.ul).norm() * PT_TO_MM, 3), round((q.ll - q.ul).norm() * PT_TO_MM, 3))
                    )
        print(f"  page {number} : {w:.2f} x {h:.2f} mm, A4 = {a4}, images = {len(page.get_images())}")
        if squares:
            print(f"    rectangles noirs (mm) : {squares}")
        if lengths:
            print(f"    traits noirs > 5 mm (mm) : {sorted(lengths, reverse=True)[:8]}")
        if number == 1:
            calibrated = any(abs(a - 100.0) < 0.01 and abs(b - 100.0) < 0.01 for a, b in squares)
            print(f"    carré étalon 100 x 100 mm trouvé : {calibrated}")
            ok &= calibrated
        if out_dir:
            page.get_pixmap(dpi=100).save(f"{out_dir}/page-{number}.png")
    print("RÉSULTAT :", "CONFORME" if ok else "NON CONFORME")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())

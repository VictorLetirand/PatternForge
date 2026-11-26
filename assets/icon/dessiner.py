# -*- coding: utf-8 -*-
"""Dessine l'icone de PatternForge : un perroquet (gabarit de courbes), qui forme aussi un P.

Usage : python assets/icon/dessiner.py   (Pillow requis)
"""
import os

from PIL import Image, ImageDraw

S = 4096  # dessin en grand puis reduction : bords lisses
ROUILLE, PAPIER = (155, 77, 38), (255, 255, 255)
ICI = os.path.dirname(os.path.abspath(__file__))


def pt(p):
    return (round(p[0] * S / 1024), round(p[1] * S / 1024))


def bez(p0, p1, p2, p3, n=240):
    out = []
    for i in range(n + 1):
        t = i / n
        u = 1 - t
        out.append(tuple(u**3 * p0[k] + 3 * u * u * t * p1[k] + 3 * u * t * t * p2[k] + t**3 * p3[k] for k in (0, 1)))
    return out


def disque(d, p, r, couleur):
    d.ellipse([pt((p[0] - r, p[1] - r)), pt((p[0] + r, p[1] + r))], fill=couleur)


def dessiner():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([pt((24, 24)), pt((1000, 1000))], radius=round(224 * S / 1024), fill=ROUILLE)
    forme = bez((230, 800), (170, 450), (360, 170), (660, 190))
    forme += bez((660, 190), (880, 205), (880, 480), (680, 490))[1:]
    forme += bez((680, 490), (540, 495), (560, 370), (650, 370))[1:]
    forme += bez((650, 370), (500, 300), (380, 520), (430, 690))[1:]
    forme += bez((430, 690), (470, 850), (260, 920), (230, 800))[1:]
    d.polygon([pt(p) for p in forme], fill=PAPIER)
    disque(d, (350, 760), 52, ROUILLE)
    disque(d, (720, 320), 62, ROUILLE)
    return img


if __name__ == "__main__":
    img = dessiner()
    img.resize((1024, 1024), Image.LANCZOS).save(os.path.join(ICI, "icon.png"), optimize=True)
    # 512 px : plus grande taille que l'empaqueteur sait mettre dans l'icône macOS (1024 fait échouer).
    img.resize((512, 512), Image.LANCZOS).save(os.path.join(ICI, "icon-512.png"), optimize=True)
    img.resize((256, 256), Image.LANCZOS).save(os.path.join(ICI, "icon-256.png"), optimize=True)
    img.resize((256, 256), Image.LANCZOS).save(os.path.join(ICI, "icon.ico"),
                                              sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
    print("icones ecrites dans", ICI)

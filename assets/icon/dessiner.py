# -*- coding: utf-8 -*-
"""Dessine l'icone de PatternForge (une piece de patron, sa marge et son droit-fil).

Usage : python assets/icon/dessiner.py   (Pillow requis)
"""
import os

from PIL import Image, ImageDraw

S = 4096  # dessin en grand puis reduction : bords lisses
FOND, PAPIER, TRAIT, MARGE, FIL = (31, 58, 92), (250, 250, 247), (20, 33, 51), (232, 140, 48), (30, 140, 90)
ICI = os.path.dirname(os.path.abspath(__file__))


def pt(x, y):
    return (round(x * S / 1024), round(y * S / 1024))


def bezier(p0, p1, p2, p3, n=60):
    out = []
    for i in range(n + 1):
        t = i / n
        u = 1 - t
        out.append(tuple(u**3 * p0[k] + 3 * u * u * t * p1[k] + 3 * u * t * t * p2[k] + t**3 * p3[k] for k in (0, 1)))
    return out


def contour():
    """Devant de corsage simplifie : encolure, epaule, emmanchure, cote, ourlet."""
    pts = [(300, 250)]
    pts += bezier((300, 250), (330, 360), (450, 380), (500, 360))[1:]  # encolure
    pts += [(500, 360)]
    pts = [(300, 840), (300, 330)]
    pts += bezier((300, 330), (390, 330), (440, 280), (450, 200))[1:]  # encolure
    pts += [(610, 240)]  # epaule
    pts += bezier((610, 240), (585, 370), (630, 450), (740, 465))[1:]  # emmanchure
    pts += [(705, 840)]  # cote
    return pts


def decaler(pts, d):
    """Contour parallele a distance d vers l'exterieur (onglets aux coins, comme une marge de couture)."""
    # Points confondus retires, puis sens du parcours (signe de l'aire).
    net = [p for i, p in enumerate(pts) if abs(p[0] - pts[i - 1][0]) + abs(p[1] - pts[i - 1][1]) > 1e-6]
    aire = sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(net, net[1:] + net[:1]))
    signe = 1 if aire > 0 else -1

    def normale(a, b):
        dx, dy = b[0] - a[0], b[1] - a[1]
        n = (dx * dx + dy * dy) ** 0.5
        return (signe * dy / n, -signe * dx / n)

    out = []
    for i, p in enumerate(net):
        n1 = normale(net[i - 1], p)
        n2 = normale(p, net[(i + 1) % len(net)])
        k = 1 + n1[0] * n2[0] + n1[1] * n2[1]
        k = max(k, 0.4)  # onglet borne aux angles aigus
        out.append((p[0] + d * (n1[0] + n2[0]) / k, p[1] + d * (n1[1] + n2[1]) / k))
    return out


def tirets(draw, pts, largeur, plein=46, vide=30):
    ferme = pts + [pts[0]]
    reste, trace = plein, True
    for (x0, y0), (x1, y1) in zip(ferme, ferme[1:]):
        longueur = ((x1 - x0) ** 2 + (y1 - y0) ** 2) ** 0.5
        pos = 0.0
        while pos < longueur:
            pas = min(reste, longueur - pos)
            if trace:
                a, b = pos / longueur, (pos + pas) / longueur
                draw.line([pt(x0 + (x1 - x0) * a, y0 + (y1 - y0) * a), pt(x0 + (x1 - x0) * b, y0 + (y1 - y0) * b)],
                          fill=MARGE, width=largeur)
            pos += pas
            reste -= pas
            if reste <= 1e-6:
                trace = not trace
                reste = plein if trace else vide


def dessiner():
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([pt(32, 32), pt(992, 992)], radius=round(210 * S / 1024), fill=FOND)
    piece = contour()
    coupe = decaler(piece, 58)
    d.polygon([pt(*p) for p in coupe], fill=PAPIER)
    tirets(d, coupe, round(20 * S / 1024))
    d.line([pt(*p) for p in piece + [piece[0], piece[1]]], fill=TRAIT, width=round(26 * S / 1024), joint="curve")
    # droit-fil a deux pointes
    d.line([pt(470, 500), pt(470, 750)], fill=FIL, width=round(24 * S / 1024))
    for y, s in ((470, 1), (780, -1)):
        d.polygon([pt(470, y), pt(432, y + s * 62), pt(508, y + s * 62)], fill=FIL)
    for p in (piece[0], piece[-1]):
        r = 24
        d.ellipse([pt(p[0] - r, p[1] - r), pt(p[0] + r, p[1] + r)], fill=TRAIT)
    return img


if __name__ == "__main__":
    img = dessiner()
    img.resize((1024, 1024), Image.LANCZOS).save(os.path.join(ICI, "icon.png"), optimize=True)
    img.resize((256, 256), Image.LANCZOS).save(os.path.join(ICI, "icon-256.png"), optimize=True)
    img.resize((256, 256), Image.LANCZOS).save(os.path.join(ICI, "icon.ico"),
                                              sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
    print("icones ecrites dans", ICI)

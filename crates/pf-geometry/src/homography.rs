//! Homographie du plan (US-14.1) : correction de perspective d'une photo par quatre points.
//!
//! `Homography::from_points(src, dst)` est l'unique transformation projective qui envoie chacun
//! des quatre points `src[i]` sur `dst[i]` (8 inconnues, 8 équations). Les coordonnées sont
//! normalisées (centrées, échelle moyenne 1) avant la résolution : des pixels (≈ 1e3 à 1e4) et des
//! millimètres (≈ 1e2) se mélangeraient mal dans un système non normalisé. Les trois points d'un
//! même quadruplet jamais alignés : sinon la transformation n'existe pas, refus expliqué.

use crate::{GeometryError, PointMm};

type Mat3 = [[f64; 3]; 3];

/// Transformation projective `p -> (h·(x, y, 1))` rapportée à sa troisième coordonnée.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Homography {
    m: Mat3,
}

fn err(msg: &str) -> GeometryError {
    GeometryError::InvalidConstruction(msg.to_string())
}

fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut r = [[0.0; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

fn det(m: &Mat3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn invert(m: &Mat3) -> Option<Mat3> {
    let d = det(m);
    if !d.is_finite() || d == 0.0 {
        return None;
    }
    let c =
        |r1: usize, c1: usize, r2: usize, c2: usize| m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1];
    let adj = [
        [c(1, 1, 2, 2), -c(0, 1, 2, 2), c(0, 1, 1, 2)],
        [-c(1, 0, 2, 2), c(0, 0, 2, 2), -c(0, 0, 1, 2)],
        [c(1, 0, 2, 1), -c(0, 0, 2, 1), c(0, 0, 1, 1)],
    ];
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = adj[i][j] / d;
        }
    }
    Some(r)
}

/// Normalisation : translation au centroïde, échelle pour que la distance moyenne au centre vaille 1.
fn normalization(p: &[PointMm; 4]) -> Option<Mat3> {
    let cx = p.iter().map(|q| q.x).sum::<f64>() / 4.0;
    let cy = p.iter().map(|q| q.y).sum::<f64>() / 4.0;
    let mean = p
        .iter()
        .map(|q| ((q.x - cx).powi(2) + (q.y - cy).powi(2)).sqrt())
        .sum::<f64>()
        / 4.0;
    if !mean.is_finite() || mean <= 0.0 {
        return None;
    }
    let s = 1.0 / mean;
    Some([[s, 0.0, -s * cx], [0.0, s, -s * cy], [0.0, 0.0, 1.0]])
}

fn apply_mat(m: &Mat3, p: PointMm) -> Option<PointMm> {
    let w = m[2][0] * p.x + m[2][1] * p.y + m[2][2];
    if !w.is_finite() || w.abs() < 1e-300 {
        return None;
    }
    let x = (m[0][0] * p.x + m[0][1] * p.y + m[0][2]) / w;
    let y = (m[1][0] * p.x + m[1][1] * p.y + m[1][2]) / w;
    (x.is_finite() && y.is_finite()).then_some(PointMm { x, y })
}

/// Vrai si trois des quatre points sont alignés (aire du triangle négligeable devant l'échelle).
pub fn has_collinear_triple(p: &[PointMm; 4]) -> bool {
    let scale = p
        .iter()
        .flat_map(|q| [q.x.abs(), q.y.abs()])
        .fold(0.0_f64, f64::max)
        .max(1e-300);
    for i in 0..4 {
        for j in (i + 1)..4 {
            for k in (j + 1)..4 {
                let cross =
                    (p[j].x - p[i].x) * (p[k].y - p[i].y) - (p[j].y - p[i].y) * (p[k].x - p[i].x);
                if cross.abs() <= 1e-9 * scale * scale {
                    return true;
                }
            }
        }
    }
    false
}

impl Homography {
    /// L'homographie qui envoie `src[i]` sur `dst[i]`. Refus : coordonnée non finie, trois points
    /// alignés d'un côté ou de l'autre, système singulier.
    pub fn from_points(
        src: &[PointMm; 4],
        dst: &[PointMm; 4],
    ) -> Result<Homography, GeometryError> {
        if src
            .iter()
            .chain(dst.iter())
            .any(|q| !q.x.is_finite() || !q.y.is_finite())
        {
            return Err(err("coordonnée non finie"));
        }
        if has_collinear_triple(src) || has_collinear_triple(dst) {
            return Err(err("trois des quatre points sont alignés"));
        }
        let ts = normalization(src).ok_or_else(|| err("points confondus"))?;
        let td = normalization(dst).ok_or_else(|| err("points confondus"))?;
        let s: Vec<PointMm> = src.iter().map(|q| apply_mat(&ts, *q).unwrap()).collect();
        let d: Vec<PointMm> = dst.iter().map(|q| apply_mat(&td, *q).unwrap()).collect();
        // h22 = 1 ; inconnues h0..h7.
        let mut a = [[0.0_f64; 9]; 8];
        for i in 0..4 {
            let (x, y, u, v) = (s[i].x, s[i].y, d[i].x, d[i].y);
            a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        // Élimination de Gauss à pivot partiel.
        for col in 0..8 {
            let piv = (col..8)
                .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))
                .unwrap();
            if a[piv][col].abs() < 1e-12 {
                return Err(err("système singulier (points mal répartis)"));
            }
            a.swap(col, piv);
            let pivot_row = a[col];
            for (r, row) in a.iter_mut().enumerate() {
                if r != col {
                    let f = row[col] / pivot_row[col];
                    for (x, y) in row.iter_mut().zip(pivot_row.iter()).skip(col) {
                        *x -= f * y;
                    }
                }
            }
        }
        let h: Vec<f64> = (0..8).map(|i| a[i][8] / a[i][i]).collect();
        let hn = [[h[0], h[1], h[2]], [h[3], h[4], h[5]], [h[6], h[7], 1.0]];
        let tdi = invert(&td).ok_or_else(|| err("normalisation singulière"))?;
        let m = mul(&mul(&tdi, &hn), &ts);
        if m.iter().flatten().any(|v| !v.is_finite()) {
            return Err(err("transformation non finie"));
        }
        Ok(Homography { m })
    }

    /// Image du point ; `None` si elle part à l'infini (troisième coordonnée nulle).
    pub fn apply(&self, p: PointMm) -> Option<PointMm> {
        apply_mat(&self.m, p)
    }

    /// Transformation inverse ; `None` si elle est singulière.
    pub fn inverse(&self) -> Option<Homography> {
        invert(&self.m).map(|m| Homography { m })
    }

    /// Coefficients (ligne par ligne), pour les tests et l'affichage.
    pub fn matrix(&self) -> [[f64; 3]; 3] {
        self.m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm { x, y }
    }

    #[test]
    fn rectangle_en_perspective_donne_les_coins_exacts_et_un_centre_connu() {
        // Rectangle 100 x 50 mm vu en perspective : coins en pixels choisis à la main.
        let px = [
            p(100.0, 80.0),
            p(520.0, 120.0),
            p(480.0, 360.0),
            p(60.0, 300.0),
        ];
        let mm = [p(0.0, 0.0), p(100.0, 0.0), p(100.0, 50.0), p(0.0, 50.0)];
        let h = Homography::from_points(&px, &mm).unwrap();
        for i in 0..4 {
            let q = h.apply(px[i]).unwrap();
            assert!((q.x - mm[i].x).abs() < 1e-9 && (q.y - mm[i].y).abs() < 1e-9);
        }
        // Le centre du rectangle est à l'intersection des diagonales (invariant projectif).
        // Diagonales : (100,80)-(480,360) et (520,120)-(60,300).
        let (x1, y1, x2, y2) = (100.0, 80.0, 480.0, 360.0);
        let (x3, y3, x4, y4) = (520.0, 120.0, 60.0, 300.0);
        let den = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
        let ix = ((x1 * y2 - y1 * x2) * (x3 - x4) - (x1 - x2) * (x3 * y4 - y3 * x4)) / den;
        let iy = ((x1 * y2 - y1 * x2) * (y3 - y4) - (y1 - y2) * (x3 * y4 - y3 * x4)) / den;
        let c = h.apply(p(ix, iy)).unwrap();
        assert!(
            (c.x - 50.0).abs() < 1e-9 && (c.y - 25.0).abs() < 1e-9,
            "{c:?}"
        );
        let back = h.inverse().unwrap().apply(c).unwrap();
        assert!((back.x - ix).abs() < 1e-9 && (back.y - iy).abs() < 1e-9);
    }

    #[test]
    fn homographie_connue_retrouvee() {
        // H connue : x' = (2x + 1) / (0,001 x + 1), y' = (3y) / (0,001 x + 1).
        let f = |q: PointMm| {
            let w = 0.001 * q.x + 1.0;
            p((2.0 * q.x + 1.0) / w, 3.0 * q.y / w)
        };
        let src = [p(0.0, 0.0), p(1000.0, 0.0), p(1000.0, 800.0), p(0.0, 800.0)];
        let dst = [f(src[0]), f(src[1]), f(src[2]), f(src[3])];
        let h = Homography::from_points(&src, &dst).unwrap();
        let t = p(400.0, 250.0);
        let (a, b) = (h.apply(t).unwrap(), f(t));
        assert!((a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6);
    }

    #[test]
    fn refus_points_alignes_confondus_non_finis() {
        let ok = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0), p(0.0, 5.0)];
        let aligned = [p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0), p(3.0, 5.0)];
        assert!(Homography::from_points(&aligned, &ok).is_err());
        assert!(Homography::from_points(&ok, &aligned).is_err());
        let same = [p(1.0, 1.0); 4];
        assert!(Homography::from_points(&same, &ok).is_err());
        let mut nan = ok;
        nan[2].x = f64::NAN;
        assert!(Homography::from_points(&nan, &ok).is_err());
    }
}

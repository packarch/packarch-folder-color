//! Détection et remplacement de la couleur des dossiers dans un SVG.
//!
//! Principe : on repère la couleur dominante du dossier (la couleur « saturée » la plus
//! fréquente), puis chaque couleur de la même famille est transposée vers la couleur
//! choisie en gardant son écart de teinte, de saturation et de luminosité. Les thèmes à
//! une seule couleur (Luna) comme ceux à plusieurs nuances (Papirus, Breeze) gardent
//! ainsi leur relief.

use regex::{Captures, Regex};
use std::collections::HashMap;
use std::sync::LazyLock;

static COLOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)((?:stop-color|flood-color|lighting-color|fill|stroke|color)\s*[:=]\s*["']?\s*)#([0-9a-f]{6}|[0-9a-f]{3})\b"#,
    )
    .unwrap()
});

/// Écart de teinte maximal (en degrés) pour qu'une couleur soit considérée
/// comme faisant partie du dossier.
const HUE_TOLERANCE: f64 = 40.0;
const MIN_SATURATION: f64 = 0.12;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(hex: &str) -> Option<Self> {
        let h = hex.trim().trim_start_matches('#');
        let v = |s: &str| u8::from_str_radix(s, 16).ok();
        match h.len() {
            6 => Some(Rgb(v(&h[0..2])?, v(&h[2..4])?, v(&h[4..6])?)),
            3 => {
                let d = |i: usize| v(&h[i..i + 1]).map(|x| x * 17);
                Some(Rgb(d(0)?, d(1)?, d(2)?))
            }
            _ => None,
        }
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    pub fn to_hsl(self) -> Hsl {
        let r = self.0 as f64 / 255.0;
        let g = self.1 as f64 / 255.0;
        let b = self.2 as f64 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d < 1e-9 {
            return Hsl { h: 0.0, s: 0.0, l };
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * (((g - b) / d).rem_euclid(6.0))
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        Hsl { h, s, l }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsl {
    pub h: f64,
    pub s: f64,
    pub l: f64,
}

impl Hsl {
    pub fn to_rgb(self) -> Rgb {
        let c = (1.0 - (2.0 * self.l - 1.0).abs()) * self.s;
        let hp = self.h.rem_euclid(360.0) / 60.0;
        let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match hp as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = self.l - c / 2.0;
        let f = |v: f64| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgb(f(r), f(g), f(b))
    }

    fn is_colorful(self) -> bool {
        self.s >= MIN_SATURATION && self.l > 0.06 && self.l < 0.94
    }
}

fn hue_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

fn colorful_counts(svg: &str) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for c in COLOR_RE.captures_iter(svg) {
        if let Some(rgb) = Rgb::parse(&c[2])
            && rgb.to_hsl().is_colorful() {
                // Un remplissage pèse plus qu'un contour : c'est lui qui fait la couleur
                // du dossier.
                let weight = if c[1].to_ascii_lowercase().starts_with("stroke") { 1 } else { 2 };
                *counts.entry(rgb.hex()).or_default() += weight;
            }
    }
    counts
}

/// Couleur dominante du dossier dans un SVG.
pub fn detect_color(svg: &str) -> Option<Rgb> {
    dominant(colorful_counts(svg))
}

/// Couleur du dossier dans un SVG, sachant que le thème utilise d'habitude `reference` :
/// on prend la couleur la plus fréquente proche de cette teinte, sinon la dominante du
/// fichier (thèmes dont les icônes n'ont pas toutes la même couleur).
pub fn detect_color_near(svg: &str, reference: Rgb) -> Option<Rgb> {
    let counts = colorful_counts(svg);
    let rh = reference.to_hsl().h;
    let near: HashMap<String, usize> = counts
        .iter()
        .filter(|(h, _)| {
            Rgb::parse(h).is_some_and(|c| hue_distance(c.to_hsl().h, rh) <= HUE_TOLERANCE)
        })
        .map(|(h, n)| (h.clone(), *n))
        .collect();
    dominant(near).or_else(|| dominant(counts))
}

fn dominant(counts: HashMap<String, usize>) -> Option<Rgb> {
    counts
        .into_iter()
        .filter_map(|(h, n)| Some((Rgb::parse(&h)?, n)))
        .max_by(|(a, na), (b, nb)| {
            na.cmp(nb)
                .then(a.to_hsl().s.total_cmp(&b.to_hsl().s))
                .then(a.hex().cmp(&b.hex()))
        })
        .map(|(c, _)| c)
}

/// Transpose une couleur `c` de la famille de `from` vers `to`.
fn shift(c: Hsl, from: Hsl, to: Hsl) -> Hsl {
    let s = if from.s > 1e-6 {
        (to.s * c.s / from.s).clamp(0.0, 1.0)
    } else {
        to.s
    };
    Hsl {
        h: (to.h + (c.h - from.h)).rem_euclid(360.0),
        s,
        l: (to.l + (c.l - from.l)).clamp(0.02, 0.98),
    }
}

/// Recolore un SVG. `reference` est la couleur habituelle des dossiers du thème ; la
/// couleur réelle de ce fichier est détectée à partir d'elle.
/// Renvoie `None` si aucune couleur n'a été modifiée.
pub fn recolor(svg: &str, reference: Rgb, to: Rgb) -> Option<String> {
    let from = detect_color_near(svg, reference)?.to_hsl();
    let to = to.to_hsl();
    let mut changed = false;
    let out = COLOR_RE.replace_all(svg, |c: &Captures| {
        let original = &c[0];
        let Some(rgb) = Rgb::parse(&c[2]) else {
            return original.to_string();
        };
        let hsl = rgb.to_hsl();
        if !hsl.is_colorful() || hue_distance(hsl.h, from.h) > HUE_TOLERANCE {
            return original.to_string();
        }
        changed = true;
        format!("{}{}", &c[1], shift(hsl, from, to).to_rgb().hex())
    });
    changed.then(|| out.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for hex in ["#0096ff", "#ffc86a", "#ff4747", "#5294e2", "#4877b1", "#808080"] {
            let c = Rgb::parse(hex).unwrap();
            assert_eq!(c.to_hsl().to_rgb(), c, "{hex}");
        }
    }

    #[test]
    fn single_color() {
        let svg = r##"<path style="fill:#000000;opacity:.2"/><path style="fill:#0096ff"/>"##;
        let from = detect_color(svg).unwrap();
        assert_eq!(from.hex(), "#0096ff");
        let out = recolor(svg, from, Rgb::parse("#ff4747").unwrap()).unwrap();
        assert!(out.contains("fill:#ff4747"));
        assert!(out.contains("fill:#000000"));
    }

    #[test]
    fn fill_beats_stroke() {
        let svg = r##"<path stroke="#003579"/><path fill="#6ba4e7"/>"##;
        assert_eq!(detect_color(svg).unwrap().hex(), "#6ba4e7");
    }

    #[test]
    fn file_with_other_color() {
        // Fichier rouge dans un thème dont la référence est bleue.
        let svg = r##"<path style="fill:#000000"/><path style="fill:#ff4747"/>"##;
        let out = recolor(svg, Rgb::parse("#0096ff").unwrap(), Rgb::parse("#33d17a").unwrap())
            .unwrap();
        assert!(out.contains("fill:#33d17a"));
    }

    #[test]
    fn keeps_shades_and_ids() {
        let svg = r##"<path fill="#4877b1"/><path fill="#5294e2"/><path fill="#5294e2"/><use href="#abc"/><path fill="url(#fade)"/>"##;
        let from = detect_color(svg).unwrap();
        assert_eq!(from.hex(), "#5294e2");
        let out = recolor(svg, from, Rgb::parse("#e25252").unwrap()).unwrap();
        assert!(out.contains(r##"fill="#e25252""##));
        assert!(!out.contains("#4877b1"));
        assert!(out.contains(r##"href="#abc""##));
        assert!(out.contains("url(#fade)"));
    }
}

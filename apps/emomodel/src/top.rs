use glam::Vec3;

pub(super) const COVERAGE_ROWS: usize = 128;
pub(super) const FIRST_TOP_ROW: usize = COVERAGE_ROWS / 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pastel {
    Rose,
    Peach,
    Butter,
    Mint,
    Sky,
    Lavender,
    Lilac,
    Aqua,
}

impl Pastel {
    pub(super) const ALL: [Self; 8] = [
        Self::Rose,
        Self::Peach,
        Self::Butter,
        Self::Mint,
        Self::Sky,
        Self::Lavender,
        Self::Lilac,
        Self::Aqua,
    ];

    fn swatch(self) -> (&'static str, [u8; 3]) {
        match self {
            Self::Rose => ("Rose", [247, 180, 201]),
            Self::Peach => ("Peach", [252, 195, 167]),
            Self::Butter => ("Butter", [253, 230, 151]),
            Self::Mint => ("Mint", [176, 228, 183]),
            Self::Sky => ("Sky", [172, 211, 250]),
            Self::Lavender => ("Lavender", [186, 175, 242]),
            Self::Lilac => ("Lilac", [221, 180, 239]),
            Self::Aqua => ("Aqua", [157, 224, 227]),
        }
    }

    pub(super) fn name(self) -> &'static str {
        self.swatch().0
    }

    pub(super) fn rgb(self) -> [u8; 3] {
        self.swatch().1
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct TopStyle {
    pub color: Option<Pastel>,
    pub left_sleeve: f32,
    pub right_sleeve: f32,
    pub hem: f32,
}

pub(super) struct Fit {
    x: f32,
    y: f32,
    neckline: f32,
}

impl Fit {
    pub(super) fn at(position: Vec3) -> Self {
        // The embedded body faces -Z and its left arm extends toward -X.
        // Raise the collar across the shoulders and on the back of the neck.
        let shoulder = ((position.x.abs() - 0.015) / 0.060).clamp(0.0, 1.0);
        let shoulder = shoulder * shoulder * (3.0 - 2.0 * shoulder);
        let back = ((position.z + 0.045) / 0.10).clamp(0.0, 1.0);
        Self {
            x: position.x,
            y: position.y,
            neckline: 1.204 + 0.025 * back + 0.042 * shoulder - position.y,
        }
    }

    pub(super) fn coverage_row(&self, style: TopStyle) -> f32 {
        let cuff = |length| 0.145 + (0.535 - 0.145) * length;
        let hem = 1.04 + (0.755 - 1.04) * style.hem;
        let signed_distance = (self.y - hem)
            .min(cuff(style.left_sleeve) + self.x)
            .min(cuff(style.right_sleeve) - self.x)
            .min(self.neckline);
        // Preserve the signed distance across triangles that cross an edge.
        // The indexed shader samples the two halves of the tile with nearest
        // filtering, so interpolated coverage gives a sharp fabric edge.
        (FIRST_TOP_ROW as f32 + 96.0 * signed_distance).clamp(0.5, COVERAGE_ROWS as f32 - 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_covers_the_torso_and_adjusts_sleeves_and_hem_independently() {
        let short = TopStyle {
            color: Some(Pastel::Lavender),
            ..TopStyle::default()
        };
        let long_left = TopStyle {
            left_sleeve: 1.0,
            ..short
        };
        let long_right = TopStyle {
            right_sleeve: 1.0,
            ..short
        };
        let low_hem = TopStyle { hem: 1.0, ..short };
        let painted = |position, style| {
            Fit::at(Vec3::from_array(position)).coverage_row(style) >= FIRST_TOP_ROW as f32
        };

        for style in [short, long_left, long_right, low_hem] {
            assert!(painted([0.04, 1.12, -0.12], style)); // Chest
            assert!(painted([0.04, 1.12, 0.05], style)); // Back
            assert!(!painted([0.01, 1.28, 0.00], style)); // Neck
        }
        for (side, matching, opposite) in
            [(-1.0, long_left, long_right), (1.0, long_right, long_left)]
        {
            assert!(painted([side * 0.14, 1.22, -0.02], short));
            let forearm = [side * 0.53, 1.21, -0.02];
            assert!(!painted(forearm, short));
            assert!(!painted(forearm, low_hem));
            assert!(!painted(forearm, opposite));
            assert!(painted(forearm, matching));
            assert!(!painted([side * 0.55, 1.21, -0.02], matching));
        }
        for height in [0.99, 0.80, 0.76] {
            let front = [0.06, height, -0.09];
            assert!(!painted(front, short));
            assert!(!painted(front, long_left));
            assert!(!painted(front, long_right));
            assert!(painted(front, low_hem));
        }
        assert!(!painted([0.06, 0.74, -0.09], low_hem));
    }

    #[test]
    fn pastel_palette_has_distinct_swatches() {
        for (index, color) in Pastel::ALL.into_iter().enumerate() {
            assert!(color.rgb().into_iter().all(|channel| channel >= 128));
            for earlier in &Pastel::ALL[..index] {
                assert_ne!(color.name(), earlier.name());
                assert_ne!(color.rgb(), earlier.rgb());
            }
        }
    }
}

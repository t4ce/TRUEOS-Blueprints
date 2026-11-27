use glam::Vec3;
use std::f32::consts::TAU;

pub(super) mod chart;
pub(super) mod finish;
pub(super) mod trim;

pub(super) const COVERAGE_ROWS: usize = 128;
const OPENING_ROW: usize = 32;
const COLLAR_ROW: usize = 100;
const OPENING_TRIM_END: usize = OPENING_ROW + Surface::TRIM.len();
const COLLAR_TRIM_START: usize = COLLAR_ROW - Surface::TRIM.len();
const FABRIC_ROW: f32 = 68.5;
const COLLAR_FABRIC_ROW: f32 = 66.5;
const OPENING_ROWS_PER_METER: f32 = 500.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Surface {
    Skin,
    Fabric,
    Band,
    Thread,
    Outline,
}

impl Surface {
    const TRIM: [Self; 10] = [
        Self::Outline,
        Self::Band,
        Self::Band,
        Self::Thread,
        Self::Band,
        Self::Band,
        Self::Thread,
        Self::Band,
        Self::Band,
        Self::Outline,
    ];

    pub(super) const ALL: [Self; 5] = [
        Self::Skin,
        Self::Fabric,
        Self::Band,
        Self::Thread,
        Self::Outline,
    ];

    pub(super) fn at_row(row: usize) -> Self {
        match row {
            ..OPENING_ROW | COLLAR_ROW.. => Self::Skin,
            OPENING_ROW..OPENING_TRIM_END => Self::TRIM[row - OPENING_ROW],
            COLLAR_TRIM_START..COLLAR_ROW => Self::TRIM[COLLAR_ROW - 1 - row],
            _ => Self::Fabric,
        }
    }

    pub(super) fn albedo(self, color: Pastel) -> [u8; 3] {
        match self {
            Self::Skin => crate::light::SKIN_SRGB,
            Self::Fabric => color.rgb(),
            Self::Band => color
                .rgb()
                .map(|channel| (f32::from(channel) * 0.75).round() as u8),
            Self::Thread => [178, 175, 181],
            Self::Outline => [53, 50, 58],
        }
    }
}

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
    pub socks: Option<f32>,
}

pub(super) struct Fit {
    position: Vec3,
    neckline: f32,
}

#[derive(Clone, Copy)]
enum Opening {
    Hem,
    LeftCuff,
    RightCuff,
    Sock(f32),
}

impl Fit {
    pub(super) fn at(position: Vec3) -> Self {
        // The embedded body faces -Z and its left arm extends toward -X.
        // Raise the collar across the shoulders and on the back of the neck.
        let across = position.x.abs();
        let shoulder = smooth((across - 0.009) / 0.075);
        let back = smooth((position.z + 0.023) / 0.078);
        // Ease the depth-dependent rise through the crease at the shoulder.
        let side = 0.62 * smooth((across - 0.025) / 0.035);
        let back = back + side * (0.57 - back);
        Self {
            position,
            neckline: 1.204 + 0.025 * back + 0.042 * shoulder - position.y,
        }
    }

    fn near_collar(&self) -> bool {
        self.position.x.abs() < 0.10 && self.position.y > 1.15
    }

    fn opening(&self, style: TopStyle) -> (f32, Opening) {
        let Vec3 { x, y, z } = self.position;
        let cuff = |length: f32| {
            let vertical = round_limit((y - (1.207 + 0.009 * length)) / (0.034 - 0.016 * length));
            let depth =
                ((z - (0.017 + 0.004 * length)) / (0.036 - 0.011 * length)).clamp(-1.0, 1.0);
            0.145 + (0.535 - 0.145) * length + 0.006 * (1.0 - vertical * vertical) - 0.003 * depth
        };
        let across = round_limit(x / (0.085 + 0.050 * style.hem));
        let depth = ((z + 0.012) / 0.10).clamp(-1.0, 1.0);
        // Even the lowest part of the curved hem stays above the leg split.
        let hem =
            1.04 + (0.835 - 1.04) * style.hem + 0.018 * (across * across - 0.5) - 0.004 * depth;
        let left = cuff(style.left_sleeve) + x;
        let right = cuff(style.right_sleeve) - x;
        let hem = y - hem;
        let shirt = if hem <= left.min(right) {
            (hem, Opening::Hem)
        } else if left <= right {
            (left, Opening::LeftCuff)
        } else {
            (right, Opening::RightCuff)
        };
        if let Some(length) = style.socks {
            // The highest cuff stays below the point where the legs join.
            let height = 0.115 + (0.780 - 0.115) * length;
            let depth = round_limit((z - 0.020) / 0.060);
            let sock = height - y + 0.003 * depth;
            if sock > shirt.0 {
                return (sock, Opening::Sock(height));
            }
        }
        shirt
    }

    fn collar_row(&self) -> f32 {
        (COLLAR_ROW as f32 - 2.0 * OPENING_ROWS_PER_METER * self.neckline)
            .clamp(COLLAR_FABRIC_ROW, COVERAGE_ROWS as f32 - 0.5)
    }

    pub(super) fn coverage_row(&self, style: TopStyle) -> f32 {
        if self.near_collar() {
            self.collar_row()
        } else {
            opening_row(self.opening(style).0)
        }
    }

    pub(crate) fn trim_coordinates(&self, style: TopStyle) -> (f32, f32) {
        let Vec3 { x, y, z } = self.position;
        let (row, angle, circumference) = if self.near_collar() {
            (self.collar_row(), (x / 0.9).atan2(z), 0.46)
        } else {
            let (distance, opening) = self.opening(style);
            let (angle, circumference) = match opening {
                Opening::Hem => ((x / 1.3).atan2(z - 0.015), 0.61 + 0.26 * style.hem),
                Opening::LeftCuff | Opening::RightCuff => {
                    let length = match opening {
                        Opening::LeftCuff => style.left_sleeve,
                        _ => style.right_sleeve,
                    };
                    let vertical = y - (1.207 + 0.009 * length);
                    let depth = z - (0.017 + 0.004 * length);
                    (vertical.atan2(depth), 0.30 - 0.15 * length)
                }
                Opening::Sock(height) => {
                    let below_knee = 1.0 - smooth((height - 0.350) / 0.220);
                    let calf = smooth((height - 0.170) / 0.180) * below_knee;
                    let thigh = smooth((height - 0.450) / 0.330);
                    let center_z = 0.030 * below_knee + 0.010 * calf;
                    let across = (x.abs() - 0.070) / 0.8;
                    (
                        across.atan2(z - center_z),
                        0.180 + 0.075 * calf + 0.210 * thigh,
                    )
                }
            };
            (opening_row(distance), angle, circumference)
        };
        // A whole number of stitches around each opening keeps the pattern
        // continuous across the angle wrap.
        let pitch = trim::STITCH_PERIOD_METERS;
        let perimeter = (circumference / pitch).round() * pitch;
        (row, angle * (perimeter / TAU))
    }
}

fn opening_row(distance: f32) -> f32 {
    (OPENING_ROW as f32 + OPENING_ROWS_PER_METER * distance).clamp(0.5, FABRIC_ROW)
}

fn smooth(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn round_limit(value: f32) -> f32 {
    let magnitude = value.abs().min(1.0);
    let edge = ((magnitude - 0.8) / 0.2).max(0.0);
    (magnitude + 0.2 * edge * edge * (1.0 - edge)).copysign(value)
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
            let row = Fit::at(Vec3::from_array(position)).coverage_row(style);
            Surface::at_row(row as usize) != Surface::Skin
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
        for height in [0.99, 0.90, 0.84] {
            let front = [0.06, height, -0.09];
            assert!(!painted(front, short));
            assert!(!painted(front, long_left));
            assert!(!painted(front, long_right));
            assert!(painted(front, low_hem));
        }
        assert!(!painted([0.06, 0.82, -0.09], low_hem));
    }

    #[test]
    fn the_oval_endings_sample_outline_band_and_two_threads() {
        let style = TopStyle::default();
        let row = |position| Fit::at(Vec3::from_array(position)).coverage_row(style);
        for side in [-1.0, 1.0] {
            let bowed = row([side * 0.148, 1.207, 0.017]);
            let upper = row([side * 0.148, 1.241, 0.017]);
            assert!(bowed >= OPENING_ROW as f32 && upper < OPENING_ROW as f32);
        }
        let hem_center = row([0.0, 1.04, -0.012]);
        let hem_side = row([0.075, 1.04, -0.012]);
        assert!(hem_center >= OPENING_ROW as f32 && hem_side < OPENING_ROW as f32);

        use Surface::{Band, Fabric, Outline, Skin, Thread};
        let trim = [
            Skin, Outline, Band, Band, Thread, Band, Band, Thread, Band, Band, Outline, Fabric,
        ];
        assert_eq!((31..43).map(Surface::at_row).collect::<Vec<_>>(), trim);
    }

    #[test]
    fn the_fixed_collar_uses_the_opening_trim_at_half_width() {
        let shortest = TopStyle::default();
        let longest = TopStyle {
            left_sleeve: 1.0,
            right_sleeve: 1.0,
            hem: 1.0,
            ..shortest
        };
        for offset in -1..=Surface::TRIM.len() as i32 {
            let distance = (offset as f32 + 0.5) / OPENING_ROWS_PER_METER;
            let cuff = Fit::at(Vec3::new(0.151 - distance, 1.207, 0.017));
            let collar = Fit::at(Vec3::new(0.0, 1.204 - distance * 0.5, -0.045));
            let collar_row = collar.coverage_row(shortest);
            assert_eq!(collar.coverage_row(longest), collar_row);
            assert_eq!(
                Surface::at_row(collar_row as usize),
                Surface::at_row(cuff.coverage_row(shortest) as usize)
            );
        }
    }

    #[test]
    fn the_atlas_ranges_meet_in_plain_fabric_on_the_avatar_mesh() {
        let glb = gltf::Gltf::from_slice(crate::HUMANOID_GLB).unwrap();
        let body = glb
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .find(|primitive| primitive.material().name() == Some(crate::mesh::BODY_MATERIAL))
            .unwrap();
        let reader = body.reader(|_| glb.blob.as_deref());
        let fits: Vec<_> = reader
            .read_positions()
            .unwrap()
            .map(|position| Fit::at(Vec3::from_array(position)))
            .collect();
        let indices: Vec<_> = reader.read_indices().unwrap().into_u32().collect();
        let shortest = TopStyle::default();
        let mut crossing = 0;
        for &[a, b, c] in indices.as_chunks::<3>().0 {
            let triangle = [a, b, c].map(|index| &fits[index as usize]);
            let count = triangle.iter().filter(|fit| fit.near_collar()).count();
            if (1..=2).contains(&count) {
                crossing += 1;
                for fit in triangle {
                    assert_eq!(
                        Surface::at_row(fit.coverage_row(shortest) as usize),
                        Surface::Fabric
                    );
                }
            }
        }
        assert!(crossing > 0);
    }

    #[test]
    fn pastel_palette_has_distinct_swatches() {
        for (index, color) in Pastel::ALL.into_iter().enumerate() {
            assert!(color.rgb().into_iter().all(|channel| channel >= 128));
            for earlier in &Pastel::ALL[..index] {
                assert_ne!(color.name(), earlier.name());
                assert_ne!(color.rgb(), earlier.rgb());
            }
            let [_, fabric, band, thread, outline] =
                Surface::ALL.map(|surface| surface.albedo(color));
            assert!(band.into_iter().zip(fabric).all(|(trim, body)| trim < body));
            assert!(
                outline
                    .into_iter()
                    .zip(thread)
                    .all(|(edge, stitch)| edge < stitch)
            );
        }
    }
}

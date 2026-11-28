use glam::Vec3;
use std::f32::consts::TAU;

mod bottom;
pub(super) mod chart;
pub(super) mod finish;
pub(super) mod outfit_motion;
mod shoe;
pub(super) mod trim;

#[cfg(target_os = "trueos")]
pub(super) use bottom::space_sock_length;

pub(super) const COVERAGE_ROWS: usize = 128;
const OPENING_ROW: usize = 32;
const COLLAR_ROW: usize = 100;
const OPENING_TRIM_END: usize = OPENING_ROW + Surface::TRIM.len();
const COLLAR_TRIM_START: usize = COLLAR_ROW - Surface::TRIM.len();
const FABRIC_ROW: f32 = 68.5;
const COLLAR_FABRIC_ROW: f32 = 66.5;
const OPENING_ROWS_PER_METER: f32 = 500.0;
const OPENING_WIDTH_METERS: f32 = (OPENING_TRIM_END - OPENING_ROW) as f32 / OPENING_ROWS_PER_METER;
const HALF_WIDTH_METERS: f32 = OPENING_WIDTH_METERS * 0.5;
const BLACK_FABRIC_SRGB: [u8; 3] = [22, 22, 22];
const WHITE_RIBBON_SRGB: [u8; 3] = [241, 239, 233];
const PANTS_VERTEX_ROW: usize = 52;
const SHIRT_VERTEX_ROW: usize = 68;
const SOCK_VERTEX_ROW: usize = 80;
pub(super) const SHOE_VERTEX_ROW: usize = COVERAGE_ROWS - 1;
const SHIRT_UNDERLAP_METERS: f32 = 0.004;
const JOIN_GAP_METERS: f32 = 0.002;
const PANT_TRIM_FADE_METERS: f32 = 0.008;
const SOCK_CONTOUR_METERS: f32 = 0.003;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Surface {
    Skin,
    Fabric,
    Band,
    Thread,
    Outline,
    Sock,
    SockBand,
    Shirt,
    ShirtBand,
    Shoe,
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

    pub(super) const ALL: [Self; 10] = [
        Self::Skin,
        Self::Fabric,
        Self::Band,
        Self::Thread,
        Self::Outline,
        Self::Sock,
        Self::SockBand,
        Self::Shirt,
        Self::ShirtBand,
        Self::Shoe,
    ];

    pub(super) fn at_row(row: usize) -> Self {
        match row {
            ..OPENING_ROW | COLLAR_ROW.. => Self::Skin,
            OPENING_ROW..OPENING_TRIM_END => Self::TRIM[row - OPENING_ROW],
            COLLAR_TRIM_START..COLLAR_ROW => Self::TRIM[COLLAR_ROW - 1 - row],
            _ => Self::Fabric,
        }
    }

    pub(super) fn albedo(self, fabric: [u8; 3]) -> [u8; 3] {
        match self {
            Self::Skin => crate::light::SKIN_SRGB,
            Self::Fabric | Self::Sock | Self::Shirt | Self::Shoe => fabric,
            Self::Band | Self::SockBand | Self::ShirtBand => {
                fabric.map(|channel| (f32::from(channel) * 0.75).round() as u8)
            }
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
    pub sock_color: Option<Pastel>,
    pub bottoms: Option<BottomStyle>,
    pub pants_color: Option<Pastel>,
    pub shoe_color: Option<Pastel>,
}

impl TopStyle {
    pub(super) fn lower_style(self) -> Option<LowerStyle> {
        let pants = self.bottoms?;
        Some(LowerStyle {
            pants,
            socks: self.socks,
            hem: self.hem,
            waist_join: self.color.map_or(0.0, |_| {
                bottom::shirt_waist_join_strength(self.hem, pants.waist)
            }),
        })
    }

    pub(crate) fn fabric(self) -> Option<[u8; 3]> {
        if self.bottoms.is_some() {
            Some(self.pants_color.map_or(BLACK_FABRIC_SRGB, |color| {
                color
                    .rgb()
                    .map(|channel| (f32::from(channel) * 0.82).round() as u8)
            }))
        } else {
            self.color.map(Pastel::rgb)
        }
    }

    pub(crate) fn surface_at_row(self, row: usize) -> Surface {
        if row == SHOE_VERTEX_ROW && self.shoe_color.is_some() {
            return Surface::Shoe;
        }
        if self.bottoms.is_some() {
            return match row {
                ..OPENING_ROW => Surface::Skin,
                SHIRT_VERTEX_ROW..SOCK_VERTEX_ROW if self.color.is_some() => Surface::Shirt,
                SOCK_VERTEX_ROW..COLLAR_ROW => Surface::Sock,
                COLLAR_ROW.. => Surface::Skin,
                _ => Surface::Fabric,
            };
        }
        Surface::at_row(row)
    }

    pub(crate) fn albedo(self, surface: Surface) -> [u8; 3] {
        let fabric = self.fabric().unwrap_or(BLACK_FABRIC_SRGB);
        match surface {
            Surface::Band if self.bottoms.is_some() => WHITE_RIBBON_SRGB,
            Surface::Sock | Surface::SockBand => {
                surface.albedo(self.sock_color.or(self.color).map_or(fabric, Pastel::rgb))
            }
            Surface::Shirt | Surface::ShirtBand => {
                surface.albedo(self.color.map_or(fabric, Pastel::rgb))
            }
            Surface::Shoe => self.shoe_color.map_or(BLACK_FABRIC_SRGB, Pastel::rgb),
            _ => surface.albedo(fabric),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct OutfitAppearance {
    pub style: TopStyle,
    pub colors: [[u8; 3]; Surface::ALL.len()],
    pub opacity: f32,
}

impl OutfitAppearance {
    pub(super) fn from_style(style: TopStyle) -> Self {
        Self {
            style,
            colors: Surface::ALL.map(|surface| style.albedo(surface)),
            opacity: 1.0,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct BottomStyle {
    pub left_leg: f32,
    pub right_leg: f32,
    pub waist: f32,
}

#[derive(Clone, Copy)]
pub(super) struct LowerStyle {
    pants: BottomStyle,
    socks: Option<f32>,
    hem: f32,
    waist_join: f32,
}

impl LowerStyle {
    fn fit_at(self, position: Vec3) -> LowerFit {
        let mut fit = LowerFit::at(position, self.pants, self.socks);
        if self.waist_join > 0.0 {
            let under_shirt = shirt_hem_height(position, self.hem) + SHIRT_UNDERLAP_METERS;
            let extension = (under_shirt - position.y - fit.waist).max(0.0);
            fit.waist += extension * self.waist_join;
        }
        fit
    }
}

#[derive(Clone, Copy)]
struct LowerFit {
    waist: f32,
    leg: f32,
    sock: Option<(f32, f32)>,
}

impl LowerFit {
    fn at(position: Vec3, bottoms: BottomStyle, socks: Option<f32>) -> Self {
        let [waist, leg] =
            bottom::edges(position, bottoms.left_leg, bottoms.right_leg, bottoms.waist);
        Self {
            waist,
            leg,
            sock: socks.map(|length| sock_opening(position, length)),
        }
    }

    fn gap(self) -> f32 {
        self.sock
            .map_or(f32::INFINITY, |(sock, _)| -(self.leg + sock))
    }

    fn cuff_separation(self) -> f32 {
        smooth((self.gap() - JOIN_GAP_METERS) / PANT_TRIM_FADE_METERS)
    }

    fn sock_edge(self) -> Option<(f32, f32)> {
        self.sock.map(|(distance, height)| {
            let separation = self.cuff_separation();
            let edge = -self.leg * (1.0 - separation) + distance * separation;
            (edge, height)
        })
    }

    fn vertex_row(self) -> f32 {
        let row = if self.waist >= 0.0 && self.leg >= 0.0 {
            PANTS_VERTEX_ROW
        } else if self
            .sock_edge()
            .is_some_and(|(distance, _)| distance >= 0.0)
        {
            SOCK_VERTEX_ROW
        } else {
            0
        };
        row as f32 + 0.5
    }
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
        let left = cuff(style.left_sleeve) + x;
        let right = cuff(style.right_sleeve) - x;
        let hem = y - shirt_hem_height(self.position, style.hem);
        let shirt = if hem <= left.min(right) {
            (hem, Opening::Hem)
        } else if left <= right {
            (left, Opening::LeftCuff)
        } else {
            (right, Opening::RightCuff)
        };
        if style.bottoms.is_none()
            && let Some(length) = style.socks
        {
            let (sock, height) = sock_opening(self.position, length);
            if sock > shirt.0 {
                return (sock, Opening::Sock(height));
            }
        }
        shirt
    }

    fn collar_row(&self) -> f32 {
        (COLLAR_ROW as f32 - 2.0 * OPENING_ROWS_PER_METER * self.neckline)
            .clamp(COLLAR_FABRIC_ROW, SHOE_VERTEX_ROW as f32 - 0.5)
    }

    pub(super) fn coverage_row(&self, style: TopStyle, lower: Option<LowerStyle>) -> f32 {
        if style.shoe_color.is_some()
            && self.position.y <= shoe::MAX_HEIGHT_METERS
            && shoe::edge(self.position) >= 0.0
        {
            return SHOE_VERTEX_ROW as f32 + 0.5;
        }
        if let Some(lower) = lower {
            if style.color.is_some()
                && Surface::at_row(self.shirt_row(style) as usize) != Surface::Skin
            {
                return SHIRT_VERTEX_ROW as f32 + 0.5;
            }
            lower.fit_at(self.position).vertex_row()
        } else if style.shoe_color.is_some() && style.color.is_none() {
            0.5
        } else {
            self.shirt_row(style)
        }
    }

    fn shirt_row(&self, style: TopStyle) -> f32 {
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
                Opening::Sock(height) => leg_phase(self.position, height),
            };
            (opening_row(distance), angle, circumference)
        };
        (row, stitch_along(angle, circumference))
    }
}

fn shirt_hem_height(Vec3 { x, z, .. }: Vec3, hem: f32) -> f32 {
    let across = round_limit(x / (0.085 + 0.050 * hem));
    let depth = ((z + 0.012) / 0.10).clamp(-1.0, 1.0);
    // Even the lowest part of the curved hem stays above the leg split.
    1.04 + (0.835 - 1.04) * hem + 0.018 * (across * across - 0.5) - 0.004 * depth
}

fn sock_opening(position: Vec3, length: f32) -> (f32, f32) {
    // The highest cuff stays below the point where the legs join.
    let height = sock_height(length);
    let depth = round_limit((position.z - 0.020) / 0.060);
    (height - position.y + SOCK_CONTOUR_METERS * depth, height)
}

fn sock_height(length: f32) -> f32 {
    0.115 + (0.780 - 0.115) * length.clamp(0.0, 1.0)
}

fn leg_phase(position: Vec3, height: f32) -> (f32, f32) {
    let below_knee = 1.0 - smooth((height - 0.350) / 0.220);
    let calf = smooth((height - 0.170) / 0.180) * below_knee;
    let thigh = smooth((height - 0.450) / 0.330);
    let center_z = 0.030 * below_knee + 0.010 * calf;
    let across = (position.x.abs() - 0.070) / 0.8;
    (
        across.atan2(position.z - center_z),
        0.180 + 0.075 * calf + 0.210 * thigh,
    )
}

fn stitch_along(angle: f32, circumference: f32) -> f32 {
    // A whole number of stitches keeps the pattern continuous at the angle wrap.
    let pitch = trim::STITCH_PERIOD_METERS;
    let perimeter = (circumference / pitch).round() * pitch;
    angle * (perimeter / TAU)
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
            let row = Fit::at(Vec3::from_array(position)).coverage_row(style, None);
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
        let row = |position| Fit::at(Vec3::from_array(position)).coverage_row(style, None);
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
            let collar_row = collar.coverage_row(shortest, None);
            assert_eq!(collar.coverage_row(longest, None), collar_row);
            assert_eq!(
                Surface::at_row(collar_row as usize),
                Surface::at_row(cuff.coverage_row(shortest, None) as usize)
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
                        Surface::at_row(fit.coverage_row(shortest, None) as usize),
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
            let [fabric, band, thread, outline] = [
                Surface::Fabric,
                Surface::Band,
                Surface::Thread,
                Surface::Outline,
            ]
            .map(|surface| surface.albedo(color.rgb()));
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

use super::{BottomStyle, OutfitAppearance, Surface, TopStyle, smooth};

const FADE_NS: u64 = 350_000_000;
const SHRINK_NS: u64 = 650_000_000;
const FINISH_NS: u64 = 1_750_000_000;

pub(crate) struct OutfitMotion {
    from: OutfitAppearance,
    target: OutfitAppearance,
    started_ns: u64,
}

impl OutfitMotion {
    pub(crate) fn new(from: OutfitAppearance, target: TopStyle, now_ns: u64) -> Self {
        Self {
            from,
            target: OutfitAppearance::from_style(target),
            started_ns: now_ns,
        }
    }

    pub(crate) fn is_active(&self, now_ns: u64) -> bool {
        now_ns.saturating_sub(self.started_ns) < FINISH_NS
    }

    pub(crate) fn sample(&self, now_ns: u64) -> OutfitAppearance {
        let elapsed = now_ns.saturating_sub(self.started_ns);
        if elapsed >= FINISH_NS {
            return self.target;
        }

        let mut middle = OutfitAppearance {
            style: minimal(self.target.style),
            ..self.from
        };
        let intro = (self.from.style.color.is_none() && self.from.style.bottoms.is_none())
            || self.from.opacity == 0.0;
        let grow_start = if intro {
            middle.colors = self.target.colors;
            let skin = self.from.colors[Surface::Skin as usize];
            for fabric in [Surface::Fabric, Surface::Sock, Surface::Shirt] {
                middle.colors[fabric as usize] = skin;
            }
            if elapsed < FADE_NS {
                middle.opacity = smooth(elapsed as f32 / FADE_NS as f32);
                return middle;
            }
            middle.opacity = 1.0;
            FADE_NS
        } else {
            if elapsed < SHRINK_NS {
                let progress = smooth(elapsed as f32 / SHRINK_NS as f32);
                return OutfitAppearance {
                    style: interpolate_fit(self.from.style, minimal(self.from.style), progress),
                    ..self.from
                };
            }
            let skin = self.from.colors[Surface::Skin as usize];
            for (was_present, fabric, band) in [
                (
                    self.from.style.bottoms.is_some(),
                    Surface::Fabric,
                    Surface::Band,
                ),
                (
                    self.from.style.socks.is_some(),
                    Surface::Sock,
                    Surface::SockBand,
                ),
                (
                    self.from.style.color.is_some(),
                    Surface::Shirt,
                    Surface::ShirtBand,
                ),
            ] {
                if !was_present {
                    middle.colors[fabric as usize] = skin;
                    middle.colors[band as usize] = self.target.colors[band as usize];
                }
            }
            SHRINK_NS
        };

        let progress = smooth((elapsed - grow_start) as f32 / (FINISH_NS - grow_start) as f32);
        let colors = std::array::from_fn(|surface| {
            let start = middle.colors[surface];
            let end = self.target.colors[surface];
            std::array::from_fn(|channel| {
                mix(f32::from(start[channel]), f32::from(end[channel]), progress).round() as u8
            })
        });
        OutfitAppearance {
            style: interpolate_fit(middle.style, self.target.style, progress),
            colors,
            opacity: mix(middle.opacity, self.target.opacity, progress),
        }
    }
}

fn minimal(style: TopStyle) -> TopStyle {
    TopStyle {
        left_sleeve: 0.0,
        right_sleeve: 0.0,
        hem: 0.0,
        socks: style.socks.map(|_| 0.0),
        bottoms: style.bottoms.map(|_| BottomStyle::default()),
        ..style
    }
}

fn interpolate_fit(from: TopStyle, to: TopStyle, progress: f32) -> TopStyle {
    TopStyle {
        left_sleeve: mix(from.left_sleeve, to.left_sleeve, progress),
        right_sleeve: mix(from.right_sleeve, to.right_sleeve, progress),
        hem: mix(from.hem, to.hem, progress),
        socks: from.socks.zip(to.socks).map(|(a, b)| mix(a, b, progress)),
        bottoms: from.bottoms.zip(to.bottoms).map(|(a, b)| BottomStyle {
            left_leg: mix(a.left_leg, b.left_leg, progress),
            right_leg: mix(a.right_leg, b.right_leg, progress),
            waist: mix(a.waist, b.waist, progress),
        }),
        ..from
    }
}

fn mix(from: f32, to: f32, progress: f32) -> f32 {
    from + (to - from) * progress
}

#[cfg(test)]
mod tests {
    use super::super::Pastel;
    use super::*;

    fn outfit(color: Pastel) -> TopStyle {
        TopStyle {
            color: Some(color),
            sock_color: Some(color),
            pants_color: Some(color),
            left_sleeve: 0.8,
            right_sleeve: 0.8,
            hem: 0.6,
            socks: Some(0.8),
            bottoms: Some(BottomStyle {
                left_leg: 0.6,
                right_leg: 0.6,
                waist: 0.4,
            }),
        }
    }

    #[test]
    fn first_outfit_fades_in_at_minimal_fit_then_grows_in_color() {
        let skin = OutfitAppearance::from_style(TopStyle::default());
        let target = OutfitAppearance::from_style(outfit(Pastel::Lavender));
        let motion = OutfitMotion::new(skin, target.style, 20);

        let start = motion.sample(20);
        assert!(start.style == minimal(target.style));
        assert_eq!(start.opacity, 0.0);
        assert_eq!(
            start.colors[Surface::Fabric as usize],
            skin.colors[Surface::Skin as usize]
        );
        for band in [Surface::Band, Surface::SockBand, Surface::ShirtBand] {
            assert_eq!(start.colors[band as usize], target.colors[band as usize]);
        }
        let faded = motion.sample(20 + FADE_NS);
        assert!(faded.style == start.style);
        assert_eq!(faded.opacity, 1.0);

        let grown = motion.sample(20 + (FADE_NS + FINISH_NS) / 2);
        assert_eq!(grown.style.left_sleeve, 0.4);
        assert_eq!(grown.style.right_sleeve, 0.4);
        assert_ne!(grown.colors, start.colors);
        assert_ne!(grown.colors, target.colors);
        let end = motion.sample(20 + FINISH_NS);
        assert!(!motion.is_active(20 + FINISH_NS));
        assert!(end.style == target.style);
        assert_eq!(end.colors, target.colors);
        assert_eq!(end.opacity, target.opacity);
    }

    #[test]
    fn reroll_retargets_from_current_fit_opacity_and_palette_before_growing() {
        let skin = OutfitAppearance::from_style(TopStyle::default());
        let rose = OutfitMotion::new(skin, outfit(Pastel::Rose), 0);
        let sample_time = FADE_NS / 2;
        let current = rose.sample(sample_time);
        let target = OutfitAppearance::from_style(outfit(Pastel::Mint));
        let motion = OutfitMotion::new(current, target.style, sample_time);

        let start = motion.sample(sample_time);
        assert!(start.style == current.style);
        assert_eq!(start.colors, current.colors);
        assert_eq!(start.opacity, current.opacity);
        let middle = motion.sample(sample_time + SHRINK_NS);
        assert!(motion.is_active(sample_time + SHRINK_NS));
        assert_eq!(middle.style.left_sleeve, 0.0);
        assert_eq!(middle.colors, current.colors);
        assert_eq!(middle.opacity, current.opacity);

        let grown = motion.sample(sample_time + (SHRINK_NS + FINISH_NS) / 2);
        assert_eq!(grown.style.left_sleeve, 0.4);
        assert!(grown.opacity > current.opacity);
        assert_ne!(grown.colors, current.colors);
        let end = motion.sample(sample_time + FINISH_NS);
        assert!(end.style == target.style);
        assert_eq!(end.colors, target.colors);
        assert_eq!(end.opacity, 1.0);
    }
}

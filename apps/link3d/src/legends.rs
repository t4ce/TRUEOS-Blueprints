extern crate alloc;
use crate::scene::{BucketCaption, CaptionKind, row_spacing};
use alloc::{format, string::String, vec::Vec};
use trueos::{
    hid::{InputComboSourceKind, TrueosInputCombo},
    ui4_scene::{
        self, Error, Font, FontSpriteRequest, FontSpriteStatus, FontSpriteTicket, Frame,
        SpriteCorner, SpriteQuad,
    },
    vgpu::RetainedCamera,
};

#[derive(Clone, Copy, Debug)]
pub struct Legend {
    pub scalar: char,
    pub mesh: usize,
    pub center: [f32; 3],
    pub normal: [f32; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub usage: Option<u8>,
    pub mesh: usize,
    pub center: [f32; 3],
    pub normal: [f32; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct WordLegend {
    pub text: &'static str,
    pub mesh: usize,
    pub center: [f32; 3],
    pub normal: [f32; 3],
    pub width: f32,
}
include!(concat!(env!("OUT_DIR"), "/keyboard.legends.rs"));
include!(concat!(env!("OUT_DIR"), "/keyboard.keys.rs"));
include!(concat!(env!("OUT_DIR"), "/keyboard.words.rs"));

pub const TEXTURE_HEIGHT: f32 = 64.0;
pub const EXTRA_STROKE_PX: f32 = 1.5;
pub const GLYPH_HEIGHT: f32 = 0.7;
const KEY_HIGHLIGHT_INNER_SIZE: f32 = 1.34;
const GLYPH_TOP_INSET: f32 = 0.05;
// Captions reuse the keyboard's capitals/digits, plus shared service masks for
// mixed-case source tags. This is only display text; combo IDs remain unchanged.
const CAPTION_GLYPHS: &str = "abcdefghijklmnopqrstuvwxyz/-._?";

fn glyph_scalars() -> impl Iterator<Item = char> {
    LEGENDS
        .iter()
        .map(|legend| legend.scalar)
        .chain(CAPTION_GLYPHS.chars())
}

/// Masks belong to the shared kernel font service. One window-owned glyph
/// texture is reused for the matching key on every board.
pub struct Legends {
    glyphs: Vec<(Option<FontSpriteTicket>, Option<u32>)>,
    bucket_names: Vec<(u32, String)>,
}
impl Default for Legends {
    fn default() -> Self {
        Self {
            glyphs: alloc::vec![(None, None); glyph_scalars().count()],
            bucket_names: Vec::new(),
        }
    }
}
impl Legends {
    pub fn set_buckets(&mut self, combos: &[TrueosInputCombo]) -> bool {
        let names = bucket_names(combos);
        if names == self.bucket_names {
            return false;
        }
        self.bucket_names = names;
        true
    }

    pub fn ready(&self) -> bool {
        self.glyphs.iter().all(|(_, sprite)| sprite.is_some())
    }

    /// Poll outside the writable UI4 lease. Never hold up input or the first mesh frame.
    pub fn poll(&mut self, frame: &mut Frame) -> Result<bool, Error> {
        let mut changed = false;
        for (scalar, (ticket, sprite)) in glyph_scalars().zip(&mut self.glyphs) {
            if sprite.is_some() {
                continue;
            }
            if ticket.is_none() {
                match frame.request_font_sprite_with_stroke(
                    FontSpriteRequest {
                        font: Font::Default,
                        scalar,
                        font_pixels: TEXTURE_HEIGHT,
                        color_rgba: ui4_scene::rgba(0, 0, 0, 255),
                    },
                    EXTRA_STROKE_PX,
                ) {
                    Ok(value) => *ticket = Some(value),
                    Err(Error::Busy) => break,
                    Err(error) => return Err(error),
                }
            }
            match frame.font_sprite_status(ticket.unwrap()) {
                Ok(FontSpriteStatus::Ready { sprite_id, .. }) => {
                    *sprite = Some(sprite_id);
                    changed = true;
                }
                Ok(FontSpriteStatus::Pending) | Err(Error::Busy) => {}
                Ok(FontSpriteStatus::Failed) => return Err(Error::InvalidState),
                Err(error) => return Err(error),
            }
        }
        Ok(changed)
    }

    pub fn draw(
        &self,
        frame: &mut Frame,
        camera: &RetainedCamera,
        width: u32,
        height: u32,
        boards: usize,
        radius: f32,
        feedback: &crate::feedback::Feedback,
        captions: &[BucketCaption],
        offsets: &[f32],
    ) -> Result<(), Error> {
        let quads = self.overlay_quads(
            camera, width, height, boards, radius, feedback, captions, offsets,
        );
        if !quads.is_empty() {
            frame.draw_sprite_quads(&quads)?;
        }
        Ok(())
    }

    fn overlay_quads(
        &self,
        camera: &RetainedCamera,
        width: u32,
        height: u32,
        boards: usize,
        radius: f32,
        feedback: &crate::feedback::Feedback,
        captions: &[BucketCaption],
        offsets: &[f32],
    ) -> Vec<SpriteQuad> {
        // A cold stroke variant is built asynchronously on Render0. In
        // particular, caption backings must not start a Render0 sprite pass
        // while their missing glyphs are still being rasterized there.
        // The keyboard can already render/publish independently on Render1.
        if !self.ready() {
            return Vec::new();
        }
        let mut quads = Vec::with_capacity(boards * LEGENDS.len());
        // Constant-color quads use UI4's existing white pixel. Glyph textures
        // remain shared, black, and untinted; blank keys receive feedback too.
        for board in 0..boards {
            let offset = offsets.get(board).copied().unwrap_or(0.);
            for (index, key) in KEYS.iter().enumerate() {
                if !feedback.active(board, index) {
                    continue;
                }
                let mut center = key.center;
                center[0] += offset;
                for (size, color) in [
                    (1.50, ui4_scene::rgba(255, 255, 255, 235)),
                    (KEY_HIGHLIGHT_INNER_SIZE, ui4_scene::rgba(255, 32, 32, 235)),
                ] {
                    let Some(corners) = project_plane(
                        center,
                        key.normal,
                        camera,
                        width,
                        height,
                        board as f32 * row_spacing(radius),
                        [size; 2],
                    ) else {
                        continue;
                    };
                    quads.push(SpriteQuad {
                        sprite_id: 0,
                        c0: corners[0],
                        c1: corners[1],
                        c2: corners[2],
                        c3: corners[3],
                        color_rgba: color,
                        source_over: true,
                    });
                }
            }
        }
        // Group repeated glyphs so the sprite renderer shares texture bindings.
        for (legend, (_, sprite)) in LEGENDS.iter().zip(&self.glyphs) {
            let Some(sprite) = *sprite else {
                continue;
            };
            for board in 0..boards {
                let offset = offsets.get(board).copied().unwrap_or(0.);
                let mut legend = *legend;
                legend.center[0] += offset;
                if let Some(corners) = project_quad(
                    &legend,
                    camera,
                    width,
                    height,
                    board as f32 * row_spacing(radius),
                ) {
                    quads.push(SpriteQuad {
                        sprite_id: sprite,
                        c0: corners[0],
                        c1: corners[1],
                        c2: corners[2],
                        c3: corners[3],
                        // The service already colored the texture black.
                        color_rgba: ui4_scene::rgba(255, 255, 255, 255),
                        source_over: true,
                    });
                }
                for word in WORD_LEGENDS {
                    let mut word = *word;
                    word.center[0] += offset;
                    for (index, scalar) in word.text.chars().enumerate() {
                        if scalar != legend.scalar {
                            continue;
                        }
                        if let Some(corners) = project_word_quad(
                            &word,
                            index,
                            camera,
                            width,
                            height,
                            board as f32 * row_spacing(radius),
                        ) {
                            quads.push(SpriteQuad {
                                sprite_id: sprite,
                                c0: corners[0],
                                c1: corners[1],
                                c2: corners[2],
                                c3: corners[3],
                                color_rgba: ui4_scene::rgba(255, 255, 255, 255),
                                source_over: true,
                            });
                        }
                    }
                }
            }
        }
        for caption in captions {
            let mut center = caption.center;
            center[0] += offsets.get(caption.instance).copied().unwrap_or(0.);
            let Some([x, y]) = project_caption_anchor(center, camera, width, height) else {
                continue;
            };
            let text = caption_text(caption.combo_id, caption.kind, &self.bucket_names);
            const HEIGHT: f32 = 16.;
            const CELL: f32 = HEIGHT * 6. / 11.;
            let left = x - text.chars().count() as f32 * CELL * 0.5;
            let top = y - HEIGHT * 0.5;
            quads.push(screen_quad(
                0,
                left - 4.,
                top - 3.,
                text.chars().count() as f32 * CELL + 8.,
                HEIGHT + 6.,
                ui4_scene::rgba(255, 255, 255, 220),
            ));
            for (index, scalar) in text.chars().enumerate() {
                if scalar == ' ' {
                    continue;
                }
                let Some(sprite) = glyph_scalars()
                    .position(|glyph| glyph == scalar)
                    .or_else(|| glyph_scalars().position(|glyph| glyph == '?'))
                    .and_then(|i| self.glyphs[i].1)
                else {
                    continue;
                };
                quads.push(screen_quad(
                    sprite,
                    left + index as f32 * CELL,
                    top,
                    CELL,
                    HEIGHT,
                    ui4_scene::rgba(255, 255, 255, 255),
                ));
            }
        }
        quads
    }
}

/// A small presentation-only mapping of the existing combo metadata. The
/// ordinal distinguishes sources of the same kind in this inventory snapshot.
fn bucket_names(combos: &[TrueosInputCombo]) -> Vec<(u32, String)> {
    combos
        .iter()
        .enumerate()
        .map(|(index, combo)| {
            let kind = match InputComboSourceKind::from_code(combo.source_kind) {
                InputComboSourceKind::Ai => "AI",
                InputComboSourceKind::Human => "Human",
                InputComboSourceKind::Remote => "Remote",
                InputComboSourceKind::Unknown => "Other",
            };
            let number = 1 + combos[..index]
                .iter()
                .filter(|c| c.source_kind == combo.source_kind)
                .count();
            let length = usize::from(combo.source_tag_len).min(combo.source_tag.len());
            let tag = core::str::from_utf8(&combo.source_tag[..length])
                .unwrap_or("")
                .trim();
            let label = if tag.is_empty() || matches!(tag, "auto" | "udp" | "ai") {
                format!("Bucket {kind} {number}")
            } else {
                let tag = tag
                    .split('/')
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join(" / ");
                format!("Bucket {kind} / {tag} {number}")
            };
            (combo.combo_id, label)
        })
        .collect()
}

fn caption_text(combo_id: u32, kind: CaptionKind, names: &[(u32, String)]) -> String {
    match kind {
        CaptionKind::Mouse => return String::from("MOUSE"),
        CaptionKind::Tablet => return String::from("TABLET"),
        CaptionKind::Bucket => {}
    }
    if combo_id == 0 {
        String::from("Unassigned")
    } else {
        names
            .iter()
            .find(|(id, _)| *id == combo_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| String::from("Bucket"))
    }
}

/// Captions follow their bucket in 3D but face the viewer at a readable UI size.
fn project_caption_anchor(
    center: [f32; 3],
    camera: &RetainedCamera,
    width: u32,
    height: u32,
) -> Option<[f32; 2]> {
    let point = [center[0], center[1], center[2], 1.];
    let clip: [f32; 4] = core::array::from_fn(|row| {
        (0..4)
            .map(|col| camera.view_projection[col * 4 + row] * point[col])
            .sum()
    });
    if !clip.iter().all(|v| v.is_finite()) || clip[3] <= 0. || clip[2] < 0. || clip[2] > clip[3] {
        return None;
    }
    Some([
        (clip[0] / clip[3] + 1.) * width as f32 * 0.5,
        (1. - clip[1] / clip[3]) * height as f32 * 0.5,
    ])
}

fn screen_quad(
    sprite_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color_rgba: u32,
) -> SpriteQuad {
    let corners = [(0., 0.), (1., 0.), (1., 1.), (0., 1.)].map(|(u, v)| SpriteCorner {
        x: x + u * width,
        y: y + v * height,
        u,
        v,
    });
    SpriteQuad {
        sprite_id,
        c0: corners[0],
        c1: corners[1],
        c2: corners[2],
        c3: corners[3],
        color_rgba,
        source_over: true,
    }
}

/// A flat 6:11 cell inset from the red key highlight's top edge, 0.025 units above
/// the key. Capitals are horizontally centered; digits retain their left offset.
/// UI4 consumes the four projected corners and samples the service's alpha.
/// This first pass is an overlay: top-facing rejection, not a depth attachment.
pub fn project_quad(
    legend: &Legend,
    camera: &RetainedCamera,
    width: u32,
    height: u32,
    board_z: f32,
) -> Option<[SpriteCorner; 4]> {
    // Keyboard-local left is -X and the number-row direction is -Z. Follow
    // the sloped top plane so the offset keeps the same clearance while orbiting.
    let x = if legend.scalar.is_ascii_uppercase() {
        0.0
    } else {
        -0.4
    };
    let z = -KEY_HIGHLIGHT_INNER_SIZE * 0.5 + GLYPH_HEIGHT * 0.5 + GLYPH_TOP_INSET;
    let mut center = legend.center;
    center[0] += x;
    center[1] -= (legend.normal[0] * x + legend.normal[2] * z) / legend.normal[1];
    center[2] += z;
    project_plane(
        center,
        legend.normal,
        camera,
        width,
        height,
        board_z,
        [GLYPH_HEIGHT * 6.0 / 11.0, GLYPH_HEIGHT],
    )
}

fn project_word_quad(
    word: &WordLegend,
    index: usize,
    camera: &RetainedCamera,
    width: u32,
    height: u32,
    board_z: f32,
) -> Option<[SpriteCorner; 4]> {
    let count = word.text.len() as f32;
    let glyph_height = GLYPH_HEIGHT.min(word.width / (count * 6. / 11.));
    let cell = glyph_height * 6. / 11.;
    let x = (index as f32 + 0.5 - count * 0.5) * cell;
    let z = -KEY_HIGHLIGHT_INNER_SIZE * 0.5 + glyph_height * 0.5 + GLYPH_TOP_INSET;
    let mut center = word.center;
    center[0] += x;
    center[1] -= (word.normal[0] * x + word.normal[2] * z) / word.normal[1];
    center[2] += z;
    project_plane(
        center,
        word.normal,
        camera,
        width,
        height,
        board_z,
        [cell, glyph_height],
    )
}

fn project_plane(
    mut center: [f32; 3],
    normal: [f32; 3],
    camera: &RetainedCamera,
    width: u32,
    height: u32,
    board_z: f32,
    size: [f32; 2],
) -> Option<[SpriteCorner; 4]> {
    center[2] += board_z;
    let facing: f32 = (0..3)
        .map(|i| normal[i] * (camera.position_near[i] - center[i]))
        .sum();
    if facing <= 0.0 {
        return None;
    }
    let mut corners = [SpriteCorner::default(); 4];
    for (corner, (u, v)) in corners
        .iter_mut()
        .zip([(0., 0.), (1., 0.), (1., 1.), (0., 1.)])
    {
        let x = (u - 0.5) * size[0];
        let z = (v - 0.5) * size[1];
        let point = [
            center[0] + x,
            center[1] - (normal[0] * x + normal[2] * z) / normal[1],
            center[2] + z,
            1.0,
        ];
        let clip: [f32; 4] = core::array::from_fn(|row| {
            (0..4)
                .map(|col| camera.view_projection[col * 4 + row] * point[col])
                .sum()
        });
        if !clip.iter().all(|x| x.is_finite())
            || clip[3] <= 0.0
            || clip[2] < 0.0
            || clip[2] > clip[3]
        {
            return None;
        }
        *corner = SpriteCorner {
            x: (clip[0] / clip[3] + 1.) * width as f32 * 0.5,
            y: (1. - clip[1] / clip[3]) * height as f32 * 0.5,
            u,
            v,
        };
    }
    if corners.iter().all(|c| c.x < 0.)
        || corners.iter().all(|c| c.x > width as f32)
        || corners.iter().all(|c| c.y < 0.)
        || corners.iter().all(|c| c.y > height as f32)
    {
        return None;
    }
    Some(corners)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OrbitCamera;

    #[test]
    fn control_words_follow_the_audited_keys_including_both_sides_and_fn() {
        assert_eq!(WORD_LEGENDS.len(), 10);
        for (usage, text) in [
            (Some(0x2b), "TAB"),
            (Some(0x39), "CAPS"),
            (Some(0xe1), "SHIFT"),
            (Some(0xe5), "SHIFT"),
            (Some(0xe0), "STRG"),
            (Some(0xe4), "STRG"),
            (Some(0xe2), "ALT"),
            (Some(0xe6), "ALTGR"),
            (Some(0x28), "ENTER"),
            (None, "FN"),
        ] {
            let key = KEYS.iter().find(|key| key.usage == usage).unwrap();
            let word = WORD_LEGENDS
                .iter()
                .find(|word| word.mesh == key.mesh)
                .unwrap();
            assert_eq!(word.text, text);
            assert!(word.width > 1.3);
            assert!(
                word.text
                    .chars()
                    .all(|scalar| LEGENDS.iter().any(|l| l.scalar == scalar))
            );
        }
        let enter = WORD_LEGENDS.iter().find(|w| w.text == "ENTER").unwrap();
        let tab = WORD_LEGENDS.iter().find(|w| w.text == "TAB").unwrap();
        assert_eq!(enter.center[2], tab.center[2]);
        assert!(enter.width > GLYPH_HEIGHT * 6. / 11. * 5.);
        let mut legends = Legends::default();
        for (index, glyph) in legends.glyphs.iter_mut().enumerate() {
            glyph.1 = Some(index as u32 + 100);
        }
        let mut orbit = OrbitCamera::new(20.);
        orbit.set_scene(
            [0., 0., row_spacing(20.) * 0.5],
            20. + row_spacing(20.) * 0.5,
        );
        let camera = orbit.retained(800, 600, [0.; 16]);
        let feedback = crate::feedback::Feedback::default();
        let quads = legends.overlay_quads(&camera, 800, 600, 2, 20., &feedback, &[], &[]);
        assert_eq!(
            quads.len(),
            (LEGENDS.len() + WORD_LEGENDS.iter().map(|w| w.text.len()).sum::<usize>()) * 2
        );
        assert_eq!(legends.glyphs.len(), 36 + CAPTION_GLYPHS.chars().count()); // Words reuse the resident alphabet.
    }

    #[test]
    fn control_words_stay_centered_fit_the_top_and_preserve_texture_aspect() {
        let mut camera = RetainedCamera::default();
        camera.position_near = [0., 10., 0., 0.01];
        camera.view_projection[0] = 0.01;
        camera.view_projection[9] = -0.01;
        camera.view_projection[14] = 0.5;
        camera.view_projection[15] = 1.;
        for word in WORD_LEGENDS {
            let first = project_word_quad(word, 0, &camera, 800, 800, 0.).unwrap();
            let last = project_word_quad(word, word.text.len() - 1, &camera, 800, 800, 0.).unwrap();
            let center_x = (word.center[0] * 0.01 + 1.) * 400.;
            assert!(((first[0].x + last[2].x) * 0.5 - center_x).abs() < 1e-4);
            assert!(last[2].x - first[0].x <= word.width * 4. + 1e-4);
            let top_y = (1. + (word.center[2] - 0.62) * 0.01) * 400.;
            assert!((first[0].y - top_y).abs() < 1e-4);
            let width = first[1].x - first[0].x;
            let height = first[3].y - first[0].y;
            assert!((width / height - 6. / 11.).abs() < 1e-4);
            assert_eq!(
                (first[0].u, first[0].v, first[2].u, first[2].v),
                (0., 0., 1., 1.)
            );
        }
    }

    #[test]
    fn arriving_key_legends_words_and_caption_move_with_their_own_instance() {
        const EPSILON: f32 = 0.001; // Subpixel f32 projection roundoff.
        let mut legends = Legends::default();
        for (index, glyph) in legends.glyphs.iter_mut().enumerate() {
            glyph.1 = Some(index as u32 + 100);
        }
        let mut camera = RetainedCamera::default();
        camera.position_near = [0., 100., 0., 0.01];
        camera.view_projection[0] = 0.01;
        camera.view_projection[9] = -0.01;
        camera.view_projection[14] = 0.5;
        camera.view_projection[15] = 1.;
        let captions = [BucketCaption {
            instance: 1,
            combo_id: 7,
            center: [0.; 3],
            kind: CaptionKind::Mouse,
        }];
        let feedback = crate::feedback::Feedback::default();
        let fixed =
            legends.overlay_quads(&camera, 800, 800, 2, 20., &feedback, &captions, &[0., 0.]);
        let sliding =
            legends.overlay_quads(&camera, 800, 800, 2, 20., &feedback, &captions, &[0., 10.]);
        assert_eq!(fixed.len(), sliding.len());
        let mut translated = 0;
        for (a, b) in fixed.iter().zip(&sliding) {
            let delta = b.c0.x - a.c0.x;
            assert!(delta.abs() < EPSILON || (delta - 40.).abs() < EPSILON);
            if delta > 1. {
                translated += 1;
            }
            for (a, b) in [(a.c0, b.c0), (a.c1, b.c1), (a.c2, b.c2), (a.c3, b.c3)] {
                assert!(
                    (b.x - a.x - delta).abs() < EPSILON,
                    "corner drift: fixed={} moved={} first-delta={} error={}",
                    a.x,
                    b.x,
                    delta,
                    b.x - a.x - delta
                );
                assert!((b.y - a.y).abs() < EPSILON);
            }
        }
        let per_board = LEGENDS.len()
            + WORD_LEGENDS
                .iter()
                .map(|word| word.text.len())
                .sum::<usize>();
        assert_eq!(translated, per_board + 1 + "MOUSE".len());
        assert_eq!(fixed.len() - translated, per_board);
    }

    #[test]
    fn cold_and_partial_font_readiness_cannot_submit_caption_backings() {
        let camera = OrbitCamera::new(20.).retained(800, 600, [0.; 16]);
        let captions = [BucketCaption {
            instance: 0,
            combo_id: 7,
            center: [0.; 3],
            kind: CaptionKind::Bucket,
        }];
        let feedback = crate::feedback::Feedback::default();
        let mut legends = Legends::default();
        assert!(
            legends
                .overlay_quads(&camera, 800, 600, 1, 20., &feedback, &captions, &[])
                .is_empty()
        );
        let waiting = legends.glyphs.len() - 1;
        for glyph in legends.glyphs.iter_mut().take(waiting) {
            glyph.1 = Some(100);
        }
        assert!(
            legends
                .overlay_quads(&camera, 800, 600, 1, 20., &feedback, &captions, &[])
                .is_empty()
        );
        legends.glyphs.last_mut().unwrap().1 = Some(100);
        let warm = legends.overlay_quads(&camera, 800, 600, 1, 20., &feedback, &captions, &[]);
        assert!(warm.iter().any(|quad| quad.sprite_id == 0)); // Caption backing.
        assert!(warm.iter().filter(|quad| quad.sprite_id == 100).count() >= LEGENDS.len());
        assert_eq!(caption_text(7, CaptionKind::Mouse, &[]), "MOUSE");
        assert_eq!(caption_text(7, CaptionKind::Tablet, &[]), "TABLET");
    }

    #[test]
    fn friendly_bucket_names_use_metadata_and_refresh_without_changing_ids() {
        fn combo(id: u32, kind: InputComboSourceKind, tag: &str) -> TrueosInputCombo {
            let mut combo = TrueosInputCombo {
                combo_id: id,
                source_kind: kind as u8,
                source_tag_len: tag.len() as u8,
                ..Default::default()
            };
            combo.source_tag[..tag.len()].copy_from_slice(tag.as_bytes());
            combo
        }
        let mut combos = [
            combo(1279872089, InputComboSourceKind::Ai, "Spirit/Lilly"),
            combo(1229127681, InputComboSourceKind::Human, "auto"),
            combo(1229127682, InputComboSourceKind::Human, "Alice"),
        ];
        let mut legends = Legends::default();
        assert!(legends.set_buckets(&combos));
        assert!(!legends.set_buckets(&combos));
        assert_eq!(
            caption_text(
                combos[0].combo_id,
                CaptionKind::Bucket,
                &legends.bucket_names
            ),
            "Bucket AI / Spirit / Lilly 1"
        );
        assert_eq!(
            caption_text(
                combos[1].combo_id,
                CaptionKind::Bucket,
                &legends.bucket_names
            ),
            "Bucket Human 1"
        );
        assert_eq!(
            caption_text(
                combos[2].combo_id,
                CaptionKind::Bucket,
                &legends.bucket_names
            ),
            "Bucket Human / Alice 2"
        );
        for (id, name) in &legends.bucket_names {
            assert!(!name.contains(&format!("{id}")));
            assert!(
                name.chars()
                    .all(|scalar| scalar == ' ' || glyph_scalars().any(|glyph| glyph == scalar))
            );
        }
        // Caption-only lowercase and separators get real shared font sprites.
        for (index, glyph) in legends.glyphs.iter_mut().enumerate() {
            glyph.1 = Some(index as u32 + 100);
        }
        let camera = OrbitCamera::new(20.).retained(800, 600, [0.; 16]);
        let captions = [BucketCaption {
            instance: 0,
            combo_id: combos[0].combo_id,
            center: [0.; 3],
            kind: CaptionKind::Bucket,
        }];
        let quads = legends.overlay_quads(
            &camera,
            800,
            600,
            0,
            20.,
            &crate::feedback::Feedback::default(),
            &captions,
            &[],
        );
        assert_eq!(
            quads.len(),
            1 + legends.bucket_names[0]
                .1
                .chars()
                .filter(|&ch| ch != ' ')
                .count()
        );
        let slash = glyph_scalars().position(|ch| ch == '/').unwrap() as u32 + 100;
        assert_eq!(
            quads.iter().filter(|quad| quad.sprite_id == slash).count(),
            2
        );
        combos[2] = combo(combos[2].combo_id, InputComboSourceKind::Human, "Bob");
        assert!(legends.set_buckets(&combos));
        assert_eq!(
            legends.bucket_names[2],
            (1229127682, String::from("Bucket Human / Bob 2"))
        );
    }

    #[test]
    fn bucket_captions_reuse_loaded_glyphs_and_reject_clipped_anchors() {
        assert_eq!(caption_text(0, CaptionKind::Bucket, &[]), "Unassigned");
        assert_eq!(caption_text(u32::MAX, CaptionKind::Bucket, &[]), "Bucket");
        for scalar in caption_text(u32::MAX, CaptionKind::Bucket, &[])
            .chars()
            .chain(caption_text(0, CaptionKind::Bucket, &[]).chars())
        {
            assert!(scalar == ' ' || glyph_scalars().any(|glyph| glyph == scalar));
        }
        let mut camera = OrbitCamera::new(20.).retained(800, 600, [0.; 16]);
        let anchor = project_caption_anchor([0.; 3], &camera, 800, 600).unwrap();
        assert!((anchor[0] - 400.).abs() < 1e-4 && (anchor[1] - 300.).abs() < 1e-4);
        camera.view_projection[15] = -100.;
        assert!(project_caption_anchor([0.; 3], &camera, 800, 600).is_none());
    }

    #[test]
    fn audited_layout_has_exactly_36_capitals_and_digits_and_correct_yz() {
        let mut chars: Vec<_> = LEGENDS.iter().map(|l| l.scalar).collect();
        chars.sort_unstable();
        assert_eq!(
            chars,
            "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
                .chars()
                .collect::<Vec<_>>()
        );
        assert_eq!(LEGENDS.iter().find(|l| l.mesh == 34).unwrap().scalar, 'Z');
        assert_eq!(LEGENDS.iter().find(|l| l.mesh == 52).unwrap().scalar, 'Y');
        let camera = OrbitCamera::new(20.).retained(800, 600, [0.; 16]);
        for legend in LEGENDS {
            let quad = project_quad(legend, &camera, 800, 600, 0.).unwrap();
            assert_eq!((quad[0].u, quad[0].v), (0., 0.));
            assert_eq!((quad[2].u, quad[2].v), (1., 1.));
            assert!(quad.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        }
    }

    #[test]
    fn flat_quad_follows_board_translation_and_hides_from_below() {
        let legend = &LEGENDS[0];
        let camera = OrbitCamera::new(20.).retained(800, 600, [0.; 16]);
        let first = project_quad(legend, &camera, 800, 600, 0.).unwrap();
        let translated = project_quad(legend, &camera, 800, 600, 4.).unwrap();
        assert!((first[0].x - translated[0].x).abs() > 1.);
        let mut below = camera;
        below.position_near[1] = -100.;
        assert!(project_quad(legend, &below, 800, 600, 0.).is_none());
    }

    #[test]
    fn legends_fit_red_highlight_with_centered_capitals_and_left_digits() {
        let mut legend = Legend {
            scalar: 'A',
            mesh: 15,
            center: [0., 1., 0.],
            normal: [0., 1., 0.],
        };
        let mut camera = RetainedCamera::default();
        camera.position_near = [0., 10., 0., 0.01];
        // Orthographic view straight down at the key, 40 pixels/model unit.
        camera.view_projection[0] = 0.1;
        camera.view_projection[9] = -0.1;
        camera.view_projection[14] = 0.5;
        camera.view_projection[15] = 1.;
        let red = project_plane(
            legend.center,
            legend.normal,
            &camera,
            800,
            800,
            0.,
            [KEY_HIGHLIGHT_INNER_SIZE; 2],
        )
        .unwrap();
        for (scalar, center_x) in [('A', 400.), ('Z', 400.), ('0', 384.), ('9', 384.)] {
            legend.scalar = scalar;
            let q = project_quad(&legend, &camera, 800, 800, 0.).unwrap();
            let w = q[1].x - q[0].x;
            let h = q[3].y - q[0].y;
            assert!((h - 28.).abs() < 1e-4);
            assert!((w / h - 6. / 11.).abs() < 1e-5);
            assert!(((q[0].x + q[2].x) * 0.5 - center_x).abs() < 1e-4);
            assert!((q[0].y - red[0].y - 2.).abs() < 1e-4);
            assert!(
                q.iter().all(|p| p.x >= red[0].x
                    && p.x <= red[2].x
                    && p.y >= red[0].y
                    && p.y <= red[2].y)
            );
            assert_eq!((q[0].u, q[0].v, q[2].u, q[2].v), (0., 0., 1., 1.));
        }
    }
}

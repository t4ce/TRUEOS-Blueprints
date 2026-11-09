extern crate alloc;
use crate::scene::BucketCaption;
use alloc::{format, string::String, vec::Vec};
use trueos::{
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
include!(concat!(env!("OUT_DIR"), "/keyboard.legends.rs"));
include!(concat!(env!("OUT_DIR"), "/keyboard.keys.rs"));

pub const TEXTURE_HEIGHT: f32 = 64.0;
pub const EXTRA_STROKE_PX: f32 = 1.5;
pub const GLYPH_HEIGHT: f32 = 0.7;
const KEY_HIGHLIGHT_INNER_SIZE: f32 = 1.34;
const GLYPH_TOP_INSET: f32 = 0.05;

/// Masks belong to the shared kernel font service. One window-owned glyph
/// texture is reused for the matching key on every board.
pub struct Legends {
    glyphs: Vec<(Option<FontSpriteTicket>, Option<u32>)>,
}
impl Default for Legends {
    fn default() -> Self {
        Self {
            glyphs: alloc::vec![(None, None); LEGENDS.len()],
        }
    }
}
impl Legends {
    /// Poll outside the writable UI4 lease. Never hold up input or the first mesh frame.
    pub fn poll(&mut self, frame: &mut Frame) -> Result<bool, Error> {
        let mut changed = false;
        for (legend, (ticket, sprite)) in LEGENDS.iter().zip(&mut self.glyphs) {
            if sprite.is_some() {
                continue;
            }
            if ticket.is_none() {
                match frame.request_font_sprite_with_stroke(
                    FontSpriteRequest {
                        font: Font::Default,
                        scalar: legend.scalar,
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
    ) -> Result<(), Error> {
        let mut quads = Vec::with_capacity(boards * LEGENDS.len());
        // Constant-color quads use UI4's existing white pixel. Glyph textures
        // remain shared, black, and untinted; blank keys receive feedback too.
        for board in 0..boards {
            for (index, key) in KEYS.iter().enumerate() {
                if !feedback.active(board, index) {
                    continue;
                }
                for (size, color) in [
                    (1.50, ui4_scene::rgba(255, 255, 255, 235)),
                    (KEY_HIGHLIGHT_INNER_SIZE, ui4_scene::rgba(255, 32, 32, 235)),
                ] {
                    let Some(corners) = project_plane(
                        key.center,
                        key.normal,
                        camera,
                        width,
                        height,
                        board as f32 * radius * 2.2,
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
                if let Some(corners) =
                    project_quad(legend, camera, width, height, board as f32 * radius * 2.2)
                {
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
            }
        }
        for caption in captions {
            let Some([x, y]) = project_caption_anchor(caption.center, camera, width, height) else {
                continue;
            };
            let text = caption_text(caption.combo_id);
            const HEIGHT: f32 = 16.;
            const CELL: f32 = HEIGHT * 6. / 11.;
            let left = x - text.len() as f32 * CELL * 0.5;
            let top = y - HEIGHT * 0.5;
            quads.push(screen_quad(
                0,
                left - 4.,
                top - 3.,
                text.len() as f32 * CELL + 8.,
                HEIGHT + 6.,
                ui4_scene::rgba(255, 255, 255, 220),
            ));
            for (index, scalar) in text.chars().enumerate() {
                let Some(sprite) = LEGENDS
                    .iter()
                    .position(|legend| legend.scalar == scalar)
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
        if !quads.is_empty() {
            frame.draw_sprite_quads(&quads)?;
        }
        Ok(())
    }
}

fn caption_text(combo_id: u32) -> String {
    if combo_id == 0 {
        String::from("UNASSIGNED")
    } else {
        format!("BUCKET {combo_id}")
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
    fn bucket_captions_reuse_loaded_glyphs_and_reject_clipped_anchors() {
        assert_eq!(caption_text(0), "UNASSIGNED");
        assert_eq!(caption_text(u32::MAX), "BUCKET 4294967295");
        for scalar in caption_text(u32::MAX)
            .chars()
            .chain(caption_text(0).chars())
        {
            assert!(scalar == ' ' || LEGENDS.iter().any(|legend| legend.scalar == scalar));
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

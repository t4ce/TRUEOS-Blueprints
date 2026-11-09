extern crate alloc;
use alloc::vec::Vec;
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
pub const EXTRA_STROKE_PX: f32 = 2.5;
pub const GLYPH_HEIGHT: f32 = 1.25 * 0.8;

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
                    (1.34, ui4_scene::rgba(255, 32, 32, 235)),
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
        if !quads.is_empty() {
            frame.draw_sprite_quads(&quads)?;
        }
        Ok(())
    }
}

/// A flat 6:11 cell, centered 0.025 model units above its key's top plane.
/// UI4 consumes the four projected corners and samples the service's alpha.
/// This first pass is an overlay: top-facing rejection, not a depth attachment.
pub fn project_quad(
    legend: &Legend,
    camera: &RetainedCamera,
    width: u32,
    height: u32,
    board_z: f32,
) -> Option<[SpriteCorner; 4]> {
    project_plane(
        legend.center,
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
    fn smaller_placement_preserves_cell_aspect_full_uvs_and_key_center() {
        let legend = Legend {
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
        let q = project_quad(&legend, &camera, 800, 800, 0.).unwrap();
        let w = q[1].x - q[0].x;
        let h = q[3].y - q[0].y;
        assert!((h / 50. - 0.8).abs() < 1e-5);
        assert!((w / h - 6. / 11.).abs() < 1e-5);
        assert!(((q[0].x + q[2].x) * 0.5 - 400.).abs() < 1e-5);
        assert!(((q[0].y + q[2].y) * 0.5 - 400.).abs() < 1e-5);
        assert_eq!((q[0].u, q[0].v, q[2].u, q[2].v), (0., 0., 1., 1.));
    }
}

use trueos::{
    ui4_scene::{BackgroundLayer, Damage, Error, Frame},
    ui4_winit::{SceneTarget, SpriteBackend, SpriteCommand, SpriteCorner, SpriteQuad},
    vgpu::Device,
};

const SPRITE_ID: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Picture {
    extent: (u32, u32),
    tint: Option<[u8; 3]>,
}

pub(super) struct Backdrop {
    layer: BackgroundLayer,
    target: SceneTarget,
    published: Option<Picture>,
    pending_publish: Option<Picture>,
    uploaded: Option<Picture>,
    pending_upload: Option<(Picture, Vec<u8>)>,
}

impl Backdrop {
    pub(super) fn new(frame: &Frame, _device: Device) -> Result<Self, Error> {
        let layer = frame.background()?;
        let target = SceneTarget::for_window(layer.render_target(), frame.width(), frame.height())?;
        Ok(Self {
            layer,
            target,
            published: None,
            pending_publish: None,
            uploaded: None,
            pending_upload: None,
        })
    }

    /// A resize stages a new backing even when the extent stays the same.
    pub(super) fn invalidate(&mut self) {
        self.published = None;
    }

    pub(super) fn update(
        &mut self,
        extent: (u32, u32),
        tint: Option<[u8; 3]>,
    ) -> Result<bool, Error> {
        match self.prepare_and_publish(Picture { extent, tint }) {
            Ok(()) => Ok(true),
            Err(Error::Busy) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn prepare_and_publish(&mut self, picture: Picture) -> Result<(), Error> {
        self.publish_pending()?;
        if self.published == Some(picture) {
            return Ok(());
        }
        let (width, height) = picture.extent;
        self.target.set_extent(width, height)?;
        if let Some(rgb) = picture.tint {
            if self.uploaded != Some(picture) {
                if self.pending_upload.as_ref().map(|(picture, _)| *picture) != Some(picture) {
                    self.pending_upload = Some((picture, gradient(width, height, rgb)?));
                }
                let (_, pixels) = self.pending_upload.as_ref().unwrap();
                self.target
                    .upload_sprite_rgba8(SPRITE_ID, width, height, pixels)?;
                self.uploaded = Some(picture);
                self.pending_upload = None;
            }
            self.layer.begin_gpu_frame()?;
            // A busy BCS0 submission cancels the unwritten lease. Retry the copy
            // from the retained source once the background can acquire again.
            self.target.draw_sprite_commands(&[copy(width, height)])?;
        } else {
            self.target.begin(0)?;
        }
        self.pending_publish = Some(picture);
        self.publish_pending()
    }

    fn publish_pending(&mut self) -> Result<(), Error> {
        if let Some(picture) = self.pending_publish {
            let (width, height) = picture.extent;
            self.layer.publish(Damage::full(width, height))?;
            self.published = Some(picture);
            self.pending_publish = None;
        }
        Ok(())
    }
}

fn copy(width: u32, height: u32) -> SpriteCommand {
    let corner = |x: u32, y: u32, u: f32, v: f32| SpriteCorner {
        x: x as f32,
        y: y as f32,
        u,
        v,
    };
    SpriteCommand {
        quad: SpriteQuad {
            sprite_id: SPRITE_ID,
            c0: corner(0, 0, 0.0, 0.0),
            c1: corner(width, 0, 1.0, 0.0),
            c2: corner(width, height, 1.0, 1.0),
            c3: corner(0, height, 0.0, 1.0),
            color_rgba: u32::MAX,
            source_over: false,
        },
        backend: SpriteBackend::Bcs0Overwrite,
    }
}

fn gradient(width: u32, height: u32, rgb: [u8; 3]) -> Result<Vec<u8>, Error> {
    let pitch = (width as usize).checked_mul(4).ok_or(Error::Invalid)?;
    let size = pitch.checked_mul(height as usize).ok_or(Error::Invalid)?;
    let mut pixels = vec![0; size];
    let coordinate = |value: u32, extent: u32| {
        if extent > 1 {
            value as f32 / (extent - 1) as f32
        } else {
            0.5
        }
    };
    // Use Bfly's bottom-left gradient and static alpha dither.
    for (y, row) in (0..height).zip(pixels.chunks_exact_mut(pitch)) {
        let v = coordinate(y, height);
        for (x, pixel) in (0..width).zip(row.chunks_exact_mut(4)) {
            let t = (v - coordinate(x, width)).clamp(0.0, 1.0);
            let ideal = 255.0 * 0.33 * t * t * (3.0 - 2.0 * t);
            let low = 2 * ((x ^ y) & 1) + (y & 1);
            let high = ((x ^ y) & 2) + ((y >> 1) & 1);
            let dither = ((4 * low + high) as f32 + 0.5) / 16.0;
            let alpha = (ideal + dither) as u8;
            let [r, g, b] =
                rgb.map(|channel| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8);
            pixel.copy_from_slice(&[r, g, b, alpha]);
        }
    }
    Ok(pixels)
}

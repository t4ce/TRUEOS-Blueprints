//! App-owned collapse/restore, using Solara's UI4 dynamic menu and activation path.
use crate::{logo, presenter};
use trueos::ui4_scene::{CursorSource, Error, Frame, MenuEntry, POINTER_BUTTON_PRIMARY};

#[derive(Clone, Copy)]
struct Expanded {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}
struct MenuInvocation {
    serial: u64,
    collapsed: bool,
    point: Option<(f64, f64)>,
    label: String,
}
struct Swoop {
    started: u64,
    from: (i32, i32),
    to: (i32, i32),
}
pub struct Compact {
    saved: Option<Expanded>,
    swoop: Option<Swoop>,
    press: Option<(CursorSource, u32, u32)>,
    collapse_requested: bool,
    invocation: Option<MenuInvocation>,
    logo: image::RgbaImage,
    dirty: bool,
}
#[derive(Default)]
struct MenuActions {
    collapse: bool,
    frog: bool,
}
fn menu<'a>(
    collapsed: bool,
    point: Option<(f64, f64)>,
    label: &'a str,
) -> [MenuEntry<'a, MenuActions>; 2] {
    [
        if collapsed {
            MenuEntry::disabled("collapse")
        } else {
            MenuEntry::new("collapse", |actions| actions.collapse = true)
        },
        if point.is_some() {
            MenuEntry::new(label, |actions| actions.frog = true)
        } else {
            MenuEntry::disabled("Frog")
        },
    ]
}
impl Compact {
    pub fn new(frame: &mut Frame) -> Result<Self, Error> {
        // Register before the first publication: unregistered windows get the
        // desktop/monitor menu instead of this app-owned menu.
        frame.register_dynamic_context_menu()?;
        Ok(Self {
            saved: None,
            swoop: None,
            press: None,
            collapse_requested: false,
            invocation: None,
            logo: logo::decode().map_err(|_| Error::Invalid)?,
            dirty: false,
        })
    }
    pub fn collapsed(&self) -> bool {
        self.saved.is_some()
    }
    async fn collapse(&mut self, frame: &mut Frame) -> Result<(), Error> {
        let (x, y) = frame.position()?;
        let saved = Expanded {
            x,
            y,
            width: frame.width(),
            height: frame.height(),
        };
        presenter::retry(|| frame.resize(logo::SIZE, logo::SIZE)).await?;
        self.saved = Some(saved);
        presenter::retry(|| frame.set_primary_activation(true)).await?;
        self.swoop = Some(Swoop {
            started: trueos::clock::monotonic_millis(),
            from: (x, y),
            to: (
                x,
                y.saturating_add(saved.height.saturating_sub(logo::SIZE) as i32),
            ),
        });
        self.press = None;
        self.dirty = true;
        trueos::logl::log(
            trueos::logl::level::IMPORTANT,
            format_args!(
                "osm: collapsed window={} saved={}x{}@{},{} tile=128x128",
                frame.window_id(),
                saved.width,
                saved.height,
                x,
                y
            ),
        );
        Ok(())
    }
    async fn restore(&mut self, frame: &mut Frame) -> Result<(), Error> {
        let Some(saved) = self.saved else {
            return Ok(());
        };
        presenter::retry(|| frame.resize(saved.width, saved.height)).await?;
        presenter::retry(|| frame.set_position(saved.x, saved.y)).await?;
        presenter::retry(|| frame.set_primary_activation(false)).await?;
        self.saved = None;
        self.swoop = None;
        self.press = None;
        self.dirty = false;
        trueos::logl::log(
            trueos::logl::level::IMPORTANT,
            format_args!(
                "osm: restored window={} extent={}x{}@{},{}",
                frame.window_id(),
                saved.width,
                saved.height,
                saved.x,
                saved.y
            ),
        );
        Ok(())
    }
    /// Returns true while compact. Expanded input remains for the map loop.
    pub async fn tick(
        &mut self,
        frame: &mut Frame,
        view: &crate::viewport::Viewport,
    ) -> Result<bool, Error> {
        while let Some(event) = frame.take_dynamic_context_menu_event()? {
            if event.closed.is_none() {
                let collapsed = self.collapsed();
                let point = if collapsed {
                    None
                } else {
                    view.coordinates_at(event.local_x, event.local_y)
                };
                let label = point
                    .map(|(lon, lat)| format!("Frog {lon:.4},{lat:.4}"))
                    .unwrap_or_else(|| "Frog".into());
                self.invocation = None;
                if frame.resolve_context_menu(event.serial, &menu(collapsed, point, &label))? {
                    self.invocation = Some(MenuInvocation {
                        serial: event.serial,
                        collapsed,
                        point,
                        label,
                    });
                }
            } else if self
                .invocation
                .as_ref()
                .is_some_and(|inv| inv.serial == event.serial)
            {
                let MenuInvocation {
                    collapsed,
                    point,
                    label,
                    ..
                } = self.invocation.take().unwrap();
                let mut actions = MenuActions::default();
                event.dispatch(&menu(collapsed, point, &label), &mut actions);
                self.collapse_requested |= actions.collapse;
                if actions.frog
                    && let Some((lon, lat)) = point
                {
                    if let Err(error) = trueos::vshell::launch_with_destination(
                        "appstore",
                        &format!("launch --sh3 Frog -- weather {lon} {lat}\n"),
                        trueos::vshell::LaunchDestination::Headless,
                    ) {
                        eprintln!("OSM: Frog launch failed: {error}");
                    }
                }
            }
        }
        if self.collapse_requested && !self.collapsed() {
            self.collapse(frame).await?;
        }
        self.collapse_requested = false;
        if !self.collapsed() {
            return Ok(false);
        }
        if let Some(swoop) = &self.swoop {
            let t = (trueos::clock::monotonic_millis().saturating_sub(swoop.started) as f32
                / 180.0)
                .min(1.0);
            let eased = 1.0 - (1.0 - t).powi(3);
            let x = (swoop.from.0 as f32 + (swoop.to.0 as f32 - swoop.from.0 as f32) * eased)
                .round() as i32;
            let y = (swoop.from.1 as f32 + (swoop.to.1 as f32 - swoop.from.1 as f32) * eased)
                .round() as i32;
            presenter::retry(|| frame.set_position(x, y)).await?;
            if t >= 1.0 {
                self.swoop = None;
            }
        }
        let mut restore = false;
        while let Some(event) = frame.take_pointer_event()? {
            if event.buttons_pressed & POINTER_BUTTON_PRIMARY != 0 {
                self.press = Some((event.source, event.x, event.y));
            }
            if let Some((source, x, y)) = self.press {
                if source != event.source || event.x.abs_diff(x) > 4 || event.y.abs_diff(y) > 4 {
                    self.press = None;
                }
            }
            if event.buttons_released & POINTER_BUTTON_PRIMARY != 0 {
                restore |= self.press.take().is_some()
                    && (0..logo::SIZE as i32).contains(&event.local_x)
                    && (0..logo::SIZE as i32).contains(&event.local_y);
            }
        }
        while frame.take_resize_event()?.is_some() {}
        while frame.take_pan_event()?.is_some() {}
        if restore {
            self.restore(frame).await?;
            return Ok(false);
        }
        if self.dirty {
            presenter::present_logo(frame, &self.logo).await?;
            self.dirty = false;
        }
        Ok(true)
    }
}

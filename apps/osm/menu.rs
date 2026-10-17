//! Map-specific menu action; standard collapse/restore belongs to UI4.
use trueos::ui4_scene::{Error, Frame, MenuEntry};
#[derive(Default)]
pub struct MapMenu {
    invocation: Option<(u64, Option<(f64, f64)>, String)>,
}
fn entries(label: &str, enabled: bool) -> [MenuEntry<'_, bool>; 1] {
    [if enabled {
        MenuEntry::new(label, |open| *open = true)
    } else {
        MenuEntry::disabled(label)
    }]
}
impl MapMenu {
    pub fn new(frame: &mut Frame) -> Result<Self, Error> {
        frame.register_dynamic_context_menu()?;
        Ok(Self::default())
    }
    pub fn tick(
        &mut self,
        frame: &mut Frame,
        view: &crate::viewport::Viewport,
    ) -> Result<(), Error> {
        while let Some(event) = frame.take_dynamic_context_menu_event()? {
            if event.closed.is_none() {
                let point = view.coordinates_at(event.local_x, event.local_y);
                let label = point
                    .map(|(lon, lat)| format!("Frog {lon:.4},{lat:.4}"))
                    .unwrap_or_else(|| "Frog".into());
                self.invocation = None;
                if frame.resolve_context_menu(event.serial, &entries(&label, point.is_some()))? {
                    self.invocation = Some((event.serial, point, label));
                }
            } else if self
                .invocation
                .as_ref()
                .is_some_and(|inv| inv.0 == event.serial)
            {
                let (_, point, label) = self.invocation.take().unwrap();
                let mut open = false;
                event.dispatch(&entries(&label, point.is_some()), &mut open);
                if open && let Some((lon, lat)) = point {
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
        Ok(())
    }
}

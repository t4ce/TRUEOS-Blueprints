//! Presentation state only: privileged claims and validation stay in the host.
#[derive(Clone, Debug)]
pub struct Disk {
    pub id: u32,
    pub label: String,
    pub bytes: u64,
    pub block_size: u32,
    pub free: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct Image {
    pub id: usize,
    pub root: u32,
    pub label: String,
    pub bytes: u64,
    pub block_size: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Home,
    Source,
    Method(usize),
    Destination(usize),
    Images,
    Target(usize),
    Confirm { image: usize, disk: usize },
}
pub struct App {
    pub disks: Vec<Disk>,
    pub images: Vec<Image>,
    pub screen: Screen,
    pub selected: usize,
}
impl App {
    pub fn new(args: impl Iterator<Item = String>) -> Self {
        let mut disks = Vec::new();
        let mut images = Vec::new();
        for arg in args {
            if let Some(raw) = arg.strip_prefix("disk=") {
                let f: Vec<_> = raw.splitn(5, '|').collect();
                if f.len() == 5 {
                    if let (Ok(id), Ok(bytes), Ok(block_size)) =
                        (f[0].parse(), f[1].parse(), f[2].parse())
                    {
                        disks.push(Disk {
                            id,
                            bytes,
                            block_size,
                            free: f[3].parse().ok(),
                            label: f[4].into(),
                        });
                    }
                }
            } else if let Some(raw) = arg.strip_prefix("image=") {
                let f: Vec<_> = raw.splitn(6, '|').collect();
                if f.len() == 6 {
                    if let (Ok(id), Ok(root), Ok(bytes), Ok(block_size)) =
                        (f[0].parse(), f[1].parse(), f[2].parse(), f[3].parse())
                    {
                        images.push(Image {
                            id,
                            root,
                            bytes,
                            block_size,
                            label: format!("{} · {}", f[4], f[5]),
                        });
                    }
                }
            }
        }
        Self {
            disks,
            images,
            screen: Screen::Home,
            selected: 0,
        }
    }
    pub fn destinations(&self, source: usize) -> Vec<usize> {
        self.disks
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                (i != source && d.free.is_some_and(|free| free >= self.disks[source].bytes))
                    .then_some(i)
            })
            .collect()
    }
    pub fn targets(&self, image: usize) -> Vec<usize> {
        let image = &self.images[image];
        self.disks
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                (d.id != image.root && d.bytes == image.bytes && d.block_size == image.block_size)
                    .then_some(i)
            })
            .collect()
    }
    pub fn count(&self) -> usize {
        match self.screen {
            Screen::Home => 3,
            Screen::Source => self.disks.len(),
            Screen::Method(source) => {
                if self.destinations(source).is_empty() {
                    1
                } else {
                    2
                }
            }
            Screen::Destination(source) => self.destinations(source).len(),
            Screen::Images => self.images.len(),
            Screen::Target(image) => self.targets(image).len(),
            Screen::Confirm { .. } => 2,
        }
    }
    pub fn move_selection(&mut self, delta: isize) {
        let count = self.count().max(1);
        self.selected = (self.selected as isize + delta).rem_euclid(count as isize) as usize;
    }
    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.selected = 0;
    }
    pub fn back(&mut self) -> bool {
        let screen = match self.screen {
            Screen::Home => return true,
            Screen::Source | Screen::Images => Screen::Home,
            Screen::Method(_) => Screen::Source,
            Screen::Destination(source) => Screen::Method(source),
            Screen::Target(_) => Screen::Images,
            Screen::Confirm { image, .. } => Screen::Target(image),
        };
        self.go(screen);
        false
    }
    pub fn activate(&mut self) -> Option<String> {
        if self.selected >= self.count() {
            return None;
        }
        match self.screen {
            Screen::Home => match self.selected {
                0 => self.go(Screen::Source),
                1 => self.go(Screen::Images),
                _ => return Some("backup:quit".into()),
            },
            Screen::Source => self.go(Screen::Method(self.selected)),
            Screen::Method(source) => {
                if self.selected == 0 {
                    return Some(format!("backup:network:{}", self.disks[source].id));
                }
                self.go(Screen::Destination(source));
            }
            Screen::Destination(source) => {
                let destination = self.destinations(source)[self.selected];
                return Some(format!(
                    "backup:local:{}:{}",
                    self.disks[source].id, self.disks[destination].id
                ));
            }
            Screen::Images => self.go(Screen::Target(self.selected)),
            Screen::Target(image) => self.go(Screen::Confirm {
                image,
                disk: self.targets(image)[self.selected],
            }),
            Screen::Confirm { image, disk } => {
                if self.selected == 0 {
                    self.go(Screen::Target(image));
                } else {
                    return Some(format!(
                        "backup:restore:{}:{}",
                        self.disks[disk].id, self.images[image].id
                    ));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        App::new(
            [
                "disk=1|4096|512|-|Source",
                "disk=2|8192|512|5000|Other",
                "disk=3|4096|512|100|Target",
                "image=0|2|4096|512|disc001|backups/test.manifest",
            ]
            .into_iter()
            .map(String::from),
        )
    }
    #[test]
    fn source_cannot_back_up_to_itself_or_full_disk() {
        assert_eq!(app().destinations(0), vec![1]);
    }
    #[test]
    fn restore_requires_matching_geometry_and_other_root() {
        assert_eq!(app().targets(0), vec![0, 2]);
    }
    #[test]
    fn restore_confirmation_defaults_to_cancel() {
        let mut app = app();
        app.go(Screen::Confirm { image: 0, disk: 2 });
        assert_eq!(app.activate(), None);
        assert_eq!(app.screen, Screen::Target(0));
        app.go(Screen::Confirm { image: 0, disk: 2 });
        app.selected = 1;
        assert_eq!(app.activate(), Some("backup:restore:3:0".into()));
    }
    #[test]
    fn local_and_network_actions_are_separate() {
        let mut app = app();
        app.go(Screen::Method(0));
        assert_eq!(app.activate(), Some("backup:network:1".into()));
        app.go(Screen::Destination(0));
        assert_eq!(app.activate(), Some("backup:local:1:2".into()));
    }
    #[test]
    fn empty_lists_never_emit_action() {
        let mut app = App::new(std::iter::empty());
        app.go(Screen::Source);
        assert_eq!(app.activate(), None);
        app.go(Screen::Images);
        assert_eq!(app.activate(), None);
    }
}

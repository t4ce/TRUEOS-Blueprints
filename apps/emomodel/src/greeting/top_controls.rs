use trueos::ui4_scene::{Error, Frame, MenuEntry};

use crate::top::{Pastel, TopStyle};

#[derive(Default)]
pub(super) struct TopControls {
    pub(super) style: TopStyle,
    invocation: Option<(u64, TopStyle)>,
}

impl TopControls {
    pub(super) fn new(frame: &mut Frame) -> Result<Self, Error> {
        frame.register_dynamic_context_menu()?;
        Ok(Self::default())
    }

    pub(super) fn tick(&mut self, frame: &mut Frame) -> Result<(), Error> {
        while let Some(event) = frame.take_dynamic_context_menu_event()? {
            if event.closed.is_none() {
                let style = self.style;
                let resolved = with_entries(style, |entries| {
                    frame.resolve_context_menu(event.serial, entries)
                })?;
                self.invocation = resolved.then_some((event.serial, style));
            } else if let Some((serial, style)) = self.invocation
                && serial == event.serial
            {
                self.invocation = None;
                with_entries(style, |entries| event.dispatch(entries, &mut self.style));
            }
        }
        Ok(())
    }

    pub(super) fn toggle_random(&mut self) {
        if self.style.color.is_some() {
            clear_outfit(&mut self.style);
            return;
        }
        let roll = trueos::rng::u64();
        let colors = Pastel::ALL.len() as u64;
        let lengths = roll / colors;
        let sleeves = (lengths % 11) as f32 / 10.0;
        self.style = TopStyle {
            color: Some(Pastel::ALL[(roll % colors) as usize]),
            left_sleeve: sleeves,
            right_sleeve: sleeves,
            hem: (lengths / 11 % 11) as f32 / 10.0,
            socks: Some((lengths / 121 % 11) as f32 / 10.0),
        };
    }

    pub(super) fn key(&mut self, key: char) {
        if matches!(key, 't' | 'T') {
            let index = self
                .style
                .color
                .and_then(|color| Pastel::ALL.iter().position(|&entry| entry == color))
                .map_or(0, |index| index + 1);
            if let Some(&color) = Pastel::ALL.get(index) {
                self.style.color = Some(color);
            } else {
                clear_outfit(&mut self.style);
            }
            return;
        }
        if self.style.color.is_some() {
            match key {
                '[' => shorten_left(&mut self.style),
                ']' => lengthen_left(&mut self.style),
                '{' => shorten_right(&mut self.style),
                '}' => lengthen_right(&mut self.style),
                '-' => raise_hem(&mut self.style),
                '=' => lower_hem(&mut self.style),
                _ => (),
            }
        }
    }

    pub(super) fn title(&self, base: &str, samples: u8) -> String {
        let sampling = match samples {
            2 => " · 2× MSAA",
            4 => " · 4× MSAA",
            _ => "",
        };
        match self.style.color {
            Some(color) => {
                let socks = self.style.socks.map_or_else(String::new, |length| {
                    format!(" · Socks {}%", percentage(length))
                });
                format!(
                    "{base}{sampling} · {} top · Left sleeve {}% · Right sleeve {}% · Hem {}%{socks}",
                    color.name(),
                    percentage(self.style.left_sleeve),
                    percentage(self.style.right_sleeve),
                    percentage(self.style.hem),
                )
            }
            None => format!("{base}{sampling} · Space for random outfit · Right-click for tops"),
        }
    }
}

fn with_entries<T>(
    style: TopStyle,
    use_entries: impl FnOnce(&[MenuEntry<'_, TopStyle>]) -> T,
) -> T {
    let color = style.color.map_or("Original", Pastel::name);
    let heading = format!(
        "Top · {color} · L {}% · R {}% · Hem {}%",
        percentage(style.left_sleeve),
        percentage(style.right_sleeve),
        percentage(style.hem),
    );
    let has_top = style.color.is_some();
    let entries = [
        MenuEntry::disabled(&heading),
        MenuEntry::new("Original", clear_outfit),
        MenuEntry::new(Pastel::ALL[0].name(), select_color::<0>),
        MenuEntry::new(Pastel::ALL[1].name(), select_color::<1>),
        MenuEntry::new(Pastel::ALL[2].name(), select_color::<2>),
        MenuEntry::new(Pastel::ALL[3].name(), select_color::<3>),
        MenuEntry::new(Pastel::ALL[4].name(), select_color::<4>),
        MenuEntry::new(Pastel::ALL[5].name(), select_color::<5>),
        MenuEntry::new(Pastel::ALL[6].name(), select_color::<6>),
        MenuEntry::new(Pastel::ALL[7].name(), select_color::<7>),
        adjustment(
            "Shorten left sleeve  [",
            has_top && style.left_sleeve > 0.0,
            shorten_left,
        ),
        adjustment(
            "Lengthen left sleeve  ]",
            has_top && style.left_sleeve < 1.0,
            lengthen_left,
        ),
        adjustment(
            "Shorten right sleeve  {",
            has_top && style.right_sleeve > 0.0,
            shorten_right,
        ),
        adjustment(
            "Lengthen right sleeve  }",
            has_top && style.right_sleeve < 1.0,
            lengthen_right,
        ),
        adjustment("Raise hem  -", has_top && style.hem > 0.0, raise_hem),
        adjustment("Lower hem  =", has_top && style.hem < 1.0, lower_hem),
    ];
    use_entries(&entries)
}

fn adjustment(label: &str, enabled: bool, action: fn(&mut TopStyle)) -> MenuEntry<'_, TopStyle> {
    if enabled {
        MenuEntry::new(label, action)
    } else {
        MenuEntry::disabled(label)
    }
}

fn select_color<const INDEX: usize>(style: &mut TopStyle) {
    style.color = Some(Pastel::ALL[INDEX]);
}

fn clear_outfit(style: &mut TopStyle) {
    style.color = None;
    style.socks = None;
}

fn percentage(length: f32) -> i32 {
    (length * 100.0).round() as i32
}

fn adjust(length: &mut f32, step: i32) {
    *length = (percentage(*length) + step).clamp(0, 100) as f32 / 100.0;
}

fn shorten_left(style: &mut TopStyle) {
    adjust(&mut style.left_sleeve, -10);
}

fn lengthen_left(style: &mut TopStyle) {
    adjust(&mut style.left_sleeve, 10);
}

fn shorten_right(style: &mut TopStyle) {
    adjust(&mut style.right_sleeve, -10);
}

fn lengthen_right(style: &mut TopStyle) {
    adjust(&mut style.right_sleeve, 10);
}

fn raise_hem(style: &mut TopStyle) {
    adjust(&mut style.hem, -10);
}

fn lower_hem(style: &mut TopStyle) {
    adjust(&mut style.hem, 10);
}

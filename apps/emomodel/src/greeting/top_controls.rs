use trueos::ui4_scene::{Error, Frame, MenuEntry};

use crate::top::{BottomStyle, Pastel, TopStyle, space_sock_length};

#[derive(Default)]
pub(super) struct TopControls {
    pub(super) style: TopStyle,
    invocation: Option<(u64, TopStyle)>,
    bottoms_sock_roll: Option<f32>,
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
                if style.bottoms.is_some() == self.style.bottoms.is_some() {
                    with_entries(style, |entries| event.dispatch(entries, &mut self.style));
                    if self.style.bottoms.is_none() {
                        self.bottoms_sock_roll = None;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn step_bottoms(&mut self) {
        if let Some(bottoms) = &mut self.style.bottoms {
            if [bottoms.left_leg, bottoms.right_leg, bottoms.waist]
                .into_iter()
                .all(|length| length >= 1.0)
            {
                *bottoms = BottomStyle::default();
            } else {
                adjust(&mut bottoms.left_leg, 10);
                adjust(&mut bottoms.right_leg, 10);
                adjust(&mut bottoms.waist, 10);
            }
        } else {
            select_bottoms(&mut self.style);
        }

        if let (Some(bottoms), Some(socks)) = (self.style.bottoms, self.style.socks) {
            let roll = *self.bottoms_sock_roll.get_or_insert(socks);
            self.style.socks = Some(space_sock_length(roll, bottoms));
        }
    }

    pub(super) fn key(&mut self, key: char) {
        if matches!(key, 't' | 'T') {
            let index = self
                .style
                .color
                .and_then(|color| Pastel::ALL.iter().position(|&entry| entry == color))
                .map_or(0, |index| index + 1);
            if let Some(&color) = Pastel::ALL.get(index) {
                choose_pastel(&mut self.style, color);
            } else {
                clear_outfit(&mut self.style);
            }
            self.bottoms_sock_roll = None;
            return;
        }
        if self.style.bottoms.is_some() || self.style.color.is_some() {
            match key {
                '[' => adjust_left::<-10>(&mut self.style),
                ']' => adjust_left::<10>(&mut self.style),
                '{' => adjust_right::<-10>(&mut self.style),
                '}' => adjust_right::<10>(&mut self.style),
                '-' => adjust_height::<-10>(&mut self.style),
                '=' => adjust_height::<10>(&mut self.style),
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
        if let Some(bottoms) = self.style.bottoms {
            let socks = self
                .style
                .socks
                .zip(self.style.sock_color)
                .map_or_else(String::new, |(length, color)| {
                    format!(" · {} socks {}%", color.name(), percentage(length))
                });
            return format!(
                "{base}{sampling} · Black bottoms · Left leg {}% · Right leg {}% · Waist {}%{socks}",
                percentage(bottoms.left_leg),
                percentage(bottoms.right_leg),
                percentage(bottoms.waist),
            );
        }
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
            None => {
                format!(
                    "{base}{sampling} · Space for black bottoms and socks · Right-click for clothing"
                )
            }
        }
    }
}

fn with_entries<T>(
    style: TopStyle,
    use_entries: impl FnOnce(&[MenuEntry<'_, TopStyle>]) -> T,
) -> T {
    let heading = match (style.bottoms, style.color) {
        (Some(bottoms), _) => format!(
            "Black  L{} R{} W{}",
            percentage(bottoms.left_leg),
            percentage(bottoms.right_leg),
            percentage(bottoms.waist),
        ),
        (None, Some(color)) => format!(
            "{}  L{} R{} H{}",
            color.name(),
            percentage(style.left_sleeve),
            percentage(style.right_sleeve),
            percentage(style.hem),
        ),
        (None, None) => "Clothing".to_owned(),
    };
    let mut entries = Vec::from([
        MenuEntry::disabled(&heading),
        MenuEntry::new("Original", clear_outfit),
    ]);
    if style.bottoms.is_none() {
        entries.push(MenuEntry::new("Random pastel outfit", roll_outfit));
    }
    if style.bottoms.is_none() && style.color.is_none() {
        entries.push(MenuEntry::new("Black bottoms and socks", select_bottoms));
    }
    let choices: [fn(&mut TopStyle); Pastel::ALL.len()] = [
        select_color::<0>,
        select_color::<1>,
        select_color::<2>,
        select_color::<3>,
        select_color::<4>,
        select_color::<5>,
        select_color::<6>,
        select_color::<7>,
    ];
    for (color, select) in Pastel::ALL.into_iter().zip(choices) {
        if style.color != Some(color) {
            entries.push(MenuEntry::new(color.name(), select));
        }
    }
    if let Some(bottoms) = style.bottoms {
        entries.extend([
            adjustment(
                "Shorten left leg  [",
                bottoms.left_leg > 0.0,
                adjust_left::<-10>,
            ),
            adjustment(
                "Lengthen left leg  ]",
                bottoms.left_leg < 1.0,
                adjust_left::<10>,
            ),
            adjustment(
                "Shorten right leg  {",
                bottoms.right_leg > 0.0,
                adjust_right::<-10>,
            ),
            adjustment(
                "Lengthen right leg  }",
                bottoms.right_leg < 1.0,
                adjust_right::<10>,
            ),
            adjustment("Lower waist  -", bottoms.waist > 0.0, adjust_height::<-10>),
            adjustment("Raise waist  =", bottoms.waist < 1.0, adjust_height::<10>),
        ]);
    } else if style.color.is_some() {
        entries.extend([
            adjustment(
                "Shorten left sleeve  [",
                style.left_sleeve > 0.0,
                adjust_left::<-10>,
            ),
            adjustment(
                "Lengthen left sleeve  ]",
                style.left_sleeve < 1.0,
                adjust_left::<10>,
            ),
            adjustment(
                "Shorten right sleeve  {",
                style.right_sleeve > 0.0,
                adjust_right::<-10>,
            ),
            adjustment(
                "Lengthen right sleeve  }",
                style.right_sleeve < 1.0,
                adjust_right::<10>,
            ),
            adjustment("Raise hem  -", style.hem > 0.0, adjust_height::<-10>),
            adjustment("Lower hem  =", style.hem < 1.0, adjust_height::<10>),
        ]);
    }
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
    choose_pastel(style, Pastel::ALL[INDEX]);
}

fn choose_pastel(style: &mut TopStyle, color: Pastel) {
    if style.bottoms.take().is_some() {
        style.socks = None;
    }
    style.sock_color = None;
    style.color = Some(color);
}

fn select_bottoms(style: &mut TopStyle) {
    let roll = trueos::rng::u64();
    let colors = Pastel::ALL.len() as u64;
    let socks = style.socks.unwrap_or((roll / colors % 11) as f32 / 10.0);
    let sock_color = style
        .sock_color
        .or(style.color)
        .unwrap_or(Pastel::ALL[(roll % colors) as usize]);
    style.color = None;
    style.socks = Some(socks);
    style.sock_color = Some(sock_color);
    style.bottoms = Some(BottomStyle::default());
}

fn roll_outfit(style: &mut TopStyle) {
    let roll = trueos::rng::u64();
    let colors = Pastel::ALL.len() as u64;
    let lengths = roll / colors;
    let sleeves = (lengths % 11) as f32 / 10.0;
    *style = TopStyle {
        color: Some(Pastel::ALL[(roll % colors) as usize]),
        left_sleeve: sleeves,
        right_sleeve: sleeves,
        hem: (lengths / 11 % 11) as f32 / 10.0,
        socks: Some((lengths / 121 % 11) as f32 / 10.0),
        sock_color: None,
        bottoms: None,
    };
}

fn clear_outfit(style: &mut TopStyle) {
    style.color = None;
    style.socks = None;
    style.sock_color = None;
    style.bottoms = None;
}

fn percentage(length: f32) -> i32 {
    (length * 100.0).round() as i32
}

fn adjust(length: &mut f32, step: i32) {
    *length = (percentage(*length) + step).clamp(0, 100) as f32 / 100.0;
}

fn adjust_left<const STEP: i32>(style: &mut TopStyle) {
    let length = style
        .bottoms
        .as_mut()
        .map_or(&mut style.left_sleeve, |bottoms| &mut bottoms.left_leg);
    adjust(length, STEP);
}

fn adjust_right<const STEP: i32>(style: &mut TopStyle) {
    let length = style
        .bottoms
        .as_mut()
        .map_or(&mut style.right_sleeve, |bottoms| &mut bottoms.right_leg);
    adjust(length, STEP);
}

fn adjust_height<const STEP: i32>(style: &mut TopStyle) {
    let length = style
        .bottoms
        .as_mut()
        .map_or(&mut style.hem, |bottoms| &mut bottoms.waist);
    adjust(length, STEP);
}

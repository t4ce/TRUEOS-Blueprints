use crate::model::Selection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSection {
    Database,
    View,
    Action,
}

impl MenuSection {
    pub const ORDER: [Self; 3] = [Self::Database, Self::View, Self::Action];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Database => "DB",
            Self::View => "View",
            Self::Action => "Action",
        }
    }

    pub fn stepped(self, reverse: bool) -> Self {
        let index = Self::ORDER
            .iter()
            .position(|section| *section == self)
            .unwrap_or(0);
        let next = if reverse {
            (index + Self::ORDER.len() - 1) % Self::ORDER.len()
        } else {
            (index + 1) % Self::ORDER.len()
        };
        Self::ORDER[next]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Refresh,
    ResetDemo,
    ResetEmpty,
    Center,
    CycleSpacing,
    ToggleValues,
    Help,
    NewTable,
    NewRow,
    EditKey,
    EditValue,
    Delete,
    Exit,
}

#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub label: &'static str,
    pub command: Command,
}

#[derive(Clone, Copy, Debug)]
pub struct MenuState {
    pub section: MenuSection,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            section: MenuSection::Database,
        }
    }
}

impl MenuState {
    pub fn cycle(&mut self, reverse: bool) {
        self.section = self.section.stepped(reverse);
    }

    pub fn entries(&self, selection: &Selection) -> Vec<MenuEntry> {
        entries_for(self.section, selection)
    }

    pub fn command_for_digit(&self, selection: &Selection, digit: char) -> Option<Command> {
        let index = digit.to_digit(10)? as usize;
        self.entries(selection).get(index).map(|entry| entry.command)
    }
}

pub fn entries_for(section: MenuSection, selection: &Selection) -> Vec<MenuEntry> {
    match section {
        MenuSection::Database => vec![
            MenuEntry {
                label: "refresh",
                command: Command::Refresh,
            },
            MenuEntry {
                label: "demo reset",
                command: Command::ResetDemo,
            },
            MenuEntry {
                label: "empty reset",
                command: Command::ResetEmpty,
            },
        ],
        MenuSection::View => vec![
            MenuEntry {
                label: "center",
                command: Command::Center,
            },
            MenuEntry {
                label: "spacing",
                command: Command::CycleSpacing,
            },
            MenuEntry {
                label: "values",
                command: Command::ToggleValues,
            },
            MenuEntry {
                label: "help",
                command: Command::Help,
            },
        ],
        MenuSection::Action => match selection {
            Selection::Database => vec![
                MenuEntry {
                    label: "new table",
                    command: Command::NewTable,
                },
                MenuEntry {
                    label: "exit",
                    command: Command::Exit,
                },
            ],
            Selection::Table { .. } => vec![
                MenuEntry {
                    label: "new row",
                    command: Command::NewRow,
                },
                MenuEntry {
                    label: "delete table",
                    command: Command::Delete,
                },
                MenuEntry {
                    label: "new table",
                    command: Command::NewTable,
                },
                MenuEntry {
                    label: "exit",
                    command: Command::Exit,
                },
            ],
            Selection::Row { .. } => vec![
                MenuEntry {
                    label: "edit value",
                    command: Command::EditValue,
                },
                MenuEntry {
                    label: "edit key",
                    command: Command::EditKey,
                },
                MenuEntry {
                    label: "delete row",
                    command: Command::Delete,
                },
                MenuEntry {
                    label: "new row",
                    command: Command::NewRow,
                },
                MenuEntry {
                    label: "exit",
                    command: Command::Exit,
                },
            ],
        },
    }
}

//! Command parsing and dispatch for the player UI.
//!
//! This module is intentionally small and backend-agnostic. The UI can parse
//! prompt input into a [`ParsedCommand`], then dispatch it into any type that
//! implements [`ControlEventHandler`].

/// All commands currently represented by the UI and prompt parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Load,
    Play,
    Pause,
    Quit,
    Terminate,
    MainView,
    Next,
    Prev,
    Goto,
    Label,
    Loop,
    ClearLoop,
    Volume,
    VolumeUp,
    VolumeDown,
    Mute,
    Unmute,
    Gain,
    Pitch,
    Cut,
    Rec,
    Mic,
    Save,
    Playlist,
    PlaylistTop,
    PlaylistBottom,
    PlaylistUp,
    PlaylistDown,
    Webradio,
    Scope,
    Spectro,
    Transcribe,
    Help,
}

impl Command {
    /// Returns the canonical prompt name for this command.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Quit => "quit",
            Self::Terminate => "terminate",
            Self::MainView => "mainview",
            Self::Next => "next",
            Self::Prev => "prev",
            Self::Goto => "goto",
            Self::Label => "label",
            Self::Loop => "loop",
            Self::ClearLoop => "clearloop",
            Self::Volume => "volume",
            Self::VolumeUp => "+",
            Self::VolumeDown => "-",
            Self::Mute => "mute",
            Self::Unmute => "unmute",
            Self::Gain => "gain",
            Self::Pitch => "pitch",
            Self::Cut => "cut",
            Self::Rec => "rec",
            Self::Mic => "mic",
            Self::Save => "save",
            Self::Playlist => "playlist",
            Self::PlaylistTop => "top",
            Self::PlaylistBottom => "bottom",
            Self::PlaylistUp => "up",
            Self::PlaylistDown => "down",
            Self::Webradio => "webradio",
            Self::Scope => "scope",
            Self::Spectro => "spectro",
            Self::Transcribe => "transcribe",
            Self::Help => "help",
        }
    }
}

/// Canonical command plus accepted aliases.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    /// The command routed by this spec.
    pub command: Command,
    /// Extra names accepted by the parser.
    pub aliases: &'static [&'static str],
}

/// Commands and aliases accepted by [`parse_command`].
pub const COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        command: Command::Load,
        aliases: &["l", "open"],
    },
    CommandSpec {
        command: Command::Play,
        aliases: &["p", "▶"],
    },
    CommandSpec {
        command: Command::Pause,
        aliases: &["a", "stop", "⏸", "⏹"],
    },
    CommandSpec {
        command: Command::Quit,
        aliases: &["q"],
    },
    CommandSpec {
        command: Command::Terminate,
        aliases: &["shutdown", "x", "leave", "exit"],
    },
    CommandSpec {
        command: Command::MainView,
        aliases: &["main", "player", "back", "close"],
    },
    CommandSpec {
        command: Command::Next,
        aliases: &["n"],
    },
    CommandSpec {
        command: Command::Prev,
        aliases: &["v", "previous"],
    },
    CommandSpec {
        command: Command::Goto,
        aliases: &["g", "seek"],
    },
    CommandSpec {
        command: Command::Label,
        aliases: &["e"],
    },
    CommandSpec {
        command: Command::Loop,
        aliases: &[],
    },
    CommandSpec {
        command: Command::ClearLoop,
        aliases: &["unloop"],
    },
    CommandSpec {
        command: Command::Volume,
        aliases: &["vol"],
    },
    CommandSpec {
        command: Command::VolumeUp,
        aliases: &[],
    },
    CommandSpec {
        command: Command::VolumeDown,
        aliases: &[],
    },
    CommandSpec {
        command: Command::Mute,
        aliases: &["m"],
    },
    CommandSpec {
        command: Command::Unmute,
        aliases: &["u"],
    },
    CommandSpec {
        command: Command::Gain,
        aliases: &[],
    },
    CommandSpec {
        command: Command::Pitch,
        aliases: &["t"],
    },
    CommandSpec {
        command: Command::Cut,
        aliases: &["c"],
    },
    CommandSpec {
        command: Command::Rec,
        aliases: &["r", "record"],
    },
    CommandSpec {
        command: Command::Mic,
        aliases: &["i"],
    },
    CommandSpec {
        command: Command::Save,
        aliases: &["s"],
    },
    CommandSpec {
        command: Command::Playlist,
        aliases: &["y"],
    },
    CommandSpec {
        command: Command::PlaylistTop,
        aliases: &[],
    },
    CommandSpec {
        command: Command::PlaylistBottom,
        aliases: &[],
    },
    CommandSpec {
        command: Command::PlaylistUp,
        aliases: &[],
    },
    CommandSpec {
        command: Command::PlaylistDown,
        aliases: &[],
    },
    CommandSpec {
        command: Command::Webradio,
        aliases: &["radio", "w"],
    },
    CommandSpec {
        command: Command::Scope,
        aliases: &["o", "oscillo", "oscilloscope"],
    },
    CommandSpec {
        command: Command::Spectro,
        aliases: &["spectroscope"],
    },
    CommandSpec {
        command: Command::Transcribe,
        aliases: &[],
    },
    CommandSpec {
        command: Command::Help,
        aliases: &["?"],
    },
];

/// A parsed command action plus the remaining prompt arguments.
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    /// Resolved command.
    pub command: Command,
    #[allow(dead_code)]
    pub raw: String,
    #[allow(dead_code)]
    pub raw_action: String,
    /// Whitespace-separated arguments after the command/action.
    pub args: Vec<String>,
}

impl ParsedCommand {
    /// Returns an argument by index.
    pub fn arg(&self, index: usize) -> Option<&str> {
        self.args.get(index).map(String::as_str)
    }

    /// Joins all arguments back into one space-separated string.
    pub fn rest(&self) -> String {
        self.args.join(" ")
    }
}

/// Errors returned by [`parse_command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseCommandError {
    /// The prompt contained no command text.
    Empty,
    /// The action did not match any command or alias.
    Unknown(String),
    /// A short alias matched more than one command.
    Ambiguous { input: String, matches: Vec<String> },
}

/// Parses prompt text into a command event.
pub fn parse_command(input: &str) -> Result<ParsedCommand, ParseCommandError> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(ParseCommandError::Empty);
    }

    let mut parts = raw.split_whitespace();
    let raw_action = parts.next().unwrap_or_default().to_lowercase();
    let args = parts.map(ToOwned::to_owned).collect::<Vec<_>>();
    let command = resolve_command(raw_action.as_str())?;

    Ok(ParsedCommand {
        command,
        raw: raw.to_owned(),
        raw_action,
        args,
    })
}

/// Returns all canonical command names as a slash-separated help string.
pub fn command_names() -> String {
    COMMAND_SPECS
        .iter()
        .map(|spec| spec.command.name())
        .collect::<Vec<_>>()
        .join("/")
}

fn resolve_command(action: &str) -> Result<Command, ParseCommandError> {
    if let Some(spec) = COMMAND_SPECS.iter().find(|spec| {
        spec.command.name() == action || spec.aliases.iter().any(|alias| *alias == action)
    }) {
        return Ok(spec.command);
    }

    if action.chars().count() <= 2 && action.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        let matches = COMMAND_SPECS
            .iter()
            .filter(|spec| spec.command.name().starts_with(action))
            .map(|spec| spec.command)
            .collect::<Vec<_>>();

        return match matches.as_slice() {
            [] => Err(ParseCommandError::Unknown(action.to_owned())),
            [command] => Ok(*command),
            _ => Err(ParseCommandError::Ambiguous {
                input: action.to_owned(),
                matches: matches
                    .into_iter()
                    .map(Command::name)
                    .map(ToOwned::to_owned)
                    .collect(),
            }),
        };
    }

    Err(ParseCommandError::Unknown(action.to_owned()))
}

/// Event sink implemented by the UI or a future playback backend.
pub trait ControlEventHandler {
    fn on_load(&mut self, _event: &ParsedCommand) {}
    fn on_play(&mut self, _event: &ParsedCommand) {}
    fn on_pause(&mut self, _event: &ParsedCommand) {}
    fn on_quit(&mut self, _event: &ParsedCommand) {}
    fn on_terminate(&mut self, _event: &ParsedCommand) {}
    fn on_mainview(&mut self, _event: &ParsedCommand) {}
    fn on_next(&mut self, _event: &ParsedCommand) {}
    fn on_prev(&mut self, _event: &ParsedCommand) {}
    fn on_goto(&mut self, _event: &ParsedCommand) {}
    fn on_label(&mut self, _event: &ParsedCommand) {}
    fn on_loop(&mut self, _event: &ParsedCommand) {}
    fn on_clear_loop(&mut self, _event: &ParsedCommand) {}
    fn on_volume(&mut self, _event: &ParsedCommand) {}
    fn on_volume_up(&mut self, _event: &ParsedCommand) {}
    fn on_volume_down(&mut self, _event: &ParsedCommand) {}
    fn on_mute(&mut self, _event: &ParsedCommand) {}
    fn on_unmute(&mut self, _event: &ParsedCommand) {}
    fn on_gain(&mut self, _event: &ParsedCommand) {}
    fn on_pitch(&mut self, _event: &ParsedCommand) {}
    fn on_cut(&mut self, _event: &ParsedCommand) {}
    fn on_rec(&mut self, _event: &ParsedCommand) {}
    fn on_mic(&mut self, _event: &ParsedCommand) {}
    fn on_save(&mut self, _event: &ParsedCommand) {}
    fn on_playlist(&mut self, _event: &ParsedCommand) {}
    fn on_playlist_top(&mut self, _event: &ParsedCommand) {}
    fn on_playlist_bottom(&mut self, _event: &ParsedCommand) {}
    fn on_playlist_up(&mut self, _event: &ParsedCommand) {}
    fn on_playlist_down(&mut self, _event: &ParsedCommand) {}
    fn on_webradio(&mut self, _event: &ParsedCommand) {}
    fn on_scope(&mut self, _event: &ParsedCommand) {}
    fn on_spectro(&mut self, _event: &ParsedCommand) {}
    fn on_transcribe(&mut self, _event: &ParsedCommand) {}
    fn on_help(&mut self, _event: &ParsedCommand) {}
}

/// Routes a parsed command to the matching [`ControlEventHandler`] callback.
pub fn dispatch(handler: &mut impl ControlEventHandler, event: &ParsedCommand) {
    match event.command {
        Command::Load => handler.on_load(event),
        Command::Play => handler.on_play(event),
        Command::Pause => handler.on_pause(event),
        Command::Quit => handler.on_quit(event),
        Command::Terminate => handler.on_terminate(event),
        Command::MainView => handler.on_mainview(event),
        Command::Next => handler.on_next(event),
        Command::Prev => handler.on_prev(event),
        Command::Goto => handler.on_goto(event),
        Command::Label => handler.on_label(event),
        Command::Loop => handler.on_loop(event),
        Command::ClearLoop => handler.on_clear_loop(event),
        Command::Volume => handler.on_volume(event),
        Command::VolumeUp => handler.on_volume_up(event),
        Command::VolumeDown => handler.on_volume_down(event),
        Command::Mute => handler.on_mute(event),
        Command::Unmute => handler.on_unmute(event),
        Command::Gain => handler.on_gain(event),
        Command::Pitch => handler.on_pitch(event),
        Command::Cut => handler.on_cut(event),
        Command::Rec => handler.on_rec(event),
        Command::Mic => handler.on_mic(event),
        Command::Save => handler.on_save(event),
        Command::Playlist => handler.on_playlist(event),
        Command::PlaylistTop => handler.on_playlist_top(event),
        Command::PlaylistBottom => handler.on_playlist_bottom(event),
        Command::PlaylistUp => handler.on_playlist_up(event),
        Command::PlaylistDown => handler.on_playlist_down(event),
        Command::Webradio => handler.on_webradio(event),
        Command::Scope => handler.on_scope(event),
        Command::Spectro => handler.on_spectro(event),
        Command::Transcribe => handler.on_transcribe(event),
        Command::Help => handler.on_help(event),
    }
}

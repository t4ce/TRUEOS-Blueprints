use player_scope::ui::{TrackData, UiConfig};

const DEFAULT_FILE_PATH: &str = "aud.m4a";
const NEXT_FILE_PATH: &str = "/apps/scope/tui/wide-demo.flac";

pub fn config() -> UiConfig {
    UiConfig {
        ready_response: "ready".into(),
        default_file_path: DEFAULT_FILE_PATH.into(),
        next_file_path: NEXT_FILE_PATH.into(),
        default_track: default_track(),
        next_track: next_track(),
        initial_volume: 50,
        initial_gain: 0,
        initial_pitch: 0,
        initial_progress_secs: 84,
        next_progress_secs: 24,
        default_duration_secs: 227,
        next_duration_secs: 196,
        initial_loop_range: Some((0, 73)),
        logs: vec![
            "TUI demo is ready.".into(),
            "playlist lists M4A files in /common/audio; arrows select, Enter plays.".into(),
        ],
        playlist_entries: Vec::new(),
    }
}

fn default_track() -> TrackData {
    TrackData {
        file: "aud.m4a".into(),
        album: "scope".into(),
        artist: "tui".into(),
        codec: "AAC (LC)".into(),
        bitrate: "256 kbps".into(),
        sample_rate: "44.1 kHz".into(),
        channels: "stereo".into(),
        size: "5.21 MB".into(),
    }
}

fn next_track() -> TrackData {
    TrackData {
        file: "wide-demo.flac".into(),
        album: "terminal sketches".into(),
        artist: "player lab".into(),
        codec: "FLAC".into(),
        bitrate: "lossless".into(),
        sample_rate: "48 kHz".into(),
        channels: "stereo".into(),
        size: "18.04 MB".into(),
    }
}

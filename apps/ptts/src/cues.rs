//! Predecoded notification samples; synthesis text never contains cue names.
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Intro {
    Startthink,
    Starttalk,
}
impl Intro {
    pub fn pcm(self) -> &'static [u8] {
        match self {
            Self::Startthink => crate::bundle::STARTTHINK,
            Self::Starttalk => crate::bundle::STARTTALK,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CueRequest {
    pub cue: Done,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Done {
    Done,
}

pub fn done_pcm() -> &'static [u8] {
    crate::bundle::DONE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_distinct_bounded_complete_stereo_frames() {
        let clips = [Intro::Startthink.pcm(), Intro::Starttalk.pcm(), done_pcm()];
        for pcm in clips {
            assert_eq!(pcm.len() % 4, 0);
            assert!((48_000 * 4..=48_000 * 4 * 3).contains(&pcm.len()));
            assert!(pcm.iter().any(|&byte| byte != 0));
        }
        assert_ne!(clips[0], clips[1]);
        assert_ne!(clips[1], clips[2]);
        assert!(serde_json::from_str::<CueRequest>(r#"{"cue":"done"}"#).is_ok());
        assert!(serde_json::from_str::<CueRequest>(r#"{"cue":"starttalk"}"#).is_err());
    }
}

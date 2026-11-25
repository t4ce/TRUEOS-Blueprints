//! Predecoded notification samples; synthesis text never contains cue names.
use serde::Deserialize;
use std::num::NonZeroU64;

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
    pub cue: Cue,
    pub spirit_turn: Option<NonZeroU64>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cue {
    Done,
    End,
    Abort,
}

pub fn done_pcm() -> &'static [u8] {
    crate::bundle::DONE
}

#[cfg(any(target_os = "trueos", test))]
#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(crate) enum SpiritCue {
    Speech = 0,
    StartThink = 1,
    StartTalk = 2,
    Done = 3,
    End = 4,
    Abort = 5,
}

#[cfg(any(target_os = "trueos", test))]
impl SpiritCue {
    pub(crate) fn speech(intro: Option<Intro>) -> Self {
        match intro {
            None => Self::Speech,
            Some(Intro::Startthink) => Self::StartThink,
            Some(Intro::Starttalk) => Self::StartTalk,
        }
    }

    pub(crate) fn payload(self, turn: NonZeroU64) -> [u8; 9] {
        let mut payload = [0; 9];
        payload[0] = self as u8;
        payload[1..].copy_from_slice(&turn.get().to_le_bytes());
        payload
    }
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

    #[test]
    fn spirit_cues_encode_the_kind_and_little_endian_turn() {
        let turn = NonZeroU64::new(0x0807_0605_0403_0201).unwrap();
        for (kind, cue) in [
            (0, SpiritCue::speech(None)),
            (1, SpiritCue::speech(Some(Intro::Startthink))),
            (2, SpiritCue::speech(Some(Intro::Starttalk))),
            (3, SpiritCue::Done),
            (4, SpiritCue::End),
            (5, SpiritCue::Abort),
        ] {
            assert_eq!(cue.payload(turn), [kind, 1, 2, 3, 4, 5, 6, 7, 8]);
        }
    }
}

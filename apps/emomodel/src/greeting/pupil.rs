use crate::{
    animation::ARM_RECOVERY_SECONDS,
    greeting_motion::{MotionPhase, MotionSample},
};

const TRANSITION_NS: u64 = 300_000_000;
const TRANSITION_SECONDS: f32 = TRANSITION_NS as f32 / 1_000_000_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Stage {
    #[default]
    Neutral,
    Attentive,
    Engaged,
    Wide,
}

impl Stage {
    fn area(self) -> f32 {
        match self {
            Self::Neutral => 1.0,
            Self::Attentive => 2.0,
            Self::Engaged => 3.0,
            Self::Wide => 4.0,
        }
    }
}

pub(super) struct Transition {
    from: f32,
    target: Stage,
    started_ns: u64,
}

impl Default for Transition {
    fn default() -> Self {
        Self {
            from: Stage::Neutral.area(),
            target: Stage::Neutral,
            started_ns: 0,
        }
    }
}

impl Transition {
    pub(super) fn sample(&mut self, stage: Stage, now_ns: u64) -> f32 {
        let progress = now_ns.saturating_sub(self.started_ns) as f32 / TRANSITION_NS as f32;
        let area = blend(self.from, self.target.area(), progress);
        if stage != self.target {
            self.from = area;
            self.target = stage;
            self.started_ns = now_ns;
        }
        area
    }
}

pub(super) fn nod_stage(seconds: f32, duration: f32) -> Stage {
    if seconds < duration - nod_edge(duration) {
        Stage::Attentive
    } else {
        Stage::Neutral
    }
}

pub(super) fn retained_area(sample: MotionSample, nod_duration: f32, has_greeting: bool) -> f32 {
    let neutral = Stage::Neutral.area();
    let attentive = Stage::Attentive.area();
    let wide = Stage::Wide.area();
    match sample.phase {
        MotionPhase::Greeting | MotionPhase::ReverseGreeting => {
            blend(neutral, wide, sample.seconds / TRANSITION_SECONDS)
        }
        MotionPhase::ToNod | MotionPhase::FromNod => {
            blend(wide, attentive, sample.seconds / ARM_RECOVERY_SECONDS)
        }
        MotionPhase::Nod if has_greeting => attentive,
        MotionPhase::Nod => blend(
            neutral,
            attentive,
            sample.seconds.min(nod_duration - sample.seconds) / nod_edge(nod_duration),
        ),
    }
}

fn nod_edge(duration: f32) -> f32 {
    TRANSITION_SECONDS.min(duration * 0.25).max(f32::EPSILON)
}

fn blend(from: f32, to: f32, progress: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);
    from + (to - from) * (t * t * (3.0 - 2.0 * t))
}

#[cfg(test)]
mod tests {
    use crate::greeting_motion::Timeline;

    use super::*;

    #[test]
    fn reel_stage_changes_can_interrupt_a_fade_at_its_current_size() {
        let mut pupil = Transition::default();
        assert_eq!(pupil.sample(Stage::Wide, TRANSITION_NS), 1.0);
        let halfway = TRANSITION_NS + TRANSITION_NS / 2;
        assert_eq!(pupil.sample(Stage::Engaged, halfway), 2.5);
        assert_eq!(pupil.sample(Stage::Engaged, halfway + TRANSITION_NS), 3.0);
        assert_eq!(pupil.sample(Stage::Neutral, halfway + TRANSITION_NS), 3.0);
        assert_eq!(
            pupil.sample(Stage::Neutral, halfway + 2 * TRANSITION_NS),
            1.0
        );
    }

    #[test]
    fn retained_greeting_reuses_pupil_sizes_in_reverse_and_loops_smoothly() {
        let timeline = Timeline::new(Some(2.3), 2.4);
        let area = |frame| retained_area(timeline.sample(frame), 2.4, true);
        for pose in 0..timeline.forward_frame_count() {
            if let Some(reverse) = timeline.reverse_frame(pose) {
                assert_eq!(area(pose), area(reverse));
            }
        }
        assert_eq!(area(0), 1.0);
        assert!((0..timeline.frame_count()).any(|frame| area(frame) == 4.0));
        for frame in 0..timeline.frame_count() {
            assert!((area(frame + 1) - area(frame)).abs() < 0.35);
        }
    }

    #[test]
    fn standalone_nod_returns_to_neutral_before_the_idle_pose() {
        let timeline = Timeline::new(None, 2.4);
        let area = |frame| retained_area(timeline.sample(frame), 2.4, false);
        assert_eq!(area(0), 1.0);
        assert_eq!(area(timeline.forward_frame_count() / 2), 2.0);
        assert_eq!(area(timeline.forward_frame_count() - 1), 1.0);
    }
}

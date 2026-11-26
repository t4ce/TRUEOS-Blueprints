use crate::animation::{ARM_RECOVERY_SECONDS, SAMPLE_HZ};

const STANDALONE_SETTLE_SECONDS: f32 = 0.45;
const ARM_FRAMES: usize = (ARM_RECOVERY_SECONDS * SAMPLE_HZ as f32) as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MotionPhase {
    Greeting,
    ToNod,
    Nod,
    FromNod,
    ReverseGreeting,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MotionSample {
    pub(super) phase: MotionPhase,
    pub(super) seconds: f32,
    pub(super) body_pose: usize,
}

impl MotionSample {
    pub(super) fn hair_reference(self) -> Option<(usize, f32)> {
        let (reference, elapsed) = match self.phase {
            // Release the opening drape gradually while the character is crouched.
            MotionPhase::Greeting if self.body_pose > 0 => (0, self.body_pose),
            // Match those same drapes while the character returns to that crouch.
            MotionPhase::ReverseGreeting => (self.body_pose, self.body_pose - 1),
            _ => return None,
        };
        let span = (SAMPLE_HZ - 1) as usize;
        let t = 1.0 - elapsed.min(span) as f32 / span as f32;
        (t > 0.0).then_some((reference, t * t * (3.0 - 2.0 * t)))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Timeline {
    nod_duration: f32,
    greeting: Option<Greeting>,
}

#[derive(Clone, Copy)]
struct Greeting {
    duration: f32,
    frames: usize,
    nod_frames: usize,
}

impl Timeline {
    pub(super) fn new(greeting_duration: Option<f32>, nod_duration: f32) -> Self {
        let frames_for = |seconds: f32| (seconds * SAMPLE_HZ as f32).round().max(1.0) as usize;
        Self {
            nod_duration,
            greeting: greeting_duration.map(|duration| Greeting {
                duration,
                frames: frames_for(duration),
                nod_frames: frames_for(nod_duration - ARM_RECOVERY_SECONDS),
            }),
        }
    }

    pub(super) fn forward_frame_count(&self) -> usize {
        self.greeting.map_or_else(
            || ((self.nod_duration + STANDALONE_SETTLE_SECONDS) * SAMPLE_HZ as f32).ceil() as usize,
            |greeting| greeting.frames + ARM_FRAMES + greeting.nod_frames + 1,
        )
    }

    pub(super) fn frame_count(&self) -> usize {
        self.forward_frame_count()
            + self
                .greeting
                .map_or(0, |greeting| greeting.frames + ARM_FRAMES - 1)
    }

    pub(super) fn sample(&self, frame: usize) -> MotionSample {
        let frame = frame % self.frame_count();
        let Some(greeting) = self.greeting else {
            return MotionSample {
                phase: MotionPhase::Nod,
                seconds: (frame as f32 / SAMPLE_HZ as f32).min(self.nod_duration),
                body_pose: frame,
            };
        };

        let forward_frames = self.forward_frame_count();
        let arm_end = greeting.frames + ARM_FRAMES;
        let reversing = frame >= forward_frames;
        let body_pose = if reversing {
            // The final Nod already matches the arms-down pose. The opening
            // crouch appears once, as frame zero when playback wraps.
            arm_end - 1 - (frame - forward_frames)
        } else {
            frame
        };

        let (phase, seconds) = if body_pose < greeting.frames {
            let phase = if reversing {
                MotionPhase::ReverseGreeting
            } else {
                MotionPhase::Greeting
            };
            (
                phase,
                greeting.duration * (body_pose as f32 / greeting.frames as f32),
            )
        } else if body_pose < arm_end {
            let phase = if reversing {
                MotionPhase::FromNod
            } else {
                MotionPhase::ToNod
            };
            let progress = (body_pose - greeting.frames) as f32 / ARM_FRAMES as f32;
            (phase, progress * ARM_RECOVERY_SECONDS)
        } else {
            let seconds = if body_pose + 1 == forward_frames {
                self.nod_duration
            } else {
                let progress = (body_pose - arm_end) as f32 / greeting.nod_frames as f32;
                ARM_RECOVERY_SECONDS + progress * (self.nod_duration - ARM_RECOVERY_SECONDS)
            };
            (MotionPhase::Nod, seconds)
        };
        MotionSample {
            phase,
            seconds,
            body_pose,
        }
    }

    pub(super) fn reverse_frame(&self, body_pose: usize) -> Option<usize> {
        let greeting = self.greeting?;
        let arm_end = greeting.frames + ARM_FRAMES;
        (1..arm_end)
            .contains(&body_pose)
            .then(|| self.forward_frame_count() + (arm_end - 1 - body_pose))
    }

    pub(super) fn landing_weight(&self, frame: usize) -> f32 {
        let Some(greeting) = self.greeting else {
            return 0.0;
        };
        let sample = self.sample(frame);
        let t = match sample.phase {
            MotionPhase::Greeting => 0.0,
            MotionPhase::ToNod | MotionPhase::Nod => {
                (sample.body_pose - greeting.frames) as f32
                    / (ARM_FRAMES + greeting.nod_frames) as f32
            }
            MotionPhase::FromNod => 1.0,
            MotionPhase::ReverseGreeting => sample.body_pose as f32 / greeting.frames as f32,
        };
        t * t * t * (10.0 + t * (-15.0 + t * 6.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_lowers_arms_and_nods_once_before_reversing() {
        let timeline = Timeline::new(Some(2.3), 2.4);
        let forward = (0..timeline.forward_frame_count())
            .map(|frame| timeline.sample(frame))
            .collect::<Vec<_>>();
        let landing = forward
            .iter()
            .position(|sample| sample.phase == MotionPhase::ToNod)
            .unwrap();
        let nod = forward
            .iter()
            .position(|sample| sample.phase == MotionPhase::Nod)
            .unwrap();
        assert_eq!(landing, 104);
        assert_eq!(nod - landing, ARM_FRAMES);
        assert_eq!(forward[0].seconds, 0.0);
        assert_eq!(forward[landing].seconds, 0.0);
        assert_eq!(forward[nod].seconds, ARM_RECOVERY_SECONDS);
        assert_eq!(forward.last().unwrap().seconds, 2.4);
        assert!(
            forward[nod..]
                .iter()
                .all(|sample| sample.phase == MotionPhase::Nod)
        );

        let reverse = (timeline.forward_frame_count()..timeline.frame_count())
            .map(|frame| timeline.sample(frame))
            .collect::<Vec<_>>();
        assert_eq!(reverse.len(), nod - 1);
        for (index, (sample, mirror)) in
            reverse.iter().zip(forward[1..nod].iter().rev()).enumerate()
        {
            assert_eq!(sample.body_pose, mirror.body_pose);
            assert_eq!(sample.seconds, mirror.seconds);
            let phase = if mirror.phase == MotionPhase::ToNod {
                MotionPhase::FromNod
            } else {
                MotionPhase::ReverseGreeting
            };
            assert_eq!(sample.phase, phase);
            assert_eq!(
                timeline.reverse_frame(sample.body_pose),
                Some(timeline.forward_frame_count() + index)
            );
        }
        assert_eq!(timeline.reverse_frame(0), None);
        assert_eq!(timeline.reverse_frame(nod), None);
        assert_eq!(reverse.last().unwrap().body_pose, 1);
        assert_eq!(reverse.last().unwrap().hair_reference(), Some((1, 1.0)));
        assert_eq!(forward[0].hair_reference(), None);
        assert_eq!(forward[1].hair_reference().unwrap().0, 0);
        assert!(reverse.windows(2).all(|pair| {
            pair[0].hair_reference().map_or(0.0, |(_, weight)| weight)
                <= pair[1].hair_reference().map_or(0.0, |(_, weight)| weight)
        }));
        assert_eq!(timeline.sample(timeline.frame_count()), forward[0]);
    }

    #[test]
    fn camera_pans_to_landing_and_eases_back_through_the_reverse() {
        let timeline = Timeline::new(Some(7.266_812), 2.4);
        let greeting = timeline.greeting.unwrap();
        let peak = timeline.forward_frame_count() - 1;
        assert_eq!(timeline.landing_weight(0), 0.0);
        assert_eq!(timeline.landing_weight(greeting.frames), 0.0);
        assert_eq!(timeline.landing_weight(peak), 1.0);
        assert_eq!(timeline.landing_weight(timeline.forward_frame_count()), 1.0);
        for frame in greeting.frames..peak {
            assert!(timeline.landing_weight(frame) <= timeline.landing_weight(frame + 1));
        }
        for frame in peak..timeline.frame_count() {
            assert!(timeline.landing_weight(frame) >= timeline.landing_weight(frame + 1));
        }
        assert!(timeline.landing_weight(timeline.frame_count() - 1) < 0.0001);
        assert_eq!(timeline.landing_weight(timeline.frame_count()), 0.0);
    }

    #[test]
    fn standalone_nod_keeps_its_settled_tail() {
        let timeline = Timeline::new(None, 1.4);
        assert_eq!(timeline.frame_count(), 84);
        assert_eq!(timeline.forward_frame_count(), timeline.frame_count());
        assert_eq!(timeline.sample(0).phase, MotionPhase::Nod);
        assert_eq!(timeline.sample(45).seconds, 1.0);
        assert_eq!(timeline.sample(83).seconds, 1.4);
        assert_eq!(timeline.sample(timeline.frame_count()), timeline.sample(0));
        assert_eq!(timeline.reverse_frame(1), None);
        assert_eq!(timeline.landing_weight(45), 0.0);
    }
}

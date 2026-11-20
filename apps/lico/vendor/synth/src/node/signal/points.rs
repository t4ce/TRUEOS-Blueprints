use crate::GlicolPara;
use crate::{Buffer, Input, Message, Node};
use glicol_parser::nodes::{Duration, TimeList};
use hashbrown::HashMap;

#[derive(Debug, Clone)]
pub struct Points {
    pub point_list: Vec<(usize, f32)>, // time and value
    points: Vec<(TimeList, f32)>,
    pub span: f32,
    pub bpm: f32,
    pub sr: usize,
    step: usize,
    _index: usize,
    is_looping: bool,
    input_order: Vec<usize>,
}

impl Default for Points {
    fn default() -> Self {
        Self::new()
    }
}

impl Points {
    pub fn new() -> Self {
        // points: GlicolPara, bpm: f32, sr: usize, span: f32
        // let mut point_list = process_points(points, bpm, sr, span);
        // println!("point_list {:?}", point_list);
        Self {
            point_list: vec![],
            points: vec![],
            span: 1.0,
            bpm: 120.,
            sr: 44100,
            step: 0,
            _index: 0,
            is_looping: false,
            input_order: vec![],
        }
    }

    pub fn span(self, span: f32) -> Self {
        Self { span, ..self }
    }

    pub fn is_looping(self, is_looping: bool) -> Self {
        Self { is_looping, ..self }
    }

    pub fn bpm(self, bpm: f32) -> Self {
        Self { bpm, ..self }
    }

    pub fn sr(self, sr: usize) -> Self {
        Self { sr, ..self }
    }

    pub fn points(mut self, points: Vec<(TimeList, f32)>) -> Self {
        self.points = points;
        self.update_point_list();
        self
    }

    fn cycle_frames(&self) -> f32 {
        // A negative parser span means that the patch omitted its span.
        let span = if self.span > 0.0 { self.span } else { 1.0 };
        240.0 / self.bpm * self.sr as f32 * span
    }

    fn update_point_list(&mut self) {
        let cycle_frames = self.cycle_frames();
        let sr = self.sr as f32;
        self.point_list.clear();
        self.point_list
            .extend(self.points.iter().map(|(time, value)| {
                let bar_pos = time.bar * cycle_frames;
                let time_pos = time.time.as_ref().map_or(0.0, |kind| match kind {
                    Duration::Bar(b) => b * cycle_frames,
                    Duration::Seconds(s) => s * sr,
                    Duration::Milliseconds(ms) => ms / 1000.0 * sr,
                });
                ((bar_pos + time_pos) as usize, *value)
            }));
        self.point_list.sort_by_key(|point| point.0);
        if self.point_list.first().is_some_and(|point| point.0 != 0) {
            self.point_list.insert(0, (0, 0.0));
        }
    }
}

impl<const N: usize> Node<N> for Points {
    fn process(&mut self, _inputs: &mut HashMap<usize, Input<N>>, output: &mut [Buffer<N>]) {
        let samples = &self.point_list;
        let Some(&(first_pos, first_value)) = samples.first() else {
            output[0].silence();
            return;
        };
        let period = (self.cycle_frames() as usize).max(1);

        for out in &mut *output[0] {
            let pos = if self.is_looping {
                self.step % period
            } else {
                self.step
            };
            let next = samples.partition_point(|point| point.0 <= pos);
            let (start, value) = samples[next.saturating_sub(1)];
            let (end, next_value) = samples.get(next).copied().unwrap_or_else(|| {
                if self.is_looping {
                    (period.saturating_add(first_pos), first_value)
                } else {
                    (start, value)
                }
            });
            let duration = end.saturating_sub(start);
            *out = if duration == 0 {
                value
            } else {
                let progress = pos.saturating_sub(start) as f32 / duration as f32;
                value + progress * (next_value - value)
            };
            self.step += 1;
        }
    }
    fn send_msg(&mut self, info: Message) {
        match info {
            Message::SetParam(0, GlicolPara::Points(time_list)) => {
                self.points = time_list;
                self.update_point_list();
                self.step = 0;
            }
            Message::SetToNumber(1, num) => {
                self.span = num;
                self.update_point_list();
                self.step = 0;
            }
            Message::SetToBool(2, b) => {
                self.is_looping = b;
                self.step = 0;
            }
            Message::SetBPM(bpm) => {
                self.bpm = bpm;
                self.update_point_list();
            }
            Message::Index(i) => self.input_order.push(i),
            Message::IndexOrder(pos, index) => self.input_order.insert(pos, index),
            _ => {}
        }
    }
}

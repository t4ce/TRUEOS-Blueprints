use lico_graph::{Arrangement, Graph, NodeKind};

const MIN_STEP_NS: u64 = 260_000_000;
const MAX_STEP_NS: u64 = 860_000_000;
const DIRECT: u8 = 1 << 4;
const ROLES: [Role; 4] = [Role::Kick, Role::Snare, Role::Lead, Role::Shaker];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Shaker,
    Lead,
    Snare,
    Kick,
}

impl Role {
    fn of(kind: NodeKind) -> Option<Self> {
        match kind {
            NodeKind::Kick => Some(Self::Kick),
            NodeKind::Snare => Some(Self::Snare),
            NodeKind::Hat | NodeKind::Noise => Some(Self::Shaker),
            NodeKind::Tone | NodeKind::Sample => Some(Self::Lead),
            _ => None,
        }
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }

    fn matches(self, roles: u8) -> bool {
        let mask = if self == Self::Lead {
            self.bit() | DIRECT
        } else {
            self.bit()
        };
        roles & mask != 0
    }

    fn duration(self) -> f32 {
        match self {
            Self::Kick => 0.24,
            Self::Snare => 0.20,
            Self::Shaker => 0.10,
            Self::Lead => 0.34,
        }
    }
}

struct Lane {
    node: usize,
    period: u64,
    phase: u64,
    hits: Vec<u64>,
}

struct Interval {
    index: u128,
    age: u64,
    duration: u64,
}

impl Lane {
    fn interval(&self, elapsed: u64) -> Option<Interval> {
        let position = u128::from(elapsed) + u128::from(self.phase);
        let phase = (position % u128::from(self.period)) as u64;
        let repeat = position / u128::from(self.period);
        let next = self.hits.partition_point(|&hit| hit <= phase);
        let (repeat, index, age) = if next > 0 {
            (repeat, next - 1, phase - self.hits[next - 1])
        } else if repeat > 0 {
            let last = self.hits.len() - 1;
            (repeat - 1, last, self.period - self.hits[last] + phase)
        } else {
            return None;
        };
        let duration = if index + 1 == self.hits.len() {
            self.period - self.hits[index] + self.hits[0]
        } else {
            self.hits[index + 1] - self.hits[index]
        };
        Some(Interval {
            index: repeat * self.hits.len() as u128 + index as u128,
            age,
            duration,
        })
    }

    fn for_steps(&self) -> Self {
        let mut hits = Vec::new();
        for &hit in self.hits.iter() {
            if hits.last().is_none_or(|&last| hit - last >= MIN_STEP_NS) {
                hits.push(hit);
            }
        }
        if hits.len() > 1 && self.period - hits[hits.len() - 1] + hits[0] < MIN_STEP_NS {
            hits.pop();
        }
        Self {
            node: self.node,
            period: self.period,
            phase: self.phase,
            hits,
        }
    }
}

#[derive(Default)]
pub(super) struct Expression {
    pub(super) grounded: f32,
    pub(super) chest: f32,
    pub(super) wrist: f32,
    pub(super) lead: f32,
}

#[derive(Default)]
pub(super) struct Choreography {
    inputs: Vec<Vec<usize>>,
    kinds: Vec<NodeKind>,
    outlets: Vec<usize>,
    arrangements: Vec<Option<Arrangement>>,
    lanes: Vec<Lane>,
    step_lane: Option<Lane>,
    audible: Vec<u8>,
    visit: Vec<(usize, u8)>,
    pub(super) suspended: bool,
    pub(super) sweep: f32,
}

impl Choreography {
    pub(super) fn new(graph: &Graph) -> Self {
        let len = graph.nodes.len();
        let mut motion = Self {
            inputs: vec![Vec::new(); len],
            kinds: graph.nodes.iter().map(|node| node.kind).collect(),
            arrangements: vec![None; len],
            audible: vec![0; len],
            visit: Vec::with_capacity(len),
            ..Self::default()
        };
        for (index, node) in graph.nodes.iter().enumerate() {
            for input in node.inputs.iter().map(|&input| input as usize) {
                if input < len {
                    motion.inputs[index].push(input);
                }
            }
            if node.kind == NodeKind::Output {
                motion.outlets.push(index);
            }
        }
        for arrangement in &graph.arrangements {
            if let Some(slot) = motion.arrangements.get_mut(arrangement.node as usize) {
                *slot = Some(arrangement.clone());
            }
        }
        motion.trace(None, true);
        let all_audible = motion.audible.clone();
        motion.trace(None, false);

        for pattern in &graph.patterns {
            let node = pattern.node as usize;
            if all_audible.get(node).copied().unwrap_or(0) == 0 || pattern.period_ns == 0 {
                continue;
            }
            let mut hits = pattern.hits.clone();
            hits.retain(|&hit| hit < pattern.period_ns);
            hits.sort_unstable();
            hits.dedup();
            if !hits.is_empty() {
                motion.lanes.push(Lane {
                    node,
                    period: pattern.period_ns,
                    phase: pattern.phase_ns,
                    hits,
                });
            }
        }
        // A step lane must reach the output regardless of the arrangement section.
        // Swapping step lanes mid-stride would move planted feet.
        motion.step_lane = motion
            .lanes
            .iter()
            .filter(|lane| lane.period >= MIN_STEP_NS)
            .filter_map(|lane| {
                ROLES
                    .into_iter()
                    .find(|&role| role != Role::Shaker && role.matches(motion.audible[lane.node]))
                    .map(|role| (role, lane))
            })
            .max_by_key(|&(role, _)| role)
            .map(|(_, lane)| lane.for_steps());
        for (node, _) in graph
            .nodes
            .iter()
            .zip(all_audible)
            .filter(|&(_, roles)| roles != 0)
        {
            motion.suspended |= matches!(node.kind, NodeKind::Tone | NodeKind::Space);
            if matches!(node.kind, NodeKind::Space | NodeKind::Pan) {
                motion.sweep += 0.12;
            }
        }
        motion.sweep = motion.sweep.clamp(0.0, 0.32);
        motion
    }

    fn trace(&mut self, elapsed: Option<u64>, all_sections: bool) {
        self.audible.fill(0);
        self.visit.clear();
        self.visit
            .extend(self.outlets.iter().map(|&node| (node, DIRECT)));
        while let Some((node, downstream_roles)) = self.visit.pop() {
            // Each path uses the instrument nearest its source.
            let roles = Role::of(self.kinds[node]).map_or(downstream_roles, Role::bit);
            let new_roles = roles & !self.audible[node];
            if new_roles == 0 {
                continue;
            }
            self.audible[node] |= new_roles;
            if let Some(arrangement) = &self.arrangements[node]
                && !all_sections
            {
                if let Some(input) = elapsed.and_then(|now| section(arrangement, now))
                    && self.inputs[node].contains(&input)
                {
                    self.visit.push((input, new_roles));
                }
            } else {
                self.visit
                    .extend(self.inputs[node].iter().map(|&input| (input, new_roles)));
            }
        }
    }

    pub(super) fn step(&self, elapsed: f64, counts: usize) -> Option<(usize, f32)> {
        let lane = self.step_lane.as_ref()?;
        Some(lane.interval(nanos(elapsed)).map_or((0, 0.0), |interval| {
            let duration = interval.duration.min(MAX_STEP_NS);
            (
                (interval.index % counts as u128) as usize,
                (interval.age.min(duration) as f64 / duration as f64) as f32,
            )
        }))
    }

    pub(super) fn expression(&mut self, elapsed: f64) -> Expression {
        let now = nanos(elapsed);
        if self.arrangements.iter().any(Option::is_some) {
            self.trace(Some(now), false);
        }
        let mut expression = Expression::default();
        for lane in &self.lanes {
            let roles = self.audible[lane.node];
            if roles == 0 {
                continue;
            }
            let Some(interval) = lane.interval(now) else {
                continue;
            };
            let age = interval.age as f32 * 1e-9;
            let spacing = interval.duration as f32 * 1e-9;
            // Dense audio-rate impulses carry timbre but do not become visual pulses.
            if spacing < 0.045 {
                continue;
            }
            for role in ROLES.into_iter().filter(|&role| role.matches(roles)) {
                let duration = role.duration().min(spacing);
                let attack = (duration * 0.24).min(0.043);
                let value = ease(age / attack) * (1.0 - ease((age - attack) / (duration - attack)));
                let target = match role {
                    Role::Kick => &mut expression.grounded,
                    Role::Snare => &mut expression.chest,
                    Role::Shaker => &mut expression.wrist,
                    Role::Lead => &mut expression.lead,
                };
                *target = target.max(value);
            }
        }
        expression
    }
}

fn section(arrangement: &Arrangement, elapsed: u64) -> Option<usize> {
    if arrangement.period_ns == 0 {
        return None;
    }
    let position = u128::from(elapsed) + u128::from(arrangement.phase_ns);
    let phase = (position % u128::from(arrangement.period_ns)) as u64;
    let next = arrangement
        .sections
        .partition_point(|part| part.start_ns <= phase);
    next.checked_sub(1)
        .map(|index| arrangement.sections[index].input as usize)
}

fn nanos(elapsed: f64) -> u64 {
    (elapsed.max(0.0) * 1e9).round() as u64
}

fn ease(t: f32) -> f32 {
    let u = t.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

#[cfg(test)]
mod tests {
    use lico_graph::{Node, Pattern, Section};

    use super::*;

    fn node(kind: NodeKind, inputs: &[u32]) -> Node {
        Node {
            kind,
            inputs: inputs.to_vec(),
            parameters: Vec::new(),
        }
    }

    fn pattern(node: u32, period_ms: u64, phase_ms: u64, hits_ms: &[u64]) -> Pattern {
        Pattern {
            node,
            period_ns: period_ms * 1_000_000,
            phase_ns: phase_ms * 1_000_000,
            hits: hits_ms.iter().map(|hit| hit * 1_000_000).collect(),
        }
    }

    fn kicks(hits: &[u64]) -> Graph {
        Graph {
            nodes: vec![
                node(NodeKind::Sequence, &[]),
                node(NodeKind::Kick, &[0]),
                node(NodeKind::Output, &[1]),
            ],
            patterns: vec![pattern(0, 1500, 0, hits)],
            arrangements: Vec::new(),
        }
    }

    #[test]
    fn equal_period_patterns_move_at_different_times() {
        let mut straight = Choreography::new(&kicks(&[0, 500, 1000]));
        let mut syncopated = Choreography::new(&kicks(&[0, 650, 1025]));
        assert_eq!(straight.step(0.54, 32).unwrap().0, 1);
        assert_eq!(syncopated.step(0.54, 32).unwrap().0, 0);
        assert!(straight.expression(0.54).grounded > 0.8);
        assert_eq!(syncopated.expression(0.54).grounded, 0.0);
        assert!(syncopated.expression(0.69).grounded > 0.8);
    }

    #[test]
    fn an_impulse_picks_up_the_instrument_it_triggers() {
        let mut graph = kicks(&[0, 500, 1000]);
        graph.nodes[0].kind = NodeKind::Impulse;
        let routed = Choreography::new(&graph).expression(0.04);
        assert!(routed.grounded > 0.8);
        assert_eq!(routed.lead, 0.0);

        graph.nodes[2].inputs = vec![0];
        let direct = Choreography::new(&graph).expression(0.04);
        assert!(direct.lead > 0.8);
        assert_eq!(direct.grounded, 0.0);
    }

    #[test]
    fn run_phase_enters_the_existing_interval() {
        let mut graph = kicks(&[100, 600, 1100]);
        let starting = Choreography::new(&graph);
        assert_eq!(starting.step(0.0, 32), Some((0, 0.0)));
        assert_eq!(starting.step(0.149, 32), Some((0, 0.098)));
        graph.patterns[0].phase_ns = 800_000_000;
        let live = Choreography::new(&graph);
        assert_eq!(live.step(0.0, 32), Some((1, 0.4)));
        assert_eq!(live.step(0.3, 32), Some((2, 0.0)));
        assert_eq!(live.step(0.8, 32), Some((3, 0.0)));
    }

    #[test]
    fn downstream_branches_and_arrangement_choose_the_audible_gesture() {
        let graph = Graph {
            nodes: vec![
                node(NodeKind::Sequence, &[]),
                node(NodeKind::Kick, &[0]),
                node(NodeKind::Hat, &[0]),
                node(NodeKind::Arrangement, &[1, 2]),
                node(NodeKind::Output, &[3]),
                node(NodeKind::Snare, &[0]),
            ],
            patterns: vec![pattern(0, 500, 0, &[0])],
            arrangements: vec![Arrangement {
                node: 3,
                period_ns: 1_000_000_000,
                phase_ns: 0,
                sections: vec![
                    Section {
                        input: 1,
                        start_ns: 0,
                    },
                    Section {
                        input: 2,
                        start_ns: 500_000_000,
                    },
                ],
            }],
        };
        let mut motion = Choreography::new(&graph);
        assert!(motion.step(0.0, 32).is_none());
        let first = motion.expression(0.04);
        assert!(first.grounded > 0.8);
        assert_eq!(first.wrist, 0.0);
        assert_eq!(first.chest, 0.0);
        let second = motion.expression(0.54);
        assert!(second.wrist > 0.8);
        assert_eq!(second.grounded, 0.0);
        assert_eq!(second.chest, 0.0);
    }

    #[test]
    fn unscheduled_tone_extends_the_line_without_inventing_steps() {
        let graph = Graph {
            nodes: vec![
                Node {
                    kind: NodeKind::Tone,
                    inputs: vec![],
                    parameters: vec![440.0],
                },
                node(NodeKind::Space, &[0]),
                node(NodeKind::Output, &[1]),
            ],
            ..Graph::default()
        };
        let mut motion = Choreography::new(&graph);
        assert!(motion.suspended && motion.sweep > 0.0);
        assert_eq!(motion.step(1.243, 32), None);
        assert_eq!(motion.expression(1.243).lead, 0.0);
    }

    #[test]
    fn close_hits_remain_accents_and_long_rests_hold_the_step() {
        let graph = kicks(&[0, 100, 600]);
        let mut motion = Choreography::new(&graph);
        assert_eq!(motion.step(0.14, 32).unwrap().0, 0);
        assert!(motion.expression(0.14).grounded > 0.8);
        assert_eq!(motion.step(0.6, 32), Some((1, 0.0)));
        assert_eq!(motion.step(1.49, 32), Some((1, 1.0)));
    }
}

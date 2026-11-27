use crate::audio::SAMPLE_RATE;
use glicol::{CyclePlayhead, Engine};
use glicol_parser::nodes::{Component, EventValue, NumberOrRef, PSampler, Signal, UsizeOrRef};
use lico_graph::{Arrangement, Graph, Node, NodeKind, Pattern, Section};
use std::collections::{BTreeMap, BTreeSet};

/// Export the audible part of the accepted DSP graph, with phase at the first submitted frame.
pub fn capture<const N: usize>(engine: &Engine<N>, batch_frames: usize, bpm: u16) -> Option<Graph> {
    let ast = engine.active_ast()?;
    let graph = &engine.context.graph;
    let output = engine.context.destination;
    let mut incoming = BTreeMap::<_, Vec<_>>::new();
    for edge in graph.edge_indices() {
        let (input, node) = graph.edge_endpoints(edge)?;
        incoming.entry(node).or_default().push(input);
    }

    let mut audible = BTreeSet::new();
    let mut pending = vec![output];
    while let Some(node) = pending.pop() {
        if audible.insert(node)
            && let Some(inputs) = incoming.get(&node)
        {
            pending.extend(inputs.iter().copied());
        }
    }

    // Order the compiled nodes by chain, independent of the DSP allocator's node indexes.
    let mut chains: Vec<_> = engine.index_info.iter().collect();
    chains.sort_unstable_by_key(|(name, _)| name.as_str());
    let mut components = Vec::with_capacity(audible.len());
    let mut indexes = BTreeMap::new();
    let mut result = Graph::default();
    for (name, chain) in chains {
        let source = ast.nodes.get(name.as_str());
        for (position, &index) in chain.iter().enumerate() {
            if audible.remove(&index) {
                indexes.insert(index, result.nodes.len() as u32);
                let component = source.and_then(|chain| chain.get(position));
                result.nodes.push(
                    component
                        .map_or_else(|| empty_node(NodeKind::Input), |node| describe(node, bpm)),
                );
                components.push((index, component));
            }
        }
    }
    for index in audible {
        indexes.insert(index, result.nodes.len() as u32);
        result.nodes.push(empty_node(if index == output {
            NodeKind::Output
        } else {
            NodeKind::Other
        }));
    }
    for (&index, &node) in &indexes {
        if let Some(input_indexes) = incoming.get(&index) {
            let inputs = &mut result.nodes[node as usize].inputs;
            inputs.extend(input_indexes.iter().map(|input| indexes[input]));
            inputs.sort_unstable();
        }
    }

    for (index, component) in components {
        let (Some(component), Some(playhead)) = (component, engine.cycle_playhead(index)) else {
            continue;
        };
        let node = indexes[&index];
        let cycle = Cycle::at_batch_start(playhead, batch_frames);
        match component {
            Component::Seq(sequence) => {
                let hits = sequence.events.iter().filter_map(|(position, value)| {
                    (!matches!(value, UsizeOrRef::Usize(0))).then_some(*position)
                });
                add_pattern(&mut result, node, cycle, hits);
            }
            Component::PSampler(PSampler::Pattern(pattern)) => {
                // The sampler has one compiled output. Keep its simultaneous drum lanes
                // separately typed so their accents can still be distinguished downstream.
                for kind in [
                    NodeKind::Kick,
                    NodeKind::Snare,
                    NodeKind::Hat,
                    NodeKind::Sample,
                ] {
                    let hits = pattern
                        .event
                        .val_times
                        .iter()
                        .filter_map(|(value, position)| {
                            let EventValue::Symbol(sample) = value else {
                                return None;
                            };
                            (sample_kind(sample) == kind).then_some(*position)
                        });
                    let input = result.nodes.len() as u32;
                    if add_pattern(&mut result, input, cycle, hits) {
                        result.nodes.push(empty_node(kind));
                        result.nodes[node as usize].inputs.push(input);
                    }
                }
            }
            Component::PatternSynth(pattern) => {
                let hits = pattern
                    .symbol
                    .trim_matches('`')
                    .split(',')
                    .filter_map(|hit| hit.split_ascii_whitespace().next()?.parse::<f32>().ok());
                add_pattern(&mut result, node, cycle, hits);
            }
            Component::Imp(_) => {
                add_pattern(&mut result, node, cycle, [0.0]);
            }
            Component::Arrange(arrange) => {
                let input_for = |name: &str| {
                    let input = *engine.index_info.get(name)?.last()?;
                    let input = *indexes.get(&input)?;
                    result.nodes[node as usize]
                        .inputs
                        .contains(&input)
                        .then_some(input)
                };
                if let Some(sections) = arrangement_sections(&arrange.events, cycle, input_for) {
                    result.arrangements.push(Arrangement {
                        node,
                        period_ns: cycle.period_ns,
                        phase_ns: cycle.phase_ns,
                        sections,
                    });
                }
            }
            _ => {}
        }
    }

    Some(result)
}

#[derive(Clone, Copy)]
struct Cycle {
    frames_per_bar: f64,
    period_ns: u64,
    phase_ns: u64,
}

impl Cycle {
    fn at_batch_start(playhead: CyclePlayhead, batch_frames: usize) -> Self {
        let period = playhead.period_frames;
        let end = playhead.next_frame % period;
        let batch = batch_frames % period;
        let start = if end >= batch {
            end - batch
        } else {
            period - (batch - end)
        };
        Self {
            frames_per_bar: playhead.frames_per_bar,
            period_ns: frame_time(period),
            phase_ns: frame_time(start),
        }
    }

    fn time(self, bars: f32, round_up: bool) -> Option<u64> {
        if !bars.is_finite() || bars < 0.0 {
            return None;
        }
        let frames = f64::from(bars) * self.frames_per_bar;
        let frames = if round_up {
            frames.ceil()
        } else {
            frames.floor()
        };
        let at = frame_time(frames as usize);
        (at < self.period_ns).then_some(at)
    }
}

fn frame_time(frames: usize) -> u64 {
    ((frames as u128 * 1_000_000_000) / u128::from(SAMPLE_RATE)).min(u128::from(u64::MAX)) as u64
}

fn add_pattern(
    graph: &mut Graph,
    node: u32,
    cycle: Cycle,
    positions: impl IntoIterator<Item = f32>,
) -> bool {
    let mut hits: Vec<_> = positions
        .into_iter()
        .filter_map(|position| cycle.time(position, false))
        .collect();
    if hits.is_empty() {
        return false;
    }
    hits.sort_unstable();
    hits.dedup();
    graph.patterns.push(Pattern {
        node,
        period_ns: cycle.period_ns,
        phase_ns: cycle.phase_ns,
        hits,
    });
    true
}

fn arrangement_sections(
    events: &[NumberOrRef<&str>],
    cycle: Cycle,
    input_for: impl Fn(&str) -> Option<u32>,
) -> Option<Vec<Section>> {
    let mut bars = 0.0;
    let mut sections: Vec<Section> = Vec::new();
    for pair in events.chunks_exact(2) {
        let [NumberOrRef::Ref(name), NumberOrRef::Number(length)] = pair else {
            return None;
        };
        if !length.is_finite() || *length < 0.0 {
            return None;
        }
        if *length > 0.0
            && let Some(start_ns) = cycle.time(bars, true)
        {
            let section = Section {
                input: input_for(name)?,
                start_ns,
            };
            if let Some(previous) = sections.last_mut().filter(|last| last.start_ns == start_ns) {
                *previous = section;
            } else {
                sections.push(section);
            }
        }
        bars += length;
    }
    (!sections.is_empty()).then_some(sections)
}

fn empty_node(kind: NodeKind) -> Node {
    Node {
        kind,
        inputs: Vec::new(),
        parameters: Vec::new(),
    }
}

fn scalar(value: &NumberOrRef<&str>) -> Vec<f32> {
    match value {
        NumberOrRef::Number(number) => vec![*number],
        NumberOrRef::Ref(_) => Vec::new(),
    }
}

fn sample_kind(sample: &str) -> NodeKind {
    match sample {
        r"\808bd" | r"\kit_kick" => NodeKind::Kick,
        r"\808sd" | r"\kit_snare" => NodeKind::Snare,
        r"\808ch" | r"\kit_hat_closed" | r"\kit_hat_pedal" | r"\kit_hat_open" => NodeKind::Hat,
        _ => NodeKind::Sample,
    }
}

fn describe(component: &Component<'_>, bpm: u16) -> Node {
    use NodeKind::*;
    use glicol_parser::nodes;

    let (kind, mut parameters) = match component {
        Component::Adc(_) => (Input, vec![]),
        Component::Sin(nodes::Sin { param })
        | Component::Tri(nodes::Tri { param })
        | Component::Saw(nodes::Saw { param })
        | Component::Squ(nodes::Squ { param }) => (Tone, scalar(param)),
        Component::SawSynth(nodes::SawSynth { attack, decay })
        | Component::SquSynth(nodes::SquSynth { attack, decay })
        | Component::TriSynth(nodes::TriSynth { attack, decay })
        | Component::MsgSynth(nodes::MsgSynth { attack, decay, .. }) => {
            (Tone, vec![*attack, *decay])
        }
        Component::PatternSynth(nodes::PatternSynth { span, .. }) => (Tone, vec![*span]),
        Component::Noise(_) => (Noise, vec![]),
        Component::Bd(nodes::Bd { param }) => (Kick, scalar(param)),
        Component::Sn(nodes::Sn { param }) => (Snare, scalar(param)),
        Component::Hh(nodes::Hh { param }) => (Hat, scalar(param)),
        Component::Sp(sample) => (sample_kind(sample.sample_sym), vec![]),
        Component::PSampler(_) | Component::Mix(_) | Component::Balance(_) => (Mix, vec![]),
        Component::Seq(_) => (Sequence, vec![]),
        Component::Imp(nodes::Imp { param }) => (Impulse, scalar(param)),
        Component::Arrange(_) => (Arrangement, vec![]),
        Component::EnvPerc(env) => (Envelope, vec![env.attack, env.decay]),
        Component::Adsr(env) => (
            Envelope,
            vec![env.attack, env.decay, env.sustain, env.release],
        ),
        Component::Points(points) if points.is_looping => {
            (Modulation, vec![points.span * 240.0 / f32::from(bpm)])
        }
        Component::Points(_) => (Modulation, vec![]),
        Component::Speed(speed) => (Modulation, vec![speed.speed]),
        Component::Delayn(nodes::Delayn {
            param: UsizeOrRef::Usize(frames),
        }) => (Delay, vec![*frames as f32 / SAMPLE_RATE as f32]),
        Component::Delayn(_) => (Delay, vec![]),
        Component::Delayms(delay) => (
            Delay,
            scalar(&delay.param)
                .into_iter()
                .map(|ms| ms * 0.001)
                .collect(),
        ),
        Component::ApfmsGain(delay) => (Filter, vec![delay.gain]),
        Component::Plate(space) => (Space, vec![space.mix]),
        Component::Reverb(space) => (
            Space,
            vec![
                space.dampening,
                space.room_size,
                space.width,
                space.wet,
                space.dry,
            ],
        ),
        Component::Lpf(filter) => {
            let mut params = vec![filter.qvalue];
            if let Signal::Number(cutoff) = &filter.signal {
                params.push(*cutoff);
            }
            (Filter, params)
        }
        Component::Rhpf(filter) => {
            let mut params = vec![filter.qvalue];
            params.extend(scalar(&filter.cutoff));
            (Filter, params)
        }
        Component::Onepole(nodes::Onepole { param }) => (Filter, scalar(param)),
        Component::Pan(nodes::Pan { param }) => (Pan, scalar(param)),
        Component::MidiSynth(midi) => (Midi, vec![midi.attack, midi.release]),
        Component::MidiArp(midi) => (Midi, vec![midi.step_beats]),
        Component::Add(nodes::Add { param }) | Component::Mul(nodes::Mul { param }) => {
            (Other, scalar(param))
        }
        Component::ConstSig(value) => (Other, vec![value.value]),
        Component::Choose(_)
        | Component::Get(_)
        | Component::Meta(_)
        | Component::Expr(_)
        | Component::Eval(_) => (Other, vec![]),
    };
    parameters.retain(|value| value.is_finite());
    Node {
        kind,
        inputs: Vec::new(),
        parameters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{BLOCK, DEFAULT_BPM, engine};

    fn render_blocks(engine: &mut Engine<BLOCK>, count: usize) {
        for _ in 0..count {
            engine.next_block(Vec::new());
        }
    }

    fn example(code: &str) -> Graph {
        let mut engine = engine().unwrap();
        engine.update_with_code(code).unwrap();
        render_blocks(&mut engine, 4);
        let graph = capture(&engine, 4 * BLOCK, DEFAULT_BPM).unwrap();
        graph.validate().unwrap();
        graph
    }

    #[test]
    fn snapshots_exclude_inactive_chains_and_continuous_oscillators_have_no_hits() {
        let graph = example("~unused: seq 60 >> sp \\808bd;\nout: sin 220");
        assert_eq!(graph.nodes.len(), 2);
        assert!(graph.nodes.iter().any(|node| node.kind == NodeKind::Tone));
        assert!(graph.patterns.is_empty());
    }

    #[test]
    fn retained_sequencer_has_its_live_phase_and_renders_only_nonzero_slots() {
        let mut engine = engine().unwrap();
        let code = "out: speed 2 >> seq 60 0 _ 60 >> sp \\808bd";
        engine.update_with_code(code).unwrap();
        render_blocks(&mut engine, 4);
        let first = capture(&engine, 4 * BLOCK, DEFAULT_BPM).unwrap();
        let pattern = &first.patterns[0];
        assert_eq!(pattern.period_ns, 1_000_000_000);
        assert_eq!(pattern.phase_ns, 0);
        assert_eq!(pattern.hits, [0, 750_000_000]);
        assert_eq!(first.nodes[pattern.node as usize].kind, NodeKind::Sequence);

        engine.update_with_code(code).unwrap();
        render_blocks(&mut engine, 2);
        let updated = capture(&engine, 2 * BLOCK, DEFAULT_BPM).unwrap();
        assert_eq!(updated.patterns[0].phase_ns, frame_time(4 * BLOCK));
    }

    #[test]
    fn sample_patterns_preserve_independent_drum_accents() {
        let graph = example(include_str!("../assets/examples/pattern.glicol"));
        let hits = |kind| {
            &graph
                .patterns
                .iter()
                .find(|pattern| graph.nodes[pattern.node as usize].kind == kind)
                .unwrap()
                .hits
        };
        assert_eq!(hits(NodeKind::Kick), &[0, 1_000_000_000]);
        assert_eq!(hits(NodeKind::Snare), &[500_000_000, 1_500_000_000]);
        assert_eq!(
            hits(NodeKind::Hat),
            &[250_000_000, 750_000_000, 1_250_000_000, 1_750_000_000]
        );
        let mix = graph
            .nodes
            .iter()
            .find(|node| node.kind == NodeKind::Mix)
            .unwrap();
        assert_eq!(mix.inputs.len(), 3);
    }

    #[test]
    fn arrangement_connects_authored_sections_to_compiled_input_branches() {
        let graph = example(include_str!("../assets/examples/arrangement.glicol"));
        let arrangement = &graph.arrangements[0];
        let node = &graph.nodes[arrangement.node as usize];
        let [first, second] = arrangement.sections.as_slice() else {
            panic!("example arrangement has two sections");
        };
        assert_eq!(arrangement.phase_ns, 0);
        assert_eq!(arrangement.period_ns, 4_000_000_000);
        assert_eq!((first.start_ns, second.start_ns), (0, 2_000_000_000));
        assert_ne!(first.input, second.input);
        assert!(node.inputs.contains(&first.input));
        assert!(node.inputs.contains(&second.input));
    }

    #[test]
    fn built_in_patches_export_valid_snapshots() {
        for (name, patch) in crate::examples::PATCHES {
            let graph = example(patch);
            assert!(
                graph.nodes.iter().any(|node| node.kind == NodeKind::Output),
                "{name}"
            );
        }
    }
}

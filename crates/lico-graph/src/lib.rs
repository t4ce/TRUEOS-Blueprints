use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const DIRECTORY: &str = "/apps/common/lico-dance-v1";
pub const LEASE_REFRESH_NS: u64 = 2_000_000_000;
pub const LEASE_TTL_NS: u64 = 6_000_000_000;
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
pub const MAX_LEASE_BYTES: usize = 256;

/// Each Lico instance publishes its immutable graph before its first lease.
pub fn paths(instance: &str) -> Option<(String, String)> {
    let valid = !instance.is_empty()
        && instance.len() <= 100
        && instance
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    valid.then(|| {
        (
            format!("{DIRECTORY}/{instance}.json"),
            format!("{DIRECTORY}/{instance}.lease"),
        )
    })
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub patterns: Vec<Pattern>,
    pub arrangements: Vec<Arrangement>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub kind: NodeKind,
    pub inputs: Vec<u32>,
    pub parameters: Vec<f32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Input,
    Output,
    Tone,
    Noise,
    Kick,
    Snare,
    Hat,
    Sample,
    Sequence,
    Impulse,
    Arrangement,
    Envelope,
    Modulation,
    Delay,
    Space,
    Mix,
    Filter,
    Pan,
    Midi,
    Other,
}

/// A repeating pattern with hits measured from the beginning of its cycle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub node: u32,
    pub period_ns: u64,
    /// Position within the cycle at the snapshot's start timestamp.
    pub phase_ns: u64,
    pub hits: Vec<u64>,
}

/// Sections in an arrangement run until the next section or the end of the period.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Arrangement {
    pub node: u32,
    pub period_ns: u64,
    /// Position within the cycle at the snapshot's start timestamp.
    pub phase_ns: u64,
    pub sections: Vec<Section>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// Graph index of the selected input node.
    pub input: u32,
    pub start_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DanceClueSnapshot {
    pub version: u32,
    pub run: u64,
    /// Shared monotonic timestamp when Lico began this run.
    pub started_ns: u64,
    pub graph: Graph,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    pub run: u64,
    /// Shared monotonic timestamp when the producer last signaled an active run.
    pub refreshed_ns: u64,
}

impl Lease {
    pub fn is_live(self, now_ns: u64) -> bool {
        self.run != 0
            && now_ns
                .checked_sub(self.refreshed_ns)
                .is_some_and(|age| age < LEASE_TTL_NS)
    }
}

impl DanceClueSnapshot {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != VERSION {
            return Err("unsupported Lico graph version");
        }
        if self.run == 0 {
            return Err("invalid Lico run ID");
        }
        self.graph.validate()
    }
}

impl Graph {
    pub fn validate(&self) -> Result<(), &'static str> {
        const MAX_NODES: usize = 4096;
        const MAX_VALUES: usize = 65_536;

        if self.nodes.len() > MAX_NODES
            || self.patterns.len() > MAX_NODES
            || self.arrangements.len() > MAX_NODES
        {
            return Err("too many Lico graph entries");
        }

        let has_node = |index: u32| (index as usize) < self.nodes.len();
        let mut values = 0usize;
        for node in &self.nodes {
            values = values
                .saturating_add(node.inputs.len())
                .saturating_add(node.parameters.len());
            if values > MAX_VALUES {
                return Err("too many Lico graph values");
            }
            if !node.inputs.iter().copied().all(has_node)
                || node.parameters.iter().any(|p| !p.is_finite())
            {
                return Err("invalid Lico graph node");
            }
        }
        for pattern in &self.patterns {
            values = values.saturating_add(pattern.hits.len());
            if values > MAX_VALUES {
                return Err("too many Lico graph values");
            }
            if !has_node(pattern.node)
                || pattern.period_ns == 0
                || pattern.phase_ns >= pattern.period_ns
                || pattern.hits.iter().any(|&hit| hit >= pattern.period_ns)
            {
                return Err("invalid Lico graph pattern");
            }
        }
        for arrangement in &self.arrangements {
            values = values.saturating_add(arrangement.sections.len());
            if values > MAX_VALUES {
                return Err("too many Lico graph values");
            }
            if !has_node(arrangement.node)
                || arrangement.period_ns == 0
                || arrangement.phase_ns >= arrangement.period_ns
                || arrangement.sections.iter().any(|section| {
                    !has_node(section.input) || section.start_ns >= arrangement.period_ns
                })
                || arrangement
                    .sections
                    .windows(2)
                    .any(|pair| pair[0].start_ns >= pair[1].start_ns)
            {
                return Err("invalid Lico graph arrangement");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_expires_and_rejects_future_or_stopped_runs() {
        let lease = Lease {
            run: 1,
            refreshed_ns: 10,
        };
        assert!(lease.is_live(10));
        assert!(!lease.is_live(9));
        assert!(lease.is_live(10 + LEASE_TTL_NS - 1));
        assert!(!lease.is_live(10 + LEASE_TTL_NS));
        assert!(!Lease { run: 0, ..lease }.is_live(10));
    }

    #[test]
    fn instance_names_stay_in_the_shared_directory() {
        assert_eq!(
            paths("Lico-42").unwrap().1,
            format!("{DIRECTORY}/Lico-42.lease")
        );
        assert!(paths("../Lico-42").is_none());
        assert!(paths("").is_none());
    }

    #[test]
    fn graph_rejects_out_of_range_edges_and_timing() {
        let mut graph = Graph {
            nodes: vec![Node {
                kind: NodeKind::Sequence,
                inputs: vec![],
                parameters: vec![],
            }],
            patterns: vec![Pattern {
                node: 0,
                period_ns: 100,
                phase_ns: 0,
                hits: vec![0, 10],
            }],
            arrangements: vec![],
        };
        assert!(graph.validate().is_ok());
        graph.nodes[0].inputs.push(1);
        assert!(graph.validate().is_err());
        graph.nodes[0].inputs.clear();
        graph.patterns[0].hits.push(100);
        assert!(graph.validate().is_err());
    }
}

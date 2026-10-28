//! CPU preparation for the existing, unmodified Maxpix native shader.
//! PrismQ runs only at scene/stage selection. Born probabilities allocate
//! particles to geometric bins; algorithm phases shape the new filaments.
//! These are mathematical artworks, not orbital simulations.

use alloc::{format, string::String, vec::Vec};
use core::f32::consts::TAU;
use libm::{cosf, sinf, sqrtf};
use prism_q::{BackendKind, circuit::openqasm, simulate};

use crate::circuits::{self, BasisSample, CircuitPreset, GROVER_TARGET};
use crate::{VERTEX_STRIDE, particle_mesh};

const QUBITS: usize = 8;
const STATES: usize = 1 << QUBITS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneMode {
    Tornado,
    Trefoil,
    Hopf,
    Fourier,
    Grover,
    Walk,
    Entanglement,
}

impl SceneMode {
    pub fn from_key(codepoint: u32) -> Option<Self> {
        match codepoint {
            48 => Some(Self::Tornado),      // 0
            50 => Some(Self::Trefoil),      // 2
            51 => Some(Self::Hopf),         // 3
            52 => Some(Self::Fourier),      // 4
            53 => Some(Self::Grover),       // 5
            54 => Some(Self::Walk),         // 6
            55 => Some(Self::Entanglement), // 7
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Tornado => "tornado",
            Self::Trefoil => "quantum-trefoil",
            Self::Hopf => "quantum-hopf",
            Self::Fourier => "fourier-bloom",
            Self::Grover => "grover-beacon",
            Self::Walk => "quantum-walk",
            Self::Entanglement => "bell-weave-20q",
        }
    }

    pub fn qubits(self) -> usize {
        match self {
            Self::Tornado => 0,
            Self::Entanglement => 20,
            _ => 8,
        }
    }

    pub fn default_step(self) -> u32 {
        match self {
            Self::Grover => 6,
            Self::Walk => 16,
            _ => 0,
        }
    }

    pub fn max_step(self) -> u32 {
        match self {
            Self::Fourier | Self::Entanglement => 15,
            Self::Grover => 12,
            Self::Walk => 32,
            _ => 0,
        }
    }

    /// Bounded native motion keeps the seed height away from fract()'s wrap.
    /// It also keeps the mathematical shapes readable while they turn.
    pub fn shader_time(self, seconds: f32) -> f32 {
        match self {
            Self::Tornado => seconds,
            _ => sinf(seconds * 0.35) * 0.45,
        }
    }

    pub fn rotation(self, seconds: f32, steering: f32) -> [f32; 4] {
        match self {
            Self::Tornado => [0.0, 0.0, 0.0, 1.0],
            _ => {
                let yaw = trueos_picasso::cam::Quaternion::from_axis_angle(
                    [0.0, 1.0, 0.0],
                    seconds * 0.16 + steering,
                );
                let tilt = trueos_picasso::cam::Quaternion::from_axis_angle(
                    [1.0, 0.0, 0.0],
                    sinf(seconds * 0.13) * 0.35,
                );
                (yaw * tilt).normalized().0
            }
        }
    }
}

/// Small entangled interferometer. Disabling PrismQ's parallel feature keeps
/// this 256-amplitude workload on the calling thread, including on TRUEOS.
fn probabilities(mode: SceneMode) -> Result<Vec<f64>, String> {
    let mut qasm = String::from("OPENQASM 3.0;\ninclude \"stdgates.inc\";\nqubit[8] q;\n");
    for q in 0..QUBITS {
        qasm.push_str(&format!("ry({}) q[{q}];\n", 1.1 + q as f64 * 0.09));
    }
    for q in 0..QUBITS {
        qasm.push_str(&format!("cx q[{q}], q[{}];\n", (q + 1) % QUBITS));
    }
    let phase = if mode == SceneMode::Trefoil {
        0.47
    } else {
        1.13
    };
    for q in 0..QUBITS {
        qasm.push_str(&format!(
            "rz({}) q[{q}];\nh q[{q}];\n",
            phase * (q + 1) as f64
        ));
    }
    let circuit = openqasm::parse(&qasm).map_err(|err| format!("quantum scene parse: {err}"))?;
    let result = simulate(&circuit)
        .backend(BackendKind::Statevector)
        .seed(0)
        .run()
        .map_err(|err| format!("quantum scene simulation: {err}"))?;
    let distribution = result
        .probabilities
        .ok_or_else(|| String::from("missing quantum probabilities"))?;
    Ok(distribution.iter().collect())
}

/// Systematic quantiles: each bin's particle count differs from N*p by < 1.
/// Skips zero-probability bins and always produces exactly `count` points.
fn population(probabilities: &[f64], count: u32) -> Vec<u32> {
    let total: f64 = probabilities.iter().sum();
    let mut counts = alloc::vec![0; probabilities.len()];
    let mut bin = 0;
    let mut cumulative = probabilities[0];
    for point in 0..count {
        let quantile = (f64::from(point) + 0.5) * total / f64::from(count);
        while bin + 1 < probabilities.len() && quantile >= cumulative {
            bin += 1;
            cumulative += probabilities[bin];
        }
        counts[bin] += 1;
    }
    counts
}

pub fn scene_mesh(mode: SceneMode, count: u32) -> Result<(Vec<u8>, Vec<u8>), String> {
    scene_mesh_at(mode, count, mode.default_step())
}

pub fn scene_mesh_at(mode: SceneMode, count: u32, step: u32) -> Result<(Vec<u8>, Vec<u8>), String> {
    if mode == SceneMode::Tornado {
        return Ok(particle_mesh(count));
    }
    let step = step.min(mode.max_step());
    let preset = match mode {
        SceneMode::Fourier => Some(CircuitPreset::Fourier),
        SceneMode::Grover => Some(CircuitPreset::Grover),
        SceneMode::Walk => Some(CircuitPreset::Walk),
        SceneMode::Entanglement => Some(CircuitPreset::Entanglement),
        _ => None,
    };
    let samples = if let Some(preset) = preset {
        let snapshot = circuits::snapshot(preset, step)?;
        debug_assert_eq!(snapshot.qubits, mode.qubits());
        snapshot.samples
    } else {
        probabilities(mode)?
            .into_iter()
            .enumerate()
            .map(|(index, probability)| BasisSample {
                index,
                probability,
                phase: 0.0,
            })
            .collect()
    };
    Ok(quantum_mesh(mode, count, &samples, step))
}

fn quantum_mesh(
    mode: SceneMode,
    count: u32,
    samples: &[BasisSample],
    step: u32,
) -> (Vec<u8>, Vec<u8>) {
    let probabilities: Vec<f64> = samples.iter().map(|sample| sample.probability).collect();
    let counts = population(&probabilities, count);
    let mut vertices = Vec::with_capacity(count as usize * VERTEX_STRIDE);
    let mut indices = Vec::with_capacity(count as usize * 4);
    let mut rng = 0x5052_4953u32;
    let mut unit = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        (rng >> 8) as f32 / 16_777_216.0
    };
    for (sample, &points) in samples.iter().zip(&counts) {
        let bin = sample.index;
        // Remove a common global phase: only relative phase may affect the
        // artwork. The legacy scenes use zero phase and keep their payload.
        let relative_phase = sample.phase - samples[0].phase;
        for _ in 0..points {
            let desired = match mode {
                SceneMode::Trefoil => {
                    let t = TAU * (bin as f32 + unit()) / STATES as f32;
                    // Thickened (2,3) torus knot, sampled around its tube.
                    let braid = TAU * unit();
                    let tube = 0.12 + 0.28 * sqrtf(unit());
                    let radius = 3.1 + 1.3 * cosf(3.0 * t) + tube * cosf(braid);
                    [
                        radius * cosf(2.0 * t),
                        1.8 * sinf(3.0 * t) + tube * sinf(braid),
                        radius * sinf(2.0 * t),
                    ]
                }
                SceneMode::Hopf => {
                    // 256 linked fibers: inverse Hopf map in S^3, followed by
                    // stereographic projection (x1,x2,x3)/(1-x4). Restrict the
                    // base latitude to keep the projection away from its pole.
                    let latitude = 0.65 + 1.2 * ((bin / 16) as f32 + 0.5) / 16.0;
                    let longitude = TAU * (bin % 16) as f32 / 16.0;
                    let t = TAU * unit();
                    let a = cosf(latitude * 0.5);
                    let b = sinf(latitude * 0.5);
                    let denominator = 1.0 - b * sinf(t + longitude);
                    let scale = 2.0 / denominator;
                    let fuzz = 0.025;
                    [
                        a * cosf(t) * scale + (unit() - 0.5) * fuzz,
                        b * cosf(t + longitude) * scale + (unit() - 0.5) * fuzz,
                        a * sinf(t) * scale + (unit() - 0.5) * fuzz,
                    ]
                }
                SceneMode::Fourier => {
                    // A complete petal per occupied Fourier frequency. Phase
                    // controls its curl; Born weight controls its population.
                    let azimuth = TAU * bin as f32 / 256.0;
                    let t = TAU * unit();
                    let radius = 0.35 + 4.7 * sinf(t * 0.5);
                    let curl = azimuth + 0.4 * sinf(t + relative_phase);
                    [
                        radius * cosf(curl) + (unit() - 0.5) * 0.025,
                        1.7 * cosf(t) + 0.35 * sinf(3.0 * t + relative_phase),
                        radius * sinf(curl) + (unit() - 0.5) * 0.025,
                    ]
                }
                SceneMode::Grover => {
                    let t = TAU * unit();
                    if bin == GROVER_TARGET {
                        // The searched-for state illuminates three tall
                        // interlocked orbits, rather than an opaque point.
                        let orbit = (unit() * 3.0) as u32;
                        let angle = orbit as f32 * TAU / 3.0;
                        let radius = 1.15 * cosf(t);
                        [radius * cosf(angle), 3.0 * sinf(t), radius * sinf(angle)]
                    } else {
                        let angle = TAU * bin as f32 / 256.0;
                        let radius = 4.0 + 0.32 * cosf(t);
                        [
                            radius * cosf(angle),
                            1.15 * sinf(3.0 * angle + relative_phase) + 0.32 * sinf(t),
                            radius * sinf(angle),
                        ]
                    }
                }
                SceneMode::Walk => {
                    // Seven position bits and one coin bit. Each occupied
                    // site gets a whole thin loop; dark sites stay dark.
                    let position = bin & 127;
                    let signed = position as f32 - 64.0;
                    let site = signed / step.max(1) as f32;
                    let t = TAU * unit();
                    let coin = (bin >> 7) as f32;
                    let radius = 0.7 + 1.25 * sqrtf((1.0 - site * site).max(0.0));
                    [
                        4.7 * site + 0.08 * cosf(3.0 * t + relative_phase),
                        radius * cosf(t) + (coin - 0.5) * 0.18,
                        radius * sinf(t) * (0.7 + coin * 0.3),
                    ]
                }
                SceneMode::Entanglement => {
                    // Project the correlated 10-bit register outcomes onto
                    // 32 visible channels. Four bits select the endpoints;
                    // the fifth selects one or two full helical turns. This
                    // keeps all 1024 outcomes from filling a solid surface.
                    let left = bin & 1023;
                    let right = bin >> 10;
                    let t = unit();
                    let a = TAU * (left & 15) as f32 / 16.0;
                    let b = TAU * (right & 15) as f32 / 16.0;
                    let turns = 1.0 + ((right >> 4) & 1) as f32;
                    let angle = a * (1.0 - t)
                        + b * t
                        + TAU * turns * t
                        + 0.06 * sinf(core::f32::consts::PI * t) * sinf(relative_phase);
                    let waist = cosf(core::f32::consts::PI * t);
                    let radius = 0.45 + 1.6 * waist * waist;
                    [(t - 0.5) * 8.2, radius * cosf(angle), radius * sinf(angle)]
                }
                SceneMode::Tornado => unreachable!(),
            };
            let phase = TAU * unit();
            let height = 0.48 + 0.04 * unit();
            let radial = 0.08 + 0.04 * unit();
            let radius = (0.75 + 4.75 * height * height) * radial;
            let angle = phase + height * 9.0;
            // Cancel the native tornado position at t=0 using its existing
            // second Float3. Shader motion then adds a small orbit/ripple.
            let row = [
                phase,
                height,
                radial,
                desired[0] - cosf(angle) * radius,
                desired[1] - (height * 8.8 - 4.4),
                desired[2] - sinf(angle) * radius,
            ];
            let index = indices.len() as u32 / 4;
            for value in row {
                vertices.extend_from_slice(&value.to_le_bytes());
            }
            indices.extend_from_slice(&index.to_le_bytes());
        }
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn born_populations_preserve_mass_and_dark_bins() {
        let p = [0.0, 0.125, 0.0, 0.375, 0.5, 0.0];
        let counts = population(&p, 10_003);
        assert_eq!(counts.iter().sum::<u32>(), 10_003);
        for (&actual, expected) in counts.iter().zip(p) {
            assert!((f64::from(actual) - expected * 10_003.0).abs() < 1.0);
            if expected == 0.0 {
                assert_eq!(actual, 0);
            }
        }
        assert_eq!(population(&p, 0), alloc::vec![0; p.len()]);
    }

    #[test]
    fn interferometers_produce_distinct_normalized_nonuniform_distributions() {
        let knot = probabilities(SceneMode::Trefoil).unwrap();
        let hopf = probabilities(SceneMode::Hopf).unwrap();
        for p in [&knot, &hopf] {
            assert_eq!(p.len(), STATES);
            assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-10);
            assert!(p.iter().all(|v| v.is_finite() && *v >= 0.0));
            assert!(p.iter().copied().fold(0.0, f64::max) > 2.0 / STATES as f64);
        }
        assert!(
            knot.iter()
                .zip(hopf)
                .map(|(a, b)| (a - b).abs())
                .sum::<f64>()
                > 0.1
        );
    }

    #[test]
    fn a_common_global_phase_does_not_move_algorithm_filaments() {
        let samples = [
            BasisSample {
                index: 16,
                probability: 0.4,
                phase: 0.25,
            },
            BasisSample {
                index: 80,
                probability: 0.6,
                phase: -0.5,
            },
        ];
        let shifted: Vec<BasisSample> = samples
            .iter()
            .map(|sample| BasisSample {
                phase: sample.phase + 1.25,
                ..*sample
            })
            .collect();
        assert_eq!(
            quantum_mesh(SceneMode::Fourier, 2048, &samples, 0),
            quantum_mesh(SceneMode::Fourier, 2048, &shifted, 0),
        );
    }

    #[test]
    fn native_payload_stays_inside_bounds_through_animation() {
        for mode in [SceneMode::Trefoil, SceneMode::Hopf] {
            let (vertices, indices) = scene_mesh(mode, 8192).unwrap();
            assert_eq!(vertices.len(), 8192 * VERTEX_STRIDE);
            assert_eq!(indices.len(), 8192 * 4);
            assert_eq!(
                (vertices.clone(), indices.clone()),
                scene_mesh(mode, 8192).unwrap()
            );
            for (index, row) in vertices.chunks_exact(VERTEX_STRIDE).enumerate() {
                let v: Vec<f32> = row
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
                assert!(v.iter().all(|v| v.is_finite()));
                assert_eq!(
                    u32::from_le_bytes(indices[index * 4..index * 4 + 4].try_into().unwrap()),
                    index as u32
                );
                for seconds in [0.0, 4.5, 13.0, 120.0] {
                    let time = mode.shader_time(seconds);
                    let h = v[1] + time * 0.1;
                    assert!((0.0..1.0).contains(&h));
                    let r = (0.75 + 4.75 * h * h) * v[2];
                    let angle = v[0] + time * (2.3 - 1.2 * h) + h * 9.0;
                    let point = [
                        cosf(angle) * r + v[3],
                        h * 8.8 - 4.4 + v[4],
                        sinf(angle) * r + v[5],
                    ];
                    assert!(point.iter().map(|v| v * v).sum::<f32>() < 10.0 * 10.0);
                }
            }
        }
    }

    #[test]
    fn algorithm_filaments_are_deterministic_and_fit_the_native_animation_bounds() {
        for (mode, preset) in [
            (SceneMode::Fourier, CircuitPreset::Fourier),
            (SceneMode::Grover, CircuitPreset::Grover),
            (SceneMode::Walk, CircuitPreset::Walk),
            (SceneMode::Entanglement, CircuitPreset::Entanglement),
        ] {
            let step = mode.default_step();
            let snapshot = circuits::snapshot(preset, step).unwrap();
            let (vertices, indices) = quantum_mesh(mode, 8192, &snapshot.samples, step);
            assert_eq!(vertices.len(), 8192 * VERTEX_STRIDE);
            assert_eq!(indices.len(), 8192 * 4);
            assert_eq!(
                (vertices.clone(), indices.clone()),
                quantum_mesh(mode, 8192, &snapshot.samples, step)
            );
            for (index, row) in vertices.chunks_exact(VERTEX_STRIDE).enumerate() {
                let v: Vec<f32> = row
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
                assert!(v.iter().all(|v| v.is_finite()));
                assert_eq!(
                    u32::from_le_bytes(indices[index * 4..index * 4 + 4].try_into().unwrap()),
                    index as u32
                );
                // Includes the extrema of bounded shader time, and +/-PI
                // steering from the physical pointer transport contract.
                for time in [-0.45, 0.0, 0.45] {
                    let h = v[1] + time * 0.1;
                    assert!((0.0..1.0).contains(&h));
                    let radius = (0.75 + 4.75 * h * h) * v[2];
                    for steering in [-core::f32::consts::PI, 0.0, core::f32::consts::PI] {
                        let angle = v[0] + steering + time * (2.3 - 1.2 * h) + h * 9.0;
                        let p = [
                            cosf(angle) * radius + v[3],
                            h * 8.8 - 4.4 + v[4],
                            sinf(angle) * radius + v[5],
                        ];
                        assert!(
                            p.iter().map(|v| v * v).sum::<f32>() < 7.0 * 7.0,
                            "{} exceeds visual bounds: {p:?}",
                            mode.name()
                        );
                    }
                }
            }
        }
    }
}

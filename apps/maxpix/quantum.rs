//! CPU preparation for the existing, unmodified Maxpix native shader.
//! PrismQ runs only at scene selection. Born probabilities allocate particles
//! to geometric bins; these are mathematical artworks, not orbital simulations.

use alloc::{format, string::String, vec::Vec};
use core::f32::consts::TAU;
use libm::{cosf, sinf, sqrtf};
use prism_q::{BackendKind, circuit::openqasm, simulate};

use crate::{VERTEX_STRIDE, particle_mesh};

const QUBITS: usize = 8;
const STATES: usize = 1 << QUBITS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneMode {
    Tornado,
    Trefoil,
    Hopf,
}

impl SceneMode {
    pub fn from_key(codepoint: u32) -> Option<Self> {
        match codepoint {
            48 => Some(Self::Tornado), // 0
            50 => Some(Self::Trefoil), // 2
            51 => Some(Self::Hopf),    // 3
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Tornado => "tornado",
            Self::Trefoil => "quantum-trefoil",
            Self::Hopf => "quantum-hopf",
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
    if mode == SceneMode::Tornado {
        return Ok(particle_mesh(count));
    }
    let probabilities = probabilities(mode)?;
    Ok(quantum_mesh(mode, count, &probabilities))
}

fn quantum_mesh(mode: SceneMode, count: u32, probabilities: &[f64]) -> (Vec<u8>, Vec<u8>) {
    let counts = population(probabilities, count);
    let mut vertices = Vec::with_capacity(count as usize * VERTEX_STRIDE);
    let mut indices = Vec::with_capacity(count as usize * 4);
    let mut rng = 0x5052_4953u32;
    let mut unit = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        (rng >> 8) as f32 / 16_777_216.0
    };
    for (bin, &points) in counts.iter().enumerate() {
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
}

//! Deterministic circuit snapshots for Maxpix's CPU-prepared particle scenes.
//!
//! These run PrismQ gates, retain both Born weights and complex phases, and
//! never measure the state. The existing native shader consumes only geometry.

use alloc::{format, string::String, vec::Vec};
use core::f64::consts::{FRAC_PI_2, PI};
use libm::{atan2, sin};
use prism_q::{CircuitBuilder, Gate, StatevectorBackend, backend::Backend};

const SMALL_QUBITS: usize = 8;
const POSITION_QUBITS: usize = 7;
const REGISTER_QUBITS: usize = 10;
const PROBABILITY_EPSILON: f64 = 1e-18;

/// A nontrivial eight-bit oracle, rather than a single-qubit phase flip.
pub(crate) const GROVER_TARGET: usize = 0xA5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CircuitPreset {
    Fourier,
    Grover,
    Walk,
    Entanglement,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BasisSample {
    pub index: usize,
    pub probability: f64,
    pub phase: f32,
}

#[derive(Debug)]
pub(crate) struct CircuitSnapshot {
    pub qubits: usize,
    pub samples: Vec<BasisSample>,
}

/// `step` changes the coherent input for Fourier and entanglement, selects
/// Grover's iteration count modulo 13, and selects a walk length modulo 65.
/// Construction is bounded even if a caller keeps advancing the stage counter.
pub(crate) fn snapshot(preset: CircuitPreset, step: u32) -> Result<CircuitSnapshot, String> {
    let builder = match preset {
        CircuitPreset::Fourier => fourier(step),
        CircuitPreset::Grover => grover((step % 13) as usize),
        CircuitPreset::Walk => walk((step % 65) as usize),
        CircuitPreset::Entanglement => entanglement(step),
    };
    run(&builder)
}

fn run(builder: &CircuitBuilder) -> Result<CircuitSnapshot, String> {
    // Explicit backend and seed avoid automatic decomposition, measurement,
    // runtime randomness, and statevector export's second dense allocation.
    // PrismQ's parallel feature is disabled in Maxpix's manifest.
    let mut backend = StatevectorBackend::new(0);
    let qubits = builder.circuit().num_qubits;
    backend
        .init(qubits, 0)
        .and_then(|()| backend.apply_instructions(&builder.circuit().instructions))
        .map_err(|err| format!("PrismQ scene circuit: {err}"))?;
    let samples = backend
        .state_vector()
        .iter()
        .enumerate()
        .filter_map(|(index, amplitude)| {
            let probability = amplitude.norm_sqr();
            (probability > PROBABILITY_EPSILON).then(|| BasisSample {
                index,
                probability,
                phase: atan2(amplitude.im, amplitude.re) as f32,
            })
        })
        .collect();
    Ok(CircuitSnapshot { qubits, samples })
}

/// Positive-sign DFT in PrismQ's little-endian basis order. For each high
/// qubit, lower-bit controls use pi/2^distance, followed by bit reversal.
fn qft(builder: &mut CircuitBuilder, qubits: usize) {
    for target in (0..qubits).rev() {
        builder.h(target);
        for control in (0..target).rev() {
            let angle = PI / (1usize << (target - control)) as f64;
            builder.cphase(angle, control, target);
        }
    }
    for low in 0..qubits / 2 {
        builder.swap(low, qubits - low - 1);
    }
}

fn fourier(step: u32) -> CircuitBuilder {
    let mut builder = CircuitBuilder::new(SMALL_QUBITS);
    let stage = (step % 64) as f64;
    // Four coherent residue classes, x = 4 + a + 2b (mod 16). The high
    // register supplies sixteen repeated periods; unequal rotations and
    // relative phases make their sixteen Fourier peaks interfere unevenly.
    builder
        .ry(1.23 + 0.2 * sin(stage * 0.31), 0)
        .p(0.61 + stage * 0.19, 0)
        .ry(1.81 + 0.2 * sin(stage * 0.23), 1)
        .p(-0.93 + stage * 0.13, 1)
        .x(2);
    for qubit in 4..SMALL_QUBITS {
        builder.h(qubit);
    }
    qft(&mut builder, SMALL_QUBITS);
    builder
}

/// Apply -1 only to |11...1>, using a genuine multi-controlled Z.
fn phase_all_ones(builder: &mut CircuitBuilder) {
    let controls = [0, 1, 2, 3, 4, 5, 6];
    builder.mcu(Gate::Z.matrix_2x2(), &controls, SMALL_QUBITS - 1);
}

fn grover(iterations: usize) -> CircuitBuilder {
    let mut builder = CircuitBuilder::new(SMALL_QUBITS);
    for qubit in 0..SMALL_QUBITS {
        builder.h(qubit);
    }
    for _ in 0..iterations {
        // Map the marked eight-bit word to all ones for the phase oracle.
        for qubit in 0..SMALL_QUBITS {
            if GROVER_TARGET & (1 << qubit) == 0 {
                builder.x(qubit);
            }
        }
        phase_all_ones(&mut builder);
        for qubit in 0..SMALL_QUBITS {
            if GROVER_TARGET & (1 << qubit) == 0 {
                builder.x(qubit);
            }
        }
        // Inversion about the mean, up to an irrelevant global -1 phase.
        for qubit in 0..SMALL_QUBITS {
            builder.h(qubit).x(qubit);
        }
        phase_all_ones(&mut builder);
        for qubit in 0..SMALL_QUBITS {
            builder.x(qubit).h(qubit);
        }
    }
    builder
}

/// Increment the seven-bit position modulo 128 iff the coin is |1>.
fn controlled_increment(builder: &mut CircuitBuilder) {
    let coin = POSITION_QUBITS;
    let mut controls = [0; POSITION_QUBITS];
    controls[0] = coin;
    // Apply high carries before changing the low bits on which they depend.
    for target in (1..POSITION_QUBITS).rev() {
        for (slot, lower) in controls[1..=target].iter_mut().zip(0..target) {
            *slot = lower;
        }
        builder.mcu(Gate::X.matrix_2x2(), &controls[..=target], target);
    }
    builder.cx(coin, 0);
}

fn walk(steps: usize) -> CircuitBuilder {
    let mut builder = CircuitBuilder::new(POSITION_QUBITS + 1);
    let coin = POSITION_QUBITS;
    // Center the walker at 64. The imaginary balanced coin produces the
    // symmetric Hadamard-walk distribution, rather than classical diffusion.
    builder
        .x(POSITION_QUBITS - 1)
        .ry(FRAC_PI_2, coin)
        .p(FRAC_PI_2, coin);
    for _ in 0..steps {
        builder.h(coin);
        // coin=0: x -> x+1; coin=1: x -> x-1. Complementing the position
        // conjugates an increment into a decrement without an extra ancilla.
        builder.x(coin);
        controlled_increment(&mut builder);
        builder.x(coin);
        for qubit in 0..POSITION_QUBITS {
            builder.x(qubit);
        }
        controlled_increment(&mut builder);
        for qubit in 0..POSITION_QUBITS {
            builder.x(qubit);
        }
    }
    builder
}

fn entanglement(step: u32) -> CircuitBuilder {
    let mut builder = CircuitBuilder::new(REGISTER_QUBITS * 2);
    let stage = (step % 64) as f64;
    // Generalized Bell pairs: sum_x a_x |x>_A |x>_B. Ring CZ edges couple
    // their phases without breaking the exact A=B correlation. This really
    // evolves a 20-qubit / 1,048,576-amplitude state, but only 1024 amplitudes
    // can be nonzero, so the exported geometry data remains small.
    for qubit in 0..REGISTER_QUBITS {
        let angle = FRAC_PI_2 + 0.35 * sin(qubit as f64 * 0.77 + stage * 0.29);
        builder
            .ry(angle, qubit)
            .p(0.23 * (qubit + 1) as f64 + stage * 0.11, qubit)
            .cx(qubit, qubit + REGISTER_QUBITS);
    }
    for qubit in 0..REGISTER_QUBITS {
        builder.cz(qubit, (qubit + 1) % REGISTER_QUBITS);
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::TAU;
    use libm::{asin, cos};

    fn probability(snapshot: &CircuitSnapshot, index: usize) -> f64 {
        snapshot
            .samples
            .iter()
            .find(|sample| sample.index == index)
            .map_or(0.0, |sample| sample.probability)
    }

    fn assert_normalized(snapshot: &CircuitSnapshot) {
        let total: f64 = snapshot.samples.iter().map(|s| s.probability).sum();
        assert!((total - 1.0).abs() < 1e-10, "total probability {total}");
        assert!(snapshot.samples.iter().all(|sample| {
            sample.index < 1 << snapshot.qubits
                && sample.probability.is_finite()
                && sample.probability > 0.0
                && sample.phase.is_finite()
        }));
    }

    #[test]
    fn fourier_comb_has_known_spectral_peaks_and_phases() {
        let mut builder = CircuitBuilder::new(SMALL_QUBITS);
        builder.x(0).x(1); // x = 3 (mod 16)
        for qubit in 4..SMALL_QUBITS {
            builder.h(qubit);
        }
        qft(&mut builder, SMALL_QUBITS);
        let snapshot = run(&builder).unwrap();
        assert_normalized(&snapshot);
        assert_eq!(snapshot.samples.len(), 16);
        for sample in snapshot.samples {
            assert_eq!(sample.index % 16, 0);
            assert!((sample.probability - 1.0 / 16.0).abs() < 1e-12);
            let phase = TAU * 3.0 * sample.index as f64 / 256.0;
            assert!((cos(sample.phase as f64) - cos(phase)).abs() < 2e-7);
            assert!((sin(sample.phase as f64) - sin(phase)).abs() < 2e-7);
        }
    }

    #[test]
    fn fourier_preset_interferes_with_nonuniform_weights_and_complex_phases() {
        let first = snapshot(CircuitPreset::Fourier, 0).unwrap();
        let next = snapshot(CircuitPreset::Fourier, 1).unwrap();
        for snapshot in [&first, &next] {
            assert_normalized(snapshot);
            assert_eq!(snapshot.qubits, 8);
            assert_eq!(snapshot.samples.len(), 16);
            assert!(snapshot.samples.iter().all(|sample| sample.index % 16 == 0));
            assert!(
                snapshot
                    .samples
                    .iter()
                    .any(|sample| sample.probability > 0.12)
            );
            assert!(
                snapshot
                    .samples
                    .iter()
                    .any(|sample| sample.probability < 0.02)
            );
            assert!(
                snapshot
                    .samples
                    .iter()
                    .any(|sample| sin(sample.phase as f64).abs() > 0.5)
            );
        }
        let distance: f64 = first
            .samples
            .iter()
            .zip(&next.samples)
            .map(|(a, b)| (a.probability - b.probability).abs())
            .sum();
        assert!(distance > 0.02);
    }

    #[test]
    fn grover_amplifies_the_actual_eight_bit_target_by_the_theoretical_amount() {
        let theta = asin(1.0 / 16.0);
        for iterations in [0, 1, 6, 12] {
            let snapshot = snapshot(CircuitPreset::Grover, iterations).unwrap();
            assert_normalized(&snapshot);
            let expected = sin((2 * iterations + 1) as f64 * theta).powi(2);
            assert!((probability(&snapshot, GROVER_TARGET) - expected).abs() < 1e-10);
            let background = (1.0 - expected) / 255.0;
            for sample in &snapshot.samples {
                if sample.index != GROVER_TARGET {
                    assert!((sample.probability - background).abs() < 1e-10);
                }
            }
        }
        assert!(probability(&snapshot(CircuitPreset::Grover, 12).unwrap(), GROVER_TARGET) > 0.99);
    }

    #[test]
    fn coined_walk_is_normalized_symmetric_and_obeys_support_parity() {
        for steps in [0, 1, 2, 8, 21, 64] {
            let snapshot = snapshot(CircuitPreset::Walk, steps).unwrap();
            assert_normalized(&snapshot);
            let mut position = [0.0; 128];
            for sample in snapshot.samples {
                let index = sample.index & 127;
                assert_eq!(index % 2, steps as usize % 2);
                let displacement = index.abs_diff(64).min(128 - index.abs_diff(64));
                assert!(displacement <= steps as usize);
                position[index] += sample.probability;
            }
            for index in 0..128 {
                assert!((position[index] - position[(128 - index) % 128]).abs() < 1e-10);
            }
            if steps == 2 {
                assert!((position[62] - 0.25).abs() < 1e-10);
                assert!((position[64] - 0.5).abs() < 1e-10);
                assert!((position[66] - 0.25).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn twenty_qubit_registers_have_exact_bell_correlations_and_sparse_output() {
        let first = snapshot(CircuitPreset::Entanglement, 0).unwrap();
        let next = snapshot(CircuitPreset::Entanglement, 1).unwrap();
        for snapshot in [&first, &next] {
            assert_normalized(snapshot);
            assert_eq!(snapshot.qubits, 20);
            assert_eq!(snapshot.samples.len(), 1024);
            for sample in &snapshot.samples {
                assert_eq!(sample.index & 1023, sample.index >> 10);
            }
            for qubit in 0..10 {
                let one: f64 = snapshot
                    .samples
                    .iter()
                    .filter(|sample| sample.index & (1 << qubit) != 0)
                    .map(|sample| sample.probability)
                    .sum();
                assert!((0.3..0.7).contains(&one));
            }
        }
        assert!(
            first
                .samples
                .iter()
                .zip(&next.samples)
                .any(|(a, b)| (a.probability - b.probability).abs() > 1e-5)
        );
        assert!(
            first
                .samples
                .iter()
                .zip(&next.samples)
                .any(|(a, b)| (a.phase - b.phase).abs() > 0.1)
        );
    }
}

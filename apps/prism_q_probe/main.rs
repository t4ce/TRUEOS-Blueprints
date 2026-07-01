use prism_q::CircuitBuilder;
use trueos::logl::{self, level};

const MAX_QUBITS: usize = 26;
const GHZ_QUBITS: usize = 8;

fn main() {
    logl::log(level::INFO, format_args!("prism_q_probe: start"));
    logl::log(
        level::INFO,
        format_args!("prism_q_probe: softcap max_qubits={}", MAX_QUBITS),
    );
    log_cap_env("PRISM_MAX_SV_QUBITS");
    log_cap_env("PRISM_MAX_PROB_QUBITS");
    log_cap_env("PRISM_MAX_EXPORT_QUBITS");

    match run_probe() {
        Ok(()) => logl::log(level::INFO, format_args!("prism_q_probe: done")),
        Err(stage) => logl::log(
            level::ERROR,
            format_args!("prism_q_probe: failed stage={}", stage),
        ),
    }
}

fn log_cap_env(key: &str) {
    let std_value = std::env::var(key).unwrap_or_else(|_| "missing".to_string());
    let trueos_value = trueos::env::var(key).unwrap_or_else(|_| "missing".to_string());
    logl::log(
        level::INFO,
        format_args!(
            "prism_q_probe: env {} std={} trueos={}",
            key, std_value, trueos_value
        ),
    );
}

fn run_probe() -> Result<(), &'static str> {
    run_bell_state()?;
    run_ghz_state(GHZ_QUBITS)?;
    if ensure_within_cap(MAX_QUBITS + 1).is_ok() {
        return Err("softcap.expected_block");
    }
    logl::log(
        level::INFO,
        format_args!(
            "prism_q_probe: softcap check blocked {} qubits as expected",
            MAX_QUBITS + 1
        ),
    );
    Ok(())
}

fn ensure_within_cap(qubits: usize) -> Result<(), &'static str> {
    if qubits <= MAX_QUBITS {
        Ok(())
    } else {
        Err("softcap.qubits")
    }
}

fn run_bell_state() -> Result<(), &'static str> {
    logl::log(
        level::INFO,
        format_args!("prism_q_probe: stage bell_state.build_run"),
    );

    ensure_within_cap(2)?;
    let result = CircuitBuilder::new(2)
        .h(0)
        .cx(0, 1)
        .run(42)
        .map_err(|_| "bell_state.run")?;

    let probabilities = result.probabilities.ok_or("bell_state.probabilities")?;
    let p00 = probabilities.get(0);
    let p01 = probabilities.get(1);
    let p10 = probabilities.get(2);
    let p11 = probabilities.get(3);

    logl::log(
        level::INFO,
        format_args!(
            "prism_q_probe: bell probabilities p00={:.6} p01={:.6} p10={:.6} p11={:.6}",
            p00, p01, p10, p11
        ),
    );

    if (p00 - 0.5).abs() > 1e-10
        || p01.abs() > 1e-10
        || p10.abs() > 1e-10
        || (p11 - 0.5).abs() > 1e-10
    {
        return Err("bell_state.value");
    }

    logl::log(
        level::INFO,
        format_args!("prism_q_probe: success bell_state.build_run"),
    );
    Ok(())
}

fn run_ghz_state(qubits: usize) -> Result<(), &'static str> {
    logl::log(
        level::INFO,
        format_args!("prism_q_probe: stage ghz_state.build_run qubits={}", qubits),
    );

    ensure_within_cap(qubits)?;
    if qubits < 2 {
        return Err("ghz_state.qubits");
    }

    let mut builder = CircuitBuilder::new(qubits);
    builder.h(0);
    for target in 1..qubits {
        builder.cx(0, target);
    }

    let result = builder.run(43).map_err(|_| "ghz_state.run")?;
    let probabilities = result.probabilities.ok_or("ghz_state.probabilities")?;
    let high_index = (1usize << qubits) - 1;
    let p_zero = probabilities.get(0);
    let p_one = probabilities.get(high_index);
    let total: f64 = probabilities.iter().sum();

    logl::log(
        level::INFO,
        format_args!(
            "prism_q_probe: ghz probabilities qubits={} p0={:.6} p1={:.6} total={:.6}",
            qubits, p_zero, p_one, total
        ),
    );

    if (p_zero - 0.5).abs() > 1e-10 || (p_one - 0.5).abs() > 1e-10 || (total - 1.0).abs() > 1e-10 {
        return Err("ghz_state.value");
    }

    logl::log(
        level::INFO,
        format_args!("prism_q_probe: success ghz_state.build_run"),
    );
    Ok(())
}

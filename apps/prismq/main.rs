use std::{
    env, fs,
    io::{self, Read},
};

use prism_q::{Instruction, SvgOptions, TextOptions, bitstring, circuit::openqasm, simulate};
use serde::Deserialize;

const MAX_SIM_QUBITS: usize = 26;
const DEFAULT_SEED: u64 = 42;
const DEFAULT_SHOTS: usize = 1024;
const DEFAULT_THRESHOLD: f64 = 1e-10;

const DEFAULT_BELL_QASM: &str = r#"OPENQASM 3.0;
include "stdgates.inc";

qubit[2] q;

h q[0];
cx q[0], q[1];
"#;

const DEFAULT_BELL_MEASURED_QASM: &str = r#"OPENQASM 3.0;
include "stdgates.inc";

qubit[2] q;
bit[2] c;

h q[0];
cx q[0], q[1];
c[0] = measure q[0];
c[1] = measure q[1];
"#;

const BELL_PAIR_QASM2: &str = r#"OPENQASM 2.0;
include "qelib1.inc";

qreg q[2];
creg c[2];

// Create entanglement.
h q[0];
cx q[0], q[1];

// Measure both qubits.
measure q[0] -> c[0];
measure q[1] -> c[1];
"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Run,
    Probs,
    Shots,
    Counts,
    Draw,
    Inspect,
    Validate,
    Example,
    Help,
}

struct Options {
    command: Command,
    input: Option<String>,
    seed: u64,
    shots: usize,
    threshold: f64,
    draw_format: DrawFormat,
    out: Option<String>,
}

#[derive(Deserialize)]
struct JsonCircuit {
    qubits: usize,
    bits: Option<usize>,
    #[serde(default)]
    gates: Vec<JsonGate>,
}

#[derive(Deserialize)]
struct JsonGate {
    #[serde(alias = "op")]
    gate: String,
    #[serde(default)]
    targets: Vec<usize>,
    #[serde(default)]
    controls: Vec<usize>,
    #[serde(default)]
    params: Vec<f64>,
    target: Option<usize>,
    control: Option<usize>,
    qubit: Option<usize>,
    bit: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DrawFormat {
    Text,
    Svg,
}

fn main() {
    if let Err(err) = run_with_platform_runtime() {
        eprintln!("prismq: error: {err}");
        std::process::exit(1);
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn run_with_platform_runtime() -> Result<(), String> {
    let runtime = match tokio::runtime::Builder::new_current_thread().build() {
        Ok(runtime) => runtime,
        Err(err) => return Err(format!("tokio runtime build failed: {err}")),
    };

    runtime.block_on(async {
        tokio::task::yield_now().await;
        run()
    })
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn run_with_platform_runtime() -> Result<(), String> {
    run()
}

fn run() -> Result<(), String> {
    let options = parse_args(env::args().skip(1).collect())?;
    if options.command == Command::Help {
        print_help();
        return Ok(());
    }
    if options.command == Command::Example {
        print_example(options.input.as_deref())?;
        return Ok(());
    }

    let qasm = read_input(options.command, options.input.as_deref())?;
    let circuit = openqasm::parse(&qasm).map_err(|err| format!("parse failed: {err}"))?;

    match options.command {
        Command::Run => run_exact(&circuit, options.seed, options.threshold),
        Command::Probs => run_probs(&circuit, options.seed, options.threshold),
        Command::Shots => run_shots(&circuit, options.seed, options.shots),
        Command::Counts => run_counts(&circuit, options.seed, options.shots),
        Command::Draw => draw_circuit(&circuit, options.draw_format, options.out.as_deref()),
        Command::Inspect => inspect_circuit(&circuit),
        Command::Validate => {
            println!("valid");
            inspect_circuit(&circuit)
        }
        Command::Example | Command::Help => Ok(()),
    }
}

fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let mut command = Command::Run;
    let mut index = 0;
    if let Some(first) = args.first() {
        if let Some(parsed) = parse_command(first) {
            command = parsed;
            index = 1;
        }
    }

    let mut input = None;
    let mut seed = DEFAULT_SEED;
    let mut shots = DEFAULT_SHOTS;
    let mut threshold = DEFAULT_THRESHOLD;
    let mut draw_format = DrawFormat::Text;
    let mut out = None;

    while index < args.len() {
        match args[index].as_str() {
            "--seed" => {
                index += 1;
                seed = parse_value(args.get(index), "--seed")?;
            }
            "--shots" => {
                index += 1;
                shots = parse_value(args.get(index), "--shots")?;
            }
            "--threshold" => {
                index += 1;
                threshold = args
                    .get(index)
                    .ok_or_else(|| "--threshold needs a value".to_string())?
                    .parse()
                    .map_err(|_| "--threshold expects a float".to_string())?;
            }
            "--format" => {
                index += 1;
                draw_format = match args.get(index).map(String::as_str) {
                    Some("text") => DrawFormat::Text,
                    Some("svg") => DrawFormat::Svg,
                    Some(other) => return Err(format!("unknown draw format `{other}`")),
                    None => return Err("--format needs a value".to_string()),
                };
            }
            "--svg" => draw_format = DrawFormat::Svg,
            "--out" => {
                index += 1;
                out = Some(
                    args.get(index)
                        .ok_or_else(|| "--out needs a path".to_string())?
                        .clone(),
                );
            }
            "--backend" => {
                index += 1;
                match args.get(index).map(String::as_str) {
                    Some("auto") => {}
                    Some(other) => {
                        return Err(format!(
                            "backend `{other}` is not wired yet; use `--backend auto`"
                        ));
                    }
                    None => return Err("--backend needs a value".to_string()),
                }
            }
            "--help" | "-h" => command = Command::Help,
            arg if arg.starts_with('-') && arg != "-" => {
                return Err(format!("unknown option `{arg}`"));
            }
            path => {
                if input.replace(path.to_string()).is_some() {
                    return Err("only one input path is supported".to_string());
                }
            }
        }
        index += 1;
    }

    Ok(Options {
        command,
        input,
        seed,
        shots,
        threshold,
        draw_format,
        out,
    })
}

fn parse_command(arg: &str) -> Option<Command> {
    match arg {
        "run" => Some(Command::Run),
        "probs" => Some(Command::Probs),
        "shots" => Some(Command::Shots),
        "counts" => Some(Command::Counts),
        "draw" => Some(Command::Draw),
        "inspect" => Some(Command::Inspect),
        "validate" => Some(Command::Validate),
        "example" | "examples" => Some(Command::Example),
        "help" | "--help" | "-h" => Some(Command::Help),
        _ => None,
    }
}

fn parse_value<T>(value: Option<&String>, name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .ok_or_else(|| format!("{name} needs a value"))?
        .parse()
        .map_err(|_| format!("{name} has an invalid value"))
}

fn read_input(command: Command, input: Option<&str>) -> Result<String, String> {
    match input {
        None if matches!(command, Command::Shots | Command::Counts) => {
            Ok(DEFAULT_BELL_MEASURED_QASM.to_string())
        }
        None => Ok(DEFAULT_BELL_QASM.to_string()),
        Some("-") => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|err| format!("failed to read stdin: {err}"))?;
            input_text_to_qasm("-", &text)
        }
        Some(name) if builtin_example_qasm(name).is_some() => {
            Ok(builtin_example_qasm(name).unwrap().to_string())
        }
        Some(path) => {
            let text =
                fs::read_to_string(path).map_err(|err| format!("failed to read {path}: {err}"))?;
            input_text_to_qasm(path, &text)
        }
    }
}

fn builtin_example_qasm(name: &str) -> Option<&'static str> {
    match name {
        "bell" | "bell-pair" | "hello-bell" | "entanglement" | "example:bell"
        | "example:bell-pair" | "builtin:bell" | "builtin:bell-pair" => Some(BELL_PAIR_QASM2),
        _ => None,
    }
}

fn print_example(name: Option<&str>) -> Result<(), String> {
    let name = name.unwrap_or("bell-pair");
    if let Some(qasm) = builtin_example_qasm(name) {
        print!("{qasm}");
        Ok(())
    } else {
        Err(format!(
            "unknown built-in example `{name}`; available: bell-pair"
        ))
    }
}

fn input_text_to_qasm(path: &str, text: &str) -> Result<String, String> {
    let trimmed = text.trim_start();
    if path.ends_with(".json") || trimmed.starts_with('{') {
        json_to_qasm(text)
    } else {
        Ok(text.to_string())
    }
}

fn json_to_qasm(text: &str) -> Result<String, String> {
    let circuit: JsonCircuit =
        serde_json::from_str(text).map_err(|err| format!("json parse failed: {err}"))?;
    if circuit.qubits == 0 {
        return Err("json circuit needs at least one qubit".to_string());
    }
    if circuit.qubits > MAX_SIM_QUBITS {
        return Err(format!(
            "json circuit qubits exceed softcap: {} > {MAX_SIM_QUBITS}",
            circuit.qubits
        ));
    }

    let mut bits = circuit.bits.unwrap_or(0);
    for gate in &circuit.gates {
        if let Some(bit) = gate.bit {
            bits = bits.max(bit + 1);
        }
    }

    let mut qasm = String::new();
    qasm.push_str("OPENQASM 3.0;\ninclude \"stdgates.inc\";\n\n");
    qasm.push_str(&format!("qubit[{}] q;\n", circuit.qubits));
    if bits > 0 {
        qasm.push_str(&format!("bit[{bits}] c;\n"));
    }
    qasm.push('\n');

    for (index, gate) in circuit.gates.iter().enumerate() {
        qasm.push_str(&json_gate_to_qasm(index, gate, circuit.qubits, bits)?);
    }

    Ok(qasm)
}

fn json_gate_to_qasm(
    index: usize,
    gate: &JsonGate,
    qubits: usize,
    bits: usize,
) -> Result<String, String> {
    let name = gate.gate.to_ascii_lowercase();
    match name.as_str() {
        "measure" | "m" => {
            let qubit = gate
                .qubit
                .or(gate.target)
                .or_else(|| gate.targets.first().copied());
            let bit = gate.bit.unwrap_or_else(|| qubit.unwrap_or(0));
            let qubit = qubit.ok_or_else(|| format!("gate {index}: measure needs a qubit"))?;
            check_qubit(index, qubit, qubits)?;
            if bit >= bits {
                return Err(format!("gate {index}: bit {bit} out of range 0..{bits}"));
            }
            Ok(format!("c[{bit}] = measure q[{qubit}];\n"))
        }
        "reset" => {
            let target = one_target(index, gate, qubits)?;
            Ok(format!("reset q[{target}];\n"))
        }
        "barrier" => {
            let targets = if gate.targets.is_empty() {
                (0..qubits).collect()
            } else {
                checked_targets(index, &gate.targets, qubits)?
            };
            Ok(format!("barrier {};\n", format_qubits(&targets)))
        }
        "id" | "x" | "y" | "z" | "h" | "s" | "sdg" | "t" | "tdg" | "sx" | "sxdg" => {
            let target = one_target(index, gate, qubits)?;
            Ok(format!("{name} q[{target}];\n"))
        }
        "rx" | "ry" | "rz" | "p" | "phase" | "u1" | "gpi" | "gpi2" => {
            let target = one_target(index, gate, qubits)?;
            let theta = one_param(index, gate)?;
            let qasm_name = if name == "phase" { "p" } else { name.as_str() };
            Ok(format!("{qasm_name}({theta}) q[{target}];\n"))
        }
        "r" | "u2" => {
            let target = one_target(index, gate, qubits)?;
            Ok(format!(
                "{name}({}) q[{target}];\n",
                params_list(index, gate, 2)?
            ))
        }
        "u" | "u3" => {
            let target = one_target(index, gate, qubits)?;
            Ok(format!(
                "{name}({}) q[{target}];\n",
                params_list(index, gate, 3)?
            ))
        }
        "cx" | "cnot" | "cy" | "cz" | "ch" | "cs" | "csdg" | "csx" => {
            let (control, target) = control_target(index, gate, qubits)?;
            let qasm_name = if name == "cnot" { "cx" } else { name.as_str() };
            Ok(format!("{qasm_name} q[{control}], q[{target}];\n"))
        }
        "cu" => {
            let (control, target) = control_target(index, gate, qubits)?;
            Ok(format!(
                "cu({}) q[{control}], q[{target}];\n",
                params_list(index, gate, 4)?
            ))
        }
        "cp" | "cphase" | "crx" | "cry" | "crz" => {
            let theta = one_param(index, gate)?;
            let (control, target) = control_target(index, gate, qubits)?;
            Ok(format!("{name}({theta}) q[{control}], q[{target}];\n"))
        }
        "rzz" | "rxx" | "ryy" => {
            let theta = one_param(index, gate)?;
            let (a, b) = two_targets(index, gate, qubits)?;
            Ok(format!("{name}({theta}) q[{a}], q[{b}];\n"))
        }
        "xx_plus_yy" | "xx_minus_yy" => {
            let (a, b) = two_targets(index, gate, qubits)?;
            Ok(format!(
                "{name}({}) q[{a}], q[{b}];\n",
                params_list(index, gate, 2)?
            ))
        }
        "ms" => {
            if !(gate.params.len() == 2 || gate.params.len() == 3) {
                return Err(format!("gate {index}: ms needs two or three parameters"));
            }
            let (a, b) = two_targets(index, gate, qubits)?;
            Ok(format!(
                "ms({}) q[{a}], q[{b}];\n",
                format_params(&gate.params)
            ))
        }
        "swap" | "iswap" | "ecr" | "dcx" | "syc" | "sqrt_iswap" | "sqrt_iswap_inv" => {
            let (a, b) = two_targets(index, gate, qubits)?;
            Ok(format!("{name} q[{a}], q[{b}];\n"))
        }
        "ccx" | "toffoli" | "ccz" | "rccx" => {
            let (a, b, c) = three_targets(index, gate, qubits)?;
            let qasm_name = if name == "toffoli" {
                "ccx"
            } else {
                name.as_str()
            };
            Ok(format!("{qasm_name} q[{a}], q[{b}], q[{c}];\n"))
        }
        "cswap" | "fredkin" => {
            let (a, b, c) = three_targets(index, gate, qubits)?;
            let qasm_name = if name == "fredkin" {
                "cswap"
            } else {
                name.as_str()
            };
            Ok(format!("{qasm_name} q[{a}], q[{b}], q[{c}];\n"))
        }
        "c3x" | "rc3x" | "rcccx" => {
            let targets = n_targets(index, gate, qubits, 4)?;
            Ok(format!("{name} {};\n", format_qubits(&targets)))
        }
        "c4x" => {
            let targets = n_targets(index, gate, qubits, 5)?;
            Ok(format!("{name} {};\n", format_qubits(&targets)))
        }
        "mcx" => {
            if gate.targets.len() < 2 {
                return Err(format!("gate {index}: mcx needs at least two targets"));
            }
            let targets = checked_targets(index, &gate.targets, qubits)?;
            Ok(format!("mcx {};\n", format_qubits(&targets)))
        }
        other => Err(format!("gate {index}: unsupported json gate `{other}`")),
    }
}

fn one_target(index: usize, gate: &JsonGate, qubits: usize) -> Result<usize, String> {
    let target = gate
        .target
        .or_else(|| gate.targets.first().copied())
        .ok_or_else(|| format!("gate {index}: needs one target"))?;
    check_qubit(index, target, qubits)?;
    Ok(target)
}

fn one_param(index: usize, gate: &JsonGate) -> Result<f64, String> {
    gate.params
        .first()
        .copied()
        .ok_or_else(|| format!("gate {index}: needs one parameter"))
}

fn params_list(index: usize, gate: &JsonGate, count: usize) -> Result<String, String> {
    if gate.params.len() == count {
        Ok(format_params(&gate.params))
    } else {
        Err(format!("gate {index}: needs {count} parameters"))
    }
}

fn control_target(index: usize, gate: &JsonGate, qubits: usize) -> Result<(usize, usize), String> {
    let control = gate
        .control
        .or_else(|| gate.controls.first().copied())
        .or_else(|| gate.targets.first().copied())
        .ok_or_else(|| format!("gate {index}: needs a control"))?;
    let target = gate
        .target
        .or_else(|| {
            if gate.controls.is_empty() {
                gate.targets.get(1).copied()
            } else {
                gate.targets.first().copied()
            }
        })
        .ok_or_else(|| format!("gate {index}: needs a target"))?;
    check_qubit(index, control, qubits)?;
    check_qubit(index, target, qubits)?;
    Ok((control, target))
}

fn two_targets(index: usize, gate: &JsonGate, qubits: usize) -> Result<(usize, usize), String> {
    if gate.targets.len() < 2 {
        return Err(format!("gate {index}: needs two targets"));
    }
    let targets = checked_targets(index, &gate.targets[..2], qubits)?;
    Ok((targets[0], targets[1]))
}

fn three_targets(
    index: usize,
    gate: &JsonGate,
    qubits: usize,
) -> Result<(usize, usize, usize), String> {
    if gate.targets.len() < 3 {
        return Err(format!("gate {index}: needs three targets"));
    }
    let targets = checked_targets(index, &gate.targets[..3], qubits)?;
    Ok((targets[0], targets[1], targets[2]))
}

fn n_targets(
    index: usize,
    gate: &JsonGate,
    qubits: usize,
    count: usize,
) -> Result<Vec<usize>, String> {
    if gate.targets.len() < count {
        return Err(format!("gate {index}: needs {count} targets"));
    }
    checked_targets(index, &gate.targets[..count], qubits)
}

fn checked_targets(index: usize, targets: &[usize], qubits: usize) -> Result<Vec<usize>, String> {
    for &target in targets {
        check_qubit(index, target, qubits)?;
    }
    Ok(targets.to_vec())
}

fn check_qubit(index: usize, qubit: usize, qubits: usize) -> Result<(), String> {
    if qubit < qubits {
        Ok(())
    } else {
        Err(format!(
            "gate {index}: qubit {qubit} out of range 0..{qubits}"
        ))
    }
}

fn format_qubits(targets: &[usize]) -> String {
    targets
        .iter()
        .map(|target| format!("q[{target}]"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_params(params: &[f64]) -> String {
    params
        .iter()
        .map(|param| param.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn ensure_sim_cap(circuit: &prism_q::Circuit) -> Result<(), String> {
    if circuit.num_qubits <= MAX_SIM_QUBITS {
        Ok(())
    } else {
        Err(format!(
            "simulation softcap is {MAX_SIM_QUBITS} qubits; input has {}",
            circuit.num_qubits
        ))
    }
}

fn run_exact(circuit: &prism_q::Circuit, seed: u64, threshold: f64) -> Result<(), String> {
    ensure_sim_cap(circuit)?;
    let result = simulate(circuit)
        .seed(seed)
        .run()
        .map_err(|err| format!("simulation failed: {err}"))?;

    println!(
        "classical_bits: {}",
        format_classical_bits(&result.classical_bits)
    );
    if let Some(probabilities) = result.probabilities {
        print_probabilities(circuit.num_qubits, &probabilities, threshold);
    } else {
        println!("probabilities: unavailable");
    }
    Ok(())
}

fn run_probs(circuit: &prism_q::Circuit, seed: u64, threshold: f64) -> Result<(), String> {
    ensure_sim_cap(circuit)?;
    let result = simulate(circuit)
        .seed(seed)
        .run()
        .map_err(|err| format!("simulation failed: {err}"))?;
    let probabilities = result
        .probabilities
        .ok_or_else(|| "probabilities unavailable for selected backend".to_string())?;
    print_probabilities(circuit.num_qubits, &probabilities, threshold);
    Ok(())
}

fn run_shots(circuit: &prism_q::Circuit, seed: u64, shots: usize) -> Result<(), String> {
    ensure_sim_cap(circuit)?;
    let result = simulate(circuit)
        .seed(seed)
        .shots(shots)
        .map_err(|err| format!("shot simulation failed: {err}"))?;
    print!("{result}");
    Ok(())
}

fn run_counts(circuit: &prism_q::Circuit, seed: u64, shots: usize) -> Result<(), String> {
    ensure_sim_cap(circuit)?;
    let result = simulate(circuit)
        .seed(seed)
        .sample_counts(shots)
        .map_err(|err| format!("count sampling failed: {err}"))?;
    let mut entries: Vec<_> = result.counts.into_iter().collect();
    entries.sort_by_key(|(bits, count)| {
        (
            std::cmp::Reverse(*count),
            bitstring(bits, result.num_classical_bits),
        )
    });
    for (bits, count) in entries {
        println!("{}: {}", bitstring(&bits, result.num_classical_bits), count);
    }
    Ok(())
}

fn draw_circuit(
    circuit: &prism_q::Circuit,
    format: DrawFormat,
    out: Option<&str>,
) -> Result<(), String> {
    let text = match format {
        DrawFormat::Text => circuit.draw(&TextOptions::default()),
        DrawFormat::Svg => circuit.to_svg(&SvgOptions::default()),
    };

    if let Some(path) = out {
        fs::write(path, text).map_err(|err| format!("failed to write {path}: {err}"))?;
    } else {
        println!("{text}");
    }
    Ok(())
}

fn inspect_circuit(circuit: &prism_q::Circuit) -> Result<(), String> {
    let mut measurements = 0usize;
    let mut barriers = 0usize;
    let mut resets = 0usize;
    for instruction in &circuit.instructions {
        match instruction {
            Instruction::Measure { .. } => measurements += 1,
            Instruction::Barrier { .. } => barriers += 1,
            Instruction::Reset { .. } => resets += 1,
            Instruction::Gate { .. } | Instruction::Conditional { .. } => {}
        }
    }

    println!("qubits: {}", circuit.num_qubits);
    println!("classical_bits: {}", circuit.num_classical_bits);
    println!("instructions: {}", circuit.instructions.len());
    println!("gates: {}", circuit.gate_count());
    println!("measurements: {measurements}");
    println!("barriers: {barriers}");
    println!("resets: {resets}");
    println!("depth: {}", circuit.depth());
    Ok(())
}

fn print_probabilities(
    num_qubits: usize,
    probabilities: &prism_q::sim::Probabilities,
    threshold: f64,
) {
    for index in 0..probabilities.len() {
        let p = probabilities.get(index);
        if p > threshold {
            println!("|{index:0width$b}> = {p:.6}", width = num_qubits);
        }
    }
}

fn format_classical_bits(bits: &[bool]) -> String {
    bits.iter()
        .map(|bit| if *bit { '1' } else { '0' })
        .collect()
}

fn print_help() {
    println!("usage:");
    println!("  prismq run [file.qasm|file.json|-] [--seed 42] [--threshold 1e-10]");
    println!("  prismq probs [file.qasm|file.json|-] [--seed 42] [--threshold 1e-10]");
    println!("  prismq shots [file.qasm|file.json|-] [--shots 1024] [--seed 42]");
    println!("  prismq counts [file.qasm|file.json|-] [--shots 1024] [--seed 42]");
    println!("  prismq inspect [file.qasm|file.json|-]");
    println!("  prismq validate [file.qasm|file.json|-]");
    println!("  prismq draw [file.qasm|file.json|-] [--format text|svg] [--out path]");
    println!("  prismq example [bell-pair]");
    println!();
    println!("With no input path, prismq runs an embedded Bell-state OpenQASM program.");
    println!("Built-in example input aliases include `example:bell-pair` and `bell-pair`.");
    println!("JSON input is accepted for .json files or stdin beginning with `{{`.");
    println!(
        "The embedded shots/counts program includes measurements; run/probs leaves Bell unmeasured."
    );
}

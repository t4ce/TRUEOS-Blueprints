use std::{
    env, fs,
    io::{self, Read},
};

use prism_q::{Instruction, SvgOptions, TextOptions, bitstring, circuit::openqasm, simulate};

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Run,
    Probs,
    Shots,
    Counts,
    Draw,
    Inspect,
    Validate,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum DrawFormat {
    Text,
    Svg,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("prismq: error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(env::args().skip(1).collect())?;
    if options.command == Command::Help {
        print_help();
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
        Command::Help => Ok(()),
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
            let mut qasm = String::new();
            io::stdin()
                .read_to_string(&mut qasm)
                .map_err(|err| format!("failed to read stdin: {err}"))?;
            Ok(qasm)
        }
        Some(path) => {
            fs::read_to_string(path).map_err(|err| format!("failed to read {path}: {err}"))
        }
    }
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
    println!("  prismq run [file.qasm|-] [--seed 42] [--threshold 1e-10]");
    println!("  prismq probs [file.qasm|-] [--seed 42] [--threshold 1e-10]");
    println!("  prismq shots [file.qasm|-] [--shots 1024] [--seed 42]");
    println!("  prismq counts [file.qasm|-] [--shots 1024] [--seed 42]");
    println!("  prismq inspect [file.qasm|-]");
    println!("  prismq validate [file.qasm|-]");
    println!("  prismq draw [file.qasm|-] [--format text|svg] [--out path]");
    println!();
    println!("With no input path, prismq runs an embedded Bell-state OpenQASM program.");
    println!("The embedded shots/counts program includes measurements; run/probs leaves Bell unmeasured.");
}

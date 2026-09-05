// trueos-blueprint: features=["tokio-net-probe"]

use std::{
    env, fs,
    io::{self, Read},
    net::SocketAddr,
    time::Instant,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use prism_q::{Instruction, SvgOptions, TextOptions, bitstring, circuit::openqasm, simulate};
mod storage;
use storage::{CircuitDatabase, persist_circuit_database};
use serde::{Deserialize, Serialize};

const MAX_SIM_QUBITS: usize = 26;
const DEFAULT_SEED: u64 = 0;
const DEFAULT_SHOTS: usize = 1024;
const DEFAULT_THRESHOLD: f64 = 1e-10;
const DESIGNER_PORT: u16 = 8338;
const DESIGNER_HTML: &str = include_str!("designer.html");
const MAX_WEB_SHOTS: usize = 100_000;
const MAX_WEB_OUTCOMES: usize = 256;
// App filesystem paths are relative to TRUEOS's dedicated `apps/prismq` root.
const CIRCUIT_DB_PATH: &str = "prismq.redb";

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use tokio::fs as app_fs;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use trueos::fs as app_fs;

static CIRCUIT_DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    Serve,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
struct JsonCircuit {
    #[serde(default = "default_schema", skip_serializing_if = "String::is_empty")]
    schema: String,
    qubits: usize,
    bits: Option<usize>,
    #[serde(default)]
    gates: Vec<JsonGate>,
    #[serde(default = "default_seed")]
    seed: u64,
    #[serde(default = "default_shots")]
    shots: usize,
    #[serde(default = "default_threshold")]
    threshold: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    history: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    readout: Option<JsonReadout>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct JsonReadout {
    mode: JsonReadoutMode,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum JsonReadoutMode {
    #[default]
    Explicit,
    MeasureUnmeasured,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

fn default_schema() -> String {
    "prismq.circuit.v1".to_string()
}

const fn default_seed() -> u64 {
    DEFAULT_SEED
}

const fn default_shots() -> usize {
    DEFAULT_SHOTS
}

const fn default_threshold() -> f64 {
    DEFAULT_THRESHOLD
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CircuitSummary {
    qubits: usize,
    classical_bits: usize,
    instructions: usize,
    gates: usize,
    measurements: usize,
    barriers: usize,
    resets: usize,
    depth: usize,
}

#[derive(Debug, Serialize)]
struct ProbabilityRow {
    state: String,
    probability: f64,
}

#[derive(Debug, Serialize)]
struct CountRow {
    state: String,
    count: u64,
    probability: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SimulationResponse {
    schema: &'static str,
    ok: bool,
    elapsed_ms: u64,
    seed: u64,
    shots: usize,
    threshold: f64,
    circuit: CircuitSummary,
    classical_bits: String,
    probabilities: Vec<ProbabilityRow>,
    probabilities_truncated: bool,
    counts: Vec<CountRow>,
    readout: ReadoutSummary,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadoutSummary {
    mode: JsonReadoutMode,
    implicit_measurements: Vec<ReadoutMapping>,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ReadoutMapping {
    qubit: usize,
    bit: usize,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn simulation(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        eprintln!("prismq: API error ({}): {}", self.status, self.message);
        (
            self.status,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({
                "schema": "prismq.error.v1",
                "ok": false,
                "error": self.message,
            })),
        )
            .into_response()
    }
}

async fn handle_index() -> Html<&'static str> {
    println!("prismq: HTTP GET /");
    Html(DESIGNER_HTML)
}

async fn handle_health() -> impl IntoResponse {
    Json(serde_json::json!({
        "schema": "prismq.health.v1",
        "ok": true,
        "app": "prismq",
        "port": DESIGNER_PORT,
    }))
}

async fn handle_simulate(
    payload: Result<Json<JsonCircuit>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(document) = payload.map_err(|err| ApiError::bad_request(err.body_text()))?;
    let response = simulate_json_document(&document)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)))
}

async fn handle_circuit_list() -> Result<impl IntoResponse, ApiError> {
    let _guard = CIRCUIT_DB_LOCK.lock().await;
    let database = CircuitDatabase::open().await?;
    let circuits = database.list()?;
    println!("prismq: circuit list count={}", circuits.len());
    Ok(Json(serde_json::json!({
        "schema": "prismq.saved-circuits.v1",
        "ok": true,
        "circuits": circuits,
    })))
}

async fn handle_storage_health() -> Result<impl IntoResponse, ApiError> {
    let _guard = CIRCUIT_DB_LOCK.lock().await;
    let database = CircuitDatabase::open().await?;
    let circuits = database.list()?;
    let revisions = database.revision_count()?;
    let existed_before_open = database.existed_before_open;
    let loaded_bytes = database.loaded_bytes;
    let serialized_bytes = database.persisted_image()?.len();
    Ok(Json(serde_json::json!({
        "schema": "prismq.storage.v1",
        "ok": true,
        "backend": "redb",
        "path": CIRCUIT_DB_PATH,
        "redbVersion": "4.2.0",
        "existedBeforeOpen": existed_before_open,
        "loadedBytes": loaded_bytes,
        "serializedBytes": serialized_bytes,
        "circuits": circuits.len(),
        "revisions": revisions,
    })))
}

async fn handle_circuit_load(Path(raw_name): Path<String>) -> Result<impl IntoResponse, ApiError> {
    let name = normalize_circuit_name(&raw_name)?;
    let _guard = CIRCUIT_DB_LOCK.lock().await;
    let database = CircuitDatabase::open().await?;
    let circuit = database
        .load(&name, None)?
        .ok_or_else(|| ApiError::not_found(format!("saved circuit `{name}` was not found")))?;
    Ok(Json(serde_json::json!({
        "schema": "prismq.saved-circuit.v1",
        "ok": true,
        "name": name,
        "circuit": circuit,
    })))
}

async fn handle_circuit_revision_load(
    Path((raw_name, revision)): Path<(String, usize)>,
) -> Result<impl IntoResponse, ApiError> {
    let name = normalize_circuit_name(&raw_name)?;
    if revision == 0 {
        return Err(ApiError::bad_request("circuit revisions start at 1"));
    }
    let _guard = CIRCUIT_DB_LOCK.lock().await;
    let database = CircuitDatabase::open().await?;
    let circuit = database.load(&name, Some(revision))?.ok_or_else(|| {
        ApiError::not_found(format!(
            "revision {revision} of circuit `{name}` was not found"
        ))
    })?;
    Ok(Json(serde_json::json!({
        "schema": "prismq.saved-circuit.v1",
        "ok": true,
        "name": name,
        "revision": revision,
        "circuit": circuit,
    })))
}

async fn handle_circuit_save(
    Path(raw_name): Path<String>,
    payload: Result<Json<JsonCircuit>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let name = normalize_circuit_name(&raw_name)?;
    println!("prismq: save request begin name={name}");
    let Json(circuit) = payload.map_err(|err| ApiError::bad_request(err.body_text()))?;
    validate_circuit_document(&circuit)?;
    let _guard = CIRCUIT_DB_LOCK.lock().await;
    let mut database = CircuitDatabase::open().await?;
    let archived_revision = database.save(&name, &circuit)?;
    let database_image = database.persisted_image()?;
    let database_bytes = persist_circuit_database(database_image).await?;

    println!(
        "prismq: saved circuit `{name}` to redb bytes={database_bytes}{}",
        archived_revision
            .as_ref()
            .map(|revision| format!(" (archived revision {revision})"))
            .unwrap_or_default()
    );

    Ok(Json(serde_json::json!({
        "schema": "prismq.saved-circuit.v1",
        "ok": true,
        "name": name,
        "archivedRevision": archived_revision,
    })))
}

async fn handle_circuit_delete(
    Path(raw_name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let name = normalize_circuit_name(&raw_name)?;
    let _guard = CIRCUIT_DB_LOCK.lock().await;
    let mut database = CircuitDatabase::open().await?;
    if !database.delete(&name)? {
        return Err(ApiError::not_found(format!(
            "saved circuit `{name}` was not found"
        )));
    }
    let database_image = database.persisted_image()?;
    persist_circuit_database(database_image).await?;
    Ok(Json(serde_json::json!({
        "schema": "prismq.saved-circuit.v1",
        "ok": true,
        "name": name,
        "deleted": true,
    })))
}

fn normalize_circuit_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim();
    let lower = name.to_ascii_lowercase();
    let reserved_revision = lower.rsplit_once("_rev").is_some_and(|(_, suffix)| {
        !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
    });
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_'));
    if !valid || name.eq_ignore_ascii_case("index.json") || reserved_revision {
        return Err(ApiError::bad_request(
            "circuit name must be 1-64 letters, numbers, spaces, dashes, or underscores; `_revN` is reserved for history",
        ));
    }
    Ok(name.to_string())
}

fn designer_router() -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/designer.html", get(handle_index))
        .route("/healthz", get(handle_health))
        .route("/api/healthz", get(handle_health))
        .route("/api/storage", get(handle_storage_health))
        .route("/api/simulate", post(handle_simulate))
        .route("/api/circuits", get(handle_circuit_list))
        .route(
            "/api/circuits/{name}",
            get(handle_circuit_load)
                .post(handle_circuit_save)
                .delete(handle_circuit_delete),
        )
        .route(
            "/api/circuits/{name}/revisions/{revision}",
            get(handle_circuit_revision_load),
        )
        .layer(DefaultBodyLimit::max(1024 * 1024))
}

fn serve_designer() -> Result<(), String> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        use trueos::{logl, logl::level, platform, runtime};

        let runtime = runtime::current_thread_net()
            .build()
            .map_err(|err| format!("tokio network runtime build failed: {err}"))?;
        let local = tokio::task::LocalSet::new();
        let result = local.block_on(&runtime, designer_http_runtime());
        platform::poll_once();
        if let Err(err) = &result {
            logl::log(level::ERROR, format_args!("prismq: server failed {err}"));
        }
        result
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("tokio network runtime build failed: {err}"))?;
        runtime.block_on(designer_http_runtime())
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn configure_trueos_rayon() -> Result<(), String> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .use_current_thread()
        .build_global()
        .map_err(|err| format!("configure single-thread simulation pool failed: {err}"))?;
    println!("prismq: simulation pool ready threads=1 mode=current-thread");
    Ok(())
}

async fn designer_http_runtime() -> Result<(), String> {
    let addr = SocketAddr::from(([0, 0, 0, 0], DESIGNER_PORT));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| format!("cannot bind http://{addr}: {err}"))?;
    // The listener is exposed on the guest network.  Printing loopback here is
    // misleading to a browser running on the host (127.0.0.1 would point at
    // the host, not the TRUEOS guest), so advertise the actual bind scope.
    println!("PrismQ Designer listening on http://0.0.0.0:{DESIGNER_PORT}/ (use the guest IP)");
    if let Ok(ip) = env::var("TRUEOS_NET_IPV4") {
        if !ip.trim().is_empty() {
            println!("PrismQ Designer guest URL: http://{ip}:{DESIGNER_PORT}/");
        }
    }
    axum::serve(listener, designer_router())
        .await
        .map_err(|err| format!("server failed: {err}"))
}

fn simulate_json_document(document: &JsonCircuit) -> Result<SimulationResponse, ApiError> {
    validate_circuit_document(document)?;

    let started = Instant::now();
    println!(
        "prismq: simulation begin qubits={} gates={} shots={}",
        document.qubits,
        document.gates.len(),
        document.shots
    );
    let (execution, implicit_measurements) = materialize_readout(document)?;
    let qasm = json_circuit_to_qasm_raw(&execution).map_err(ApiError::bad_request)?;
    let circuit = openqasm::parse(&qasm)
        .map_err(|err| ApiError::bad_request(format!("parse failed: {err}")))?;
    ensure_sim_cap(&circuit).map_err(ApiError::bad_request)?;

    let exact = simulate(&circuit)
        .seed(document.seed)
        .run()
        .map_err(|err| ApiError::simulation(format!("simulation failed: {err}")))?;

    let mut probabilities = Vec::new();
    let mut probabilities_truncated = false;
    if let Some(distribution) = exact.probabilities {
        for (index, probability) in distribution.iter().enumerate() {
            if probability > document.threshold {
                probabilities.push(ProbabilityRow {
                    state: format!("{index:0width$b}", width = circuit.num_qubits),
                    probability,
                });
            }
        }
        probabilities.sort_by(|a, b| {
            b.probability
                .partial_cmp(&a.probability)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.state.cmp(&b.state))
        });
        if probabilities.len() > MAX_WEB_OUTCOMES {
            probabilities.truncate(MAX_WEB_OUTCOMES);
            probabilities_truncated = true;
        }
    }

    let summary = summarize_circuit(&circuit);
    let mut counts = Vec::new();
    if summary.measurements > 0 {
        let sampled = simulate(&circuit)
            .seed(document.seed)
            .sample_counts(document.shots)
            .map_err(|err| ApiError::simulation(format!("count sampling failed: {err}")))?;
        counts = sampled
            .counts
            .into_iter()
            .map(|(bits, count)| CountRow {
                state: bitstring(&bits, sampled.num_classical_bits),
                count,
                probability: count as f64 / document.shots as f64,
            })
            .collect();
        counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.state.cmp(&b.state)));
    }

    let response = SimulationResponse {
        schema: "prismq.simulation.v1",
        ok: true,
        elapsed_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        seed: document.seed,
        shots: document.shots,
        threshold: document.threshold,
        circuit: summary,
        classical_bits: format_classical_bits(&exact.classical_bits),
        probabilities,
        probabilities_truncated,
        counts,
        readout: ReadoutSummary {
            mode: document
                .readout
                .as_ref()
                .map(|readout| readout.mode)
                .unwrap_or_default(),
            implicit_measurements,
        },
    };
    println!(
        "prismq: simulation complete qubits={} elapsed_ms={}",
        document.qubits, response.elapsed_ms
    );
    Ok(response)
}

fn validate_circuit_document(document: &JsonCircuit) -> Result<(), ApiError> {
    if document.schema != "prismq.circuit.v1" {
        return Err(ApiError::bad_request(format!(
            "unsupported circuit schema `{}`; expected `prismq.circuit.v1`",
            document.schema
        )));
    }
    if document.shots == 0 || document.shots > MAX_WEB_SHOTS {
        return Err(ApiError::bad_request(format!(
            "shots must be between 1 and {MAX_WEB_SHOTS}"
        )));
    }
    if !document.threshold.is_finite() || !(0.0..=1.0).contains(&document.threshold) {
        return Err(ApiError::bad_request(
            "threshold must be a finite number between 0 and 1",
        ));
    }
    let qasm = json_circuit_to_qasm(document).map_err(ApiError::bad_request)?;
    let circuit = openqasm::parse(&qasm)
        .map_err(|err| ApiError::bad_request(format!("parse failed: {err}")))?;
    ensure_sim_cap(&circuit).map_err(ApiError::bad_request)?;
    Ok(())
}

fn summarize_circuit(circuit: &prism_q::Circuit) -> CircuitSummary {
    let mut summary = CircuitSummary {
        qubits: circuit.num_qubits,
        classical_bits: circuit.num_classical_bits,
        instructions: circuit.instructions.len(),
        gates: circuit.gate_count(),
        measurements: 0,
        barriers: 0,
        resets: 0,
        depth: circuit.depth(),
    };
    for instruction in &circuit.instructions {
        match instruction {
            Instruction::Measure { .. } => summary.measurements += 1,
            Instruction::Barrier { .. } => summary.barriers += 1,
            Instruction::Reset { .. } => summary.resets += 1,
            Instruction::Gate { .. } | Instruction::Conditional { .. } => {}
        }
    }
    summary
}

fn main() {
    if let Err(err) = dispatch() {
        eprintln!("prismq: error: {err}");
        std::process::exit(1);
    }
}

fn dispatch() -> Result<(), String> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    configure_trueos_rayon()?;
    let options = parse_args(env::args().skip(1).collect())?;
    if options.command == Command::Serve {
        return serve_designer();
    }
    run_cli(options)
}

fn run_cli(options: Options) -> Result<(), String> {
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
        Command::Serve => unreachable!("serve is dispatched before CLI execution"),
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
    let mut command = Command::Serve;
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
        "serve" | "designer" | "web" => Some(Command::Serve),
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
            Ok(builtin_example_qasm(name).unwrap())
        }
        Some(path) => {
            let text =
                fs::read_to_string(path).map_err(|err| format!("failed to read {path}: {err}"))?;
            input_text_to_qasm(path, &text)
        }
    }
}

fn builtin_example_qasm(name: &str) -> Option<String> {
    match name {
        "bell" | "bell-pair" | "hello-bell" | "entanglement" | "example:bell"
        | "example:bell-pair" | "builtin:bell" | "builtin:bell-pair" => {
            Some(BELL_PAIR_QASM2.to_string())
        }
        "ghz-8" | "example:ghz-8" | "stress-ghz-8" | "example:stress-ghz-8" => {
            Some(build_ghz_qasm(8, true))
        }
        "ghz-16" | "example:ghz-16" | "stress-ghz-16" | "example:stress-ghz-16" => {
            Some(build_ghz_qasm(16, true))
        }
        "ghz-24" | "example:ghz-24" | "stress-ghz-24" | "example:stress-ghz-24" => {
            Some(build_ghz_qasm(24, true))
        }
        "ghz-26" | "example:ghz-26" | "stress-ghz-26" | "example:stress-ghz-26" => {
            Some(build_ghz_qasm(26, true))
        }
        "mesh-16" | "example:mesh-16" | "stress-mesh-16" | "example:stress-mesh-16" => {
            Some(build_mesh_qasm(16, 6, true))
        }
        "mesh-20" | "example:mesh-20" | "stress-mesh-20" | "example:stress-mesh-20" => {
            Some(build_mesh_qasm(20, 7, true))
        }
        "mesh-22" | "example:mesh-22" | "stress-mesh-22" | "example:stress-mesh-22" => {
            Some(build_mesh_qasm(22, 8, true))
        }
        "mesh-24" | "example:mesh-24" | "stress-mesh-24" | "example:stress-mesh-24" => {
            Some(build_mesh_qasm(24, 8, true))
        }
        "mesh-26" | "example:mesh-26" | "stress-mesh-26" | "example:stress-mesh-26" => {
            Some(build_mesh_qasm(26, 9, true))
        }
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
            "unknown built-in example `{name}`; available: bell-pair, ghz-8, ghz-16, ghz-24, ghz-26, mesh-16, mesh-20, mesh-22, mesh-24, mesh-26"
        ))
    }
}

fn build_ghz_qasm(qubits: usize, measured: bool) -> String {
    let mut qasm = String::new();
    qasm.push_str("OPENQASM 3.0;\ninclude \"stdgates.inc\";\n\n");
    qasm.push_str(&format!("qubit[{qubits}] q;\n"));
    if measured {
        qasm.push_str(&format!("bit[{qubits}] c;\n"));
    }
    qasm.push('\n');
    qasm.push_str("h q[0];\n");
    for target in 1..qubits {
        qasm.push_str(&format!("cx q[{}], q[{target}];\n", target - 1));
    }
    if measured {
        qasm.push('\n');
        for qubit in 0..qubits {
            qasm.push_str(&format!("c[{qubit}] = measure q[{qubit}];\n"));
        }
    }
    qasm
}

fn build_mesh_qasm(qubits: usize, layers: usize, measured: bool) -> String {
    let mut qasm = String::new();
    qasm.push_str("OPENQASM 3.0;\ninclude \"stdgates.inc\";\n\n");
    qasm.push_str(&format!("qubit[{qubits}] q;\n"));
    if measured {
        qasm.push_str(&format!("bit[{qubits}] c;\n"));
    }
    qasm.push('\n');

    for qubit in 0..qubits {
        qasm.push_str(&format!("h q[{qubit}];\n"));
    }

    for layer in 0..layers {
        qasm.push_str(&format!("\n// stress layer {layer}\n"));
        for qubit in 0..qubits {
            let theta = ((layer + 1) * (qubit + 3)) as f64 / 37.0;
            let phi = ((layer + 2) * (qubit + 5)) as f64 / 41.0;
            qasm.push_str(&format!("rz({theta:.12}) q[{qubit}];\n"));
            if (layer + qubit) % 2 == 0 {
                qasm.push_str(&format!("rx({phi:.12}) q[{qubit}];\n"));
            } else {
                qasm.push_str(&format!("ry({phi:.12}) q[{qubit}];\n"));
            }
            if (layer + qubit) % 3 == 0 {
                qasm.push_str(&format!("t q[{qubit}];\n"));
            }
        }

        let parity = layer % 2;
        let mut control = parity;
        while control + 1 < qubits {
            qasm.push_str(&format!("cx q[{control}], q[{}];\n", control + 1));
            control += 2;
        }

        for qubit in 0..qubits {
            let other = (qubit + layer + 3) % qubits;
            if qubit != other && (qubit + layer) % 4 == 0 {
                qasm.push_str(&format!("cz q[{qubit}], q[{other}];\n"));
            }
        }
    }

    if measured {
        qasm.push('\n');
        for qubit in 0..qubits {
            qasm.push_str(&format!("c[{qubit}] = measure q[{qubit}];\n"));
        }
    }
    qasm
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
    json_circuit_to_qasm(&circuit)
}

fn json_circuit_to_qasm(circuit: &JsonCircuit) -> Result<String, String> {
    let (execution, _) = materialize_readout(circuit).map_err(|err| err.message)?;
    json_circuit_to_qasm_raw(&execution)
}

fn materialize_readout(
    circuit: &JsonCircuit,
) -> Result<(JsonCircuit, Vec<ReadoutMapping>), ApiError> {
    let mut execution = circuit.clone();
    execution.readout = None;
    if circuit
        .readout
        .as_ref()
        .map(|readout| readout.mode)
        .unwrap_or_default()
        != JsonReadoutMode::MeasureUnmeasured
    {
        return Ok((execution, Vec::new()));
    }

    let mut measured_qubits = vec![false; circuit.qubits];
    let mut used_bits = vec![false; circuit.bits.unwrap_or(0)];
    for gate in &circuit.gates {
        if !matches!(gate.gate.to_ascii_lowercase().as_str(), "measure" | "m") {
            continue;
        }
        let qubit = gate
            .qubit
            .or(gate.target)
            .or_else(|| gate.targets.first().copied());
        if let Some(qubit) = qubit.filter(|qubit| *qubit < circuit.qubits) {
            measured_qubits[qubit] = true;
            let bit = gate.bit.unwrap_or(qubit);
            if used_bits.len() <= bit {
                used_bits.resize(bit + 1, false);
            }
            used_bits[bit] = true;
        }
    }

    let mut implicit = Vec::new();
    for (qubit, measured) in measured_qubits.into_iter().enumerate() {
        if measured {
            continue;
        }
        let bit = if !used_bits.get(qubit).copied().unwrap_or(false) {
            qubit
        } else {
            used_bits
                .iter()
                .position(|used| !used)
                .unwrap_or(used_bits.len())
        };
        if used_bits.len() <= bit {
            used_bits.resize(bit + 1, false);
        }
        used_bits[bit] = true;
        execution.gates.push(JsonGate {
            gate: "measure".to_string(),
            targets: Vec::new(),
            controls: Vec::new(),
            params: Vec::new(),
            target: None,
            control: None,
            qubit: Some(qubit),
            bit: Some(bit),
        });
        implicit.push(ReadoutMapping { qubit, bit });
    }
    execution.bits = Some(used_bits.len());
    Ok((execution, implicit))
}

fn json_circuit_to_qasm_raw(circuit: &JsonCircuit) -> Result<String, String> {
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
    println!("  prismq [serve|designer]                         # http://localhost:8338");
    println!("  prismq run [file.qasm|file.json|-] [--seed 0] [--threshold 1e-10]");
    println!("  prismq probs [file.qasm|file.json|-] [--seed 0] [--threshold 1e-10]");
    println!("  prismq shots [file.qasm|file.json|-] [--shots 1024] [--seed 0]");
    println!("  prismq counts [file.qasm|file.json|-] [--shots 1024] [--seed 0]");
    println!("  prismq inspect [file.qasm|file.json|-]");
    println!("  prismq validate [file.qasm|file.json|-]");
    println!("  prismq draw [file.qasm|file.json|-] [--format text|svg] [--out path]");
    println!("  prismq example [bell-pair|ghz-26|mesh-20|mesh-24|mesh-26]");
    println!();
    println!("With no arguments, prismq starts the integrated web designer on port 8338.");
    println!("With no input path, prismq runs an embedded Bell-state OpenQASM program.");
    println!("Built-in example aliases include `example:bell-pair`, `ghz-26`, and `mesh-24`.");
    println!("Stress ladder: `counts mesh-20`, `counts mesh-24`, then `counts mesh-26`.");
    println!("JSON input is accepted for .json files or stdin beginning with `{{`.");
    println!(
        "The embedded shots/counts program includes measurements; run/probs leaves Bell unmeasured."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bell_document() -> JsonCircuit {
        serde_json::from_str(
            r#"{
                "schema": "prismq.circuit.v1",
                "qubits": 2,
                "bits": 2,
                "seed": 42,
                "shots": 128,
                "threshold": 1e-10,
                "gates": [
                    { "gate": "h", "target": 0 },
                    { "gate": "cx", "control": 0, "target": 1 },
                    { "gate": "measure", "qubit": 0, "bit": 0 },
                    { "gate": "measure", "qubit": 1, "bit": 1 }
                ]
            }"#,
        )
        .expect("Bell JSON should parse")
    }

    #[test]
    fn web_simulation_returns_structured_bell_counts() {
        let result = simulate_json_document(&bell_document()).expect("Bell circuit should run");
        assert!(result.ok);
        assert_eq!(result.schema, "prismq.simulation.v1");
        assert_eq!(result.circuit.qubits, 2);
        assert_eq!(result.circuit.measurements, 2);
        assert_eq!(result.counts.iter().map(|row| row.count).sum::<u64>(), 128);
        assert!(
            result
                .counts
                .iter()
                .all(|row| row.state == "00" || row.state == "11")
        );
    }

    #[test]
    fn unmeasured_hadamard_returns_exact_state_without_fake_classical_counts() {
        let document: JsonCircuit = serde_json::from_value(serde_json::json!({
            "schema": "prismq.circuit.v1",
            "qubits": 1,
            "bits": 1,
            "shots": 128,
            "gates": [{ "gate": "h", "target": 0 }]
        }))
        .unwrap();
        let result = simulate_json_document(&document).expect("Hadamard circuit should run");
        assert_eq!(result.circuit.measurements, 0);
        assert!(result.counts.is_empty());
        assert_eq!(result.probabilities.len(), 2);
        assert!(result.probabilities.iter().all(|row| {
            (row.probability - 0.5).abs() < 1e-9 && matches!(row.state.as_str(), "0" | "1")
        }));
    }

    #[test]
    fn opted_in_readout_materializes_and_samples_missing_measurements() {
        let document: JsonCircuit = serde_json::from_value(serde_json::json!({
            "schema": "prismq.circuit.v1",
            "qubits": 1,
            "bits": 0,
            "shots": 128,
            "gates": [{ "gate": "h", "target": 0 }],
            "readout": { "mode": "measure_unmeasured" }
        }))
        .unwrap();
        let result = simulate_json_document(&document).expect("readout circuit should run");
        assert_eq!(result.circuit.measurements, 1);
        assert_eq!(result.readout.implicit_measurements.len(), 1);
        assert_eq!(result.readout.implicit_measurements[0].qubit, 0);
        assert_eq!(result.readout.implicit_measurements[0].bit, 0);
        assert_eq!(result.counts.iter().map(|row| row.count).sum::<u64>(), 128);
        assert!(
            result
                .counts
                .iter()
                .all(|row| matches!(row.state.as_str(), "0" | "1"))
        );
    }

    #[test]
    fn web_simulation_rejects_unknown_schema() {
        let mut document = bell_document();
        document.schema = "something.else".to_string();
        let error = simulate_json_document(&document).expect_err("schema should be rejected");
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn saved_circuit_names_reserve_revision_suffixes() {
        assert_eq!(
            normalize_circuit_name("  My circuit_2  ").unwrap(),
            "My circuit_2"
        );
        assert!(normalize_circuit_name("../escape").is_err());
        assert!(normalize_circuit_name("My circuit_rev12").is_err());
    }
}

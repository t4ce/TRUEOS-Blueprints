// trueos-blueprint: features=["tokio-net-probe"]

use std::fmt::Write as _;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde_json::{Value, json};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use tokio::path::{Path, PathBuf};
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::path::{Path, PathBuf};

const APP: &str = "shadertoy";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const API_KEY_ENV: &str = "SHADERTOY_API_KEY";
const SHADERTOY_ORIGIN: &str = "https://www.shadertoy.com";
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

type AppResult<T> = Result<T, String>;
type HttpClient = Client<HttpsConnector<HttpConnector>, Full<Bytes>>;

#[derive(Debug)]
struct Options {
    output_root: PathBuf,
    api_key: Option<String>,
    sources: Vec<String>,
    help: bool,
}

struct FetchResult {
    raw: Vec<u8>,
    payload: Value,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    match run().await {
        Ok(()) => {}
        Err(message) => {
            error(message.as_str());
            std::process::exit(1);
        }
    }
}

async fn run() -> AppResult<()> {
    let options = parse_options(process_args().into_iter().skip(1))?;
    if options.help {
        return Ok(());
    }
    if options.sources.is_empty() {
        print_usage();
        return Err(String::from("no Shadertoy URLs or shader IDs supplied"));
    }

    let client = build_client()?;
    let mut failures = Vec::new();
    for source in &options.sources {
        match archive_source(&client, &options, source).await {
            Ok(path) => info(format!("{APP}: saved {}", path.display()).as_str()),
            Err(message) => {
                error(format!("{APP}: {source}: {message}").as_str());
                failures.push(source.as_str());
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} of {} shader fetches failed",
            failures.len(),
            options.sources.len()
        ))
    }
}

async fn archive_source(
    client: &HttpClient,
    options: &Options,
    source: &str,
) -> AppResult<PathBuf> {
    let shader_id = extract_shader_id(source)?;
    info(format!("{APP}: fetching shader id={shader_id}").as_str());

    let fetched = fetch_shader(client, shader_id.as_str(), options.api_key.as_deref()).await?;
    let shader = extract_shader_object(&fetched.payload, shader_id.as_str())?;
    save_shader(
        options.output_root.as_path(),
        source,
        shader_id.as_str(),
        fetched.raw.as_slice(),
        shader,
    )
    .await
}

fn build_client() -> AppResult<HttpClient> {
    let connector = HttpsConnectorBuilder::new()
        .with_provider_and_webpki_roots(rustls_rustcrypto::provider())
        .map_err(|cause| format!("TLS configuration failed: {cause}"))?
        .https_only()
        .enable_http1()
        .build();
    Ok(Client::builder(TokioExecutor::new()).build(connector))
}

async fn fetch_shader(
    client: &HttpClient,
    shader_id: &str,
    api_key: Option<&str>,
) -> AppResult<FetchResult> {
    let request = match api_key {
        Some(key) => {
            let uri = format!(
                "{SHADERTOY_ORIGIN}/api/v1/shaders/{shader_id}?key={}",
                percent_encode(key)
            );
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .header(hyper::header::USER_AGENT, USER_AGENT)
                .header(hyper::header::ACCEPT, "application/json")
                .body(Full::new(Bytes::new()))
                .map_err(|cause| format!("could not build API request: {cause}"))?
        }
        None => {
            let query = json!({ "shaders": [shader_id] }).to_string();
            let body = format!("s={}&nt=1&nl=1&np=1", percent_encode(query.as_str()));
            Request::builder()
                .method(Method::POST)
                .uri(format!("{SHADERTOY_ORIGIN}/shadertoy"))
                .header(hyper::header::USER_AGENT, USER_AGENT)
                .header(hyper::header::ACCEPT, "application/json, text/plain, */*")
                .header(hyper::header::ORIGIN, SHADERTOY_ORIGIN)
                .header(
                    hyper::header::REFERER,
                    format!("{SHADERTOY_ORIGIN}/view/{shader_id}"),
                )
                .header(
                    hyper::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded; charset=UTF-8",
                )
                .body(Full::new(Bytes::from(body)))
                .map_err(|cause| format!("could not build legacy request: {cause}"))?
        }
    };

    let response = tokio::time::timeout(REQUEST_TIMEOUT, client.request(request))
        .await
        .map_err(|_| String::from("request timed out"))?
        .map_err(|cause| format!("request failed: {cause}"))?;
    let status = response.status();
    let cloudflare = response.headers().contains_key("cf-mitigated");
    if response
        .headers()
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err(format!(
            "response exceeds {} byte archive limit",
            MAX_RESPONSE_BYTES
        ));
    }

    let collected = tokio::time::timeout(REQUEST_TIMEOUT, response.into_body().collect())
        .await
        .map_err(|_| String::from("response body timed out"))?
        .map_err(|cause| format!("response body failed: {cause}"))?;
    let raw = collected.to_bytes().to_vec();
    if raw.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "response exceeds {} byte archive limit",
            MAX_RESPONSE_BYTES
        ));
    }

    if !status.is_success() {
        return Err(http_status_error(
            status,
            cloudflare,
            raw.as_slice(),
            api_key.is_some(),
        ));
    }
    if looks_like_cloudflare(raw.as_slice()) {
        return Err(cloudflare_hint(api_key.is_some()));
    }
    let payload = serde_json::from_slice(raw.as_slice())
        .map_err(|cause| format!("Shadertoy returned invalid JSON: {cause}"))?;
    Ok(FetchResult { raw, payload })
}

fn http_status_error(
    status: StatusCode,
    cloudflare: bool,
    body: &[u8],
    used_api_key: bool,
) -> String {
    if cloudflare
        || looks_like_cloudflare(body)
        || (!used_api_key
            && matches!(
                status,
                StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
            ))
    {
        return format!("HTTP {status}: {}", cloudflare_hint(used_api_key));
    }
    let detail = String::from_utf8_lossy(&body[..body.len().min(240)])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if detail.is_empty() {
        format!("Shadertoy returned HTTP {status}")
    } else {
        format!("Shadertoy returned HTTP {status}: {detail}")
    }
}

fn cloudflare_hint(used_api_key: bool) -> String {
    if used_api_key {
        String::from("Shadertoy/Cloudflare rejected the official API request")
    } else {
        format!(
            "Shadertoy/Cloudflare rejected the legacy page request; set {API_KEY_ENV} or pass --api-key"
        )
    }
}

fn looks_like_cloudflare(body: &[u8]) -> bool {
    let sample = String::from_utf8_lossy(&body[..body.len().min(8 * 1024)]).to_lowercase();
    sample.contains("just a moment")
        || sample.contains("cf-mitigated")
        || sample.contains("cloudflare challenge")
}

fn extract_shader_object<'a>(payload: &'a Value, shader_id: &str) -> AppResult<&'a Value> {
    if let Some(array) = payload.as_array() {
        return array
            .iter()
            .find(|item| shader_object_id(item) == Some(shader_id))
            .or_else(|| array.first().filter(|item| item.is_object()))
            .ok_or_else(|| String::from("legacy response contains no shader object"));
    }

    let Some(object) = payload.as_object() else {
        return Err(String::from("response is neither an object nor an array"));
    };
    if object.get("Error").is_some_and(|value| !value.is_null()) {
        return Err(format!("Shadertoy API error: {}", object["Error"]));
    }
    for key in ["Shader", "shader"] {
        if let Some(shader) = object.get(key).filter(|value| value.is_object()) {
            return Ok(shader);
        }
    }
    if object.contains_key("info") && object.contains_key("renderpass") {
        return Ok(payload);
    }
    Err(String::from("response contains no shader object"))
}

fn shader_object_id(shader: &Value) -> Option<&str> {
    shader.get("info")?.get("id")?.as_str()
}

async fn save_shader(
    output_root: &Path,
    source: &str,
    shader_id: &str,
    raw_response: &[u8],
    shader: &Value,
) -> AppResult<PathBuf> {
    let info_value = shader.get("info").cloned().unwrap_or_else(|| json!({}));
    let title = info_value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(shader_id);
    let directory_name = sanitize_component(title, shader_id);
    let shader_dir = resolve_shader_dir(output_root, directory_name.as_str(), shader_id).await;
    let passes_dir = shader_dir.join("passes");
    tokio::fs::create_dir_all(passes_dir.as_path())
        .await
        .map_err(|cause| format!("could not create {}: {cause}", passes_dir.display()))?;

    write_file(shader_dir.join("response.json"), raw_response).await?;
    write_json(shader_dir.join("shader.json"), shader).await?;
    write_json(shader_dir.join("info.json"), &info_value).await?;
    write_file(
        shader_dir.join("id.txt"),
        format!("{shader_id}\n").as_bytes(),
    )
    .await?;
    write_file(
        shader_dir.join("source.txt"),
        format!("{source}\n").as_bytes(),
    )
    .await?;

    let passes = shader
        .get("renderpass")
        .and_then(Value::as_array)
        .ok_or_else(|| String::from("shader has no renderpass array"))?;
    let mut manifest_passes = Vec::with_capacity(passes.len());
    for (index, pass) in passes.iter().enumerate() {
        let pass_object = pass
            .as_object()
            .ok_or_else(|| format!("render pass {index} is not an object"))?;
        let pass_name = pass_object
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| pass_object.get("type").and_then(Value::as_str))
            .unwrap_or("Pass");
        let pass_type = pass_object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let pass_component = sanitize_component(pass_name, "Pass");
        let pass_directory = format!("{index:02}-{pass_component}");
        let pass_dir = passes_dir.join(pass_directory.as_str());
        tokio::fs::create_dir_all(pass_dir.as_path())
            .await
            .map_err(|cause| format!("could not create {}: {cause}", pass_dir.display()))?;

        let code = pass_object
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("");
        let inputs = pass_object
            .get("inputs")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let outputs = pass_object
            .get("outputs")
            .cloned()
            .unwrap_or_else(|| json!([]));
        write_file(pass_dir.join("code.glsl"), code.as_bytes()).await?;
        write_json(pass_dir.join("inputs.json"), &inputs).await?;
        write_json(pass_dir.join("outputs.json"), &outputs).await?;
        write_json(pass_dir.join("pass.json"), pass).await?;

        manifest_passes.push(json!({
            "index": index,
            "name": pass_name,
            "type": pass_type,
            "directory": format!("passes/{pass_directory}")
        }));
    }

    let manifest = json!({
        "schema": 1,
        "id": shader_id,
        "name": title,
        "source": source,
        "passes": manifest_passes
    });
    write_json(shader_dir.join("manifest.json"), &manifest).await?;
    Ok(shader_dir)
}

async fn resolve_shader_dir(output_root: &Path, title: &str, shader_id: &str) -> PathBuf {
    let preferred = output_root.join(title);
    let existing_id = tokio::fs::read_to_string(preferred.join("id.txt"))
        .await
        .ok();
    if existing_id
        .as_deref()
        .is_none_or(|value| value.trim() == shader_id)
    {
        preferred
    } else {
        output_root.join(format!("{title} [{shader_id}]"))
    }
}

async fn write_json(path: PathBuf, value: &Value) -> AppResult<()> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|cause| format!("could not serialize {}: {cause}", path.display()))?;
    bytes.push(b'\n');
    write_file(path, bytes.as_slice()).await
}

async fn write_file(path: PathBuf, bytes: &[u8]) -> AppResult<()> {
    tokio::fs::write(path.as_path(), bytes)
        .await
        .map_err(|cause| format!("could not write {}: {cause}", path.display()))
}

fn extract_shader_id(source: &str) -> AppResult<String> {
    let source = source.trim();
    if source.len() == 6 && source.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Ok(source.to_owned());
    }
    let (_, tail) = source
        .split_once("/view/")
        .ok_or_else(|| format!("invalid Shadertoy URL or shader ID '{source}'"))?;
    let shader_id: String = tail
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric())
        .take(6)
        .collect();
    if shader_id.len() == 6 {
        Ok(shader_id)
    } else {
        Err(format!("invalid Shadertoy URL or shader ID '{source}'"))
    }
}

fn sanitize_component(value: &str, fallback: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.trim().chars() {
        if ch == '/' || ch == '\\' || ch.is_control() {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    if out.is_empty() || out == "." || out == ".." {
        fallback.to_owned()
    } else {
        out
    }
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn parse_options(args: impl Iterator<Item = String>) -> AppResult<Options> {
    let mut output_root = PathBuf::from(".");
    let mut api_key = process_var(API_KEY_ENV).ok().filter(|key| !key.is_empty());
    let mut sources = Vec::new();
    let mut help = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{arg} requires a directory"))?;
                output_root = PathBuf::from(value);
            }
            "--api-key" => {
                let value = args
                    .next()
                    .ok_or_else(|| String::from("--api-key requires a value"))?;
                api_key = Some(value);
            }
            "-h" | "--help" | "help" => {
                print_usage();
                help = true;
                break;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown option '{value}'"));
            }
            _ => sources.push(arg),
        }
    }
    Ok(Options {
        output_root,
        api_key,
        sources,
        help,
    })
}

fn print_usage() {
    info("Shadertoy render-pass archiver");
    info("usage: shadertoy [-o DIR] [--api-key KEY] <url-or-id> [<url-or-id> ...]");
    info(format!("       API key may also be supplied as {API_KEY_ENV}").as_str());
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn process_args() -> Vec<String> {
    trueos::env::args().collect()
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn process_args() -> Vec<String> {
    std::env::args().collect()
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn process_var(key: &str) -> Result<String, trueos::env::VarError> {
    trueos::env::var(key)
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn process_var(key: &str) -> Result<String, std::env::VarError> {
    std::env::var(key)
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn info(message: &str) {
    trueos::logl::log(trueos::logl::level::INFO, message);
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn info(message: &str) {
    println!("{message}");
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn error(message: &str) {
    trueos::logl::log(trueos::logl::level::ERROR, message);
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn error(message: &str) {
    eprintln!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ids_and_view_urls() {
        assert_eq!(extract_shader_id("mslfR2").unwrap(), "mslfR2");
        assert_eq!(
            extract_shader_id("https://www.shadertoy.com/view/mslfR2?foo=bar").unwrap(),
            "mslfR2"
        );
        assert!(extract_shader_id("https://example.com/mslfR2").is_err());
        assert!(extract_shader_id("short").is_err());
    }

    #[test]
    fn extracts_legacy_official_and_direct_payloads() {
        let shader = json!({
            "info": {"id": "mslfR2", "name": "More cubes"},
            "renderpass": []
        });
        assert_eq!(
            shader_object_id(extract_shader_object(&json!([shader.clone()]), "mslfR2").unwrap()),
            Some("mslfR2")
        );
        assert_eq!(
            shader_object_id(
                extract_shader_object(&json!({"Shader": shader.clone()}), "mslfR2").unwrap()
            ),
            Some("mslfR2")
        );
        assert_eq!(
            shader_object_id(extract_shader_object(&shader, "mslfR2").unwrap()),
            Some("mslfR2")
        );
    }

    #[test]
    fn keeps_titles_exact_except_path_separators() {
        assert_eq!(sanitize_component("More cubes", "id"), "More cubes");
        assert_eq!(sanitize_component("a/b\\c", "id"), "a_b_c");
        assert_eq!(sanitize_component("..", "id"), "id");
    }

    #[test]
    fn form_encoding_is_stable() {
        assert_eq!(
            percent_encode(r#"{"shaders":["mslfR2"]}"#),
            "%7B%22shaders%22%3A%5B%22mslfR2%22%5D%7D"
        );
    }

    #[tokio::test]
    async fn archive_keeps_sound_code_and_special_input_descriptors() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "trueos-shadertoy-test-{}-{nonce}",
            std::process::id()
        ));
        let shader = json!({
            "info": {"id": "mslfR2", "name": "More cubes"},
            "renderpass": [
                {
                    "name": "Common",
                    "type": "common",
                    "inputs": [],
                    "outputs": [],
                    "code": "float shared = 1.0;"
                },
                {
                    "name": "Sound",
                    "type": "sound",
                    "inputs": [{
                        "id": "mic",
                        "type": "mic",
                        "channel": 0,
                        "sampler": {"filter": "linear", "wrap": "clamp"}
                    }],
                    "outputs": [],
                    "code": "vec2 mainSound(float t) { return vec2(t); }"
                }
            ]
        });
        let raw = br#"[{"fixture":true}]"#;

        let saved = save_shader(
            root.as_path(),
            "https://www.shadertoy.com/view/mslfR2",
            "mslfR2",
            raw,
            &shader,
        )
        .await
        .unwrap();

        assert_eq!(saved.file_name().unwrap(), "More cubes");
        assert_eq!(
            tokio::fs::read(saved.join("response.json")).await.unwrap(),
            raw
        );
        assert_eq!(
            tokio::fs::read_to_string(saved.join("passes/01-Sound/code.glsl"))
                .await
                .unwrap(),
            "vec2 mainSound(float t) { return vec2(t); }"
        );
        let inputs: Value = serde_json::from_slice(
            tokio::fs::read(saved.join("passes/01-Sound/inputs.json"))
                .await
                .unwrap()
                .as_slice(),
        )
        .unwrap();
        assert_eq!(inputs, shader["renderpass"][1]["inputs"]);

        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}

// trueos-blueprint: features=["ui4-scene"]
//! Finite, production-ABI HTTPS comparison; no alternative HTTP/TLS stack.
mod native;
use serde_json::{Value, json};

const SCHEMA: &str = "download_smoke_app_v1";
const DEFAULT_URLS: &[&str] = &[
    "https://www.wikipedia.org/portal/wikipedia.org/assets/img/Wikipedia-logo-v2.png",
    "https://wikipedia.org/portal/wikipedia.org/assets/img/Wikipedia-logo-v2.png",
    "https://www.wikipedia.org/",
    "https://www.wikipedia.org/portal/wikipedia.org/assets/js/index-8dfbdb3a7a.js",
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Route {
    Image,
    General,
    GeneralHappy,
}
impl Route {
    fn name(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::General => "general",
            Self::GeneralHappy => "general-happy",
        }
    }
}
struct Config {
    urls: Vec<String>,
    routes: Vec<Route>,
    timeout_ms: u64,
    list: bool,
}
impl Config {
    fn parse() -> Result<Self, String> {
        let mut config = Self {
            urls: Vec::new(),
            routes: vec![Route::General, Route::Image],
            timeout_ms: 20_000,
            list: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--vmx-minishell" => {}
                "--list" => config.list = true,
                "--url" => {
                    let url = args.next().ok_or("--url requires an HTTPS URL")?;
                    if !url.starts_with("https://")
                        || url.len() > 4096
                        || url
                            .bytes()
                            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
                    {
                        return Err(String::from("--url requires a bounded HTTPS URL"));
                    }
                    if config.urls.len() == 8 {
                        return Err(String::from("at most eight --url arguments are allowed"));
                    }
                    config.urls.push(url);
                }
                "--route" => {
                    config.routes = match args.next().as_deref() {
                        Some("image") => vec![Route::Image],
                        Some("general") => vec![Route::General],
                        Some("general-happy") => vec![Route::GeneralHappy],
                        Some("both") => vec![Route::General, Route::Image],
                        _ => {
                            return Err(String::from(
                                "--route requires image, general, general-happy, or both",
                            ));
                        }
                    }
                }
                "--timeout-ms" => {
                    config.timeout_ms = args
                        .next()
                        .ok_or("--timeout-ms requires a value")?
                        .parse()
                        .map_err(|_| "invalid timeout")?;
                    if !(1000..=60_000).contains(&config.timeout_ms) {
                        return Err(String::from("timeout must be 1000..60000 ms"));
                    }
                }
                "--help" | "-h" => {
                    return Err(String::from(
                        "download-smoke [--url HTTPS_URL ...] [--route both|image|general|general-happy] [--timeout-ms 20000] [--list]",
                    ));
                }
                _ => return Err(format!("unknown argument: {arg}")),
            }
        }
        if config.urls.is_empty() {
            config.urls = DEFAULT_URLS.iter().map(|url| (*url).to_owned()).collect();
        }
        Ok(config)
    }
}

fn emit(record: Value) {
    let line = format!("download-smoke-app: {record}");
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let _ = trueos::vshell::line(&line);
        let _ = trueos::logl::log_record(
            trueos::logl::level::IMPORTANT,
            "apps/download-smoke",
            line.as_str(),
        );
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    println!("{line}");
}

#[inline]
fn stamp() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let low: u32;
        let high: u32;
        core::arch::asm!("lfence", "rdtsc", out("eax") low, out("edx") high, options(nostack, preserves_flags));
        (u64::from(high) << 32) | u64::from(low)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

fn main() {
    let config = match Config::parse() {
        Ok(config) => config,
        Err(error) => {
            emit(json!({"schema": SCHEMA,"event":"error","error":error}));
            return;
        }
    };
    if config.list {
        emit(
            json!({"schema":SCHEMA,"event":"matrix","urls":config.urls,"routes":config.routes.iter().map(|r| r.name()).collect::<Vec<_>>()}),
        );
        return;
    }
    native::run(&config);
}

fn display_url(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or("")
}

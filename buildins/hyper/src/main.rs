#![no_std]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use trueos::{async_fs as trueosfs, clock::Duration, env, runtime, task::LocalSet, time, vshell};

type Result<T> = core::result::Result<T, String>;

const DOWNLOAD_DIR: &str = "common/dl";
const FETCH_TIMEOUT: Duration = Duration::from_secs(90);
const INPUT_POLL: Duration = Duration::from_millis(5);

fn normalize_url(input: &str) -> Result<reqwest::Url> {
    let input = input.trim();
    if input.is_empty() {
        return Err(String::from("empty URL"));
    }
    let normalized = if input.starts_with("http://") || input.starts_with("https://") {
        String::from(input)
    } else {
        format!("https://{input}")
    };
    let url = reqwest::Url::parse(&normalized).map_err(|error| format!("invalid URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(String::from("only HTTP and HTTPS URLs are supported"));
    }
    Ok(url)
}

fn safe_name(candidate: &str) -> String {
    let sanitized: String = candidate
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() || matches!(sanitized.as_str(), "." | "..") {
        String::from("download.bin")
    } else {
        sanitized
    }
}

fn download_name(url: &reqwest::Url) -> String {
    safe_name(
        url.path_segments()
            .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
            .unwrap_or("download.bin"),
    )
}

async fn download(
    client: &reqwest::Client,
    input: &str,
    requested_name: Option<&str>,
) -> Result<(String, usize)> {
    let url = normalize_url(input)?;
    let name = requested_name
        .map(safe_name)
        .unwrap_or_else(|| download_name(&url));
    let destination = format!("{DOWNLOAD_DIR}/{name}");
    terminal_write(format!("hyper: download {url} -> {destination}\r\n").as_bytes());

    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| format!("request {url}: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("read response body: {error}"))?;
    trueosfs::create_dir_all(DOWNLOAD_DIR.as_bytes())
        .await
        .map_err(|code| format!("create {DOWNLOAD_DIR}: TRUEOSFS error {code}"))?;
    trueosfs::write_file(destination.as_bytes(), bytes.as_ref())
        .await
        .map_err(|code| format!("write {destination}: TRUEOSFS error {code}"))?;
    Ok((destination, bytes.len()))
}

async fn run_minishell(initial_url: Option<String>) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT.as_core_duration())
        .build()
        .map_err(|error| format!("build HTTP client: {error}"))?;
    let mut pending = initial_url;
    loop {
        let input = match pending.take() {
            Some(url) => url,
            None => terminal_read_line("hyper> ").await?,
        };
        let input = input.trim();
        match input {
            "" => continue,
            "exit" | "quit" => return Ok(()),
            "help" | "-h" | "--help" => {
                terminal_write(
                    b"enter URL [filename]; files are saved under common/dl; `exit` returns\r\n",
                );
            }
            line => {
                let mut fields = line.split_whitespace();
                let url = fields.next().expect("non-empty input has a URL");
                let requested_name = fields.next();
                if fields.next().is_some() {
                    terminal_write(b"hyper: usage URL [filename]\r\n");
                    continue;
                }
                match download(&client, url, requested_name).await {
                    Ok((path, bytes)) => terminal_write(
                        format!("hyper: saved {bytes} bytes -> {path}\r\n").as_bytes(),
                    ),
                    Err(error) => {
                        terminal_write(format!("hyper: download failed: {error:#}\r\n").as_bytes())
                    }
                }
            }
        }
    }
}

async fn terminal_read_line(prompt: &str) -> Result<String> {
    terminal_write(prompt.as_bytes());
    let mut bytes = Vec::new();
    loop {
        for byte in terminal_read_available() {
            match byte {
                b'\r' | b'\n' => {
                    terminal_write(b"\r\n");
                    return String::from_utf8(bytes).map_err(|_| String::from("URL is not UTF-8"));
                }
                3 => return Err(String::from("cancelled")),
                8 | 127 if !bytes.is_empty() => {
                    bytes.pop();
                    terminal_write(b"\x08 \x08");
                }
                byte if byte >= 0x20 => {
                    bytes.push(byte);
                    terminal_write(&[byte]);
                }
                _ => {}
            }
        }
        time::sleep(INPUT_POLL.as_core_duration()).await;
    }
}

fn terminal_enter() {
    let size = vshell::konsole_size().unwrap_or(vshell::KonsoleSize { cols: 80, rows: 24 });
    let _ =
        vshell::konsole_begin_frame(size.cols, size.rows, vshell::KONSOLE_FRAME_TERMINAL_HANDOFF);
}

fn terminal_write(bytes: &[u8]) {
    let _ = vshell::attached_write(bytes);
}

fn terminal_read_available() -> Vec<u8> {
    let mut buffer = [0_u8; 4096];
    let read = vshell::attached_read_available(&mut buffer);
    Vec::from(&buffer[..read])
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let initial_url = match args.as_slice() {
        [] => None,
        [url] => Some(url.clone()),
        _ => {
            terminal_write(b"usage: hyper [URL]\r\n");
            return;
        }
    };

    terminal_enter();
    terminal_write(b"hyper: HTTP/HTTPS downloader; destination common/dl\r\n");

    let runtime = runtime::current_thread_net().build();

    match runtime {
        Ok(runtime) => {
            let local = LocalSet::new();
            if let Err(error) = local.block_on(&runtime, run_minishell(initial_url)) {
                terminal_write(format!("hyper: {error:#}\r\n").as_bytes());
            }
            runtime.shutdown_background();
        }
        Err(error) => {
            terminal_write(format!("hyper: runtime initialization failed: {error}\r\n").as_bytes())
        }
    }

    vshell::leave_terminal_handoff();
    let _ = vshell::shutdown_current_blueprint("hyper downloader exited");
}

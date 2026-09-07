use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};

const DOWNLOAD_DIR: &str = "common/dl";
const FETCH_TIMEOUT: Duration = Duration::from_secs(90);
const INPUT_POLL: Duration = Duration::from_millis(5);

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn process_args() -> Vec<String> {
    trueos::env::args().collect()
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn process_args() -> Vec<String> {
    std::env::args().collect()
}

fn normalize_url(input: &str) -> Result<reqwest::Url> {
    let input = input.trim();
    if input.is_empty() {
        bail!("empty URL");
    }
    let normalized = if input.starts_with("http://") || input.starts_with("https://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = reqwest::Url::parse(&normalized).context("invalid URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("only HTTP and HTTPS URLs are supported");
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
) -> Result<(PathBuf, usize)> {
    let url = normalize_url(input)?;
    let name = requested_name
        .map(safe_name)
        .unwrap_or_else(|| download_name(&url));
    let destination = Path::new(DOWNLOAD_DIR).join(name);
    terminal_write(format!("hyper: download {url} -> {}\r\n", destination.display()).as_bytes());

    let response = client
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("request {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("HTTP {}", status.as_u16());
    }
    let bytes = response.bytes().await.context("read response body")?;
    fs::create_dir_all(DOWNLOAD_DIR).context("create common/dl")?;
    fs::write(&destination, &bytes).with_context(|| format!("write {}", destination.display()))?;
    Ok((destination, bytes.len()))
}

async fn run_minishell(initial_url: Option<String>) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .context("build HTTP client")?;
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
                        format!("hyper: saved {bytes} bytes -> {}\r\n", path.display()).as_bytes(),
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
                    return String::from_utf8(bytes).map_err(|_| anyhow!("URL is not UTF-8"));
                }
                3 => bail!("cancelled"),
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
        tokio::time::sleep(INPUT_POLL).await;
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_enter() {
    let size = trueos::vshell::konsole_size()
        .unwrap_or(trueos::vshell::KonsoleSize { cols: 80, rows: 24 });
    let _ = trueos::vshell::konsole_begin_frame(
        size.cols,
        size.rows,
        trueos::vshell::KONSOLE_FRAME_TERMINAL_HANDOFF,
    );
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_enter() {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_write(bytes: &[u8]) {
    let _ = trueos::vshell::attached_write(bytes);
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_write(bytes: &[u8]) {
    let mut output = io::stdout().lock();
    let _ = output.write_all(bytes);
    let _ = output.flush();
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_read_available() -> Vec<u8> {
    let mut buffer = [0_u8; 4096];
    let read = trueos::vshell::attached_read_available(&mut buffer);
    buffer[..read].to_vec()
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_read_available() -> Vec<u8> {
    Vec::new()
}

fn main() {
    let args = process_args().into_iter().skip(1).collect::<Vec<_>>();
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

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    let runtime = trueos::runtime::current_thread_net().build();
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();

    match runtime {
        Ok(runtime) => {
            let local = tokio::task::LocalSet::new();
            if let Err(error) = local.block_on(&runtime, run_minishell(initial_url)) {
                terminal_write(format!("hyper: {error:#}\r\n").as_bytes());
            }
            runtime.shutdown_background();
        }
        Err(error) => {
            terminal_write(format!("hyper: runtime initialization failed: {error}\r\n").as_bytes())
        }
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        trueos::vshell::leave_terminal_handoff();
        let _ = trueos::vshell::shutdown_current_blueprint("hyper downloader exited");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_safe_names_from_urls() {
        let url = normalize_url("example.com/releases/a file.bin?token=secret").unwrap();
        assert_eq!(download_name(&url), "a_20file.bin");
        assert_eq!(
            download_name(&normalize_url("https://example.com/").unwrap()),
            "download.bin"
        );
        assert_eq!(safe_name("../named file.html"), ".._named_file.html");
    }

    #[test]
    fn accepts_only_http_transports() {
        assert_eq!(normalize_url("example.com/file").unwrap().scheme(), "https");
        assert!(normalize_url("ftp://example.com/file").is_err());
    }
}

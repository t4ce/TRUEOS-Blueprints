use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use russh::client::{self, AuthResult, KeyboardInteractiveAuthResponse};
use russh::keys::ssh_key::{Algorithm, HashAlg, LineEnding, PublicKey};
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg, load_secret_key};
use russh::{ChannelMsg, Disconnect, Pty};

const DEFAULT_PORT: u16 = 22;
const DEFAULT_USER: &str = "root";
const INPUT_POLL: Duration = Duration::from_millis(5);
const KNOWN_HOSTS: &str = ".ssh/known_hosts";
const DEFAULT_IDENTITY: &str = ".ssh/id_ed25519";

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn stage_log(stage: &str) {
    trueos::logl::log(
        trueos::logl::level::INFO,
        format_args!("ssh: stage={stage}"),
    );
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn stage_log(_stage: &str) {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn process_args() -> Vec<String> {
    trueos::env::args().collect()
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn process_args() -> Vec<String> {
    std::env::args().collect()
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn process_var(key: &str) -> Option<String> {
    trueos::env::var(key).ok()
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn process_var(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

#[derive(Debug)]
struct Options {
    host: String,
    host_token: String,
    port: u16,
    user: String,
    identity: PathBuf,
    accept_new: bool,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut args = process_args().into_iter().skip(1);
        let mut port = None;
        let mut user = None;
        let mut identity = PathBuf::from(DEFAULT_IDENTITY);
        let mut accept_new = false;
        let mut target = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-p" => {
                    let value = args.next().context("ssh: -p requires a port")?;
                    port = Some(parse_port(&value)?);
                }
                "-l" => user = Some(args.next().context("ssh: -l requires a user")?),
                "-i" => identity = PathBuf::from(args.next().context("ssh: -i requires a path")?),
                "--accept-new" => accept_new = true,
                "-h" | "--help" | "help" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ if arg.starts_with('-') => bail!("ssh: unsupported option {arg}"),
                _ if target.is_none() => target = Some(arg),
                _ => bail!("ssh: only one destination is supported"),
            }
        }

        let target = target.context("ssh: missing destination")?;
        let (target_user, endpoint) = target
            .rsplit_once('@')
            .map_or((None, target.as_str()), |(user, endpoint)| {
                (Some(user.to_owned()), endpoint)
            });
        let (host, endpoint_port) = split_endpoint(endpoint)?;
        let port = port.or(endpoint_port).unwrap_or(DEFAULT_PORT);
        let user = user
            .or(target_user)
            .unwrap_or_else(|| DEFAULT_USER.to_owned());
        let host_token = if port == DEFAULT_PORT {
            host.clone()
        } else {
            format!("[{host}]:{port}")
        };

        Ok(Self {
            host,
            host_token,
            port,
            user,
            identity,
            accept_new,
        })
    }
}

fn split_endpoint(endpoint: &str) -> Result<(String, Option<u16>)> {
    if let Some(bracketed) = endpoint.strip_prefix('[') {
        let (host, rest) = bracketed
            .split_once(']')
            .context("ssh: missing ] in IPv6 destination")?;
        let port = match rest {
            "" => None,
            value => Some(parse_port(
                value
                    .strip_prefix(':')
                    .context("ssh: expected :port after ]")?,
            )?),
        };
        return Ok((host.to_owned(), port));
    }

    match endpoint.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            if host.is_empty() {
                bail!("ssh: empty host");
            }
            Ok((host.to_owned(), Some(parse_port(port)?)))
        }
        _ if endpoint.is_empty() => bail!("ssh: empty host"),
        _ => Ok((endpoint.to_owned(), None)),
    }
}

fn parse_port(value: &str) -> Result<u16> {
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .with_context(|| format!("ssh: invalid port {value:?}"))
}

fn print_usage() {
    terminal_write(
        b"usage: ssh [-p port] [-l user] [-i identity] [--accept-new] [user@]host[:port]\r\n",
    );
}

struct ClientHandler {
    host_token: String,
    accept_new: bool,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        match known_host_status(Path::new(KNOWN_HOSTS), &self.host_token, server_public_key) {
            KnownHostStatus::Match => Ok(true),
            KnownHostStatus::Changed => {
                terminal_write(
                    format!(
                        "\r\nssh: HOST KEY CHANGED for {}; refusing connection\r\n",
                        self.host_token
                    )
                    .as_bytes(),
                );
                Ok(false)
            }
            KnownHostStatus::Unknown => {
                let fingerprint = server_public_key.fingerprint(HashAlg::Sha256);
                terminal_write(
                    format!(
                        "\r\nThe authenticity of host {} is unknown.\r\n{} fingerprint is {}.\r\n",
                        self.host_token,
                        server_public_key.algorithm(),
                        fingerprint
                    )
                    .as_bytes(),
                );
                let accepted = if self.accept_new {
                    true
                } else {
                    matches!(
                        terminal_read_line("Trust this host key? [yes/no] ", true).await,
                        Ok(answer) if answer.eq_ignore_ascii_case("yes")
                    )
                };
                if !accepted {
                    return Ok(false);
                }
                if let Err(err) =
                    learn_known_host(Path::new(KNOWN_HOSTS), &self.host_token, server_public_key)
                {
                    terminal_write(format!("ssh: could not save known host: {err}\r\n").as_bytes());
                    return Ok(false);
                }
                Ok(true)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KnownHostStatus {
    Match,
    Changed,
    Unknown,
}

fn known_host_status(path: &Path, host: &str, key: &PublicKey) -> KnownHostStatus {
    let Ok(text) = fs::read_to_string(path) else {
        return KnownHostStatus::Unknown;
    };
    let mut found_host = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(hosts) = fields.next() else {
            continue;
        };
        if !hosts.split(',').any(|candidate| candidate == host) {
            continue;
        }
        found_host = true;
        let (Some(algorithm), Some(base64)) = (fields.next(), fields.next()) else {
            continue;
        };
        if PublicKey::from_openssh(&format!("{algorithm} {base64}"))
            .is_ok_and(|known| known == *key)
        {
            return KnownHostStatus::Match;
        }
    }
    if found_host {
        KnownHostStatus::Changed
    } else {
        KnownHostStatus::Unknown
    }
}

fn learn_known_host(path: &Path, host: &str, key: &PublicKey) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let encoded = key.to_openssh().context("encode server public key")?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(file, "{host} {encoded}")?;
    Ok(())
}

fn load_or_create_identity(path: &Path) -> Result<PrivateKey> {
    if path.exists() {
        return load_secret_key(path, None)
            .with_context(|| format!("load SSH identity {}", path.display()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .context("generate Ed25519 SSH identity")?;
    let encoded = key
        .to_openssh(LineEnding::LF)
        .context("encode Ed25519 SSH identity")?;
    fs::write(path, encoded.as_bytes())
        .with_context(|| format!("save SSH identity {}", path.display()))?;
    terminal_write(
        format!(
            "ssh: generated Ed25519 identity {}\r\nssh: public key: {}\r\n",
            path.display(),
            key.public_key()
                .to_openssh()
                .unwrap_or_else(|_| String::from("<encoding failed>"))
        )
        .as_bytes(),
    );
    Ok(key)
}

async fn authenticate(
    session: &mut client::Handle<ClientHandler>,
    options: &Options,
) -> Result<()> {
    let identity = load_or_create_identity(&options.identity)?;
    let result = session
        .authenticate_publickey(
            options.user.clone(),
            PrivateKeyWithHashAlg::new(Arc::new(identity), None),
        )
        .await
        .context("public-key authentication")?;
    if result.success() {
        return Ok(());
    }

    let partial = matches!(
        result,
        AuthResult::Failure {
            partial_success: true,
            ..
        }
    );
    if authenticate_keyboard_interactive(session, &options.user).await? {
        return Ok(());
    }
    if partial {
        bail!("server accepted the key but rejected the second factor");
    }

    let password = terminal_read_line(
        &format!("{}@{} password: ", options.user, options.host),
        false,
    )
    .await?;
    let result = session
        .authenticate_password(options.user.clone(), password)
        .await
        .context("password authentication")?;
    if result.success() {
        Ok(())
    } else {
        bail!("authentication failed")
    }
}

async fn authenticate_keyboard_interactive(
    session: &mut client::Handle<ClientHandler>,
    user: &str,
) -> Result<bool> {
    let mut response = session
        .authenticate_keyboard_interactive_start(user.to_owned(), None)
        .await
        .context("start keyboard-interactive authentication")?;
    loop {
        match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                if !name.is_empty() {
                    terminal_write(format!("{name}\r\n").as_bytes());
                }
                if !instructions.is_empty() {
                    terminal_write(format!("{instructions}\r\n").as_bytes());
                }
                let mut answers = Vec::with_capacity(prompts.len());
                for prompt in prompts {
                    answers.push(terminal_read_line(&prompt.prompt, prompt.echo).await?);
                }
                response = session
                    .authenticate_keyboard_interactive_respond(answers)
                    .await
                    .context("answer keyboard-interactive authentication")?;
            }
        }
    }
}

async fn run_session(options: Options) -> Result<()> {
    let config = Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(600)),
        ..Default::default()
    });
    let handler = ClientHandler {
        host_token: options.host_token.clone(),
        accept_new: options.accept_new,
    };
    let mut session = tokio::time::timeout(
        Duration::from_secs(15),
        client::connect(config, (options.host.as_str(), options.port), handler),
    )
    .await
    .context("SSH connect timed out after 15 seconds")?
    .with_context(|| format!("connect {}:{}", options.host, options.port))?;
    stage_log("transport-connected");
    authenticate(&mut session, &options).await?;
    stage_log("authenticated");

    let channel = session
        .channel_open_session()
        .await
        .context("open SSH session channel")?;
    let (mut cols, mut rows) = terminal_size();
    let term = process_var("TERM").unwrap_or_else(|| String::from("xterm-256color"));
    channel
        .request_pty(true, &term, cols, rows, 0, 0, &[(Pty::ECHO, 1)])
        .await
        .context("request remote PTY")?;
    channel
        .request_shell(true)
        .await
        .context("request remote shell")?;

    terminal_write(b"ssh: authenticated; local escape is ~. at the start of a line\r\n");
    let mut escape = LocalEscape::new();
    let mut channel = channel;
    loop {
        tokio::select! {
            message = channel.wait() => {
                match message {
                    Some(ChannelMsg::Data { data }) => terminal_write(data.as_ref()),
                    Some(ChannelMsg::ExtendedData { data, .. }) => terminal_write(data.as_ref()),
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        terminal_write(format!("\r\nssh: remote exit status {exit_status}\r\n").as_bytes());
                        break;
                    }
                    Some(ChannelMsg::Eof | ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            () = tokio::time::sleep(INPUT_POLL) => {
                let input = terminal_read_available();
                if !input.is_empty() {
                    let mut forwarded = Vec::with_capacity(input.len());
                    if escape.forward(&input, &mut forwarded) {
                        terminal_write(b"\r\nssh: disconnected by local escape\r\n");
                        break;
                    }
                    if !forwarded.is_empty() {
                        channel.data(forwarded.as_slice()).await.context("send SSH channel data")?;
                    }
                }
                let size = terminal_size();
                if size != (cols, rows) {
                    (cols, rows) = size;
                    channel.window_change(cols, rows, 0, 0).await.context("resize remote PTY")?;
                }
            }
        }
    }

    let _ = channel.eof().await;
    let _ = session
        .disconnect(Disconnect::ByApplication, "terminal closed", "en")
        .await;
    Ok(())
}

#[derive(Default)]
struct LocalEscape {
    at_line_start: bool,
    pending_tilde: bool,
}

impl LocalEscape {
    fn new() -> Self {
        Self {
            at_line_start: true,
            pending_tilde: false,
        }
    }

    fn forward(&mut self, input: &[u8], output: &mut Vec<u8>) -> bool {
        for &byte in input {
            if self.pending_tilde {
                self.pending_tilde = false;
                match byte {
                    b'.' => return true,
                    b'~' => {
                        output.push(b'~');
                        self.at_line_start = false;
                        continue;
                    }
                    _ => {
                        output.push(b'~');
                        self.at_line_start = false;
                    }
                }
            } else if self.at_line_start && byte == b'~' {
                self.pending_tilde = true;
                continue;
            }
            output.push(byte);
            self.at_line_start = matches!(byte, b'\r' | b'\n');
        }
        false
    }
}

async fn terminal_read_line(prompt: &str, echo: bool) -> Result<String> {
    terminal_write(prompt.as_bytes());
    let mut bytes = Vec::new();
    loop {
        for byte in terminal_read_available() {
            match byte {
                b'\r' | b'\n' => {
                    terminal_write(b"\r\n");
                    return String::from_utf8(bytes).context("terminal response is not UTF-8");
                }
                3 => bail!("cancelled"),
                8 | 127 if !bytes.is_empty() => {
                    bytes.pop();
                    if echo {
                        terminal_write(b"\x08 \x08");
                    }
                }
                byte if byte >= 0x20 => {
                    bytes.push(byte);
                    if echo {
                        terminal_write(&[byte]);
                    }
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
    let mut buf = [0u8; 4096];
    let read = trueos::vshell::attached_read_available(&mut buf);
    buf[..read].to_vec()
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_read_available() -> Vec<u8> {
    Vec::new()
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_size() -> (u32, u32) {
    trueos::vshell::konsole_size()
        .map(|size| (size.cols, size.rows))
        .unwrap_or((80, 24))
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_size() -> (u32, u32) {
    (80, 24)
}

fn main() {
    stage_log("main-enter");
    let options = match Options::parse() {
        Ok(options) => options,
        Err(err) => {
            terminal_write(format!("{err:#}\r\n").as_bytes());
            print_usage();
            return;
        }
    };
    stage_log("options-parsed");
    terminal_enter();
    terminal_write(
        format!(
            "ssh: connecting to {}:{} as {}\r\n",
            options.host, options.port, options.user
        )
        .as_bytes(),
    );
    stage_log("terminal-entered");

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    let runtime = trueos::runtime::current_thread_net().build();
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();

    match runtime {
        Ok(runtime) => {
            stage_log("runtime-ready");
            let local = tokio::task::LocalSet::new();
            let result = local.block_on(&runtime, run_session(options));
            runtime.shutdown_background();
            if let Err(err) = result {
                terminal_write(format!("\r\nssh: {err:#}\r\n").as_bytes());
            }
        }
        Err(err) => {
            terminal_write(format!("ssh: runtime initialization failed: {err}\r\n").as_bytes())
        }
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        stage_log("session-ended");
        trueos::vshell::leave_terminal_handoff();
        let _ = trueos::vshell::shutdown_current_blueprint("ssh session ended");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_accepts_shell2_and_ipv6_forms() {
        assert_eq!(
            split_endpoint("192.168.178.94:4548").unwrap(),
            ("192.168.178.94".into(), Some(4548))
        );
        assert_eq!(
            split_endpoint("192.168.178.111").unwrap(),
            ("192.168.178.111".into(), None)
        );
        assert_eq!(
            split_endpoint("[fe80::1]:2222").unwrap(),
            ("fe80::1".into(), Some(2222))
        );
        assert_eq!(split_endpoint("fe80::1").unwrap(), ("fe80::1".into(), None));
    }

    #[test]
    fn endpoint_rejects_a_bare_trailing_colon() {
        assert!(split_endpoint("192.168.178.111:").is_err());
    }

    #[test]
    fn local_escape_only_disconnects_at_line_start() {
        let mut escape = LocalEscape::new();
        let mut output = Vec::new();
        assert!(!escape.forward(b"echo ~. stays\r\n", &mut output));
        assert_eq!(output, b"echo ~. stays\r\n");

        output.clear();
        assert!(escape.forward(b"~.", &mut output));
        assert!(output.is_empty());
    }
}

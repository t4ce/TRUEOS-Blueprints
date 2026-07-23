use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use russh::keys::ssh_key::{Algorithm, LineEnding, PublicKey};
use russh::keys::PrivateKey;
use russh::server::{self, Msg, Server as _, Session};
use russh::{Channel, ChannelId, MethodKind, MethodSet};

const DEFAULT_PORT: u16 = 22;
const HOST_KEY_PATH: &str = ".ssh/ssh_host_ed25519_key";
const AUTHORIZED_KEYS_PATH: &str = ".ssh/authorized_keys";
const OUTPUT_POLL: Duration = Duration::from_millis(5);

#[derive(Clone)]
struct SshServer {
    authorized: Arc<Vec<PublicKey>>,
}

struct Client {
    authorized: Arc<Vec<PublicKey>>,
    channel: Option<ChannelId>,
    bridge: Option<u32>,
    cols: u32,
    rows: u32,
}

impl server::Server for SshServer {
    type Handler = Client;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
        Client {
            authorized: self.authorized.clone(),
            channel: None,
            bridge: None,
            cols: 80,
            rows: 24,
        }
    }
}

impl server::Handler for Client {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        Ok(if user == "root" && self.authorized.iter().any(|known| known == key) {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.channel.is_some() {
            reply.reject(
                russh::ChannelOpenFailure::ResourceShortage,
                "one session channel per connection",
            )
            .await;
        } else {
            self.channel = Some(channel.id());
            reply.accept().await;
        }
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _: &str,
        col_width: u32,
        row_height: u32,
        _: u32,
        _: u32,
        _: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.cols = col_width.max(1);
        self.rows = row_height.max(1);
        session.channel_success(channel);
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let Some(bridge) = trueos::vshell::ssh_shell_open(self.cols, self.rows) else {
            session.channel_failure(channel);
            return Ok(());
        };
        self.bridge = Some(bridge);
        session.channel_success(channel);
        let handle = session.handle();
        tokio::spawn(async move {
            let mut output = vec![0u8; 32 * 1024];
            loop {
                match trueos::vshell::ssh_shell_read(bridge, &mut output) {
                    Ok(0) => tokio::time::sleep(OUTPUT_POLL).await,
                    Ok(read) => {
                        if handle.data(channel, output[..read].to_vec()).await.is_err() {
                            break;
                        }
                    }
                    Err(()) => break,
                }
            }
            let _ = trueos::vshell::ssh_shell_close(bridge);
        });
        Ok(())
    }

    async fn data(
        &mut self,
        _: ChannelId,
        data: &[u8],
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(bridge) = self.bridge {
            let _ = trueos::vshell::ssh_shell_write(bridge, data);
        }
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _: ChannelId,
        col_width: u32,
        row_height: u32,
        _: u32,
        _: u32,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.cols = col_width.max(1);
        self.rows = row_height.max(1);
        if let Some(bridge) = self.bridge {
            let _ = trueos::vshell::ssh_shell_resize(bridge, self.cols, self.rows);
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        _: ChannelId,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.close_bridge();
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        _: ChannelId,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.close_bridge();
        Ok(())
    }
}

impl Client {
    fn close_bridge(&mut self) {
        if let Some(bridge) = self.bridge.take() {
            let _ = trueos::vshell::ssh_shell_close(bridge);
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.close_bridge();
    }
}

fn load_or_create_host_key(path: &Path) -> Result<PrivateKey> {
    if path.exists() {
        return russh::keys::load_secret_key(path, None)
            .with_context(|| format!("load host key {}", path.display()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .context("generate Ed25519 host key")?;
    let encoded = key.to_openssh(LineEnding::LF)?;
    fs::write(path, encoded.as_bytes())?;
    Ok(key)
}

fn load_authorized_keys(path: &Path) -> Result<Vec<PublicKey>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("no authorized keys; run `sshd authorize <public-key>` first"))?;
    let keys: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| PublicKey::from_openssh(line).ok())
        .collect();
    if keys.is_empty() {
        bail!("authorized_keys contains no valid SSH public keys");
    }
    Ok(keys)
}

fn authorize(args: &[String]) -> Result<()> {
    let line = args.join(" ");
    let key = PublicKey::from_openssh(&line).context("invalid OpenSSH public key")?;
    let canonical = key.to_openssh()?;
    let path = Path::new(AUTHORIZED_KEYS_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = fs::read_to_string(path).unwrap_or_default();
    if existing
        .lines()
        .filter_map(|line| PublicKey::from_openssh(line).ok())
        .any(|known| known == key)
    {
        println!("sshd: key already authorized");
        return Ok(());
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&canonical);
    next.push('\n');
    fs::write(path, next)?;
    println!("sshd: authorized {}", key.fingerprint(Default::default()));
    Ok(())
}

async fn serve(port: u16) -> Result<()> {
    let authorized = Arc::new(load_authorized_keys(Path::new(AUTHORIZED_KEYS_PATH))?);
    let host_key = load_or_create_host_key(Path::new(HOST_KEY_PATH))?;
    let config = Arc::new(server::Config {
        server_id: russh::SshId::Standard("SSH-2.0-TRUEOS".into()),
        methods: MethodSet::from([MethodKind::PublicKey]),
        keys: vec![host_key],
        auth_rejection_time: Duration::from_secs(1),
        inactivity_timeout: Some(Duration::from_secs(3600)),
        ..Default::default()
    });
    let address = SocketAddr::from(([0, 0, 0, 0], port));
    println!("sshd: listening on {address}; auth=ed25519-publickey shell=Shell2");
    let listener = tokio::net::TcpListener::bind(address).await?;
    let mut server = SshServer { authorized };
    server.run_on_socket(config, &listener).await?;
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "authorize") {
        if let Err(err) = authorize(&args[1..]) {
            eprintln!("sshd: {err:#}");
        }
        return;
    }
    let port = args
        .windows(2)
        .find(|pair| pair[0] == "-p")
        .and_then(|pair| pair[1].parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    let runtime = match trueos::runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("sshd: runtime initialization failed: {err}");
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    if let Err(err) = local.block_on(&runtime, serve(port)) {
        eprintln!("sshd: {err:#}");
    }
}

use idedit_htmlbuild_srv::Preview;
use std::{net::Ipv4Addr, time::Duration};
use trueos_ide::{
    LoadedSource,
    html_preview::{Config, PROTOCOL, READY_PREFIX, Ready},
    store_protocol::{Command, Reply},
};

struct SourceStore {
    config: Config,
}
impl SourceStore {
    async fn call(&self, command: Command) -> Result<Reply, String> {
        #[cfg(target_os = "trueos")]
        {
            let request = serde_json::to_vec(&trueos_ide::store_protocol::Request {
                database: self.config.database.clone(),
                command,
            })
            .map_err(|e| e.to_string())?;
            trueos::async_fs::write_file(trueos_ide::store_protocol::REQUEST.as_bytes(), &request)
                .await
                .map_err(|e| format!("IDE source request failed ({e})"))?;
            let bytes = trueos::async_fs::read_file(trueos_ide::store_protocol::REPLY.as_bytes())
                .await
                .map_err(|e| format!("IDE source reply failed ({e})"))?;
            if bytes.len() > trueos_ide::store_protocol::MAX_FRAME_BYTES {
                return Err("Source reply too large".into());
            }
            let reply = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if let Reply::Error { code, message } = reply {
                return Err(format!("{message} ({code})"));
            }
            Ok(reply)
        }
        #[cfg(not(target_os = "trueos"))]
        {
            // Host smoke tests use the same redb RAM-image schema. On TRUEOS
            // the existing source service owns this image; we never reopen it.
            let bytes = tokio::fs::read(&self.config.database)
                .await
                .map_err(|e| e.to_string())?;
            let image =
                trueos_ide::source_image::SourceImage::open(&bytes).map_err(|e| e.to_string())?;
            match command {
                Command::List {} => image
                    .database
                    .list_projects()
                    .map(|projects| Reply::Projects { projects }),
                Command::Get { project, revision } => image
                    .database
                    .get(&project, revision)
                    .map(|source| Reply::Source { source }),
                _ => return Err("Preview only reads project sources".into()),
            }
            .map_err(|e| e.to_string())
        }
    }
    async fn latest(&self) -> Result<u64, String> {
        let Reply::Projects { projects } = self.call(Command::List {}).await? else {
            return Err("Invalid project list reply".into());
        };
        projects
            .into_iter()
            .find(|p| p.name == self.config.project)
            .and_then(|p| p.latest_revision)
            .ok_or_else(|| "HTML project no longer exists".into())
    }
    async fn load(&self, revision: u64) -> Result<LoadedSource, String> {
        let Reply::Source {
            source: Some(source),
        } = self
            .call(Command::Get {
                project: self.config.project.clone(),
                revision: Some(revision),
            })
            .await?
        else {
            return Err("HTML project revision is unavailable".into());
        };
        Ok(source)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    #[cfg(target_os = "trueos")]
    let script = trueos::async_fs::read_file_utf8(b"vFile:launch")
        .await
        .map_err(|e| format!("Cannot read IDEdit startup script ({e})"))?;
    #[cfg(not(target_os = "trueos"))]
    let script = tokio::fs::read_to_string(
        std::env::args()
            .nth(1)
            .ok_or("Usage: idedit_htmlbuild_srv startup-script")?,
    )
    .await
    .map_err(|e| e.to_string())?;
    let config = Config::parse(&script)?;
    let store = SourceStore {
        config: config.clone(),
    };
    let source = store.load(store.latest().await?).await?;
    let preview = Preview::new(config.entry.clone(), source)?;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let entry = config
        .entry
        .split('/')
        .map(|part| {
            percent_encoding::utf8_percent_encode(part, percent_encoding::NON_ALPHANUMERIC)
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("/");
    let ready = Ready {
        protocol: PROTOCOL.into(),
        endpoint: config.endpoint,
        url: format!("http://127.0.0.1:{port}/{entry}"),
    };
    println!(
        "{READY_PREFIX}{}",
        serde_json::to_string(&ready).map_err(|e| e.to_string())?
    );
    // Read revisions from the IDE's shared RAM store, never watch a directory.
    let updates = preview.clone();
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_millis(500));
        loop {
            timer.tick().await;
            let update = async {
                let revision = store.latest().await?;
                if revision != updates.revision() {
                    updates.replace(store.load(revision).await?)?;
                }
                Ok::<_, String>(())
            }
            .await;
            if let Err(error) = update {
                eprintln!("idedit_htmlbuild_srv: {error}");
            }
        }
    });
    axum::serve(listener, idedit_htmlbuild_srv::router(preview))
        .await
        .map_err(|e| e.to_string())
}

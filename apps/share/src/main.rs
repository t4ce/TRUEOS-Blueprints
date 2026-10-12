use std::env;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let root = env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        env::var_os("TRUEOS_APP_FS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("share-data"))
    });
    let address: SocketAddr = env::var("TRUEOS_SHARE_BIND")
        .unwrap_or_else(|_| "0.0.0.0:4918".into())
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let handler = share::handler(&root).await?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    eprintln!(
        "share: serving {} at http://{}/ (authentication disabled)",
        root.display(),
        listener.local_addr()?
    );
    share::http::serve(listener, handler).await
}

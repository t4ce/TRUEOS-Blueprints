//! Stage the pinned Rama TCP provider with native TRUEOS socket ownership.
//! Host source paths remain selected behind their original platform APIs.
use std::path::{Path, PathBuf};

const NATIVE: &str = include_str!("rama_tcp_native.rs");

pub(crate) fn stage(work_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "rama-tcp"
        || crate::package_version(&manifest)?.as_deref() != Some("0.3.0-alpha.4")
    {
        return Err(format!(
            "native Rama TCP overlay requires original rama-tcp 0.3.0-alpha.4 source: {}",
            source.display()
        ));
    }
    let staged = work_dir
        .join("source-overlay-crates")
        .join("rama-tcp-0.3.0-alpha.4-trueos");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    patch(&staged)?;
    Ok(staged)
}

fn patch(staged: &Path) -> Result<(), String> {
    replace(
        staged,
        "src/lib.rs",
        r###"pub mod client;"###,
        r###"#[cfg(target_os = "trueos")]
mod trueos_native;

pub mod client;"###,
    )?;
    replace(
        staged,
        "src/client/connect.rs",
        r###"        let opts = self.clone();
        tokio::task::spawn_blocking(move || tcp_connect_with_socket_opts(&opts, addr))
            .await
            .context("wait for blocking tcp bind using custom socket opts")?"###,
        r###"        #[cfg(target_os = "trueos")]
        { return crate::trueos_native::connect(self, addr).await.map(Into::into).map_err(OpaqueError::from_std); }
        #[cfg(not(target_os = "trueos"))]
        {
        let opts = self.clone();
        tokio::task::spawn_blocking(move || tcp_connect_with_socket_opts(&opts, addr))
            .await
            .context("wait for blocking tcp bind using custom socket opts")?
        }"###,
    )?;
    replace(
        staged,
        "src/client/connect.rs",
        r###"        tokio::task::spawn_blocking(move || tcp_connect_with_socket_opts(&opts, addr))
            .await
            .context("wait for blocking tcp bind using provided Socket Address")?"###,
        r###"        #[cfg(target_os = "trueos")]
        { return crate::trueos_native::connect(&opts, addr).await.map(Into::into).map_err(OpaqueError::from_std); }
        #[cfg(not(target_os = "trueos"))]
        {
        tokio::task::spawn_blocking(move || tcp_connect_with_socket_opts(&opts, addr))
            .await
            .context("wait for blocking tcp bind using provided Socket Address")?
        }"###,
    )?;
    replace(
        staged,
        "src/client/connect.rs",
        r###"    let socket = opts
        .try_build_socket()
        .context("try to build TCP socket's underlying OS socket")?;
    socket
        .connect(&addr.into())
        .context("connect to the provided socket addr")?;
    socket
        .set_nonblocking(true)
        .context("set socket non-blocking")?;
    let stream = tokio::net::TcpStream::from_std(std::net::TcpStream::from(socket))
        .context("create tokio tcp stream from created raw (tcp) socket")?;

    Ok(stream.into())"###,
        r###"    #[cfg(target_os = "trueos")]
    { let _ = (opts, addr); return Err(OpaqueError::from_std(crate::trueos_native::unsupported("native TCP does not accept blocking Unix/socket2 connectors"))); }
    #[cfg(not(target_os = "trueos"))]
    {
    let socket = opts
        .try_build_socket()
        .context("try to build TCP socket's underlying OS socket")?;
    socket
        .connect(&addr.into())
        .context("connect to the provided socket addr")?;
    socket
        .set_nonblocking(true)
        .context("set socket non-blocking")?;
    let stream = tokio::net::TcpStream::from_std(std::net::TcpStream::from(socket))
        .context("create tokio tcp stream from created raw (tcp) socket")?;

    Ok(stream.into())
    }"###,
    )?;
    replace(
        staged,
        "src/server/listener.rs",
        r###"                let socket = opts
                    .try_build_socket()
                    .context("build socket from options")?;
                socket
                    .listen(4096)
                    .context("mark the socket as ready to accept incoming connection requests")?;
                self.bind_socket(socket).await"###,
        r###"                #[cfg(target_os = "trueos")]
                {
                    if self.ttl.is_some() { return Err(crate::trueos_native::unsupported("native TCP listener TTL inheritance is unsupported").into()); }
                    return Ok(TcpListener { inner: crate::trueos_native::listener(&opts)? });
                }
                #[cfg(not(target_os = "trueos"))]
                {
                let socket = opts
                    .try_build_socket()
                    .context("build socket from options")?;
                socket
                    .listen(4096)
                    .context("mark the socket as ready to accept incoming connection requests")?;
                self.bind_socket(socket).await
                }"###,
    )?;
    replace(
        staged,
        "src/server/listener.rs",
        r###"    let listener = std::net::TcpListener::from(socket);
    listener
        .set_nonblocking(true)
        .context("set socket as non-blocking")?;
    Ok(TcpListener {
        inner: TokioTcpListener::from_std(listener)?,
    })"###,
        r###"    #[cfg(target_os = "trueos")]
    { let _ = socket; return Err(crate::trueos_native::unsupported("native TCP cannot import a Unix/socket2 listener").into()); }
    #[cfg(not(target_os = "trueos"))]
    {
    let listener = std::net::TcpListener::from(socket);
    listener
        .set_nonblocking(true)
        .context("set socket as non-blocking")?;
    Ok(TcpListener {
        inner: TokioTcpListener::from_std(listener)?,
    })
    }"###,
    )?;
    replace(
        staged,
        "src/server/listener.rs",
        r###"        let listener = std::net::TcpListener::from(value);
        listener.try_into()"###,
        r###"        #[cfg(target_os = "trueos")]
        { let _ = value; return Err(crate::trueos_native::unsupported("native TCP cannot import a Unix/socket2 listener")); }
        #[cfg(not(target_os = "trueos"))]
        {
        let listener = std::net::TcpListener::from(value);
        listener.try_into()
        }"###,
    )?;
    replace(
        staged,
        "src/server/listener.rs",
        r###"        value.set_nonblocking(true)?;
        Ok(Self {
            inner: TokioTcpListener::from_std(value)?,
        })"###,
        r###"        #[cfg(target_os = "trueos")]
        { let _ = value; return Err(crate::trueos_native::unsupported("native TCP cannot import a Unix listener")); }
        #[cfg(not(target_os = "trueos"))]
        {
        value.set_nonblocking(true)?;
        Ok(Self {
            inner: TokioTcpListener::from_std(value)?,
        })
        }"###,
    )?;
    replace(
        staged,
        "src/server/listener.rs",
        r###"#[cfg(target_family = "unix")]
mod unix_fd {"###,
        r###"#[cfg(all(target_family = "unix", not(target_os = "trueos")))]
mod unix_fd {"###,
    )?;
    std::fs::write(staged.join("src/trueos_native.rs"), NATIVE)
        .map_err(|error| format!("write native Rama TCP provider: {error}"))?;
    Ok(())
}

fn replace(staged: &Path, relative: &str, before: &str, after: &str) -> Result<(), String> {
    let path = staged.join(relative);
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    if source.matches(before).count() != 1 {
        return Err(format!(
            "pinned native Rama TCP boundary drifted in {}",
            path.display()
        ));
    }
    std::fs::write(&path, source.replacen(before, after, 1))
        .map_err(|error| format!("write {}: {error}", path.display()))
}

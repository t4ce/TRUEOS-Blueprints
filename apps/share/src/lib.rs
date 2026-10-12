mod filesystem;
pub mod http;

use std::io;
use std::path::Path;

use dav_server::{DavHandler, memls::MemLs};

/// Build an unauthenticated, read/write share rooted at one directory.
pub async fn handler(root: impl AsRef<Path>) -> io::Result<DavHandler> {
    Ok(DavHandler::builder()
        .filesystem(Box::new(filesystem::ShareFs::new(root.as_ref()).await?))
        .locksystem(MemLs::new())
        .hide_symlinks(true)
        .autoindex(true)
        .build_handler())
}

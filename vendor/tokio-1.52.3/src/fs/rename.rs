#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use crate::fs::asyncify;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use alloc::borrow::ToOwned;

use crate::io;
use crate::path::Path;

/// Renames a file or directory to a new name. On TRUEOS, an existing `to`
/// returns `AlreadyExists`; replacement is not supported by its native rename.
/// On other platforms, an existing file may be replaced.
///
/// This will not work if the new name is on a different mount point.
///
/// This is an async version of [`std::fs::rename`].
pub async fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let from = from.as_ref().to_owned();
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let to = to.as_ref().to_owned();

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    return crate::fs::trueos::rename(from.as_ref(), to.as_ref()).await;
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    asyncify(move || std::fs::rename(from, to)).await
}

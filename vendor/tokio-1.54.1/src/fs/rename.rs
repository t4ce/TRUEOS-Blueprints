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
<<<<<<< /home/t4ce/Repos/TRUEOS-Blueprints/vendor/tokio-1.52.3/src/fs/rename.rs
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let from = from.as_ref().to_owned();
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let to = to.as_ref().to_owned();
||||||| /tmp/trueos-kernel-crate-audit/sources/tokio-1.52.3/src/fs/rename.rs
    let from = from.as_ref().to_owned();
    let to = to.as_ref().to_owned();
=======
    let from = from.as_ref();
    let to = to.as_ref();
>>>>>>> /home/t4ce/Repos/TRUEOS-Blueprints/vendor/tokio-1.54.1/src/fs/rename.rs

<<<<<<< /home/t4ce/Repos/TRUEOS-Blueprints/vendor/tokio-1.52.3/src/fs/rename.rs
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    return crate::fs::trueos::rename(from.as_ref(), to.as_ref()).await;
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
||||||| /tmp/trueos-kernel-crate-audit/sources/tokio-1.52.3/src/fs/rename.rs
=======
    #[cfg(all(
        tokio_unstable,
        feature = "io-uring",
        feature = "rt",
        feature = "fs",
        target_os = "linux",
    ))]
    {
        use crate::io::uring::rename::Rename;
        use crate::runtime::driver::op::Op;

        type RenameOp = Op<Rename>;

        if let Ok(handle) = crate::runtime::Handle::try_current() {
            if let Some(driver_handle) = handle.inner.driver().io.as_ref() {
                if driver_handle
                    .check_and_init(io_uring::opcode::RenameAt::CODE)
                    .await?
                {
                    return RenameOp::rename(from, to)?.await;
                }
            }
        }
    }

    rename_blocking(from, to).await
}

async fn rename_blocking(from: &Path, to: &Path) -> io::Result<()> {
    let [from, to] = [from, to].map(Path::to_owned);
>>>>>>> /home/t4ce/Repos/TRUEOS-Blueprints/vendor/tokio-1.54.1/src/fs/rename.rs
    asyncify(move || std::fs::rename(from, to)).await
}

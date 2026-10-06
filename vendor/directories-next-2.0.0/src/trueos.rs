use std::{env, ffi::OsString, path::PathBuf};

use crate::UserDirs;

pub use crate::lin::{base_dirs, project_dirs_from, project_dirs_from_path};

/// Resolves the TRUEOS pictures directory without probing the filesystem.
fn picture_dir(value: Option<OsString>) -> PathBuf {
    value
        .and_then(dirs_sys_next::is_absolute_path)
        .unwrap_or_else(|| PathBuf::from("/screenshots"))
}

pub fn user_dirs() -> Option<UserDirs> {
    let mut user_dirs = crate::lin::user_dirs()?;
    user_dirs.picture_dir = Some(picture_dir(env::var_os("XDG_PICTURES_DIR")));
    Some(user_dirs)
}

#[cfg(test)]
mod tests {
    use super::picture_dir;
    use std::{ffi::OsString, path::PathBuf};

    #[test]
    fn absolute_platform_override_is_used() {
        assert_eq!(
            picture_dir(Some(OsString::from("/var/lib/trueos/screenshots"))),
            PathBuf::from("/var/lib/trueos/screenshots")
        );
    }

    #[test]
    fn missing_or_relative_override_uses_default() {
        assert_eq!(picture_dir(None), PathBuf::from("/screenshots"));
        assert_eq!(
            picture_dir(Some(OsString::from("Pictures"))),
            PathBuf::from("/screenshots")
        );
    }
}

use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(crate) enum CargoProfile {
    Dev,
    Release,
}

impl CargoProfile {
    pub(crate) fn target_subdir(self) -> &'static str {
        match self {
            CargoProfile::Dev => "debug",
            CargoProfile::Release => "release",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            CargoProfile::Dev => "dev",
            CargoProfile::Release => "release",
        }
    }
}

pub(crate) fn parse_cli_args(
    args: &[std::ffi::OsString],
) -> Result<(PathBuf, Vec<String>, CargoProfile), String> {
    let mut cargo_profile = CargoProfile::Release;
    let mut filtered_args = Vec::with_capacity(args.len());
    for arg in args {
        if arg == "--release" {
            cargo_profile = CargoProfile::Release;
        } else if arg.to_str().is_some_and(|arg| arg.trim().is_empty()) {
            continue;
        } else {
            filtered_args.push(arg.clone());
        }
    }

    if filtered_args.is_empty() {
        return Ok((PathBuf::from("."), Vec::new(), cargo_profile));
    }

    let first = PathBuf::from(&filtered_args[0]);
    if first.join("Cargo.toml").is_file() {
        if filtered_args.len() > 1 {
            return Err("directory mode does not accept app names".to_string());
        }
        return Ok((first, Vec::new(), cargo_profile));
    }

    let mut app_names = Vec::with_capacity(filtered_args.len());
    for arg in filtered_args {
        app_names.push(
            arg.into_string()
                .map_err(|_| "app name must be valid UTF-8".to_string())?,
        );
    }

    Ok((PathBuf::from("."), app_names, cargo_profile))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn parse_cli_args_ignores_blank_app_names() {
        let args = [OsString::from(""), OsString::from("   ")];

        let (app_dir, app_names, profile) = parse_cli_args(&args).unwrap();

        assert_eq!(app_dir, PathBuf::from("."));
        assert!(app_names.is_empty());
        assert!(matches!(profile, CargoProfile::Release));
    }

    #[test]
    fn parse_cli_args_keeps_nonblank_app_names() {
        let args = [OsString::from("hello_world")];

        let (app_dir, app_names, profile) = parse_cli_args(&args).unwrap();

        assert_eq!(app_dir, PathBuf::from("."));
        assert_eq!(app_names, vec!["hello_world"]);
        assert!(matches!(profile, CargoProfile::Release));
    }
}

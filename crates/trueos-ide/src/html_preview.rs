//! IDEdit's on-demand HTML development Blueprint contract.
use alloc::{format, string::String};
use serde::{Deserialize, Serialize};

pub const PROGRAM: &str = "idedit_htmlbuild_srv.bp";
pub const PROTOCOL: &str = "idedit-htmlbuild/v1";
pub const READY_PREFIX: &str = "idedit_htmlbuild_srv: READY ";
pub const REVISION_PATH: &str = "/__idedit/revision";
pub const REVISION_MARKER: &str = "<!-- idedit_htmlbuild_srv revision=";
pub const HTML: &str = "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>HTML project</title>\n  <link rel=\"stylesheet\" href=\"./style.css\">\n  <script src=\"./main.js\" defer></script>\n</head>\n<body>\n</body>\n</html>\n";
pub const JS: &str = "document.addEventListener('DOMContentLoaded', () => {\n  console.log('template console log');\n}, { once: true });\n";
pub const CSS: &str = "body { background-color: #f4f4f4; }\n";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ready {
    pub protocol: String,
    pub endpoint: u64,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub endpoint: u64,
    pub database: String,
    pub project: String,
    pub entry: String,
}

impl Config {
    pub fn script(&self) -> Result<String, String> {
        self.validate()?;
        Ok(format!(
            "fs-scope trueosfs\nide-endpoint {}\nide-db {}\nide-project {}\nide-entry {}\n",
            self.endpoint, self.database, self.project, self.entry
        ))
    }
    pub fn parse(script: &str) -> Result<Self, String> {
        let (mut endpoint, mut database, mut project, mut entry) = (None, None, None, None);
        for line in script.lines() {
            if line == "fs-scope trueosfs" || line.is_empty() {
                continue;
            }
            let (key, value) = line.split_once(' ').ok_or("Invalid IDEdit directive")?;
            let slot = match key {
                "ide-endpoint" => &mut endpoint,
                "ide-db" => &mut database,
                "ide-project" => &mut project,
                "ide-entry" => &mut entry,
                _ => return Err("Unknown IDEdit directive".into()),
            };
            if slot.replace(String::from(value)).is_some() {
                return Err("Duplicate IDEdit directive".into());
            }
        }
        let config = Self {
            endpoint: endpoint
                .ok_or("Missing IDE endpoint")?
                .parse()
                .map_err(|_| "Invalid IDE endpoint")?,
            database: database.ok_or("Missing IDE database")?,
            project: project.ok_or("Missing IDE project")?,
            entry: entry.unwrap_or_else(|| "index.html".into()),
        };
        config.validate()?;
        Ok(config)
    }
    fn validate(&self) -> Result<(), String> {
        if self.endpoint == 0
            || [&self.database, &self.project]
                .iter()
                .any(|s| s.is_empty() || s.len() > 4096 || s.chars().any(char::is_control))
        {
            return Err("Invalid IDEdit workspace".into());
        }
        // The server VM must resolve the same source image as Edit.
        if !(self.database.starts_with('/') || self.database.starts_with("trueosfs:disc")) {
            return Err("IDE database must have an absolute path".into());
        }
        if !valid_asset_path(&self.entry) {
            return Err("Invalid preview entry".into());
        }
        Ok(())
    }
}

pub fn valid_asset_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_selects_the_source_store_without_a_public_folder() {
        let config = Config {
            endpoint: 7,
            database: "/apps/common/My sources.redb".into(),
            project: "My page".into(),
            entry: "index.html".into(),
        };
        assert_eq!(Config::parse(&config.script().unwrap()).unwrap(), config);
        assert!(Config::parse(&(config.script().unwrap() + "ide-project other\n")).is_err());
        assert!(
            Config {
                project: "page\nide-project other".into(),
                ..config.clone()
            }
            .script()
            .is_err()
        );
        assert!(
            Config {
                database: "relative.redb".into(),
                ..config.clone()
            }
            .script()
            .is_err()
        );
        assert!(
            Config {
                endpoint: 0,
                ..config
            }
            .script()
            .is_err()
        );
        for path in ["../secret", "a/../b", "/root", "a\\b", "a//b", "a\0b"] {
            assert!(!valid_asset_path(path));
        }
        assert!(valid_asset_path("assets/hello world.css"));
    }
}

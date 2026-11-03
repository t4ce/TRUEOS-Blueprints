//! Folder-based contract shared by Edit and the standalone preview Blueprint.
use alloc::{format, string::String};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "trueos-html-preview/v1";
pub const ROOT: &str = "/apps/common/ide/previews";
pub const HTML: &str = "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>HTML project</title>\n  <link rel=\"stylesheet\" href=\"./style.css\">\n  <script src=\"./main.js\" defer></script>\n</head>\n<body>\n</body>\n</html>\n";
pub const JS: &str = "document.addEventListener('DOMContentLoaded', () => {\n  console.log('template console log');\n}, { once: true });\n";
pub const CSS: &str = "body { background-color: #f4f4f4; }\n";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ready {
    pub protocol: String,
    pub root: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub root: String,
    /// Readiness lives outside the public document root.
    pub ready: String,
    pub entry: String,
}

impl Config {
    pub fn script(&self) -> Result<String, String> {
        self.validate()?;
        Ok(format!(
            "fs-scope trueosfs\npreview-root {}\npreview-ready {}\npreview-entry {}\n",
            self.root, self.ready, self.entry
        ))
    }
    pub fn parse(script: &str) -> Result<Self, String> {
        let (mut root, mut ready, mut entry) = (None, None, None);
        for line in script.lines() {
            if line == "fs-scope trueosfs" || line.is_empty() {
                continue;
            }
            let (key, value) = line.split_once(' ').ok_or("Invalid preview directive")?;
            let slot = match key {
                "preview-root" => &mut root,
                "preview-ready" => &mut ready,
                "preview-entry" => &mut entry,
                _ => return Err("Unknown preview directive".into()),
            };
            if slot.replace(value.into()).is_some() {
                return Err("Duplicate preview directive".into());
            }
        }
        let config = Self {
            root: root.ok_or("Missing preview root")?,
            ready: ready.ok_or("Missing preview readiness path")?,
            entry: entry.unwrap_or_else(|| "index.html".into()),
        };
        config.validate()?;
        Ok(config)
    }
    fn validate(&self) -> Result<(), String> {
        for path in [&self.root, &self.ready] {
            if !path.starts_with('/')
                || path.len() > 4096
                || path.chars().any(char::is_control)
                || path.split('/').any(|p| p == "." || p == "..")
            {
                return Err("Preview paths must be absolute and normalized".into());
            }
        }
        if !valid_asset_path(&self.entry) {
            return Err("Invalid preview entry".into());
        }
        if self.ready == self.root
            || self
                .ready
                .starts_with(&format!("{}/", self.root.trim_end_matches('/')))
        {
            return Err("Readiness must be outside the document root".into());
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
    fn script_is_literal_and_rejects_redirected_control() {
        let c = Config {
            root: "/apps/common/ide/My page/public".into(),
            ready: "/apps/common/ide/My page/ready.json".into(),
            entry: "index.html".into(),
        };
        assert_eq!(Config::parse(&c.script().unwrap()).unwrap(), c);
        assert!(Config::parse(&(c.script().unwrap() + "preview-root /other\n")).is_err());
        assert!(
            Config {
                ready: format!("{}/ready.json", c.root),
                ..c.clone()
            }
            .script()
            .is_err()
        );
        for p in ["../secret", "a/../b", "/root", "a\\b", "a//b", "a\0b"] {
            assert!(!valid_asset_path(p));
        }
        assert!(valid_asset_path("assets/hello world.css"));
    }
}

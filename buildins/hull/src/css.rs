//! One-shot CSS parsing. HTML extraction stays in rp5; CSS syntax belongs to Lightning CSS.
use std::collections::{HashMap, HashSet};

use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, StyleAttribute, StyleSheet};
use static_self::IntoOwned;
use trueos::{logl, netfs};
use url::Url;

#[derive(Default)]
pub(super) struct Css {
    sheets: Vec<StyleSheet<'static>>,
    attributes: Vec<StyleAttribute<'static>>,
    downloads: HashMap<u32, Url>,
    requested: HashSet<String>,
    pub sources: Vec<CssSource>,
}

pub(super) struct CssSource {
    pub node: Option<rp5::NodeId>,
    pub inline: bool,
    pub origin: String,
    pub code: String,
}

impl Css {
    pub fn parse_document(&mut self, document: &rp5::Document, page_url: &str) {
        let Ok(mut base) = Url::parse(page_url) else {
            return;
        };
        let mut nodes = Vec::new();
        let mut pending = vec![document.root];
        while let Some(id) = pending.pop() {
            nodes.push(id);
            pending.extend(document.nodes[id].children.iter().rev().copied());
        }
        // HTML's first base[href] applies to all document-relative CSS URLs.
        for &id in &nodes {
            if let Some(("base", rp5::Namespace::Html, attrs)) = document.element(id) {
                if let Some(href) = attribute(attrs, "href") {
                    if let Ok(resolved) = base.join(href) {
                        base = resolved;
                    }
                    break;
                }
            }
        }
        for id in nodes {
            let Some((name, namespace, attrs)) = document.element(id) else {
                continue;
            };
            if let Some(code) = attribute(attrs, "style") {
                self.sources.push(CssSource {
                    node: Some(id),
                    inline: true,
                    origin: base.to_string(),
                    code: code.to_owned(),
                });
                match StyleAttribute::parse(code, ParserOptions::default()) {
                    Ok(style) => self.attributes.push(style.into_owned()),
                    Err(error) => report(format_args!("inline CSS node={id}: {error}")),
                }
            }
            if name == "style"
                && matches!(namespace, rp5::Namespace::Html | rp5::Namespace::Svg)
                && attribute(attrs, "type")
                    .is_none_or(|value| value.is_empty() || value.eq_ignore_ascii_case("text/css"))
            {
                let code = document.text_content(id);
                self.sources.push(CssSource {
                    node: Some(id),
                    inline: false,
                    origin: base.to_string(),
                    code: code.clone(),
                });
                self.parse_sheet(&code, &base);
            }
            if name == "link"
                && namespace == rp5::Namespace::Html
                && attribute(attrs, "rel").is_some_and(|rel| {
                    rel.split_ascii_whitespace()
                        .any(|token| token.eq_ignore_ascii_case("stylesheet"))
                })
            {
                if let Some(href) = attribute(attrs, "href") {
                    self.request(&base, href);
                }
            }
        }
        self.report_counts();
    }

    fn parse_sheet(&mut self, code: &str, source: &Url) {
        let options = ParserOptions {
            filename: source.to_string(),
            ..ParserOptions::default()
        };
        match StyleSheet::parse(code, options) {
            Ok(sheet) => {
                // Only stylesheet imports are downloaded. Images, fonts and scripts are untouched.
                for rule in &sheet.rules.0 {
                    if let CssRule::Import(import) = rule {
                        self.request(source, &import.url);
                    }
                }
                self.sheets.push(sheet.into_owned());
            }
            Err(error) => report(format_args!("CSS {source}: {error}")),
        }
    }

    fn request(&mut self, base: &Url, reference: &str) {
        let mut url = match base.join(reference) {
            Ok(url) => url,
            Err(error) => {
                report(format_args!("CSS URL {reference:?}: {error}"));
                return;
            }
        };
        url.set_fragment(None);
        if !self.requested.insert(url.to_string()) {
            return;
        }
        if url.scheme() != "https" {
            report(format_args!(
                "CSS URL unsupported by HTTPS downloader: {url}"
            ));
            return;
        }
        match netfs::fetch_bytes(url.as_str().as_bytes()) {
            Ok(operation) => {
                self.downloads.insert(operation, url);
            }
            Err(code) => report(format_args!("CSS download start {url}: {code}")),
        }
    }

    pub fn poll_downloads(&mut self) {
        let completed: Vec<_> = self
            .downloads
            .keys()
            .copied()
            .filter(|&operation| !matches!(netfs::fetch_bytes_result_len(operation), Err(-8)))
            .collect();
        for operation in completed {
            let source = self.downloads.remove(&operation).unwrap();
            let result = netfs::fetch_bytes_read(operation);
            let _ = netfs::fetch_bytes_discard(operation);
            match result {
                Ok(bytes) => {
                    let code = String::from_utf8_lossy(&bytes).into_owned();
                    self.sources.push(CssSource {
                        node: None,
                        inline: false,
                        origin: source.to_string(),
                        code: code.clone(),
                    });
                    self.parse_sheet(&code, &source);
                }
                Err(code) => report(format_args!("CSS download {source}: {code}")),
            }
            self.report_counts();
        }
    }

    pub fn is_pending(&self) -> bool {
        !self.downloads.is_empty()
    }

    fn report_counts(&self) {
        logl::log(
            logl::level::INFO,
            format_args!(
                "hull: Lightning CSS parsed sheets={} inline={} pending={}",
                self.sheets.len(),
                self.attributes.len(),
                self.downloads.len()
            ),
        );
    }
}

impl Drop for Css {
    fn drop(&mut self) {
        for &operation in self.downloads.keys() {
            let _ = netfs::fetch_bytes_discard(operation);
        }
    }
}

fn attribute<'a>(attrs: &'a [rp5::Attribute], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attr| attr.namespace.is_none() && attr.name == name)
        .map(|attr| attr.value.as_str())
}

fn report(message: std::fmt::Arguments<'_>) {
    logl::log(logl::level::ERROR, format_args!("hull: {message}"));
}

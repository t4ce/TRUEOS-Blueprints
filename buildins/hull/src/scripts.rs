//! Extract script sources for inspection. Never parse, evaluate or execute them.
use std::collections::HashMap;

use rp5::{Document, Namespace, NodeId};
use trueos::{logl, netfs};
use url::Url;

#[derive(Default)]
pub(super) struct Scripts {
    pub sources: Vec<ScriptSource>,
    downloads: HashMap<u32, usize>,
}

pub(super) struct ScriptSource {
    pub nodes: Vec<NodeId>,
    pub origin: String,
    pub kind: String,
    pub bytes: Option<Vec<u8>>,
}

impl Scripts {
    pub fn extract(&mut self, document: &Document, page_url: &str) {
        let Ok(mut base) = Url::parse(page_url) else {
            return;
        };
        let mut nodes = Vec::new();
        let mut pending = vec![document.root];
        while let Some(id) = pending.pop() {
            nodes.push(id);
            pending.extend(document.nodes[id].children.iter().rev().copied());
        }
        for &id in &nodes {
            if let Some(("base", Namespace::Html, attrs)) = document.element(id) {
                if let Some(href) = attribute(attrs, "href") {
                    if let Ok(url) = base.join(href) {
                        base = url;
                    }
                    break;
                }
            }
        }
        let mut external: HashMap<String, usize> = HashMap::new();
        for id in nodes {
            let Some(("script", namespace, attrs)) = document.element(id) else {
                continue;
            };
            if !matches!(namespace, Namespace::Html | Namespace::Svg) {
                continue;
            }
            let kind = attribute(attrs, "type")
                .unwrap_or("text/javascript")
                .to_owned();
            let src = attribute(attrs, "src").or_else(|| attribute(attrs, "href"));
            if let Some(src) = src {
                let mut url = match base.join(src) {
                    Ok(url) => url,
                    Err(error) => {
                        report(format_args!("script URL {src:?}: {error}"));
                        continue;
                    }
                };
                url.set_fragment(None);
                let origin = url.to_string();
                if let Some(&index) = external.get(&origin) {
                    self.sources[index].nodes.push(id);
                    continue;
                }
                let index = self.sources.len();
                external.insert(origin.clone(), index);
                self.sources.push(ScriptSource {
                    nodes: vec![id],
                    origin,
                    kind,
                    bytes: None,
                });
                if url.scheme() != "https" {
                    report(format_args!(
                        "script URL unsupported by HTTPS downloader: {url}"
                    ));
                    continue;
                }
                match netfs::fetch_bytes(url.as_str().as_bytes()) {
                    Ok(operation) => {
                        self.downloads.insert(operation, index);
                    }
                    Err(code) => report(format_args!("script download start {url}: {code}")),
                }
            } else {
                self.sources.push(ScriptSource {
                    nodes: vec![id],
                    origin: base.to_string(),
                    kind,
                    bytes: Some(document.text_content(id).into_bytes()),
                });
            }
        }
        self.report_counts();
    }

    pub fn poll_downloads(&mut self) {
        let completed: Vec<_> = self
            .downloads
            .keys()
            .copied()
            .filter(|&operation| !matches!(netfs::fetch_bytes_result_len(operation), Err(-8)))
            .collect();
        for operation in completed {
            let index = self.downloads.remove(&operation).unwrap();
            let result = netfs::fetch_bytes_read(operation);
            let _ = netfs::fetch_bytes_discard(operation);
            match result {
                Ok(bytes) => self.sources[index].bytes = Some(bytes),
                Err(code) => report(format_args!(
                    "script download {}: {code}",
                    self.sources[index].origin
                )),
            }
            self.report_counts();
        }
    }

    fn report_counts(&self) {
        logl::log(
            logl::level::INFO,
            format_args!(
                "hull: scripts collected={} pending={} (inspection only)",
                self.sources.len(),
                self.downloads.len(),
            ),
        );
    }

    pub fn is_pending(&self) -> bool {
        !self.downloads.is_empty()
    }
}

impl Drop for Scripts {
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

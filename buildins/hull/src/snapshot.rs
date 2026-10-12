//! Save one flat inspection snapshot with CSS and scripts outside the HTML.
use std::fmt::Write;

use rp5::{Document, Namespace, NodeId, NodeKind};
use trueos::{async_fs, env, logl};

use super::css::Css;
use super::scripts::Scripts;

#[allow(non_snake_case)]
pub(super) fn uneccecaryStoreToFilesystemForDebug(
    document: &Document,
    css: &Css,
    scripts: &Scripts,
    page_url: &str,
) -> Result<(), i32> {
    let app_root = env::var("TRUEOS_APP_FS_ROOT").unwrap_or_else(|_| "apps/hull".into());
    let url = url::Url::parse(page_url).map_err(|_| -1)?;
    let host = url.host_str().ok_or(-1)?;
    let root = format!("{app_root}/{host}");
    let html = html_without_css_and_scripts(document, css, scripts);
    async_fs::block_on(async {
        async_fs::create_dir_all(root.as_bytes()).await?;
        for (index, source) in css.sources.iter().enumerate() {
            let filename = format!("style-{index:04}.css");
            async_fs::write_file(
                format!("{root}/{filename}").as_bytes(),
                source.code.as_bytes(),
            )
            .await?;
        }
        for (index, source) in scripts.sources.iter().enumerate() {
            if let Some(bytes) = &source.bytes {
                let filename = format!("script-{index:04}.js");
                async_fs::write_file(format!("{root}/{filename}").as_bytes(), bytes).await?;
            }
        }
        // Publish the HTML last, after all its associated CSS files exist.
        async_fs::write_file(format!("{root}/index.html").as_bytes(), html.as_bytes()).await
    })?;
    logl::log(
        logl::level::INFO,
        format_args!(
            "hull: saved HTML, {} CSS sources and {} script sources under {root}",
            css.sources.len(),
            scripts.sources.len()
        ),
    );
    Ok(())
}

pub(super) fn html_without_css_and_scripts(
    document: &Document,
    css: &Css,
    scripts: &Scripts,
) -> String {
    enum Step {
        Node(NodeId),
        End(String),
    }
    let mut output = String::new();
    let mut pending = vec![Step::Node(document.root)];
    while let Some(step) = pending.pop() {
        let id = match step {
            Step::End(name) => {
                let _ = write!(output, "</{name}>");
                continue;
            }
            Step::Node(id) => id,
        };
        let node = &document.nodes[id];
        match &node.kind {
            NodeKind::Document | NodeKind::Fragment => {}
            NodeKind::Text(text) => {
                let raw = node.parent.is_some_and(|parent| {
                    document.namespace(parent) == Namespace::Html
                        && matches!(
                            document.name(parent),
                            "script"
                                | "style"
                                | "xmp"
                                | "iframe"
                                | "noembed"
                                | "noframes"
                                | "plaintext"
                        )
                });
                if raw {
                    output.push_str(text);
                } else {
                    escape(&mut output, text, false);
                }
                continue;
            }
            NodeKind::Comment(text) => {
                let _ = write!(output, "<!--{text}-->");
                continue;
            }
            NodeKind::Doctype {
                name,
                public_id,
                system_id,
            } => {
                let _ = write!(output, "<!DOCTYPE {name}");
                if !public_id.is_empty() {
                    output.push_str(" PUBLIC \"");
                    escape(&mut output, public_id, true);
                    output.push('"');
                    if !system_id.is_empty() {
                        output.push_str(" \"");
                        escape(&mut output, system_id, true);
                        output.push('"');
                    }
                } else if !system_id.is_empty() {
                    output.push_str(" SYSTEM \"");
                    escape(&mut output, system_id, true);
                    output.push('"');
                }
                output.push('>');
                continue;
            }
            NodeKind::Element {
                name,
                namespace,
                attrs,
                template_content,
            } => {
                // CSS remains in raw source files. Mark its former position for inspection.
                if let Some((index, _)) = css
                    .sources
                    .iter()
                    .enumerate()
                    .find(|(_, source)| source.node == Some(id) && !source.inline)
                {
                    let _ = write!(output, "<!-- hull: stylesheet style-{index:04}.css -->");
                    continue;
                }
                let stylesheet_link = name == "link"
                    && *namespace == Namespace::Html
                    && attrs.iter().any(|attr| {
                        attr.name == "rel"
                            && attr
                                .value
                                .split_ascii_whitespace()
                                .any(|value| value.eq_ignore_ascii_case("stylesheet"))
                    });
                if stylesheet_link {
                    output.push_str("<!-- hull: external stylesheet extracted -->");
                    continue;
                }
                if let Some((index, source)) = scripts
                    .sources
                    .iter()
                    .enumerate()
                    .find(|(_, source)| source.nodes.contains(&id))
                {
                    let _ = write!(
                        output,
                        "<!-- hull: script script-{index:04}.js{} -->",
                        if source.bytes.is_some() {
                            ""
                        } else {
                            " unavailable"
                        }
                    );
                    continue;
                }
                let _ = write!(output, "<{name}");
                for attr in attrs {
                    if attr.namespace.is_none() && attr.name == "style" {
                        continue;
                    }
                    output.push(' ');
                    if let Some(prefix) = &attr.prefix {
                        output.push_str(prefix);
                        output.push(':');
                    }
                    output.push_str(&attr.name);
                    output.push_str("=\"");
                    escape(&mut output, &attr.value, true);
                    output.push('"');
                }
                if let Some((index, _)) = css
                    .sources
                    .iter()
                    .enumerate()
                    .find(|(_, source)| source.node == Some(id) && source.inline)
                {
                    let _ = write!(output, " data-hull-inline-css=\"style-{index:04}.css\"");
                }
                output.push('>');
                if *namespace == Namespace::Html
                    && matches!(
                        name.as_str(),
                        "area"
                            | "base"
                            | "basefont"
                            | "bgsound"
                            | "link"
                            | "meta"
                            | "img"
                            | "embed"
                            | "param"
                            | "hr"
                            | "input"
                            | "source"
                            | "track"
                            | "br"
                            | "wbr"
                            | "col"
                    )
                {
                    continue;
                }
                pending.push(Step::End(name.clone()));
                if let Some(fragment) = template_content {
                    pending.push(Step::Node(*fragment));
                    continue;
                }
            }
        }
        pending.extend(node.children.iter().rev().copied().map(Step::Node));
    }
    output
}

fn escape(output: &mut String, text: &str, attribute: bool) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '\u{a0}' => output.push_str("&nbsp;"),
            '"' if attribute => output.push_str("&quot;"),
            '<' if !attribute => output.push_str("&lt;"),
            '>' if !attribute => output.push_str("&gt;"),
            character => output.push(character),
        }
    }
}

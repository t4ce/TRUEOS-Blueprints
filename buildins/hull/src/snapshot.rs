//! Save one inspection snapshot with CSS outside the HTML, without applying styles.
use std::fmt::Write;

use rp5::{Document, Namespace, NodeId, NodeKind};
use trueos::{async_fs, env, logl};

use super::css::Css;

#[allow(non_snake_case)]
pub(super) fn uneccecaryStoreToFilesystemForDebug(
    document: &Document,
    css: &Css,
) -> Result<(), i32> {
    let root = env::var("TRUEOS_APP_FS_ROOT").unwrap_or_else(|_| "apps/hull".into());
    let html = html_without_css(document, css);
    async_fs::block_on(async {
        async_fs::create_dir_all(format!("{root}/css").as_bytes()).await?;
        let mut manifest = String::from("file\tkind\tnode\torigin\n");
        for (index, source) in css.sources.iter().enumerate() {
            let filename = format!("css/{index:04}.css");
            async_fs::write_file(
                format!("{root}/{filename}").as_bytes(),
                source.code.as_bytes(),
            )
            .await?;
            let node = source.node.map(|id| id.to_string()).unwrap_or_default();
            let _ = writeln!(
                manifest,
                "{filename}\t{}\t{node}\t{}",
                if source.inline {
                    "declarations"
                } else {
                    "stylesheet"
                },
                source.origin
            );
        }
        async_fs::write_file(
            format!("{root}/css/sources.tsv").as_bytes(),
            manifest.as_bytes(),
        )
        .await?;
        // Publish the HTML last, after all its associated CSS files exist.
        async_fs::write_file(format!("{root}/index.html").as_bytes(), html.as_bytes()).await
    })?;
    logl::log(
        logl::level::INFO,
        format_args!(
            "hull: saved HTML and {} CSS sources under {root}",
            css.sources.len()
        ),
    );
    Ok(())
}

pub(super) fn html_without_css(document: &Document, css: &Css) -> String {
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
                    let _ = write!(output, "<!-- hull: stylesheet css/{index:04}.css -->");
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
                    output.push_str("<!-- hull: external stylesheet; see css/sources.tsv -->");
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
                    let _ = write!(output, " data-hull-inline-css=\"css/{index:04}.css\"");
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

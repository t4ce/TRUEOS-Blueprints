#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::fs;
use websys::path::{Path, PathBuf};

use crate::AppError;

#[derive(Debug)]
pub enum TreeNode {
    Dir {
        name: String,
        rel_path: String,
        children: Vec<TreeNode>,
    },
    File {
        name: String,
        rel_path: String,
        url_path: String,
        record_key: RecordKeyLabel,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(any(target_os = "trueos", target_os = "zkvm")), allow(dead_code))]
pub enum RecordKeyLabel {
    Ffa,
    Key { provider: String, handle: String },
}

pub async fn scan_dir(dir: &Path, rel: &str) -> Result<Vec<TreeNode>, AppError> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        scan_dir_trueos(dir, rel).await
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        scan_dir_std(dir, rel)
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn scan_dir_std(dir: &Path, rel: &str) -> Result<Vec<TreeNode>, AppError> {
    let mut entries = Vec::new();
    let read_dir = fs::read_dir(dir).map_err(|_| AppError::NotFound)?;

    for entry in read_dir.flatten() {
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }

        let child_rel = child_rel_path(rel, &name);

        if file_type.is_dir() {
            let child_path = entry.path();
            let children = scan_dir_std(&child_path, &child_rel)?;
            entries.push(TreeNode::Dir {
                name,
                rel_path: child_rel,
                children,
            });
        } else if file_type.is_file() {
            entries.push(TreeNode::File {
                name,
                rel_path: child_rel.clone(),
                url_path: format!("/{}", encode_path_segments(&child_rel)),
                record_key: RecordKeyLabel::Ffa,
            });
        }
    }

    sort_nodes(&mut entries);
    Ok(entries)
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn scan_dir_trueos<'a>(
    dir: &'a Path,
    rel: &'a str,
) -> core::pin::Pin<
    Box<dyn core::future::Future<Output = Result<Vec<TreeNode>, AppError>> + Send + 'a>,
> {
    Box::pin(async move {
        let mut entries = Vec::new();

        let listing = trueos_list_dir(dir).await?;
        for entry in listing.entries {
            if entry.name.starts_with('.') {
                continue;
            }

            let child_path = dir.join(entry.name.as_str());
            let child_rel = child_rel_path(rel, &entry.name);

            match entry.kind {
                v::vfs_async::NodeKind::Directory => {
                    let children = scan_dir_trueos(&child_path, &child_rel).await?;
                    entries.push(TreeNode::Dir {
                        name: entry.name,
                        rel_path: child_rel,
                        children,
                    });
                }
                v::vfs_async::NodeKind::File => {
                    let record_key = trueos_record_key(&child_path).await?;
                    entries.push(TreeNode::File {
                        name: entry.name,
                        rel_path: child_rel.clone(),
                        url_path: format!("/{}", encode_path_segments(&child_rel)),
                        record_key,
                    });
                }
            }
        }

        sort_nodes(&mut entries);
        Ok(entries)
    })
}

fn child_rel_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn sort_nodes(entries: &mut [TreeNode]) {
    entries.sort_by(|a, b| {
        let (a_dir, a_name) = node_sort_key(a);
        let (b_dir, b_name) = node_sort_key(b);
        b_dir.cmp(&a_dir).then_with(|| a_name.cmp(b_name))
    });
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
async fn trueos_list_dir(dir: &Path) -> Result<v::vfs_async::DirListing, AppError> {
    let path = dir.to_str().ok_or(AppError::NotFound)?;
    v::vfs_async::list_dir(path.as_bytes())
        .await
        .map_err(|_| AppError::NotFound)
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
async fn trueos_record_key(path: &Path) -> Result<RecordKeyLabel, AppError> {
    let path = path.to_str().ok_or(AppError::NotFound)?;
    match v::vfs_async::record_key(path.as_bytes())
        .await
        .map_err(|_| AppError::NotFound)?
    {
        v::vfs_async::RecordKey::Ffa => Ok(RecordKeyLabel::Ffa),
        v::vfs_async::RecordKey::Key { provider, handle } => Ok(RecordKeyLabel::Key {
            provider: hex(&provider),
            handle: hex(&handle),
        }),
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn node_sort_key(node: &TreeNode) -> (bool, &str) {
    match node {
        TreeNode::Dir { name, .. } => (true, name.as_str()),
        TreeNode::File { name, .. } => (false, name.as_str()),
    }
}

pub fn encode_path_segments(path: &str) -> String {
    path.split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for b in segment.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn resolve_under_root(root: &Path, request_path: &str) -> Option<PathBuf> {
    let trimmed = request_path.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Some(root.to_path_buf());
    }
    if trimmed.split('/').any(|p| p == ".." || p.is_empty()) {
        return None;
    }

    Some(root.join(trimmed))
}

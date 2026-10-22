use std::fmt;

use base64::Engine as _;
use serde_json::Value;

use crate::source_db::{self, SourceFileInput};

const FETCH_TIMEOUT_MS: u64 = 60_000;
const MAX_GIT_FILES: usize = 4096;
const MAX_GIT_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_GIT_SOURCE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitCommit {
    pub repository: String,
    pub commit: String,
    pub project_name: String,
}

#[derive(Debug)]
pub enum GitSourceError {
    InvalidLink(String),
    DatabaseIdentity(source_db::SourceDbError),
    Network {
        url: String,
        code: i32,
    },
    Json {
        url: String,
        source: serde_json::Error,
    },
    Protocol(String),
    TooManyFiles(usize),
    FileTooLarge {
        path: String,
        bytes: usize,
    },
    SourceTooLarge(usize),
    Base64 {
        path: String,
        source: base64::DecodeError,
    },
}

impl fmt::Display for GitSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLink(message) => write!(formatter, "invalid Git link: {message}"),
            Self::DatabaseIdentity(error) => error.fmt(formatter),
            Self::Network { url, code } => write!(formatter, "fetch {url}: code {code}"),
            Self::Json { url, source } => write!(formatter, "decode JSON from {url}: {source}"),
            Self::Protocol(message) => write!(formatter, "Git provider response: {message}"),
            Self::TooManyFiles(count) => {
                write!(
                    formatter,
                    "commit contains {count} files; limit is {MAX_GIT_FILES}"
                )
            }
            Self::FileTooLarge { path, bytes } => write!(
                formatter,
                "source file {path} is {bytes} bytes; limit is {MAX_GIT_FILE_BYTES}"
            ),
            Self::SourceTooLarge(bytes) => write!(
                formatter,
                "commit source is {bytes} bytes; limit is {MAX_GIT_SOURCE_BYTES}"
            ),
            Self::Base64 { path, source } => {
                write!(formatter, "decode Git blob {path}: {source}")
            }
        }
    }
}

impl std::error::Error for GitSourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DatabaseIdentity(error) => Some(error),
            Self::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<source_db::SourceDbError> for GitSourceError {
    fn from(error: source_db::SourceDbError) -> Self {
        Self::DatabaseIdentity(error)
    }
}

/// Parse a GitHub commit/tree link or an HTTPS repository URL plus exact commit.
///
/// Accepted single-link forms include `/commit/<oid>`, `/tree/<oid>`, and a
/// repository URL ending in `#<oid>`. A plain repository URL requires
/// `commit_override`. Branch names are intentionally not accepted: database
/// identity is always the immutable full object ID.
pub fn parse_commit_link(
    link: &str,
    commit_override: Option<&str>,
) -> Result<GitCommit, GitSourceError> {
    let link = link.trim();
    let (without_fragment, fragment) = link
        .split_once('#')
        .map_or((link, None), |(url, commit)| (url, Some(commit)));
    if without_fragment.contains('?') {
        return Err(GitSourceError::InvalidLink(String::from(
            "query parameters are not supported",
        )));
    }

    let marker = ["/commit/", "/tree/"].into_iter().find_map(|marker| {
        without_fragment
            .split_once(marker)
            .map(|(repository, commit)| (repository, commit))
    });
    let (repository, embedded_commit) = marker
        .map_or((without_fragment, fragment), |(repository, commit)| {
            (repository, Some(commit))
        });
    let commit = match (embedded_commit, commit_override) {
        (Some(left), Some(right)) if !left.eq_ignore_ascii_case(right) => {
            return Err(GitSourceError::InvalidLink(String::from(
                "link and explicit commit disagree",
            )));
        }
        (Some(commit), _) | (None, Some(commit)) => commit,
        (None, None) => {
            return Err(GitSourceError::InvalidLink(String::from(
                "an exact commit object ID is required",
            )));
        }
    };

    let repository = source_db::canonicalize_git_repository(repository)?;
    let commit = source_db::canonicalize_git_commit(commit)?;
    let (host, repo_path) = repository
        .strip_prefix("https://")
        .and_then(|rest| rest.split_once('/'))
        .ok_or_else(|| GitSourceError::InvalidLink(String::from("expected an HTTPS URL")))?;
    if !host.eq_ignore_ascii_case("github.com") {
        return Err(GitSourceError::InvalidLink(String::from(
            "the initial provider adapter supports github.com",
        )));
    }
    let mut components = repo_path.split('/');
    let owner = components.next().unwrap_or_default();
    let name = components.next().unwrap_or_default();
    if owner.is_empty() || name.is_empty() || components.next().is_some() {
        return Err(GitSourceError::InvalidLink(String::from(
            "expected https://github.com/<owner>/<repository>",
        )));
    }
    let project_name = name.to_owned();
    Ok(GitCommit {
        repository,
        commit,
        project_name,
    })
}

/// Download the exact file tree for an immutable GitHub commit through the
/// kernel HTTPS/Hyper path. File bytes are returned unchanged.
pub fn fetch_commit_files(reference: &GitCommit) -> Result<Vec<SourceFileInput>, GitSourceError> {
    let repo_path = reference
        .repository
        .strip_prefix("https://github.com/")
        .ok_or_else(|| GitSourceError::InvalidLink(String::from("expected github.com")))?;
    let tree_url = format!(
        "https://api.github.com/repos/{repo_path}/git/trees/{}?recursive=1",
        reference.commit
    );
    let tree = fetch_json(tree_url.as_str())?;
    if tree.get("truncated").and_then(Value::as_bool) == Some(true) {
        return Err(GitSourceError::Protocol(String::from(
            "recursive commit tree was truncated",
        )));
    }
    let entries = tree
        .get("tree")
        .and_then(Value::as_array)
        .ok_or_else(|| GitSourceError::Protocol(String::from("missing commit tree array")))?;
    let blobs = entries
        .iter()
        .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("blob"))
        .collect::<Vec<_>>();
    if blobs.len() > MAX_GIT_FILES {
        return Err(GitSourceError::TooManyFiles(blobs.len()));
    }

    let mut total = 0usize;
    let mut files = Vec::with_capacity(blobs.len());
    for entry in blobs {
        let path = required_string(entry, "path")?.to_owned();
        let sha = required_string(entry, "sha")?;
        let declared_size = entry
            .get("size")
            .and_then(Value::as_u64)
            .and_then(|size| usize::try_from(size).ok())
            .ok_or_else(|| GitSourceError::Protocol(format!("blob {path} has no valid size")))?;
        if declared_size > MAX_GIT_FILE_BYTES {
            return Err(GitSourceError::FileTooLarge {
                path,
                bytes: declared_size,
            });
        }
        total = total
            .checked_add(declared_size)
            .ok_or(GitSourceError::SourceTooLarge(usize::MAX))?;
        if total > MAX_GIT_SOURCE_BYTES {
            return Err(GitSourceError::SourceTooLarge(total));
        }

        let blob_url = format!("https://api.github.com/repos/{repo_path}/git/blobs/{sha}");
        let blob = fetch_json(blob_url.as_str())?;
        if blob.get("encoding").and_then(Value::as_str) != Some("base64") {
            return Err(GitSourceError::Protocol(format!(
                "blob {path} does not use base64 encoding"
            )));
        }
        let encoded = required_string(&blob, "content")?;
        let compact = encoded
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect::<Vec<_>>();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(compact)
            .map_err(|source| GitSourceError::Base64 {
                path: path.clone(),
                source,
            })?;
        if bytes.len() != declared_size {
            return Err(GitSourceError::Protocol(format!(
                "blob {path} declared {declared_size} bytes but returned {}",
                bytes.len()
            )));
        }
        let mode = entry
            .get("mode")
            .and_then(Value::as_str)
            .and_then(|mode| u32::from_str_radix(mode, 8).ok())
            .unwrap_or(source_db::DEFAULT_FILE_MODE);
        files.push(SourceFileInput::with_mode(path, bytes, mode));
    }
    if files.is_empty() {
        return Err(GitSourceError::Protocol(String::from(
            "commit contains no file blobs",
        )));
    }
    Ok(files)
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, GitSourceError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| GitSourceError::Protocol(format!("missing string field `{field}`")))
}

fn fetch_json(url: &str) -> Result<Value, GitSourceError> {
    let operation =
        trueos::netfs::fetch_bytes(url.as_bytes()).map_err(|code| GitSourceError::Network {
            url: url.to_owned(),
            code,
        })?;
    let outcome = (|| {
        let code = trueos::netfs::fetch_bytes_wait(operation, FETCH_TIMEOUT_MS);
        if code != 0 {
            return Err(GitSourceError::Network {
                url: url.to_owned(),
                code,
            });
        }
        let bytes =
            trueos::netfs::fetch_bytes_read(operation).map_err(|code| GitSourceError::Network {
                url: url.to_owned(),
                code,
            })?;
        serde_json::from_slice(bytes.as_slice()).map_err(|source| GitSourceError::Json {
            url: url.to_owned(),
            source,
        })
    })();
    let _ = trueos::netfs::fetch_bytes_discard(operation);
    outcome
}

#[cfg(test)]
mod tests {
    use super::parse_commit_link;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn parses_commit_and_fragment_links_to_same_identity() {
        let commit = parse_commit_link(
            format!("https://github.com/t4ce/TRUEOS/commit/{COMMIT}").as_str(),
            None,
        )
        .unwrap();
        let fragment = parse_commit_link(
            format!("https://github.com/t4ce/TRUEOS.git#{COMMIT}").as_str(),
            None,
        )
        .unwrap();
        assert_eq!(commit.repository, "https://github.com/t4ce/TRUEOS");
        assert_eq!(commit, fragment);
        assert_eq!(commit.project_name, "TRUEOS");
    }

    #[test]
    fn rejects_moving_refs_and_disagreement() {
        assert!(parse_commit_link("https://github.com/t4ce/TRUEOS/tree/main", None).is_err());
        assert!(
            parse_commit_link(
                format!("https://github.com/t4ce/TRUEOS#{COMMIT}").as_str(),
                Some("abcdefabcdefabcdefabcdefabcdefabcdefabcd"),
            )
            .is_err()
        );
    }
}

use anyhow::{Context, Result, anyhow};
use serde::de::DeserializeOwned;

pub fn split_markdown_frontmatter<T>(raw: &str) -> Result<(T, String)>
where
    T: Default + DeserializeOwned,
{
    if !raw.starts_with("---\n") {
        return Ok((T::default(), raw.to_string()));
    }

    let rest = &raw[4..];
    let Some(end) = rest.find("\n---\n") else {
        return Err(anyhow!("unterminated YAML frontmatter"));
    };

    let frontmatter_text = &rest[..end];
    let body = &rest[end + 5..];
    let frontmatter = if frontmatter_text.trim().is_empty() {
        T::default()
    } else {
        serde_yaml::from_str(frontmatter_text).context("invalid YAML frontmatter")?
    };

    Ok((frontmatter, body.to_string()))
}
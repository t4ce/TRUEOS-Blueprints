extern crate alloc;
use alloc::{string::{String, ToString}, vec::Vec};
use core::fmt::Write;
use sha2::{Digest,Sha256};
#[derive(Clone)]
pub(crate) struct OnlineApp {
    pub(crate) name: String,
    pub(crate) archive_name: String,
    pub(crate) sha256: String,
    url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OnlineCatalog {
    Apps,
    Probes,
}

impl OnlineCatalog {
    const fn url(self) -> &'static str {
        match self {
            Self::Apps => "https://trueos.eu/apps",
            Self::Probes => "https://trueos.eu/probes",
        }
    }

    const fn prefix(self) -> &'static str {
        match self {
            Self::Apps => "apps",
            Self::Probes => "probe",
        }
    }

    const fn item(self) -> &'static str {
        match self {
            Self::Apps => "app",
            Self::Probes => "probe",
        }
    }

    const fn headers(self) -> &'static [&'static str; 6] {
        match self {
            Self::Apps => &["id", "app", "sha", "id", "app", "sha"],
            Self::Probes => &["id", "probe", "sha", "id", "probe", "sha"],
        }
    }
}
const ONLINE_LIST_MAX_BYTES: usize = 1024 * 1024;
const ONLINE_APP_MAX_BYTES: usize = 512 * 1024 * 1024;
const ONLINE_FETCH_TIMEOUT_MS: u32 = 45_000;
const ONLINE_APP_HASH_SEPARATOR: &str = "§§";
const SHA256_HEX_LEN: usize = 64;

fn absolutize_online_url(href: &str, catalog: OnlineCatalog) -> String {
    if href.contains("://") {
        String::from(href)
    } else if href.starts_with('/') {
        alloc::format!("https://trueos.eu{}", href)
    } else {
        alloc::format!("{}/{}", catalog.url(), href)
    }
}

fn parse_attr_value<'a>(text: &'a str, attr: &str) -> Option<&'a str> {
    let pos = text.find(attr)?;
    let rest = &text[pos + attr.len()..];
    let quote = rest.as_bytes().first().copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let rest = &rest[1..];
    let end = rest.as_bytes().iter().position(|&b| b == quote)?;
    Some(&rest[..end])
}

fn published_app_name_parts(value: &str) -> Option<(&str, &str)> {
    if let Some((archive_name, sha256)) = value.rsplit_once(ONLINE_APP_HASH_SEPARATOR) {
        if archive_name.to_ascii_lowercase().ends_with(".bp")
            && sha256.len() == SHA256_HEX_LEN
            && sha256.as_bytes().iter().all(u8::is_ascii_hexdigit)
        {
            return Some((archive_name, sha256));
        }
        return None;
    }

    value
        .to_ascii_lowercase()
        .ends_with(".bp")
        .then_some((value, "-"))
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_nibble(bytes[index + 1]), hex_nibble(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(decoded.as_slice()).into_owned()
}

fn published_link_parts(link_text: &str, href: &str) -> Option<(String, String)> {
    let text_parts = published_app_name_parts(link_text);
    let encoded_href_name = href
        .split(['?', '#'])
        .next()
        .unwrap_or(href)
        .rsplit('/')
        .next()
        .unwrap_or(href);
    let href_name = percent_decode(encoded_href_name);
    let href_parts = published_app_name_parts(href_name.as_str());
    match (text_parts, href_parts) {
        (Some(parts), _) if parts.1 != "-" => Some((parts.0.to_string(), parts.1.to_string())),
        (_, Some(parts)) if parts.1 != "-" => Some((parts.0.to_string(), parts.1.to_string())),
        (Some(parts), _) => Some((parts.0.to_string(), parts.1.to_string())),
        (None, Some(parts)) => Some((parts.0.to_string(), parts.1.to_string())),
        (None, None) => None,
    }
}

fn clean_archive_name(value: &str) -> Option<&str> {
    let name = value.rsplit('/').next()?.rsplit('\\').next()?;
    if name.is_empty()
        || !name.to_ascii_lowercase().ends_with(".bp")
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return None;
    }
    Some(name)
}

fn parse_online_apps(html: &str, catalog: OnlineCatalog) -> Vec<OnlineApp> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(li_start) = rest.find("<li") {
        rest = &rest[li_start + 3..];
        let li_end = rest.find("</li>").unwrap_or(rest.len());
        let item = &rest[..li_end];
        let Some(a_start) = item.find("<a") else {
            rest = &rest[li_end..];
            continue;
        };
        let link = &item[a_start..];
        let Some(tag_end) = link.find('>') else {
            rest = &rest[li_end..];
            continue;
        };
        let tag = &link[..tag_end];
        let Some(href) = parse_attr_value(tag, "href=") else {
            rest = &rest[li_end..];
            continue;
        };
        let Some(text_end) = link[tag_end + 1..].find("</a>") else {
            rest = &rest[li_end..];
            continue;
        };
        let published_name = link[tag_end + 1..tag_end + 1 + text_end].trim();
        let Some((published_archive_name, sha256)) = published_link_parts(published_name, href)
        else {
            rest = &rest[li_end..];
            continue;
        };
        let Some(archive_name) = clean_archive_name(published_archive_name.as_str()) else {
            rest = &rest[li_end..];
            continue;
        };
        let url = absolutize_online_url(href, catalog);
        out.push(OnlineApp {
            name: trim_bp_suffix(archive_name).to_string(),
            archive_name: archive_name.to_string(),
            sha256,
            url,
        });
        rest = &rest[li_end..];
    }
    out
}

fn trim_bp_suffix(value: &str) -> &str {
    let suffix_at = value.len().saturating_sub(3);
    if value.is_char_boundary(suffix_at)
        && value
            .get(suffix_at..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".bp"))
    {
        &value[..suffix_at]
    } else {
        value
    }
}

fn online_app_match_key(value: &str) -> &str {
    let value = value.trim();
    let end = value
        .char_indices()
        .find_map(|(idx, ch)| matches!(ch, '?' | '#').then_some(idx))
        .unwrap_or(value.len());
    let value = &value[..end];
    let value = value.rsplit('/').next().unwrap_or(value);
    let value = value
        .rsplit_once(ONLINE_APP_HASH_SEPARATOR)
        .map(|(archive_name, _)| archive_name)
        .unwrap_or(value);
    trim_bp_suffix(value)
}

fn resolve_online_app<'a>(apps: &'a [OnlineApp], selector: &str) -> Option<&'a OnlineApp> {
    if let Ok(id) = selector.parse::<usize>() {
        return apps.get(id);
    }

    let requested = online_app_match_key(selector);
    apps.iter().find(|app| {
        online_app_match_key(app.name.as_str()).eq_ignore_ascii_case(requested)
            || online_app_match_key(app.archive_name.as_str()).eq_ignore_ascii_case(requested)
            || online_app_match_key(app.url.as_str()).eq_ignore_ascii_case(requested)
    })
}

fn online_app_sha256_matches(app: &OnlineApp, bytes: &[u8]) -> bool {
    if app.sha256 == "-" {
        return true;
    }

    let digest = Sha256::digest(bytes);
    let mut actual = String::with_capacity(SHA256_HEX_LEN);
    for byte in digest {
        let _ = write!(actual, "{byte:02x}");
    }
    actual.eq_ignore_ascii_case(app.sha256.as_str())
}


pub fn parse(html: &str) -> Vec<OnlineApp> { parse_online_apps(html, OnlineCatalog::Apps) }
pub fn verify(app: &OnlineApp, bytes: &[u8]) -> bool { online_app_sha256_matches(app, bytes) }
pub fn url(app: &OnlineApp) -> &str { &app.url }
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_links_use_the_selected_catalog() {
        for (catalog, directory) in [
            (OnlineCatalog::Apps, "apps"),
            (OnlineCatalog::Probes, "probes"),
        ] {
            let html = "<li><a href=\"tokio_mrt.bp\">tokio_mrt.bp</a></li>";
            let apps = parse_online_apps(html, catalog);
            assert_eq!(apps.len(), 1);
            assert_eq!(apps[0].url, alloc::format!("https://trueos.eu/{directory}/tokio_mrt.bp"));
        }
    }

    #[test]
    fn probe_list_resolves_ids_names_and_encoded_hashes() {
        let hash = "a".repeat(SHA256_HEX_LEN);
        let html =
            alloc::format!("<li><a href=\"tokio_mrt.bp%C2%A7%C2%A7{hash}\">tokio_mrt.bp</a></li>");
        let probes = parse_online_apps(&html, OnlineCatalog::Probes);
        for selector in ["0", "tokio_mrt", "TOKIO_MRT.BP"] {
            let probe = resolve_online_app(&probes, selector).unwrap();
            assert_eq!(probe.archive_name, "tokio_mrt.bp");
            assert_eq!(probe.sha256, hash);
            assert!(probe.url.starts_with("https://trueos.eu/probes/"));
        }
        assert!(resolve_online_app(&probes, "1").is_none());
        assert!(resolve_online_app(&probes, "missing").is_none());
    }

    #[test]
    fn rooted_and_absolute_probe_links_keep_their_destination() {
        assert_eq!(
            absolutize_online_url("/probes/tokio_mrt.bp", OnlineCatalog::Probes),
            "https://trueos.eu/probes/tokio_mrt.bp"
        );
        assert_eq!(
            absolutize_online_url("https://trueos.eu/probes/tokio_mrt.bp", OnlineCatalog::Probes),
            "https://trueos.eu/probes/tokio_mrt.bp"
        );
    }

    #[test]
    fn probe_hash_rejects_changed_payload() {
        // SHA-256 of the empty payload, carried in the same filename format
        // emitted by the Blueprint publisher.
        let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let html = alloc::format!("<li><a href=\"probe.bp§§{hash}\">probe.bp</a></li>");
        let probes = parse_online_apps(&html, OnlineCatalog::Probes);
        assert!(online_app_sha256_matches(&probes[0], b""));
        assert!(!online_app_sha256_matches(&probes[0], b"changed"));
    }
}

//! Versioned platform documentation owned by the IDE, outside project source.
use crate::protocol::{ApiReferenceDocument, PlatformApiContext, PlatformApiTopic};
use alloc::{collections::BTreeMap, string::ToString, vec::Vec};
use serde::Deserialize;

const MANIFEST: &str = include_str!("../api-prm/manifest.json");
const TOPICS: &[(&str, &str, &str, &str)] = &[
    (
        "index",
        "Platform API overview",
        "trueos-api.prm",
        include_str!("../api-prm/trueos-api.prm"),
    ),
    (
        "logging",
        "Structured logging and output",
        "logging.prm",
        include_str!("../api-prm/logging.prm"),
    ),
    (
        "terminal",
        "Terminal lifecycle and input",
        "terminal.prm",
        include_str!("../api-prm/terminal.prm"),
    ),
    (
        "filesystem",
        "Asynchronous filesystem operations",
        "filesystem.prm",
        include_str!("../api-prm/filesystem.prm"),
    ),
    (
        "lifecycle",
        "Application lifecycle and launch",
        "lifecycle.prm",
        include_str!("../api-prm/lifecycle.prm"),
    ),
    (
        "memory",
        "Allocation and buffers",
        "memory.prm",
        include_str!("../api-prm/memory.prm"),
    ),
    (
        "graphics",
        "UI4 and GPU interfaces",
        "graphics.prm",
        include_str!("../api-prm/graphics.prm"),
    ),
    (
        "services",
        "Additional platform services",
        "services.prm",
        include_str!("../api-prm/services.prm"),
    ),
    (
        "types",
        "C layouts and constants",
        "types.prm",
        include_str!("../api-prm/types.prm"),
    ),
    (
        "symbols",
        "C symbol-to-topic lookup",
        "symbols.prm",
        include_str!("../api-prm/symbols.prm"),
    ),
];

#[derive(Deserialize)]
struct Manifest {
    api_id: alloc::string::String,
    source: Source,
    files: BTreeMap<alloc::string::String, File>,
}
#[derive(Deserialize)]
struct Source {
    sha256: alloc::string::String,
}
#[derive(Deserialize)]
struct File {
    sha256: alloc::string::String,
    bytes: usize,
}

fn manifest() -> Manifest {
    serde_json::from_str(MANIFEST).expect("validated platform API reference manifest")
}

pub fn context() -> PlatformApiContext {
    let manifest = manifest();
    PlatformApiContext {
        api_id: manifest.api_id,
        source_sha256: manifest.source.sha256,
        index: TOPICS[0].3.to_string(),
        topics: TOPICS
            .iter()
            .map(|(name, title, file, _)| PlatformApiTopic {
                name: (*name).into(),
                title: (*title).into(),
                bytes: manifest.files[*file].bytes,
                sha256: manifest.files[*file].sha256.clone(),
            })
            .collect::<Vec<_>>(),
    }
}

pub fn read(topic: &str) -> Option<ApiReferenceDocument> {
    let manifest = manifest();
    let (_, _, file, text) = TOPICS.iter().find(|(name, _, _, _)| *name == topic)?;
    Some(ApiReferenceDocument {
        api_id: manifest.api_id,
        topic: topic.into(),
        sha256: manifest.files[*file].sha256.clone(),
        text: (*text).into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{IdeRequest, IdeResponse, OwnerReply, encode};

    #[test]
    fn context_is_small_and_every_topic_fits_the_broker() {
        let context = context();
        assert!(context.index.len() < 8192);
        assert!(context.render_summary().contains("API version:"));
        assert!(serde_json::to_vec(&context).unwrap().len() < 12 * 1024);
        for topic in context.topics {
            let reference = read(&topic.name).unwrap();
            assert_eq!(reference.text.len(), topic.bytes);
            assert_eq!(reference.sha256, topic.sha256);
            assert_eq!(reference.api_id, context.api_id);
            encode(&OwnerReply {
                endpoint: 1,
                client_token: 1,
                request_id: 1,
                reply: IdeResponse::ApiReference { reference },
            })
            .unwrap();
        }
        let request: IdeRequest =
            serde_json::from_str(r#"{"action":"api_reference","topic":"terminal"}"#).unwrap();
        assert!(matches!(request, IdeRequest::ApiReference { topic } if topic == "terminal"));
    }

    #[test]
    fn reference_lookup_is_read_only_and_has_no_file_path_escape() {
        assert!(read("logging").unwrap().text.contains("trueos_cabi_log"));
        assert!(
            read("terminal")
                .unwrap()
                .text
                .contains("terminal_lease_release_v1")
        );
        for name in [
            "",
            "../protocol.rs",
            "manifest.json",
            "logging.prm",
            "/etc/passwd",
        ] {
            assert!(read(name).is_none());
        }
    }
}

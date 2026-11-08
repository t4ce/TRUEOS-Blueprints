//! Native socket capability selection belongs to native Cargo execution only.
//! Upstream host manifests must not name features absent from upstream Tokio.
use crate::BuildFlavor;
use std::path::Path;

pub(crate) fn add(
    manifest: &Path,
    flavor: BuildFlavor,
    features: &mut Vec<String>,
) -> Result<(), String> {
    if matches!(flavor, BuildFlavor::TokioStd) && crate::manifest_has_dependency(manifest, "tokio")?
    {
        crate::push_feature(features, "tokio/native-net");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_tokio_network_feature_is_deduplicated_and_flavor_scoped() {
        let root =
            std::env::temp_dir().join(format!("native-tokio-features-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let manifest = root.join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname=\"native-tokio\"\nversion=\"0.1.0\"\n[dependencies]\ntokio=\"1\"\n",
        )
        .unwrap();
        let mut features = Vec::new();
        add(&manifest, BuildFlavor::ThinNoStd, &mut features).unwrap();
        assert!(features.is_empty());
        add(&manifest, BuildFlavor::TokioStd, &mut features).unwrap();
        add(&manifest, BuildFlavor::TokioStd, &mut features).unwrap();
        assert_eq!(features, ["tokio/native-net"]);
        std::fs::write(
            &manifest,
            "[package]\nname=\"plain-std\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        features.clear();
        add(&manifest, BuildFlavor::TokioStd, &mut features).unwrap();
        assert!(features.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}

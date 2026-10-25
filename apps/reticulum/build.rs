use std::io::Result;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=proto/");
    let protoc = protoc_bin_vendored::protoc_bin_path()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err))?;
    let mut prost_config = tonic_prost_build::Config::new();
    prost_config.protoc_executable(protoc);

    // Generate proto files for Kaonic
    tonic_prost_build::configure()
        .server_mod_attribute(".", "#[allow(clippy::match_single_binding)]")
        .server_mod_attribute(".", "#[allow(clippy::mixed_attributes_style)]")
        .type_attribute(
            "ConfigurationRequest",
            "#[derive(serde::Deserialize, serde::Serialize)]",
        )
        .type_attribute(
            "RadioPhyConfigFSK",
            "#[derive(serde::Deserialize, serde::Serialize)]",
        )
        .type_attribute(
            "RadioPhyConfigOFDM",
            "#[derive(serde::Deserialize, serde::Serialize)]",
        )
        .type_attribute(
            "RadioPhyConfigQPSK",
            "#[derive(serde::Deserialize, serde::Serialize)]",
        )
        .type_attribute(
            "kaonic.ConfigurationRequest.phy_config",
            "#[derive(serde::Deserialize, serde::Serialize)]",
        )
        .type_attribute(
            "kaonic.ConfigurationRequest.phy_config",
            "#[serde(tag = \"type\", content = \"data\")]",
        )
        .compile_with_config(
            prost_config,
            &["proto/kaonic/kaonic.proto"],
            &["proto/kaonic"], // The directory containing your proto files
        )?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = prost_build::Config::new();
    config.type_attribute(
        ".",
        "#[allow(dead_code, clippy::enum_variant_names, clippy::doc_overindented_list_items)]",
    );
    config.compile_protos(
        &["src/proto/checkin.proto", "src/proto/mcs.proto"],
        &["src/proto"],
    )?;
    Ok(())
}

use std::{fs, path::Path};

#[test]
fn both_images_include_mpl_source_and_recipient_instructions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in ["Dockerfile", "packaging/home-assistant/Dockerfile"] {
        let recipe = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("{relative}: {error}"));
        assert!(recipe.contains("COPY SOURCE_AVAILABILITY.md /usr/share/doc/vistoda/"));
        assert!(
            recipe.contains("COPY --from=builder /licenses /usr/share/doc/vistoda/dependencies")
        );
    }
    let collector = fs::read_to_string(root.join("packaging/collect-licenses.sh"))
        .unwrap_or_else(|error| panic!("collector: {error}"));
    assert!(collector.contains("cargo tree --locked --edges normal -i ece@2.3.1"));
    assert!(collector.contains("tar -czf \"$destination/sources/ece-2.3.1.tar.gz\""));
    let instructions = fs::read_to_string(root.join("SOURCE_AVAILABILITY.md"))
        .unwrap_or_else(|error| panic!("instructions: {error}"));
    assert!(instructions.contains("/usr/share/doc/vistoda/dependencies/sources/ece-2.3.1.tar.gz"));
    assert!(instructions.contains("MPL-2.0"));
}

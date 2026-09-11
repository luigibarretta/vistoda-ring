use std::{fs, path::Path, process::Command};

fn device_config(options: &serde_json::Value) -> std::process::Output {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("temporary options: {error}"));
    let path = temp.path().join("options.json");
    fs::write(&path, options.to_string()).unwrap_or_else(|error| panic!("write options: {error}"));
    Command::new("sh")
        .args([
            "-eu",
            "-c",
            ". \"$HELPER\"; ring_device_config \"$OPTIONS\"",
        ])
        .env(
            "HELPER",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/home-assistant/device-config.sh"),
        )
        .env("OPTIONS", path)
        .output()
        .unwrap_or_else(|error| panic!("device config helper: {error}"))
}

#[test]
fn explicit_intercoms_are_unique_and_legacy_single_device_is_preserved() {
    let output = device_config(&serde_json::json!({"alias":"entrance", "intercoms":[]}));
    assert!(output.status.success());
    let config: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("config: {error}"));
    assert_eq!(
        config,
        serde_json::json!({"entrance":{"kind":"ring_intercom_audio"}})
    );
    let output = device_config(&serde_json::json!({"alias":"legacy", "intercoms":[
        {"alias":"entrance", "device_id":101}, {"alias":"gate", "device_id":202}
    ]}));
    assert!(output.status.success());
    let config: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("config: {error}"));
    assert_eq!(
        config.as_object().unwrap_or_else(|| panic!("object")).len(),
        2
    );
    assert_eq!(config["entrance"]["device_id"], 101);
    assert_eq!(config["gate"]["device_id"], 202);
    assert!(config.get("legacy").is_none());
}

#[test]
fn invalid_or_ambiguous_bindings_are_rejected_before_startup() {
    for intercoms in [
        serde_json::json!([{"alias":"a","device_id":1},{"alias":"b","device_id":1}]),
        serde_json::json!([{"alias":"a","device_id":1},{"alias":"a","device_id":2}]),
        serde_json::json!([{"alias":"a"}]),
        serde_json::json!([{"alias":"../a","device_id":1}]),
        serde_json::json!([{"alias":"a","device_id":0}]),
        serde_json::json!([{"alias":"a","device_id":1.5}]),
        serde_json::json!([{"alias":"a","device_id":9_007_199_254_740_992_u64}]),
        serde_json::json!({"alias":"a","device_id":1}),
    ] {
        assert!(
            !device_config(&serde_json::json!({"alias":"legacy", "intercoms":intercoms}))
                .status
                .success()
        );
    }
}

fn discovery_payload(inventory: Option<&serde_json::Value>) -> serde_json::Value {
    discovery_with_devices(
        inventory,
        r#"{"entrance":{"kind":"ring_intercom_audio","device_id":101},"gate":{"kind":"ring_intercom_audio","device_id":202}}"#,
    )
}

fn discovery_with_devices(
    inventory: Option<&serde_json::Value>,
    config: &str,
) -> serde_json::Value {
    let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let devices = temp.path().join("devices.json");
    let token = temp.path().join("api-token");
    fs::write(&devices, config).unwrap_or_else(|error| panic!("devices: {error}"));
    fs::write(&token, "a".repeat(64)).unwrap_or_else(|error| panic!("token: {error}"));
    let response = temp.path().join("inventory.json");
    if let Some(inventory) = inventory {
        fs::write(&response, inventory.to_string())
            .unwrap_or_else(|error| panic!("fixture: {error}"));
    }
    let runner = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/home-assistant/run.sh"),
    )
    .unwrap_or_else(|error| panic!("runner: {error}"));
    let discovery = runner
        .split_once("private_url=")
        .unwrap_or_else(|| panic!("discovery"))
        .1
        .split("\nvistoda_wait_child")
        .next()
        .unwrap_or_else(|| panic!("tail"));
    let script = format!(
        ". \"$HELPER\"\n\
         curl() {{ IFS= read -r authorization; test \"$authorization\" = \
         \"header = \\\"Authorization: Bearer $(cat \"$token_file\")\\\"\" || exit 91; \
         test \"$*\" = '--config - --silent --fail --connect-timeout 2 --max-time 10 \
         --max-filesize 131072 http://127.0.0.1:8775/v1/intercoms' || exit 92; \
         test -f \"$RESPONSE\" || return 22; cat \"$RESPONSE\"; }}\n\
         vistoda_publish_discovery() {{ cat; }}\nprivate_url={discovery}"
    );
    let output = Command::new("sh")
        .args(["-eu", "-c", &script])
        .env("app_hostname", "fixture-provider")
        .env("alias_name", "entrance")
        .env("devices_file", &devices)
        .env("token_file", &token)
        .env("RESPONSE", &response)
        .env(
            "HELPER",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/home-assistant/device-config.sh"),
        )
        .output()
        .unwrap_or_else(|error| panic!("discovery helper: {error}"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("one JSON payload: {error}"))
}

#[test]
fn supervisor_gets_one_complete_discovery_payload_before_enrollment() {
    let payload = discovery_payload(None);
    assert_eq!(payload["service"], "media_bridge");
    assert_eq!(payload["config"]["alias"], "entrance");
    assert_eq!(
        payload["config"]["aliases"],
        serde_json::json!(["entrance", "gate"])
    );
    assert_eq!(payload["config"]["devices"][1]["device_id"], "202");
    assert_eq!(payload["config"]["url"], "http://fixture-provider:8775");
}

#[test]
fn enrolled_restart_discovers_physical_aliases_without_account_metadata() {
    let payload = discovery_payload(Some(&serde_json::json!({"intercoms":[
        {"alias":"intercom-41","device_id":"41","name":"Private","location_name":"Home"},
        {"alias":"intercom-42","device_id":"42","token":"never-publish"}
    ]})));
    assert_eq!(payload["config"]["alias"], "intercom-41");
    assert_eq!(
        payload["config"]["aliases"],
        serde_json::json!(["intercom-41", "intercom-42"])
    );
    assert_eq!(
        payload["config"]["devices"],
        serde_json::json!([
            {"alias":"intercom-41","device_id":"41"},{"alias":"intercom-42","device_id":"42"}
        ])
    );
    assert!(!payload.to_string().contains("Private"));
    assert!(!payload.to_string().contains("never-publish"));
}

#[test]
fn default_bootstrap_has_no_invented_identity_and_large_ids_remain_exact() {
    let payload = discovery_with_devices(None, r#"{"entrance":{"kind":"ring_intercom_audio"}}"#);
    assert_eq!(
        payload["config"]["devices"],
        serde_json::json!([
            {"alias":"entrance","device_id":null}
        ])
    );
    let payload = discovery_payload(Some(&serde_json::json!({"intercoms":[
        {"alias":"intercom-18446744073709551615","device_id":"18446744073709551615"}
    ]})));
    assert_eq!(
        payload["config"]["devices"][0]["device_id"],
        "18446744073709551615"
    );
}

#[test]
fn invalid_or_unbounded_inventory_preserves_bootstrap_discovery() {
    for inventory in [
        serde_json::json!({"intercoms":[]}),
        serde_json::json!({"intercoms":[{"alias":"bad/name","device_id":"1"}]}),
        serde_json::json!({"intercoms":[{"alias":"a","device_id":1}]}),
        serde_json::json!({"intercoms":[{"alias":"a","device_id":"18446744073709551616"}]}),
        serde_json::json!({"intercoms":[{"alias":"a","device_id":"1"},{"alias":"b","device_id":"1"}]}),
        serde_json::json!({"intercoms":[{"alias":"a","device_id":"1"},{"alias":"a","device_id":"2"}]}),
        serde_json::json!({"intercoms":vec![serde_json::json!({"alias":"a","device_id":"1"});33]}),
    ] {
        let payload = discovery_payload(Some(&inventory));
        assert_eq!(payload["config"]["alias"], "entrance");
        assert_eq!(payload["config"]["devices"][1]["device_id"], "202");
    }
}

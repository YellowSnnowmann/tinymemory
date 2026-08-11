//! The config is a wire contract with the host, so these pin its shape.

use super::ModuleConfig;

#[test]
fn an_absent_workspace_is_refused() {
    // The one field with no defensible default. Silently resolving an empty path
    // against the process working directory would put a user's memory store
    // somewhere nobody would look for it.
    let config = ModuleConfig::default();
    assert!(config.validate().is_err());
}

#[test]
fn a_workspace_alone_is_enough() {
    // Everything else has a defensible default, so a host that supplies only a
    // workspace gets a working module rather than a validation error.
    let config = ModuleConfig {
        workspace_dir: "/tmp/does-not-need-to-exist".into(),
        ..ModuleConfig::default()
    };
    assert!(config.validate().is_ok(), "{:?}", config.validate());
}

#[test]
fn the_refusal_names_the_field_but_never_a_path() {
    // A path can identify a user, and module errors must not carry absolute
    // paths. The empty case has no path to leak, so this guards the wording
    // rather than the value.
    let error = ModuleConfig::default().validate().unwrap_err();
    assert!(error.contains("workspace_dir"), "{error}");
}

#[test]
fn an_empty_json_object_deserializes() {
    // `#[serde(default)]` on the struct is what lets a host send `{}` and get
    // engine defaults. Without it a host would have to mirror every field.
    let config: ModuleConfig = serde_json::from_str("{}").expect("empty object is valid");
    assert_eq!(config.driver_id, tinymemory::registry::TINYCORTEX_DRIVER_ID);
}

#[test]
fn the_default_driver_id_is_the_engine_this_module_carries() {
    // A driver id appears in status output and audit events, so a module that
    // advertised something else would make the host's records wrong.
    assert_eq!(
        ModuleConfig::default().driver_id,
        tinymemory::registry::TINYCORTEX_DRIVER_ID
    );
}

#[test]
fn there_is_no_field_that_could_hold_a_credential() {
    // The central claim of this module, asserted structurally rather than
    // trusted: serialize a fully-populated config and confirm the JSON has no
    // key an api key, token or secret could arrive through. A field added later
    // with such a name fails here, which is the point — the reviewer is then
    // forced to argue for it rather than land it quietly.
    let config = ModuleConfig {
        workspace_dir: "/tmp/w".into(),
        ollama_base_url: "http://localhost:11434".to_string(),
        cloud_embedding_model: "text-embedding-3-small".to_string(),
        cloud_embedding_dimensions: 1536,
        models_supporting_dimensions: vec!["text-embedding-3-small".to_string()],
        ..ModuleConfig::default()
    };

    let json = serde_json::to_string(&config).expect("config serializes");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    let object = value.as_object().expect("config is a json object");

    for key in object.keys() {
        let lowered = key.to_ascii_lowercase();
        for forbidden in [
            "api_key",
            "apikey",
            "token",
            "secret",
            "password",
            "credential",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "config field {key:?} looks like it carries a credential; \
                 embeddings go over the bus precisely so it does not have to"
            );
        }
    }
}

#[test]
fn a_credential_nested_in_the_memory_config_is_stripped() {
    // The hole the test above cannot see. `MemoryConfig` is carried verbatim and
    // contains `agentmemory_secret`, a bearer token — so "this struct has no
    // credential field" was true and still not enough.
    let mut config = ModuleConfig {
        workspace_dir: "/tmp/w".into(),
        ..ModuleConfig::default()
    };
    config.memory.agentmemory_secret = Some("bearer-token-value".to_string());

    assert!(
        config.strip_host_credentials(),
        "it should report removing one"
    );
    assert!(config.memory.agentmemory_secret.is_none());

    // And the token must not survive anywhere in the serialized form.
    let json = serde_json::to_string(&config).expect("serializes");
    assert!(!json.contains("bearer-token-value"), "{json}");
}

#[test]
fn stripping_a_config_without_a_credential_reports_nothing_removed() {
    // So the caller's warning fires only when something actually was removed.
    let mut config = ModuleConfig {
        workspace_dir: "/tmp/w".into(),
        ..ModuleConfig::default()
    };
    assert!(!config.strip_host_credentials());
}

#[test]
fn stripping_is_idempotent() {
    // Setup runs it once, but a second call must not report a phantom removal.
    let mut config = ModuleConfig {
        workspace_dir: "/tmp/w".into(),
        ..ModuleConfig::default()
    };
    config.memory.agentmemory_secret = Some("t".to_string());
    assert!(config.strip_host_credentials());
    assert!(!config.strip_host_credentials());
}

#[test]
fn a_populated_config_round_trips() {
    let config = ModuleConfig {
        workspace_dir: "/tmp/w".into(),
        ollama_base_url: "http://localhost:11434".to_string(),
        cloud_embedding_model: "m".to_string(),
        cloud_embedding_dimensions: 8,
        models_supporting_dimensions: vec!["m".to_string()],
        driver_id: "tinycortex".to_string(),
        ..ModuleConfig::default()
    };

    let json = serde_json::to_string(&config).expect("serializes");
    let back: ModuleConfig = serde_json::from_str(&json).expect("deserializes");

    assert_eq!(back.workspace_dir, config.workspace_dir);
    assert_eq!(back.cloud_embedding_dimensions, 8);
    assert_eq!(back.models_supporting_dimensions, vec!["m".to_string()]);
    assert_eq!(back.driver_id, "tinycortex");
}

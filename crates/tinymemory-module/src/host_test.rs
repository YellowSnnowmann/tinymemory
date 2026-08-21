//! Tests for runtime-host callback argument ownership and safe diagnostics.

#[test]
fn callback_tags_are_owned_without_changing_order_or_values() {
    let key = String::from("source");
    let value = String::from("sync");
    let owned = super::owned_tags(&[(&key, &value), ("attempt", "2")]);
    drop(key);
    drop(value);
    assert_eq!(
        owned,
        vec![
            ("source".to_string(), "sync".to_string()),
            ("attempt".to_string(), "2".to_string())
        ]
    );
}

#[tokio::test]
async fn absent_runtime_host_returns_an_error_instead_of_hanging() {
    use tinybus::transport::memory::MemoryBus;
    use tinymemory_core::nlp_host::NlpHost;

    let bus = MemoryBus::new();
    let broker = tinybus::broker::Broker::new();
    let _broker_task = broker.spawn(bus.clone());
    let connection = tinybus::Connection::connect(bus.connect().await.expect("transport"))
        .await
        .expect("connection");
    let host = super::BusRuntimeHost::new(connection);
    let config = crate::config::ModuleConfig::default();
    let runtime = tinymemory_tinycortex::engine::EngineRuntimeConfig::from(&config);
    let error = host
        .extract_spacy(&runtime, "text")
        .await
        .expect_err("no runtime host is served");
    assert!(error.contains(super::RUNTIME_HOST_BUS_NAME), "{error}");
    assert!(!format!("{host:?}").contains("Connection"));
}

//! Tests for runtime-host callback argument ownership and safe diagnostics.

use tinybus::broker::Broker;
use tinybus::transport::memory::MemoryBus;
use tinybus::{Connection, Result as BusResult};
use tinymemory_api::host::{
    ErrorReporter, MemoryEvent, MemoryEventSink, SpacyEntity, SpacyResponse,
};

struct HostSeamsRestore {
    event_sink: Option<std::sync::Arc<dyn MemoryEventSink>>,
    error_reporter: Option<std::sync::Arc<dyn ErrorReporter>>,
    nlp_host: Option<std::sync::Arc<dyn tinymemory_core::nlp_host::NlpHost>>,
    scheduler_gate: Option<std::sync::Arc<dyn tinymemory_core::scheduler_gate::SchedulerGate>>,
    shutdown_host: Option<std::sync::Arc<dyn tinymemory_core::shutdown::ShutdownHost>>,
}

impl HostSeamsRestore {
    fn capture() -> Self {
        Self {
            event_sink: tinymemory_core::events::event_sink(),
            error_reporter: tinymemory_core::observability::error_reporter(),
            nlp_host: tinymemory_core::nlp_host::nlp_host(),
            scheduler_gate: tinymemory_core::scheduler_gate::scheduler_gate(),
            shutdown_host: tinymemory_core::shutdown::shutdown_host(),
        }
    }
}

impl Drop for HostSeamsRestore {
    fn drop(&mut self) {
        match self.event_sink.take() {
            Some(sink) => tinymemory_core::events::set_event_sink(sink),
            None => tinymemory_core::events::clear_event_sink(),
        }
        match self.error_reporter.take() {
            Some(reporter) => tinymemory_core::observability::set_error_reporter(reporter),
            None => tinymemory_core::observability::clear_error_reporter(),
        }
        match self.nlp_host.take() {
            Some(host) => tinymemory_core::nlp_host::set_nlp_host(host),
            None => tinymemory_core::nlp_host::clear_nlp_host(),
        }
        match self.scheduler_gate.take() {
            Some(gate) => tinymemory_core::scheduler_gate::set_scheduler_gate(gate),
            None => tinymemory_core::scheduler_gate::clear_scheduler_gate(),
        }
        match self.shutdown_host.take() {
            Some(host) => tinymemory_core::shutdown::set_shutdown_host(host),
            None => tinymemory_core::shutdown::clear_shutdown_host(),
        }
    }
}

#[derive(Debug)]
enum Callback {
    Published(MemoryEvent),
    Error {
        expected: bool,
        rendered: String,
        domain: String,
        operation: String,
        tags: Vec<(String, String)>,
    },
}

struct FakeRuntimeHost {
    callbacks: tokio::sync::mpsc::UnboundedSender<Callback>,
}

#[tinybus::interface(name = "ai.tinyhumans.tinymemory.RuntimeHost")]
impl FakeRuntimeHost {
    async fn publish_event(&self, event: MemoryEvent) -> BusResult<()> {
        std::future::ready(()).await;
        let _ = self.callbacks.send(Callback::Published(event));
        Ok(())
    }

    #[allow(clippy::too_many_arguments, reason = "wire contract")]
    async fn report_error(
        &self,
        expected: bool,
        rendered: String,
        domain: String,
        operation: String,
        tags: Vec<(String, String)>,
    ) -> BusResult<()> {
        std::future::ready(()).await;
        let _ = self.callbacks.send(Callback::Error {
            expected,
            rendered,
            domain,
            operation,
            tags,
        });
        Ok(())
    }

    async fn extract_spacy(&self, text: String) -> BusResult<SpacyResponse> {
        std::future::ready(()).await;
        Ok(SpacyResponse {
            entities: vec![SpacyEntity {
                text,
                label: "ORG".to_string(),
                start: 0,
                end: 10,
            }],
            nouns: vec!["memory".to_string()],
        })
    }
}

async fn bus_with_runtime_host() -> (Connection, tokio::sync::mpsc::UnboundedReceiver<Callback>) {
    let bus = MemoryBus::new();
    let broker = Broker::new();
    let _broker_task = broker.spawn(bus.clone());
    let (callbacks, receiver) = tokio::sync::mpsc::unbounded_channel();
    let host = Connection::connect(bus.connect().await.expect("host transport"))
        .await
        .expect("host connection");
    host.serve_at(
        super::RUNTIME_HOST_OBJECT_PATH
            .try_into()
            .expect("runtime host path"),
        FakeRuntimeHost { callbacks },
    )
    .await
    .expect("serve runtime host");
    host.request_name(super::RUNTIME_HOST_BUS_NAME)
        .await
        .expect("claim runtime host name");
    std::mem::forget(host);
    let module = Connection::connect(bus.connect().await.expect("module transport"))
        .await
        .expect("module connection");
    (module, receiver)
}

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

#[tokio::test]
async fn runtime_callbacks_and_spacy_cross_the_bus_with_their_full_payloads() {
    use tinymemory_core::nlp_host::NlpHost;

    let (connection, mut callbacks) = bus_with_runtime_host().await;
    let host = super::BusRuntimeHost::new(connection);
    host.publish(MemoryEvent::IngestionStarted {
        document_id: "document-7".to_string(),
        title: "Coverage".to_string(),
        namespace: "test".to_string(),
        queue_depth: 3,
    });
    host.report_error("failed", "sync", "publish", &[("source", "unit-test")]);
    host.report_error_or_expected("not found", "recall", "lookup", &[("namespace", "test")]);

    let config = crate::config::ModuleConfig::default();
    let runtime = tinymemory_tinycortex::engine::EngineRuntimeConfig::from(&config);
    let response = host
        .extract_spacy(&runtime, "TinyMemory")
        .await
        .expect("runtime host extracts entities");
    assert_eq!(response.entities.len(), 1);
    assert_eq!(response.entities[0].text, "TinyMemory");
    assert_eq!(response.entities[0].label, "ORG");
    assert_eq!(response.nouns, ["memory"]);

    let mut published = false;
    let mut ordinary_error = false;
    let mut expected_error = false;
    for _ in 0..3 {
        let callback = tokio::time::timeout(std::time::Duration::from_secs(1), callbacks.recv())
            .await
            .expect("callback arrives promptly")
            .expect("callback channel remains open");
        match callback {
            Callback::Published(MemoryEvent::IngestionStarted {
                document_id,
                title,
                namespace,
                queue_depth,
            }) => {
                assert_eq!(document_id, "document-7");
                assert_eq!(title, "Coverage");
                assert_eq!(namespace, "test");
                assert_eq!(queue_depth, 3);
                published = true;
            }
            Callback::Published(other) => panic!("unexpected event: {other:?}"),
            Callback::Error {
                expected,
                rendered,
                domain,
                operation,
                tags,
            } => {
                if expected {
                    assert_eq!(rendered, "not found");
                    assert_eq!(domain, "recall");
                    assert_eq!(operation, "lookup");
                    assert_eq!(tags, [("namespace".to_string(), "test".to_string())]);
                    expected_error = true;
                } else {
                    assert_eq!(rendered, "failed");
                    assert_eq!(domain, "sync");
                    assert_eq!(operation, "publish");
                    assert_eq!(tags, [("source".to_string(), "unit-test".to_string())]);
                    ordinary_error = true;
                }
            }
        }
    }
    assert!(published);
    assert!(ordinary_error);
    assert!(expected_error);
}

/// Kept as one test rather than two on purpose: both installs write
/// process-global seams, and a second test that captured, installed and
/// asserted in parallel with this one could have its assertion land after this
/// one's `HostSeamsRestore` had already put the globals back.
#[tokio::test]
async fn install_wires_every_seam_this_module_can_supply() {
    let _restore = HostSeamsRestore::capture();
    let (connection, _callbacks) = bus_with_runtime_host().await;

    // The pair `setup` calls, in the order it calls them.
    super::install(connection);
    super::install_seams(None);

    assert!(tinymemory_core::events::event_sink().is_some());
    assert!(tinymemory_core::observability::error_reporter().is_some());
    assert!(tinymemory_core::nlp_host::nlp_host().is_some());
    // The two that used to be left out entirely, and so degraded in silence
    // instead of failing with a named cause the way `config_loader` does.
    assert!(tinymemory_core::scheduler_gate::scheduler_gate().is_some());
    assert!(tinymemory_core::shutdown::shutdown_host().is_some());
}

#[test]
fn the_unserved_stubs_answer_exactly_what_an_unwired_seam_answered() {
    use tinymemory_core::scheduler_gate::SchedulerGate;
    use tinymemory_core::shutdown::ShutdownHost;

    // Loud, not different. A stub that answered anything else would change
    // scheduling as a side effect of loading the module — and with no channel
    // to the host's live gate, any other answer would be a guess that goes
    // stale the moment the user toggles background AI.
    assert_eq!(
        super::UnservedSchedulerGate.current_policy(),
        tinymemory_core::scheduler_gate::Policy::Normal
    );
    // Registering with nowhere to run reports and drops; it must never panic.
    let hook: tinymemory_core::shutdown::ShutdownHook = Box::new(|| Box::pin(async {}));
    super::UnservedShutdownHost.register(hook);
}

#[tokio::test]
async fn fire_and_forget_notification_tolerates_an_absent_host() {
    let bus = MemoryBus::new();
    let broker = Broker::new();
    let _broker_task = broker.spawn(bus.clone());
    let connection = Connection::connect(bus.connect().await.expect("transport"))
        .await
        .expect("connection");
    let host = super::BusRuntimeHost::new(connection);

    host.publish(MemoryEvent::IngestionStarted {
        document_id: "missing-host".to_string(),
        title: String::new(),
        namespace: "test".to_string(),
        queue_depth: 0,
    });
    tokio::task::yield_now().await;
}

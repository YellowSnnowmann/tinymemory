//! Tests for the host-owned Composio bridge over an in-memory TinyBus.
//!
//! Every test here runs multi-threaded with an explicit worker count, and both
//! halves of that matter. `api_key` and `is_available` block their calling
//! thread while the host answers, so the broker and the fake host's dispatch
//! loop need a worker that is not the one waiting — and `worker_threads`
//! defaults to the core count, which on a one-core CI box would leave exactly
//! one. Pinning it makes the test independent of the machine rather than
//! hanging on the small ones.

use tinybus::broker::Broker;
use tinybus::transport::memory::MemoryBus;
use tinybus::{Connection, Result as BusResult};
use tinymemory_core::composio_host::{ComposioConnection, ComposioExecuteResponse, ComposioHost};

use super::{
    BusComposioHost, COMPOSIO_HOST_BUS_NAME, COMPOSIO_HOST_OBJECT_PATH, COMPOSIO_UNSERVED,
};
use crate::config::ModuleConfig;

/// What the fake host was asked to execute.
#[derive(Debug)]
struct Executed {
    tool: String,
    arguments: Option<serde_json::Value>,
    entity_id: String,
    connection_id: Option<String>,
}

struct FakeComposioHost {
    executed: tokio::sync::mpsc::UnboundedSender<Executed>,
    api_key: Option<String>,
    available: bool,
}

#[tinybus::interface(name = "ai.tinyhumans.tinymemory.ComposioHost")]
impl FakeComposioHost {
    async fn list_connections(&self) -> BusResult<Vec<ComposioConnection>> {
        std::future::ready(()).await;
        Ok(vec![ComposioConnection {
            id: "connection-1".to_string(),
            toolkit: "Gmail".to_string(),
            status: "ACTIVE".to_string(),
            created_at: None,
            account_email: Some("user@example.com".to_string()),
            workspace: None,
            username: None,
        }])
    }

    async fn execute(
        &self,
        tool: String,
        arguments: Option<serde_json::Value>,
        entity_id: String,
        connection_id: Option<String>,
    ) -> BusResult<ComposioExecuteResponse> {
        std::future::ready(()).await;
        let _ = self.executed.send(Executed {
            tool,
            arguments,
            entity_id,
            connection_id,
        });
        Ok(ComposioExecuteResponse {
            data: serde_json::json!({ "messages": 2 }),
            successful: true,
            error: None,
            cost_usd: 0.25,
            markdown_formatted: Some("two messages".to_string()),
        })
    }

    async fn api_key(&self) -> BusResult<Option<String>> {
        std::future::ready(()).await;
        Ok(self.api_key.clone())
    }

    async fn is_available(&self) -> BusResult<bool> {
        std::future::ready(()).await;
        Ok(self.available)
    }
}

async fn bus_with_composio_host(
    api_key: Option<&str>,
    available: bool,
) -> (Connection, tokio::sync::mpsc::UnboundedReceiver<Executed>) {
    let bus = MemoryBus::new();
    let broker = Broker::new();
    let _broker_task = broker.spawn(bus.clone());
    let (executed, receiver) = tokio::sync::mpsc::unbounded_channel();
    let host = Connection::connect(bus.connect().await.expect("host transport"))
        .await
        .expect("host connection");
    host.serve_at(
        COMPOSIO_HOST_OBJECT_PATH.try_into().expect("object path"),
        FakeComposioHost {
            executed,
            api_key: api_key.map(str::to_string),
            available,
        },
    )
    .await
    .expect("serve composio host");
    host.request_name(COMPOSIO_HOST_BUS_NAME)
        .await
        .expect("claim composio host name");
    std::mem::forget(host);
    let module = Connection::connect(bus.connect().await.expect("module transport"))
        .await
        .expect("module connection");
    (module, receiver)
}

/// A bare connection with nobody serving the Composio name.
async fn bus_without_composio_host() -> Connection {
    let bus = MemoryBus::new();
    let broker = Broker::new();
    let _broker_task = broker.spawn(bus.clone());
    Connection::connect(bus.connect().await.expect("transport"))
        .await
        .expect("connection")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connections_execute_and_probes_all_cross_the_composio_bridge() {
    let (connection, mut executed) = bus_with_composio_host(Some("direct-key"), true).await;
    let bridge = BusComposioHost::new(connection);
    let config = tinymemory_tinycortex::engine::EngineRuntimeConfig::from(&ModuleConfig::default());

    let connections = bridge
        .list_connections(&config)
        .await
        .expect("host lists connections");
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].id, "connection-1");
    // The engine normalises the slug itself; the bridge must not do it for the
    // host, or a toolkit would arrive pre-mangled on one path only.
    assert_eq!(connections[0].toolkit, "Gmail");
    assert!(connections[0].is_active());

    let response = bridge
        .execute(
            &config,
            "GMAIL_FETCH_EMAILS",
            Some(serde_json::json!({ "max_results": 5 })),
            "entity-7",
            Some("connection-1"),
        )
        .await
        .expect("host executes the tool");
    assert!(response.successful);
    assert!((response.cost_usd - 0.25).abs() < f64::EPSILON);
    assert_eq!(response.markdown_formatted.as_deref(), Some("two messages"));
    assert_eq!(response.data["messages"], 2);

    // Every argument the engine passed reaches the host unchanged. `entity_id`
    // and `connection_id` matter most: backend mode ignores both, so a bridge
    // that dropped them would look correct until a user switched to direct
    // mode and their connection pin silently disappeared.
    let recorded = executed.try_recv().expect("execute reached the host");
    assert_eq!(recorded.tool, "GMAIL_FETCH_EMAILS");
    assert_eq!(recorded.entity_id, "entity-7");
    assert_eq!(recorded.connection_id.as_deref(), Some("connection-1"));
    assert_eq!(
        recorded.arguments,
        Some(serde_json::json!({ "max_results": 5 }))
    );

    assert_eq!(bridge.api_key(&config).as_deref(), Some("direct-key"));
    assert!(bridge.is_available(&config));

    let rendered = format!("{bridge:?}");
    assert!(rendered.contains("BusComposioHost"), "{rendered}");
    assert!(!rendered.contains("Connection"), "{rendered}");
}

/// A served host that answers "no" is the only thing that reads as "no".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_host_that_answers_no_is_believed() {
    let (connection, _executed) = bus_with_composio_host(None, false).await;
    let bridge = BusComposioHost::new(connection);
    let config = tinymemory_tinycortex::engine::EngineRuntimeConfig::from(&ModuleConfig::default());

    assert!(bridge.api_key(&config).is_none());
    assert!(!bridge.is_available(&config));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unserved_host_is_named_and_the_probes_bias_towards_a_loud_failure() {
    let bridge = BusComposioHost::new(bus_without_composio_host().await);
    let config = tinymemory_tinycortex::engine::EngineRuntimeConfig::from(&ModuleConfig::default());

    let error = bridge
        .list_connections(&config)
        .await
        .expect_err("no composio host is served");
    assert!(error.contains(COMPOSIO_UNSERVED), "{error}");
    assert!(error.contains(COMPOSIO_HOST_BUS_NAME), "{error}");
    assert!(error.contains(super::LIST_CONNECTIONS_METHOD), "{error}");

    // The structural fact is latched, so later probes answer locally instead of
    // re-dialling a name that is known to have no owner.
    assert!(format!("{bridge:?}").contains("host_serves: false"));

    // `None` here becomes "Composio direct API key is not configured" one frame
    // up, which is a named refusal rather than a silent skip.
    assert!(bridge.api_key(&config).is_none());
    // And this stays `true` on purpose: a `false` would make the sync layer
    // report "not signed in" and skip quietly, where `true` lets the next call
    // fail with the unserved message asserted above.
    assert!(bridge.is_available(&config));

    let execute_error = bridge
        .execute(&config, "GMAIL_FETCH_EMAILS", None, "entity-7", None)
        .await
        .expect_err("no composio host is served");
    assert!(execute_error.contains(COMPOSIO_UNSERVED), "{execute_error}");
}

/// An `Execute` failure must never quote what was executed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_execute_failure_never_carries_its_arguments() {
    let bridge = BusComposioHost::new(bus_without_composio_host().await);
    let config = tinymemory_tinycortex::engine::EngineRuntimeConfig::from(&ModuleConfig::default());

    let error = bridge
        .execute(
            &config,
            "GMAIL_FETCH_EMAILS",
            Some(serde_json::json!({ "query": "from:accountant@example.com" })),
            "entity-7",
            Some("connection-1"),
        )
        .await
        .expect_err("no composio host is served");
    assert!(!error.contains("accountant@example.com"), "{error}");
    assert!(!error.contains("query"), "{error}");
}

#[test]
fn only_the_nobody_is_listening_family_reads_as_unserved() {
    let unserved = |name: &str| {
        super::is_unserved(&tinybus::Error::MethodFailed {
            name: name.to_string(),
            message: "irrelevant".to_string(),
        })
    };

    // Every remote failure arrives as `MethodFailed`, so these four names are
    // the only evidence this side gets that nobody is listening. `UnknownMethod`
    // is the one an older host with a newer module actually produces.
    assert!(unserved("ai.tinyhumans.tinybus.Error.NameHasNoOwner"));
    assert!(unserved("ai.tinyhumans.tinybus.Error.UnknownObject"));
    assert!(unserved("ai.tinyhumans.tinybus.Error.UnknownInterface"));
    assert!(unserved("ai.tinyhumans.tinybus.Error.UnknownMethod"));

    // A host that ran the method and failed is a working seam having a bad day:
    // reporting it as unserved would latch the bridge off for the rest of the
    // process over one expired session.
    assert!(!unserved("ai.tinyhumans.tinymemory.Error.Host"));
    assert!(!unserved("ai.tinyhumans.tinybus.Error.Failed"));
    assert!(!unserved("ai.tinyhumans.tinybus.Error.Timeout"));
}

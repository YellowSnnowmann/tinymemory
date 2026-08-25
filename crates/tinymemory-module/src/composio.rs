//! Composio stays host-side; only the request crosses.
//!
//! # What this seam is for
//!
//! The engine's memory-sync layer needs four things from Composio: which
//! connections the signed-in user has, the ability to run one tool against one
//! of them, the direct-mode API key, and a cheap "is any of this wired up?"
//! probe. It needs none of the rest of the integration — OAuth, the backend
//! session, the toolkit allowlist, HMAC-verified trigger fan-out, or the choice
//! between backend-proxied and direct mode. `tinymemory_core::composio_host`
//! draws that line, and this module is the module-mode implementation of the
//! engine's half.
//!
//! # Why a proxy and not an answer from `ModuleConfig`
//!
//! Because none of the four is a *value*; all four are live host state. The
//! connection list changes when the user completes an OAuth flow in a browser,
//! the direct key changes on a `set_api_key` RPC, and neither restarts
//! anything. A load-time snapshot would report the state as it was when the
//! module loaded and keep reporting it for the life of the process — which for
//! `is_available` means telling the sync layer "not signed in" about a user who
//! signed in a minute ago, and the sync layer treats that as *skip silently*.
//! That is the looks-empty-rather-than-broken failure this whole seam exists to
//! prevent, so the answer has to come from the host at call time.
//!
//! It is the same reasoning [`crate::embedding`] gives for keeping the embed
//! host-side, and the opposite of the one [`crate::config_loader`] gives for
//! answering the config locally — the deciding question in both directions is
//! whether the host holds something the module cannot be handed once.
//!
//! # The credential does cross here, unlike everywhere else
//!
//! [`crate::embedding`] refuses to carry an inference key and this crate's
//! module docs say the module carries no credentials. `ApiKey` is the exception
//! and it is worth naming rather than hiding: the engine's
//! `sync::pipelines::host::composio_config` builds its **own** HTTP client from
//! the direct-mode key, so unlike an embed there is no host-side call to route
//! the work through. A `None` here is therefore not a degraded answer the
//! caller works around — it is "direct-mode sync cannot run at all".
//!
//! The property that survives is the one that was actually load-bearing:
//! [`crate::config::ModuleConfig`] still has nowhere to *hold* a credential, so
//! the key exists in this address space only for the duration of one call and
//! only when the sync layer asked for it. Narrowing this further means moving
//! the direct-mode sync client behind an `Execute`-shaped method, which is a
//! change to the engine's contract rather than something to smuggle in through
//! one of its two halves.

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use tinybus::Connection;
use tinymemory_core::composio_host::{ComposioConnection, ComposioExecuteResponse, ComposioHost};

use crate::host::report_unserved_once;

/// Well-known name the host serves its Composio integration under.
pub const COMPOSIO_HOST_BUS_NAME: &str = "ai.tinyhumans.tinymemory.ComposioHost";

/// Object path the host serves it at.
pub const COMPOSIO_HOST_OBJECT_PATH: &str = "/ai/tinyhumans/tinymemory/ComposioHost";

/// Interface the host serves at [`COMPOSIO_HOST_OBJECT_PATH`].
///
/// Equal to [`COMPOSIO_HOST_BUS_NAME`] by convention, but a separate constant
/// for the same reason [`crate::embedding::EMBEDDING_HOST_INTERFACE`] is: one
/// addresses a peer, the other selects a dispatch table on that peer's object.
pub const COMPOSIO_HOST_INTERFACE: &str = "ai.tinyhumans.tinymemory.ComposioHost";

/// Every connection the signed-in user has, active or not.
pub const LIST_CONNECTIONS_METHOD: &str = "ListConnections";

/// Run one Composio tool against one connection.
///
/// Takes `(tool, arguments, entity_id, connection_id)`. The last two travel
/// even though backend mode ignores both: which mode is in force is resolved
/// host-side at call time, and omitting them would silently drop the connection
/// pin the moment a user switched to direct mode.
pub const EXECUTE_METHOD: &str = "Execute";

/// The direct-mode Composio API key, or `None` when direct mode is unset.
pub const API_KEY_METHOD: &str = "ApiKey";

/// Whether *some* viable Composio client resolves host-side right now.
pub const IS_AVAILABLE_METHOD: &str = "IsAvailable";

/// The OpenHuman backend bearer for proxied mode, or `None` when signed out.
///
/// Asked per call rather than carried in `ModuleConfig` because it is a session
/// JWT the host refreshes: a snapshot works until it expires and then reads as
/// a signed-out user on every subsequent sync.
pub const SESSION_BEARER_METHOD: &str = "SessionBearer";

/// Latched so the gap is reported once per process rather than once per sync
/// tick — the periodic scheduler consults this seam on every tick, and an
/// unlatched report would page on every one of them. Same guard the scheduler
/// gate and the shutdown host in `crate::host` put on theirs.
static COMPOSIO_REPORTED: AtomicBool = AtomicBool::new(false);

/// What an unserved Composio host costs, in the terms a reader of the log
/// needs.
const COMPOSIO_UNSERVED: &str = "composio host unserved in module mode: this host serves no \
                                 `ai.tinyhumans.tinymemory.ComposioHost` interface, so memory \
                                 sync cannot list connections, run a Composio tool, or resolve a \
                                 direct-mode key — every synced source stops updating";

/// What a probe made from outside a Tokio runtime costs.
///
/// `api_key` and `is_available` are synchronous on the engine's trait and a bus
/// call is not, so they need a runtime handle to bridge onto. Every caller in
/// the engine reaches them from inside one; a caller that did not would get a
/// silent `None`/`false` without this.
const COMPOSIO_NO_RUNTIME: &str = "composio host probe made outside a Tokio runtime: the \
                                   synchronous `api_key`/`is_available` probes bridge onto the \
                                   module runtime to reach the host, and without one they cannot \
                                   ask — direct-mode sync will report its key as unconfigured";

/// The Composio integration, reached over the module's connection.
pub struct BusComposioHost {
    connection: Connection,
    /// Cleared the first time a call proves the host serves no Composio
    /// interface at all.
    ///
    /// Not a cache of the *user's* Composio state — that is deliberately never
    /// cached, see the module docs. This records one structural fact about the
    /// host, which cannot change while the process runs: tinybus never unloads
    /// a library and a host that did not serve the interface at load will not
    /// grow one. Recording it turns every later probe into a local answer
    /// instead of a round trip that is already known to fail.
    host_serves: AtomicBool,
}

// `Connection` is not `Debug`, and `ComposioHost` requires it. Rendering the
// connection would say nothing useful anyway, and this type's only other field
// is a latch.
impl std::fmt::Debug for BusComposioHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BusComposioHost")
            .field("host_serves", &self.host_serves.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl BusComposioHost {
    /// Build the host bridge over the module connection.
    ///
    /// Takes no configuration on purpose: everything this seam answers is live
    /// host state, so there is nothing about it worth capturing at load time.
    #[must_use]
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            host_serves: AtomicBool::new(true),
        }
    }

    /// Call one member of the host's Composio interface.
    ///
    /// # Errors
    ///
    /// The named-unserved message when the host exports no such interface,
    /// otherwise the bus failure with the member that produced it. Never
    /// carries `arguments`: a Composio tool call's arguments are mail queries,
    /// document bodies and connection pins, and an error string is not a place
    /// for any of them.
    async fn call<R>(
        &self,
        member: &'static str,
        arguments: impl serde::Serialize + Send,
    ) -> Result<R, String>
    where
        R: serde::de::DeserializeOwned,
    {
        let proxy = self
            .connection
            .proxy(
                COMPOSIO_HOST_BUS_NAME,
                COMPOSIO_HOST_OBJECT_PATH,
                COMPOSIO_HOST_INTERFACE,
            )
            .map_err(|error| self.classify(member, &error))?;
        proxy
            .call(member, arguments)
            .await
            .map_err(|error| self.classify(member, &error))
    }

    /// Turn a bus failure into a message, and name a structural one out loud.
    ///
    /// Two failures wear the same clothes on this seam and must not be
    /// conflated: "the user has no Composio connections" is an ordinary answer,
    /// while "this host exports no Composio interface" is a build mismatch that
    /// stops every synced source updating. The second is what
    /// `report_unserved_once` exists for.
    fn classify(&self, member: &'static str, error: &tinybus::Error) -> String {
        if is_unserved(error) {
            self.host_serves.store(false, Ordering::SeqCst);
            report_unserved_once(&COMPOSIO_REPORTED, COMPOSIO_UNSERVED, "composio_host");
            return format!("{COMPOSIO_UNSERVED} (calling {member}: {error})");
        }
        format!("composio host call {member} failed: {error}")
    }

    /// Ask the host one argument-free question from a synchronous caller.
    ///
    /// `Some` is the host's answer; `None` means it could not be asked, which
    /// each probe below turns into its own fallback.
    ///
    /// # Why an OS thread and not `block_in_place`
    ///
    /// `ComposioHost::api_key` and `ComposioHost::is_available` are synchronous
    /// on the engine's trait — they are consulted from inside
    /// `composio_config` and `ProviderContext::from_config`, neither of which
    /// can `await` — and the host serves both as ordinary async bus members. So
    /// something has to bridge, and the two candidates behave differently under
    /// the runtime flavours this code can find itself on.
    /// `tokio::task::block_in_place` panics outright on a current-thread
    /// runtime, and this module cannot prove its caller's flavour: the shipped
    /// module declares eight worker threads, but nothing stops an in-process
    /// harness from driving this code on a current-thread runtime, and a probe
    /// that aborted the process would be a far worse failure than the one it
    /// was asked about. A fresh thread with a `Handle` works identically under
    /// both, and costs one spawn on a path that is about to make a network call
    /// anyway.
    ///
    /// It does occupy the calling thread until the host answers. That is
    /// bounded by the bus's own call deadline and happens at most twice per
    /// sync tick, against a runtime sized at eight workers — but it is the
    /// reason these two are probes and not a general-purpose synchronous call
    /// helper, and why nothing else in this file uses this path.
    fn probe<R>(&self, member: &'static str) -> Option<R>
    where
        R: serde::de::DeserializeOwned + Send + 'static,
    {
        // Already proven unserved: answer locally rather than spawn a thread to
        // rediscover it. The report has fired; a second one would be noise.
        if !self.host_serves.load(Ordering::SeqCst) {
            return None;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            report_unserved_once(&COMPOSIO_REPORTED, COMPOSIO_NO_RUNTIME, "composio_host");
            return None;
        };
        let connection = self.connection.clone();
        let joined = std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    handle.block_on(async move {
                        let proxy = connection.proxy(
                            COMPOSIO_HOST_BUS_NAME,
                            COMPOSIO_HOST_OBJECT_PATH,
                            COMPOSIO_HOST_INTERFACE,
                        )?;
                        proxy.call::<R>(member, ()).await
                    })
                })
                .join()
        });
        match joined {
            Ok(Ok(answer)) => Some(answer),
            Ok(Err(error)) => {
                // Classified *before* the log call, not inside it. `classify`
                // latches the seam and fires the once-per-process report, and
                // `log::debug!` does not evaluate its arguments when debug
                // logging is off — which is every shipped build. Folding the
                // two together would make the report depend on the log level.
                let named = self.classify(member, &error);
                log::debug!("[tinymemory:module] {named}");
                None
            }
            // A panic inside the bridge thread. Nothing here can panic today,
            // but a probe that returned a plausible answer after one would be
            // worse than one that says it could not tell.
            Err(_) => {
                log::error!(
                    "[tinymemory:module] composio host probe {member} panicked; \
                     answering as unreachable"
                );
                None
            }
        }
    }
}

/// Whether `error` means "this host exports no such interface".
///
/// Matched on [`tinybus::Error::wire_name`] rather than on the enum, because a
/// remote failure is reconstructed as `MethodFailed { name, message }` on this
/// side — the structured variants exist on the *raising* side only, and
/// matching them here would silently never fire.
///
/// The four names are the whole "nobody is listening" family: no peer owns the
/// name, the peer exports no such object, the object has no such interface, and
/// the interface has no such member. The last one is what an older host with a
/// newer module actually produces.
fn is_unserved(error: &tinybus::Error) -> bool {
    matches!(
        error.wire_name(),
        "ai.tinyhumans.tinybus.Error.NameHasNoOwner"
            | "ai.tinyhumans.tinybus.Error.UnknownObject"
            | "ai.tinyhumans.tinybus.Error.UnknownInterface"
            | "ai.tinyhumans.tinybus.Error.UnknownMethod"
    )
}

#[async_trait]
impl ComposioHost for BusComposioHost {
    /// The user's connections, as the host sees them right now.
    ///
    /// The `config` argument is dropped rather than forwarded. In module mode
    /// it is the *engine's* config, built from the `ModuleConfig` this module
    /// was loaded with, and it is not the host's — sending it would ask the
    /// host to resolve a Composio client against a config it did not write.
    /// `ChatHost` makes the same call for the same reason: `Complete` carries a
    /// role and a request and nothing else.
    async fn list_connections(
        &self,
        _config: &tinymemory_core::Config,
    ) -> Result<Vec<ComposioConnection>, String> {
        self.call(LIST_CONNECTIONS_METHOD, ()).await
    }

    /// Run `tool`, host-side, against `connection_id`.
    ///
    /// A provider that answers `successful: false` is **not** an error — that
    /// rides back in the [`ComposioExecuteResponse`], because the sync layer
    /// bills a completed round trip either way.
    async fn execute(
        &self,
        _config: &tinymemory_core::Config,
        tool: &str,
        arguments: Option<serde_json::Value>,
        entity_id: &str,
        connection_id: Option<&str>,
    ) -> Result<ComposioExecuteResponse, String> {
        log::debug!("[tinymemory:module] composio execute tool={tool}");
        self.call(
            EXECUTE_METHOD,
            (
                tool.to_string(),
                arguments,
                entity_id.to_string(),
                connection_id.map(str::to_string),
            ),
        )
        .await
    }

    /// The direct-mode key, or `None` when direct mode is unset *or* the host
    /// could not be asked.
    ///
    /// The two are not distinguishable through this signature, and that is
    /// tolerable here only because the caller turns both into the same named
    /// failure: `composio_config` reports "Composio direct API key is not
    /// configured" and refuses to build a client. An unreachable host is
    /// additionally reported once through the error reporter by
    /// `probe`, so the log distinguishes what the return value cannot.
    fn api_key(&self, _config: &tinymemory_core::Config) -> Option<String> {
        self.probe::<Option<String>>(API_KEY_METHOD).flatten()
    }

    /// The proxied-mode bearer, fetched per call for the reason on
    /// [`SESSION_BEARER_METHOD`].
    ///
    /// An unreachable host flattens to `None`, which the caller turns into a
    /// named refusal rather than silence — the opposite of `is_available`'s
    /// optimistic answer below, and deliberately so: a bearer this process
    /// cannot obtain is not a credential it may guess at.
    fn session_bearer(&self, _config: &tinymemory_core::Config) -> Option<String> {
        self.probe::<Option<String>>(SESSION_BEARER_METHOD)
            .flatten()
    }

    /// Whether the sync layer should treat the user as signed in.
    ///
    /// # An unreachable host answers *yes*, deliberately
    ///
    /// This probe has no error channel, so an unreachable host has to be
    /// reported as one of the two real answers, and the two are not
    /// symmetrical. A wrong `false` makes `ProviderContext::from_config` return
    /// `None`, which the sync layer logs at debug and treats as "the user is
    /// not signed in" — the run reports nothing to do and looks healthy while
    /// no memory is being synced at all. A wrong `true` costs one more call,
    /// which reaches `execute` and fails with a named cause that says
    /// the Composio host is unserved.
    ///
    /// One of those is discoverable from a log and the other is not, so this
    /// answers `true` whenever it could not ask — including when the host is
    /// already known to serve no Composio interface, where the goal is
    /// precisely to let the next call fail loudly. Only a host that actually
    /// answered `false` reads as "not signed in".
    fn is_available(&self, _config: &tinymemory_core::Config) -> bool {
        self.probe::<bool>(IS_AVAILABLE_METHOD).unwrap_or(true)
    }
}

#[cfg(test)]
#[path = "composio_test.rs"]
mod test;

//! Mem0 REST adapter — self-hosted server and hosted platform.
//!
//! The two are different APIs behind one product name, and this adapter speaks
//! both. What differs is the credential header, the path shapes, and how a
//! listing is scoped; what does not differ is the record model, so `decode`
//! and `metadata` are shared verbatim.

use anyhow::Context;
use async_trait::async_trait;
use reqwest::Method;
use serde_json::{json, Value};
use tinymemory_api::recall::RecallOpts;
use tinymemory_api::traits::Memory;
use tinymemory_api::types::MemoryTaint;

use crate::common::{category, Dialect, HttpClient, RemoteMemory, StoredEntry};

/// Stable driver id used by configuration and status output.
pub use tinymemory_api::drivers::MEM0_DRIVER_ID;

/// Base URL of Mem0's hosted platform.
pub const MEM0_API_ENDPOINT: &str = "https://api.mem0.ai";

/// The Mem0 API this client speaks.
///
/// Selected by the constructor rather than sniffed: the two APIs answer the
/// same 401 to an unauthenticated probe, so a client that guessed would only
/// discover it guessed wrong after a credential was accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flavour {
    /// The open-source server: `X-API-Key`, un-prefixed REST paths.
    SelfHosted,
    /// The hosted platform at api.mem0.ai: `Authorization: Token`, v3 paths
    /// for add/search/list and v1 for the by-id operations. That version mix
    /// is the platform's own, not an oversight here.
    Cloud,
}

/// The `agent_id` every record this adapter writes to the hosted platform
/// carries.
///
/// The platform refuses a listing that names no entity id, so an adapter that
/// only ever set `user_id` could not enumerate across namespaces — and
/// `namespace_summaries`, `count`, and every exact-key lookup need exactly
/// that. Stamping one constant agent id makes "everything this adapter owns"
/// expressible as a filter, and keeps the adapter's records distinguishable
/// from anything else in the same Mem0 project.
const CLOUD_AGENT_ID: &str = "tinymemory";

/// Records requested per hosted-platform listing page.
const CLOUD_PAGE_SIZE: u32 = 200;

/// The most pages one hosted-platform listing will walk.
///
/// The walk already stops on an empty page and on a null `next`, which covers
/// a well-behaved server. It does not cover a server that keeps answering a
/// full page and a non-null cursor: that spins the loop and grows the buffer
/// until the process dies. 500 pages is 100_000 records -- far past any real
/// account this adapter writes, and small enough that the failure arrives as a
/// message rather than an OOM.
const CLOUD_MAX_PAGES: u32 = 500;

/// A Mem0 service — self-hosted or hosted — exposed through TinyMemory's
/// storage contract.
#[derive(Debug)]
pub struct Mem0Memory {
    inner: RemoteMemory<Mem0Dialect>,
}

impl Mem0Memory {
    /// Connect to a Mem0 REST server.
    ///
    /// `api_key` is sent as `X-API-Key`. Pass `None` only when the server is
    /// explicitly running with `AUTH_DISABLED=true` for local development.
    ///
    /// # Errors
    ///
    /// Returns an error when `endpoint` is not an HTTP(S) URL.
    pub fn new(endpoint: &str, api_key: Option<&str>) -> anyhow::Result<Self> {
        Self::self_hosted(endpoint, api_key)
    }

    /// Connect to a self-hosted Mem0 REST server (`X-API-Key`).
    ///
    /// # Errors
    ///
    /// Returns an error when `endpoint` is not an HTTP(S) URL.
    pub fn self_hosted(endpoint: &str, api_key: Option<&str>) -> anyhow::Result<Self> {
        Ok(Self {
            inner: RemoteMemory::new(Mem0Dialect {
                client: HttpClient::api_key(endpoint, api_key)?,
                flavour: Flavour::SelfHosted,
            }),
        })
    }

    /// Connect to Mem0's hosted platform at [`MEM0_API_ENDPOINT`].
    ///
    /// Authenticates with `Authorization: Token <api_key>` — the platform's
    /// scheme, and not interchangeable with a bearer token: a `Bearer`
    /// credential reaches the platform's JWT verifier instead and fails as
    /// `token_not_valid`, which reads as a broken token rather than a wrong
    /// header.
    ///
    /// # Errors
    ///
    /// Returns an error when `api_key` is blank.
    pub fn cloud(api_key: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(!api_key.trim().is_empty(), "mem0 API key must not be empty");
        Self::api(MEM0_API_ENDPOINT, api_key)
    }

    /// Connect to a Mem0 platform deployment at a custom base URL.
    ///
    /// # Errors
    ///
    /// Returns an error when `endpoint` is invalid or `api_key` is blank.
    pub fn api(endpoint: &str, api_key: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(!api_key.trim().is_empty(), "mem0 API key must not be empty");
        Ok(Self {
            inner: RemoteMemory::new(Mem0Dialect {
                client: HttpClient::token(endpoint, Some(api_key))?,
                flavour: Flavour::Cloud,
            }),
        })
    }
}

#[async_trait]
impl Memory for Mem0Memory {
    /// Returns the Mem0 driver identifier.
    fn name(&self) -> &str {
        self.inner.name()
    }
    /// Stores an internally sourced record through the shared contract.
    async fn store(
        &self,
        n: &str,
        k: &str,
        c: &str,
        cat: tinymemory_api::types::MemoryCategory,
        s: Option<&str>,
    ) -> anyhow::Result<()> {
        self.inner.store(n, k, c, cat, s).await
    }
    /// Stores a record while preserving its provenance taint.
    async fn store_with_taint(
        &self,
        n: &str,
        k: &str,
        c: &str,
        cat: tinymemory_api::types::MemoryCategory,
        s: Option<&str>,
        t: MemoryTaint,
    ) -> anyhow::Result<()> {
        self.inner.store_with_taint(n, k, c, cat, s, t).await
    }
    /// Runs native Mem0 semantic search and applies TinyMemory filters.
    async fn recall(
        &self,
        q: &str,
        l: usize,
        o: RecallOpts<'_>,
    ) -> anyhow::Result<Vec<tinymemory_api::types::MemoryEntry>> {
        self.inner.recall(q, l, o).await
    }
    /// Fetches one exact namespace/key record.
    async fn get(
        &self,
        n: &str,
        k: &str,
    ) -> anyhow::Result<Option<tinymemory_api::types::MemoryEntry>> {
        self.inner.get(n, k).await
    }
    /// Lists records matching the supplied TinyMemory filters.
    async fn list(
        &self,
        n: Option<&str>,
        c: Option<&tinymemory_api::types::MemoryCategory>,
        s: Option<&str>,
    ) -> anyhow::Result<Vec<tinymemory_api::types::MemoryEntry>> {
        self.inner.list(n, c, s).await
    }
    /// Deletes one exact namespace/key record.
    async fn forget(&self, n: &str, k: &str) -> anyhow::Result<bool> {
        self.inner.forget(n, k).await
    }
    /// Summarizes every namespace visible through this adapter.
    async fn namespace_summaries(
        &self,
    ) -> anyhow::Result<Vec<tinymemory_api::types::NamespaceSummary>> {
        self.inner.namespace_summaries().await
    }
    /// Counts every record visible through this adapter.
    async fn count(&self) -> anyhow::Result<usize> {
        self.inner.count().await
    }
    /// Checks whether the configured Mem0 service is reachable.
    async fn health_check(&self) -> bool {
        self.inner.health_check().await
    }
}

#[derive(Debug)]
/// Mem0-specific REST operations and wire-format conversion.
struct Mem0Dialect {
    client: HttpClient,
    flavour: Flavour,
}

impl Mem0Dialect {
    /// Largest listing this adapter will request in one call.
    ///
    /// Every exact-CRUD path here enumerates through [`Self::values`], so this
    /// is the ceiling on the whole store, not on one page.
    const LISTING_TOP_K: usize = 1000;

    /// Fetches Mem0's administrative memory listing.
    ///
    /// # A hard ceiling, deliberately loud
    ///
    /// This is a single unpaginated request, and it is the ONLY enumeration
    /// path in this adapter -- `get`, `list`, `count` and `export_page` all
    /// route through it. Past the ceiling the results are not merely
    /// incomplete, they are silently WRONG: `get(ns, key)` for a record beyond
    /// the cut-off returns `Ok(None)`, which the contract defines as "no such
    /// entry", so a caller reads "deleted" where the truth is "present but
    /// past the window".
    ///
    /// Returning an error instead is the honest failure. A full response is
    /// indistinguishable from a truncated one -- both are exactly `top_k`
    /// items -- so this cannot detect truncation, only its own boundary, and
    /// it refuses at that boundary rather than answering wrongly. Paginating
    /// properly needs Mem0's paging parameters verified against a live
    /// service; guessing them here would trade a loud failure for a quiet one.
    async fn values(&self) -> anyhow::Result<Vec<Value>> {
        match self.flavour {
            // Self-hosted: main's unpaginated listing with its truncation
            // guard, unchanged. The guard is why the cloud arm below had to
            // paginate rather than inherit this shape.
            Flavour::SelfHosted => {
                let top_k = Self::LISTING_TOP_K;
                let response: Value = self
                    .client
                    .json(Method::GET, &format!("memories?top_k={top_k}"), None)
                    .await?;
                let results = response
                    .get("results")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if results.len() >= top_k {
                    anyhow::bail!(
                        "mem0 returned {} memories, this adapter's unpaginated listing ceiling. \
                     Exact reads (get/list/count/export) cannot be answered correctly beyond \
                     it -- a record past the window would read as absent -- so the adapter \
                     refuses rather than answering wrongly. Recall is unaffected (it queries \
                     mem0's search API directly).",
                        results.len()
                    );
                }
                Ok(results)
            }
            // The hosted platform pages properly: it lists by POST with a
            // mandatory entity filter and answers
            // `{count, next, previous, results}`. That is the paging this
            // adapter's self-hosted arm documents as unverified — here it is
            // verified against Mem0's API reference, so this arm has no
            // ceiling to refuse at for a *correct* server. Paging stops on an
            // empty page as well as a null `next`, so a server that omits the
            // cursor cannot spin the loop -- but one that keeps answering a
            // full page and a non-null `next` still can, so the walk is
            // bounded below and fails loudly at the bound rather than
            // collecting for ever.
            Flavour::Cloud => {
                let mut all = Vec::new();
                let mut page = 1_u32;
                loop {
                    let response: Value = self
                        .client
                        .json(
                            Method::POST,
                            &format!("v3/memories/?page={page}&page_size={CLOUD_PAGE_SIZE}"),
                            Some(&json!({"filters": {"agent_id": CLOUD_AGENT_ID}})),
                        )
                        .await?;
                    let results = response
                        .get("results")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let exhausted =
                        results.is_empty() || response.get("next").is_none_or(Value::is_null);
                    all.extend(results);
                    if exhausted {
                        break;
                    }
                    anyhow::ensure!(
                        page < CLOUD_MAX_PAGES,
                        "mem0's hosted platform still reported more memories after \
                         {CLOUD_MAX_PAGES} pages of {CLOUD_PAGE_SIZE}. A cursor that never \
                         clears is a server fault, not a large account, and continuing \
                         would neither terminate nor answer correctly."
                    );
                    page = page.saturating_add(1);
                }
                Ok(all)
            }
        }
    }

    /// The search body both flavours send.
    ///
    /// `threshold` is **omitted** rather than sent as null when the caller set
    /// no minimum score: the platform types it as a number in 0..=1 and
    /// rejects an explicit null with a 400, which is how a recall against
    /// mem0's hosted API failed while store and list succeeded. `top_k` is
    /// clamped to the documented 1..=1000 for the same reason — a limit
    /// outside it is a validation error, not a smaller result set.
    fn search_body(query: &str, limit: usize, filters: Value, min_score: Option<f64>) -> Value {
        let mut body = json!({
            "query": query,
            "filters": filters,
            "top_k": limit.clamp(1, 1000),
        });
        if let (Some(object), Some(threshold)) = (body.as_object_mut(), min_score) {
            object.insert("threshold".into(), json!(threshold));
        }
        body
    }

    /// The path addressing one record by its remote id.
    ///
    /// The platform serves the by-id operations under **v1** while add,
    /// search and list are v3. That mix is the platform's own; keeping it in
    /// one place stops it being re-derived (or "corrected") at each call site.
    fn by_id_path(&self, remote_id: &str) -> String {
        match self.flavour {
            Flavour::SelfHosted => format!("memories/{remote_id}"),
            Flavour::Cloud => format!("v1/memories/{remote_id}/"),
        }
    }

    /// Decodes a Mem0 result containing TinyMemory-owned metadata.
    fn decode(value: &Value) -> Option<StoredEntry> {
        let metadata = value.get("metadata")?.as_object()?;
        let namespace = metadata.get("tinymemory_namespace")?.as_str()?.to_owned();
        let key = metadata.get("tinymemory_key")?.as_str()?.to_owned();
        Some(StoredEntry {
            remote_id: value.get("id")?.as_str()?.to_owned(),
            namespace,
            key,
            content: value
                .get("memory")
                .or_else(|| value.get("data"))?
                .as_str()?
                .to_owned(),
            category: category(metadata.get("tinymemory_category").and_then(Value::as_str)),
            timestamp: value
                .get("updated_at")
                .or_else(|| value.get("created_at"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            session_id: metadata
                .get("tinymemory_session_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            score: value.get("score").and_then(Value::as_f64),
            taint: metadata
                .get("tinymemory_taint")
                .and_then(Value::as_str)
                .map(MemoryTaint::from_db_str)
                .unwrap_or_default(),
        })
    }

    /// Encodes TinyMemory identity, classification, session, and provenance.
    fn metadata(entry: &StoredEntry) -> Value {
        let mut value = json!({
            "tinymemory_namespace": entry.namespace,
            "tinymemory_key": entry.key,
            "tinymemory_category": entry.category.to_string(),
            "tinymemory_taint": entry.taint.as_db_str(),
        });
        if let (Some(object), Some(session_id)) = (value.as_object_mut(), &entry.session_id) {
            object.insert("tinymemory_session_id".into(), json!(session_id));
        }
        value
    }
}

#[async_trait]
impl Dialect for Mem0Dialect {
    /// Returns the stable Mem0 driver identifier.
    fn name(&self) -> &'static str {
        MEM0_DRIVER_ID
    }

    /// Replaces an existing exact record or creates it with inference disabled.
    async fn upsert(&self, entry: StoredEntry) -> anyhow::Result<()> {
        let existing = self
            .entries()
            .await?
            .into_iter()
            .find(|item| item.namespace == entry.namespace && item.key == entry.key);
        let metadata = Self::metadata(&entry);
        if let Some(existing) = existing {
            // Both APIs take the same update body; only the path differs.
            self.client
                .empty(
                    Method::PUT,
                    &self.by_id_path(&existing.remote_id),
                    Some(&json!({"text": entry.content, "metadata": metadata})),
                )
                .await?;
        } else {
            let mut body = json!({
                "messages": [{"role": "user", "content": entry.content}],
                "user_id": entry.namespace,
                "run_id": entry.session_id,
                "metadata": metadata,
                "infer": false
            });
            if self.flavour == Flavour::Cloud {
                // Makes this record enumerable — see `CLOUD_AGENT_ID`.
                if let Some(object) = body.as_object_mut() {
                    object.insert("agent_id".into(), json!(CLOUD_AGENT_ID));
                }
            }
            let path = match self.flavour {
                Flavour::SelfHosted => "memories",
                Flavour::Cloud => "v3/memories/add/",
            };
            self.client.empty(Method::POST, path, Some(&body)).await?;
        }
        Ok(())
    }

    /// Enumerates and decodes TinyMemory-owned Mem0 records.
    async fn entries(&self) -> anyhow::Result<Vec<StoredEntry>> {
        Ok(self
            .values()
            .await?
            .iter()
            .filter_map(Self::decode)
            .collect())
    }

    /// Executes Mem0's native vector search.
    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: RecallOpts<'_>,
    ) -> anyhow::Result<Vec<StoredEntry>> {
        let response: Value = match self.flavour {
            Flavour::SelfHosted => {
                let mut filters = serde_json::Map::new();
                if let Some(namespace) = opts.namespace {
                    filters.insert("user_id".into(), json!(namespace));
                }
                self.client
                    .json(
                        Method::POST,
                        "search",
                        Some(&Self::search_body(
                            query,
                            limit,
                            Value::Object(filters),
                            opts.min_score,
                        )),
                    )
                    .await?
            }
            // The platform requires entity ids inside `filters` and supports
            // AND/OR; scoping to this adapter's agent id keeps a search from
            // returning records written by anything else in the project.
            Flavour::Cloud => {
                let filters = match opts.namespace {
                    Some(namespace) => json!({"AND": [
                        {"agent_id": CLOUD_AGENT_ID},
                        {"user_id": namespace}
                    ]}),
                    None => json!({"agent_id": CLOUD_AGENT_ID}),
                };
                self.client
                    .json(
                        Method::POST,
                        "v3/memories/search/",
                        Some(&Self::search_body(query, limit, filters, opts.min_score)),
                    )
                    .await?
            }
        };
        let values = response
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(values.iter().filter_map(Self::decode).collect())
    }

    /// Finds and deletes an exact TinyMemory logical record.
    async fn delete(&self, namespace: &str, key: &str) -> anyhow::Result<bool> {
        let Some(entry) = self
            .entries()
            .await?
            .into_iter()
            .find(|item| item.namespace == namespace && item.key == key)
        else {
            return Ok(false);
        };
        self.client
            .empty(Method::DELETE, &self.by_id_path(&entry.remote_id), None)
            .await
            .context("failed to delete Mem0 memory")?;
        Ok(true)
    }

    /// Accepts either Mem0's health endpoint or its redirected root page.
    async fn health(&self) -> bool {
        self.client.healthy("api/health").await || self.client.healthy("").await
    }
}

#[cfg(test)]
#[path = "mem0_test.rs"]
mod test;

#[cfg(test)]
mod search_body_tests {
    use super::*;

    /// A recall with no minimum score must omit `threshold`, not send null.
    /// The hosted platform types it as a number in 0..=1 and answers 400 to
    /// an explicit null — store and list succeeded while recall failed.
    #[test]
    fn an_unset_min_score_omits_the_threshold_field() {
        let body = Mem0Dialect::search_body("q", 10, json!({"user_id": "ns"}), None);
        assert!(
            body.get("threshold").is_none(),
            "threshold must be absent, not null: {body}"
        );
        assert_eq!(body["top_k"], 10);
        assert_eq!(body["query"], "q");
    }

    #[test]
    fn a_set_min_score_is_sent() {
        let body = Mem0Dialect::search_body("q", 10, json!({"user_id": "ns"}), Some(0.25));
        assert_eq!(body["threshold"], 0.25);
    }

    /// `top_k` outside the documented 1..=1000 is a validation error, so a
    /// caller's limit is clamped rather than forwarded into a 400.
    #[test]
    fn top_k_is_clamped_to_the_documented_range() {
        assert_eq!(
            Mem0Dialect::search_body("q", 0, json!({}), None)["top_k"],
            1
        );
        assert_eq!(
            Mem0Dialect::search_body("q", 5000, json!({}), None)["top_k"],
            1000
        );
    }
}

# Clawhip Ideas for cosmix-indexd Enhancement

**Source:** `clawhip/` — a daemon-first Discord notification router (~17,600 LOC Rust)
**Target:** `cosmix-indexd` — semantic indexing + vector storage daemon (1,025 LOC)
**Date:** 2026-04-03

---

## Executive Summary

clawhip contains four well-tested core utilities (circuit breaker, timer wheel, rate limiter, dead letter queue) plus architectural patterns (event batching, content deduplication, filesystem monitoring, multi-delivery routing) that map directly onto gaps in cosmix-indexd. This document extracts every transferable idea, prioritised by impact on indexd's evolution from a simple embed-and-search daemon into the knowledge backbone of the cosmix mesh.

---

## 1. Rate Limiter — Protect the Model from Thundering Herds

### The Problem in indexd

indexd holds a single `Arc<Mutex<AppState>>` over the entire model + database. Multiple concurrent clients (MCP bridge, skills CLI, future mesh peers) can all call `store` or `search` simultaneously, causing:

- Model queue contention (only one embed at a time anyway due to the mutex)
- Unbounded request acceptance with no backpressure
- No per-client fairness — one aggressive client starves others

### clawhip's Pattern

```rust
// clawhip/src/core/rate_limit.rs
pub struct TokenBucket {
    capacity: u32,
    tokens: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub fn consume_or_delay(&mut self, count: u32) -> Duration {
        self.refill();
        let needed = f64::from(count);
        if self.tokens >= needed {
            self.tokens -= needed;
            Duration::ZERO
        } else if self.refill_rate <= f64::EPSILON {
            Duration::from_secs(1)
        } else {
            let missing = needed - self.tokens;
            self.tokens = 0.0;
            Duration::from_secs_f64(missing / self.refill_rate)
        }
    }
}

pub struct RateLimiter {
    buckets: HashMap<String, TokenBucket>,
    capacity: u32,
    refill_per_sec: f64,
}

impl RateLimiter {
    pub fn delay_for(&mut self, key: &str) -> Duration {
        self.buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(self.capacity, self.refill_per_sec))
            .consume_or_delay(1)
    }
}
```

### How to Apply in indexd

```rust
// Per-source rate limiting in indexd
// Key by transport: "socket:{peer_cred_pid}", "amp:{service_name}", "mesh:{peer_id}"
struct IndexdState {
    model: Option<EmbedModel>,
    db: VectorDb,
    rate_limiter: RateLimiter,  // NEW
    // ...
}

async fn process_request(input: &str, state: &Arc<Mutex<AppState>>, source_key: &str) -> String {
    let delay = {
        let mut guard = state.lock().await;
        guard.rate_limiter.delay_for(source_key)
    };
    if delay > Duration::ZERO {
        tokio::time::sleep(delay).await;
    }
    // ... existing dispatch
}
```

**Recommended configuration:**
- Embed/Store operations: 10 tokens, 2/sec refill (burst of 10, then 2 embeds/sec sustained)
- Search operations: 30 tokens, 10/sec refill (search is cheaper — just one embed + SQLite query)
- Stats/List operations: no rate limiting needed (no model involvement)

**Per-source scoping** is critical. When indexd joins the mesh, a remote node shouldn't be able to starve local MCP requests. The `RateLimiter` with per-key buckets handles this naturally.

---

## 2. Circuit Breaker — Graceful Model Failure Handling

### The Problem in indexd

When the embedding model fails to load (OOM, corrupted weights, HuggingFace Hub down), indexd returns an error string and tries again on the next request. There's no backoff — if the model can't load, every single request triggers a full `EmbedModel::load()` attempt (downloading files, memory-mapping safetensors, initializing the model), wasting CPU and disk I/O.

### clawhip's Pattern

```rust
// clawhip/src/core/circuit_breaker.rs
pub struct CircuitBreaker {
    state: CircuitState,
    consecutive_failures: u32,
    failure_threshold: u32,
    cooldown: Duration,
}

impl CircuitBreaker {
    pub fn allow_request(&mut self) -> bool {
        match self.state {
            CircuitState::Closed | CircuitState::HalfOpen => true,
            CircuitState::Open { opened_at } => {
                if opened_at.elapsed() >= self.cooldown {
                    self.state = CircuitState::HalfOpen;
                    true  // Allow ONE probe request
                } else {
                    false
                }
            }
        }
    }

    pub fn record_failure(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.consecutive_failures += 1;
                if self.consecutive_failures >= self.failure_threshold {
                    self.state = CircuitState::Open { opened_at: Instant::now() };
                }
            }
            CircuitState::HalfOpen => {
                // Probe failed — back to Open
                self.state = CircuitState::Open { opened_at: Instant::now() };
            }
            CircuitState::Open { .. } => {}
        }
    }
}
```

### How to Apply in indexd

Two circuit breakers, each protecting a different failure domain:

**1. Model loading breaker** — prevents repeated load attempts when the model is broken:

```rust
struct AppState {
    model: Option<EmbedModel>,
    model_breaker: CircuitBreaker,  // NEW: threshold=2, cooldown=60s
    // ...
}

async fn ensure_model(state: &mut AppState) -> Result<()> {
    if state.model.is_some() {
        return Ok(());
    }
    if !state.model_breaker.allow_request() {
        anyhow::bail!("model loading suspended (circuit open, retrying in {:?})",
            state.model_breaker.cooldown_remaining());
    }
    match EmbedModel::load(state.dtype, &state.model_id) {
        Ok(model) => {
            state.model = Some(model);
            state.model_breaker.record_success();
            Ok(())
        }
        Err(e) => {
            state.model_breaker.record_failure();
            Err(e)
        }
    }
}
```

**2. Hub connection breaker** — prevents reconnect storms when noded is down:

```rust
// In AMP hub connection loop
let mut hub_breaker = CircuitBreaker::new(3, Duration::from_secs(30));
loop {
    if !hub_breaker.allow_request() {
        tokio::time::sleep(Duration::from_secs(5)).await;
        continue;
    }
    match HubClient::connect_default("indexd").await {
        Ok(client) => {
            hub_breaker.record_success();
            handle_amp_commands(client, state.clone(), tx.clone()).await;
            // Connection dropped — will retry
        }
        Err(_) => {
            hub_breaker.record_failure();
        }
    }
}
```

The circuit breaker is particularly valuable because indexd's `ensure_model()` currently does a blocking `EmbedModel::load()` inside the mutex — if model loading takes 30 seconds (downloading from HF Hub), ALL requests queue behind it. With a breaker, after 2 failures the circuit opens and requests get instant "model unavailable" errors instead of 30-second waits.

---

## 3. Timer Wheel — TTL, Scheduled Maintenance, Deferred Embedding

### The Problem in indexd

- No TTL on stored vectors — old/stale content accumulates forever
- No scheduled maintenance (WAL checkpointing, vacuum, stats reporting)
- No deferred embedding — when a batch of documents arrives, they're all embedded synchronously, blocking the mutex for the entire duration

### clawhip's Pattern

```rust
// clawhip/src/core/timer_wheel.rs — hierarchical 4-tier scheduling
pub struct TimerWheel {
    seconds: Vec<Vec<DelayedEntry>>,   // 60 slots
    minutes: Vec<Vec<DelayedEntry>>,   // 60 slots
    hours: Vec<Vec<DelayedEntry>>,     // 24 slots
    days: Vec<Vec<DelayedEntry>>,      // 365 slots
    current_ms: u64,
}

impl TimerWheel {
    pub fn schedule(&mut self, entry: DelayedEntry) {
        let delta = entry.deliver_at_ms.saturating_sub(self.current_ms);
        if delta < 60_000 {
            let slot = (entry.deliver_at_ms / 1_000) % 60;
            self.seconds[slot].push(entry);
        } else if delta < 3_600_000 {
            let slot = (entry.deliver_at_ms / 60_000) % 60;
            self.minutes[slot].push(entry);
        }
        // ... hours, days tiers cascade
    }

    pub fn tick(&mut self, now_ms: u64) -> Vec<DelayedEntry> {
        // Drains due entries, re-schedules not-yet-due from coarser tiers
    }
}
```

**Key design insight:** Entries in coarser tiers (minutes/hours/days) get re-scheduled into finer tiers as time passes. O(1) scheduling, efficient draining. No heap allocation per timer — just Vec slot placement.

### How to Apply in indexd

**A. TTL-based vector expiration:**

```rust
// Extended schema
// ALTER TABLE chunks ADD COLUMN expires_at TEXT;

// Timer wheel schedules cleanup sweeps
struct IndexdScheduler {
    wheel: TimerWheel,
}

impl IndexdScheduler {
    fn schedule_ttl_check(&mut self, now_ms: u64) {
        // Every 5 minutes, sweep expired entries
        self.wheel.schedule(DelayedEntry {
            deliver_at_ms: now_ms + 300_000,
            record: b"ttl_sweep".to_vec(),
        });
    }

    fn tick(&mut self, now_ms: u64) -> Vec<ScheduledTask> {
        self.wheel.tick(now_ms).into_iter()
            .filter_map(|e| match e.record.as_slice() {
                b"ttl_sweep" => {
                    self.schedule_ttl_check(now_ms); // re-schedule
                    Some(ScheduledTask::TtlSweep)
                }
                b"wal_checkpoint" => Some(ScheduledTask::WalCheckpoint),
                b"stats_report" => Some(ScheduledTask::StatsReport),
                _ => None,
            })
            .collect()
    }
}
```

**B. Deferred batch embedding** — accept store requests immediately, embed in background:

```rust
// Client sends store request → indexd returns immediately with pending IDs
// Timer wheel schedules the actual embedding 100ms later (coalescing window)
// Multiple rapid stores get batched into a single model.embed() call

struct PendingBatch {
    texts: Vec<String>,
    sources: Vec<String>,
    metadata: Vec<String>,
    callbacks: Vec<oneshot::Sender<Result<Vec<i64>>>>,
}

// On store request:
fn defer_store(&mut self, req: StoreRequest, callback: oneshot::Sender<Result<Vec<i64>>>) {
    self.pending_batch.texts.extend(req.texts);
    self.pending_batch.sources.extend(/* ... */);
    self.pending_batch.callbacks.push(callback);

    if self.pending_batch.texts.len() == 1 {
        // First item — schedule flush 100ms from now
        self.scheduler.wheel.schedule(DelayedEntry {
            deliver_at_ms: now_ms() + 100,
            record: b"flush_batch".to_vec(),
        });
    }
}
```

This transforms indexd from synchronous-per-request to batched, dramatically improving throughput when the MCP bridge or skills CLI stores multiple documents in quick succession (which it does — `skills_store` often fires right after `skills_refine`).

**C. Scheduled maintenance tasks:**

| Task | Interval | Purpose |
|------|----------|---------|
| TTL sweep | 5 min | Delete expired vectors |
| WAL checkpoint | 15 min | `PRAGMA wal_checkpoint(PASSIVE)` — keep WAL size bounded |
| Stats broadcast | 60 min | Emit stats over AMP for monitoring |
| Model idle check | per-config | Already exists, but timer wheel is cleaner than the current sleep loop |

---

## 4. Dead Letter Queue — Don't Lose Failed Operations

### The Problem in indexd

When a store or update operation fails (SQLite write error, embedding dimension mismatch, corrupt metadata JSON), the error is returned to the client and the data is lost. The client must retry, but there's no guarantee it will — especially for AMP mesh commands where the caller may have moved on.

### clawhip's Pattern

```rust
// clawhip/src/core/dlq.rs
pub struct DlqEntry {
    pub original_topic: String,
    pub retry_count: u32,
    pub last_error: String,
    pub target: String,
    pub event_kind: String,
    pub format: String,
    pub content: String,
    pub payload: Value,
}

pub struct Dlq {
    entries: Vec<DlqEntry>,
}
```

### How to Apply in indexd

A persistent DLQ for indexd needs to survive restarts. SQLite is already there — use it:

```rust
// New table in the existing database
// CREATE TABLE IF NOT EXISTS dlq (
//     id         INTEGER PRIMARY KEY AUTOINCREMENT,
//     action     TEXT NOT NULL,            -- 'store', 'update', 'delete'
//     payload    TEXT NOT NULL,            -- original JSON request
//     error      TEXT NOT NULL,
//     retries    INTEGER NOT NULL DEFAULT 0,
//     max_retries INTEGER NOT NULL DEFAULT 3,
//     next_retry TEXT NOT NULL,            -- datetime for next attempt
//     created    TEXT NOT NULL DEFAULT (datetime('now'))
// );

struct DlqManager {
    max_retries: u32,
}

impl DlqManager {
    fn enqueue(&self, db: &Connection, action: &str, payload: &str, error: &str) -> Result<()> {
        let backoff_secs = 30; // First retry in 30s
        db.execute(
            "INSERT INTO dlq (action, payload, error, next_retry) 
             VALUES (?1, ?2, ?3, datetime('now', '+' || ?4 || ' seconds'))",
            rusqlite::params![action, payload, error, backoff_secs],
        )?;
        Ok(())
    }

    fn drain_due(&self, db: &Connection) -> Result<Vec<DlqEntry>> {
        // SELECT and process entries where next_retry <= datetime('now')
        // On success: DELETE from dlq
        // On failure: UPDATE retries += 1, next_retry with exponential backoff
        // On max_retries exceeded: log and delete (or move to permanent_failures table)
    }
}
```

**Integration with timer wheel:** Schedule `dlq_drain` as a recurring task every 30 seconds. The timer wheel handles the scheduling; DlqManager handles the retry logic with exponential backoff.

**What goes in the DLQ:**
- Store operations that fail due to transient SQLite errors (SQLITE_BUSY, WAL contention)
- Update operations that fail because the model couldn't load (circuit breaker open) — retry when breaker closes
- AMP commands that fail due to response serialisation errors

**What does NOT go in the DLQ:**
- Invalid requests (bad JSON, unknown action) — these are client bugs, not transient failures
- Delete operations — idempotent, just retry inline

---

## 5. Event Batching — Coalesce Rapid-Fire Embed Requests

### The Problem in indexd

The MCP bridge's skill learning loop generates bursts of activity:
1. `skills_retrieve` → search (1 embed)
2. Task completes → `skills_store` → store (1 embed)
3. `skills_refine` → update (1 embed)

That's 3 separate model invocations within seconds. Each one locks the mutex, runs the tokenizer, does a forward pass, unlocks. The model forward pass has fixed overhead regardless of batch size (up to the tokenizer's max sequence length), so batching 3 single-text embeds into 1 three-text embed is nearly 3x faster.

### clawhip's Pattern

The `GitHubCiBatcher` in `dispatch.rs` demonstrates a clean batching architecture:

```rust
struct GitHubCiBatcher {
    pending: HashMap<String, PendingCiBatch>,
    timer_wheel: TimerWheel,
    window: Duration,  // Configurable batch window
}

impl GitHubCiBatcher {
    fn observe(&mut self, event: IncomingEvent, now_ms: u64) -> Vec<IncomingEvent> {
        // Add to pending batch
        // Schedule timer wheel entry for flush deadline
        // If batch is "complete" (all jobs terminal), flush immediately
        // Otherwise wait for window to expire
    }

    fn flush_due(&mut self, now_ms: u64) -> Vec<IncomingEvent> {
        // Timer wheel tick → find expired batches → flush them
    }
}
```

**Key design choices:**
- **Versioned batch keys** — each update increments version; stale timer entries are ignored
- **Early flush** — if the batch is "complete" before the window expires, flush immediately
- **Guaranteed delivery** — `flush_all()` on shutdown drains remaining batches

### How to Apply in indexd

```rust
struct EmbedBatcher {
    pending: Vec<PendingEmbed>,
    timer_wheel: TimerWheel,
    window_ms: u64,         // default 100ms — short enough to feel instant
    max_batch_size: usize,  // default 32 — model's practical batch limit
    flush_scheduled: bool,
}

struct PendingEmbed {
    text: String,
    prefix: String,
    callback: oneshot::Sender<Result<Vec<f32>>>,
}

impl EmbedBatcher {
    fn submit(&mut self, text: String, prefix: String) -> oneshot::Receiver<Result<Vec<f32>>> {
        let (tx, rx) = oneshot::channel();
        self.pending.push(PendingEmbed { text, prefix, callback: tx });

        // Flush immediately if batch is full
        if self.pending.len() >= self.max_batch_size {
            self.flush_now();
        } else if !self.flush_scheduled {
            // Schedule flush after window
            self.timer_wheel.schedule(DelayedEntry {
                deliver_at_ms: now_ms() + self.window_ms,
                record: b"embed_flush".to_vec(),
            });
            self.flush_scheduled = true;
        }
        rx
    }

    fn flush_now(&mut self) -> Vec<PendingEmbed> {
        self.flush_scheduled = false;
        std::mem::take(&mut self.pending)
    }
}
```

**Architecture change:** Instead of each handler calling `model.embed()` directly, they submit to the batcher and `await` the oneshot receiver. A dedicated flush task runs the actual model inference on the collected batch. This:

1. Batches multiple concurrent requests into a single model forward pass
2. Keeps the mutex hold time short (just queue insertion, not model inference)
3. Provides natural backpressure via the oneshot channel

---

## 6. Content Deduplication — Don't Store What You Already Have

### The Problem in indexd

Nothing prevents storing identical content multiple times. The skills loop can store the same skill document repeatedly if `skills_store` is called without first checking for duplicates. Over time, the vector database fills with near-duplicates that pollute search results.

### clawhip's Pattern

The keyword window dedup uses a two-level approach:

```rust
// Level 1: Exact dedup via HashSet
let key = (keyword.clone(), line.to_string());
if seen.insert(key) {
    hits.push(hit);
}

// Level 2: Content-aware overlap detection
fn overlapping_suffix_prefix_len(previous: &[&str], current: &[&str]) -> usize {
    let max_overlap = previous.len().min(current.len());
    for overlap in (0..=max_overlap).rev() {
        if previous[prev.len().saturating_sub(overlap)..] == current[..overlap] {
            return overlap;
        }
    }
    0
}
```

### How to Apply in indexd

**Two-tier deduplication for vector storage:**

**Tier 1 — Content hash dedup (exact match, O(1)):**

```rust
// Add to chunks table:
// ALTER TABLE chunks ADD COLUMN content_hash BLOB;
// CREATE UNIQUE INDEX IF NOT EXISTS idx_chunks_hash ON chunks(content_hash);

use blake3;

fn content_hash(text: &str, source: &str) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(source.as_bytes());
    hasher.update(b":");
    hasher.update(text.as_bytes());
    hasher.finalize().as_bytes()[..16].to_vec()  // 128-bit prefix is sufficient
}

fn store_with_dedup(&self, text: &str, source: &str, metadata: &str, embedding: &[f32]) -> Result<StoreResult> {
    let hash = content_hash(text, source);

    // Try insert — unique index will reject exact duplicates
    match self.conn.execute(
        "INSERT OR IGNORE INTO chunks (content, source, metadata, content_hash) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![text, source, metadata, hash],
    ) {
        Ok(0) => {
            // Duplicate — find existing and optionally update metadata
            let existing_id: i64 = self.conn.query_row(
                "SELECT id FROM chunks WHERE content_hash = ?1",
                [&hash], |r| r.get(0),
            )?;
            Ok(StoreResult::Duplicate(existing_id))
        }
        Ok(_) => {
            let rowid = self.conn.last_insert_rowid();
            // Store embedding...
            Ok(StoreResult::Stored(rowid))
        }
    }
}
```

**Tier 2 — Semantic dedup (near-duplicate detection):**

```rust
// Before storing, search for semantically similar content
fn store_with_semantic_dedup(
    &self, text: &str, embedding: &[f32], similarity_threshold: f64
) -> Result<StoreResult> {
    // Search for nearest neighbor
    let results = self.search(embedding, 1, "", &[])?;
    if let Some(nearest) = results.first() {
        if nearest.distance < similarity_threshold {  // e.g. 0.05 for very similar
            return Ok(StoreResult::NearDuplicate {
                existing_id: nearest.id,
                distance: nearest.distance,
            });
        }
    }
    // No near-duplicate — proceed with store
    // ...
}
```

**Return to client:** Let the client decide what to do with near-duplicates. The skills system can choose to update the existing skill (merge approaches) rather than creating a duplicate.

---

## 7. Filesystem Monitoring — Auto-Index Changed Documents

### The Problem in indexd

indexd is passive — it only indexes content when explicitly told to via store requests. Documents in `_doc/` and `_journal/` must be manually indexed. When a journal entry changes, the old embedding becomes stale but nobody re-indexes it.

### clawhip's Pattern

`source/workspace.rs` demonstrates robust filesystem monitoring:

- **inotify** on Linux for real-time change detection
- **Polling fallback** with `FileSignature { modified_ms, len }` for cheap change detection
- **Debouncing** via `PendingChange { path, due_at }` to coalesce rapid saves
- **Snapshot diffing** to extract semantic changes (not just "file changed")

### How to Apply in indexd

A new `indexd.watch` action or config section for auto-indexing directories:

```toml
# In cosmix settings.toml [embed] section
[[embed.watch]]
path = "~/.cosmix/src/_doc"
source = "doc"
glob = "*.md"
debounce_secs = 5

[[embed.watch]]
path = "~/.cosmix/src/_journal"
source = "journal"
glob = "*.md"
debounce_secs = 10
```

```rust
use std::collections::HashMap;
use std::time::Instant;

struct FileWatcher {
    watches: Vec<WatchConfig>,
    signatures: HashMap<PathBuf, FileSignature>,
    pending: HashMap<PathBuf, Instant>,  // path → earliest-allowed-process time
    debounce: Duration,
}

struct FileSignature {
    modified_ms: u64,
    len: u64,
}

impl FileWatcher {
    async fn poll_changes(&mut self) -> Vec<FileChange> {
        let mut changes = Vec::new();
        let now = Instant::now();

        for watch in &self.watches {
            for entry in glob::glob(&format!("{}/{}", watch.path, watch.glob))
                .into_iter().flatten().flatten()
            {
                let meta = match std::fs::metadata(&entry) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let sig = FileSignature {
                    modified_ms: meta.modified().ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                    len: meta.len(),
                };

                let changed = self.signatures.get(&entry)
                    .map(|old| old.modified_ms != sig.modified_ms || old.len != sig.len)
                    .unwrap_or(true);  // New file = changed

                if changed {
                    self.signatures.insert(entry.clone(), sig);

                    // Debounce: only process if enough time has passed
                    let due = self.pending.entry(entry.clone())
                        .or_insert(now + self.debounce);
                    if now >= *due {
                        self.pending.remove(&entry);
                        changes.push(FileChange {
                            path: entry,
                            source: watch.source.clone(),
                        });
                    }
                }
            }
        }
        changes
    }
}
```

**On change detected:**
1. Read the markdown file
2. Chunk it (by heading sections or fixed-size with overlap)
3. Delete old vectors for that file path (metadata filter: `filepath = "..."`)
4. Store new chunks with file path in metadata

This makes indexd a **live knowledge base** that stays in sync with the project's documentation, rather than a passive store that requires manual indexing.

---

## 8. Multi-Delivery Routing — Search Results to Multiple Consumers

### clawhip's Pattern

The router matches events against 0..N routes — no early exit, one event can trigger multiple deliveries:

```rust
// Simplified from clawhip/src/router.rs
fn resolve(&self, event: &IncomingEvent) -> Vec<ResolvedDelivery> {
    let mut deliveries = Vec::new();
    for route in &self.routes {
        if route_matches(route, event) {
            deliveries.push(ResolvedDelivery {
                sink: route.sink.clone(),
                target: route.channel_or_webhook(),
                format: route.format.unwrap_or_default(),
            });
        }
    }
    if deliveries.is_empty() {
        deliveries.push(self.default_delivery());
    }
    deliveries
}
```

### How to Apply in indexd

When indexd participates in the AMP mesh, search results should be routable:

```rust
// New action: "subscribe" — register interest in content matching a pattern
// When new content is stored that matches, push to subscriber

struct Subscription {
    id: String,
    query_embedding: Vec<f32>,      // What to match against
    similarity_threshold: f64,       // How close is "relevant"
    sink: SubscriptionSink,          // Where to deliver
}

enum SubscriptionSink {
    AmpService(String),              // Push via AMP to named service
    Callback(String),                // Unix socket path to notify
}

// On store:
fn notify_subscribers(&self, new_embedding: &[f32], stored_id: i64, content: &str) {
    for sub in &self.subscriptions {
        let distance = cosine_distance(new_embedding, &sub.query_embedding);
        if distance < sub.similarity_threshold {
            // Push notification: "new relevant content stored"
            self.deliver_notification(&sub.sink, stored_id, content, distance);
        }
    }
}
```

**Use case:** The MCP bridge subscribes to "rust error patterns" with threshold 0.1. When someone stores a new skill about Rust compilation errors, the MCP bridge gets notified and can proactively suggest it in the next relevant conversation.

---

## 9. Structured Error Codes — Not Just Strings

### The Problem in indexd

Every error is a plain string: `{"error": "model load failed: ..."}`. The AMP bridge checks `response.contains("\"error\"")` to set RC=10. No way to distinguish transient vs permanent errors, or to know whether retrying makes sense.

### How to Apply

Adopt AMP's RC code semantics in the error response:

```rust
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    code: u32,          // AMP-aligned: 0=ok, 5=warning, 10=error, 20=failure
    retryable: bool,    // Should the client retry?
    retry_after_secs: Option<u64>,  // Suggested backoff
}

fn json_error(msg: &str, code: u32, retryable: bool) -> String {
    serde_json::to_string(&ErrorResponse {
        error: msg.to_string(),
        code,
        retryable,
        retry_after_secs: if retryable { Some(5) } else { None },
    }).unwrap()
}

// Usage:
// Model loading suspended (circuit breaker open):
json_error("model unavailable, circuit open", 10, true)

// Invalid request (bad JSON):
json_error("invalid request: ...", 20, false)

// Rate limited:
json_error("rate limited", 5, true)  // warning, retry after delay

// Transient SQLite error:
json_error("database busy", 10, true)
```

The DLQ logic then uses `retryable` to decide whether to enqueue a failed operation.

---

## 10. Architectural Synthesis — The Enhanced indexd

### Current Architecture

```
Client → Unix Socket → Mutex<AppState> → Model + DB → Response
         AMP Hub ────↗
```

**Problems:** Single mutex, no batching, no resilience, no proactive indexing.

### Proposed Architecture (incorporating clawhip patterns)

```
                    ┌──────────────────────────────────────┐
                    │           cosmix-indexd               │
                    │                                      │
  Unix Socket ──→  │  ┌─────────┐    ┌──────────────┐    │
  AMP Hub ──────→  │  │  Rate   │    │   Embed      │    │
                    │  │ Limiter │──→ │  Batcher     │    │
                    │  │(per-key)│    │(100ms window)│    │
                    │  └─────────┘    └──────┬───────┘    │
                    │                        │            │
                    │         ┌──────────────┤            │
                    │         │              │            │
                    │  ┌──────▼──────┐  ┌───▼─────────┐  │
                    │  │  Circuit    │  │  VectorDb    │  │
                    │  │  Breaker    │  │  + DLQ       │  │
                    │  │  (model)    │  │  + Content   │  │
                    │  └──────┬──────┘  │    Hash Dedup│  │
                    │         │         └───┬─────────┘  │
                    │  ┌──────▼──────┐      │            │
                    │  │  EmbedModel │      │            │
                    │  │  (lazy load)│      │            │
                    │  └─────────────┘      │            │
                    │                       │            │
                    │  ┌────────────────────▼──────────┐  │
                    │  │      Timer Wheel Scheduler    │  │
                    │  │  - TTL sweep (5min)           │  │
                    │  │  - DLQ drain (30s)            │  │
                    │  │  - WAL checkpoint (15min)     │  │
                    │  │  - File watcher poll (10s)    │  │
                    │  │  - Stats broadcast (60min)    │  │
                    │  └──────────────────────────────┘  │
                    │                                      │
                    │  ┌──────────────────────────────┐    │
                    │  │    Subscription Router        │  │
                    │  │  (notify on relevant stores)  │  │
                    │  └──────────────────────────────┘  │
                    └──────────────────────────────────────┘
```

### Implementation Priority

| Priority | Feature | Complexity | Impact | clawhip Source |
|----------|---------|-----------|--------|---------------|
| **P0** | Circuit breaker on model load | Low (70 LOC) | High — prevents load storms | `core/circuit_breaker.rs` |
| **P0** | Content hash dedup | Low (30 LOC) | High — prevents skill duplication | `keyword_window.rs` concept |
| **P1** | Rate limiter (per-source) | Low (95 LOC) | Medium — mesh readiness | `core/rate_limit.rs` |
| **P1** | Embed batching | Medium (150 LOC) | High — 2-3x throughput | `dispatch.rs` batcher pattern |
| **P1** | Structured error codes | Low (20 LOC) | Medium — client reliability | Original design |
| **P2** | Timer wheel scheduler | Medium (120 LOC) | Medium — maintenance automation | `core/timer_wheel.rs` |
| **P2** | DLQ (persistent) | Medium (100 LOC) | Medium — data durability | `core/dlq.rs` |
| **P2** | File watcher auto-indexing | High (300 LOC) | High — live knowledge base | `source/workspace.rs` |
| **P3** | Subscription router | High (200 LOC) | Medium — mesh integration | `router.rs` multi-delivery |
| **P3** | Semantic near-dedup | Low (30 LOC) | Low — search quality | `keyword_window.rs` concept |

### New Dependencies Required

None. All patterns use only `std` types (`HashMap`, `Vec`, `Instant`, `Duration`) plus what indexd already depends on (`tokio`, `serde`, `rusqlite`). The timer wheel and circuit breaker are pure data structures with zero external dependencies — they can live in `cosmix-lib-amp` or a new `cosmix-lib-core` crate for reuse across daemons.

### Where to Put the Shared Utilities

These patterns are useful beyond indexd. Recommended placement:

```
cosmix-lib-core/           # NEW crate — zero-dep resilience primitives
├── src/
│   ├── lib.rs
│   ├── circuit_breaker.rs  # Copy from clawhip, add async-aware variant
│   ├── timer_wheel.rs      # Copy from clawhip, add typed task enum
│   ├── rate_limit.rs       # Copy from clawhip, add async delay
│   └── dlq.rs              # Adapt for SQLite persistence
```

Then `cosmix-indexd`, `cosmix-noded`, `cosmix-maild`, and `cosmix-webd` all depend on `cosmix-lib-core` for shared resilience patterns. This avoids duplicating circuit breaker logic across every daemon that connects to the hub.

---

## Appendix A: Complete clawhip Core Source (Portable)

The four core files total 306 lines and have zero external dependencies beyond `std` and `serde` (DLQ only). They can be copied verbatim as a starting point:

| File | Lines | Dependencies |
|------|-------|-------------|
| `circuit_breaker.rs` | 111 | `std::time` |
| `timer_wheel.rs` | 144 | none |
| `rate_limit.rs` | 94 | `std::collections`, `std::time` |
| `dlq.rs` | 54 | `serde`, `serde_json` |

## Appendix B: Patterns NOT Worth Porting

| clawhip Pattern | Why Skip |
|----------------|----------|
| Discord/Slack sinks | cosmix uses AMP, not webhooks |
| TOML route config | indexd config is simpler; routes via AMP subscription |
| Dynamic token interpolation | Mix handles templating natively |
| tmux session monitoring | Not relevant to indexd |
| GitHub API polling | cosmix doesn't poll external APIs |
| Glob-based event matching | AMP command dispatch is exact-match, not glob |
| Aggregated rendering | indexd returns structured JSON, not human-readable text |

# Claw Code Parity — Ideas for cosmix-indexd and the Cosmix System

**Source:** `claw-code-parity/rust/` — a Rust rewrite of the Claude Code agent harness (~47,800 LOC)
**Target:** `cosmix-indexd` (primary), broader cosmix daemon ecosystem (secondary)
**Date:** 2026-04-03

---

## Executive Summary

claw-code-parity is a 9-crate Rust workspace implementing a full Claude Code runtime: API streaming, session management, tool execution, permissions, hooks, config hierarchy, prompt caching, and auto-compaction. It's a rich source of production-grade patterns that map onto cosmix-indexd's evolution and the broader daemon ecosystem. This document extracts every transferable idea, with exact code examples and integration paths.

---

## 1. Embedding Cache with Request Fingerprinting

### The Problem in indexd

Every search and store operation calls `model.embed()`, even when the same text was embedded seconds ago. The MCP skills loop calls `skills_retrieve` at the start of every non-trivial task — often with similar or identical task descriptions. Each call loads the model (if idle-unloaded), tokenizes, runs a forward pass, and returns. No caching.

### claw-code's Pattern

`api/src/prompt_cache.rs` implements FNV-1a request fingerprinting with TTL-based validation:

```rust
// FNV-1a hash for fast, stable fingerprinting
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn request_hash_hex(request: &MessageRequest) -> String {
    // Hash the serialized request to create a stable fingerprint
    let serialized = serde_json::to_string(request).unwrap_or_default();
    format!("{}{:016x}", REQUEST_FINGERPRINT_PREFIX, fnv1a_hash(serialized.as_bytes()))
}
```

The cache stores responses keyed by request hash, with TTL-based expiry and cache break detection:

```rust
pub struct PromptCache {
    inner: Arc<Mutex<PromptCacheInner>>,
}

struct PromptCacheInner {
    config: PromptCacheConfig,
    paths: PromptCachePaths,       // File-backed persistence
    stats: PromptCacheStats,       // Hit/miss/break counters
    previous: Option<TrackedPromptState>,  // Last request state for break detection
}

// TTL-based lookup
pub fn lookup_completion(&self, request: &MessageRequest) -> Option<MessageResponse> {
    let request_hash = request_hash_hex(request);
    let entry = read_json::<CompletionCacheEntry>(&entry_path);
    // Check fingerprint version, TTL expiry
    // Update stats (hits/misses)
    // Return cached response or None
}
```

### How to Apply in indexd

**Embedding cache** — cache the embedding vector for text+prefix combinations:

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

struct EmbeddingCache {
    entries: HashMap<u64, CachedEmbedding>,
    ttl: Duration,
    max_entries: usize,
    stats: CacheStats,
}

struct CachedEmbedding {
    embedding: Vec<f32>,
    created_at: Instant,
}

#[derive(Default)]
struct CacheStats {
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl EmbeddingCache {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
            max_entries,
            stats: CacheStats::default(),
        }
    }

    fn lookup(&mut self, text: &str, prefix: &str) -> Option<Vec<f32>> {
        let key = self.hash_key(text, prefix);
        if let Some(entry) = self.entries.get(&key) {
            if entry.created_at.elapsed() < self.ttl {
                self.stats.hits += 1;
                return Some(entry.embedding.clone());
            }
            self.entries.remove(&key);
        }
        self.stats.misses += 1;
        None
    }

    fn store(&mut self, text: &str, prefix: &str, embedding: Vec<f32>) {
        if self.entries.len() >= self.max_entries {
            self.evict_oldest();
        }
        let key = self.hash_key(text, prefix);
        self.entries.insert(key, CachedEmbedding {
            embedding,
            created_at: Instant::now(),
        });
    }

    fn hash_key(&self, text: &str, prefix: &str) -> u64 {
        let mut hash = FNV_OFFSET_BASIS;
        for byte in prefix.bytes().chain(text.bytes()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }

    fn evict_oldest(&mut self) {
        if let Some((&oldest_key, _)) = self.entries.iter()
            .min_by_key(|(_, v)| v.created_at)
        {
            self.entries.remove(&oldest_key);
            self.stats.evictions += 1;
        }
    }
}
```

**Integration point:** Add to `AppState` alongside the model. Check cache before calling `model.embed()`:

```rust
async fn handle_search(req: SearchRequest, state: &Arc<Mutex<AppState>>, ...) -> String {
    let mut guard = state.lock().await;

    // Check embedding cache first
    if let Some(cached_emb) = guard.embed_cache.lookup(&req.query, "search_query: ") {
        let _ = activity_tx.send(()).await;
        // Skip model load entirely — use cached embedding for search
        return match guard.db.search(&cached_emb, req.limit, &req.source, &req.metadata_filter) {
            Ok(results) => serde_json::to_string(&SearchResponse { results }).unwrap(),
            Err(e) => json_error(&format!("search failed: {e}")),
        };
    }

    // Cache miss — load model and embed as usual
    if let Err(e) = ensure_model(&mut guard).await {
        return json_error(&format!("model load failed: {e}"));
    }
    // ... embed, cache result, then search
}
```

**Impact:** For the common case of repeated similar skill retrievals, this avoids model loading entirely. A 768-dim f32 embedding is 3KB — caching 1000 entries costs only 3MB RAM, far cheaper than a model forward pass.

**Recommended config:**
- Search query cache: TTL 5 minutes, max 500 entries (queries are repeated frequently)
- Store document cache: TTL 30 seconds, max 100 entries (coalesce rapid re-stores)

---

## 2. Task Registry — Async Embedding Job Management

### The Problem in indexd

All operations are synchronous within the mutex lock. A large batch store (e.g., indexing all `_doc/` markdown files) blocks every other client for the entire duration. There's no way to submit a job and check on it later.

### claw-code's Pattern

`runtime/src/task_registry.rs` implements a clean in-memory task lifecycle:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Created,
    Running,
    Completed,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub prompt: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub messages: Vec<TaskMessage>,
    pub output: String,
    pub team_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskRegistry {
    inner: Arc<Mutex<RegistryInner>>,
}

impl TaskRegistry {
    pub fn create(&self, prompt: &str, description: Option<&str>) -> Task {
        let mut inner = self.inner.lock().expect("registry lock poisoned");
        inner.counter += 1;
        let ts = now_secs();
        let task_id = format!("task_{:08x}_{}", ts, inner.counter);
        // ... create and insert
    }

    pub fn set_status(&self, task_id: &str, status: TaskStatus) -> Result<(), String> { ... }
    pub fn append_output(&self, task_id: &str, output: &str) -> Result<(), String> { ... }
    pub fn stop(&self, task_id: &str) -> Result<Task, String> {
        // Prevents stopping already-terminal tasks
        match task.status {
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Stopped => {
                return Err(format!("task {task_id} is already in terminal state: {}", task.status));
            }
            _ => {}
        }
        // ...
    }
}
```

### How to Apply in indexd

A new `batch_store` action that returns immediately with a task ID:

```rust
#[derive(Deserialize)]
struct BatchStoreRequest {
    texts: Vec<String>,
    #[serde(default)]
    source: String,
    #[serde(default)]
    metadata: Vec<String>,
}

#[derive(Serialize)]
struct BatchStoreResponse {
    task_id: String,
    status: &'static str,
    queued: usize,
}

// New action handler
async fn handle_batch_store(req: BatchStoreRequest, state: &Arc<Mutex<AppState>>) -> String {
    let task_id = {
        let guard = state.lock().await;
        guard.task_registry.create(
            &format!("batch_store: {} texts", req.texts.len()),
            Some(&req.source),
        ).task_id
    };

    // Spawn background embedding task
    let state_clone = state.clone();
    let tid = task_id.clone();
    tokio::spawn(async move {
        let mut guard = state_clone.lock().await;
        guard.task_registry.set_status(&tid, TaskStatus::Running).ok();
        drop(guard);

        // Process in chunks to release mutex between batches
        for chunk in req.texts.chunks(16) {
            let mut guard = state_clone.lock().await;
            if let Err(e) = ensure_model(&mut guard).await {
                guard.task_registry.set_status(&tid, TaskStatus::Failed).ok();
                guard.task_registry.append_output(&tid, &format!("error: {e}")).ok();
                return;
            }
            let model = guard.model.as_ref().unwrap();
            match model.embed(&chunk.to_vec(), "search_document: ") {
                Ok(embeddings) => {
                    // Store embeddings...
                    guard.task_registry.append_output(&tid,
                        &format!("stored {} vectors\n", embeddings.len())).ok();
                }
                Err(e) => {
                    guard.task_registry.set_status(&tid, TaskStatus::Failed).ok();
                    return;
                }
            }
            drop(guard);
            tokio::task::yield_now().await; // Let other requests through
        }

        let mut guard = state_clone.lock().await;
        guard.task_registry.set_status(&tid, TaskStatus::Completed).ok();
    });

    serde_json::to_string(&BatchStoreResponse {
        task_id,
        status: "created",
        queued: req.texts.len(),
    }).unwrap()
}
```

**New actions to add:**
- `batch_store` — submit batch, get task_id
- `task_status` — check progress by task_id
- `task_output` — get accumulated output
- `task_cancel` — cancel a running batch

This pattern directly mirrors how Claude Code's Agent tool works — fire-and-forget with status polling.

---

## 3. Auto-Compaction — Vector Database Maintenance

### The Problem in indexd

The SQLite database grows without bound. Old embeddings accumulate, WAL files grow, and search quality degrades as the vector space becomes polluted with stale content.

### claw-code's Pattern

`runtime/src/compact.rs` implements automatic context compaction:

```rust
pub struct CompactionConfig {
    pub preserve_recent_messages: usize,
    pub max_estimated_tokens: usize,
}

pub fn should_compact(session: &Session, config: CompactionConfig) -> bool {
    let compactable = &session.messages[start..];
    compactable.len() > config.preserve_recent_messages
        && compactable.iter().map(estimate_message_tokens).sum::<usize>()
            >= config.max_estimated_tokens
}

pub fn compact_session(session: &Session, config: CompactionConfig) -> CompactionResult {
    // 1. Find existing compacted summary (if re-compacting)
    // 2. Identify messages to remove (all except recent N)
    // 3. Merge old + new summaries
    // 4. Create new session with summary prefix + preserved recent messages
}
```

The conversation runtime auto-triggers this:

```rust
// In ConversationRuntime::run_turn()
const DEFAULT_AUTO_COMPACTION_INPUT_TOKENS_THRESHOLD: u32 = 100_000;

// After each turn, check if compaction needed
if self.usage_tracker.cumulative_usage().input_tokens >= self.auto_compaction_input_tokens_threshold {
    self.compact();
}
```

### How to Apply in indexd

**Auto-maintenance triggered by thresholds:**

```rust
struct MaintenanceConfig {
    max_vectors: usize,           // Trigger compaction above this count
    max_db_size_bytes: u64,       // Trigger WAL checkpoint above this
    stale_days: u32,              // Consider vectors older than this as stale candidates
    preserve_recent: usize,       // Always keep the N most recent per source
}

impl Default for MaintenanceConfig {
    fn default() -> Self {
        Self {
            max_vectors: 50_000,
            max_db_size_bytes: 100 * 1024 * 1024, // 100MB
            stale_days: 90,
            preserve_recent: 100,
        }
    }
}

struct CompactionResult {
    removed_count: usize,
    freed_bytes: u64,
    wal_checkpointed: bool,
}

fn should_compact(db: &VectorDb, config: &MaintenanceConfig) -> bool {
    let stats = db.stats("").unwrap_or_default();
    stats.total_vectors > config.max_vectors || stats.db_size_bytes > config.max_db_size_bytes
}

fn compact_vectors(db: &VectorDb, config: &MaintenanceConfig) -> Result<CompactionResult> {
    // 1. Delete vectors older than stale_days, keeping preserve_recent per source
    let removed = db.conn.execute(
        "DELETE FROM chunks WHERE id IN (
            SELECT id FROM chunks
            WHERE created < datetime('now', '-' || ?1 || ' days')
            AND id NOT IN (
                SELECT id FROM chunks c2
                WHERE c2.source = chunks.source
                ORDER BY c2.created DESC
                LIMIT ?2
            )
        )",
        rusqlite::params![config.stale_days, config.preserve_recent],
    )?;

    // 2. Clean orphaned vec_chunks
    db.conn.execute(
        "DELETE FROM vec_chunks WHERE rowid NOT IN (SELECT id FROM chunks)",
        [],
    )?;

    // 3. WAL checkpoint
    db.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;

    // 4. Optional vacuum
    if removed > 1000 {
        db.conn.execute_batch("VACUUM;")?;
    }

    Ok(CompactionResult { removed_count: removed, freed_bytes: 0, wal_checkpointed: true })
}
```

**Integration:** Check `should_compact()` after every N store operations (not every one — amortize the cost). The timer wheel from the clawhip doc can schedule periodic checks.

---

## 4. Hook System — Pre/Post Store/Search Triggers

### The Problem in indexd

indexd is a passive data store. There's no way to trigger side effects when content is stored or searched — no notifications, no cascade updates, no audit logging.

### claw-code's Pattern

`plugins/src/hooks.rs` implements a clean hook pipeline:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
}

pub struct HookRunner {
    hooks: PluginHooks,
}

impl HookRunner {
    pub fn run_pre_tool_use(&self, tool_name: &str, tool_input: &str) -> HookRunResult {
        Self::run_commands(HookEvent::PreToolUse, &self.hooks.pre_tool_use,
            tool_name, tool_input, None, false)
    }

    fn run_commands(event: HookEvent, commands: &[String], ...) -> HookRunResult {
        let payload = hook_payload(event, tool_name, tool_input, ...).to_string();
        for command in commands {
            match Self::run_command(command, event, ..., &payload) {
                HookCommandOutcome::Allow { message } => { /* continue */ }
                HookCommandOutcome::Deny { message } => {
                    return HookRunResult { denied: true, ... };
                }
                HookCommandOutcome::Failed { message } => {
                    return HookRunResult { denied: false, failed: true, ... };
                }
            }
        }
        HookRunResult::allow(messages)
    }
}
```

Key design: hooks receive a JSON payload via stdin with environment variables for metadata, and communicate via exit codes (0=allow, 2=deny, other=failed).

### How to Apply in indexd

**AMP-based hooks** — instead of shell commands, fire AMP messages:

```rust
#[derive(Debug, Clone)]
pub enum IndexdHookEvent {
    PreStore { source: String, count: usize },
    PostStore { source: String, ids: Vec<i64>, count: usize },
    PreSearch { query: String, source: String },
    PostSearch { query: String, result_count: usize, top_distance: f64 },
    PostDelete { ids: Vec<i64>, deleted: usize },
}

struct IndexdHooks {
    hub: Option<Arc<cosmix_client::HubClient>>,
}

impl IndexdHooks {
    async fn fire(&self, event: IndexdHookEvent) {
        let Some(hub) = &self.hub else { return };

        let (command, payload) = match &event {
            IndexdHookEvent::PostStore { source, ids, count } => (
                "indexd.hook.post_store",
                serde_json::json!({ "source": source, "ids": ids, "count": count }),
            ),
            IndexdHookEvent::PostSearch { query, result_count, top_distance } => (
                "indexd.hook.post_search",
                serde_json::json!({
                    "query": query,
                    "result_count": result_count,
                    "top_distance": top_distance
                }),
            ),
            // ... other events
        };

        // Fire-and-forget — don't block on hook completion
        let _ = hub.broadcast(command, &payload).await;
    }
}
```

**Use cases:**
- `PostStore` → MCP bridge notifies active sessions about new skills
- `PostSearch` with `result_count == 0` → log "no results" queries for gap analysis
- `PreStore` → validate content before accepting (e.g., reject if source is unknown)
- `PostDelete` → audit trail for compliance

---

## 5. Session Persistence with JSONL + Atomic Writes

### The Problem in indexd

indexd has no operation log. If it crashes mid-batch, there's no way to know what was stored vs. what was lost. The DLQ idea from the clawhip doc addresses retry, but doesn't provide an audit trail.

### claw-code's Pattern

`runtime/src/session.rs` implements JSONL persistence with crash safety:

```rust
// Atomic write: temp file + rename
fn write_atomic(path: &Path, contents: &str) -> Result<(), SessionError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = temporary_path_for(path);
    fs::write(&temp_path, contents)?;
    fs::rename(temp_path, path)?;  // Atomic on POSIX
    Ok(())
}

// File rotation when size exceeds threshold
const ROTATE_AFTER_BYTES: u64 = 256 * 1024;  // 256KB
const MAX_ROTATED_FILES: usize = 3;

fn rotate_session_file_if_needed(path: &Path) -> Result<(), SessionError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() < ROTATE_AFTER_BYTES { return Ok(()); }
    let rotated_path = rotated_log_path(path);
    fs::rename(path, rotated_path)?;
    Ok(())
}

fn cleanup_rotated_logs(path: &Path) -> Result<(), SessionError> {
    // Keep only MAX_ROTATED_FILES most recent, delete the rest
}
```

JSONL format with typed records:

```rust
// Each line is a self-describing record
{"type": "session_meta", "session_id": "...", "created_at_ms": 123456}
{"type": "message", "message": {"role": "user", "blocks": [...]}}
{"type": "compaction", "count": 1, "removed_message_count": 12, "summary": "..."}
```

### How to Apply in indexd

**Operation log (oplog)** — append-only JSONL for every mutation:

```rust
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const ROTATE_AFTER_BYTES: u64 = 1024 * 1024; // 1MB for indexd (more data than sessions)
const MAX_ROTATED_FILES: usize = 5;

struct OpLog {
    path: PathBuf,
}

impl OpLog {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn append(&self, record: &OpLogRecord) -> Result<()> {
        rotate_if_needed(&self.path)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let json = serde_json::to_string(record)?;
        writeln!(file, "{json}")?;
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum OpLogRecord {
    #[serde(rename = "store")]
    Store {
        ids: Vec<i64>,
        source: String,
        count: usize,
        timestamp_ms: u64,
    },
    #[serde(rename = "delete")]
    Delete {
        ids: Vec<i64>,
        deleted: usize,
        timestamp_ms: u64,
    },
    #[serde(rename = "update")]
    Update {
        id: i64,
        re_embedded: bool,
        timestamp_ms: u64,
    },
    #[serde(rename = "compact")]
    Compact {
        removed: usize,
        freed_bytes: u64,
        timestamp_ms: u64,
    },
}
```

**Benefits:**
- **Crash recovery:** Replay oplog to determine what succeeded
- **Audit trail:** Know what was indexed, when, and from which source
- **Debugging:** When search returns unexpected results, check what was stored
- **Replication:** Future mesh peers can replay oplog to sync

---

## 6. Usage Tracking — Model Cost Accounting

### The Problem in indexd

indexd has no visibility into how much compute the embedding model consumes. No way to know which source generates the most embeddings, how many tokens were processed, or estimate costs if using a remote embedding API in the future.

### claw-code's Pattern

`runtime/src/usage.rs` implements lightweight cost tracking:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_creation_input_tokens: u32,
    pub cache_read_input_tokens: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageTracker {
    latest_turn: TokenUsage,
    cumulative: TokenUsage,
    turns: u32,
}

impl UsageTracker {
    pub fn record(&mut self, usage: TokenUsage) {
        self.latest_turn = usage;
        self.cumulative.input_tokens += usage.input_tokens;
        self.cumulative.output_tokens += usage.output_tokens;
        self.turns += 1;
    }
}
```

### How to Apply in indexd

```rust
#[derive(Debug, Clone, Default)]
struct EmbedUsageTracker {
    total_texts_embedded: u64,
    total_tokens_processed: u64,       // Estimated from tokenizer
    total_embeddings_served: u64,       // Includes cache hits
    cache_hits: u64,
    model_loads: u64,
    model_unloads: u64,
    by_source: HashMap<String, SourceUsage>,
    by_action: HashMap<String, u64>,    // "store": 142, "search": 891, ...
    started_at: Instant,
}

#[derive(Debug, Clone, Default)]
struct SourceUsage {
    texts_embedded: u64,
    vectors_stored: u64,
    searches: u64,
}

impl EmbedUsageTracker {
    fn record_embed(&mut self, source: &str, text_count: usize, token_estimate: usize) {
        self.total_texts_embedded += text_count as u64;
        self.total_tokens_processed += token_estimate as u64;
        self.by_source.entry(source.to_string())
            .or_default()
            .texts_embedded += text_count as u64;
    }

    fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
        self.total_embeddings_served += 1;
    }

    fn summary(&self) -> serde_json::Value {
        let uptime = self.started_at.elapsed();
        serde_json::json!({
            "uptime_secs": uptime.as_secs(),
            "total_texts_embedded": self.total_texts_embedded,
            "total_tokens_processed": self.total_tokens_processed,
            "total_embeddings_served": self.total_embeddings_served,
            "cache_hit_rate": if self.total_embeddings_served > 0 {
                self.cache_hits as f64 / self.total_embeddings_served as f64
            } else { 0.0 },
            "model_loads": self.model_loads,
            "by_source": self.by_source,
            "by_action": self.by_action,
        })
    }
}
```

**Integration:** Extend the existing `stats` action to include usage data. Expose via AMP for monitoring dashboards.

---

## 7. SSE Streaming — Stream Search Results

### The Problem in indexd

Search results are returned as a single JSON blob. For large result sets or when the model needs time to generate embeddings, the client waits with no feedback.

### claw-code's Pattern

`api/src/sse.rs` implements a clean incremental SSE parser:

```rust
pub struct SseParser {
    buffer: Vec<u8>,
}

impl SseParser {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>, ApiError> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(frame) = self.next_frame() {
            if let Some(event) = parse_frame(&frame)? {
                events.push(event);
            }
        }
        Ok(events)
    }

    fn next_frame(&mut self) -> Option<String> {
        // Find \n\n or \r\n\r\n separator
        let separator = self.buffer.windows(2)
            .position(|w| w == b"\n\n")
            .map(|p| (p, 2));
        // Drain frame from buffer
    }
}
```

### How to Apply in indexd

**Streaming search results** — useful for AMP mesh where latency matters:

```rust
// New action: "search_stream" — returns results incrementally as they're found
// Each result is a separate JSONL line, allowing the client to process early matches

async fn handle_search_stream(
    req: SearchRequest,
    writer: &mut tokio::io::WriteHalf<UnixStream>,
    state: &Arc<Mutex<AppState>>,
) -> Result<()> {
    // 1. Embed query
    let query_emb = /* ... */;

    // 2. Stream header
    let header = serde_json::json!({"event": "search_start", "query": req.query});
    writer.write_all(format!("{}\n", header).as_bytes()).await?;

    // 3. Execute search and stream each result
    let results = guard.db.search(&query_emb, req.limit, &req.source, &req.metadata_filter)?;
    for (i, result) in results.iter().enumerate() {
        let event = serde_json::json!({
            "event": "result",
            "index": i,
            "id": result.id,
            "content": result.content,
            "source": result.source,
            "metadata": result.metadata,
            "distance": result.distance,
        });
        writer.write_all(format!("{}\n", event).as_bytes()).await?;
    }

    // 4. Stream footer with stats
    let footer = serde_json::json!({
        "event": "search_end",
        "total_results": results.len(),
    });
    writer.write_all(format!("{}\n", footer).as_bytes()).await?;

    Ok(())
}
```

This is especially valuable when indexd serves mesh peers over AMP — the first result can arrive while the database is still scanning for more.

---

## 8. Config Hierarchy — Three-Tier Config Merging

### The Problem in indexd

indexd has minimal config: dtype, model_id, socket_path, idle_timeout_secs, vectors_db path. All from a single `settings.toml` [embed] section. No way for per-project or per-user overrides.

### claw-code's Pattern

`runtime/src/config.rs` implements three-tier config with deep merging:

```rust
enum ConfigSource {
    User,     // ~/.claw/settings.json — personal defaults
    Project,  // <cwd>/.claw/settings.json — project-specific
    Local,    // <cwd>/.claw/settings.local.json — gitignored machine-local
}

impl ConfigLoader {
    pub fn load(&self) -> RuntimeConfig {
        // 1. Load user config
        // 2. Load project config
        // 3. Load local config
        // 4. Deep merge: local > project > user
    }
}

// Deep merge: objects merge recursively, scalars overwrite
fn deep_merge(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, source_value) in source {
        match (target.get_mut(key), source_value) {
            (Some(Value::Object(target_obj)), Value::Object(source_obj)) => {
                deep_merge(target_obj, source_obj);
            }
            _ => {
                target.insert(key.clone(), source_value.clone());
            }
        }
    }
}
```

### How to Apply in indexd

As indexd becomes the knowledge backbone for multiple projects and mesh nodes, per-context configuration becomes important:

```rust
// Three config sources for indexd:
// 1. System: /etc/cosmix/embed.toml (admin defaults)
// 2. User: ~/.config/cosmix/settings.toml [embed] (personal)
// 3. Environment: COSMIX_VECTORS_DB, etc. (highest priority)

// Future: per-source config overrides
// [embed.sources.skill]
// ttl_days = 365          # Skills persist longer
// dedup = true            # Deduplicate skill content
//
// [embed.sources.journal]
// ttl_days = 90           # Journal entries expire
// auto_index = true       # Watch filesystem for changes
// watch_path = "~/.cosmix/src/_journal"
//
// [embed.sources.doc]
// ttl_days = 0            # Docs never expire
// auto_index = true
// watch_path = "~/.cosmix/src/_doc"

#[derive(Default, Deserialize)]
struct SourceConfig {
    ttl_days: Option<u32>,
    dedup: Option<bool>,
    auto_index: Option<bool>,
    watch_path: Option<String>,
}
```

This is lighter than claw-code's full three-tier system — indexd doesn't need per-project configs yet — but the deep merge pattern is worth adopting early for future-proofing.

---

## 9. Permission Enforcement — Multi-Tenant Vector Access

### The Problem in indexd

Any client on the Unix socket or AMP mesh can read, write, or delete any vector. No access control. When indexd joins the mesh, remote nodes could delete local skills.

### claw-code's Pattern

`runtime/src/permission_enforcer.rs` implements per-operation permission checking:

```rust
pub enum EnforcementResult {
    Allowed,
    Denied { tool: String, active_mode: String, required_mode: String, reason: String },
}

impl PermissionEnforcer {
    pub fn check(&self, tool_name: &str, context: &PermissionContext) -> EnforcementResult {
        match self.policy.mode() {
            ReadOnly => { /* only allow read operations */ }
            WorkspaceWrite => { /* allow reads + writes within scope */ }
            DangerFullAccess => EnforcementResult::Allowed,
        }
    }

    pub fn check_file_write(&self, path: &str) -> EnforcementResult {
        // Validates path is within workspace boundary
        if !is_within_workspace(path, &self.workspace_root) { Denied }
    }
}
```

### How to Apply in indexd

**Source-scoped permissions** — restrict what remote peers can do:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessLevel {
    ReadOnly,       // Search and list only
    ReadWrite,      // Search, list, store, update
    Full,           // All operations including delete and compact
}

struct AccessPolicy {
    local_socket: AccessLevel,    // Local Unix socket — Full
    amp_local: AccessLevel,       // Local AMP services — ReadWrite
    amp_mesh: AccessLevel,        // Remote mesh peers — ReadOnly by default
}

impl Default for AccessPolicy {
    fn default() -> Self {
        Self {
            local_socket: AccessLevel::Full,
            amp_local: AccessLevel::ReadWrite,
            amp_mesh: AccessLevel::ReadOnly,
        }
    }
}

fn check_permission(action: &str, access: AccessLevel) -> Result<(), String> {
    let required = match action {
        "embed" | "search" | "list" | "stats" => AccessLevel::ReadOnly,
        "store" | "update" => AccessLevel::ReadWrite,
        "delete" | "compact" | "batch_store" => AccessLevel::Full,
        _ => AccessLevel::Full,
    };

    if (access as u8) < (required as u8) {
        return Err(format!(
            "action '{action}' requires {required:?} access, client has {access:?}"
        ));
    }
    Ok(())
}
```

**Integration:** The AMP command handler tags each request with the source's access level. Socket connections get `Full`. AMP local services get `ReadWrite`. Mesh peers get `ReadOnly` unless explicitly elevated.

---

## 10. Multi-Provider Embedding Support

### The Problem in indexd

indexd is hardcoded to Nomic BERT via Candle. No way to use a different model, use a remote embedding API, or fall back when the local model fails.

### claw-code's Pattern

`api/src/client.rs` abstracts over multiple providers:

```rust
pub enum ProviderClient {
    Anthropic(AnthropicClient),
    Xai(OpenAiCompatClient),
    OpenAi(OpenAiCompatClient),
}

impl ProviderClient {
    pub fn from_model(model: &str) -> Result<Self, ApiError> {
        if model.starts_with("grok") { return Ok(Self::Xai(...)); }
        if model.starts_with("gpt") || model.starts_with("o1") { return Ok(Self::OpenAi(...)); }
        Ok(Self::Anthropic(...))
    }

    pub async fn stream_message(&self, request: MessageRequest) -> Result<MessageStream, ApiError> {
        match self {
            Self::Anthropic(client) => client.stream_message(request).await.map(MessageStream::Anthropic),
            Self::Xai(client) => client.stream_message(request).await.map(MessageStream::OpenAi),
            Self::OpenAi(client) => client.stream_message(request).await.map(MessageStream::OpenAi),
        }
    }
}
```

### How to Apply in indexd

```rust
#[derive(Debug, Clone)]
enum EmbedProvider {
    Local {
        model: NomicBertModel,
        tokenizer: Tokenizer,
        device: Device,
    },
    Ollama {
        base_url: String,
        model_name: String,
    },
    OpenAi {
        api_key: String,
        model: String, // "text-embedding-3-small", etc.
    },
    Amp {
        service_name: String, // Remote indexd on mesh
    },
}

impl EmbedProvider {
    async fn embed(&self, texts: &[String], prefix: &str) -> Result<Vec<Vec<f32>>> {
        match self {
            Self::Local { model, tokenizer, device } => {
                // Existing Candle-based embedding
                local_embed(model, tokenizer, device, texts, prefix)
            }
            Self::Ollama { base_url, model_name } => {
                // POST /api/embed to local Ollama
                ollama_embed(base_url, model_name, texts).await
            }
            Self::OpenAi { api_key, model } => {
                // POST /v1/embeddings to OpenAI API
                openai_embed(api_key, model, texts).await
            }
            Self::Amp { service_name } => {
                // amp_call(service_name, "indexd.embed", {texts, prefix})
                amp_embed(service_name, texts, prefix).await
            }
        }
    }

    fn dimension(&self) -> usize {
        match self {
            Self::Local { .. } => 768,          // Nomic BERT
            Self::Ollama { model_name, .. } => {
                match model_name.as_str() {
                    "nomic-embed-text" => 768,
                    "mxbai-embed-large" => 1024,
                    _ => 768,
                }
            }
            Self::OpenAi { model, .. } => {
                match model.as_str() {
                    "text-embedding-3-small" => 1536,
                    "text-embedding-3-large" => 3072,
                    _ => 1536,
                }
            }
            Self::Amp { .. } => 768, // Assume same model on remote
        }
    }
}
```

**Critical constraint:** All vectors in the same database must use the same dimension. If the provider changes, the schema (and all existing vectors) must be rebuilt. This should be validated at startup and documented clearly.

**Config:**
```toml
[embed]
provider = "local"                    # or "ollama", "openai", "amp"
model_id = "nomic-ai/nomic-embed-text-v1.5"

[embed.ollama]
base_url = "http://localhost:11434"
model_name = "nomic-embed-text"

[embed.openai]
model = "text-embedding-3-small"
# api_key from OPENAI_API_KEY env var

[embed.amp]
service_name = "indexd@hub-node"      # Remote indexd on mesh
```

---

## 11. Conversation-Style Interaction Log

### claw-code's Pattern

`runtime/src/conversation.rs` manages a multi-turn interaction loop where each turn consists of:
1. User input → session
2. API call → streaming events
3. Tool use resolution (permission check → execute → result)
4. Loop until no more tool uses
5. Auto-compact if over token budget

The `ConversationRuntime<C, T>` is generic over `ApiClient` and `ToolExecutor` — the same runtime works with real APIs and mock test doubles.

### How to Apply in indexd

**Query sessions** — track multi-step retrieval interactions for the skills learning loop:

```rust
struct QuerySession {
    session_id: String,
    queries: Vec<QueryTurn>,
    created_at: u64,
}

struct QueryTurn {
    query: String,
    results: Vec<i64>,       // IDs of returned results
    selected: Option<i64>,   // Which result the client actually used
    feedback: Option<bool>,  // Did the skill help? (from skills_refine)
    timestamp: u64,
}
```

When `skills_retrieve` → `skills_refine` happens in sequence, indexd can correlate them into a session and learn which queries lead to useful results. This is implicit relevance feedback — over time, it can be used to:
- Re-rank results based on historical selection patterns
- Identify query patterns that consistently find nothing (content gaps)
- Tune the similarity threshold per source

---

## 12. Retry with Exponential Backoff

### claw-code's Pattern

`api/src/providers/anthropic.rs` implements retry with jitter:

```rust
// Retry logic for API calls
fn should_retry(error: &ApiError) -> bool {
    matches!(error, ApiError::Http(e) if e.status().map_or(false, |s|
        s == 408 || s == 409 || s == 429 || s.as_u16() >= 500
    ))
}

// Exponential backoff: 1s, 2s, 4s, 8s... with jitter
fn backoff_duration(attempt: u32) -> Duration {
    let base = Duration::from_secs(1 << attempt.min(5));
    // Add jitter to prevent thundering herd
    base + Duration::from_millis(rand::random::<u64>() % 1000)
}
```

### How to Apply in indexd

Use for model downloads from HuggingFace Hub (which can fail transiently):

```rust
async fn load_model_with_retry(dtype: DType, model_id: &str, max_attempts: u32) -> Result<EmbedModel> {
    for attempt in 0..max_attempts {
        match EmbedModel::load(dtype, model_id) {
            Ok(model) => return Ok(model),
            Err(e) if attempt + 1 < max_attempts => {
                let backoff = Duration::from_secs(1 << attempt.min(4));
                tracing::warn!("model load attempt {}/{} failed: {e}, retrying in {backoff:?}",
                    attempt + 1, max_attempts);
                tokio::time::sleep(backoff).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}
```

Combine with the circuit breaker from the clawhip doc — retry within a single request, but trip the breaker across requests.

---

## Architectural Synthesis — Enhanced indexd with Both Sources

Combining the best patterns from both clawhip and claw-code-parity:

```
┌───────────────────────────────────────────────────────────┐
│                      cosmix-indexd v2                       │
│                                                            │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │ Permission   │  │    Rate      │  │  Embedding       │  │
│  │ Enforcer     │  │  Limiter     │  │  Cache (FNV-1a)  │  │
│  │ (per-source) │  │ (per-key)    │  │  (TTL + LRU)     │  │
│  └──────┬───────┘  └──────┬───────┘  └───────┬──────────┘  │
│         │                 │                   │             │
│         └────────┬────────┘                   │             │
│                  │                            │             │
│  ┌───────────────▼────────────────────────────▼──────────┐  │
│  │              Request Dispatcher                       │  │
│  │  - Inline: search, embed, stats, list                │  │
│  │  - Batched: batch_store → TaskRegistry               │  │
│  │  - Streamed: search_stream                           │  │
│  └──────────┬────────────────────┬───────────────────────┘  │
│             │                    │                          │
│  ┌──────────▼──────┐  ┌─────────▼──────────┐              │
│  │  Circuit Breaker │  │  Embed Provider    │              │
│  │  (model load)    │  │  (Local/Ollama/    │              │
│  └──────────┬──────┘  │   OpenAI/AMP)      │              │
│             │          └─────────┬──────────┘              │
│  ┌──────────▼──────────────────▼─────────┐                │
│  │            VectorDb (SQLite)           │                │
│  │  + Content hash dedup                 │                │
│  │  + OpLog (JSONL, atomic, rotated)     │                │
│  │  + Auto-compaction (threshold-based)  │                │
│  └──────────────────┬────────────────────┘                │
│                     │                                      │
│  ┌──────────────────▼────────────────────┐                │
│  │        Timer Wheel Scheduler          │                │
│  │  + TTL sweep          + DLQ drain     │                │
│  │  + WAL checkpoint     + File watcher  │                │
│  │  + Stats broadcast    + Compact check │                │
│  └──────────────────┬────────────────────┘                │
│                     │                                      │
│  ┌──────────────────▼────────────────────┐                │
│  │        Hooks (AMP broadcast)          │                │
│  │  + PostStore → notify subscribers     │                │
│  │  + PostSearch → log for gap analysis  │                │
│  └──────────────────┬────────────────────┘                │
│                     │                                      │
│  ┌──────────────────▼────────────────────┐                │
│  │        Usage Tracker + OpLog          │                │
│  │  + Texts embedded / tokens / cache    │                │
│  │  + Per-source breakdown              │                │
│  │  + JSONL audit trail (rotated)       │                │
│  └───────────────────────────────────────┘                │
└───────────────────────────────────────────────────────────┘
```

---

## Implementation Priority (Combined with Clawhip Ideas)

| Priority | Feature | Source | Complexity | Impact |
|----------|---------|--------|-----------|--------|
| **P0** | Embedding cache (FNV-1a) | claw-code | Low (80 LOC) | High — avoids model loads |
| **P0** | Circuit breaker on model | clawhip | Low (70 LOC) | High — prevents load storms |
| **P0** | Content hash dedup | clawhip | Low (30 LOC) | High — prevents duplicates |
| **P1** | Task registry (async batch) | claw-code | Medium (150 LOC) | High — unblocks clients |
| **P1** | Rate limiter (per-source) | clawhip | Low (95 LOC) | Medium — mesh readiness |
| **P1** | Usage tracker | claw-code | Low (60 LOC) | Medium — observability |
| **P1** | Structured error codes | clawhip | Low (20 LOC) | Medium — client reliability |
| **P2** | OpLog (JSONL + atomic) | claw-code | Medium (100 LOC) | Medium — audit + recovery |
| **P2** | Auto-compaction | claw-code | Medium (120 LOC) | Medium — DB maintenance |
| **P2** | Timer wheel scheduler | clawhip | Medium (120 LOC) | Medium — scheduled tasks |
| **P2** | Permission enforcement | claw-code | Medium (80 LOC) | Medium — mesh security |
| **P2** | Hook system (AMP) | claw-code | Medium (100 LOC) | Medium — extensibility |
| **P3** | Multi-provider embedding | claw-code | High (300 LOC) | High — flexibility |
| **P3** | File watcher auto-index | clawhip | High (300 LOC) | High — live knowledge |
| **P3** | Streaming search | claw-code | Medium (100 LOC) | Low — latency improvement |
| **P3** | Query sessions | claw-code | Medium (80 LOC) | Low — implicit feedback |
| **P3** | Config hierarchy | claw-code | High (200 LOC) | Low — future-proofing |
| **P3** | Retry with backoff | claw-code | Low (30 LOC) | Low — resilience |

---

## Appendix: Patterns NOT Worth Porting

| claw-code Pattern | Why Skip for cosmix |
|-------------------|---------------------|
| Bash validation/classification | indexd doesn't execute user commands |
| Sandbox/unshare isolation | Not relevant to an embedding daemon |
| OAuth token management | AMP handles auth via WireGuard trust domain |
| REPL interaction loop | indexd is headless |
| Slash command registry | MCP bridge handles command dispatch |
| Markdown/ANSI rendering | indexd returns structured JSON |
| Session forking | No analogue in vector storage |
| Prompt assembly | indexd doesn't generate LLM prompts |
| TypeScript compat harness | Cosmix is pure Rust |
| Plugin installation/discovery | AMP services replace plugins |
| Remote/CCR proxy config | Cosmix uses AMP mesh, not HTTP proxies |

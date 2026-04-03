# 2026-04-03 — indexd resilience: circuit breaker, embedding cache, content dedup

## Context

Analysed two external Rust codebases (`clawhip/` — notification router, `claw-code-parity/rust/` — Claude Code rewrite) for patterns transferable to cosmix-indexd. Produced two design docs in `_doc/`, reviewed priorities, then implemented the three P0 items directly.

## What Changed

### Circuit breaker on model loading

`ensure_model()` was pathological under failure: a broken model load (OOM, network down, corrupt weights) would block the mutex for up to 30 seconds on every request with no backoff. Now a `CircuitBreaker` (threshold=2, cooldown=60s) gates load attempts. After 2 consecutive failures the circuit opens — requests get instant "model loading suspended" errors instead of queuing behind repeated failures. Half-open probe after cooldown allows recovery.

### Embedding cache (FNV-1a, TTL, LRU)

Every search and store called `model.embed()` even for identical text. The MCP skills loop fires `retrieve -> store -> refine` in rapid sequence, often with similar queries. Now an in-memory cache (FNV-1a hash keyed by text+prefix, 5-minute TTL, 512 entries max, LRU eviction) sits in front of all four embedding paths: embed, search, store, update. Cache hits skip model loading entirely — the most common skill retrieval queries now resolve without touching the model at all.

### Content hash deduplication

Nothing prevented storing identical content multiple times. Now a 128-bit FNV hash on content+source is stored as a BLOB with a unique partial index. `INSERT OR IGNORE` rejects exact duplicates at the SQLite level. Duplicates update metadata (in case it changed) and return the existing row ID. Store response includes `duplicates` count. Schema migration handles existing databases transparently.

### Stats extended

`stats` action now reports `model_circuit` (breaker state), `embed_cache_entries`, `embed_cache_hits`, `embed_cache_misses`. Client types in cosmix-lib-skills updated to match.

## Design Docs Produced

- `src/_doc/2026-04-03-clawhip-ideas-for-indexd.md` — circuit breaker, timer wheel, rate limiter, DLQ, event batching, content dedup, file watcher patterns from clawhip's 306-line core utilities
- `src/_doc/2026-04-03-claw-code-for-indexd.md` — embedding cache, task registry, auto-compaction, hooks, usage tracking, multi-provider, permission enforcement patterns from claw-code-parity

## Priority Review (captured in memory)

- **P0 (done):** circuit breaker, embedding cache, content hash dedup
- **P1 (mesh work):** rate limiter, structured error codes, usage tracker
- **P2 (deferred):** timer wheel, DLQ, file watcher, permissions
- **Skip:** multi-provider embedding, SSE streaming, config hierarchy, query sessions

## Files

- `src/crates/cosmix-indexd/src/main.rs` — +393/-56 lines (circuit breaker, cache, dedup, stats)
- `src/_doc/2026-04-03-clawhip-ideas-for-indexd.md` — new
- `src/_doc/2026-04-03-claw-code-for-indexd.md` — new

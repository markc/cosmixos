# 2026-04-03 Knowledge-Augmented cosmix-claud + MCP Knowledge Tools

## What was built

### cosmix-claud rewrite — knowledge-augmented LLM proxy
- Rewrote from CLI subprocess wrapper to full async daemon with cosmix-lib-llm
- `ask` command now automatically:
  1. Searches indexd for relevant skills, docs, journals (domain-filtered)
  2. Prepends matching context to system prompt
  3. Calls LLM (Haiku by default via cosmix-lib-llm)
  4. Returns response immediately
  5. Async post-response: evaluates for skill extraction, stores novel skills
- `ask_raw` bypasses knowledge injection (for internal use, avoids recursion)
- `analyze` and `generate` commands route through `ask` (get knowledge for free)
- Dropped Port abstraction for direct async socket handlers (Port handlers are sync Fn)

### context_search MCP tool
- Unified search across skills + docs + journals in one call
- Domain-aware: auto-detects from PWD, searches current domain first
- Cross-domain fallback when domain results are sparse
- Deduplicates cross-domain backfill results

### index_workspace MCP tool
- Indexes `_doc/` and `_journal/` directories for any workspace
- Walks up to 3 levels deep to find content directories
- Splits markdown on `## ` headings (50-char min sections)
- Stores one section at a time (avoids indexd mutex contention)
- Idempotent: deletes old entries before re-storing
- Extracts date from YYYY-MM-DD filename convention
- Optional `filter` param for partial re-indexing

### raw_request() on IndexdClient
- Generic JSON request method for operations not covered by typed methods
- Used by context_search and index_workspace for doc/journal operations

### Doc indexing test
- 466 sections from 60 `_doc/*.md` files indexed (source="doc", domain="cosmix")
- Semantic search verified: JMAP, WireGuard, OKLCH queries all return correct docs
- One-section-at-a-time approach avoids mutex deadlocks

### CLAUDE.md knowledge protocol
- Updated global instructions to use `context_search` instead of just `skills_retrieve`
- Added `index_workspace` documentation
- Fail-silent on connection errors

## Architecture decisions

### MCP sampling (Path C) — blocked
- rmcp 1.3.0 has full sampling types (CreateMessageRequest, etc.)
- Claude Code does NOT implement MCP sampling yet (anthropics/claude-code#1785)
- Types ready, waiting on client support

### cosmix-claud as learning proxy (Path D) — chosen
- Most powerful option available today
- Works for any client (not just Claude Code)
- Transparent knowledge injection and skill extraction
- Aligns with future Strix Halo local model routing

### Default LLM backend
- Changed from ollama (too slow) to claude-api (Haiku 4.5)
- Skills evaluation and extraction both use this default

## Gotchas
- Port abstraction handlers are sync `Fn` — can't do async indexd/LLM calls
- Multiple simultaneous indexd store requests cause mutex contention — store one at a time
- MCP binary requires session restart to pick up new tools
- indexd model loads on first embed request (~350ms cached, longer first time)

## Next steps (priority order)
1. Git post-commit hook for auto-indexing changed .md files
2. Skill graduation — high-confidence skills promote to CLAUDE.md
3. Relevance feedback on docs
4. Code indexing — Rust doc comments as source="code"

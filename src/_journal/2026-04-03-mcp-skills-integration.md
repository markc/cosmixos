# 2026-04-03 MCP Skills + Knowledge Base Integration

## What was done

### MCP skills tools (cosmix-mcp)
- Added 5 skills tools to cosmix-mcp: `skills_retrieve`, `skills_store`, `skills_refine`, `skills_list`, `skills_delete`
- Skills tools connect directly to cosmix-indexd via Unix socket (fast local path)
- `skills_store` lets Claude self-evaluate and generate skill fields — no extra LLM call for extraction
- `skills_refine` uses cosmix-lib-llm (Haiku by default) for refinement analysis
- MCP server instructions include the skill learning protocol

### Default LLM backend
- Changed from `ollama` (qwen3:30b, too slow) to `claude-api` (claude-haiku-4-5-20251001)
- Ollama remains configured as fallback, Strix Halo will replace it when available

### indexd AMP registration
- cosmix-indexd now auto-registers as "indexd" on the hub at startup
- AMP commands (indexd.search, indexd.store, etc.) map to the same JSON protocol as the Unix socket
- Falls back to socket-only mode if hub isn't running
- Remote mesh nodes can now access the vector store via `amp_call("indexd", "indexd.search", ...)`

### CLAUDE.md skill learning protocol
- Added to global `~/.claude/CLAUDE.md` — applies to all workspaces
- Instructs Claude to: retrieve before non-trivial tasks, store after success, refine when using retrieved skills
- Fails silently if indexd isn't running — opportunistic, not mandatory

### Doc indexing test
- Indexed all 60 `_doc/*.md` files into indexd (466 sections, source="doc", domain="cosmix")
- Section splitting on `## ` headings with 50-char minimum
- Semantic search confirmed working — queries like "JMAP mail server" and "WireGuard mesh" return correct docs
- Indexing script was ad-hoc Python; needs to become a proper tool

## Architecture decided but not yet built

### `context_search` MCP tool (replaces separate docs_search + skills_retrieve)
- Single unified search across all three source types: skill, doc, journal
- Domain-aware: auto-detects from PWD, searches current domain first, cross-domain fallback
- Returns merged, ranked results with snippets

### `index_workspace` MCP tool
- Scans `_doc/` and `_journal/` in current workspace
- Tags with detected domain, source type, date
- Idempotent re-indexing (deletes old entries for same path first)

### Three knowledge tiers in vector store
| Source | Contains | Signal type |
|--------|----------|-------------|
| skill | Hermes learning loop patterns | Distilled, versioned, confidence-scored |
| doc | Design docs, architecture plans | Static reference |
| journal | Operational logs, session notes | Temporal, experiential |

## Improvements identified (priority order)
1. Build `context_search` + `index_workspace` (immediate next)
2. Git post-commit hook for auto-indexing changed .md files
3. Skill graduation — high-confidence skills promote to CLAUDE.md rules
4. Relevance feedback on docs — track whether retrieved context was useful
5. Code indexing — Rust doc comments as source="code"
6. Temporal decay for journal entries

## Gotchas discovered
- Indexing many sections in one `store` call holds the indexd Mutex for too long — other connections block. Store one section at a time for concurrent access.
- Multiple Python indexing processes queuing on the socket caused deadlock. Need connection timeouts or async indexing.
- MCP server binary must be restarted (new Claude Code session) to pick up new tools.

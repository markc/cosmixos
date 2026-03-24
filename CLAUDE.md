# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Cosmix is a self-hosted sovereign intelligence stack: JMAP mail server + Dioxus cross-platform client + AMP mesh networking + AI inference pipeline. All Rust, no Python/Node/JavaScript except as Dioxus build output.

**Current phase:** Phase 3 (cosmix-jmap core server). Phases 4–7 build on top: CalDAV/CardDAV, AI inference, Dioxus client, mesh SMTP bypass.

## Workspace

Four-crate monorepo, Rust 2024 edition:

| Crate | Type | Purpose |
|-------|------|---------|
| `cosmix-jmap` | binary | JMAP (RFC 8620/8621) + SMTP server, axum + sqlx + PostgreSQL |
| `cosmix-mail` | binary | Dioxus 0.7 desktop/web mail client (early scaffold) |
| `cosmix-mesh` | library | WireGuard mesh networking, WebSocket peer sync |
| `cosmix-port` | library | ARexx-inspired IPC — Unix socket command ports, AMP wire format |

External path dependency: `spamlite` at `~/.gh/spamlite` (Bayesian spam classifier).

## Build Commands

```bash
cargo build                             # entire workspace
cargo build -p cosmix-jmap              # single crate
cargo build -p cosmix-jmap --release    # release build
cargo check                             # type-check without codegen
```

Dioxus client (requires `dx` CLI — `cargo binstall dioxus-cli`):
```bash
cd crates/cosmix-mail && dx serve                  # desktop hot-reload
cd crates/cosmix-mail && dx serve --platform web    # browser WASM
cd crates/cosmix-mail && dx serve --hotpatch        # Rust hot-patch
```

No test suite yet. Manual validation via curl (JMAP) and nc/swaks (SMTP).

## cosmix-jmap CLI

```bash
cosmix-jmap migrate                     # apply SQL migrations
cosmix-jmap account add <email> <pwd>   # create account (auto-creates default mailboxes + PIM)
cosmix-jmap account list
cosmix-jmap account delete <email>
cosmix-jmap queue list                  # SMTP outbound queue
cosmix-jmap queue flush                 # retry queued messages
cosmix-jmap serve                       # start JMAP HTTP + SMTP listeners
```

## Configuration

Config loaded from `~/.config/cosmix/jmap.toml` or `/etc/cosmix/jmap.toml`. Key fields:

- `database_url` — PostgreSQL connection (requires pgvector extension for future AI phase)
- `blob_dir` — on-disk blob storage path
- `smtp.listen_inbound` — SMTP port (default 2525, production 25)
- `tls.cert` / `tls.key` — rustls TLS
- `dkim.selector` / `dkim.private_key` — outbound DKIM signing
- `spam.enabled` / `spam.db_dir` — per-account spamlite SQLite databases

## Database Architecture

PostgreSQL with sqlx (async, compile-time checked queries). Four migrations in `crates/cosmix-jmap/migrations/`:

**State tracking:** All mutations write to `changelog(account_id, object_type, object_id, change_type)`. JMAP state = max changelog ID per (account_id, object_type). This powers `/changes` and `/query` efficiently.

**UUID primary keys** on mailboxes, threads, blobs, calendars, events, contacts. Account IDs are SERIAL INT.

**JSONB for flexibility:** email addresses, keywords, calendar events (JSCalendar RFC 8984), contacts (JSContact RFC 9553) stored as JSONB.

**Blob storage:** BLAKE3 hashed, size tracked, stored on disk at `{blob_dir}/{id}`.

**Account creation** triggers `create_default_mailboxes()` (Inbox/Drafts/Sent/Trash/Junk/Archive) and `create_default_pim()` (Personal calendar + Contacts addressbook).

## JMAP Server Architecture

**Endpoints:**
- `GET /.well-known/jmap` — session resource (capabilities, account URLs)
- `POST /jmap` — method dispatch over `methodCalls[]` array
- `GET/POST /jmap/blob/{blobId}` — blob download/upload

**Method dispatch** (`src/jmap/mod.rs`): decodes `JmapRequest`, iterates `methodCalls`, routes to `mailbox.rs`, `email.rs`, `calendar.rs`, `contact.rs`, `submission.rs`, aggregates into `JmapResponse`.

**SMTP inbound** (`src/smtp/`): accepts MAIL FROM/RCPT TO/DATA, parses RFC 5322 via mail-parser, stores blob + Email row, classifies spam via spamlite, records changelog entry.

**SMTP outbound:** EmailSubmission/set → mail-builder constructs MIME → mail-send delivers. Failed deliveries go to `smtp_queue` with exponential backoff (max 10 attempts).

## AMP Wire Format (cosmix-port)

All cosmix IPC uses AMP (AppMesh Protocol) — markdown frontmatter with BTreeMap headers + optional body:

```
---
command: mailbox.list
rc: 0
---
[{"id": "...", "name": "Inbox"}]
```

RC codes: 0=success, 5=warning, 10=error, 20=failure. Used across Unix sockets (local), WebSockets (mesh), and log files.

## Key Decisions

- **axum** for all HTTP — not actix, not warp
- **sqlx** for database — not sea-orm, not diesel
- **tokio** async runtime throughout
- **Dioxus 0.7** for client — not libcosmic (old stack was libcosmic, pivoted to Dioxus)
- **AI lives in the server** — any JMAP client gets AI via email; cosmix-mail just gets richer UI
- **No Docker** — Incus or Proxmox containers only
- **mimalloc** allocator in all binaries
- **`paru`** for AUR packages on CachyOS/Arch

## Gotchas

- Linux WebKit black screen: cosmix-mail sets `WEBKIT_DISABLE_COMPOSITING_MODE=1` before Dioxus launch
- `spamlite` is a sibling path dep (`../spamlite`), not on crates.io
- Spam databases are per-account SQLite files, not shared — prevents cross-user model contamination
- Thread formation (Message-ID + In-Reply-To matching) is not yet implemented
- The `_doc/` directory contains 30 design documents from the cosmix-cosmic era — check these before guessing at architectural intent

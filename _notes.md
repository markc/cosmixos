# Cosmix — Current State

Last updated: 2026-03-25

## Fixes Applied

- **2026-03-25:** Installed `webkit2gtk-4.1` — cosmix-mail was crashing on launch due to missing `libwebkit2gtk-4.1.so.0`. Now launches successfully.

## What's Complete

- **Phase 1-2:** COSMIC desktop + Dioxus 0.7 toolchain validated
- **Phase 3 J1:** JMAP Core + Email — 21 methods, full Mailbox/Email CRUD + changes
- **Phase 3 J2:** SMTP inbound (SPF/DKIM, spamlite spam, thread assignment) + outbound (MX lookup, DKIM signing, retry queue, bounces)
- **Phase 3 J3:** Calendars (JSCalendar RFC 8984) + Contacts (JSContact RFC 9553) — all CRUD + changes
- **Phase 4:** CalDAV/CardDAV — pivoted to native JMAP methods, no XML/WebDAV needed
- **cosmix-port:** AMP wire format library complete (441 LOC), not yet wired into server
- **cosmix-mail UI (2026-03-25):**
  - Modular component architecture: main.rs + components/{icons,mailbox_list,email_list,email_view,compose}.rs
  - Compose window: New message, Reply, Forward with MIME building via mail-builder
  - Send flow: build MIME → upload blob → Email/set create → EmailSubmission/set
  - Email actions toolbar: Reply, Forward, Archive, Delete, Mark Read/Unread
  - Unread badges on mailboxes (server-side count query)
  - Compose button in sidebar
- **Server Email/set create (2026-03-25):** Added create branch — upload blob + create email record from MIME

## Next Steps (priority order)

1. **Test compose end-to-end** — need running cosmix-jmap server + PostgreSQL to verify send flow
2. **Phase J7 hardening** — rate limiting, health check endpoint, graceful shutdown
3. **Wire cosmix-port into cosmix-jmap (J6)** — CLI scripting and Lua integration
4. **Write tests** — JMAP compliance tests at minimum
5. **Calendar/Contacts UI in cosmix-mail** — views using existing JMAP Calendar/Contact methods
6. **Push notifications** — EventSource/WebSocket for real-time updates
7. **Phase 5 AI inference** — pgvector + Ollama embedding pipeline
8. **Sieve filtering (J4)** — designed, low priority
9. **JMAP Files (J5)** — designed, low priority
10. **SMTP mesh bypass (Phase 7)** — designed, awaits mesh peer discovery

## Not Started

| Feature | Phase | Est. LOC |
|---------|-------|----------|
| Sieve filtering | J4 | ~800 |
| JMAP Files | J5 | ~600 |
| cosmix-port server wiring | J6 | ~500 |
| Hardening | J7 | ~2,000 |
| AI inference (Ollama + pgvector) | Phase 5 | ~1,500 |
| SMTP mesh bypass | Phase 7 | ~800 |
| Test suite | — | TBD |
| Calendar/Contacts UI | Phase 6 | ~1,500 |
| Push notifications (EventSource) | Phase 6 | ~500 |

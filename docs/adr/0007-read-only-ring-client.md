# ADR 0007: Bounded read-only Ring client

- Status: accepted
- Date: 2026-08-14

## Context

Phase 1 needs evidence from the owned Ring Intercom without importing the
supported integration's session, repeating password login or granting the
research bridge access-control authority. Ring rotates refresh tokens, so a
successful refresh can invalidate the only durable credential.

## Decision

The Rust research client uses only a dedicated refresh token and stable
hardware UUID. Vendor URLs are compiled constants, TLS-only in production,
redirects are disabled, requests have connection and overall deadlines, and
response bodies are bounded. The client registers one Android-compatible
session for at most 12 hours and performs only device inventory reads.

Before refresh it proves the session store writable by atomically replacing
the current document. A returned refresh token is replaced through a mode-0600
same-directory write, file sync, atomic rename and directory sync before
discovery continues. A persistence failure retains the rotated token in memory
and prevents further vendor work until persistence succeeds.

One HTTP 401 from registration or discovery permits exactly one re-authentication.
OAuth rejection, HTTP 429 and every other failure stop immediately. There is no
password fallback, unbounded retry, unlock RPC or media call in this client.

## Consequences

- credential loss after ordinary refresh is strongly mitigated;
- a writable private runtime directory is required;
- vendor throttling cannot create an automatic retry loop;
- an explicit operator-provided session remains necessary before live evidence;
- the production service stays offline until this client is deliberately wired.

# ADR 0002: Provider repository separation

- Status: accepted
- Date: 2026-08-14

## Context

EZVIZ is a standalone Rust VTM video bridge on gpu-01. Blink is a Python Home
Assistant component that deliberately reuses Core's loaded account session.
Ring Intercom requires unresolved cloud audio signalling. Their release units,
runtimes, credentials and failure modes do not align.

## Decision

Ring, Blink and EZVIZ remain separate repositories and deployment artifacts.
They share a versioned capability vocabulary and consumer expectations rather
than a monorepo or common runtime library.

## Consequences

- provider failures and releases remain isolated;
- Ansible consumes pinned artifacts instead of owning application source;
- contract drift requires cross-repository tests;
- a shared contract package may be introduced only after three implementations
  demonstrate stable common semantics.

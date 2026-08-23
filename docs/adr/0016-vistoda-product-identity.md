# ADR 0016: Vistoda product identity

- Status: Accepted
- Date: 2026-08-23

## Decision

This connector is published as Vistoda Ring in the canonical `vistoda-ring`
repository. The Rust package, binary, provider protocol client string and
deployed container service keep their existing `ring-intercom-bridge`
compatibility names.

Repository identity may change without forcing consumers to migrate runtime
contracts. Any later binary or service rename requires an independently tested
deployment migration with compatibility aliases and rollback evidence.

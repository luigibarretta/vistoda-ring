# ADR 0001: Rust product boundary

- Status: accepted
- Date: 2026-08-14

## Context

Ring Intercom Audio exposes no supported media primitive in Home Assistant,
`python-ring-doorbell`, ring-mqtt or `ring-client-api`. A future bridge must
handle hostile network input, credentials, real-time packets and strict call
lifetime bounds.

## Decision

The production bridge and protocol research tooling are Rust. The repository
forbids Python, JavaScript and TypeScript source files. Async I/O uses Tokio;
unsafe Rust is forbidden. Vendor-specific signalling remains behind a provider
trait once evidence is sufficient to define it.

## Consequences

- one implementation language from research fixtures through production;
- explicit parsing and bounded ownership without a disposable prototype;
- no reuse of JavaScript WebRTC code, so initial protocol work is slower;
- mature crates may be used only after dependency and MSRV review.
